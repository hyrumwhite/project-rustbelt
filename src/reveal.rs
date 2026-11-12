//! A level revealed once it is in place: a black cover over the window from
//! the level's load until no render pipeline has been compiling for a
//! while (materials drawn as their shaders finish popped in one by one, and
//! squares showed while the lighting's caches filled), then faded out.
//!
//! While pipelines compile (the first start compiles about 100 of them for
//! most of a minute before the level shows, and 20 more once the game
//! runs) the cover says "compiling shaders..." over a bar, shown once
//! pipelines have been pending under the cover for [`INDICATOR_AFTER`] (a
//! level change mostly settles sooner) and hidden when none is pending. The
//! process's first cover fills its bar by the time the last start's compile
//! took (`loading.txt` beside the options' file, written when that compile
//! ends): the share of pipelines finished is a poor clock, which leapt from
//! 13% to 97% in 8 s and then stood for 21 s while one pipeline compiled
//! alone. With no time remembered, and on later covers (a level change
//! compiles a few in a second or two), the bar shows the share finished or
//! a share of the time passed, whichever is more.
//! The UI's pipeline is created as soon as it is queued rather than on the
//! pool behind the rest (`PipelineCache::create_immediately`), or none of
//! this would be drawn until most of the compile is over. Meanwhile frames
//! are held to about 30 a second ([`LOADING_FRAME_TIME`]): faster frames
//! slowed the compile by a tenth and ran scripted captures' frame counts
//! out before the cover lifted. The cover lifts [`SETTLE_FRAMES`] frames
//! after the last pipeline, or when none has finished for [`STUCK_AFTER`],
//! or [`MOST_TIME`] after the load.
//!
//! The hold (protocol 54): a game this viewer starts runs no ticks after a
//! campaign map's load until the viewer releases it (`HALO_BEVY_BRIDGE_HOLD=1`
//! in its environment; each frame carries the load's generation while it
//! holds), so the level's intro starts at its first tick under the lifting
//! cover rather than half a minute in behind it. The hold is released when
//! the viewer's level is the game's and the cover may lift (or at once when
//! the game's map can't be loaded); the cover then waits for the game's tick
//! to move and `HALO_RT_REVEAL_AFTER_RELEASE` frames more before it fades.
//!
//! Switches, read once: `HALO_RT_LOADING_SCREEN=0` the cover as before (no
//! text or bar, lifted after [`MOST_FRAMES`] frames whatever is compiling,
//! no hold asked for and any hold released at once);
//! `HALO_RT_NO_LOADING_HOLD=1` no hold asked for; `HALO_RT_PIPELINE_LOG=1`
//! a line each time the pipelines' counts change;
//! `HALO_RT_REVEAL_AFTER_RELEASE=<frames>` the frames drawn after the game
//! starts before the fade (20); `HALO_RT_LOADING_FPS=<fps>` the frame rate
//! held while pipelines compile under the cover (30; 0 no limit);
//! `HALO_RT_LOADING_ESTIMATE=0` neither reads nor writes `loading.txt` (the
//! bar by the count and the time share), `=<seconds>` takes the compile to
//! last that long and writes nothing.

