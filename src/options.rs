//! The viewer's options (ROADMAP phase 6): F10 opens a panel for the
//! ray-tracing and DLSS settings and the light levels, which the viewer saves
//! (`halo_rt\settings.txt` in the user's configuration folder) and applies
//! again at the next start. While the panel is open the mouse is the
//! viewer's, not the game's.

use crate::fog::LitFog;
use crate::LightingSettings;
use bevy::prelude::*;
use bevy_solarik::prelude::SolarikLighting;
use std::path::PathBuf;

#[cfg(feature = "dlss")]
use bevy::anti_alias::dlss::{Dlss, DlssPerfQualityMode, DlssRayReconstructionFeature, DlssRayReconstructionSupported};

pub struct OptionsPlugin;

impl Plugin for OptionsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<OptionsPanel>()
            .init_resource::<PressedButtons>()
            .add_systems(Startup, spawn_panel)
            .add_systems(Update, (toggle_panel, test_options, press_buttons, log_test_options, label_rows, apply_saved, show_stats).chain())
            .add_systems(Last, save_settings);
    }
}

/// Whether the panel is open, and whether the stats text shows.
#[derive(Resource)]
pub struct OptionsPanel {
    pub open: bool,
    pub show_stats: bool,
    /// Whether the stats row was pressed this run (with [`stats_override`],
    /// only then is its value saved).
    stats_pressed: bool,
    /// Whether a lit fog row was pressed this run (with
    /// `fog::lit_fog_override`, only then is its value saved).
    lit_fog_pressed: bool,
}

impl Default for OptionsPanel {
    fn default() -> Self {
        OptionsPanel { open: false, show_stats: stats_override().unwrap_or(true), stats_pressed: false, lit_fog_pressed: false }
    }
}

/// What the viewer saves between runs.
#[derive(Clone, PartialEq)]
pub struct SavedSettings {
    pub lighting: LightingSettings,
    pub restir: bool,
    pub dlss_rr: bool,
    /// `DlssPerfQualityMode` by name.
    pub dlss_quality: String,
    pub show_stats: bool,
    /// Lit fog ([`LitFog`]) on, and its strength (its sun and only flags
    /// aren't saved).
    pub lit_fog: bool,
    pub lit_fog_strength: f32,
}

/// Lit fog's strength before any is chosen.
pub const LIT_FOG_STRENGTH: f32 = 3.0;

/// The strengths the panel steps lit fog through.
const LIT_FOG_STRENGTHS: [f32; 8] = [0.5, 1.0, 2.0, 3.0, 5.0, 8.0, 12.0, 20.0];

/// Lit fog's strength a step down (`direction` < 0) or up: the next of
/// [`LIT_FOG_STRENGTHS`] below or above it (from one not in the list too),
/// and none past the ends, the same.
fn step_lit_fog_strength(strength: f32, direction: i8) -> f32 {
    let next = if direction < 0 {
        LIT_FOG_STRENGTHS.into_iter().rev().find(|s| *s < strength)
    } else {
        LIT_FOG_STRENGTHS.into_iter().find(|s| *s > strength)
    };
    next.unwrap_or(strength)
}

/// A strength as the panel shows it: whole, or to a tenth.
fn strength_text(strength: f32) -> String {
    if strength.fract() == 0.0 { format!("{strength:.0}") } else { format!("{strength:.1}") }
}

pub(crate) fn settings_path() -> Option<PathBuf> {
    let base = std::env::var_os("APPDATA").or_else(|| std::env::var_os("XDG_CONFIG_HOME"))?;
    Some(PathBuf::from(base).join("halo_rt").join("settings.txt"))
}

impl SavedSettings {
    /// The viewer's settings before any are changed.
    fn defaults() -> Self {
        SavedSettings {
            lighting: LightingSettings::default(),
            restir: true,
            dlss_rr: true,
            dlss_quality: "Auto".into(),
            show_stats: true,
            lit_fog: false,
            lit_fog_strength: LIT_FOG_STRENGTH,
        }
    }

