//! A level revealed once it is in place: a black cover over the window from
//! the level's load until no render pipeline has been compiling for a
//! while (materials drawn as their shaders finish popped in one by one, and
//! squares showed while the lighting's caches filled), then faded out.

use std::sync::{
    Arc,
    atomic::{AtomicU32, Ordering},
};

use bevy::{
    prelude::*,
    render::{
        Render, RenderApp, RenderSystems,
        render_resource::{CachedPipelineState, PipelineCache},
    },
};

/// Frames with no pipeline compiling before the cover lifts: the lighting's
/// caches and history settling too.
const SETTLE_FRAMES: u32 = 45;
/// Frames the cover takes to fade.
const FADE_FRAMES: u32 = 20;
/// Frames after which the cover lifts whatever is compiling.
const MOST_FRAMES: u32 = 1200;

pub struct RevealPlugin;

impl Plugin for RevealPlugin {
    fn build(&self, app: &mut App) {
        let pending = PendingPipelines::default();
        app.insert_resource(pending.clone())
            .init_resource::<LevelReveal>()
            .add_systems(Startup, spawn_cover)
            .add_systems(PostUpdate, reveal);
        if let Some(render_app) = app.get_sub_app_mut(RenderApp) {
            render_app.insert_resource(pending).add_systems(Render, count_pending.in_set(RenderSystems::Cleanup));
        }
    }
}

/// How many render pipelines are queued or compiling, as the render world
/// last counted.
#[derive(Resource, Clone, Default)]
struct PendingPipelines(Arc<AtomicU32>);

/// The current level's reveal: frames since its load, and frames since a
/// pipeline last compiled. [`LevelReveal::restart`] on each load.
#[derive(Resource, Default)]
pub struct LevelReveal {
    frames: u32,
    settled: u32,
    fade: u32,
}

impl LevelReveal {
    pub fn restart(&mut self) {
        *self = LevelReveal::default();
    }
}

#[derive(Component)]
struct Cover;

fn spawn_cover(mut commands: Commands) {
    commands.spawn((
        Cover,
        Node { position_type: PositionType::Absolute, width: percent(100.0), height: percent(100.0), ..default() },
        BackgroundColor(Color::BLACK),
        GlobalZIndex(i32::MAX),
    ));
}

fn count_pending(cache: Res<PipelineCache>, pending: Res<PendingPipelines>) {
    let count = cache.pipelines().filter(|p| matches!(p.state, CachedPipelineState::Queued | CachedPipelineState::Creating(_))).count();
    pending.0.store(count as u32, Ordering::Relaxed);
}

fn reveal(
    pending: Res<PendingPipelines>,
    mut level: ResMut<LevelReveal>,
    mut cover: Single<(&mut BackgroundColor, &mut Visibility), With<Cover>>,
) {
    level.frames += 1;
    if pending.0.load(Ordering::Relaxed) == 0 {
        level.settled += 1;
    } else if level.fade == 0 {
        level.settled = 0;
    }
    if level.settled >= SETTLE_FRAMES || level.frames >= MOST_FRAMES {
        if level.fade == 0 {
            info!("level revealed after {} frames", level.frames);
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
}
