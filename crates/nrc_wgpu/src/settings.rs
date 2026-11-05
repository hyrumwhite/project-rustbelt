//! The structs NRC takes and returns, mirrored field for field from
//! `NrcCommon.h` and `NrcStructures.h` (x64 MSVC layout; a unit test compares
//! every offset with the C++ compiler's). Defaults are the headers' defaults.

use std::ffi::{CStr, c_char};

use bytemuck::{Pod, Zeroable};

/// `nrc::ContextSettings`: what [`crate::NrcContext::configure`] takes. These
/// change rarely (resolution, scene bounds); configuring can reallocate the
/// buffers and reset the cache.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ContextSettings {
    /// Learn irradiance (demodulated by albedo) rather than radiance.
    pub learn_irradiance: bool,
    /// The cache includes direct lighting.
    pub include_direct_lighting: bool,
    /// Forces a cache reset on the next configure (e.g. a new level).
    pub request_reset: bool,
    /// Everything a ray can hit must lie inside these bounds (world units).
    pub scene_bounds_min: [f32; 3],
    pub scene_bounds_max: [f32; 3],
    /// The cache's resolution floor, in world units.
    pub smallest_resolvable_feature_size: f32,
    /// The path tracing resolution.
    pub frame_dimensions: [u32; 2],
    /// The training resolution; see [`compute_ideal_training_dimensions`](crate::compute_ideal_training_dimensions).
    pub training_dimensions: [u32; 2],
    /// Paths per pixel in the main pass (buffer sizes scale with it).
    pub samples_per_pixel: u32,
    /// The most vertices a training path has.
    pub max_path_vertices: u32,
}

impl Default for ContextSettings {
    fn default() -> Self {
        Self {
            learn_irradiance: false,
            include_direct_lighting: false,
            request_reset: false,
            scene_bounds_min: [0.0; 3],
            scene_bounds_max: [0.0; 3],
            smallest_resolvable_feature_size: 0.01,
            frame_dimensions: [0, 0],
            training_dimensions: [0, 0],
            samples_per_pixel: 1,
            max_path_vertices: 8,
        }
    }
}

/// `NrcResolveMode` (NrcStructures.h): what the optional resolve pass shows.
#[repr(i32)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ResolveMode {
    #[default]
    AddQueryResultToOutput = 0,
    ReplaceOutputWithQueryResult,
    TrainingBounceHeatMap,
    TrainingBounceHeatMapSmoothed,
    PrimaryVertexTrainingRadiance,
    PrimaryVertexTrainingRadianceSmoothed,
    SecondaryVertexTrainingRadiance,
    SecondaryVertexTrainingRadianceSmoothed,
    QueryIndex,
    TrainingQueryIndex,
    DirectCacheView,
}

/// `nrc::FrameSettings`: per-frame knobs for [`crate::NrcContext::begin_frame`].
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FrameSettings {
    /// About the average radiance of a bright scene; scales radiance into the network's range.
    pub max_expected_average_radiance_value: f32,
    /// Do not terminate on mirrors; continue to the next vertex.
    pub skip_delta_vertices: bool,
    /// The termination heuristic's threshold (lower terminates earlier).
    pub termination_heuristic_threshold: f32,
    /// Normally the same as `termination_heuristic_threshold`.
    pub training_termination_heuristic_threshold: f32,
    pub resolve_mode: ResolveMode,
    /// False freezes the cache.
    pub train_the_cache: bool,
    /// The part of the training dimensions the training pass wrote; (0, 0) is all of it.
    pub used_training_dimensions: [u32; 2],
    pub proportion_primary_segments_to_train_on: f32,
    pub proportion_tertiary_plus_segments_to_train_on: f32,
    pub proportion_unbiased_to_self_train: f32,
    pub proportion_unbiased: f32,
    pub self_training_attenuation: f32,
    /// Training iterations per frame (each takes 16K training records).
    pub num_training_iterations: u32,
    pub learning_rate: f32,
}

