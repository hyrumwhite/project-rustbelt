//! The game's display settings in the viewer's window (protocol 55): with
//! `--opence`, the game hidden and its overlay on, the Settings > Video Setup
//! menu (Display Mode, Resolution, Resolution Scaling, window size, V-Sync,
//! Frame Rate Limit) sets this window, as it would the game's own
//! ([`halo_bridge::Bridge::display`]), and the viewer tells the game the size
//! it shows the game's picture at ([`halo_bridge::Bridge::write_viewer_screen`]),
//! which the game then draws its screen, overlay and HUD at.
//!
//! The picture is drawn at the window's size (the direct path, today's: the
//! 3D camera on the window), or, where the settings ask for a smaller picture
//! scaled up (borderless at a resolution under the display's, "original"
//! scaling's 640x480, a window outside 4:3 to 4:1), into an image of its own
//! that a second camera on the window shows aspect-fitted, between black
//! bars (the scaled path); the overlay, the HUD and the screen flash then
//! cover the picture's rectangle, not the window ([`PictureRect`]).
//!
//! `HALO_RT_DISPLAY=fixed|follow` (read once, before the game starts:
//! [`follows`]): fixed is the old fixed 1280x720 window, launch and all, bit
//! for bit; follow is the default in play and fixed for a `--screenshot`
//! run.

use crate::bridge::BridgeLink;
use bevy::{
    app::MainScheduleOrder,
    camera::{ImageRenderTarget, RenderTarget},
    core_pipeline::tonemapping::{DebandDither, Tonemapping},
    ecs::schedule::ScheduleLabel,
    image::ImageSampler,
    prelude::*,
    render::render_resource::{TextureFormat, TextureUsages},
    window::{
        MonitorSelection, PresentMode, PrimaryMonitor, PrimaryWindow, VideoMode, VideoModeSelection, WindowMode,
        WindowPosition, WindowResizeConstraints, WindowResolution,
    },
};
use halo_bridge::{Display as GameDisplay, DisplayMode};
use std::time::{Duration, Instant};

/// The settings before the game has shared its own: a 1280x720 window,
/// vsynced, the game's defaults.
pub const DEFAULT_SETTINGS: GameDisplay = GameDisplay {
    serial: 0,
    mode: DisplayMode::Windowed,
    resolution: None,
    window: (1280, 720),
    vsync: true,
    max_fps: 0,
    original: false,
};

/// The smallest window (logical pixels, as the window's constraints are).
const MINIMUM_WINDOW: (f32, f32) = (640.0, 480.0);

/// "original" scaling's picture.
const ORIGINAL_PICTURE: (u32, u32) = (640, 480);

/// The refresh rate taken when the monitor's isn't known (millihertz).
const DEFAULT_REFRESH_MILLIHERTZ: u32 = 60_000;

/// What the frame limiter spins for at the end of its wait, rather than
/// sleeping past the mark.
const LIMITER_SPIN: Duration = Duration::from_micros(1500);

/// Whether the viewer follows the game's display settings: a game it
/// launched, hidden, with the overlay, unless `HALO_RT_DISPLAY=fixed`; a
/// scripted run (`--screenshot`) only with `HALO_RT_DISPLAY=follow`.
pub fn follows(launched: bool, hidden: bool, overlay: bool, scripted: bool, setting: Option<&str>) -> bool {
    if !(launched && hidden && overlay) {
        return false;
    }
    match setting.map(str::trim) {
        Some(s) if s.eq_ignore_ascii_case("fixed") => false,
        Some(s) if s.eq_ignore_ascii_case("follow") => true,
        _ => !scripted,
    }
}

/// `HALO_RT_DISPLAY` (fixed or follow; unset, follow in play and fixed for
/// a scripted run), read once in `main` before the game is launched.
pub fn display_setting() -> Option<String> {
    std::env::var("HALO_RT_DISPLAY").ok()
}

/// The mode the window is in: the settings', or with F11 toggled the other
/// kind (fullscreen or borderless to the window, the window to borderless),
/// as the game's own F11 does.
pub fn effective_mode(mode: DisplayMode, f11: bool) -> DisplayMode {
    match (mode, f11) {
        (mode, false) => mode,
        (DisplayMode::Windowed, true) => DisplayMode::Borderless,
        (_, true) => DisplayMode::Windowed,
    }
}

/// A rectangle in the window, in physical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PixelRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

/// The size the 3D picture is drawn at and where it sits in the window
/// (physical pixels).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Picture {
    pub width: u32,
    pub height: u32,
    pub rect: PixelRect,
}

impl Picture {
    /// Drawn at the window's size, straight into it.
    pub fn direct(&self, window: (u32, u32)) -> bool {
        (self.width, self.height) == window
    }
}

