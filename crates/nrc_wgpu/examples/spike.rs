//! Synthetic-load spike: feeds NRC a scene with a known answer, checks that
//! the network learns it, and times QueryAndTrain under realistic load.
//!
//! `cargo run -p nrc_wgpu --release --example spike`
//!
//! The scene (spike.wgsl): outgoing irradiance c(p) = 0.2 + 0.6 * p.x / 10,
//! equal in r, g, b, in a 0..10 box; albedo 1, specular 0, roughness 1. The
//! training pass writes 197x111 paths of V vertices at random positions with
//! radiance c(p_i) and throughput 0 (so every vertex's target is c(p_i)); the
//! query pass writes one query per pixel at 1280x720 at a random position.
//! Each frame the library's answers for 4,096 sampled queries are read back,
//! unpacked with `radiance_unpack_multiplier`, and compared with c(p).
//!
//! Per frame, in one submit:
//! `[reset, BeginFrame, ts0] [training fill] [ts1] [query fill, counter copy]
//!  [ts2, QueryAndTrain, ts3] [gather, copies]`, then EndFrame. The raw
//! (NRC and timestamp) encoders hold only raw commands.
//!
//! Optional arguments: run names to run only those (full, queries, training,
//! v1, v2, v8, switch, atomic, atomic1, device1, static, sanity), e.g.
//! `-- full sanity`. Environment knobs: SPIKE_EXIT_FRACTION (training paths
//! that exited the scene, default 0), SPIKE_MAX_RADIANCE
//! (max_expected_average_radiance_value), SPIKE_PRIMARY
//! (proportion_primary_segments_to_train_on), SPIKE_REPORT_LOSS=1 (fails on
//! this SDK, see main).
//!
//! Results and findings: docs/nrc-integration.md, "Synthetic-load spike".

use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Instant,
};

use ash::vk;
use bytemuck::{Pod, Zeroable};
use nrc_wgpu::{BufferIdx, ContextSettings, FrameSettings, Nrc, NrcContext, NrcInitSettings, compute_ideal_training_dimensions};
use wgpu::hal::api::Vulkan;

// ---------------------------------------------------------------------------
// Logging: count Vulkan validation, wgpu and NRC errors (as tests/smoke.rs)
// ---------------------------------------------------------------------------

struct SpikeLogger {
    errors: AtomicUsize,
    messages: Mutex<Vec<String>>,
}

impl log::Log for SpikeLogger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() <= log::Level::Warn || (metadata.target().starts_with("nrc") && metadata.level() <= log::Level::Info)
    }

    fn log(&self, record: &log::Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let line = format!("[{} {}] {}", record.level(), record.target(), record.args());
        eprintln!("{line}");
        if record.level() == log::Level::Error && (record.target().starts_with("wgpu") || record.target().starts_with("nrc")) {
            self.errors.fetch_add(1, Ordering::SeqCst);
            self.messages.lock().unwrap().push(line);
        }
    }

    fn flush(&self) {}
}

static LOGGER: SpikeLogger = SpikeLogger { errors: AtomicUsize::new(0), messages: Mutex::new(Vec::new()) };

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

const FRAME_DIMENSIONS: [u32; 2] = [1280, 720];
const FRAMES: usize = 240;
const SAMPLE_COUNT: u32 = 4096;
const CHECKPOINTS: [usize; 5] = [10, 30, 60, 120, 240];
const TIMING_FROM: usize = 40;
const SWITCH_AT: usize = 120;
const RECOVERED_BELOW: f64 = 0.02;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct SpikeParams {
    frame: u32,
    vertices: u32,
    exit_fraction: f32,
    function: u32,
    sample_from_training: u32,
    sample_stride: u32,
    sample_count: u32,
    static_queries: u32,
}

#[derive(Clone, Copy)]
struct Phase {
    frames: usize,
    vertices: u32,
    training_pass: bool,
    query_pass: bool,
    /// The query pass takes its indices with one atomic per workgroup
    /// (as the HLSL does per wave) instead of one per thread.
    grouped_counter: bool,
    /// The fill passes count on a device-local counter of their own, copied
    /// into NRC's Counter buffer before QueryAndTrain. NRC's Counter is in
    /// host-visible memory (the library asks for a readback heap), where every
    /// shader atomic is a round trip over PCIe.
    device_counter: bool,
    train_the_cache: bool,
}

