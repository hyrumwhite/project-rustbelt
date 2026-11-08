//! Counters of the command encoding and submission work, for the host's CPU
//! profile: the commands are recorded into the backend's command buffer when
//! the encoder is finished (`encode_commands`), every dispatch of a compute
//! pass merges every resource of every bound bind group into its usage scope
//! (`flush_bindings`), and the submit checks every resource of every bind
//! group the commands used. With a bindless scene bind group of thousands of
//! resources, a frame's CPU cost is in these, and the host (a `take` a frame)
//! can see which.

use core::sync::atomic::{AtomicU64, Ordering::Relaxed};

pub struct Counter(AtomicU64);

impl Counter {
    const fn new() -> Self {
        Self(AtomicU64::new(0))
    }
    #[inline]
    pub fn inc(&self) {
        self.0.fetch_add(1, Relaxed);
    }
    #[inline]
    pub fn add(&self, n: u64) {
        self.0.fetch_add(n, Relaxed);
    }
    fn take(&self) -> u64 {
        self.0.swap(0, Relaxed)
    }
}

/// Draws encoded.
pub static DRAWS: Counter = Counter::new();
/// Dispatches encoded.
pub static DISPATCHES: Counter = Counter::new();
/// `set_bind_group`s encoded (render and compute passes).
pub static SET_BIND_GROUPS: Counter = Counter::new();
/// Bind group resources (buffers and texture views) merged into usage scopes
/// while encoding: a render pass's at each `set_bind_group`, a compute pass's
/// at each dispatch, every bound group's.
pub static BIND_GROUP_ENTRIES: Counter = Counter::new();
/// A value set, not summed: the last written is reported.
pub struct Gauge(AtomicU64);

impl Gauge {
    const fn new() -> Self {
        Self(AtomicU64::new(0))
    }
    #[inline]
    pub fn set(&self, value: u64) {
        self.0.store(value, Relaxed);
    }
    fn get(&self) -> u64 {
        self.0.load(Relaxed)
    }
}

/// The device's buffer tracker indices: the high-water mark (what every
/// pass's tables are sized to) and those in use.
pub static TRACKER_BUFFERS_PEAK: Gauge = Gauge::new();
pub static TRACKER_BUFFERS_LIVE: Gauge = Gauge::new();
/// The texture tracker indices, likewise.
pub static TRACKER_TEXTURES_PEAK: Gauge = Gauge::new();
pub static TRACKER_TEXTURES_LIVE: Gauge = Gauge::new();

/// Bind groups checked at submit.
pub static SUBMIT_BIND_GROUPS: Counter = Counter::new();
/// Their resources checked at submit.
pub static SUBMIT_BIND_GROUP_ENTRIES: Counter = Counter::new();

#[derive(Clone, Copy)]
pub enum Kind {
    /// A render pass encoded.
    EncodeRender,
    /// A compute pass encoded.
    EncodeCompute,
    /// An acceleration structure build encoded.
    EncodeAs,
    /// Any other command encoded.
    EncodeOther,
    /// A compute dispatch's bindings flushed (within `EncodeCompute`).
    ComputeFlush,
    /// A submit's resource checks (buffers, textures, bind groups).
    SubmitCheck,
    /// A submit's acceleration structure validation.
    SubmitAs,
    /// A submit's device tracker merge and barriers.
    SubmitTrackers,
    /// The backend's submit.
    SubmitHal,
    /// The submit's maintenance.
    SubmitCleanup,
    /// A render pass's setup, before its commands (within `EncodeRender`).
    RenderPassSetup,
    /// A render pass's draws (within `EncodeRender`).
    RenderDraw,
    /// A compute pass's setup, before its commands (within `EncodeCompute`).
    ComputePassSetup,
}

const KINDS: usize = 13;

const NAMES: [&str; KINDS] = [
    "wgpu.encode_render",
    "wgpu.encode_compute",
    "wgpu.encode_as",
    "wgpu.encode_other",
    "wgpu.compute_flush",
    "wgpu.submit_check",
    "wgpu.submit_as",
    "wgpu.submit_trackers",
    "wgpu.submit_hal",
    "wgpu.submit_cleanup",
    "wgpu.render_pass_setup",
    "wgpu.render_draw",
    "wgpu.compute_pass_setup",
];

static NANOS: [Counter; KINDS] = [const { Counter::new() }; KINDS];

/// Render passes encoded.
pub static RENDER_PASSES: Counter = Counter::new();
/// Compute passes encoded.
pub static COMPUTE_PASSES: Counter = Counter::new();
/// Queue submits.
pub static SUBMITS: Counter = Counter::new();
/// Command buffers submitted.
pub static SUBMITTED_BUFFERS: Counter = Counter::new();

#[cfg(feature = "std")]
pub type Started = std::time::Instant;
#[cfg(not(feature = "std"))]
pub type Started = ();

#[inline]
pub fn start() -> Started {
    #[cfg(feature = "std")]
    {
        std::time::Instant::now()
    }
}

#[inline]
pub fn add(kind: Kind, started: Started) {
    #[cfg(feature = "std")]
    {
        NANOS[kind as usize].add(started.elapsed().as_nanos() as u64);
    }
    #[cfg(not(feature = "std"))]
    {
        let _ = (kind, started);
    }
}

/// The counters since the last call, zeroed: a name, the value, its unit
/// ("ms" or "count").
pub fn take() -> alloc::vec::Vec<(&'static str, f64, &'static str)> {
    let mut out = alloc::vec::Vec::with_capacity(KINDS + 6);
    for (name, counter) in NAMES.iter().zip(NANOS.iter()) {
        out.push((*name, counter.take() as f64 / 1.0e6, "ms"));
    }
    for (name, counter) in [
        ("wgpu.draws", &DRAWS),
        ("wgpu.dispatches", &DISPATCHES),
        ("wgpu.set_bind_groups", &SET_BIND_GROUPS),
        ("wgpu.bind_group_entries", &BIND_GROUP_ENTRIES),
        ("wgpu.submit_bind_groups", &SUBMIT_BIND_GROUPS),
        ("wgpu.submit_bind_group_entries", &SUBMIT_BIND_GROUP_ENTRIES),
        ("wgpu.render_passes", &RENDER_PASSES),
        ("wgpu.compute_passes", &COMPUTE_PASSES),
        ("wgpu.submits", &SUBMITS),
        ("wgpu.submitted_buffers", &SUBMITTED_BUFFERS),
    ] {
        out.push((name, counter.take() as f64, "count"));
    }
    for (name, gauge) in [
        ("wgpu.tracker_buffers_peak", &TRACKER_BUFFERS_PEAK),
        ("wgpu.tracker_buffers_live", &TRACKER_BUFFERS_LIVE),
        ("wgpu.tracker_textures_peak", &TRACKER_TEXTURES_PEAK),
        ("wgpu.tracker_textures_live", &TRACKER_TEXTURES_LIVE),
    ] {
        out.push((name, gauge.get() as f64, "count"));
    }
    out
}
