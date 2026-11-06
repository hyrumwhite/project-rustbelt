//! NVIDIA's Neural Radiance Cache (NRC) answering the diffuse GI's query
//! (feature `nrc`; `docs/nrc-integration.md` in the game's repository has the
//! design and the spike results these choices come from).
//!
//! The application adds [`SolarikNrcInitPlugin`] before `RenderPlugin` (it
//! registers the Vulkan creation callbacks) and sets
//! [`SolarikNrcSceneBounds`]. Each frame the lighting node then records the
//! library's `BeginFrame`, a training pass, the GI's queries, the library's
//! `QueryAndTrain` and a resolve pass (`node.rs`); [`nrc_end_frame`] calls
//! `EndFrame` after the frame's submit. Any failure is logged (target `nrc`)
//! and leaves the renderer on the world cache, as without the feature.

use super::{SolarikLighting, prepare::SolarikLightingResources};
use alloc::sync::Arc;
use bevy_app::{App, Plugin};
use bevy_diagnostic::FrameCount;
use bevy_ecs::prelude::*;
use bevy_math::{UVec2, Vec3};
use bevy_render::{
    diagnostic::RecordDiagnostics,
    extract_resource::ExtractResource,
    render_resource::{
        BindGroup, BindGroupEntries, BindGroupLayoutDescriptor, BindGroupLayoutEntries, Buffer,
        BufferDescriptor, BufferUsages, CachedComputePipelineId, CachedPipelineState,
        CommandEncoderDescriptor, ComputePipeline, PipelineCache, ShaderStages,
        binding_types::{storage_buffer_read_only_sized, storage_buffer_sized, uniform_buffer_sized},
    },
    renderer::{
        RenderContext, RenderDevice, RenderQueue,
        raw_vulkan_init::{AdditionalVulkanFeatures, RawVulkanInitSettings},
    },
};
use core::{
    num::NonZero,
    sync::atomic::{AtomicBool, Ordering},
};
use nrc_wgpu::{BufferIdx, ContextSettings, FrameSettings, Nrc, NrcContext, NrcError, NrcInitSettings};
use std::path::PathBuf;
use tracing::{error, info, warn};

/// Registers NRC's Vulkan instance and device extensions. Add it before
/// `RenderPlugin` (`DefaultPlugins`): the callbacks run when the renderer
/// creates its device. When the library or an extension is missing it logs
/// why and the renderer runs without NRC.
///
/// `SOLARIK_NRC_DIR` names the directory with the four NRC DLLs (default
/// the SDK's `Bin/x64` from build time).
pub struct SolarikNrcInitPlugin;

/// Marks NRC's extensions as enabled on the render device, in
/// [`AdditionalVulkanFeatures`].
pub struct NrcSupported;

/// Where the NRC DLLs were loaded from (the library's `depsDirectoryPath`).
#[derive(Resource, Clone)]
pub(crate) struct NrcLibraryDir(pub PathBuf);

impl Plugin for SolarikNrcInitPlugin {
    #[expect(
        unsafe_code,
        reason = "registering Vulkan creation callbacks is unsafe; NRC's only add supported extensions and features"
    )]
    fn build(&self, app: &mut App) {
        let dir = std::env::var_os("SOLARIK_NRC_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| nrc_wgpu::sdk_bin_dir().to_path_buf());
        if let Err(e) = nrc_wgpu::load_library(&dir, true) {
            error!(target: "nrc", "NRC is off: loading the library from {} failed: {e}", dir.display());
            return;
        }

        // (the device's extensions are only worth enabling when the
        // instance's were)
        let instance_ready = Arc::new(AtomicBool::new(false));
        let device_needs_instance = Arc::clone(&instance_ready);
        let mut raw_vulkan_settings = app
            .world_mut()
            .get_resource_or_init::<RawVulkanInitSettings>();

        // SAFETY: register_instance_extensions only adds extensions the
        // loader reports as supported, and removes nothing.
        unsafe {
            raw_vulkan_settings.add_create_instance_callback(move |args, _| {
                match nrc_wgpu::register_instance_extensions(args) {
                    Ok(()) => instance_ready.store(true, Ordering::Release),
                    Err(e) => error!(target: "nrc", "NRC is off: instance extensions: {e}"),
                }
            });
        }
        // SAFETY: register_device_extensions only adds extensions and
        // features the physical device supports, and turns nothing off.
        unsafe {
            raw_vulkan_settings.add_create_device_callback(move |args, adapter, features| {
                if !device_needs_instance.load(Ordering::Acquire) {
                    features.remove::<NrcSupported>();
                    return;
                }
                match nrc_wgpu::register_device_extensions(args, adapter) {
                    Ok(()) => features.insert::<NrcSupported>(),
                    Err(e) => {
                        features.remove::<NrcSupported>();
                        error!(target: "nrc", "NRC is off: device extensions: {e}");
                    }
                }
            });
        }
        app.insert_resource(NrcLibraryDir(dir));
    }
}

