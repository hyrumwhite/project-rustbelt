//! NVIDIA's Neural Radiance Cache (NRC, Vulkan) on wgpu.
//!
//! The NRC library is C++; a small C shim (`shim/`) wraps it and this crate
//! wraps the shim. Use it in this order:
//!
//! 1. [`load_library`] the DLL from the directory holding the four runtime
//!    DLLs ([`sdk_bin_dir`] on the build machine).
//! 2. Create the instance and device with NRC's extensions and features:
//!    [`create_instance`] and [`request_device`], or from your own wgpu-hal
//!    callbacks with [`register_instance_extensions`] and
//!    [`register_device_extensions`].
//! 3. [`Nrc::initialize`], then [`NrcContext::new`] and
//!    [`NrcContext::configure`] (allocates the buffers, see
//!    [`NrcContext::buffers`]).
//! 4. Per frame: [`NrcContext::begin_frame`], your passes,
//!    [`NrcContext::query_and_train`], submit, [`NrcContext::end_frame`].
//!
//! Buffer memory is app-managed: the crate allocates each buffer with
//! Vulkan (device address on) and wraps it as a [`wgpu::Buffer`] for binding.
//!
//! Windows x64 only. Subject to the NVIDIA RTX SDKs license (`vendor/NRC/License.md`).

mod context;
mod initialization;
mod library;
mod settings;
mod sys;

pub use context::{NrcBuffer, NrcBuffers, NrcContext};
pub use initialization::{
    create_instance, register_device_extensions, register_instance_extensions, request_device,
    required_device_extensions, required_device_features, required_instance_extensions,
};
pub use library::{NRC_DLL, Nrc, NrcInitSettings, RUNTIME_DLLS, load_library, sdk_bin_dir, verify_signature_of};
pub use settings::{AllocationInfo, BufferIdx, ContextSettings, FrameSettings, NrcConstants, ResolveMode};

use std::path::PathBuf;

/// `nrc::Status`: the library's result codes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Ok,
    SdkVersionMismatch,
    AlreadyInitialized,
    SdkNotInitialized,
    InternalError,
    MemoryNotProvided,
    OutOfMemory,
    AllocationFailed,
    ErrorParsingJson,
    WrongParameter,
    UnsupportedDriver,
    UnsupportedHardware,
    /// A value the headers do not list.
    Unknown(i32),
}

impl Status {
    pub(crate) fn from_raw(raw: i32) -> Self {
        use Status::*;
        match raw {
            0 => Ok,
            1 => SdkVersionMismatch,
            2 => AlreadyInitialized,
            3 => SdkNotInitialized,
            4 => InternalError,
            5 => MemoryNotProvided,
            6 => OutOfMemory,
            7 => AllocationFailed,
            8 => ErrorParsingJson,
            9 => WrongParameter,
            10 => UnsupportedDriver,
            11 => UnsupportedHardware,
            other => Unknown(other),
        }
    }
}

/// Errors from this crate.
#[derive(thiserror::Error, Debug)]
pub enum NrcError {
    #[error("NRC returned {0:?} (the log, target \"nrc\", says more)")]
    Status(Status),
    #[error("NRC_Vulkan.dll is not loaded; call nrc_wgpu::load_library first")]
    NotLoaded,
    #[error("{0} does not exist")]
    LibraryNotFound(PathBuf),
    #[error("loading {0} failed: {1}")]
    LoadFailed(PathBuf, std::io::Error),
    #[error("{0} has no valid signature")]
    BadSignature(PathBuf),
    #[error("the device or adapter is not using the Vulkan backend")]
    NotVulkan,
    #[error("creating the instance failed: {0}")]
    Instance(String),
    #[error("creating the device failed: {0}")]
    Device(String),
    #[error("Vulkan error: {0}")]
    Vulkan(ash::vk::Result),
    #[error("the Vulkan instance does not support {0}, which NRC needs")]
    MissingInstanceExtension(String),
    #[error("the GPU does not support the device extension {0}, which NRC needs")]
    MissingDeviceExtension(String),
    #[error("the GPU does not support {0}, which NRC needs")]
    MissingDeviceFeature(String),
    #[error("NRC asks for the device feature {0}, which this crate does not know how to enable")]
    UnknownDeviceFeature(String),
    #[error("no suitable memory type for the {0:?} buffer")]
    NoMemoryType(BufferIdx),
    #[error("the {0:?} buffer has no device address (buffer device address not enabled?)")]
    NoDeviceAddress(BufferIdx),
    #[error("the context is not configured yet")]
    NotConfigured,
    #[error("the command encoder is not recording (finished, or used by a pass)")]
    EncoderInvalid,
}

/// A training resolution for `frame_dimensions` (`ComputeIdealTrainingDimensions`).
/// 0 for `num_training_iterations` or `avg_training_vertices_per_path` uses
/// the library's defaults.
pub fn compute_ideal_training_dimensions(
    frame_dimensions: [u32; 2],
    num_training_iterations: u32,
    avg_training_vertices_per_path: f32,
) -> Result<[u32; 2], NrcError> {
    library::ensure_loaded()?;
    let mut out = [0u32; 2];
    // SAFETY: the DLL is loaded; both arrays hold two u32s.
    unsafe {
        sys::nrc_shim_compute_ideal_training_dimensions(
            frame_dimensions.as_ptr(),
            num_training_iterations,
            avg_training_vertices_per_path,
            out.as_mut_ptr(),
        )
    };
    Ok(out)
}