#[derive(Clone)]
struct Run {
    name: &'static str,
    label: &'static str,
    /// Trained full-load frames before the measured ones (not reported).
    warmup: Option<Phase>,
    measured: Phase,
    exit_fraction: f32,
    /// Measure the error at the training paths' self-training queries
    /// instead of the pixel queries.
    sample_from_training: bool,
    switch_function_at: Option<usize>,
    /// Pixel queries at the same points every frame.
    static_queries: bool,
    loss_every: Option<usize>,
}

fn runs(exit_fraction: f32, report_loss: bool) -> Vec<Run> {
    let full = |vertices| Phase {
        frames: FRAMES,
        vertices,
        training_pass: true,
        query_pass: true,
        grouped_counter: true,
        device_counter: true,
        train_the_cache: true,
    };
    let base = Run {
        name: "full",
        label: "1. full load, 3 vertices",
        warmup: None,
        measured: full(3),
        exit_fraction,
        sample_from_training: false,
        switch_function_at: None,
        static_queries: false,
        // (report_loss = true makes QueryAndTrain fail on Vulkan, see main)
        loss_every: report_loss.then_some(30),
    };
    vec![
        base.clone(),
        Run {
            name: "queries",
            label: "2. queries only (no training, after 120 trained frames)",
            warmup: Some(Phase { frames: 120, ..full(3) }),
            measured: Phase { training_pass: false, train_the_cache: false, ..full(3) },
            loss_every: None,
            ..base.clone()
        },
        Run {
            name: "training",
            label: "3. training only (0 pixel queries), 3 vertices",
            measured: Phase { query_pass: false, ..full(3) },
            sample_from_training: true,
            loss_every: None,
            ..base.clone()
        },
        Run { name: "v1", label: "4a. full load, 1 vertex", measured: full(1), loss_every: None, ..base.clone() },
        Run { name: "v2", label: "4c. full load, 2 vertices", measured: full(2), loss_every: None, ..base.clone() },
        Run { name: "v8", label: "4b. full load, 8 vertices", measured: full(8), loss_every: None, ..base.clone() },
        Run {
            name: "switch",
            label: "5. full load, c(p) flipped at frame 120",
            switch_function_at: Some(SWITCH_AT),
            loss_every: None,
            ..base.clone()
        },
        Run {
            name: "atomic",
            label: "1b. as 1, atomics on NRC's Counter, one per workgroup",
            measured: Phase { device_counter: false, ..full(3) },
            loss_every: None,
            ..base.clone()
        },
        Run {
            name: "atomic1",
            label: "1c. as 1, atomics on NRC's Counter, one per thread",
            measured: Phase { device_counter: false, grouped_counter: false, ..full(3) },
            loss_every: None,
            ..base.clone()
        },
        Run {
            name: "device1",
            label: "1d. as 1, own device counter, one atomic per thread",
            measured: Phase { grouped_counter: false, ..full(3) },
            loss_every: None,
            ..base.clone()
        },
        Run {
            name: "static",
            label: "6. full load, queries at fixed points",
            static_queries: true,
            loss_every: None,
            ..base.clone()
        },
        Run {
            name: "sanity",
            label: "S. full load, error at the training positions",
            sample_from_training: true,
            loss_every: None,
            ..base
        },
    ]
}

// ---------------------------------------------------------------------------
// Results
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Default)]
struct FrameResult {
    train_fill_ms: f64,
    query_fill_ms: f64,
    query_and_train_ms: f64,
    queries: u32,
    /// NRC's TrainingRecords counter after QueryAndTrain (the library's own count).
    training_records: u32,
    mae: f64,
    p95: f64,
    mean_predicted: f64,
    mean_expected: f64,
    /// Least-squares fit predicted = slope * expected + intercept.
    slope: f64,
    intercept: f64,
    /// Samples that had a query (exited training paths have none).
    valid: usize,
    /// Mean |prediction - last frame's prediction| over the samples (static queries only).
    temporal_delta: f64,
}

struct RunResult {
    run: Run,
    frames: Vec<FrameResult>,
    multiplier: f32,
    losses: Vec<(usize, f32)>,
}

fn env_f32(name: &str) -> Option<f32> {
    std::env::var(name).ok().and_then(|v| v.parse().ok())
}