/// The picture for these settings in the window as it is (`mode` the
/// effective one, after F11): "original" scaling's 640x480; borderless at a
/// resolution that fits in the window, that resolution (as the game does:
/// 1280x720 on a 3440x1440 display drawn small and scaled up); else the
/// window's size with its aspect held to 4:3 .. 4:1, the shapes the game
/// can draw. Aspect-fitted into the window, centred. `native_only`
/// (`HALO_RT_PICTURE_NATIVE=1`) leaves out the first two rules.
pub fn picture(settings: &GameDisplay, mode: DisplayMode, window: (u32, u32), native_only: bool) -> Picture {
    let (w, h) = window;
    if w == 0 || h == 0 {
        return Picture::default();
    }
    let size = if !native_only && settings.original {
        ORIGINAL_PICTURE
    } else if let Some((rw, rh)) = settings.resolution.filter(|&(rw, rh)| !native_only && mode == DisplayMode::Borderless && rw > 0 && rh > 0 && rw <= w && rh <= h) {
        (rw, rh)
    } else if u64::from(w) * 3 < u64::from(h) * 4 {
        // (narrower than 4:3)
        (w, (w * 3 / 4).max(1))
    } else if u64::from(w) > u64::from(h) * 4 {
        // (wider than 4:1)
        (h * 4, h)
    } else {
        (w, h)
    };
    Picture { width: size.0, height: size.1, rect: fit(size, window) }
}

/// `picture` aspect-fitted into `window`, centred (the window itself when
/// the two are the same size).
fn fit(picture: (u32, u32), window: (u32, u32)) -> PixelRect {
    let (pw, ph) = (u64::from(picture.0), u64::from(picture.1));
    let (w, h) = (u64::from(window.0), u64::from(window.1));
    let (rw, rh) = if pw * h <= ph * w {
        // (as tall as the window)
        (((pw * h + ph / 2) / ph).min(w), h)
    } else {
        (w, ((ph * w + pw / 2) / pw).min(h))
    };
    PixelRect { x: ((w - rw) / 2) as u32, y: ((h - rh) / 2) as u32, width: rw as u32, height: rh as u32 }
}

/// The monitor's video mode nearest a resolution: its own size if it has
/// it, else the nearest by size; among those the highest refresh rate,
/// then the deepest colour.
pub fn choose_video_mode(modes: &[VideoMode], resolution: (u32, u32)) -> Option<VideoMode> {
    let distance = |mode: &VideoMode| {
        u64::from(mode.physical_size.x.abs_diff(resolution.0)).pow(2) + u64::from(mode.physical_size.y.abs_diff(resolution.1)).pow(2)
    };
    modes
        .iter()
        .min_by_key(|mode| (distance(mode), std::cmp::Reverse(mode.refresh_rate_millihertz), std::cmp::Reverse(mode.bit_depth)))
        .copied()
}

/// The frame limiter's interval: none with vsync or a negative limit, else
/// `max_fps` frames a second, 0 twice the refresh rate (60 Hz if unknown).
pub fn frame_interval(vsync: bool, max_fps: i32, refresh_millihertz: Option<u32>) -> Option<Duration> {
    if vsync || max_fps < 0 {
        return None;
    }
    let rate = if max_fps == 0 {
        2.0 * f64::from(refresh_millihertz.filter(|&r| r > 0).unwrap_or(DEFAULT_REFRESH_MILLIHERTZ)) / 1000.0
    } else {
        f64::from(max_fps)
    };
    Some(Duration::from_secs_f64(1.0 / rate))
}

/// One change of a `HALO_RT_TEST_DISPLAY` step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Change {
    Mode(DisplayMode),
    Resolution(Option<(u32, u32)>),
    Window((u32, u32)),
    Vsync(bool),
    Fps(i32),
    Original(bool),
    F11,
}

impl Change {
    /// The settings with this change (F11 is the toggle's, not theirs).
    fn apply(self, settings: &mut GameDisplay) {
        match self {
            Change::Mode(mode) => settings.mode = mode,
            Change::Resolution(resolution) => settings.resolution = resolution,
            Change::Window(window) => settings.window = window,
            Change::Vsync(vsync) => settings.vsync = vsync,
            Change::Fps(fps) => settings.max_fps = fps,
            Change::Original(original) => settings.original = original,
            Change::F11 => {}
        }
    }
}

fn parse_size(text: &str) -> Option<(u32, u32)> {
    let (width, height) = text.split_once(['x', 'X'])?;
    let (width, height) = (width.trim().parse::<u32>().ok()?, height.trim().parse::<u32>().ok()?);
    (width > 0 && height > 0).then_some((width, height))
}