impl Default for FrameSettings {
    fn default() -> Self {
        Self {
            max_expected_average_radiance_value: 1.0,
            skip_delta_vertices: false,
            termination_heuristic_threshold: 0.1,
            training_termination_heuristic_threshold: 0.1,
            resolve_mode: ResolveMode::AddQueryResultToOutput,
            train_the_cache: true,
            used_training_dimensions: [0, 0],
            proportion_primary_segments_to_train_on: 0.05,
            proportion_tertiary_plus_segments_to_train_on: 1.0,
            proportion_unbiased_to_self_train: 1.0,
            proportion_unbiased: 0.0625,
            self_training_attenuation: 1.0,
            num_training_iterations: 4,
            learning_rate: 1e-2,
        }
    }
}

/// `NrcConstants` (NrcStructures.h, 96 bytes): the shader constants from
/// [`crate::NrcContext::shader_constants`], ready for a uniform buffer.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct NrcConstants {
    pub frame_dimensions: [u32; 2],
    pub training_dimensions: [u32; 2],
    pub scene_pos_scale: [f32; 3],
    pub samples_per_pixel: u32,
    pub scene_pos_bias: [f32; 3],
    pub max_path_vertices: u32,
    pub learn_irradiance: u32,
    pub radiance_cache_direct: u32,
    /// See `NrcUnpackQueryRadiance`.
    pub radiance_unpack_multiplier: f32,
    /// A [`ResolveMode`] value.
    pub resolve_mode: u32,
    pub enable_termination_heuristic: u32,
    pub skip_delta_vertices: u32,
    pub termination_heuristic_threshold: f32,
    pub training_termination_heuristic_threshold: f32,
    pub proportion_unbiased: f32,
    pub pad: [u32; 3],
}

/// `nrc::BufferIdx`: the buffers NRC and the path tracer share.
#[repr(i32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BufferIdx {
    /// Atomic counters (queries, training records): 2 x u32.
    Counter = 0,
    /// Per path: query index and prefix throughput (`NrcPackedQueryPathInfo`).
    QueryPathInfo,
    /// Per training path (`NrcPackedTrainingPathInfo`).
    TrainingPathInfo,
    /// Per training vertex (`NrcPackedPathVertex`).
    TrainingPathVertices,
    /// Training radiance, one per training vertex.
    TrainingRadiance,
    /// `NrcRadianceParams` for each training radiance entry.
    TrainingRadianceParams,
    /// The answers: radiance per query.
    QueryRadiance,
    /// The questions: `NrcRadianceParams` per query.
    QueryRadianceParams,
    /// Only with debug buffers: data for the debug resolve modes.
    DebugTrainingPathInfo,
}

impl BufferIdx {
    /// Every buffer, in index order.
    pub const ALL: [BufferIdx; crate::sys::BUFFER_COUNT] = [
        BufferIdx::Counter,
        BufferIdx::QueryPathInfo,
        BufferIdx::TrainingPathInfo,
        BufferIdx::TrainingPathVertices,
        BufferIdx::TrainingRadiance,
        BufferIdx::TrainingRadianceParams,
        BufferIdx::QueryRadiance,
        BufferIdx::QueryRadianceParams,
        BufferIdx::DebugTrainingPathInfo,
    ];
}

/// `nrc::AllocationInfo` (32 bytes): how big one buffer must be and where it lives.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct AllocationInfo {
    pub element_count: usize,
    pub element_size: usize,
    /// Shared with CUDA (D3D12's shared heap; a plain device-local buffer on Vulkan).
    pub is_on_shared_heap: bool,
    pub allow_uav: bool,
    /// CPU-readable (the counters).
    pub use_readback_heap: bool,
    pub(crate) debug_name: *const c_char,
}

impl AllocationInfo {
    /// The buffer's size in bytes (0 = not needed with these settings).
    pub fn size(&self) -> u64 {
        (self.element_count as u64) * (self.element_size as u64)
    }

    /// The library's name for the buffer.
    pub fn debug_name(&self) -> &str {
        if self.debug_name.is_null() {
            return "";
        }
        // SAFETY: the library returns pointers to static strings inside
        // NRC_Vulkan.dll, which this crate never unloads.
        unsafe { CStr::from_ptr(self.debug_name) }.to_str().unwrap_or("")
    }
}

// SAFETY: `debug_name` points at an immutable static string in the DLL.
unsafe impl Send for AllocationInfo {}
// SAFETY: as above.
unsafe impl Sync for AllocationInfo {}
