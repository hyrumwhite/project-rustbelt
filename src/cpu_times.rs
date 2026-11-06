//! The CPU's side of a frame: the main schedule's time, and the render
//! thread's without its wait for a window texture (which waits on the GPU
//! and the display), split by its schedule's stretches (views: assets,
//! meshes, views and pipelines; prepare: the queue and the prepare sets,
//! where the scene's acceleration structures and uploads are; graph: the
//! render graph's encoding and submit; cleanup), each 5 seconds; logged
//! beside the GPU's pass times (`HALO_RT_GPU_LOG=1`), and the latest written
//! with each F9 capture.

use bevy::{
    prelude::*,
    render::{view::prepare_windows, Render, RenderApp, RenderSystems},
};
use std::{
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
                window_wait_started.in_set(RenderSystems::PrepareViews).before(prepare_windows),
                window_wait_ended.in_set(RenderSystems::PrepareViews).after(prepare_windows),
                views_ended.after(RenderSystems::PrepareViews).before(RenderSystems::Queue),
                prepare_ended.after(RenderSystems::PrepareBindGroups).before(RenderSystems::Render),
                graph_ended.after(RenderSystems::Render).before(RenderSystems::Cleanup),
                render_ended.in_set(RenderSystems::PostCleanup),
            ),
        );
    }
}

#[derive(Default)]
struct Totals {
    main: Duration,
    main_frames: u32,
    render: Duration,
    views: Duration,
    prepare: Duration,
    graph: Duration,
    window_wait: Duration,
    render_frames: u32,
    since: Option<Instant>,
}

#[derive(Default)]
struct Marks {
    main: Option<Instant>,
    render: Option<Instant>,
    phase: Option<Instant>,
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
    let since = *totals.since.get_or_insert_with(Instant::now);
    if since.elapsed().as_secs_f32() >= 5.0 {
        let per = |d: Duration, n: u32| d.as_secs_f32() * 1000.0 / n.max(1) as f32;
        let line = format!(
            "cpu: main {:.2} ms, render {:.2} ms (views {:.2}, prepare {:.2}, graph {:.2}; and {:.2} ms waiting for the window), {:.0} frames/s",
            per(totals.main, totals.main_frames),
            per(totals.render, totals.render_frames),
            per(totals.views, totals.render_frames),
            per(totals.prepare, totals.render_frames),
            per(totals.graph, totals.render_frames),
            per(totals.window_wait, totals.render_frames),
            totals.render_frames as f32 / since.elapsed().as_secs_f32()
        );
        if std::env::var_os("HALO_RT_GPU_LOG").is_some() {
            info!("{line}");
        }
        if let Ok(mut latest) = latest_line().lock() {
            *latest = Some(line);
        }
        *totals = Totals::default();
    }
}

fn render_started() {
    let now = Instant::now();
    let mut guard = totals().lock().unwrap();
    guard.1.render = Some(now);
    guard.1.phase = Some(now);
}

// The stretch since the last mark, into one of the totals.
fn phase_ended(into: fn(&mut Totals) -> &mut Duration) {
    let now = Instant::now();
    let mut guard = totals().lock().unwrap();
    let (totals, marks) = &mut *guard;
    if let Some(started) = marks.phase.replace(now) {
        *into(totals) += now.saturating_duration_since(started);
    }
}

fn views_ended() {
    phase_ended(|t| &mut t.views);
}

fn prepare_ended() {
    phase_ended(|t| &mut t.prepare);
}

fn graph_ended() {
    phase_ended(|t| &mut t.graph);
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
        if let Some(phase) = marks.phase.as_mut() {
            *phase += waited;
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