use std::{
    sync::{
        Arc, OnceLock,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use bevy::{
    prelude::*,
    shader::ShaderCacheError,
    render::{
        Render, RenderApp, RenderStartup, RenderSystems,
        render_resource::{CachedPipelineState, PipelineCache, PipelineDescriptor},
    },
};

use crate::{bridge::BridgeLink, level::LevelStats};

/// Frames with no pipeline compiling before the cover lifts: the lighting's
/// caches and history settling too.
const SETTLE_FRAMES: u32 = 45;
/// Frames the cover takes to fade.
const FADE_FRAMES: u32 = 20;
/// (`HALO_RT_LOADING_SCREEN=0`) Frames after which the cover lifts whatever
/// is compiling. With a pipeline waited for no longer blocking the frame,
/// frames run fast while the pool compiles and this many pass in seconds,
/// so the loading screen counts wall time instead.
const MOST_FRAMES: u32 = 1200;
/// The cover lifts when no pipeline has finished for this long: one is
/// stuck (the slowest compile in seconds).
const STUCK_AFTER: Duration = Duration::from_secs(120);
/// The cover lifts this long after the load whatever is compiling.
const MOST_TIME: Duration = Duration::from_secs(600);
/// How long pipelines are pending under the cover before it says so: a
/// level change mostly settles in 45 to 70 frames, which must not flash it.
const INDICATOR_AFTER: Duration = Duration::from_millis(750);
/// The bar's time share, which it shows when more than the share of
/// pipelines finished: `TIME_SHARE_CEILING * (1 - exp(-seconds pending /
/// TIME_SHARE_SECONDS))`. The first start takes about a minute on the
/// development machine; the count is honest but lumpy (four heavy compute
/// pipelines hold the pool's four threads for most of the first half
/// minute, the count near a fifth all that while), the time share keeps the
/// bar moving and never claims more than nine tenths.
const TIME_SHARE_CEILING: f32 = 0.9;
const TIME_SHARE_SECONDS: f32 = 45.0;
/// (With the startup compile's time remembered) the share of it after which
/// the bar slows, the share it then creeps toward over that, and how fast
/// (in shares of the expected time): a start that runs long keeps creeping
/// and never claims to be done.
const ESTIMATE_KNEE: f32 = 0.9;
const ESTIMATE_OVERRUN_SHARE: f32 = 0.09;
const ESTIMATE_OVERRUN_SCALE: f32 = 0.3;
/// The startup compile times believed from `loading.txt`, in seconds.
const ESTIMATE_RANGE: std::ops::RangeInclusive<f32> = 1.0..=3600.0;
/// The bar shows no more than this until nothing is pending.
const PROGRESS_CAP: f32 = 0.99;
/// After a release, the cover fades anyway when the game's tick hasn't
/// moved for this long (the game paused or stopped).
const RELEASE_TICK_WAIT: Duration = Duration::from_secs(3);
/// The frame time held while pipelines compile under the cover (30 frames a
/// second; `HALO_RT_LOADING_FPS`).
const LOADING_FRAME_TIME: Duration = Duration::from_nanos(1_000_000_000 / 30);
/// The bar's track, in logical pixels.
const BAR_WIDTH: f32 = 320.0;
const BAR_HEIGHT: f32 = 6.0;

/// The switches, read once from the environment.
struct Settings {
    loading_screen: bool,
    hold: bool,
    pipeline_log: bool,
    reveal_after_release: u32,
    /// None: no limit.
    loading_frame_time: Option<Duration>,
    /// The seconds the startup compile is expected to take, and whether
    /// this start's is written down for the next.
    expected_compile: Option<f32>,
    remember_compile: bool,
}

fn settings() -> &'static Settings {
    static SETTINGS: OnceLock<Settings> = OnceLock::new();
    SETTINGS.get_or_init(|| {
        let loading_screen = std::env::var("HALO_RT_LOADING_SCREEN").map_or(true, |v| v != "0");
        Settings {
            loading_screen,
            hold: loading_screen && std::env::var("HALO_RT_NO_LOADING_HOLD").map_or(true, |v| v != "1"),
            pipeline_log: std::env::var("HALO_RT_PIPELINE_LOG").is_ok_and(|v| v == "1"),
            // (the first shot's cut and the lighting caches' fill stay
            // under the cover, at the price of that much of the shot)
            reveal_after_release: std::env::var("HALO_RT_REVEAL_AFTER_RELEASE").ok().and_then(|v| v.parse().ok()).unwrap_or(20),
            loading_frame_time: match std::env::var("HALO_RT_LOADING_FPS").ok().and_then(|v| v.parse::<f64>().ok()) {
                Some(fps) if fps > 0.0 => Some(Duration::from_secs_f64(1.0 / fps)),
                Some(_) => None,
                None => Some(LOADING_FRAME_TIME),
            },
            expected_compile: match std::env::var("HALO_RT_LOADING_ESTIMATE").ok().and_then(|v| v.parse::<f32>().ok()) {
                Some(seconds) if seconds > 0.0 && seconds.is_finite() => Some(seconds),
                Some(_) => None,
                None => loading_screen.then(read_compile_seconds).flatten(),
            },
            remember_compile: loading_screen && std::env::var("HALO_RT_LOADING_ESTIMATE").ok().and_then(|v| v.parse::<f32>().ok()).is_none(),
        }
    })
}

/// Where the startup compile's time is kept: `loading.txt` beside the
/// options' file.
fn loading_path() -> Option<std::path::PathBuf> {
    Some(crate::options::settings_path()?.with_file_name("loading.txt"))
}

/// The last start's compile time, in seconds, if one was written and is
/// believable.
fn read_compile_seconds() -> Option<f32> {
    let text = std::fs::read_to_string(loading_path()?).ok()?;
    let seconds = text.lines().find_map(|line| line.split_once('=').filter(|(key, _)| key.trim() == "startup_compile_seconds")?.1.trim().parse::<f32>().ok())?;
    (seconds.is_finite() && ESTIMATE_RANGE.contains(&seconds)).then_some(seconds)
}

fn write_compile_seconds(seconds: f32) {
    let Some(path) = loading_path() else { return };
    let written = path.parent().map_or(Ok(()), std::fs::create_dir_all).and_then(|_| std::fs::write(&path, format!("startup_compile_seconds = {seconds:.1}\n")));
    match written {
        Ok(()) => info!("startup shaders compiled in {seconds:.1} s (remembered for the next start's bar)"),
        Err(e) => debug!("could not write {}: {e}", path.display()),
    }
}

/// Whether a game this viewer starts is asked to hold after each map load
/// for the viewer's release (`HALO_BEVY_BRIDGE_HOLD=1` in its environment).
pub fn loading_hold() -> bool {
    settings().hold
}

pub struct RevealPlugin;

impl Plugin for RevealPlugin {
    fn build(&self, app: &mut App) {
        let pending = PendingPipelines::default();
        app.insert_resource(pending.clone())
            .init_resource::<LevelReveal>()
            .init_resource::<UnloadableMap>()
            .add_systems(Startup, spawn_cover)
            .add_systems(PostUpdate, reveal);
        if let Some(render_app) = app.get_sub_app_mut(RenderApp) {
            render_app.insert_resource(pending).add_systems(Render, count_pending.in_set(RenderSystems::Cleanup));
            if settings().loading_screen {
                render_app.add_systems(RenderStartup, create_ui_pipeline_immediately);
            }
        }
    }
}

/// How many render pipelines are queued or compiling, and how many the
/// cache holds, as the render world last counted (the pending count in the
/// low half, the total in the high, so the two are read together).
#[derive(Resource, Clone, Default)]
struct PendingPipelines(Arc<AtomicU64>);

impl PendingPipelines {
    fn store(&self, pending: u32, total: u32) {
        self.0.store((total as u64) << 32 | pending as u64, Ordering::Relaxed);
    }

    /// (pending, total)
    fn load(&self) -> (u32, u32) {
        let both = self.0.load(Ordering::Relaxed);
        (both as u32, (both >> 32) as u32)
    }
}

/// The map the game changed to that couldn't be loaded (the old level
/// stays), set by `follow_level`: a hold for it is released at once.
#[derive(Resource, Default)]
pub struct UnloadableMap(pub Option<String>);

/// The current level's reveal: frames since its load, and frames since a
/// pipeline last compiled. [`LevelReveal::restart`] on each load.
#[derive(Resource, Default)]
pub struct LevelReveal {
    frames: u32,
    settled: u32,
    fade: u32,
    /// When the cover went up (the first frame after the restart).
    started: Option<Instant>,
    /// The pipelines finished when it went up: the bar counts from there.
    baseline: Option<u32>,
    /// The pipelines finished as last seen, and when that count last rose.
    finished: u32,
    finished_at: Option<Instant>,
    /// Since when pipelines have been pending, and when they first were
    /// under this cover (the bar's time share counts from there).
    pending_since: Option<Instant>,
    first_pending: Option<Instant>,
    /// The bar's shown share, 0 to 1.
    progress: f32,
    /// After the game's hold was released: the wait for its tick to move.
    release: Option<ReleaseWait>,
}

impl LevelReveal {
    pub fn restart(&mut self) {
        *self = LevelReveal::default();
    }

    /// Why the cover may lift, if it may.
    fn ready(&self) -> Option<&'static str> {
        if self.settled >= SETTLE_FRAMES {
            Some("settled")
        } else if !settings().loading_screen {
            (self.frames >= MOST_FRAMES).then_some("the frame cap")
        } else if self.finished_at.is_some_and(|at| at.elapsed() >= STUCK_AFTER) {
            Some("no pipeline finished for 120 s")
        } else if self.started.is_some_and(|at| at.elapsed() >= MOST_TIME) {
            Some("600 s after the load")
        } else {
            None
        }
    }

    fn seconds(&self) -> f32 {
        self.started.map_or(0.0, |at| at.elapsed().as_secs_f32())
    }
}

