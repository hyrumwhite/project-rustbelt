//! Headless smoke test: a Vulkan device with NRC enabled, a context
//! configured for 1280x720, and ten frames of BeginFrame + QueryAndTrain
//! (no queries or training records yet) + submit + EndFrame.
//!
//! `cargo test -p nrc_wgpu --release -- --ignored --nocapture`
//!
//! Needs an NVIDIA GPU (Turing or newer) and the SDK's DLLs in
//! `<NRC_SDK>/Bin/x64`. Prints the CPU time of each QueryAndTrain call and
//! its GPU time (Vulkan timestamps around it).

use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Instant,
};

use ash::vk;
use nrc_wgpu::{
    BufferIdx, ContextSettings, FrameSettings, Nrc, NrcContext, NrcInitSettings, compute_ideal_training_dimensions,
};
use wgpu::hal::api::Vulkan;

/// Prints records and counts the errors that matter: Vulkan validation
/// (wgpu-hal logs those as errors under `wgpu_hal::vulkan::instance`),
/// wgpu's own errors, and the library's.
struct TestLogger {
    errors: AtomicUsize,
    messages: Mutex<Vec<String>>,
}

impl log::Log for TestLogger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() <= log::Level::Warn
            || (metadata.target().starts_with("nrc") && metadata.level() <= log::Level::Debug)
    }

    fn log(&self, record: &log::Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let line = format!("[{} {}] {}", record.level(), record.target(), record.args());
        eprintln!("{line}");
        let counts = record.level() == log::Level::Error
            && (record.target().starts_with("wgpu") || record.target().starts_with("nrc"));
        if counts {
            self.errors.fetch_add(1, Ordering::SeqCst);
            self.messages.lock().unwrap().push(line);
        }
    }

    fn flush(&self) {}
}

static LOGGER: TestLogger = TestLogger { errors: AtomicUsize::new(0), messages: Mutex::new(Vec::new()) };

const FRAMES: usize = 10;