fn median(values: impl Iterator<Item = f64>) -> f64 {
    let mut v: Vec<f64> = values.collect();
    if v.is_empty() {
        return f64::NAN;
    }
    v.sort_by(f64::total_cmp);
    v[v.len() / 2]
}

fn expected(function: u32, x: f32) -> f64 {
    let t = x as f64 / 10.0;
    if function == 1 { 0.8 - 0.6 * t } else { 0.2 + 0.6 * t }
}

// ---------------------------------------------------------------------------
// GPU plumbing
// ---------------------------------------------------------------------------

struct Gpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
    vk_device: ash::Device,
    timestamp_period: f64,
    query_pool: vk::QueryPool,
    layout: wgpu::BindGroupLayout,
    train_pipeline: wgpu::ComputePipeline,
    query_pipeline: wgpu::ComputePipeline,
    query_grouped_pipeline: wgpu::ComputePipeline,
    gather_pipeline: wgpu::ComputePipeline,
    constants_buffer: wgpu::Buffer,
    params_buffer: wgpu::Buffer,
    samples_buffer: wgpu::Buffer,
    /// The fill passes' own Queries/TrainingRecords counters (device-local).
    device_counter: wgpu::Buffer,
    results_buffer: wgpu::Buffer,
    /// MAP_READ: results (SAMPLE_COUNT x 16 bytes), then NRC's two counters
    /// before QueryAndTrain and the same two after it.
    staging: wgpu::Buffer,
}

const RESULTS_BYTES: u64 = SAMPLE_COUNT as u64 * 16;
const COUNTER_OFFSET: u64 = RESULTS_BYTES;

impl Gpu {
    /// A raw-only encoder step: runs `f` on its Vulkan command buffer.
    fn raw(&self, encoder: &mut wgpu::CommandEncoder, f: impl FnOnce(&ash::Device, vk::CommandBuffer)) {
        // SAFETY: the encoder only ever holds raw commands; the pool is ours.
        unsafe {
            encoder.as_hal_mut::<Vulkan, _, _>(|hal| {
                let command_buffer = hal.expect("encoder not recording").raw_handle();
                f(&self.vk_device, command_buffer);
            })
        }
    }

    fn timestamp(&self, encoder: &mut wgpu::CommandEncoder, query: u32) {
        let pool = self.query_pool;
        self.raw(encoder, |device, command_buffer| {
            // SAFETY: a recording command buffer and our pool.
            unsafe { device.cmd_write_timestamp(command_buffer, vk::PipelineStageFlags::ALL_COMMANDS, pool, query) };
        });
    }
}

fn storage_entry(binding: u32, read_only: bool) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Storage { read_only },
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

fn uniform_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
        count: None,
    }
}