/// A released hold: the game's tick when it was released, when, whether the
/// tick has moved since, and the frames drawn since it did.
struct ReleaseWait {
    tick: u32,
    at: Instant,
    started: bool,
    frames: u32,
}

impl ReleaseWait {
    fn done(&self) -> bool {
        if self.started { self.frames >= settings().reveal_after_release } else { self.at.elapsed() >= RELEASE_TICK_WAIT }
    }
}

#[derive(Component)]
struct Cover;

/// The text and bar on the cover.
#[derive(Component)]
struct Indicator;

/// The bar's fill.
#[derive(Component)]
struct ProgressFill;

fn spawn_cover(mut commands: Commands) {
    commands.spawn((
        Cover,
        Node {
            position_type: PositionType::Absolute,
            width: percent(100.0),
            height: percent(100.0),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..default()
        },
        BackgroundColor(Color::BLACK),
        // (under the banner that says the game has stopped)
        GlobalZIndex(i32::MAX - 1),
        children![(
            Indicator,
            Node { flex_direction: FlexDirection::Column, align_items: AlignItems::Center, row_gap: px(12.0), ..default() },
            Visibility::Hidden,
            children![
                (Text::new("compiling shaders..."), TextFont { font_size: FontSize::Px(18.0), ..default() }, TextColor(Color::WHITE)),
                (
                    Node { width: px(BAR_WIDTH), max_width: vw(80.0), height: px(BAR_HEIGHT), ..default() },
                    BackgroundColor(Color::WHITE.with_alpha(0.2)),
                    children![(ProgressFill, Node { width: percent(0.0), height: percent(100.0), ..default() }, BackgroundColor(Color::WHITE))],
                ),
            ],
        )],
    ));
}