/// One `HALO_RT_TEST_DISPLAY` step, `<frame>:<key=value,...>`: keys
/// `mode=windowed|borderless|fullscreen`, `resolution=native|<W>x<H>`,
/// `window=<W>x<H>`, `vsync=0|1`, `fps=<N>`, `scaling=native|original`, and
/// the bare word `f11`.
pub fn parse_test_step(entry: &str) -> Result<(u32, Vec<Change>), String> {
    let (frame, changes) = entry.split_once(':').ok_or("no <frame>:")?;
    let frame = frame.trim().parse::<u32>().map_err(|e| format!("frame {frame:?}: {e}"))?;
    let mut parsed = Vec::new();
    for change in changes.split(',').map(str::trim).filter(|c| !c.is_empty()) {
        if change.eq_ignore_ascii_case("f11") {
            parsed.push(Change::F11);
            continue;
        }
        let (key, value) = change.split_once('=').ok_or_else(|| format!("{change:?} isn't key=value"))?;
        let (key, value) = (key.trim().to_ascii_lowercase(), value.trim().to_ascii_lowercase());
        let bad = || format!("{change:?}: bad value");
        parsed.push(match key.as_str() {
            "mode" => Change::Mode(match value.as_str() {
                "windowed" | "window" => DisplayMode::Windowed,
                "borderless" => DisplayMode::Borderless,
                "fullscreen" => DisplayMode::Fullscreen,
                _ => return Err(bad()),
            }),
            "resolution" => Change::Resolution(if value == "native" { None } else { Some(parse_size(&value).ok_or_else(bad)?) }),
            "window" => Change::Window(parse_size(&value).ok_or_else(bad)?),
            "vsync" => Change::Vsync(match value.as_str() {
                "1" | "on" => true,
                "0" | "off" => false,
                _ => return Err(bad()),
            }),
            "fps" => Change::Fps(value.parse().map_err(|_| bad())?),
            "scaling" => Change::Original(match value.as_str() {
                "original" => true,
                "native" => false,
                _ => return Err(bad()),
            }),
            _ => return Err(format!("{change:?}: unknown key")),
        });
    }
    Ok((frame, parsed))
}

/// `HALO_RT_TEST_DISPLAY="<frame>:<key=value,...>[;<frame>:...]"`: display
/// settings as if the game had shared them, at frames of
/// `crate::CaptureFrames` (follow mode only; a later serial from the game
/// replaces them all); an entry that doesn't parse is left out with a
/// warning. Read once.
fn test_steps() -> &'static [(u32, Vec<Change>)] {
    static STEPS: std::sync::OnceLock<Vec<(u32, Vec<Change>)>> = std::sync::OnceLock::new();
    STEPS.get_or_init(|| {
        std::env::var("HALO_RT_TEST_DISPLAY")
            .unwrap_or_default()
            .split(';')
            .filter(|entry| !entry.trim().is_empty())
            .filter_map(|entry| match parse_test_step(entry) {
                Ok(step) => Some(step),
                Err(e) => {
                    warn!("HALO_RT_TEST_DISPLAY: {entry:?} left out: {e}");
                    None
                }
            })
            .collect()
    })
}

/// The window the app starts with in follow mode, from the settings the game
/// shared before it (`None`: the defaults, and the settings applied once
/// they come): resizable down to 640x480, centred; the window's size
/// (physical pixels from the first frame, [`apply_display`]: made in logical
/// ones, the window's scale factor is not known yet), borderless, or
/// exclusive fullscreen (at a resolution of its own, borderless until the
/// monitor's video modes are known).
pub fn initial_window(window: &mut Window, settings: Option<&GameDisplay>) {
    let settings = settings.unwrap_or(&DEFAULT_SETTINGS);
    window.resizable = true;
    window.resize_constraints = WindowResizeConstraints { min_width: MINIMUM_WINDOW.0, min_height: MINIMUM_WINDOW.1, ..default() };
    window.position = WindowPosition::Centered(MonitorSelection::Primary);
    let (width, height) = settings.window;
    window.resolution = WindowResolution::new(width.max(MINIMUM_WINDOW.0 as u32), height.max(MINIMUM_WINDOW.1 as u32));
    // (the window being made has no monitor of its own yet: the primary)
    window.mode = match (settings.mode, settings.resolution) {
        (DisplayMode::Windowed, _) => WindowMode::Windowed,
        (DisplayMode::Fullscreen, None) => WindowMode::Fullscreen(MonitorSelection::Primary, VideoModeSelection::Current),
        (DisplayMode::Borderless, _) | (DisplayMode::Fullscreen, Some(_)) => WindowMode::BorderlessFullscreen(MonitorSelection::Primary),
    };
    if std::env::var_os("HALO_RT_NO_VSYNC").is_none() {
        window.present_mode = if settings.vsync { PresentMode::AutoVsync } else { PresentMode::AutoNoVsync };
    }
}

/// Follow mode's systems (added only then).
pub struct DisplayPlugin {
    /// The settings the game shared before the window was made.
    pub initial: Option<GameDisplay>,
}

/// The frame limiter's place: after `Last`, the very end of the frame.
#[derive(ScheduleLabel, Debug, Clone, PartialEq, Eq, Hash)]
struct FrameLimit;