fn create_gpu() -> Gpu {
    let instance = nrc_wgpu::create_instance(&wgpu::InstanceDescriptor {
        backends: wgpu::Backends::VULKAN,
        flags: wgpu::InstanceFlags::VALIDATION | wgpu::InstanceFlags::DEBUG,
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    })
    .expect("creating the instance");
    let adapters = futures_lite::future::block_on(instance.enumerate_adapters(wgpu::Backends::VULKAN));
    let adapter = adapters.into_iter().find(|a| a.get_info().vendor == 0x10de).expect("no NVIDIA Vulkan adapter");
    let info = adapter.get_info();
    println!("adapter: {} (driver {} {})", info.name, info.driver, info.driver_info);
    let required_features = adapter.features() & wgpu::Features::EXPERIMENTAL_RAY_QUERY;
    let (device, queue) = nrc_wgpu::request_device(
        &adapter,
        &wgpu::DeviceDescriptor {
            label: Some("nrc spike"),
            required_features,
            required_limits: adapter.limits(),
            // SAFETY: ray queries are experimental in wgpu; the game uses them the same way.
            experimental_features: unsafe { wgpu::ExperimentalFeatures::enabled() },
            ..Default::default()
        },
        None,
    )
    .expect("creating the device");
    device.on_uncaptured_error(Arc::new(|error| {
        eprintln!("wgpu error: {error}");
        LOGGER.errors.fetch_add(1, Ordering::SeqCst);
        LOGGER.messages.lock().unwrap().push(format!("wgpu: {error}"));
    }));

    let (vk_device, timestamp_period) = {
        // SAFETY: handles only, used while `device` lives.
        let hal = unsafe { device.as_hal::<Vulkan>() }.unwrap();
        let instance = hal.shared_instance().raw_instance();
        // SAFETY: a query on the device's physical device.
        let properties = unsafe { instance.get_physical_device_properties(hal.raw_physical_device()) };
        (hal.raw_device().clone(), properties.limits.timestamp_period as f64)
    };
    // SAFETY: a valid create info on our device; destroyed at the end.
    let query_pool = unsafe {
        vk_device.create_query_pool(&vk::QueryPoolCreateInfo::default().query_type(vk::QueryType::TIMESTAMP).query_count(4), None)
    }
    .unwrap();

    let source = format!("{}\n{}\n{}", nrc_records::WGSL, nrc_records::WGSL_BUFFERS, include_str!("spike.wgsl"));
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("nrc spike"),
        source: wgpu::ShaderSource::Wgsl(source.into()),
    });
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("nrc spike"),
        entries: &[
            uniform_entry(0),
            uniform_entry(1),
            storage_entry(2, false),
            storage_entry(3, false),
            storage_entry(4, false),
            storage_entry(5, false),
            storage_entry(6, false),
            storage_entry(7, true),
            storage_entry(8, false),
            storage_entry(9, false),
        ],
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("nrc spike"),
        bind_group_layouts: &[Some(&layout)],
        immediate_size: 0,
    });
    let pipeline = |entry: &str| {
        device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some(entry),
            layout: Some(&pipeline_layout),
            module: &module,
            entry_point: Some(entry),
            compilation_options: Default::default(),
            cache: None,
        })
    };
    let train_pipeline = pipeline("train_fill");
    let query_pipeline = pipeline("query_fill");
    let query_grouped_pipeline = pipeline("query_fill_grouped");
    let gather_pipeline = pipeline("gather");

    let buffer = |label: &str, size: u64, usage: wgpu::BufferUsages| {
        device.create_buffer(&wgpu::BufferDescriptor { label: Some(label), size, usage, mapped_at_creation: false })
    };
    let constants_buffer = buffer("nrc constants", 96, wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST);
    let params_buffer = buffer("spike params", 32, wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST);
    let samples_buffer = buffer("spike samples", RESULTS_BYTES, wgpu::BufferUsages::STORAGE);
    let device_counter = buffer(
        "spike counters",
        8,
        wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::COPY_DST,
    );
    let results_buffer = buffer("spike results", RESULTS_BYTES, wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC);
    let staging = buffer("spike staging", RESULTS_BYTES + 16, wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST);

    Gpu {
        device,
        queue,
        vk_device,
        timestamp_period,
        query_pool,
        layout,
        train_pipeline,
        query_pipeline,
        query_grouped_pipeline,
        gather_pipeline,
        constants_buffer,
        params_buffer,
        samples_buffer,
        device_counter,
        results_buffer,
        staging,
    }
}

// ---------------------------------------------------------------------------
// One run
// ---------------------------------------------------------------------------