/// The UI's pipeline (`ui_pipeline`, bevy_ui_render's, which draws its
/// nodes' backgrounds and text glyphs alike) created as soon as it is
/// queued, so the cover and its indicator are drawn while the pool
/// compiles. (RenderStartup runs once a device's PipelineCache is in place,
/// before any Render schedule processes the queue, and again for a new
/// cache after a lost device.)
fn create_ui_pipeline_immediately(mut cache: ResMut<PipelineCache>) {
    cache.create_immediately("ui_pipeline");
}

/// The opt-in log's clock and the counts it last printed.
#[derive(Default)]
struct PipelineLog {
    frame: u32,
    start: Option<Instant>,
    last: Option<(u32, u32)>,
}

fn count_pending(cache: Res<PipelineCache>, pending: Res<PendingPipelines>, mut log: Local<PipelineLog>) {
    // (a pipeline whose shader isn't loaded yet is put back on the queue and
    // retried: still to come; other errors never compile, and count as done)
    let is_pending = |state: &CachedPipelineState| {
        matches!(
            state,
            CachedPipelineState::Queued
                | CachedPipelineState::Creating(_)
                | CachedPipelineState::Err(ShaderCacheError::ShaderNotLoaded(_) | ShaderCacheError::ShaderImportNotYetAvailable)
        )
    };
    let count = cache.pipelines().filter(|p| is_pending(&p.state)).count() as u32;
    let total = cache.pipelines().count() as u32;
    pending.store(count, total);
    if !settings().pipeline_log {
        return;
    }
    // (`HALO_RT_PIPELINE_LOG=1`: the counts as they change, and the last
    // few still waiting by name)
    let start = *log.start.get_or_insert_with(Instant::now);
    log.frame += 1;
    if log.last != Some((count, total)) {
        info!("pipelines: {:.2} s, frame {}: {total} in all, {count} pending", start.elapsed().as_secs_f32(), log.frame);
        if log.last.is_none_or(|(last, _)| last != count) && count <= 14 {
            for p in cache.pipelines().filter(|p| is_pending(&p.state)) {
                let label = match &p.descriptor {
                    PipelineDescriptor::RenderPipelineDescriptor(d) => format!("render {:?}", d.label),
                    PipelineDescriptor::ComputePipelineDescriptor(d) => format!("compute {:?} {:?}", d.label, d.entry_point),
                    _ => "mesh".to_string(),
                };
                info!("pipelines:   waiting: {label}");
            }
        }
        log.last = Some((count, total));
    }
}