#[test]
#[ignore = "needs an NVIDIA GPU and the NRC SDK's DLLs"]
fn smoke() {
    log::set_logger(&LOGGER).unwrap();
    log::set_max_level(log::LevelFilter::Debug);

    // The DLLs: NRC_Vulkan.dll is delay-loaded, so it comes from here.
    let bin = nrc_wgpu::sdk_bin_dir();
    for name in nrc_wgpu::RUNTIME_DLLS {
        let signed = nrc_wgpu::verify_signature_of(&bin.join(name));
        println!("{name}: signature {}", if signed { "valid" } else { "NOT valid" });
    }
    nrc_wgpu::load_library(bin, true).expect("loading NRC_Vulkan.dll");
    println!("instance extensions: {:?}", nrc_wgpu::required_instance_extensions().unwrap());
    println!("device extensions:   {:?}", nrc_wgpu::required_device_extensions().unwrap());
    println!("device features:     {:?}", nrc_wgpu::required_device_features().unwrap());

    // Instance (validation on) and device, with NRC's callbacks.
    let instance = nrc_wgpu::create_instance(&wgpu::InstanceDescriptor {
        backends: wgpu::Backends::VULKAN,
        flags: wgpu::InstanceFlags::VALIDATION | wgpu::InstanceFlags::DEBUG,
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    })
    .expect("creating the instance");
    let adapters = futures_lite::future::block_on(instance.enumerate_adapters(wgpu::Backends::VULKAN));
    let adapter = adapters
        .into_iter()
        .find(|a| a.get_info().vendor == 0x10de)
        .expect("no NVIDIA Vulkan adapter");
    let info = adapter.get_info();
    println!("adapter: {} (driver {} {})", info.name, info.driver, info.driver_info);
    // (ray queries as in the game: wgpu then enables buffer device address itself)
    let required_features = adapter.features() & wgpu::Features::EXPERIMENTAL_RAY_QUERY;
    let (device, queue) = nrc_wgpu::request_device(
        &adapter,
        &wgpu::DeviceDescriptor {
            label: Some("nrc smoke"),
            required_features,
            required_limits: adapter.limits(),
            // SAFETY: ray queries are experimental in wgpu; the game uses them the same way.
            experimental_features: unsafe { wgpu::ExperimentalFeatures::enabled() },
            ..Default::default()
        },
        None,
    )
    .expect("creating the device");
    let wgpu_errors = Arc::new(AtomicUsize::new(0));
    {
        let wgpu_errors = Arc::clone(&wgpu_errors);
        device.on_uncaptured_error(Arc::new(move |error| {
            eprintln!("wgpu error: {error}");
            wgpu_errors.fetch_add(1, Ordering::SeqCst);
        }));
    }

    // The library, a context and its buffers.
    let nrc = Nrc::initialize(&NrcInitSettings::new(bin)).expect("nrc::vulkan::Initialize");
    let mut context = NrcContext::new(&nrc, &device).expect("Context::Create");
    let frame_dimensions = [1280, 720];
    let training_dimensions = compute_ideal_training_dimensions(frame_dimensions, 0, 0.0).unwrap();
    println!("training dimensions: {training_dimensions:?}");
    let settings = ContextSettings {
        learn_irradiance: true,
        include_direct_lighting: true,
        scene_bounds_min: [-1000.0; 3],
        scene_bounds_max: [1000.0; 3],
        frame_dimensions,
        training_dimensions,
        samples_per_pixel: 1,
        max_path_vertices: 8,
        ..Default::default()
    };
    let started = Instant::now();
    context.configure(&settings).expect("Context::Configure");
    println!("configure: {:.1} ms", started.elapsed().as_secs_f64() * 1e3);
    let buffers = context.buffers().unwrap();
    for index in BufferIdx::ALL {
        if let Some(buffer) = buffers.get(index) {
            let a = &buffer.allocation;
            println!(
                "  {index:?} ({}): {} x {} = {} bytes{}{}, address {:#x}",
                a.debug_name(),
                a.element_count,
                a.element_size,
                buffer.buffer.size(),
                if a.is_on_shared_heap { ", shared heap" } else { "" },
                if a.use_readback_heap { ", readback" } else { "" },
                buffer.device_address,
            );
            assert_ne!(buffer.device_address, 0);
        }
    }
    println!("  total {:.1} MB", buffers.total_size() as f64 / 1e6);
    let constants = context.shader_constants().expect("PopulateShaderConstants");
    println!("constants: {constants:?}");
    assert_eq!(constants.frame_dimensions, frame_dimensions);
    assert_eq!(constants.training_dimensions, training_dimensions);

    // Timestamps around QueryAndTrain.
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
        vk_device.create_query_pool(
            &vk::QueryPoolCreateInfo::default().query_type(vk::QueryType::TIMESTAMP).query_count(2),
            None,
        )
    }
    .unwrap();
    let timestamp = |encoder: &mut wgpu::CommandEncoder, query: u32| {
        // SAFETY: the encoder only holds raw commands; the pool is ours.
        unsafe {
            encoder.as_hal_mut::<Vulkan, _, _>(|hal| {
                let command_buffer = hal.expect("encoder not recording").raw_handle();
                if query == 0 {
                    vk_device.cmd_reset_query_pool(command_buffer, query_pool, 0, 2);
                }
                vk_device.cmd_write_timestamp(command_buffer, vk::PipelineStageFlags::ALL_COMMANDS, query_pool, query);
            })
        }
    };

    let frame_settings = FrameSettings::default();
    let mut cpu_ms = Vec::new();
    let mut gpu_ms = Vec::new();
    for frame in 0..FRAMES {
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("nrc") });
        context.begin_frame(&mut encoder, &frame_settings).expect("BeginFrame");
        // (the passes writing query and training records go here)
        timestamp(&mut encoder, 0);
        let started = Instant::now();
        context.query_and_train(&mut encoder, false).expect("QueryAndTrain");
        let cpu = started.elapsed().as_secs_f64() * 1e3;
        timestamp(&mut encoder, 1);
        let frame_started = Instant::now();
        queue.submit([encoder.finish()]);
        context.end_frame(&queue).expect("EndFrame");
        device.poll(wgpu::PollType::wait_indefinitely()).expect("waiting for the GPU");
        let wall = frame_started.elapsed().as_secs_f64() * 1e3;
        let mut stamps = [0u64; 2];
        // SAFETY: both queries were written by the finished submission.
        unsafe {
            vk_device.get_query_pool_results(query_pool, 0, &mut stamps, vk::QueryResultFlags::TYPE_64 | vk::QueryResultFlags::WAIT)
        }
        .unwrap();
        let gpu = stamps[1].wrapping_sub(stamps[0]) as f64 * timestamp_period / 1e6;
        println!(
            "frame {frame}: QueryAndTrain recording {cpu:.3} ms CPU, {gpu:.3} ms GPU; submit+EndFrame+wait {wall:.3} ms"
        );
        cpu_ms.push(cpu);
        gpu_ms.push(gpu);
    }
    // (the first frames compile and warm up; the median of the rest is the number)
    let median = |v: &[f64]| {
        let mut v = v[2..].to_vec();
        v.sort_by(f64::total_cmp);
        v[v.len() / 2]
    };
    println!(
        "QueryAndTrain, empty input, median of frames 2..{FRAMES}: {:.3} ms CPU recording, {:.3} ms GPU",
        median(&cpu_ms),
        median(&gpu_ms)
    );

    // SAFETY: the GPU is idle (waited above); the pool is no longer used.
    unsafe { vk_device.destroy_query_pool(query_pool, None) };
    drop(context);
    drop(nrc);
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();

    let errors = LOGGER.errors.load(Ordering::SeqCst);
    assert_eq!(
        errors,
        0,
        "validation or NRC errors were logged:
{}",
        LOGGER.messages.lock().unwrap().join("
")
    );
    assert_eq!(wgpu_errors.load(Ordering::SeqCst), 0, "wgpu reported errors");

    // Known: Context::Destroy and Shutdown leave the library's own Vulkan
    // objects (CUDA modules and functions, its descriptor pool, fence,
    // internal buffers) alive, so validation reports them as leaked when the
    // device goes. Reported here, not failed on: nothing this crate can free.
    drop(queue);
    drop(device);
    let teardown = LOGGER.errors.load(Ordering::SeqCst);
    if teardown > 0 {
        println!("teardown: {teardown} validation message(s) at device destruction (NRC's internal objects, see above)");
    }
}