impl Plugin for DisplayPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(DisplayState::new(self.initial))
            .init_resource::<ScaledView>()
            .init_resource::<FrameLimiter>()
            .add_systems(Update, (apply_display, layout_picture).chain());
        app.world_mut().resource_mut::<MainScheduleOrder>().insert_after(Last, FrameLimit);
        app.add_systems(FrameLimit, limit_frames);
    }
}

/// A full-window UI node that covers the picture's rectangle instead (the
/// game's overlay, the HUD, the screen flash): left as it was spawned (the
/// whole window) in fixed mode and the direct path.
#[derive(Component)]
pub struct PictureRect;

/// The settings followed and what they made.
#[derive(Resource)]
pub struct DisplayState {
    /// The game's settings, or the test hook's over them.
    settings: GameDisplay,
    /// The game's serial last taken (0 none yet).
    game_serial: u32,
    /// F11's toggle (a new serial clears it).
    f11: bool,
    /// Settings to apply this frame.
    dirty: bool,
    /// Exclusive fullscreen at a resolution, waiting for the monitor's modes.
    awaiting_monitor: bool,
    /// The effective mode and the settings' window size last applied.
    applied_mode: Option<DisplayMode>,
    applied_window: Option<(u32, u32)>,
    /// The window's size (physical) and the picture last laid out, and the
    /// UI's scale then.
    window: (u32, u32),
    picture: Option<Picture>,
    ui_scale: f32,
    limit: Option<Duration>,
    /// The limit is twice a refresh rate not known yet (60 Hz's taken
    /// meanwhile): made again once the monitor is.
    refresh_pending: bool,
    /// `HALO_RT_PICTURE_NATIVE=1`: the picture always the window's size
    /// (never borderless at a resolution nor "original" scaled up; an A/B
    /// for the scaled path).
    native_only: bool,
    /// `HALO_RT_NO_VSYNC=1` (default_plugins'): no vsync whatever the
    /// settings say.
    no_vsync: bool,
    /// `HALO_RT_NO_FRAME_LIMIT=1`: no frame limiter.
    no_frame_limit: bool,
}

impl DisplayState {
    fn new(initial: Option<GameDisplay>) -> Self {
        Self {
            settings: initial.unwrap_or(DEFAULT_SETTINGS),
            game_serial: initial.map_or(0, |d| d.serial),
            f11: false,
            dirty: true,
            awaiting_monitor: false,
            applied_mode: None,
            applied_window: None,
            window: (0, 0),
            picture: None,
            ui_scale: 1.0,
            limit: None,
            refresh_pending: false,
            native_only: std::env::var_os("HALO_RT_PICTURE_NATIVE").is_some(),
            no_vsync: std::env::var_os("HALO_RT_NO_VSYNC").is_some(),
            no_frame_limit: std::env::var_os("HALO_RT_NO_FRAME_LIMIT").is_some(),
        }
    }

    fn mode(&self) -> DisplayMode {
        effective_mode(self.settings.mode, self.f11)
    }

    /// The picture's rectangle while it is scaled into the window (`None` in
    /// the direct path).
    pub fn scaled_rect(&self) -> Option<PixelRect> {
        self.picture.filter(|p| !p.direct(self.window)).map(|p| p.rect)
    }

    /// The capture note's line.
    pub fn note_line(&self) -> String {
        let picture = self.picture.unwrap_or_default();
        format!(
            "display: {:?}{} (settings {:?}{}{}), window {}x{}, picture {}x{} ({}), vsync {}, limit {}",
            self.mode(),
            if self.f11 { " by F11" } else { "" },
            self.settings.mode,
            self.settings.resolution.map_or_else(|| ", native".to_string(), |(w, h)| format!(", {w}x{h}")),
            if self.settings.original { ", original scaling" } else { "" },
            self.window.0,
            self.window.1,
            picture.width,
            picture.height,
            if picture.direct(self.window) {
                "direct".to_string()
            } else {
                format!("scaled into {}x{} at {},{}", picture.rect.width, picture.rect.height, picture.rect.x, picture.rect.y)
            },
            if self.settings.vsync && !self.no_vsync { "on" } else { "off" },
            limit_text(self.limit)
        )
    }
}

fn limit_text(limit: Option<Duration>) -> String {
    limit.map_or_else(|| "none".to_string(), |interval| format!("{:.0} fps", 1.0 / interval.as_secs_f64()))
}