/// The box every surface a ray can hit lies inside (Bevy world space,
/// metres), which NRC's position encoding spans. The application sets it per
/// level; a change resets the cache. Absent, NRC assumes ±1000 m.
#[derive(Resource, Clone, Copy, Debug, PartialEq)]
pub struct SolarikNrcSceneBounds {
    pub min: Vec3,
    pub max: Vec3,
}

impl ExtractResource<bevy_render::RenderApp> for SolarikNrcSceneBounds {
    type Source = Self;

    fn extract_resource(source: &Self) -> Self {
        *source
    }
}

/// The pipelines the NRC frame adds (`node.rs` queues them when
/// [`NrcState`] exists).
pub(crate) struct NrcPipelineIds {
    /// `restir_gi`'s `initial_and_temporal` with `NRC`: the initial sample
    /// only, its query written instead of the world cache read.
    pub gi_initial: CachedComputePipelineId,
    /// `nrc_training.wesl`'s `nrc_train`.
    pub training: CachedComputePipelineId,
    /// `restir_gi`'s `nrc_resolve_and_temporal`.
    pub resolve: CachedComputePipelineId,
}

impl NrcPipelineIds {
    fn all(&self) -> [CachedComputePipelineId; 3] {
        [self.gi_initial, self.training, self.resolve]
    }
}

/// The render world's NRC: the library's context and what the passes bind.
#[derive(Resource)]
pub struct NrcState {
    // (the context holds an Arc of the library too; this one keeps it for
    // the state's lifetime whatever the context does)
    _nrc: Arc<Nrc>,
    context: NrcContext,
    /// The settings last configured.
    settings: Option<ContextSettings>,
    /// The `FrameCount` at the last configure: the warm-up counts from it.
    configured_frame: Option<u32>,
    /// The view the context is configured for (one view only).
    view: Option<Entity>,
    /// `NrcConstants` (96 bytes), written each frame after `BeginFrame`.
    constants: Buffer,
    /// The two counters (queries, training records) the shaders add to:
    /// device local, copied into the library's Counter buffer before
    /// `QueryAndTrain`. The library's own is in host memory, where every
    /// shader atomic is a PCI Express round trip (0.6 to 1.2 ms a frame).
    counters: Buffer,
    pub(crate) bind_group_layout: BindGroupLayoutDescriptor,
    /// Rebuilt on every configure: the library's buffers may be new.
    bind_group: Option<BindGroup>,
    frame_settings: FrameSettings,
    training_dimensions: [u32; 2],
    /// Frames after a configure during which the GI reads the world cache
    /// (`SOLARIK_NRC_WARMUP`, default 120): an untrained network's answers
    /// are worse than the cache's.
    ///
    /// The training knobs read the environment too while they are tuned
    /// (`SOLARIK_NRC_ITERATIONS` 2, `SOLARIK_NRC_LEARNING_RATE` 5e-3,
    /// `SOLARIK_NRC_ATTENUATION` 0.5, `SOLARIK_NRC_PRIMARY` 0.05); see
    /// `docs/nrc-integration.md`.
    warmup_frames: u32,
    /// Light samples at each training vertex (`SOLARIK_NRC_LIGHT_SAMPLES`,
    /// default 1; the world cache takes four an update), to the shader as
    /// `pad1`.
    light_samples: u32,
    /// Light candidates from the tile at each training vertex's resampled
    /// light sample (`SOLARIK_NRC_TILE_CANDIDATES`, default 32; the cache
    /// update takes 128), to the shader as `pad2`.
    tile_candidates: u32,
    /// `SOLARIK_NRC_FREEZE=1`: stop training once the warm-up is over (a
    /// diagnostic: whether the wander is the network drifting or its answers
    /// varying from point to point).
    freeze_after_warmup: bool,
    /// `BeginFrame` was submitted this frame, so `EndFrame` is due.
    recorded_this_frame: bool,
    /// A call failed; NRC stays off for the session.
    disabled: bool,
    warned_views: bool,
    warned_bounds: bool,
}