    /// The saved settings, or `None` before any were saved.
    pub fn load() -> Option<Self> {
        Some(Self::parse(&std::fs::read_to_string(settings_path()?).ok()?))
    }

    /// The settings a file's text holds: a key it hasn't, or whose value
    /// doesn't parse, the default.
    fn parse(text: &str) -> Self {
        let mut settings = Self::defaults();
        for line in text.lines() {
            let Some((key, value)) = line.split_once('=') else { continue };
            let (key, value) = (key.trim(), value.trim());
            let number = value.parse::<f32>().ok().filter(|v| v.is_finite());
            let flag = value == "true";
            match key {
                "emissive" => settings.lighting.emissive_scale = number.unwrap_or(settings.lighting.emissive_scale),
                "sun" => settings.lighting.sun_scale = number.unwrap_or(settings.lighting.sun_scale),
                "sky" => settings.lighting.sky_scale = number.unwrap_or(settings.lighting.sky_scale),
                "exposure" => settings.lighting.exposure_ev100 = number.unwrap_or(settings.lighting.exposure_ev100),
                "restir" => settings.restir = flag,
                "dlss_rr" => settings.dlss_rr = flag,
                "dlss_quality" => settings.dlss_quality = value.to_string(),
                "stats" => settings.show_stats = flag,
                "lit_fog" => settings.lit_fog = flag,
                "lit_fog_strength" => settings.lit_fog_strength = number.filter(|s| *s > 0.0).unwrap_or(settings.lit_fog_strength),
                _ => {}
            }
        }
        settings
    }

    /// The file's text.
    fn text(&self) -> String {
        let l = &self.lighting;
        format!(
            "# halo_rt's options (F10 in the viewer)\nemissive = {}\nsun = {}\nsky = {}\nexposure = {}\nrestir = {}\ndlss_rr = {}\ndlss_quality = {}\nstats = {}\nlit_fog = {}\nlit_fog_strength = {}\n",
            l.emissive_scale, l.sun_scale, l.sky_scale, l.exposure_ev100, self.restir, self.dlss_rr, self.dlss_quality, self.show_stats, self.lit_fog, self.lit_fog_strength
        )
    }

    fn save(&self) -> std::io::Result<()> {
        let path = settings_path().ok_or_else(|| std::io::Error::other("no configuration folder"))?;
        if let Some(folder) = path.parent() {
            std::fs::create_dir_all(folder)?;
        }
        std::fs::write(path, self.text())
    }
}

#[cfg(feature = "dlss")]
const QUALITIES: [DlssPerfQualityMode; 6] = [
    DlssPerfQualityMode::Auto,
    DlssPerfQualityMode::Dlaa,
    DlssPerfQualityMode::Quality,
    DlssPerfQualityMode::Balanced,
    DlssPerfQualityMode::Performance,
    DlssPerfQualityMode::UltraPerformance,
];

#[cfg(feature = "dlss")]
fn quality_by_name(name: &str) -> Option<DlssPerfQualityMode> {
    QUALITIES.into_iter().find(|q| format!("{q:?}") == name)
}

/// A row of the panel.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
enum Row {
    DlssRr,
    DlssQuality,
    Sun,
    Sky,
    Emissive,
    Exposure,
    LitFog,
    LitFogStrength,
    Stats,
    Reset,
}

/// A button: its row, and which way it moves the setting (-1, +1, or 0 to
/// toggle or cycle).
#[derive(Component, Clone, Copy, Debug, PartialEq)]
struct OptionButton(Row, i8);

/// The text of a row's value.
#[derive(Component)]
struct RowLabel(Row);

#[derive(Component)]
struct PanelRoot;

const BUTTON: Color = Color::srgba(0.2, 0.3, 0.45, 0.9);
const BUTTON_HOVER: Color = Color::srgba(0.3, 0.45, 0.65, 0.95);