fn run(gpu: &Gpu, nrc: &Arc<Nrc>, settings: &ContextSettings, run: &Run) -> RunResult {
    // A fresh context per run, so every run starts from an untrained cache.
    let mut context = NrcContext::new(nrc, &gpu.device).expect("Context::Create");
    context.configure(settings).expect("Context::Configure");
    let buffers = context.buffers().unwrap();
    // (clones: the context is borrowed mutably every frame)
    let nrc_buffer = |index: BufferIdx| buffers.get(index).unwrap_or_else(|| panic!("no {index:?} buffer")).buffer.clone();
    let query_radiance_capacity = buffers.get(BufferIdx::QueryRadiance).unwrap().allocation.element_count as u32;
    let make_bind_group = |counters: &wgpu::Buffer| gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("nrc spike"),
        layout: &gpu.layout,
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: gpu.constants_buffer.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 1, resource: gpu.params_buffer.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 2, resource: nrc_buffer(BufferIdx::QueryPathInfo).as_entire_binding() },
            wgpu::BindGroupEntry { binding: 3, resource: nrc_buffer(BufferIdx::TrainingPathInfo).as_entire_binding() },
            wgpu::BindGroupEntry { binding: 4, resource: nrc_buffer(BufferIdx::TrainingPathVertices).as_entire_binding() },
            wgpu::BindGroupEntry { binding: 5, resource: nrc_buffer(BufferIdx::QueryRadianceParams).as_entire_binding() },
            wgpu::BindGroupEntry { binding: 6, resource: counters.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 7, resource: nrc_buffer(BufferIdx::QueryRadiance).as_entire_binding() },
            wgpu::BindGroupEntry { binding: 8, resource: gpu.samples_buffer.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 9, resource: gpu.results_buffer.as_entire_binding() },
        ],
    });
    let counter_buffer = nrc_buffer(BufferIdx::Counter);
    let nrc_counter_bind_group = make_bind_group(&counter_buffer);
    let device_counter_bind_group = make_bind_group(&gpu.device_counter);
    let training_dimensions = settings.training_dimensions;
    let training_paths = training_dimensions[0] * training_dimensions[1];
    let pixels = FRAME_DIMENSIONS[0] * FRAME_DIMENSIONS[1];

    let phases: Vec<(Phase, bool)> = run.warmup.iter().map(|p| (*p, false)).chain([(run.measured, true)]).collect();
    let mut frames = Vec::new();
    let mut losses = Vec::new();
    let mut multiplier = f32::NAN;
    let mut previous: Vec<f64> = Vec::new();
    let mut global_frame = 0u32;
    for (phase, measured) in phases {
        for frame in 0..phase.frames {
            let function = match run.switch_function_at {
                Some(at) if measured && frame >= at => 1,
                _ => 0,
            };
            let report_loss = measured && run.loss_every.is_some_and(|every| (frame + 1) % every == 0);
            let defaults = FrameSettings::default();
            let frame_settings = FrameSettings {
                train_the_cache: phase.train_the_cache,
                max_expected_average_radiance_value: env_f32("SPIKE_MAX_RADIANCE")
                    .unwrap_or(defaults.max_expected_average_radiance_value),
                proportion_primary_segments_to_train_on: env_f32("SPIKE_PRIMARY")
                    .unwrap_or(defaults.proportion_primary_segments_to_train_on),
                ..defaults
            };
            let (sample_from_training, sample_stride) = if run.sample_from_training || !phase.query_pass {
                (1, training_paths / SAMPLE_COUNT)
            } else {
                (0, pixels / SAMPLE_COUNT)
            };
            let params = SpikeParams {
                frame: global_frame,
                vertices: phase.vertices,
                exit_fraction: run.exit_fraction,
                function,
                sample_from_training,
                sample_stride,
                sample_count: SAMPLE_COUNT,
                static_queries: run.static_queries as u32,
            };
            gpu.queue.write_buffer(&gpu.params_buffer, 0, bytemuck::bytes_of(&params));

            // [reset, BeginFrame, ts0]
            let mut begin = gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("nrc begin") });
            let pool = gpu.query_pool;
            gpu.raw(&mut begin, |device, command_buffer| {
                // SAFETY: a recording command buffer and our pool.
                unsafe { device.cmd_reset_query_pool(command_buffer, pool, 0, 4) };
            });
            context.begin_frame(&mut begin, &frame_settings).expect("BeginFrame");
            gpu.timestamp(&mut begin, 0);
            // (after BeginFrame: the multiplier may follow the frame settings)
            let constants = context.shader_constants().expect("PopulateShaderConstants");
            if measured && frame == 0 {
                multiplier = constants.radiance_unpack_multiplier;
                println!("  constants: {constants:?}");
            } else if measured && constants.radiance_unpack_multiplier != multiplier {
                println!("  frame {frame}: radiance_unpack_multiplier changed to {}", constants.radiance_unpack_multiplier);
                multiplier = constants.radiance_unpack_multiplier;
            }
            gpu.queue.write_buffer(&gpu.constants_buffer, 0, bytemuck::bytes_of(&constants));

            // [training fill]
            let bind_group = if phase.device_counter { &device_counter_bind_group } else { &nrc_counter_bind_group };
            let mut train = gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("train fill") });
            if phase.device_counter {
                // (what BeginFrame does to NRC's counters)
                train.clear_buffer(&gpu.device_counter, 0, None);
            }
            if phase.training_pass {
                let mut pass = train.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("train fill"), timestamp_writes: None });
                pass.set_pipeline(&gpu.train_pipeline);
                pass.set_bind_group(0, bind_group, &[]);
                pass.dispatch_workgroups(training_dimensions[0].div_ceil(8), training_dimensions[1].div_ceil(8), 1);
            }
            // [ts1]
            let mut mid = gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("ts1") });
            gpu.timestamp(&mut mid, 1);
            // [query fill, counter copy]
            let mut query = gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("query fill") });
            if phase.query_pass {
                let mut pass = query.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("query fill"), timestamp_writes: None });
                pass.set_pipeline(if phase.grouped_counter { &gpu.query_grouped_pipeline } else { &gpu.query_pipeline });
                pass.set_bind_group(0, bind_group, &[]);
                pass.dispatch_workgroups(FRAME_DIMENSIONS[0].div_ceil(8), FRAME_DIMENSIONS[1].div_ceil(8), 1);
            }
            if phase.device_counter {
                query.copy_buffer_to_buffer(&gpu.device_counter, 0, &counter_buffer, 0, 8);
            }
            query.copy_buffer_to_buffer(&counter_buffer, 0, &gpu.staging, COUNTER_OFFSET, 8);
            // [ts2, QueryAndTrain, ts3]
            let mut nrc_encoder = gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("nrc query and train") });
            gpu.timestamp(&mut nrc_encoder, 2);
            context.query_and_train(&mut nrc_encoder, report_loss).expect("QueryAndTrain");
            gpu.timestamp(&mut nrc_encoder, 3);
            // [gather, copies]
            let mut post = gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("gather") });
            {
                let mut pass = post.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("gather"), timestamp_writes: None });
                pass.set_pipeline(&gpu.gather_pipeline);
                pass.set_bind_group(0, bind_group, &[]);
                pass.dispatch_workgroups(SAMPLE_COUNT.div_ceil(64), 1, 1);
            }
            post.copy_buffer_to_buffer(&gpu.results_buffer, 0, &gpu.staging, 0, RESULTS_BYTES);
            post.copy_buffer_to_buffer(&counter_buffer, 0, &gpu.staging, COUNTER_OFFSET + 8, 8);

            gpu.queue.submit([begin.finish(), train.finish(), mid.finish(), query.finish(), nrc_encoder.finish(), post.finish()]);
            context.end_frame(&gpu.queue).expect("EndFrame");
            gpu.staging.slice(..).map_async(wgpu::MapMode::Read, |r| r.expect("mapping the staging buffer"));
            gpu.device.poll(wgpu::PollType::wait_indefinitely()).expect("waiting for the GPU");

            // (a rejected submit never writes the timestamps: the wait below would hang)
            let errors = LOGGER.errors.load(Ordering::SeqCst);
            assert_eq!(errors, 0, "errors were logged:
{}", LOGGER.messages.lock().unwrap().join("
"));
            let mut stamps = [0u64; 4];
            // SAFETY: all four queries were written by the finished submission.
            unsafe {
                gpu.vk_device.get_query_pool_results(gpu.query_pool, 0, &mut stamps, vk::QueryResultFlags::TYPE_64 | vk::QueryResultFlags::WAIT)
            }
            .unwrap();
            let ms = |a: usize, b: usize| stamps[b].wrapping_sub(stamps[a]) as f64 * gpu.timestamp_period / 1e6;

            let mut result = FrameResult {
                train_fill_ms: ms(0, 1),
                query_fill_ms: ms(1, 2),
                query_and_train_ms: ms(2, 3),
                ..Default::default()
            };
            {
                let data = gpu.staging.slice(..).get_mapped_range().expect("mapped range");
                let floats: &[f32] = bytemuck::cast_slice(&data[..RESULTS_BYTES as usize]);
                let counters: &[u32] = bytemuck::cast_slice(&data[RESULTS_BYTES as usize..]);
                result.queries = counters[0];
                result.training_records = counters[3];
                let multiplier = constants.radiance_unpack_multiplier as f64;
                let mut errors = Vec::with_capacity(SAMPLE_COUNT as usize);
                let mut predictions = Vec::with_capacity(SAMPLE_COUNT as usize);
                let (mut sp, mut se, mut spe, mut see) = (0.0, 0.0, 0.0, 0.0);
                for sample in floats.chunks_exact(4) {
                    if sample[0] < 0.0 {
                        continue; // no query (an exited training path)
                    }
                    let want = expected(function, sample[3]);
                    let got = [sample[0], sample[1], sample[2]].map(|c| c as f64 * multiplier);
                    let mean = (got[0] + got[1] + got[2]) / 3.0;
                    predictions.push(mean);
                    errors.push(got.iter().map(|g| (g - want).abs()).sum::<f64>() / 3.0);
                    sp += mean;
                    se += want;
                    spe += mean * want;
                    see += want * want;
                }
                let n = errors.len() as f64;
                result.valid = errors.len();
                if !errors.is_empty() {
                    result.mae = errors.iter().sum::<f64>() / n;
                    errors.sort_by(f64::total_cmp);
                    result.p95 = errors[((errors.len() as f64 * 0.95) as usize).min(errors.len() - 1)];
                    result.mean_predicted = sp / n;
                    result.mean_expected = se / n;
                    let cov = spe / n - (sp / n) * (se / n);
                    let var = see / n - (se / n) * (se / n);
                    result.slope = cov / var;
                    result.intercept = sp / n - result.slope * se / n;
                }
                result.temporal_delta = if run.static_queries && previous.len() == predictions.len() {
                    previous.iter().zip(&predictions).map(|(a, b)| (a - b).abs()).sum::<f64>() / n
                } else {
                    f64::NAN
                };
                previous = predictions;
            }
            gpu.staging.unmap();

            let expected_queries = if phase.query_pass { pixels } else { 0 }
                + if phase.training_pass && phase.vertices > 0 {
                    (training_paths as f32 * (1.0 - run.exit_fraction)) as u32
                } else {
                    0
                };
            assert!(
                result.queries <= query_radiance_capacity,
                "{} queries, more than the QueryRadiance buffer's {query_radiance_capacity}",
                result.queries
            );
            if run.exit_fraction == 0.0 {
                assert_eq!(result.queries, expected_queries, "Queries counter");
            }
            if report_loss {
                losses.push((frame + 1, context.training_loss()));
            }
            if measured {
                if (frame + 1) % 30 == 0 || frame == 0 {
                    println!(
                        "  frame {:3}: train fill {:.3} ms, query fill {:.3} ms, QueryAndTrain {:.3} ms; {} queries, {} training records; MAE {:.4}, p95 {:.4}, mean {:.4} (want {:.4}), fit {:.3}c{:+.3}{}",
                        frame + 1,
                        result.train_fill_ms,
                        result.query_fill_ms,
                        result.query_and_train_ms,
                        result.queries,
                        result.training_records,
                        result.mae,
                        result.p95,
                        result.mean_predicted,
                        result.mean_expected,
                        result.slope,
                        result.intercept,
                        if report_loss { format!("; loss {:.5}", context.training_loss()) } else { String::new() },
                    );
                }
                frames.push(result);
            }
            global_frame += 1;
        }
    }
    drop((nrc_counter_bind_group, device_counter_bind_group));
    drop(context);
    RunResult { run: run.clone(), frames, multiplier, losses }
}