/// Takes the game's settings when it shares new ones (and the test hook's,
/// and F11), and applies them to the window: its mode, its size (on entering
/// the window or a new window size: a drag's size is kept otherwise), its
/// vsync and the frame limit.
fn apply_display(
    link: Res<BridgeLink>,
    mut state: ResMut<DisplayState>,
    keys: Res<ButtonInput<KeyCode>>,
    frames: Res<crate::CaptureFrames>,
    mut window: Single<&mut Window, With<PrimaryWindow>>,
    monitors: Query<&bevy::window::Monitor, With<PrimaryMonitor>>,
) {
    if let Some(game) = link.bridge.display()
        && game.serial != state.game_serial
    {
        state.settings = game;
        state.game_serial = game.serial;
        state.f11 = false;
        state.dirty = true;
    }
    for (frame, changes) in test_steps() {
        if *frame != frames.0 {
            continue;
        }
        info!("test display at frame {frame}: {changes:?}");
        for change in changes {
            change.apply(&mut state.settings);
            if *change == Change::F11 {
                state.f11 = !state.f11;
            }
        }
        state.dirty = true;
    }
    if keys.just_pressed(KeyCode::F11) {
        state.f11 = !state.f11;
        state.dirty = true;
    }
    if state.refresh_pending
        && let Some(refresh) = monitors.iter().next().and_then(|m| m.refresh_rate_millihertz)
    {
        state.refresh_pending = false;
        state.limit = frame_interval(state.settings.vsync, state.settings.max_fps, Some(refresh));
        info!("display: the monitor's refresh rate {:.1} Hz, limit {}", f64::from(refresh) / 1000.0, limit_text(state.limit));
    }
    if !state.dirty && !state.awaiting_monitor {
        return;
    }
    state.dirty = false;
    state.awaiting_monitor = false;
    let settings = state.settings;
    let mode = state.mode();
    let monitor = monitors.iter().next();
    // (the window's size after this, for the log)
    let mut size = (window.physical_width(), window.physical_height());
    let monitor_size = monitor.map(|m| (m.physical_width, m.physical_height));
    match mode {
        DisplayMode::Windowed => {
            window.mode = WindowMode::Windowed;
            if state.applied_mode != Some(DisplayMode::Windowed) || state.applied_window != Some(settings.window) {
                size = (settings.window.0.max(MINIMUM_WINDOW.0 as u32), settings.window.1.max(MINIMUM_WINDOW.1 as u32));
                window.resolution.set_physical_resolution(size.0, size.1);
                window.position = WindowPosition::Centered(MonitorSelection::Current);
            }
        }
        DisplayMode::Borderless => {
            window.mode = WindowMode::BorderlessFullscreen(MonitorSelection::Current);
            size = monitor_size.unwrap_or(size);
        }
        DisplayMode::Fullscreen => match settings.resolution {
            None => {
                window.mode = WindowMode::Fullscreen(MonitorSelection::Current, VideoModeSelection::Current);
                size = monitor_size.unwrap_or(size);
            }
            Some(resolution) => match monitor.map(|m| choose_video_mode(&m.video_modes, resolution)) {
                Some(Some(video_mode)) => {
                    window.mode = WindowMode::Fullscreen(MonitorSelection::Current, VideoModeSelection::Specific(video_mode));
                    size = (video_mode.physical_size.x, video_mode.physical_size.y);
                }
                // (a monitor without modes: its own)
                Some(None) => {
                    window.mode = WindowMode::Fullscreen(MonitorSelection::Current, VideoModeSelection::Current);
                    size = monitor_size.unwrap_or(size);
                }
                // (no monitor known yet: borderless until there is)
                None => {
                    window.mode = WindowMode::BorderlessFullscreen(MonitorSelection::Current);
                    state.awaiting_monitor = true;
                }
            },
        },
    }
    let present_mode = if settings.vsync && !state.no_vsync { PresentMode::AutoVsync } else { PresentMode::AutoNoVsync };
    if window.present_mode != present_mode {
        window.present_mode = present_mode;
    }
    let refresh = monitor.and_then(|m| m.refresh_rate_millihertz);
    state.limit = if state.no_frame_limit { None } else { frame_interval(settings.vsync, settings.max_fps, refresh) };
    state.refresh_pending = state.limit.is_some() && settings.max_fps == 0 && refresh.is_none();
    state.applied_mode = Some(mode);
    state.applied_window = Some(settings.window);
    let picture = picture(&settings, mode, size, state.native_only);
    info!(
        "display: {mode:?}{} (settings serial {}: {:?}, resolution {}, window {}x{}{}), window {}x{}, picture {}x{}, rect {}x{} at {},{}, vsync {}, limit {}{}",
        if state.f11 { " by F11" } else { "" },
        settings.serial,
        settings.mode,
        settings.resolution.map_or_else(|| "native".to_string(), |(w, h)| format!("{w}x{h}")),
        settings.window.0,
        settings.window.1,
        if settings.original { ", original scaling" } else { "" },
        size.0,
        size.1,
        picture.width,
        picture.height,
        picture.rect.width,
        picture.rect.height,
        picture.rect.x,
        picture.rect.y,
        if present_mode == PresentMode::AutoVsync { "on" } else { "off" },
        limit_text(state.limit),
        if state.awaiting_monitor { " (borderless until the monitor's modes are known)" } else { "" }
    );
}

