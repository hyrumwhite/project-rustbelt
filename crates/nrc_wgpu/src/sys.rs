//! The C shim's functions (shim/nrc_shim.h) and the C++ structs only the
//! crate itself needs. The structs the caller fills in live in
//! [`crate::settings`].

use std::ffi::{c_char, c_void};

use crate::settings::{AllocationInfo, ContextSettings, FrameSettings, NrcConstants};

/// `nrc::CustomLoggerPtr`.
pub type LoggerFn = unsafe extern "C" fn(message: *const c_char, level: i32);

/// `nrc::GlobalSettings` (56 bytes).
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct GlobalSettings {
    pub major_version: i32,
    pub minor_version: i32,
    pub logger_fn: Option<LoggerFn>,
    pub memory_logger_fn: *const c_void,
    pub allocator_fn: *const c_void,
    pub deallocator_fn: *const c_void,
    pub enable_gpu_memory_allocation: bool,
    pub enable_debug_buffers: bool,
    pub max_num_frames_in_flight: u32,
    pub deps_directory_path: *const u16,
}

/// The version of the headers the shim was built against (NrcCommon.h).
pub const NRC_VERSION_MAJOR: i32 = 0;
pub const NRC_VERSION_MINOR: i32 = 16;

/// `nrc::BufferIdx::Count`.
pub const BUFFER_COUNT: usize = 9;

/// `nrc::BuffersAllocationInfo`.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct BuffersAllocationInfo {
    pub allocation_info: [AllocationInfo; BUFFER_COUNT],
}

/// `nrc::vulkan::BufferInfo` (40 bytes).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct BufferInfo {
    /// `VkBuffer`
    pub resource: u64,
    pub allocated_size: usize,
    pub allocated_offset: usize,
    /// `VkDeviceMemory`
    pub memory: u64,
    /// `VkDeviceAddress`
    pub device_address: u64,
}

/// `nrc::vulkan::Buffers`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct Buffers {
    pub buffers: [BufferInfo; BUFFER_COUNT],
}

unsafe extern "C" {
    pub fn nrc_shim_initialize(global_settings: *const GlobalSettings) -> i32;
    pub fn nrc_shim_shutdown();

    pub fn nrc_shim_get_instance_extensions(out_names: *mut *const *const c_char) -> u32;
    pub fn nrc_shim_get_device_extensions(out_names: *mut *const *const c_char) -> u32;
    pub fn nrc_shim_get_device_features(out_names: *mut *const *const c_char) -> u32;

    pub fn nrc_shim_compute_ideal_training_dimensions(
        frame_dimensions: *const u32,
        num_training_iterations: u32,
        avg_training_vertices_per_path: f32,
        out_dimensions: *mut u32,
    );

    pub fn nrc_shim_context_create(
        device: u64,
        physical_device: u64,
        instance: u64,
        out_context: *mut *mut c_void,
    ) -> i32;
    pub fn nrc_shim_context_destroy(context: *mut c_void) -> i32;
    pub fn nrc_shim_get_buffers_allocation_info(
        context_settings: *const ContextSettings,
        out_allocation_info: *mut BuffersAllocationInfo,
    ) -> i32;
    pub fn nrc_shim_context_configure(
        context: *mut c_void,
        context_settings: *const ContextSettings,
        buffers: *const Buffers,
        network_config_filename: *const c_char,
    ) -> i32;
    pub fn nrc_shim_context_populate_shader_constants(
        context: *const c_void,
        out_constants: *mut NrcConstants,
    ) -> i32;
    pub fn nrc_shim_context_begin_frame(
        context: *mut c_void,
        command_buffer: u64,
        frame_settings: *const FrameSettings,
    ) -> i32;
    pub fn nrc_shim_context_query_and_train(
        context: *mut c_void,
        command_buffer: u64,
        training_loss: *mut f32,
    ) -> i32;
    pub fn nrc_shim_context_end_frame(context: *mut c_void, queue: u64) -> i32;
    pub fn nrc_shim_context_get_buffer_usage_flags(
        context: *const c_void,
        allocation_info: *const AllocationInfo,
    ) -> u32;

    /// Size or offset `index` of the C++ structs; `u64::MAX` past the end.
    /// Calls nothing in NRC_Vulkan.dll.
    #[cfg(test)]
    pub fn nrc_shim_layout(index: u32) -> u64;
}

