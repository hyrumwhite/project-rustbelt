//! The viewer's options (ROADMAP phase 6): F10 opens a panel for the
//! ray-tracing and DLSS settings and the light levels, which the viewer saves
//! (`halo_rt\settings.txt` in the user's configuration folder) and applies
//! again at the next start. While the panel is open the mouse is the
//! viewer's, not the game's.

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
            .add_systems(Update, (toggle_panel, press_buttons, label_rows, apply_saved, show_stats).chain())
            .add_systems(Last, save_settings);
    }
}

/// Whether the panel is open, and whether the stats text shows.
#[derive(Resource)]
pub struct OptionsPanel {
    pub open: bool,
    pub show_stats: bool,
}

impl Default for OptionsPanel {
    fn default() -> Self {
        OptionsPanel { open: false, show_stats: true }
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
}

fn settings_path() -> Option<PathBuf> {
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
        }
    }

    /// The saved settings, or `None` before any were saved.
    pub fn load() -> Option<Self> {
        let text = std::fs::read_to_string(settings_path()?).ok()?;
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
                _ => {}
            }
        }
        Some(settings)
    }

    fn save(&self) -> std::io::Result<()> {
        let path = settings_path().ok_or_else(|| std::io::Error::other("no configuration folder"))?;
        if let Some(folder) = path.parent() {
            std::fs::create_dir_all(folder)?;
        }
        let l = &self.lighting;
        std::fs::write(
            path,
            format!(
                "# halo_rt's options (F10 in the viewer)\nemissive = {}\nsun = {}\nsky = {}\nexposure = {}\nrestir = {}\ndlss_rr = {}\ndlss_quality = {}\nstats = {}\n",
                l.emissive_scale, l.sun_scale, l.sky_scale, l.exposure_ev100, self.restir, self.dlss_rr, self.dlss_quality, self.show_stats
            ),
        )
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
#[derive(Component, Clone, Copy, PartialEq)]
enum Row {
    DlssRr,
    DlssQuality,
    Sun,
    Sky,
    Emissive,
    Exposure,
    Stats,
    Reset,
}

/// A button: its row, and which way it moves the setting (-1, +1, or 0 to
/// toggle or cycle).
#[derive(Component, Clone, Copy)]
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
            Row::Sun | Row::Sky | Row::Emissive | Row::Exposure => &[(-1, "-"), (1, "+")],
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
    panel.open = !panel.open;
    root.display = if panel.open { Display::Flex } else { Display::None };
    if panel.open {
        // (the mouse is the viewer's while the panel is open)
        let (_, mut cursor) = window.into_inner();
        cursor.grab_mode = bevy::window::CursorGrabMode::None;
        cursor.visible = true;
    }
}

/// Buttons clicked since the last update.
#[derive(Resource, Default)]
struct PressedButtons(Vec<OptionButton>);

fn press_buttons(
    mut pressed: ResMut<PressedButtons>,
    mut panel: ResMut<OptionsPanel>,
    mut lighting: ResMut<LightingSettings>,
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
            Row::Stats => panel.show_stats = !panel.show_stats,
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
    panel.show_stats = saved.show_stats;
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
    let current = SavedSettings {
        lighting: lighting.clone(),
        restir: last.restir,
        dlss_rr,
        dlss_quality,
        show_stats: panel.show_stats,
    };
    if *last == current {
        return;
    }
    if let Err(e) = current.save() {
        warn!("saving the options: {e}");
    }
    *last = current;
}