fn spawn_panel(mut commands: Commands) {
    let rows = [
        Row::DlssRr,
        Row::DlssQuality,
        Row::Sun,
        Row::Sky,
        Row::Emissive,
        Row::Exposure,
        Row::LitFog,
        Row::LitFogStrength,
        Row::Stats,
        Row::Reset,
    ];
    let panel = commands
        .spawn((
            PanelRoot,
            Name::new("options"),
            Node {
                position_type: PositionType::Absolute,
                left: percent(50),
                top: percent(50),
                margin: UiRect { left: px(-210.0), top: px(-170.0), ..default() },
                width: px(420.0),
                flex_direction: FlexDirection::Column,
                row_gap: px(6.0),
                padding: px(14.0).all(),
                border_radius: BorderRadius::all(px(8.0)),
                display: Display::None,
                ..default()
            },
            BackgroundColor(Color::srgba(0.05, 0.07, 0.10, 0.92)),
            GlobalZIndex(10),
        ))
        .id();
    commands.spawn((
        Text::new("Options  (F10 closes)"),
        TextFont { font_size: FontSize::Px(18.0), ..default() },
        ChildOf(panel),
    ));
    for row in rows {
        let line = commands
            .spawn((
                Node { flex_direction: FlexDirection::Row, align_items: AlignItems::Center, column_gap: px(6.0), ..default() },
                ChildOf(panel),
            ))
            .id();
        commands.spawn((
            RowLabel(row),
            Text::default(),
            TextFont { font_size: FontSize::Px(14.0), ..default() },
            Node { flex_grow: 1.0, ..default() },
            ChildOf(line),
        ));
        let buttons: &[(i8, &str)] = match row {
            Row::Sun | Row::Sky | Row::Emissive | Row::Exposure | Row::LitFogStrength => &[(-1, "-"), (1, "+")],
            Row::Reset => &[(0, "Reset")],
            _ => &[(0, "Change")],
        };
        for &(direction, caption) in buttons {
            commands
                .spawn((
                OptionButton(row, direction),
                Node {
                    padding: UiRect::axes(px(10.0), px(3.0)),
                    border_radius: BorderRadius::all(px(4.0)),
                    ..default()
                },
                BackgroundColor(BUTTON),
                ChildOf(line),
                children![(Text::new(caption), TextFont { font_size: FontSize::Px(14.0), ..default() }, Pickable::IGNORE)],
            ))
                .observe(|click: On<bevy::picking::events::PointerClick>, buttons: Query<&OptionButton>, mut pressed: ResMut<PressedButtons>| {
                    if let Ok(button) = buttons.get(click.entity) {
                        pressed.0.push(*button);
                    }
                })
                .observe(|over: On<bevy::picking::events::PointerOver>, mut colors: Query<&mut BackgroundColor>| {
                    if let Ok(mut color) = colors.get_mut(over.entity) {
                        color.0 = BUTTON_HOVER;
                    }
                })
                .observe(|out: On<bevy::picking::events::PointerOut>, mut colors: Query<&mut BackgroundColor>| {
                    if let Ok(mut color) = colors.get_mut(out.entity) {
                        color.0 = BUTTON;
                    }
                });
        }
    }
}

/// F10: open or close the panel.
fn toggle_panel(
    keys: Res<ButtonInput<KeyCode>>,
    mut panel: ResMut<OptionsPanel>,
    mut root: Single<&mut Node, With<PanelRoot>>,
    window: Single<(&mut Window, &mut bevy::window::CursorOptions), With<bevy::window::PrimaryWindow>>,
) {
    if !keys.just_pressed(KeyCode::F10) {
        return;
    }
    let (_, mut cursor) = window.into_inner();
    flip_panel(&mut panel, &mut root, &mut cursor);
}

/// The panel opened or closed (F10's, and `HALO_RT_TEST_OPTIONS`' `panel`).
fn flip_panel(panel: &mut OptionsPanel, root: &mut Node, cursor: &mut bevy::window::CursorOptions) {
    panel.open = !panel.open;
    root.display = if panel.open { Display::Flex } else { Display::None };
    if panel.open {
        // (the mouse is the viewer's while the panel is open)
        cursor.grab_mode = bevy::window::CursorGrabMode::None;
        cursor.visible = true;
    }
}