/// The scaled path's window camera and picture node, while there are any.
#[derive(Resource, Default)]
struct ScaledView {
    image: Option<Handle<Image>>,
    camera: Option<Entity>,
    node: Option<Entity>,
}

/// The node showing the scaled picture.
#[derive(Component)]
struct ScaledPicture;

/// The image the 3D camera draws into in the scaled path: the display's
/// encoding, as the window's surface is (Bevy's passes take it as
/// Rgba8UnormSrgb either way), sampled linearly where it is shown.
fn picture_image(width: u32, height: u32) -> Image {
    let mut image = Image::new_target_texture(width, height, TextureFormat::Rgba8UnormSrgb, None);
    image.texture_descriptor.usage |= TextureUsages::COPY_SRC;
    // (drawn by the camera: nothing to upload, nor to copy from the image
    // of another size before it)
    image.data = None;
    image.copy_on_resize = false;
    image.sampler = ImageSampler::linear();
    image
}

/// A node at a rectangle of physical pixels (UI pixels are logical: the
/// window's scale factor times `UiScale`).
fn rect_node(node: &mut Node, rect: PixelRect, scale: f32) {
    let (left, top, width, height) = (px(rect.x as f32 / scale), px(rect.y as f32 / scale), px(rect.width as f32 / scale), px(rect.height as f32 / scale));
    if node.left != left || node.top != top || node.width != width || node.height != height {
        (node.left, node.top, node.width, node.height) = (left, top, width, height);
    }
}

/// The whole window, as the nodes were spawned.
fn window_node(node: &mut Node) {
    let (left, top, width, height) = (Val::Auto, Val::Auto, percent(100), percent(100));
    if node.left != left || node.top != top || node.width != width || node.height != height {
        (node.left, node.top, node.width, node.height) = (left, top, width, height);
    }
}

/// The picture for the window as it is now: written for the game when its
/// size changes, and drawn by the direct path (the 3D camera on the window,
/// the window camera gone) or the scaled one (the 3D camera on an image of
/// the picture's size, shown in the rectangle by a camera on the window,
/// which then draws all the UI).
fn layout_picture(
    mut commands: Commands,
    link: Res<BridgeLink>,
    mut state: ResMut<DisplayState>,
    mut view: ResMut<ScaledView>,
    window: Single<&Window, With<PrimaryWindow>>,
    ui_scale: Res<UiScale>,
    mut images: ResMut<Assets<Image>>,
    camera: Single<(&mut RenderTarget, &mut Projection), With<Camera3d>>,
    mut rect_nodes: Query<&mut Node, (With<PictureRect>, Without<ScaledPicture>)>,
    mut picture_nodes: Query<&mut Node, (With<ScaledPicture>, Without<PictureRect>)>,
) {
    let size = (window.physical_width(), window.physical_height());
    // (minimised: the picture as it was)
    if size.0 == 0 || size.1 == 0 {
        return;
    }
    let scale = window.scale_factor() * ui_scale.0;
    let picture = picture(&state.settings, state.mode(), size, state.native_only);
    // (every frame: the game makes the shared memory and zeroes it, so a
    // size written before it had would be lost)
    link.bridge.write_viewer_screen(picture.width, picture.height);
    if state.picture == Some(picture) && state.window == size && state.ui_scale == scale {
        return;
    }
    let was_direct = state.picture.is_none_or(|p| p.direct(state.window));
    state.picture = Some(picture);
    state.window = size;
    state.ui_scale = scale;
    let direct = picture.direct(size);
    let (mut target, mut projection) = camera.into_inner();
    if direct {
        if !matches!(*target, RenderTarget::Window(bevy::window::WindowRef::Primary)) {
            *target = RenderTarget::Window(bevy::window::WindowRef::Primary);
            // (Bevy's camera_system looks at a camera's target again only
            // when its projection, window or image changed, not its target)
            projection.set_changed();
        }
        for entity in [view.camera.take(), view.node.take()].into_iter().flatten() {
            commands.entity(entity).despawn();
        }
        for mut node in &mut rect_nodes {
            window_node(&mut node);
        }
    } else {
        // (the picture's image at its size: a new texture, the camera and
        // the node's bind group told by the asset's Modified event)
        let image = match view.image.clone() {
            Some(image) => {
                if images.get(&image).is_none_or(|i| (i.width(), i.height()) != (picture.width, picture.height)) {
                    images.insert(image.id(), picture_image(picture.width, picture.height)).ok();
                }
                image
            }
            None => {
                let image = images.add(picture_image(picture.width, picture.height));
                view.image = Some(image.clone());
                image
            }
        };
        if target.as_image() != Some(&image) {
            *target = RenderTarget::Image(ImageRenderTarget { handle: image.clone(), scale_factor: 1.0 });
            projection.set_changed();
        }
        if view.camera.is_none() {
            view.camera = Some(
                commands
                    .spawn((
                        Name::new("Window camera"),
                        Camera2d,
                        // (after the 3D camera, which draws the picture)
                        Camera { order: 1, clear_color: ClearColorConfig::Custom(Color::BLACK), ..default() },
                        Msaa::Off,
                        Tonemapping::None,
                        DebandDither::Disabled,
                        // (the UI's camera: the 3D one draws into the image)
                        IsDefaultUiCamera,
                    ))
                    .id(),
            );
        }
        match view.node {
            Some(_) => {
                for mut node in &mut picture_nodes {
                    rect_node(&mut node, picture.rect, scale);
                }
            }
            None => {
                let mut node = Node { position_type: PositionType::Absolute, ..default() };
                rect_node(&mut node, picture.rect, scale);
                view.node = Some(
                    commands
                        .spawn((
                            Name::new("Scaled picture"),
                            ScaledPicture,
                            ImageNode::new(image),
                            node,
                            // under the game's overlay (-3), the HUD (-2) and
                            // the rest
                            GlobalZIndex(-4),
                        ))
                        .id(),
                );
            }
        }
        for mut node in &mut rect_nodes {
            rect_node(&mut node, picture.rect, scale);
        }
    }
    if direct != was_direct || !direct {
        info!(
            "display picture: {}x{} in a {}x{} window, {}",
            picture.width,
            picture.height,
            size.0,
            size.1,
            if direct {
                "direct".to_string()
            } else {
                format!("scaled into {}x{} at {},{}", picture.rect.width, picture.rect.height, picture.rect.x, picture.rect.y)
            }
        );
    }
}

