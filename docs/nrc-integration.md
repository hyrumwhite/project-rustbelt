# Exploration: NVIDIA's Neural Radiance Cache (NRC) in Solarik

*5 November 2026. From NVIDIA's [NRC integration guide](https://github.com/NVIDIA-RTX/RTXGI/blob/main/Docs/NrcGuide.md), the SDK headers (v0.16, dated 1 October 2026, `NVIDIA-RTX/NRC`), the RTXGI sample's `NrcIntegration.cpp`, the strings inside `NRC_Vulkan.dll`, and Solarik's code. The probe script below was run against the DLL on this machine (RTX 5080, driver 616.56): it accepts the driver, and the numbers in the SDK section are its.*

## Verdict

Feasible, and it attacks the right thing. NRC would replace what the world cache does for ReSTIR GI's sample points (the "radiance at the end of the short path"), which is where most of the remaining boiling comes from: cells born from one sample, young cells in motion, missed and leaked cells, and cell noise feeding the GI reservoirs' weights. A neural cache has no cells, so none of those failure modes exist; its output is smooth in space and time, which also steadies the ReSTIR GI target function.

It is not free. Expect about 2.3 to 3.0 ms more GPU at 1280x720 DLAA (query and training around 1.5 ms, a training path tracer around 1 ms, resolve 0.1 ms, minus about 0.5 ms of world cache GI work that goes away), which fits the 60 frames/s budget at every spot (worst today: the b40 shaft at 10.06 ms) but spends most of the headroom. It ships 120 MB of extra DLLs and is labelled experimental by NVIDIA. It does not touch the direct-lighting side of the boiling (the shaft's dropped picks), and it brings its own artifacts: slow low-frequency drift while the network trains, warm-up after a level load, and blur of small dim features.

About two to three weeks of work; a two-day spike first settles the one unknown left, the fixed cost of `QueryAndTrain`.

## What NRC is, against what Solarik has

| | Solarik's world cache | NRC |
|---|---|---|
| Store | hash grid of cells (1M), keyed by quantized position and normal, LOD by distance | a small MLP (multi-resolution hash encoding + fully fused network) trained every frame on Tensor Cores |
| Stores | incident irradiance per cell (32-sample history), plus 4 direct light slots per cell | outgoing radiance for (position, normal, view direction, roughness, albedo, specular); or irradiance with `learnIrradiance` |
| Update | cells selected by priority, 1 GI ray + 4 shadow-tested DI samples per cell per frame; cells die after 10 frames unqueried | a low-resolution path tracer (about 2% of the pixels, up to 8 vertices) writes training paths; 4 training iterations per frame |
| Query | a hash lookup, 3 search steps, blend with neighbours | a batched network inference over all queries of the frame, in one call |
| Known trouble | birth noise, young cells in motion, key misses, leaks, cost scales with cells touched | drift while training, bias on small features, warm-up after reset, fixed cost per frame |

Where the query would go (the places that call `query_world_cache` today):

- `restir_gi.wesl` `generate_initial_reservoir`: the one-bounce sample point's radiance. This is the main consumer and the one that matters for boiling.
- `specular_gi.wesl` `trace_unfogged_glossy_path`: rough hits after a glossy bounce (`cache_rough_hits`) and the third vertex. NRC's view-dependent radiance fits these better than irradiance does.
- `world_cache_update.wesl` `sample_gi`: the cache's own GI rays. Goes away if the GI part of the cache goes away.

What stays: the world cache's DI side. The DI pass takes 8 of its 40 candidates from the cell's light slots (`DI_CELL_LIGHTS`), and that is unrelated to NRC. So the cache keeps `sample_di` and the slots; `sample_gi` and the GI blend can be dropped (about 0.5 ms of the 1.2 to 1.7 ms world cache pass), or kept as a fallback during NRC's warm-up.

## The SDK as shipped

- **Version** 0.16 (`NRC_VERSION_DATE "1 October 2026"`), RTXGI 2.8.0. Binary only, "experimental mode and actively developed". The RTXGI repo holds it as a submodule, `NVIDIA-RTX/NRC`, and the binaries are Git LFS objects (clone needs `git lfs`, or fetch from `media.githubusercontent.com`).
- **Files**: `Include/` (`NrcVk.h`, `NrcD3d12.h`, `NrcCommon.h`, `NrcStructures.h`, `Nrc.hlsli`, `NrcHelpers.hlsli`, `NrcSecurity.h`), `Lib/x64/NRC_Vulkan.lib`, `Bin/x64/`:

| DLL | Size | Role |
|---|---|---|
| `NRC_Vulkan.dll` | 5.8 MB | the library (tiny-cuda-nn inside; names `nvcuda64.dll`, the driver's CUDA) |
| `cudart64_13.dll` | 0.55 MB | CUDA 13 runtime |
| `nvrtc64_130_0.dll` | 106 MB | CUDA runtime compiler: the network kernels are built at run time (the DLL only carries `sm_75` and `sm_86`; a 5080 is newer) |
| `nvrtc-builtins64_134.dll` | 7.2 MB | its builtins |

  `dist\halo_rt.exe` is 224 MB today; bundling these takes it to about 345 MB. The first run on a machine compiles kernels (time unknown; `GlobalSettings::depsDirectoryPath` points at where the DLLs live).
- **GPU and driver**: NVIDIA Turing or newer; the RTXGI readme says driver 617.42 or newer, but `Initialize` returned `OK` on this machine's **616.56** (RTX 5080), so no update is needed for the spike. `Initialize` loads CUDA through the driver and needs `nvrtc64_130_0.dll` in `depsDirectoryPath`; without it the log says "CUDA libraries cannot be loaded" and it returns `InternalError`.
- **Vulkan**: `Context::Create(VkDevice, VkPhysicalDevice, VkInstance)`. What the DLL asks for (from its own `GetVulkan*` functions): instance extension `VK_KHR_get_physical_device_properties2`; device extensions `VK_NVX_binary_import`, `VK_NVX_image_view_handle` (how CUDA kernels get launched from a Vulkan command buffer), `VK_EXT_memory_budget`, `VK_KHR_buffer_device_address`; device features `VkPhysicalDeviceUniformBufferStandardLayoutFeatures` and `VkPhysicalDeviceScalarBlockLayoutFeatures` (both Vulkan 1.2 core features, enabled through `pNext`). `vulkaninfo` here lists all of them on 616.56. No external memory or semaphore extensions, so the buffers are plain Vulkan buffers with device addresses.
- **Training size and memory** (measured with `GetBuffersAllocationInfo` and `ComputeIdealTrainingDimensions`, 1 spp): the training resolution is a constant 197x111 = 21,867 paths at every frame size with the default 4 iterations (139x78 = 10,842 with 2), so at 1280x720 the training pass covers 2.4% of the pixels.

| 1280x720, 8 vertices | element | count | MB |
|---|---|---|---|
| QueryPathInfo | 8 | 921,600 | 7.4 |
| TrainingPathInfo | 8 | 21,867 | 0.2 |
| TrainingPathVertices | 48 | 174,936 | 8.4 |
| TrainingRadiance | 12 | 174,936 | 2.1 |
| TrainingRadianceParams | 56 | 174,936 | 9.8 |
| QueryRadiance | 12 | 945,515 | 11.4 |
| QueryRadianceParams | 56 | 945,515 | 53.0 |
| total | | | **92.1** |

  With 4 vertices 82.0 MB; at 1920x1080 with 8 vertices 179.7 MB. `Counter` is 2 x u32 on a readback heap; the radiance buffers are on the shared heap (CUDA-visible).
- **License**: the same "NVIDIA RTX SDKs" license as the DLSS runtime the build already ships (RTXGI's `License.md`): object-code distribution inside an application with material functionality of its own is allowed; modified sample source needs the "This software contains source code provided by NVIDIA Corporation" notice. Same footing as `nvngx_dlss.dll`.

## How it would attach to wgpu and Bevy

Everything DLSS needed, NRC needs too, and the hooks are all there:

1. **Device creation.** Bevy's `RawVulkanInitSettings::add_create_device_callback` (what `DlssInitPlugin` uses) gives `CreateDeviceCallbackArgs` with the extension list, the feature structs and the `pNext` chain. Push NRC's device extensions and enable `scalarBlockLayout` (a Vulkan 1.2 feature, in `pNext`) and `bufferDeviceAddress` (wgpu already turns it on for ray queries and creates every buffer with `SHADER_DEVICE_ADDRESS`). Same for the instance callback.
2. **Context.** `device.as_hal::<Vulkan>()` gives `raw_device()`, `raw_physical_device()` and `shared_instance().raw_instance()`, which is all `Context::Create` takes.
3. **Buffers.** Two ways, both supported by wgpu 30: let the SDK allocate (`enableGPUMemoryAllocation = true`) and wrap its `VkBuffer`s with `Device::create_buffer_from_hal` so WESL can bind them; or create them in wgpu from `GetBuffersAllocationInfo` and hand `raw_handle()` plus `vkGetBufferDeviceAddress` to `Configure`. The second keeps wgpu's resource tracking honest and is what the sample does under `g_enableSDKMemoryAllocation = false`.
4. **Per frame**, inside Solarik's render node, at points where no compute pass is open (the node already closes its pass around `primary_glass`): `command_encoder.as_hal_mut::<Vulkan>` for `BeginFrame` (clears the counters) and later `QueryAndTrain`. wgpu does not know what the library's commands read and write, so bracket them with full memory barriers, as `dlss_wgpu` does for Ray Reconstruction. `PopulateShaderConstants` fills a 96-byte struct that goes into a uniform.
5. **After submit**, `EndFrame(VkQueue)` on `queue.as_hal::<Vulkan>().raw_queue()`. Bevy submits at the end of the render graph, so this is a system ordered after the graph runs, late in the render schedule.
6. **Binding crate.** The API is C++ (classes, namespaces, references; the exports are mangled `?Create@Context@vulkan@nrc@@...`), so a thin C shim compiled with the `cc` crate (MSVC) and linked against `NRC_Vulkan.lib`, exposed to Rust as a small `nrc_wgpu` crate modelled on `dlss_wgpu`: `register_*_extensions`, `NrcContext::new`, `configure`, `begin_frame`, `query_and_train`, `end_frame`. Signature verification (`NrcSecurity.h`, WinVerifyTrust) is a few lines of `windows-sys`.
7. **Bundle.** `src/bundle.rs` unpacks DLSS's DLLs into `%LOCALAPPDATA%\halo_rt\runtime-<hash>`; the four NRC DLLs join them and `depsDirectoryPath` points there.

## The shader side

`Nrc.hlsli` and `NrcHelpers.hlsli` are HLSL (about 1,200 lines, most of it comments and stubs). The functions the path tracer calls are small and port to WESL by hand; the library's own kernels are inside the DLL, so nothing else is needed:

- Packing: LogLuv HDR (`NrcEncodeLogLuvHdr`), octahedral normals in 2x16, R11G11B10, spherical unorm directions. About 150 lines.
- Records: `NrcRadianceParams` (56 bytes, tightly packed: position, roughness, normal, view direction as 2x unorm pairs, albedo, specular), `NrcPackedPathVertex` (48 bytes: 7 u32 + position), `NrcPackedQueryPathInfo` (8 bytes), `NrcPackedTrainingPathInfo` (8 bytes), the counters. The library reads them with DX scalar layout (`-fvk-use-dx-layout`), and a WGSL storage struct would pad every `vec3` to 16 bytes, so declare these buffers as `array<u32>` / `array<f32>` and index by hand.
- Path logic: `NrcCreatePathState`, `NrcUpdateOnHit` (spread-radius heuristic, the training vertex write, the query record), `NrcUpdateOnMiss`, `NrcSetBrdfPdf`, `NrcWriteFinalPathInfo` (the self-training query at the last vertex, the path info). About 250 lines.
- Position encoding: `NrcEncodePosition` is a scale and bias from the scene bounds in `ContextSettings` (a Halo BSP's bounds are known at load; a level change reconfigures, which is allowed to hitch there).

## The passes, in order

```
presample light tiles, sky
world cache: compact, priority, sample_di (+ sample_gi while it is the fallback), blend
NRC BeginFrame                                   (raw command buffer)
NRC training pass        NEW   197x111 = 21,867 paths (2.4% of 1280x720), up to 8 vertices,
                               NEE at every vertex, BRDF sampling from brdf.wesl, writes training vertices
direct lighting (ReSTIR DI)
GI initial               SPLIT the current initial_and_temporal: trace the sample ray, write the
                               query record for the sample point, keep the query index in the reservoir
specular GI              writes query records at rough hits instead of reading the cache
NRC QueryAndTrain                                (raw command buffer; the network runs here)
NRC resolve              NEW   a WESL compute pass: reservoir.radiance = QueryRadiance[index]
                               * radianceUnpackMultiplier; specular adds throughput * radiance
GI temporal + spatial + shade
DLSS-RR
NRC EndFrame                                     (after submit)
```

The split is the one structural change: temporal resampling needs the sample's radiance for its target function, and the radiance is only known after `QueryAndTrain`. Reservoirs are already in a buffer, so the temporal step moves to the start of the spatial pass (one more read of the reservoir buffer per pixel, under 0.1 ms).

Settings to start from: `learnIrradiance = true` (the consumers multiply by `base_color / PI` themselves, as they do with the cache today, so hand NRC albedo 1 and specular 0 at the query, or divide in resolve), `includeDirectLighting = true` (the sample point's direct light is in the cache today too), `maxPathVertices = 8`, termination heuristic off for the GI query (the sample point is already one diffuse bounce in; NRC's `createQuery = vertexCount == 0` path), on for the training pass (`trainingTerminationHeuristicThreshold = 0.1`), `proportionUnbiased = 1/16`. Sky: Solarik's sky is sampled analytically, so training paths that escape add `sample_sky` as their final radiance and set the exited flag.

## Cost

| Item | Estimate at 1280x720 DLAA | Basis |
|---|---|---|
| `QueryAndTrain` | **0.77 to 0.91 ms, measured** (3 to 8 vertex paths, 0.92M queries; see the spike sections) | was estimated at 1.0 to 1.5 from NVIDIA's paper: 2.6 ms at 1080p on an RTX 3090 |
| training path tracer | 0.7 to 1.1 ms | 21,867 paths x ~3 vertices x (1 ray + 1 shadow ray) is about 130k rays; the cache's `sample_gi` + `sample_di` do ~200k rays in 1.2 to 1.7 ms |
| resolve + reservoir split | ~0.1 ms | one buffer read and write per pixel |
| world cache GI removed | -0.4 to -0.6 ms | `sample_gi` and the blend |
| net | +1.6 to +2.2 ms (was +2.3 to +3.0 before the spike) | |

Against today's table: shaft 10.06 to about 12.8, canyon 8.69 to about 11.4, hall 6.47 to about 9.2 ms, all under 16.7 ms. DLSS Performance at 1440p would not fit with it. Numbers to be replaced by the spike's measurements.

## What it would and would not fix

Would fix, by construction:
- cell birth and young-cell flicker in motion (the shaft's 0.128 three-shot spread in motion, the canyon from a Banshee);
- the lookup residual (13% of shaft lookups still miss their cell) and leaked cells;
- cell noise in the GI reservoirs' weights, which is the blotch wander in dim halls;
- view-dependent radiance for the specular GI's rough hits.

Would not fix:
- direct-lighting picks (the shaft's strips behind perforated covers: 91% of DI picks dropped). That stays a DI problem;
- anything from the raster side (bump-mapped normals in the G-buffer).

Brings its own:
- **drift**: the network keeps training, so a static view slowly breathes at low frequency. NVIDIA damps it with `selfTrainingAttenuation` and the 4 iterations per frame; the harness's three-shot spread measures exactly this and must gate it;
- **warm-up**: after `Configure` (level load) the cache is wrong for a second or so. Keep the world cache's GI as the source for the first N frames, or blend by frame age;
- **bias**: small, dim, high-contrast features blur (the `smallestResolvableFeatureSize` floor is 1 cm);
- **start-up**: NVRTC compiles the kernels per machine on first run;
- **debugging**: Nsight cannot see the network calls; the built-in `DirectCacheView` and heatmaps are the tools.

## Plan

1. **Probe (done, 5 November).** `scripts/nrc/probe.py` loaded the DLL without a device: `Initialize` OK on driver 616.56, the extension and feature lists and buffer sizes above.
2. **Spike (2 days).** `nrc_wgpu` crate with the C shim; device callbacks; `Create`, `Configure`, `BeginFrame`, `QueryAndTrain` with zero queries, `EndFrame`. Measure the fixed cost of `QueryAndTrain` on empty input and with the training pass sending a fixed number of synthetic paths. This decides go or no-go against the budget.
3. **Shader port (1 to 2 days).** The packing helpers and records in a `nrc.wesl`, validated against the DLL by writing known records and reading `QueryRadiance` back with the `DirectCacheView` resolve mode.
4. **Training pass (2 days).** A new compute entry: jittered G-buffer pixel at training resolution, bounce loop from `brdf.wesl`, NEE via `sample_random_light_transmitted`, records per vertex.
5. **GI query + resolve + pass split (2 days).** Behind `SOLARIK_NRC=1`, world cache GI as fallback. Specular GI's rough hits after.
6. **Measure (2 to 3 days).** `scripts/boil/run.sh nrc` against the `default` table; play A/B at the three spots; then decide whether to drop `sample_gi`.
7. **Bundle (half a day).** DLLs into `src/bundle.rs`, `depsDirectoryPath`, signature check, README license lines.

## Spike results (5 November)

**Binding crate `crates/nrc_wgpu`** (C++ shim compiled with `cc`, delay-loaded `NRC_Vulkan.dll`, WinVerifyTrust on the four DLLs, device-creation callbacks, app-managed buffers, per-frame calls; `cargo test -p nrc_wgpu --release -- --ignored --nocapture` is the smoke test). Measured on the RTX 5080, driver 616.56, Vulkan validation on, 1280x720, 197x111 training, 8 vertices:

| | |
|---|---|
| `QueryAndTrain` with zero queries and zero training records, GPU (Vulkan timestamps, median of 8 frames) | **1.24 ms** |
| the same call, CPU time to record | 0.35 ms (13 ms on the first frame) |
| `Configure` | 7.4 ms |
| buffers | 92.1 MB, as the probe said |

This empty-input number turned out to be a special case, not a floor: with real records the call is faster (0.77 ms at full 720p load; see "Synthetic-load spike" below, surprise 3).

What the crate had to do that the guide does not say:
- **Buffers are created with ash, not wgpu.** The vendored wgpu-hal only sets `SHADER_DEVICE_ADDRESS` on acceleration-structure buffers, so the crate creates each `VkBuffer` and its memory itself (with the usage `GetBufferUsageFlags` returns plus storage and transfer, `DEVICE_ADDRESS` allocation flag, host-visible for the Counter buffer the library flags for readback) and hands them to wgpu through `create_buffer_from_hal`, which then owns and frees them. Every buffer-sized-zero slot (the debug one) gets a 256-byte placeholder: the library writes a descriptor for all nine and a null buffer fails validation.
- **The library's own barriers name the ray-tracing shader stage**, which validation only allows with the `rayTracingPipeline` feature. The device callback enables `VK_KHR_ray_tracing_pipeline` when the GPU has it. Not in the SDK's extension list.
- **NRC commands need their own command encoder.** wgpu-core panics when raw Vulkan commands and wgpu commands share an encoder, so `begin_frame` and `query_and_train` take an encoder used for nothing else, submitted in order between the pass encoders. In Solarik's node that means the lighting pass gets split into encoders around the two calls.
- **The feature names the library returns are sType strings** (`VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_SCALAR_BLOCK_LAYOUT_FEATURES`), matched to the Vulkan 1.2 feature structs and switched on in the pNext chain (in place if wgpu already chained the struct, else added and leaked).
- **The library leaks its own Vulkan objects at shutdown** (55 after `Destroy` + `Shutdown`: CUDA modules and functions, a descriptor pool, a fence, 9 internal buffers). Harmless at process exit; a validation message on device destruction.
- **The game's `build.rs` must emit `cargo:rustc-link-arg-bins=/DELAYLOAD:NRC_Vulkan.dll`**, or the DLL becomes a load-time import that has to sit next to the exe (the crate's own flag reaches only its tests and examples).

## Plan B: take SHaRC's ideas without the network

RTXGI's other library, SHaRC, is a spatial hash radiance cache: structurally what Solarik's world cache already is. Its recent changes list what a hash cache needs that ours lacks, and each is a small, measurable change inside the existing estimator:
- resolve-time blending with the adjacent LOD level when the first lookup misses (our lookup residual item);
- an anti-firefly filter and a fade-acceleration path in the resolve (history reset when luminance keeps falling);
- 32-bit compact keys (`HASH_GRID_COMPACT`) for a cheaper probe.
These do not remove cells, so they do not address birth noise in motion the way NRC does, but they cost nothing to ship.

## Probe script

`scripts/nrc/probe.py` downloads nothing: the four DLLs from `NVIDIA-RTX/NRC` (LFS, `Bin/x64`) go in `scripts/nrc/bin/` (ignored by git; they are there now) and `python scripts/nrc/probe.py` prints the extension and feature lists, `ComputeIdealTrainingDimensions`, `GetBuffersAllocationInfo` per buffer, and the `Initialize` status on the installed driver. `Initialize` needs all four DLLs, not just the library and `cudart`.

## Sources

- [NRC integration guide](https://github.com/NVIDIA-RTX/RTXGI/blob/main/Docs/NrcGuide.md), [RTXGI readme and prerequisites](https://github.com/NVIDIA-RTX/RTXGI), [NRC library](https://github.com/NVIDIA-RTX/NRC) (headers v0.16).
- [Real-time Neural Radiance Caching for Path Tracing (Müller et al., 2021)](https://research.nvidia.com/labs/rtr/publication/muller2021nrc): the 2.6 ms at 1080p on an RTX 3090 figure.
- Driver 617.42 release: [TechSpot listing](https://www.techspot.com/drivers/driver/file/information/18443/).

## Synthetic-load spike (5 November)

`cargo run -p nrc_wgpu --release --example spike` (`crates/nrc_wgpu/examples/spike.rs` + `spike.wgsl`, records from `nrc_records`). It feeds the library a scene with a known answer: outgoing irradiance c(p) = 0.2 + 0.6 * p.x / 10, grey, in a 0..10 box, every surface albedo 1, specular 0, roughness 1. The training fill pass writes 197x111 paths of V vertices the way `NrcUpdateOnHit` (update mode) and `NrcWriteFinalPathInfo` do: random position, normal and view direction per vertex, each finalized with radiance c(p_i) and throughput 0 (so every vertex's target is its own c), a self-training query at the last vertex, packedData = V. The query fill pass writes one query per pixel at 1280x720 (random point, prefix throughput LogLuv(1)) and keeps 4,096 sampled positions; after `QueryAndTrain` a gather pass reads those answers back, unpacks them, and compares with c(p). Settings as in the plan: `learnIrradiance`, `includeDirectLighting`, 8 max vertices, `FrameSettings` defaults (4 iterations, max expected radiance 1). RTX 5080, driver 616.56, Vulkan validation on; 240 frames per run, a fresh context per run; GPU times are Vulkan timestamps, median of frames 40..240.

**Verdict: the records are right and the network learns the function. `QueryAndTrain` at full 720p load costs 0.77 ms (0.91 ms with 8-vertex paths), not the 1.8 to 2.5 ms guessed above. No validation errors during the frames.**

### GPU time per configuration (ms)

| run | queries/frame | training records/frame (library's count) | training fill | query fill | `QueryAndTrain` |
|---|---|---|---|---|---|
| 1. full load, 3 vertices | 943,467 | 22,948 | 0.032 | 0.196 | **0.771** |
| 2. queries only (`train_the_cache` off, no training fill, after 120 trained frames) | 921,600 | (22,994, from the stale buffers) | - | 0.192 | 0.485 |
| 3. training only (no pixel queries; the 21,867 self-training queries remain) | 21,867 | 22,948 | 0.031 | - | 0.473 |
| 4a. full load, 1 vertex | 943,467 | **0** | 0.024 | 0.197 | 1.258 |
| 4c. full load, 2 vertices | 943,467 | 1,075 | 0.028 | 0.196 | 0.746 |
| 4b. full load, 8 vertices | 943,467 | 132,308 | 0.053 | 0.197 | 0.910 |
| 1b. as 1, the fill's atomics on NRC's Counter buffer, one per workgroup | 943,467 | 22,948 | 0.046 | 0.587 | 0.772 |
| 1c. as 1, atomics on NRC's Counter buffer, one per thread | 943,467 | 22,948 | 0.046 | 1.160 | 0.770 |
| 1d. as 1, own device-local counter, one atomic per thread | 943,467 | 22,948 | 0.035 | 0.214 | 0.770 |

Roughly: inference over 0.92M queries 0.3 ms, the 4 training iterations 0.29 ms, and a fixed part of about 0.18 ms. (The fill passes do the minimum: a few hashes and the record writes; in Solarik the record writes fold into passes that already exist.)

### Convergence

Mean / 95th percentile absolute error of the unpacked radiance against c(p) over the 4,096 sampled queries. The error of the best constant (the flat average) is 0.15; "fit" is the least-squares line predicted = a c + b at frame 240 (a flat answer would show a = 0).

| run | frame 10 | 30 | 60 | 120 | 240 | fit at 240 | bias, frames 120..240 |
|---|---|---|---|---|---|---|---|
| 1. full load, 3 vertices | 0.305 / 0.570 | 0.129 / 0.277 | 0.044 / 0.119 | 0.040 / 0.068 | **0.0064 / 0.020** | 0.975c + 0.010 | -0.040 .. -0.002 |
| 2. queries only (frozen after 120 frames) | 0.039 / 0.066 | 0.039 / 0.067 | 0.038 / 0.064 | 0.039 / 0.066 | 0.039 / 0.067 | 0.923c + 0.000 | -0.039 .. -0.037 |
| 3. training only (error at the self-training queries) | 0.295 / 0.564 | 0.126 / 0.271 | 0.043 / 0.122 | 0.039 / 0.067 | 0.0068 / 0.020 | 0.973c + 0.011 | |
| 4a. 1 vertex | 0.485 / 0.744 | 0.498 / 0.760 | 0.497 / 0.765 | 0.499 / 0.774 | 0.497 / 0.770 | 0.000c + 0.000 | never trains |
| 4c. 2 vertices | 0.308 / 0.571 | 0.117 / 0.277 | 0.039 / 0.117 | 0.021 / 0.059 | 0.010 / 0.027 | 0.985c + 0.010 | |
| 4b. 8 vertices | 0.300 / 0.565 | 0.130 / 0.276 | 0.045 / 0.123 | 0.037 / 0.062 | 0.0069 / 0.021 | 0.972c + 0.011 | |
| S. full load, error at the training positions | 0.297 / 0.565 | 0.128 / 0.278 | 0.041 / 0.115 | 0.039 / 0.066 | 0.0072 / 0.021 | 0.968c + 0.014 | |

Run 1 at intermediate frames: MAE 0.034 at 90, 0.030 at 150, 0.013 at 180, 0.0066 at 210. The shape is the same in every run: the slope is learned in about 60 frames (a = 0.72 at frame 60, 0.93 at 120), then a global offset of about -0.04 decays over the next 60 to 100 frames. How far that offset has gone by frame 120 varies from run to run (MAE 0.010 to 0.040 at frame 120 across repeats; the record order changes with the atomics, so runs are not bit-identical). Call it **2.5 to 3 seconds at 60 frames/s to converge from a cold start**, and about 1 second before the shape is right.

- **Responsiveness** (run 5: c(p) flipped to 0.8 - 0.6 p.x / 10 at frame 120): MAE 0.29 on the first frame after, 0.23 after 8 frames, 0.19 after 16, 0.13 after 32, 0.047 after 64, **under 0.02 after 89 frames** (1.5 s), 0.011 after 119. A lighting change (a door opening, a light switching) takes about a second and a half to settle.
- **Stability at fixed points** (run 6: the same 921,600 query points every frame): the mean frame-to-frame change of a point's answer is 0.0147 in frames 2..30, 0.00088 in 30..120 and **0.00049 in 120..240**, about 0.1% of the signal per frame. Converged, the cache does not boil; what is left is the slow global offset drift (a few hundredths over a hundred frames).
- **The position encoding is right**: the error at the training positions themselves (run S) equals the error at random pixel positions, and the fit slope is about 0.97 in every trained run.

### The unpack multiplier

`radiance_unpack_multiplier` is **0.05** with `max_expected_average_radiance_value` = 1, and 0.025 at 0.5, 0.2 at 4: it is **max expected radiance / 20**. The query buffer holds radiance / multiplier (about 4 to 16 for this scene), and `NrcUnpackQueryRadiance` (`packed * radianceUnpackMultiplier`; the `NrcHelpers.hlsli` comment says it "puts them back into the correct value range") gives the real value. **With it, the unpacked mean matches c with no extra factor** (frame 240: mean 0.4950 against 0.4972 wanted; 0.4973 against 0.4972 at max radiance 4). Read it after `BeginFrame` with the frame's settings; it did not change between frames.

The setting also changes training speed: at 4.0 (internal values 1 to 4) the frame-60 error is 0.024 instead of 0.044, and the frame-240 error is similar (0.010). Worth tuning against Halo's actual radiance range.

### Surprises

1. **The Counter buffer is in host memory, and every shader atomic on it is a PCIe round trip.** The library asks for a readback heap for `Counter`, so `nrc_wgpu` puts it in host-visible, host-cached memory. Incrementing it from a shader costs about 40 ns per atomic: 1.16 ms for 921,600 per-thread atomics (the driver merges them per warp: 28,800 round trips) and 0.59 ms with one atomic per 64-thread workgroup. **Fix used by the spike, no crate change**: the fill passes count on their own 8-byte device-local buffer (cleared each frame, as `BeginFrame` clears NRC's), then one `copy_buffer_to_buffer` of the 8 bytes into NRC's Counter before `QueryAndTrain`. The query fill drops to 0.196 ms (0.214 with per-thread atomics). Solarik must do the same; using the library's Counter directly would add 0.6 to 1.2 ms.
2. **The last vertex of a training path is never trained on.** The library's own TrainingRecords count (Counter[1], read back after `QueryAndTrain`) is 5% of the vertex-0s (`proportionPrimarySegmentsToTrainOn` = 0.05) plus every vertex from 1 to V-2: 22,948 for V = 3 (21,867 + about 1,090), 132,308 for V = 8, 1,075 for V = 2, **0 for V = 1**. The last vertex only anchors the self-training query. Paths that exited the scene do train their last vertex (25% exited gave 28,363 records, as predicted). So **a 1-vertex training path teaches nothing**, even with the primary proportion at 1.0, and the training pass must trace at least two vertices, preferably three. Solarik's training pass must not stop at the first hit.
3. **With zero training records `QueryAndTrain` gets slower**: 1.26 ms against 0.77 ms. That is the 1.24 ms the empty smoke test measured, so the "fixed cost" in the spike results above was this empty-input path, not a floor. With real input the whole call is 0.77 ms.
4. **`report_loss` is broken on Vulkan** (SDK 0.16): passing a loss pointer makes `QueryAndTrain` return `InternalError`, and the log says `tiny-cuda-nn/reduce_sum.h:134 cudaMemcpyAsync(... DeviceToHost ...) failed: operation not supported`. A synchronous device-to-host copy cannot happen inside a Vulkan-recorded CUDA launch. No training loss is available; the readback error against a known answer (this spike) or `DirectCacheView` are the substitutes. The example skips it unless `SPIKE_REPORT_LOSS=1`.
5. **`train_the_cache = false` still generates training records** from whatever is in the training buffers (22,994 from the warm-up's stale paths with no training fill that frame), and the call costs 0.485 ms; with training off it is mostly inference.
6. **Frozen weights show the warm-up state**: run 2 keeps the frame-120 network, error 0.039 with the -0.04 offset still in it. That is what a game frame sees for the first two seconds after a level load, which supports keeping the world cache's GI as the warm-up fallback.

What this changes in the cost table: `QueryAndTrain` 0.77 to 0.91 ms instead of 1.0 to 1.5, plus the record writes inside the GI and training passes, plus the training pass's own tracing (not measured here). The net estimate drops from +2.3 to 3.0 ms to roughly **+1.6 to 2.2 ms**.

Knobs: run names as arguments run a subset (`full queries training v1 v2 v8 switch atomic atomic1 device1 static sanity`); `SPIKE_EXIT_FRACTION` (default 0; 0.25 tested: no validation errors, converges to 0.014), `SPIKE_MAX_RADIANCE`, `SPIKE_PRIMARY`, `SPIKE_REPORT_LOSS`. The whole set runs in about 6 seconds. Each frame the example asserts the Queries counter against the expected count and the QueryRadiance capacity (945,515), and before teardown it asserts no validation, wgpu or NRC errors (the library's known leak report at device destruction follows).