impl NrcState {
    /// Logs a failed NRC call once and turns NRC off for the session.
    fn fail(&mut self, what: &str, e: &NrcError) {
        if !self.disabled {
            error!(target: "nrc", "NRC is off for this session: {what} failed: {e}");
        }
        self.disabled = true;
        self.bind_group = None;
    }
}

/// Copies the library directory into the render world (the plugin that
/// found it ran before the render app existed).
pub(crate) fn copy_library_dir(app: &mut App) {
    if let Some(dir) = app.world().get_resource::<NrcLibraryDir>().cloned() {
        app.sub_app_mut(bevy_render::RenderApp).insert_resource(dir);
    }
}

/// Initializes the library and a context on the render device when the
/// device was created with NRC's extensions.
pub fn init_nrc(
    mut commands: Commands,
    features: Option<Res<AdditionalVulkanFeatures>>,
    dir: Option<Res<NrcLibraryDir>>,
    render_device: Res<RenderDevice>,
) {
    let Some(dir) = dir else {
        return;
    };
    if !features.is_some_and(|f| f.has::<NrcSupported>()) {
        warn!(target: "nrc", "NRC is off: the render device was created without its extensions");
        return;
    }
    let nrc = match Nrc::initialize(&NrcInitSettings::new(dir.0.clone())) {
        Ok(nrc) => nrc,
        Err(e) => {
            error!(target: "nrc", "NRC is off: Initialize failed: {e}");
            return;
        }
    };
    let context = match NrcContext::new(&nrc, render_device.wgpu_device()) {
        Ok(context) => context,
        Err(e) => {
            error!(target: "nrc", "NRC is off: creating the context failed: {e}");
            return;
        }
    };
    let constants = render_device.create_buffer(&BufferDescriptor {
        label: Some("solarik_nrc_constants"),
        size: size_of::<nrc_wgpu::NrcConstants>() as u64,
        usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let counters = render_device.create_buffer(&BufferDescriptor {
        label: Some("solarik_nrc_counters"),
        size: 8,
        usage: BufferUsages::STORAGE | BufferUsages::COPY_SRC | BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let bind_group_layout = BindGroupLayoutDescriptor::new(
        "solarik_nrc_bind_group_layout",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::COMPUTE,
            (
                // 0 nrc_constants
                uniform_buffer_sized(false, NonZero::new(size_of::<nrc_wgpu::NrcConstants>() as u64)),
                // 1 query path info, 2 training path info, 3 training path vertices,
                // 4 query radiance params
                storage_buffer_sized(false, None),
                storage_buffer_sized(false, None),
                storage_buffer_sized(false, None),
                storage_buffer_sized(false, None),
                // 5 the device-local counters
                storage_buffer_sized(false, None),
                // 6 the answers
                storage_buffer_read_only_sized(false, None),
            ),
        ),
    );
    let warmup_frames = env_or("SOLARIK_NRC_WARMUP", 120u32);
    let light_samples = env_or("SOLARIK_NRC_LIGHT_SAMPLES", 1u32).clamp(1, 16);
    let freeze_after_warmup = env_or("SOLARIK_NRC_FREEZE", 0u32) != 0;
    let tile_candidates = env_or("SOLARIK_NRC_TILE_CANDIDATES", 32u32).clamp(1, 128);
    let frame_settings = FrameSettings {
        // (the shaders hand NRC exposed radiance, which averages 0.1 to 1)
        max_expected_average_radiance_value: 1.0,
        train_the_cache: true,
        // (the query side's heuristic, used by restir_gi's
        // NRC_QUERY_AT_SPREAD: query once the path's spread has passed this
        // times the primary vertex's, else bounce once more)
        termination_heuristic_threshold: env_or("SOLARIK_NRC_QUERY_THRESHOLD", 0.1f32),
        training_termination_heuristic_threshold: 0.1,
        proportion_unbiased: 1.0 / 16.0,
        // (the training knobs, from the environment while they are tuned;
        // the defaults are half the library's: the measured breathing in
        // stills at a10's corridor and b30's passage went with them, see
        // docs/nrc-integration.md)
        num_training_iterations: env_or("SOLARIK_NRC_ITERATIONS", 2u32),
        learning_rate: env_or("SOLARIK_NRC_LEARNING_RATE", 5e-3f32),
        self_training_attenuation: env_or("SOLARIK_NRC_ATTENUATION", 0.5f32),
        proportion_primary_segments_to_train_on: env_or("SOLARIK_NRC_PRIMARY", 0.05f32),
        ..FrameSettings::default()
    };
    info!(
        target: "nrc",
        "NRC training: {} iterations, learning rate {}, self-training attenuation {}, primary segments {}, {light_samples} light sample(s) a vertex, {tile_candidates} tile candidates, query threshold {}",
        frame_settings.num_training_iterations,
        frame_settings.learning_rate,
        frame_settings.self_training_attenuation,
        frame_settings.proportion_primary_segments_to_train_on,
        frame_settings.termination_heuristic_threshold,
    );
    info!(target: "nrc", "NRC initialized from {} (warm-up {warmup_frames} frames)", dir.0.display());
    commands.insert_resource(NrcState {
        _nrc: nrc,
        context,
        settings: None,
        configured_frame: None,
        view: None,
        constants,
        counters,
        bind_group_layout,
        bind_group: None,
        frame_settings,
        training_dimensions: [0, 0],
        warmup_frames,
        light_samples,
        tile_candidates,
        freeze_after_warmup,
        recorded_this_frame: false,
        disabled: false,
        warned_views: false,
        warned_bounds: false,
    });
}

/// An environment variable parsed as `T`, or `default` when unset or unparsable.
fn env_or<T: std::str::FromStr>(name: &str, default: T) -> T {
    std::env::var(name)
        .ok()
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(default)
}

/// Configures the context for the lighting view when its size or the scene
/// bounds change (a hitch, allowed then), and rebuilds the bind group.
pub fn prepare_nrc(
    state: Option<ResMut<NrcState>>,
    views: Query<(Entity, &SolarikLightingResources), With<SolarikLighting>>,
    bounds: Option<Res<SolarikNrcSceneBounds>>,
    frame_count: Res<FrameCount>,
    pipeline_cache: Res<PipelineCache>,
    render_device: Res<RenderDevice>,
) {
    let Some(mut state) = state else {
        return;
    };
    if state.disabled {
        return;
    }
    // One view only: the one configured, while it lasts, else the first.
    if views.iter().count() > 1 && !state.warned_views {
        warn!(target: "nrc", "NRC serves one view; the other Solarik views use the world cache");
        state.warned_views = true;
    }
    let view = state
        .view
        .and_then(|v| views.get(v).ok())
        .or_else(|| views.iter().next());
    let Some((view, resources)) = view else {
        return;
    };
    let view_size: UVec2 = resources.view_size;
    if view_size.x == 0 || view_size.y == 0 {
        return;
    }

    let (bounds_min, bounds_max) = match bounds {
        Some(b) => (b.min.to_array(), b.max.to_array()),
        None => {
            if !state.warned_bounds {
                warn!(target: "nrc", "no SolarikNrcSceneBounds; NRC assumes ±1000 m");
                state.warned_bounds = true;
            }
            ([-1000.0; 3], [1000.0; 3])
        }
    };
    let frame_dimensions = view_size.to_array();
    let training_dimensions = match nrc_wgpu::compute_ideal_training_dimensions(frame_dimensions, 0, 0.0)
    {
        Ok(d) => d,
        Err(e) => {
            state.fail("ComputeIdealTrainingDimensions", &e);
            return;
        }
    };
    let mut wanted = ContextSettings {
        learn_irradiance: true,
        include_direct_lighting: true,
        request_reset: false,
        scene_bounds_min: bounds_min,
        scene_bounds_max: bounds_max,
        // (`SOLARIK_NRC_FEATURE_SIZE`, metres: the hash grid's finest level;
        // coarser smooths the answers from point to point)
        smallest_resolvable_feature_size: env_or("SOLARIK_NRC_FEATURE_SIZE", 0.01f32),
        frame_dimensions,
        training_dimensions,
        samples_per_pixel: 1,
        max_path_vertices: 8,
    };
    let bounds_changed = state.settings.is_some_and(|last| {
        last.scene_bounds_min != wanted.scene_bounds_min || last.scene_bounds_max != wanted.scene_bounds_max
    });
    let unchanged = state.settings.is_some_and(|last| {
        ContextSettings { request_reset: false, ..last } == wanted
    });
    if unchanged && state.view == Some(view) && state.bind_group.is_some() {
        return;
    }
    // (a new level: what the network learnt describes the old one)
    wanted.request_reset = bounds_changed;
    if let Err(e) = state.context.configure(&wanted) {
        state.fail("Configure", &e);
        return;
    }
    let bind_group = {
        let Some(buffers) = state.context.buffers() else {
            state.fail("Configure", &NrcError::NotConfigured);
            return;
        };
        let wanted_buffers = [
            BufferIdx::QueryPathInfo,
            BufferIdx::TrainingPathInfo,
            BufferIdx::TrainingPathVertices,
            BufferIdx::QueryRadianceParams,
            BufferIdx::QueryRadiance,
            BufferIdx::Counter,
        ];
        if let Some(missing) = wanted_buffers.into_iter().find(|&i| buffers.get(i).is_none()) {
            error!(target: "nrc", "NRC is off for this session: no {missing:?} buffer after Configure");
            state.disabled = true;
            return;
        }
        let buffer = |i| &buffers.get(i).expect("checked above").buffer;
        render_device.create_bind_group(
            "solarik_nrc_bind_group",
            &pipeline_cache.get_bind_group_layout(&state.bind_group_layout),
            &BindGroupEntries::sequential((
                state.constants.as_entire_binding(),
                buffer(BufferIdx::QueryPathInfo).as_entire_binding(),
                buffer(BufferIdx::TrainingPathInfo).as_entire_binding(),
                buffer(BufferIdx::TrainingPathVertices).as_entire_binding(),
                buffer(BufferIdx::QueryRadianceParams).as_entire_binding(),
                state.counters.as_entire_binding(),
                buffer(BufferIdx::QueryRadiance).as_entire_binding(),
            )),
        )
    };
    info!(
        target: "nrc",
        "NRC configured: {}x{} frame, {}x{} training, bounds {bounds_min:?}..{bounds_max:?}{}, {:.1} MB",
        frame_dimensions[0],
        frame_dimensions[1],
        training_dimensions[0],
        training_dimensions[1],
        if wanted.request_reset { " (reset)" } else { "" },
        state.context.buffers().map_or(0, nrc_wgpu::NrcBuffers::total_size) as f64 / 1e6,
    );
    state.bind_group = Some(bind_group);
    state.settings = Some(wanted);
    state.training_dimensions = training_dimensions;
    state.configured_frame = Some(frame_count.0);
    state.view = Some(view);
}

/// `EndFrame`, after the frame's command buffers were submitted.
pub fn nrc_end_frame(state: Option<ResMut<NrcState>>, render_queue: Res<RenderQueue>) {
    let Some(mut state) = state else {
        return;
    };
    if !core::mem::take(&mut state.recorded_this_frame) {
        return;
    }
    if let Err(e) = state.context.end_frame(&render_queue) {
        state.fail("EndFrame", &e);
    }
}

/// Whether an NRC pipeline is still compiling. One that failed counts as
/// done: the node then falls back to the world cache.
pub(crate) fn pipelines_pending(
    ids: Option<&NrcPipelineIds>,
    state: Option<&NrcState>,
    cache: &PipelineCache,
) -> bool {
    let (Some(ids), Some(state)) = (ids, state) else {
        return false;
    };
    !state.disabled
        && ids.all().into_iter().any(|id| {
            matches!(
                cache.get_compute_pipeline_state(id),
                CachedPipelineState::Queued | CachedPipelineState::Creating(_)
            )
        })
}

/// What the lighting node needs for an NRC frame.
pub(crate) struct NrcFrame<'a> {
    state: ResMut<'a, NrcState>,
    bind_group: BindGroup,
    gi_initial: &'a ComputePipeline,
    training: &'a ComputePipeline,
    resolve: &'a ComputePipeline,
}

/// Whether the node runs this frame with NRC.
pub(crate) enum NrcReadiness<'a> {
    Ready(NrcFrame<'a>),
    /// Its pipelines are compiling: the node waits, as for its own.
    Pending,
    /// The world cache path, as without NRC.
    Off,
}

impl<'a> NrcFrame<'a> {
    /// NRC for `view` this frame: ready when the context is configured for
    /// it at `view_size` and its pipelines compiled.
    pub(crate) fn prepare(
        state: Option<ResMut<'a, NrcState>>,
        ids: Option<&NrcPipelineIds>,
        cache: &'a PipelineCache,
        view: Entity,
        view_size: UVec2,
    ) -> NrcReadiness<'a> {
        let (Some(mut state), Some(ids)) = (state, ids) else {
            return NrcReadiness::Off;
        };
        if state.disabled
            || state.view != Some(view)
            || state.settings.map(|s| s.frame_dimensions) != Some(view_size.to_array())
        {
            return NrcReadiness::Off;
        }
        let Some(bind_group) = state.bind_group.clone() else {
            return NrcReadiness::Off;
        };
        let pipeline = |id| cache.get_compute_pipeline(id);
        if let (Some(gi_initial), Some(training), Some(resolve)) =
            (pipeline(ids.gi_initial), pipeline(ids.training), pipeline(ids.resolve))
        {
            return NrcReadiness::Ready(NrcFrame { state, bind_group, gi_initial, training, resolve });
        }
        if pipelines_pending(Some(ids), Some(&state), cache) {
            return NrcReadiness::Pending;
        }
        error!(target: "nrc", "NRC is off for this session: its pipelines failed to compile (the log above says why)");
        state.disabled = true;
        state.bind_group = None;
        NrcReadiness::Off
    }

    pub(crate) fn bind_group(&self) -> &BindGroup {
        &self.bind_group
    }

    pub(crate) fn gi_initial_pipeline(&self) -> &'a ComputePipeline {
        self.gi_initial
    }

    pub(crate) fn training_pipeline(&self) -> &'a ComputePipeline {
        self.training
    }

    pub(crate) fn resolve_pipeline(&self) -> &'a ComputePipeline {
        self.resolve
    }

    /// The training pass's workgroups (8x8 threads, one per training path).
    pub(crate) fn training_workgroups(&self) -> (u32, u32) {
        let [x, y] = self.state.training_dimensions;
        (x.div_ceil(8), y.div_ceil(8))
    }

    /// Records `BeginFrame` in an encoder of its own (wgpu panics when raw
    /// Vulkan commands share an encoder with its own), writes the frame's
    /// constants (after `BeginFrame`: the unpack multiplier follows the frame
    /// settings) and clears the counters. False when NRC failed and the
    /// frame must take the world cache path.
    pub(crate) fn begin_frame(
        &mut self,
        ctx: &mut RenderContext,
        render_device: &RenderDevice,
        render_queue: &RenderQueue,
        frame: u32,
    ) -> bool {
        let state = &mut *self.state;
        let mut encoder = render_device.create_command_encoder(&CommandEncoderDescriptor {
            label: Some("solarik_nrc_begin_frame"),
        });
        let since_configure = state.configured_frame.map_or(0, |f| frame.wrapping_sub(f));
        let warm = since_configure >= state.warmup_frames;
        let frame_settings = FrameSettings {
            train_the_cache: !(state.freeze_after_warmup && warm),
            ..state.frame_settings
        };
        if let Err(e) = state.context.begin_frame(&mut encoder, &frame_settings) {
            state.fail("BeginFrame", &e);
            return false;
        }
        ctx.add_command_buffer(encoder.finish());
        state.recorded_this_frame = true;

        let mut constants = match state.context.shader_constants() {
            Ok(constants) => constants,
            Err(e) => {
                state.fail("PopulateShaderConstants", &e);
                return false;
            }
        };
        // pad0: whether the GI uses NRC's answers yet (else the world cache)
        constants.pad[0] = u32::from(warm);
        // pad1: light samples at each training vertex
        constants.pad[1] = state.light_samples;
        // pad2: tile candidates at each training vertex's light sample
        constants.pad[2] = state.tile_candidates;
        render_queue.write_buffer(&state.constants, 0, bytemuck::bytes_of(&constants));
        ctx.command_encoder().clear_buffer(&state.counters, 0, None);
        true
    }

    /// Copies the frame's counters into the library's Counter buffer and
    /// records `QueryAndTrain` in an encoder of its own. False when NRC
    /// failed (it is then off from the next frame).
    pub(crate) fn query_and_train(
        &mut self,
        ctx: &mut RenderContext,
        render_device: &RenderDevice,
        diagnostics: &impl RecordDiagnostics,
    ) -> bool {
        let state = &mut *self.state;
        let Some(buffers) = state.context.buffers() else {
            state.fail("QueryAndTrain", &NrcError::NotConfigured);
            return false;
        };
        let Some(counter) = buffers.get(BufferIdx::Counter) else {
            state.fail("QueryAndTrain", &NrcError::NotConfigured);
            return false;
        };
        // (HALO_RT_CACHE_LOG diagnostics: this frame's query count and the
        // first words of the records and of the last frame's answers, as
        // the game prints them)
        let word = |index: u64| index * 4..index * 4 + 4;
        diagnostics.record_u32(ctx.command_encoder(), &state.counters.slice(word(0)), "solarik_lighting/nrc_queries");
        for (index, name, offset, float) in [
            (BufferIdx::QueryRadiance, "nrc_answer0", 0, true),
            (BufferIdx::QueryRadianceParams, "nrc_query0_position_x", 0, true),
            (BufferIdx::QueryRadianceParams, "nrc_query0_albedo_r", 8, true),
            (BufferIdx::TrainingPathVertices, "nrc_vertex0_radiance", 0, false),
            (BufferIdx::TrainingPathVertices, "nrc_vertex0_throughput", 1, false),
            (BufferIdx::TrainingPathInfo, "nrc_path0_packed", 0, false),
        ] {
            if let Some(buffer) = buffers.get(index) {
                let name = format!("solarik_lighting/{name}");
                if float {
                    diagnostics.record_f32(ctx.command_encoder(), &buffer.buffer.slice(word(offset)), name);
                } else {
                    diagnostics.record_u32(ctx.command_encoder(), &buffer.buffer.slice(word(offset)), name);
                }
            }
        }
        ctx.command_encoder()
            .copy_buffer_to_buffer(&state.counters, 0, &counter.buffer, 0, 8);
        let mut encoder = render_device.create_command_encoder(&CommandEncoderDescriptor {
            label: Some("solarik_nrc_query_and_train"),
        });
        // (report_loss is broken on Vulkan in SDK 0.16: InternalError)
        if let Err(e) = state.context.query_and_train(&mut encoder, false) {
            state.fail("QueryAndTrain", &e);
            return false;
        }
        ctx.add_command_buffer(encoder.finish());
        true
    }
}