/// The frame limiter's mark.
#[derive(Resource, Default)]
struct FrameLimiter {
    previous: Option<Instant>,
}

/// Holds the frame to the limit (vsync off: the settings' frames a second,
/// [`frame_interval`]): sleeps to just short of the mark, then spins.
fn limit_frames(state: Res<DisplayState>, mut limiter: ResMut<FrameLimiter>) {
    let Some(interval) = state.limit else {
        limiter.previous = None;
        return;
    };
    let Some(previous) = limiter.previous else {
        limiter.previous = Some(Instant::now());
        return;
    };
    let mark = previous + interval;
    let now = Instant::now();
    if mark > now {
        let wait = mark - now;
        if wait > LIMITER_SPIN {
            std::thread::sleep(wait - LIMITER_SPIN);
        }
        while Instant::now() < mark {
            std::thread::yield_now();
        }
    }
    // (more than an interval behind, as after a hitch: started again from
    // now rather than hurrying to catch up)
    let now = Instant::now();
    limiter.previous = Some(if now.duration_since(mark) > interval { now } else { mark });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(mode: DisplayMode, resolution: Option<(u32, u32)>, original: bool) -> GameDisplay {
        GameDisplay { mode, resolution, original, ..DEFAULT_SETTINGS }
    }

    fn rect(x: u32, y: u32, width: u32, height: u32) -> PixelRect {
        PixelRect { x, y, width, height }
    }

    #[test]
    fn display_picture_direct_and_scaled() {
        // a 1280x720 window: drawn straight into it
        let windowed = settings(DisplayMode::Windowed, Some((1280, 720)), false);
        let p = picture(&windowed, DisplayMode::Windowed, (1280, 720), false);
        assert_eq!(p, Picture { width: 1280, height: 720, rect: rect(0, 0, 1280, 720) });
        assert!(p.direct((1280, 720)));
        // borderless at 1280x720 on 3440x1440: drawn small, scaled up
        let borderless = settings(DisplayMode::Borderless, Some((1280, 720)), false);
        let p = picture(&borderless, DisplayMode::Borderless, (3440, 1440), false);
        assert_eq!(p, Picture { width: 1280, height: 720, rect: rect(440, 0, 2560, 1440) });
        assert!(!p.direct((3440, 1440)));
        // the same at native: the window
        let p = picture(&borderless, DisplayMode::Borderless, (3440, 1440), true);
        assert_eq!(p, Picture { width: 3440, height: 1440, rect: rect(0, 0, 3440, 1440) });
        // "original" scaling
        let original = settings(DisplayMode::Borderless, None, true);
        let p = picture(&original, DisplayMode::Borderless, (3440, 1440), false);
        assert_eq!(p, Picture { width: 640, height: 480, rect: rect(760, 0, 1920, 1440) });
        // a square window: 4:3 at most
        let p = picture(&DEFAULT_SETTINGS, DisplayMode::Windowed, (1000, 1000), false);
        assert_eq!(p, Picture { width: 1000, height: 750, rect: rect(0, 125, 1000, 750) });
        // wider than 4:1
        let p = picture(&DEFAULT_SETTINGS, DisplayMode::Windowed, (5000, 1000), false);
        assert_eq!(p, Picture { width: 4000, height: 1000, rect: rect(500, 0, 4000, 1000) });
        // a resolution larger than the window: the window
        let large = settings(DisplayMode::Borderless, Some((3840, 2160)), false);
        let p = picture(&large, DisplayMode::Borderless, (3440, 1440), false);
        assert_eq!(p, Picture { width: 3440, height: 1440, rect: rect(0, 0, 3440, 1440) });
        // a resolution in the window mode, or by F11's window: the window
        let p = picture(&borderless, effective_mode(DisplayMode::Borderless, true), (1600, 900), false);
        assert_eq!(p, Picture { width: 1600, height: 900, rect: rect(0, 0, 1600, 900) });
        // minimised
        assert_eq!(picture(&DEFAULT_SETTINGS, DisplayMode::Windowed, (0, 0), false), Picture::default());
    }

    #[test]
    fn display_f11_toggles() {
        assert_eq!(effective_mode(DisplayMode::Windowed, true), DisplayMode::Borderless);
        assert_eq!(effective_mode(DisplayMode::Borderless, true), DisplayMode::Windowed);
        assert_eq!(effective_mode(DisplayMode::Fullscreen, true), DisplayMode::Windowed);
        assert_eq!(effective_mode(DisplayMode::Fullscreen, false), DisplayMode::Fullscreen);
    }

    #[test]
    fn display_follows() {
        assert!(follows(true, true, true, false, None));
        assert!(!follows(true, true, true, true, None));
        assert!(follows(true, true, true, true, Some("follow")));
        assert!(!follows(true, true, true, false, Some("fixed")));
        assert!(!follows(true, false, true, false, Some("follow")));
        assert!(!follows(true, true, false, false, None));
        assert!(!follows(false, true, true, false, Some("follow")));
    }

    #[test]
    fn display_video_mode_choice() {
        let mode = |w: u32, h: u32, hz: u32, depth: u16| VideoMode { physical_size: UVec2::new(w, h), bit_depth: depth, refresh_rate_millihertz: hz * 1000 };
        let modes = [mode(3440, 1440, 144, 32), mode(3440, 1440, 60, 32), mode(1920, 1080, 60, 32), mode(1920, 1080, 120, 24), mode(1920, 1080, 120, 32), mode(1280, 720, 60, 32)];
        // exact, the highest refresh rate, then the deepest colour
        assert_eq!(choose_video_mode(&modes, (1920, 1080)), Some(mode(1920, 1080, 120, 32)));
        assert_eq!(choose_video_mode(&modes, (3440, 1440)), Some(mode(3440, 1440, 144, 32)));
        // the nearest by size
        assert_eq!(choose_video_mode(&modes, (1300, 740)), Some(mode(1280, 720, 60, 32)));
        assert_eq!(choose_video_mode(&[], (1280, 720)), None);
    }

    #[test]
    fn display_frame_interval() {
        assert_eq!(frame_interval(true, 60, Some(144_000)), None);
        assert_eq!(frame_interval(false, -1, Some(144_000)), None);
        let twice = frame_interval(false, 0, Some(144_000)).unwrap();
        assert!((twice.as_secs_f64() - 1.0 / 288.0).abs() < 1e-9);
        let unknown = frame_interval(false, 0, None).unwrap();
        assert!((unknown.as_secs_f64() - 1.0 / 120.0).abs() < 1e-9);
        let fixed = frame_interval(false, 75, Some(144_000)).unwrap();
        assert!((fixed.as_secs_f64() - 1.0 / 75.0).abs() < 1e-9);
    }

    #[test]
    fn display_test_steps_parse() {
        assert_eq!(
            parse_test_step("600:mode=borderless,resolution=1280x720, vsync=0,fps=90"),
            Ok((600, vec![Change::Mode(DisplayMode::Borderless), Change::Resolution(Some((1280, 720))), Change::Vsync(false), Change::Fps(90)]))
        );
        assert_eq!(
            parse_test_step("10:f11,scaling=original,window=1600x900,resolution=native,mode=fullscreen"),
            Ok((10, vec![Change::F11, Change::Original(true), Change::Window((1600, 900)), Change::Resolution(None), Change::Mode(DisplayMode::Fullscreen)]))
        );
        assert!(parse_test_step("mode=borderless").is_err());
        assert!(parse_test_step("5:mode=huge").is_err());
        assert!(parse_test_step("5:window=0x10").is_err());
        assert!(parse_test_step("5:colour=red").is_err());
        let mut s = DEFAULT_SETTINGS;
        for change in parse_test_step("1:mode=borderless,fps=-1").unwrap().1 {
            change.apply(&mut s);
        }
        assert_eq!((s.mode, s.max_fps, s.window), (DisplayMode::Borderless, -1, (1280, 720)));
    }
}
