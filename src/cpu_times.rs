//! The CPU's side of a frame: the main schedule's time, and the render
//! thread's without its wait for a window texture (which waits on the GPU
//! and the display), split by its schedule's stretches, each 5 seconds;
//! logged beside the GPU's pass times (`HALO_RT_GPU_LOG=1`), and the latest
//! written with each F9 capture.
//!
//! The stretches, in the render schedule's order: extract (the extract
//! commands applied), assets (meshes and images to the GPU, with the BLAS
//! builds and compactions inside it, timed on their own as `blas`), meshes
//! (the extracted meshes prepared), specialize (views created, the material
//! meshes specialized), views (the views prepared, the window wait taken
//! out), queue (the phases queued and sorted), resources (the prepare set's
//! buffers and uniforms, with the scene's bindings and instance rows inside
//! it, timed as `bindings`), batches (the phases batched and written), bind
//! groups, graph (the render graph's encoding and submit), cleanup.

use bevy::{
    prelude::*,
    render::{view::prepare_windows, Render, RenderApp, RenderSystems},
};
use bevy_solarik::scene::{compact_raytracing_blas, prepare_raytracing_blas, PrepareSceneBindings};
use std::{
    collections::BTreeMap,
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};

pub struct CpuTimesPlugin;

impl Plugin for CpuTimesPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(First, main_started)
            .add_systems(Last, main_ended);
        app.sub_app_mut(RenderApp).add_systems(
            Render,
            (
                render_started.in_set(RenderSystems::ExtractCommands),
                stretch_ended::<EXTRACT>.after(RenderSystems::ExtractCommands).before(RenderSystems::PrepareAssets),
                span_started::<BLAS>
                    .in_set(RenderSystems::PrepareAssets)
                    .after(bevy::render::mesh::allocator::allocate_and_free_meshes)
                    .before(prepare_raytracing_blas),
                span_ended::<BLAS>.in_set(RenderSystems::PrepareAssets).after(prepare_raytracing_blas).before(compact_raytracing_blas),
                span_started::<COMPACT>.in_set(RenderSystems::PrepareAssets).after(prepare_raytracing_blas).before(compact_raytracing_blas),
                span_ended::<COMPACT>.in_set(RenderSystems::PrepareAssets).after(compact_raytracing_blas),
                stretch_ended::<ASSETS>.after(RenderSystems::PrepareAssets).before(RenderSystems::PrepareMeshes),
                stretch_ended::<MESHES>.after(RenderSystems::PrepareMeshes).before(RenderSystems::CreateViews),
                stretch_ended::<SPECIALIZE>.after(RenderSystems::Specialize).before(RenderSystems::PrepareViews),
                window_wait_started.in_set(RenderSystems::PrepareViews).before(prepare_windows),
                window_wait_ended.in_set(RenderSystems::PrepareViews).after(prepare_windows),
                stretch_ended::<VIEWS>.after(RenderSystems::PrepareViews).before(RenderSystems::Queue),
                stretch_ended::<QUEUE>.after(RenderSystems::PhaseSort).before(RenderSystems::Prepare),
                span_started::<BINDINGS>.in_set(RenderSystems::PrepareResources).before(PrepareSceneBindings),
                span_ended::<BINDINGS>.in_set(RenderSystems::PrepareResources).after(PrepareSceneBindings),
                stretch_ended::<RESOURCES>.after(RenderSystems::PrepareResources).before(RenderSystems::PrepareResourcesBatchPhases),
                stretch_ended::<BATCHES>.after(RenderSystems::PrepareResourcesFlush).before(RenderSystems::PrepareBindGroups),
                stretch_ended::<BIND_GROUPS>.after(RenderSystems::PrepareBindGroups).before(RenderSystems::Render),
                stretch_ended::<GRAPH>.after(RenderSystems::Render).before(RenderSystems::Cleanup),
                render_ended.in_set(RenderSystems::PostCleanup),
            ),
        );
    }
}

// The render thread's stretches, in order, and the two spans inside them.
const EXTRACT: usize = 0;
const ASSETS: usize = 1;
const MESHES: usize = 2;
const SPECIALIZE: usize = 3;
const VIEWS: usize = 4;
const QUEUE: usize = 5;
const RESOURCES: usize = 6;
const BATCHES: usize = 7;
const BIND_GROUPS: usize = 8;
const GRAPH: usize = 9;
const STRETCHES: usize = 10;
const STRETCH_NAMES: [&str; STRETCHES] =
    ["extract", "assets", "meshes", "specialize", "views", "queue", "resources", "batches", "bind groups", "graph"];
const BLAS: usize = 0;
const BINDINGS: usize = 1;
const COMPACT: usize = 2;
const SPANS: usize = 3;
const SPAN_NAMES: [&str; SPANS] = ["blas", "bindings", "compact"];
// (the stretch each span sits inside, for the line's brackets)
const SPAN_STRETCH: [usize; SPANS] = [ASSETS, RESOURCES, ASSETS];

#[derive(Default)]
struct Totals {
    // (`BEVY_RENDER_PROFILE=1`: the renderer's own CPU scopes (ms) and counts, summed by name)
    profile: BTreeMap<&'static str, (f64, u32)>,
    main: Duration,
    main_frames: u32,
    render: Duration,
    stretches: [Duration; STRETCHES],
    spans: [Duration; SPANS],
    window_wait: Duration,
    render_frames: u32,
    since: Option<Instant>,
}

