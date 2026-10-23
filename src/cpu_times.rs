//! (`HALO_RT_GPU_LOG=1`) The CPU's side of a frame, logged beside the GPU's
//! pass times: the main schedule's time, and the render thread's without
//! its wait for a window texture (which waits on the GPU and the display).

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
    window_wait: Duration,
    render_frames: u32,
    since: Option<Instant>,
}

#[derive(Default)]
struct Marks {
    main: Option<Instant>,
    render: Option<Instant>,
    wait: Option<Instant>,
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
        info!(
            "cpu: main {:.2} ms, render {:.2} ms (and {:.2} ms waiting for the window), {:.0} frames/s",
            per(totals.main, totals.main_frames),
            per(totals.render, totals.render_frames),
            per(totals.window_wait, totals.render_frames),
            totals.render_frames as f32 / since.elapsed().as_secs_f32()
        );
        *totals = Totals::default();
    }
}

fn render_started() {
    totals().lock().unwrap().1.render = Some(Instant::now());
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