/// The Rust side of `nrc_shim_layout`'s table, in the same order.
#[cfg(test)]
pub fn rust_layout() -> Vec<(&'static str, usize)> {
    use std::mem::{offset_of, size_of};
    vec![
        ("sizeof GlobalSettings", size_of::<GlobalSettings>()),
        ("GlobalSettings.loggerFn", offset_of!(GlobalSettings, logger_fn)),
        ("GlobalSettings.enableGPUMemoryAllocation", offset_of!(GlobalSettings, enable_gpu_memory_allocation)),
        ("GlobalSettings.enableDebugBuffers", offset_of!(GlobalSettings, enable_debug_buffers)),
        ("GlobalSettings.maxNumFramesInFlight", offset_of!(GlobalSettings, max_num_frames_in_flight)),
        ("GlobalSettings.depsDirectoryPath", offset_of!(GlobalSettings, deps_directory_path)),
        ("sizeof ContextSettings", size_of::<ContextSettings>()),
        ("ContextSettings.requestReset", offset_of!(ContextSettings, request_reset)),
        ("ContextSettings.sceneBoundsMin", offset_of!(ContextSettings, scene_bounds_min)),
        ("ContextSettings.sceneBoundsMax", offset_of!(ContextSettings, scene_bounds_max)),
        ("ContextSettings.smallestResolvableFeatureSize", offset_of!(ContextSettings, smallest_resolvable_feature_size)),
        ("ContextSettings.frameDimensions", offset_of!(ContextSettings, frame_dimensions)),
        ("ContextSettings.trainingDimensions", offset_of!(ContextSettings, training_dimensions)),
        ("ContextSettings.samplesPerPixel", offset_of!(ContextSettings, samples_per_pixel)),
        ("ContextSettings.maxPathVertices", offset_of!(ContextSettings, max_path_vertices)),
        ("sizeof FrameSettings", size_of::<FrameSettings>()),
        ("FrameSettings.skipDeltaVertices", offset_of!(FrameSettings, skip_delta_vertices)),
        ("FrameSettings.terminationHeuristicThreshold", offset_of!(FrameSettings, termination_heuristic_threshold)),
        ("FrameSettings.resolveMode", offset_of!(FrameSettings, resolve_mode)),
        ("FrameSettings.trainTheCache", offset_of!(FrameSettings, train_the_cache)),
        ("FrameSettings.usedTrainingDimensions", offset_of!(FrameSettings, used_training_dimensions)),
        ("FrameSettings.proportionPrimarySegmentsToTrainOn", offset_of!(FrameSettings, proportion_primary_segments_to_train_on)),
        ("FrameSettings.proportionUnbiased", offset_of!(FrameSettings, proportion_unbiased)),
        ("FrameSettings.selfTrainingAttenuation", offset_of!(FrameSettings, self_training_attenuation)),
        ("FrameSettings.numTrainingIterations", offset_of!(FrameSettings, num_training_iterations)),
        ("FrameSettings.learningRate", offset_of!(FrameSettings, learning_rate)),
        ("sizeof AllocationInfo", size_of::<AllocationInfo>()),
        ("AllocationInfo.isOnSharedHeap", offset_of!(AllocationInfo, is_on_shared_heap)),
        ("AllocationInfo.useReadbackHeap", offset_of!(AllocationInfo, use_readback_heap)),
        ("AllocationInfo.debugName", offset_of!(AllocationInfo, debug_name)),
        ("sizeof BufferInfo", size_of::<BufferInfo>()),
        ("BufferInfo.allocatedOffset", offset_of!(BufferInfo, allocated_offset)),
        ("BufferInfo.memory", offset_of!(BufferInfo, memory)),
        ("BufferInfo.deviceAddress", offset_of!(BufferInfo, device_address)),
        ("sizeof NrcConstants", size_of::<NrcConstants>()),
        ("NrcConstants.scenePosScale", offset_of!(NrcConstants, scene_pos_scale)),
        ("NrcConstants.maxPathVertices", offset_of!(NrcConstants, max_path_vertices)),
        ("NrcConstants.resolveMode", offset_of!(NrcConstants, resolve_mode)),
        ("NrcConstants.proportionUnbiased", offset_of!(NrcConstants, proportion_unbiased)),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The #[repr(C)] mirrors match the C++ structs field for field. Only
    /// calls the shim, so NRC_Vulkan.dll need not be loadable.
    #[test]
    fn layouts_match_the_sdk_headers() {
        let rust = rust_layout();
        for (index, (name, rust_value)) in rust.iter().enumerate() {
            // SAFETY: nrc_shim_layout reads a static table and touches nothing in NRC.
            let cpp = unsafe { nrc_shim_layout(index as u32) };
            assert_eq!(cpp, *rust_value as u64, "{name}: C++ {cpp}, Rust {rust_value}");
        }
        // SAFETY: as above.
        assert_eq!(unsafe { nrc_shim_layout(rust.len() as u32) }, u64::MAX, "the C++ table has more entries");
    }
}