fn reveal(
    pending: Res<PendingPipelines>,
    mut level: ResMut<LevelReveal>,
    mut link: Option<ResMut<BridgeLink>>,
    stats: Option<Res<LevelStats>>,
    unloadable: Res<UnloadableMap>,
    // (the last hold generation released, and the one open and when it
    // was first seen)
    mut released: Local<u32>,
    mut hold_seen: Local<Option<(u32, Instant)>>,
    mut last_frame: Local<Option<Instant>>,
    // (the startup compile over, or given up on: the first cover's done)
    mut startup_done: Local<bool>,
    mut cover: Single<(&mut BackgroundColor, &mut Visibility), With<Cover>>,
    mut indicator: Single<&mut Visibility, (With<Indicator>, Without<Cover>)>,
    mut fill: Single<&mut Node, With<ProgressFill>>,
) {
    let settings = settings();
    let now = Instant::now();
    let (pending, total) = pending.load();
    let finished = total.saturating_sub(pending);
    level.frames += 1;
    level.started.get_or_insert(now);
    let baseline = *level.baseline.get_or_insert(finished);
    if level.finished_at.is_none() || finished > level.finished {
        level.finished_at = Some(now);
    }
    level.finished = finished;
    if pending == 0 {
        level.settled += 1;
        level.pending_since = None;
    } else {
        if level.fade == 0 {
            level.settled = 0;
        }
        level.pending_since.get_or_insert(now);
        level.first_pending.get_or_insert(now);
    }
    let ready = level.ready();
    // The startup compile's time, written down when the first cover's
    // pipelines are all done (not when it gave up on them by a cap).
    let by_estimate = settings.expected_compile.filter(|_| !*startup_done);
    if !*startup_done {
        if level.fade > 0 || ready.is_some_and(|ready| ready != "settled") {
            *startup_done = true;
        } else if pending == 0
            && let Some(at) = level.first_pending
        {
            *startup_done = true;
            if settings.remember_compile {
                write_compile_seconds((now - at).as_secs_f32());
            }
        }
    }

    // The game's hold, released once this level is the game's and in place.
    // (follow_level runs in PreUpdate, and its commands, the new level's
    // LevelStats and this reveal's restart among them, are applied when
    // PreUpdate ends: a snapshot of a map not yet loaded never matches the
    // stats here, so the last level's finished reveal can't release a new
    // map's hold.)
    let mut hold_open = false;
    // (a release written again while the game still shows it held: the
    // game zeroes the word when it makes the mapping, and frames exported
    // before the write landed say so harmlessly)
    if let Some(link) = link.as_mut()
        && let Some(generation) = link.latest.as_ref().map(|s| s.hold_generation)
        && generation != 0
        && generation == *released
    {
        link.bridge.release_hold(generation);
    }
    let latest = link.as_ref().and_then(|link| link.latest.as_ref()).map(|s| (s.hold_generation, s.game_tick, s.map_name().to_string()));
    if let Some((generation, tick, map)) = latest
        && generation != 0
        && generation != *released
        && let Some(link) = link.as_mut()
    {
        if hold_seen.is_none_or(|(seen, _)| seen != generation) {
            *hold_seen = Some((generation, now));
        }
        let held_long = hold_seen.is_some_and(|(_, at)| now - at >= MOST_TIME);
        let loaded = stats.as_ref().is_some_and(|stats| map.eq_ignore_ascii_case(&stats.map_name));
        let why = if !settings.loading_screen {
            Some("at once (HALO_RT_LOADING_SCREEN=0)".to_string())
        } else if unloadable.0.as_deref().is_some_and(|failed| failed.eq_ignore_ascii_case(&map)) {
            Some("at once: the level could not be loaded".to_string())
        } else if loaded && level.fade > 0 {
            Some("the level already shown".to_string())
        } else if loaded && let Some(ready) = ready {
            Some(if ready == "settled" { "the level in place".to_string() } else { format!("by a cap ({ready})") })
        } else if held_long {
            Some("by a cap (held 600 s)".to_string())
        } else {
            None
        };
        if let Some(why) = why {
            link.bridge.release_hold(generation);
            *released = generation;
            info!("released the game's hold on {map} (generation {generation}): {why}, {:.1} s after the cover went up", level.seconds());
            // (the fade waits for the game to start; a level already shown
            // stays as it is)
            if settings.loading_screen && level.fade == 0 {
                level.release = Some(ReleaseWait { tick, at: now, started: false, frames: 0 });
            }
        } else {
            hold_open = true;
        }
    }
    // After a release, the game's first tick and a few frames more.
    let tick = link.as_ref().and_then(|link| link.latest.as_ref()).map(|s| s.game_tick);
    if let Some(wait) = &mut level.release {
        if wait.started {
            wait.frames += 1;
        } else if tick.is_some_and(|tick| tick != wait.tick) {
            wait.started = true;
        }
    }

    if level.fade > 0 || (ready.is_some() && !hold_open && level.release.as_ref().is_none_or(ReleaseWait::done)) {
        if level.fade == 0 {
            info!("level revealed after {} frames ({:.1} s)", level.frames, level.seconds());
        }
        level.fade = (level.fade + 1).min(FADE_FRAMES);
    }
    let alpha = 1.0 - level.fade as f32 / FADE_FRAMES as f32;
    let (background, visibility) = &mut *cover;
    background.0 = Color::BLACK.with_alpha(alpha);
    let shown = if alpha > 0.0 { Visibility::Inherited } else { Visibility::Hidden };
    if **visibility != shown {
        **visibility = shown;
    }

    // The indicator: pipelines pending a while under the cover, and the
    // share of them finished since it went up, never going back and never
    // still for long.
    let show_indicator = settings.loading_screen
        && pending > 0
        && level.fade == 0
        && level.started.is_some_and(|at| now - at >= INDICATOR_AFTER)
        && level.pending_since.is_some_and(|at| now - at >= INDICATOR_AFTER);
    indicator.set_if_neq(if show_indicator { Visibility::Inherited } else { Visibility::Hidden });
    // (with nothing pending it stays as it is: the first frames after a
    // restart count nothing yet, and the indicator is hidden then anyway)
    if pending > 0 {
        let seconds_pending = level.first_pending.map_or(0.0, |at| (now - at).as_secs_f32());
        let share = match by_estimate {
            // (the first cover, the compile's time known: by time alone)
            Some(expected) => {
                let x = seconds_pending / expected;
                if x <= ESTIMATE_KNEE {
                    x
                } else {
                    ESTIMATE_KNEE + ESTIMATE_OVERRUN_SHARE * (1.0 - (-(x - ESTIMATE_KNEE) / ESTIMATE_OVERRUN_SCALE).exp())
                }
            }
            None => {
                let (done, all) = (finished.saturating_sub(baseline) as f32, total.saturating_sub(baseline).max(1) as f32);
                let time_share = TIME_SHARE_CEILING * (1.0 - (-seconds_pending / TIME_SHARE_SECONDS).exp());
                (done / all).max(time_share)
            }
        };
        level.progress = level.progress.max(share).min(PROGRESS_CAP);
    }
    if show_indicator {
        let width = percent(level.progress * 100.0);
        if fill.width != width {
            fill.width = width;
        }
    }

    // Frames held to the loading frame rate while pipelines compile under
    // the cover; the settle frames after them run at full rate.
    if settings.loading_screen
        && pending > 0
        && level.fade == 0
        && let Some(frame_time) = settings.loading_frame_time
        && let Some(last) = *last_frame
    {
        let since = last.elapsed();
        if since < frame_time {
            std::thread::sleep(frame_time - since);
        }
    }
    *last_frame = Some(Instant::now());
}