#[derive(Default)]
struct Marks {
    main: Option<Instant>,
    render: Option<Instant>,
    stretch: Option<Instant>,
    spans: [Option<Instant>; SPANS],
    wait: Option<Instant>,
}

/// The latest 5 seconds' times, as the log puts them (for a capture).
pub fn latest() -> Option<String> {
    latest_line().lock().ok()?.clone()
}

fn latest_line() -> &'static Mutex<Option<String>> {
    static LATEST: OnceLock<Mutex<Option<String>>> = OnceLock::new();
    LATEST.get_or_init(Default::default)
}

fn totals() -> &'static Mutex<(Totals, Marks)> {
    static TOTALS: OnceLock<Mutex<(Totals, Marks)>> = OnceLock::new();
    TOTALS.get_or_init(Default::default)
}

fn main_started() {
    totals().lock().unwrap().1.main = Some(Instant::now());
}

fn main_ended() {
    let mut guard = totals().lock().unwrap();
    let (totals, marks) = &mut *guard;
    if let Some(started) = marks.main.take() {
        totals.main += started.elapsed();
        totals.main_frames += 1;
    }
    for sample in bevy::render::diagnostic::take_cpu_profile() {
        if sample.unit == "ms" || sample.unit == "count" {
            let entry = totals.profile.entry(sample.name).or_default();
            entry.0 += sample.value;
            entry.1 += 1;
        }
    }
    let since = *totals.since.get_or_insert_with(Instant::now);
    if since.elapsed().as_secs_f32() >= 5.0 {
        let frames = totals.render_frames.max(1) as f32;
        let per = |d: Duration| d.as_secs_f32() * 1000.0 / frames;
        let mut stretches = String::new();
        for (i, name) in STRETCH_NAMES.iter().enumerate() {
            if i > 0 {
                stretches.push_str(", ");
            }
            stretches.push_str(&format!("{name} {:.2}", per(totals.stretches[i])));
            for (s, inside) in SPAN_STRETCH.iter().enumerate() {
                if *inside == i {
                    stretches.push_str(&format!(" [{} {:.2}]", SPAN_NAMES[s], per(totals.spans[s])));
                }
            }
        }
        let line = format!(
            "cpu: main {:.2} ms, render {:.2} ms ({stretches}; and {:.2} ms waiting for the window), {:.0} frames/s",
            totals.main.as_secs_f32() * 1000.0 / totals.main_frames.max(1) as f32,
            per(totals.render),
            per(totals.window_wait),
            totals.render_frames as f32 / since.elapsed().as_secs_f32()
        );
        // (a scope's ms a frame, and its count a frame when not one)
        let scopes: Vec<String> = totals
            .profile
            .iter()
            .map(|(name, (ms, n))| {
                let per_frame = *n as f32 / frames;
                if (per_frame - 1.0).abs() < 0.05 {
                    format!("{name} {:.2}", *ms as f32 / frames)
                } else {
                    format!("{name} {:.2} ({per_frame:.1}/frame)", *ms as f32 / frames)
                }
            })
            .collect();
        let scopes = (!scopes.is_empty()).then(|| format!("cpu scopes: {}", scopes.join(", ")));
        if std::env::var_os("HALO_RT_GPU_LOG").is_some() {
            info!("{line}");
            if let Some(scopes) = &scopes {
                info!("{scopes}");
            }
        }
        if let Ok(mut latest) = latest_line().lock() {
            // (a capture's `.txt` gets the scopes too, when they are on)
            *latest = Some(match scopes {
                Some(scopes) => format!("{line}\n{scopes}"),
                None => line,
            });
        }
        *totals = Totals::default();
    }
}

fn render_started() {
    let now = Instant::now();
    let mut guard = totals().lock().unwrap();
    guard.1.render = Some(now);
    guard.1.stretch = Some(now);
}

// The stretch since the last mark, into stretch I.
fn stretch_ended<const I: usize>() {
    let now = Instant::now();
    let mut guard = totals().lock().unwrap();
    let (totals, marks) = &mut *guard;
    if let Some(started) = marks.stretch.replace(now) {
        totals.stretches[I] += now.saturating_duration_since(started);
    }
}

fn span_started<const S: usize>() {
    totals().lock().unwrap().1.spans[S] = Some(Instant::now());
}

fn span_ended<const S: usize>() {
    let mut guard = totals().lock().unwrap();
    let (totals, marks) = &mut *guard;
    if let Some(started) = marks.spans[S].take() {
        totals.spans[S] += started.elapsed();
    }
}

fn window_wait_started() {
    totals().lock().unwrap().1.wait = Some(Instant::now());
}

fn window_wait_ended() {
    let mut guard = totals().lock().unwrap();
    let (totals, marks) = &mut *guard;
    if let Some(started) = marks.wait.take() {
        let waited = started.elapsed();
        totals.window_wait += waited;
        // (the render thread's own time leaves out the wait)
        if let Some(render) = marks.render.as_mut() {
            *render += waited;
        }
        if let Some(stretch) = marks.stretch.as_mut() {
            *stretch += waited;
        }
    }
}

fn render_ended() {
    let mut guard = totals().lock().unwrap();
    let (totals, marks) = &mut *guard;
    if let Some(started) = marks.render.take() {
        totals.render += started.elapsed();
        totals.render_frames += 1;
    }
}
