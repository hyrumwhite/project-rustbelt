# nrc_wgpu

NVIDIA's Neural Radiance Cache (NRC, Vulkan) on wgpu, modelled on `dlss_wgpu`.
The library's API is C++ (`NrcVk.h`); `shim/nrc_shim.cpp` wraps it in plain C
functions (built by the `cc` crate with MSVC) and `src/` wraps those for Rust.
Windows x64 (MSVC) only.

Subject to the NVIDIA RTX SDKs license, `vendor/NRC/License.md`.

## SDK

`build.rs` takes the SDK from the `NRC_SDK` environment variable, defaulting to
`<repo>/vendor/NRC` (gitignored). It needs `Include/` (NrcVk.h, NrcCommon.h,
NrcStructures.h), `Lib/x64/NRC_Vulkan.lib` and `Bin/x64/NRC_Vulkan.dll`. The
Vulkan SDK is not needed: `shim/vk_stub/vulkan/vulkan.h` declares the few
handle types NrcVk.h uses, with the same definitions as `vulkan_core.h` (they
are part of the mangled export names, so a mismatch fails at link time).

## DLLs at run time

Four DLLs must be in one directory: `NRC_Vulkan.dll`, `cudart64_13.dll`,
`nvrtc64_130_0.dll` and `nvrtc-builtins64_134.dll` (about 120 MB; NVRTC
compiles the network kernels on first use).

`NRC_Vulkan.dll` is **delay-loaded** (`/DELAYLOAD:NRC_Vulkan.dll`, plus
`delayimp.lib`), so a program starts without it on the search path. Call
`nrc_wgpu::load_library(dir, verify_signature)` before anything else: it
checks the DLLs' Authenticode signatures (WinVerifyTrust, as the SDK's
`NrcSecurity.h` does), then loads `dir\NRC_Vulkan.dll` with
`LOAD_WITH_ALTERED_SEARCH_PATH` so `cudart64_13.dll` is found beside it. The
first NRC call then binds to that module. `NrcInitSettings::deps_directory`
(`GlobalSettings::depsDirectoryPath`) tells the library where the NVRTC DLLs
are; normally the same directory. `nrc_wgpu::sdk_bin_dir()` is
`<NRC_SDK>\Bin\x64` from build time, which the tests use.

`cargo:rustc-link-arg` only reaches this package's own tests and examples. A
binary in another package (the game) must pass the flag itself, e.g. in its
`build.rs`:

```rust
println!("cargo:rustc-link-arg-bins=/DELAYLOAD:NRC_Vulkan.dll");
```

Without it the DLL becomes a load-time import and must sit next to the exe.

## Use

```rust
nrc_wgpu::load_library(&dll_dir, true)?;
let instance = nrc_wgpu::create_instance(&instance_descriptor)?;        // or register_instance_extensions in a callback
let (device, queue) = nrc_wgpu::request_device(&adapter, &desc, None)?; // or register_device_extensions
let nrc = nrc_wgpu::Nrc::initialize(&nrc_wgpu::NrcInitSettings::new(&dll_dir))?;
let mut context = nrc_wgpu::NrcContext::new(&nrc, &device)?;
context.configure(&settings)?;            // allocates the buffers: context.buffers()
let constants = context.shader_constants()?; // 96-byte NrcConstants (Pod) for a uniform

// per frame; `nrc_encoder` holds only NRC commands (wgpu forbids mixing raw
// and wgpu commands in one encoder)
context.begin_frame(&mut nrc_encoder_a, &frame_settings)?;
// ... passes writing query and training records ...
context.query_and_train(&mut nrc_encoder_b, false)?;
queue.submit([/* ..., */ nrc_encoder_a.finish(), /* passes, */ nrc_encoder_b.finish() /* , ... */]);
context.end_frame(&queue)?;
```

What the device gets: the library's instance and device extensions
(`VK_KHR_get_physical_device_properties2`; `VK_NVX_binary_import`,
`VK_NVX_image_view_handle`, `VK_EXT_memory_budget`,
`VK_KHR_buffer_device_address`) and features (scalar block layout, uniform
buffer standard layout), queried from the library at run time; buffer device
address; and `VK_KHR_ray_tracing_pipeline` with its feature when the GPU has
it, because the library's own barriers name the ray tracing shader stage
(validation error otherwise). Feature structs go into the device create
info's pNext chain (switched on in place when already there, else added and
leaked: they must outlive device creation).

Buffers are app-managed (`enableGPUMemoryAllocation = false`): for each
`BufferIdx` the crate creates a `VkBuffer` with the usage the library asks
for plus storage, transfer and device address, allocates memory with
`VK_MEMORY_ALLOCATE_DEVICE_ADDRESS_BIT` (host-visible for the readback
counter), and hands both to wgpu (`Buffer::from_raw_managed` +
`create_buffer_from_hal`), which frees them. Plain wgpu storage buffers would
not do: wgpu-hal only adds `SHADER_DEVICE_ADDRESS` for acceleration-structure
usages. Buffers the settings don't need get a small placeholder, because the
library writes a descriptor for every one.

## Tests

- `cargo test -p nrc_wgpu --release --lib`: the `#[repr(C)]` mirrors against
  the C++ compiler's sizes and offsets (no GPU or DLL needed).
- `cargo test -p nrc_wgpu --release -- --ignored --nocapture`: headless
  smoke test on an NVIDIA GPU with validation on: device, context at
  1280x720, ten frames of BeginFrame + QueryAndTrain (no records) + submit +
  EndFrame, with CPU and GPU (timestamp) times per QueryAndTrain.

Known: `Context::Destroy` and `Shutdown` leave the library's internal Vulkan
objects alive (CUDA modules/functions, a descriptor pool, a fence, nine
buffers), which validation reports at device destruction. The smoke test
prints that instead of failing on it.