/// Buttons clicked since the last update (and the names of those pressed
/// by [`test_options`], for its log after them).
#[derive(Resource, Default)]
struct PressedButtons(Vec<OptionButton>, Vec<&'static str>);

/// The buttons `HALO_RT_TEST_OPTIONS` names, by name.
const TEST_BUTTONS: [(&str, OptionButton); 4] = [
    ("lit_fog", OptionButton(Row::LitFog, 0)),
    ("lit_fog_strength+", OptionButton(Row::LitFogStrength, 1)),
    ("lit_fog_strength-", OptionButton(Row::LitFogStrength, -1)),
    ("stats", OptionButton(Row::Stats, 0)),
];

/// What a `HALO_RT_TEST_OPTIONS` entry presses: a button, or F10.
#[derive(Clone, Copy, Debug, PartialEq)]
enum TestPress {
    Button(OptionButton),
    Panel,
}

/// One `HALO_RT_TEST_OPTIONS` entry, `<frame>:<name>`.
fn parse_test_press(entry: &str) -> Result<(u32, &'static str, TestPress), String> {
    let (frame, name) = entry.split_once(':').ok_or("no <frame>:")?;
    let frame = frame.trim().parse::<u32>().map_err(|e| format!("frame {frame:?}: {e}"))?;
    let name = name.trim();
    if name == "panel" {
        return Ok((frame, "panel", TestPress::Panel));
    }
    let &(name, button) = TEST_BUTTONS.iter().find(|(n, _)| *n == name).ok_or_else(|| format!("no button {name:?}"))?;
    Ok((frame, name, TestPress::Button(button)))
}

/// `HALO_RT_TEST_OPTIONS="<frame>:<name>[;<frame>:<name>...]"`: the panel's
/// buttons pressed by script at frames of Bevy's `FrameCount` (names
/// `lit_fog`, `lit_fog_strength+`, `lit_fog_strength-`, `stats`), and
/// `panel` the panel opened or closed as F10 does; an entry that doesn't
/// parse is left out with a warning. Read once.
fn test_presses() -> &'static [(u32, &'static str, TestPress)] {
    static PRESSES: std::sync::OnceLock<Vec<(u32, &'static str, TestPress)>> = std::sync::OnceLock::new();
    PRESSES.get_or_init(|| {
        std::env::var("HALO_RT_TEST_OPTIONS")
            .unwrap_or_default()
            .split(';')
            .filter(|entry| !entry.trim().is_empty())
            .filter_map(|entry| match parse_test_press(entry) {
                Ok(press) => Some(press),
                Err(e) => {
                    warn!("HALO_RT_TEST_OPTIONS: {entry:?} left out: {e}");
                    None
                }
            })
            .collect()
    })
}

/// `HALO_RT_TEST_OPTIONS`' buttons at their frames ([`test_presses`]), as
/// clicks are, and the panel as F10 opens it.
fn test_options(
    frames: Res<bevy::diagnostic::FrameCount>,
    mut pressed: ResMut<PressedButtons>,
    mut panel: ResMut<OptionsPanel>,
    mut root: Single<&mut Node, With<PanelRoot>>,
    mut cursor: Single<&mut bevy::window::CursorOptions, With<bevy::window::PrimaryWindow>>,
) {
    for &(frame, name, press) in test_presses() {
        if frame == frames.0 {
            match press {
                TestPress::Button(button) => pressed.0.push(button),
                TestPress::Panel => flip_panel(&mut panel, &mut root, &mut cursor),
            }
            pressed.1.push(name);
        }
    }
}

/// What [`test_options`] pressed, and the state after.
fn log_test_options(
    frames: Res<bevy::diagnostic::FrameCount>,
    mut pressed: ResMut<PressedButtons>,
    panel: Res<OptionsPanel>,
    lit_fog: Option<Res<LitFog>>,
) {
    if pressed.1.is_empty() {
        return;
    }
    let names = std::mem::take(&mut pressed.1).join(", ");
    let lit = lit_fog.map_or("no lit fog (needs the game's fog)".to_string(), |lit| {
        format!("lit fog {}, strength {}", if lit.on { "on" } else { "off" }, strength_text(lit.strength))
    });
    info!(
        "test options at frame {}: pressed {names}: panel {}, {lit}, stats text {}",
        frames.0,
        if panel.open { "open" } else { "closed" },
        if panel.show_stats { "on" } else { "off" }
    );
}

fn press_buttons(
    mut pressed: ResMut<PressedButtons>,
    mut panel: ResMut<OptionsPanel>,
    mut lighting: ResMut<LightingSettings>,
    mut lit_fog: Option<ResMut<LitFog>>,
    #[cfg(feature = "dlss")] camera: Query<(Entity, Option<&mut Dlss<DlssRayReconstructionFeature>>), With<SolarikLighting>>,
    #[cfg(feature = "dlss")] dlss_supported: Option<Res<DlssRayReconstructionSupported>>,
    mut commands: Commands,
) {
    #[cfg(feature = "dlss")]
    let mut camera = camera;
    for OptionButton(row, direction) in std::mem::take(&mut pressed.0) {
        let step = if direction < 0 { 1.0 / 1.25 } else { 1.25 };
        match row {
            Row::Sun => lighting.sun_scale *= step,
            Row::Sky => lighting.sky_scale *= step,
            Row::Emissive => lighting.emissive_scale *= step,
            Row::Exposure => lighting.exposure_ev100 += 0.5 * direction as f32,
            Row::Reset => *lighting = LightingSettings::default(),
            Row::Stats => {
                panel.show_stats = !panel.show_stats;
                panel.stats_pressed = true;
            }
            // (none without the game's fog)
            Row::LitFog => {
                if let Some(lit) = lit_fog.as_deref_mut() {
                    lit.on = !lit.on;
                    panel.lit_fog_pressed = true;
                }
            }
            Row::LitFogStrength => {
                if let Some(lit) = lit_fog.as_deref_mut() {
                    lit.strength = step_lit_fog_strength(lit.strength, direction);
                    panel.lit_fog_pressed = true;
                }
            }
            #[cfg(feature = "dlss")]
            Row::DlssRr => {
                if dlss_supported.is_some() {
                    for (entity, dlss) in &camera {
                        set_dlss_rr(&mut commands, entity, dlss.is_none(), None);
                    }
                }
            }
            #[cfg(feature = "dlss")]
            Row::DlssQuality => {
                for (_, dlss) in &mut camera {
                    if let Some(mut dlss) = dlss {
                        let index = QUALITIES.iter().position(|q| *q == dlss.perf_quality_mode).unwrap_or(0);
                        dlss.perf_quality_mode = QUALITIES[(index + 1) % QUALITIES.len()];
                    }
                }
            }
            #[cfg(not(feature = "dlss"))]
            Row::DlssRr | Row::DlssQuality => {}
        }
    }
    let _ = &mut commands;
}

#[cfg(feature = "dlss")]
/// DLSS Ray Reconstruction on or off. (Turning it on again is best done
/// with a restart: a camera that already renders may not take it.)
fn set_dlss_rr(commands: &mut Commands, camera: Entity, on: bool, quality: Option<DlssPerfQualityMode>) {
    use bevy::render::camera::{MipBias, TemporalJitter};
    if on {
        let mut dlss = Dlss::<DlssRayReconstructionFeature>::default();
        if let Some(quality) = quality {
            dlss.perf_quality_mode = quality;
        }
        commands.entity(camera).insert(dlss);
    } else {
        commands.entity(camera).remove::<(Dlss<DlssRayReconstructionFeature>, TemporalJitter, MipBias)>();
    }
}

fn label_rows(
    panel: Res<OptionsPanel>,
    lighting: Res<LightingSettings>,
    lit_fog: Option<Res<LitFog>>,
    #[cfg(feature = "dlss")] dlss: Query<Option<&Dlss<DlssRayReconstructionFeature>>, With<SolarikLighting>>,
    #[cfg(feature = "dlss")] dlss_supported: Option<Res<DlssRayReconstructionSupported>>,
    mut labels: Query<(&RowLabel, &mut Text)>,
) {
    if !panel.open {
        return;
    }
    let on_off = |on: bool| if on { "on" } else { "off" };
    #[cfg(feature = "dlss")]
    let (rr, quality) = match (dlss_supported.is_some(), dlss.iter().next().flatten()) {
        (false, _) => ("not supported on this GPU".to_string(), "-".to_string()),
        (true, Some(dlss)) => ("on".to_string(), format!("{:?}", dlss.perf_quality_mode)),
        (true, None) => ("off".to_string(), "-".to_string()),
    };
    #[cfg(not(feature = "dlss"))]
    let (rr, quality) = ("not built (--features dlss)".to_string(), "-".to_string());
    for (RowLabel(row), mut text) in &mut labels {
        text.0 = match row {
            Row::DlssRr => format!("DLSS Ray Reconstruction: {rr}"),
            Row::DlssQuality => format!("DLSS quality: {quality}"),
            Row::Sun => format!("Sun: {:.0}", lighting.sun_scale),
            Row::Sky => format!("Sky: {:.0}", lighting.sky_scale),
            Row::Emissive => format!("Emissive: {:.0}", lighting.emissive_scale),
            Row::Exposure => format!("Exposure: EV {:.1}", lighting.exposure_ev100),
            Row::LitFog => match lit_fog.as_deref() {
                Some(lit) => format!("Lit fog: {}", on_off(lit.on)),
                None => "Lit fog: - (needs the game's fog)".to_string(),
            },
            Row::LitFogStrength => match lit_fog.as_deref() {
                Some(lit) => format!("Lit fog strength: {}", strength_text(lit.strength)),
                None => "Lit fog strength: -".to_string(),
            },
            Row::Stats => format!("Stats text: {}", on_off(panel.show_stats)),
            Row::Reset => "Light levels".to_string(),
        };
    }
}

/// The saved settings the camera could not take when it was made: ReSTIR,
/// and DLSS (applied once the camera renders; see `start_camera`).
#[derive(Resource)]
pub struct PendingSettings(pub SavedSettings);

fn apply_saved(
    mut commands: Commands,
    pending: Option<Res<PendingSettings>>,
    mut panel: ResMut<OptionsPanel>,
    solari: Query<(Entity, &Camera), With<SolarikLighting>>,
    #[cfg(feature = "dlss")] dlss_supported: Option<Res<DlssRayReconstructionSupported>>,
    #[cfg(feature = "dlss")] mut dlss: Query<&mut Dlss<DlssRayReconstructionFeature>>,
) {
    let Some(pending) = pending else { return };
    let Some((entity, camera)) = solari.iter().next() else { return };
    if !camera.is_active {
        return;
    }
    let saved = &pending.0;
    // (Solarik has no ReSTIR switch: the saved one is kept, unused)
    panel.show_stats = stats_override().unwrap_or(saved.show_stats);
    // (DLSS is on the camera from the start, as it has to be: added to a
    // camera that already renders, it stays off; so the saved quality is
    // set on it, and turning it off removes it)
    #[cfg(feature = "dlss")]
    if dlss_supported.is_some() {
        if !saved.dlss_rr {
            set_dlss_rr(&mut commands, entity, false, None);
        } else if let (Some(quality), Ok(mut dlss)) = (quality_by_name(&saved.dlss_quality), dlss.get_mut(entity)) {
            dlss.perf_quality_mode = quality;
        }
    }
    let _ = entity;
    commands.remove_resource::<PendingSettings>();
}

/// Whether the stats text shows whatever the saved options say
/// (`HALO_RT_STATS=0` hidden, `=1` shown; unset, the saved choice): for the
/// measuring harness, whose pictures are compared between builds and whose
/// noise measure the stats' changing digits would land in. The panel can
/// still turn it, but the value it was given is not saved as the user's
/// choice (see [`save_settings`]). Read once.
fn stats_override() -> Option<bool> {
    static STATS: std::sync::OnceLock<Option<bool>> = std::sync::OnceLock::new();
    *STATS.get_or_init(|| match std::env::var("HALO_RT_STATS").ok().as_deref().map(str::trim) {
        Some("0") => Some(false),
        Some("1") => Some(true),
        _ => None,
    })
}

/// The exposure for this run in place of the saved one
/// (`HALO_RT_EXPOSURE=<ev100>`; unset or not a number, the saved choice): for
/// measuring, where captures are compared at one exposure whatever the
/// user's options hold. The level's dimming still applies on top
/// (`apply_level_exposure`), and the value is not saved as the user's choice
/// (see [`save_settings`]). Read once.
pub fn exposure_override() -> Option<f32> {
    static EXPOSURE: std::sync::OnceLock<Option<f32>> = std::sync::OnceLock::new();
    *EXPOSURE.get_or_init(|| {
        std::env::var("HALO_RT_EXPOSURE").ok().and_then(|v| v.trim().parse::<f32>().ok()).filter(|v| v.is_finite())
    })
}

/// Marks the stats text, which the panel can hide.
#[derive(Component)]
pub struct StatsText;

fn show_stats(panel: Res<OptionsPanel>, mut texts: Query<&mut Visibility, With<StatsText>>) {
    if !panel.is_changed() {
        return;
    }
    for mut visibility in &mut texts {
        *visibility = if panel.show_stats { Visibility::Inherited } else { Visibility::Hidden };
    }
}

/// Saves the settings when they change (checked once a second).
fn save_settings(
    time: Res<Time>,
    mut since: Local<f32>,
    mut last: Local<Option<SavedSettings>>,
    pending: Option<Res<PendingSettings>>,
    panel: Res<OptionsPanel>,
    lighting: Res<LightingSettings>,
    lit_fog: Option<Res<LitFog>>,
    #[cfg(feature = "dlss")] dlss: Query<Option<&Dlss<DlssRayReconstructionFeature>>, With<SolarikLighting>>,
    #[cfg(feature = "dlss")] dlss_supported: Option<Res<DlssRayReconstructionSupported>>,
) {
    *since += time.delta_secs();
    if *since < 1.0 || pending.is_some() {
        return;
    }
    *since = 0.0;
    // (against what the file holds, or the defaults: nothing is written
    // until something changes)
    let last = last.get_or_insert_with(|| SavedSettings::load().unwrap_or_else(SavedSettings::defaults));
    // (where DLSS can't run, as in a build run without its libraries, or is
    // kept off for the run by HALO_RT_NO_DLSS, the saved choice is kept:
    // off there, it was saved off for every run)
    #[cfg(feature = "dlss")]
    let dlss_can_run = dlss_supported.is_some() && std::env::var_os("HALO_RT_NO_DLSS").is_none();
    #[cfg(feature = "dlss")]
    let (dlss_rr, dlss_quality) = match (dlss.iter().next().flatten(), dlss_can_run) {
        (Some(dlss), _) => (true, format!("{:?}", dlss.perf_quality_mode)),
        (None, true) => (false, last.dlss_quality.clone()),
        (None, false) => (last.dlss_rr, last.dlss_quality.clone()),
    };
    #[cfg(not(feature = "dlss"))]
    let (dlss_rr, dlss_quality) = (last.dlss_rr, last.dlss_quality.clone());
    let mut lighting = lighting.clone();
    // (given by HALO_RT_EXPOSURE: the file's value kept, even if the
    // exposure was turned during the run)
    if exposure_override().is_some() {
        lighting.exposure_ev100 = last.lighting.exposure_ev100;
    }
    // (lit fog from its resource; without the game's fog, or given by
    // --lit-fog and not turned in the panel, the file's values kept)
    let (lit_fog, lit_fog_strength) = match lit_fog.as_deref() {
        Some(lit) if crate::fog::lit_fog_override().is_none() || panel.lit_fog_pressed => (lit.on, lit.strength),
        _ => (last.lit_fog, last.lit_fog_strength),
    };
    let current = SavedSettings {
        lighting,
        restir: last.restir,
        dlss_rr,
        dlss_quality,
        // (given by HALO_RT_STATS and not turned in the panel: the file's
        // value kept)
        show_stats: if stats_override().is_some() && !panel.stats_pressed { last.show_stats } else { panel.show_stats },
        lit_fog,
        lit_fog_strength,
    };
    if *last == current {
        return;
    }
    if let Err(e) = current.save() {
        warn!("saving the options: {e}");
    }
    *last = current;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A file from before lit fog loads it off at the default strength; a
    /// strength that isn't a number above 0 keeps the default.
    #[test]
    fn settings_text_lit_fog() {
        let old = "# halo_rt's options (F10 in the viewer)\nemissive = 2\nsun = 3\nsky = 1\nexposure = 9.5\nrestir = true\ndlss_rr = false\ndlss_quality = Quality\nstats = false\n";
        let settings = SavedSettings::parse(old);
        assert!(!settings.lit_fog);
        assert_eq!(settings.lit_fog_strength, LIT_FOG_STRENGTH);
        assert_eq!(settings.dlss_quality, "Quality");
        assert!(!settings.show_stats);
        for bad in ["0", "-2", "nan", "inf", "bright"] {
            let settings = SavedSettings::parse(&format!("lit_fog = true\nlit_fog_strength = {bad}\n"));
            assert!(settings.lit_fog);
            assert_eq!(settings.lit_fog_strength, LIT_FOG_STRENGTH, "{bad}");
        }
    }

    /// What is written reads back the same.
    #[test]
    fn settings_text_round_trip() {
        let mut settings = SavedSettings::defaults();
        settings.lit_fog = true;
        settings.lit_fog_strength = 0.5;
        settings.dlss_quality = "Dlaa".into();
        settings.lighting.exposure_ev100 = 8.25;
        let text = settings.text();
        assert!(text.contains("\nlit_fog = true\n") && text.contains("\nlit_fog_strength = 0.5\n"));
        assert!(SavedSettings::parse(&text) == settings);
        assert!(SavedSettings::parse(&SavedSettings::defaults().text()) == SavedSettings::defaults());
    }

    #[test]
    fn lit_fog_strength_steps() {
        assert_eq!(step_lit_fog_strength(3.0, 1), 5.0);
        assert_eq!(step_lit_fog_strength(3.0, -1), 2.0);
        // (the ends)
        assert_eq!(step_lit_fog_strength(20.0, 1), 20.0);
        assert_eq!(step_lit_fog_strength(0.5, -1), 0.5);
        // (from one not in the list)
        assert_eq!(step_lit_fog_strength(4.0, 1), 5.0);
        assert_eq!(step_lit_fog_strength(4.0, -1), 3.0);
        assert_eq!(step_lit_fog_strength(0.2, -1), 0.2);
        assert_eq!(step_lit_fog_strength(0.2, 1), 0.5);
        assert_eq!(step_lit_fog_strength(30.0, -1), 20.0);
        assert_eq!(strength_text(3.0), "3");
        assert_eq!(strength_text(0.5), "0.5");
    }

    #[test]
    fn test_options_entries() {
        let button = |row, direction| TestPress::Button(OptionButton(row, direction));
        assert_eq!(parse_test_press("120:lit_fog"), Ok((120, "lit_fog", button(Row::LitFog, 0))));
        assert_eq!(parse_test_press(" 5 : lit_fog_strength+"), Ok((5, "lit_fog_strength+", button(Row::LitFogStrength, 1))));
        assert_eq!(parse_test_press("6:lit_fog_strength-"), Ok((6, "lit_fog_strength-", button(Row::LitFogStrength, -1))));
        assert_eq!(parse_test_press("7:stats"), Ok((7, "stats", button(Row::Stats, 0))));
        assert_eq!(parse_test_press("9: panel"), Ok((9, "panel", TestPress::Panel)));
        assert!(parse_test_press("9:Panel").is_err());
        assert!(parse_test_press("lit_fog").is_err());
        assert!(parse_test_press("x:lit_fog").is_err());
        assert!(parse_test_press("8:sun").is_err());
    }
}