// ---------------------------------------------------------------------------
// main
// ---------------------------------------------------------------------------

fn main() {
    log::set_logger(&LOGGER).unwrap();
    log::set_max_level(log::LevelFilter::Info);
    let selected: Vec<String> = std::env::args().skip(1).collect();
    let exit_fraction: f32 = std::env::var("SPIKE_EXIT_FRACTION").ok().and_then(|v| v.parse().ok()).unwrap_or(0.0);
    // QueryAndTrain with report_loss = true returns InternalError on this SDK
    // (v0.16, Vulkan): tiny-cuda-nn's reduce_sum does a cudaMemcpyAsync to the
    // host inside the recorded command buffer, which logs "operation not
    // supported". Off unless SPIKE_REPORT_LOSS=1 asks to try it again.
    let report_loss = std::env::var("SPIKE_REPORT_LOSS").is_ok_and(|v| v == "1");

    let bin = nrc_wgpu::sdk_bin_dir();
    nrc_wgpu::load_library(bin, true).expect("loading NRC_Vulkan.dll");
    let gpu = create_gpu();
    let nrc = Nrc::initialize(&NrcInitSettings::new(bin)).expect("nrc::vulkan::Initialize");
    let training_dimensions = compute_ideal_training_dimensions(FRAME_DIMENSIONS, 0, 0.0).unwrap();
    println!("training dimensions: {training_dimensions:?}, exit fraction {exit_fraction}");
    let settings = ContextSettings {
        learn_irradiance: true,
        include_direct_lighting: true,
        scene_bounds_min: [0.0; 3],
        scene_bounds_max: [10.0; 3],
        frame_dimensions: FRAME_DIMENSIONS,
        training_dimensions,
        samples_per_pixel: 1,
        max_path_vertices: 8,
        ..Default::default()
    };

    let started = Instant::now();
    let mut results = Vec::new();
    for r in runs(exit_fraction, report_loss) {
        if !selected.is_empty() && !selected.iter().any(|s| s == r.name) {
            continue;
        }
        println!("\n{}", r.label);
        results.push(run(&gpu, &nrc, &settings, &r));
    }
    println!("\n(all runs: {:.1} s)", started.elapsed().as_secs_f64());

    // Tables
    println!("\nGPU ms, median of frames {TIMING_FROM}..{FRAMES} (Vulkan timestamps):");
    println!("| run | queries/frame | training records/frame | training fill | query fill | QueryAndTrain | multiplier |");
    println!("|---|---|---|---|---|---|---|");
    for r in &results {
        let f = &r.frames[TIMING_FROM..];
        println!(
            "| {} | {} | {} | {:.3} | {:.3} | {:.3} | {} |",
            r.run.label,
            r.frames.last().unwrap().queries,
            r.frames.last().unwrap().training_records,
            median(f.iter().map(|x| x.train_fill_ms)),
            median(f.iter().map(|x| x.query_fill_ms)),
            median(f.iter().map(|x| x.query_and_train_ms)),
            r.multiplier,
        );
    }
    println!("\nConvergence, |unpacked radiance - c(p)| over the sampled queries (MAE / p95; fit = slope, intercept of predicted against c):");
    print!("| run |");
    for c in CHECKPOINTS {
        print!(" frame {c} |");
    }
    println!(" fit at {FRAMES} | bias range 120..{FRAMES} |");
    println!("|---|{}---|---|", "---|".repeat(CHECKPOINTS.len()));
    for r in &results {
        print!("| {} |", r.run.label);
        for c in CHECKPOINTS {
            let f = &r.frames[c - 1];
            print!(" {:.4} / {:.4} |", f.mae, f.p95);
        }
        let last = r.frames.last().unwrap();
        let bias = r.frames[120..].iter().map(|f| f.mean_predicted - f.mean_expected);
        let (lo, hi) = bias.fold((f64::MAX, f64::MIN), |(lo, hi), b| (lo.min(b), hi.max(b)));
        println!(" {:.3}c{:+.3} | {lo:+.4} .. {hi:+.4} |", last.slope, last.intercept);
    }
    for r in &results {
        if r.run.static_queries {
            let d = |a: usize, b: usize| median(r.frames[a..b].iter().map(|f| f.temporal_delta));
            println!(
                "
frame-to-frame change at fixed points ({}): median mean |delta| frames 2..30 {:.5}, 30..120 {:.5}, 120..{FRAMES} {:.5}",
                r.run.name,
                d(1, 30),
                d(30, 120),
                d(120, FRAMES)
            );
        }
        if !r.losses.is_empty() {
            println!(
                "\ntraining loss ({}): {}",
                r.run.name,
                r.losses.iter().map(|(f, l)| format!("frame {f}: {l:.5}")).collect::<Vec<_>>().join(", ")
            );
        }
        if let Some(at) = r.run.switch_function_at {
            let before = &r.frames[at - 1];
            let recovered = r.frames[at..].iter().position(|f| f.mae < RECOVERED_BELOW);
            println!(
                "\nswitch at frame {at}: MAE before {:.4}, first frame after {:.4}; back under {RECOVERED_BELOW} after {}",
                before.mae,
                r.frames[at].mae,
                match recovered {
                    Some(n) => format!("{n} frames"),
                    None => "never (within the run)".into(),
                }
            );
            for n in [1usize, 2, 4, 8, 16, 32, 64, 119] {
                if let Some(f) = r.frames.get(at + n) {
                    print!("  +{n}: {:.4}", f.mae);
                }
            }
            println!();
        }
    }

    // SAFETY: the GPU is idle (waited every frame); the pool is no longer used.
    unsafe { gpu.vk_device.destroy_query_pool(gpu.query_pool, None) };
    drop(nrc);
    gpu.device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    let errors = LOGGER.errors.load(Ordering::SeqCst);
    assert_eq!(errors, 0, "validation, wgpu or NRC errors were logged:\n{}", LOGGER.messages.lock().unwrap().join("\n"));
    println!("\nno validation, wgpu or NRC errors during the frames");
    // (teardown: the library's leaked objects are reported at device destruction; known, not checked)
}
