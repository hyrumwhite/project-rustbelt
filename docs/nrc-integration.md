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

## Integration design (5 November)

The first Solarik step: the diffuse GI query answered by NRC, a training pass, the pass split, behind a Cargo feature and an environment variable. The world cache keeps running (its DI slots feed ReSTIR DI; its GI side is the fallback during NRC's warm-up and the A/B baseline). Specular GI and the glass panes stay on the world cache for now.

### Switches

- Cargo feature `nrc` on `bevy_solarik` (`dep:nrc_wgpu`, `dep:nrc_records`, both path dependencies on `../crates/...`) and on the game (`nrc = ["bevy_solarik/nrc"]`). The game's `build.rs` passes `cargo:rustc-link-arg-bins=/DELAYLOAD:NRC_Vulkan.dll` with the feature (nrc_wgpu's own link arg reaches only its own binaries).
- At run time `SOLARIK_NRC=1`: the game adds `SolarikNrcInitPlugin` before `DefaultPlugins` (it registers the Vulkan creation callbacks, so it must come before `RenderPlugin`). Without it nothing NRC-related runs and the shaders compile as before (no `NRC` shader def).
- `SOLARIK_NRC_DIR`: the directory with the four DLLs; default `nrc_wgpu::sdk_bin_dir()` (`vendor/NRC/Bin/x64` on the build machine). Bundling into `src/bundle.rs` is a later step (the DLLs are 120 MB).
- `SOLARIK_NRC_WARMUP`: frames after `Configure` during which the GI reads the world cache instead (default 120).
- `SOLARIK_SHADER_DEFS=VISUALIZE_NRC`: the resolve pass writes the query radiance to the view (a direct cache view).

### Rust, in `bevy_solarik/src/realtime/nrc.rs` (feature `nrc`)

The crate denies `unsafe_code`; the callback registration and the `as_hal` calls get item-level `#[allow(unsafe_code, reason = ...)]`, as bevy_anti_alias's DLSS module does.

- `SolarikNrcInitPlugin` (main app, before `RenderPlugin`): `nrc_wgpu::load_library(dir, true)`; on failure log and return. Then `RawVulkanInitSettings::add_create_instance_callback` → `nrc_wgpu::register_instance_extensions(args)`, and `add_create_device_callback` → `nrc_wgpu::register_device_extensions(args, adapter)`; on success `additional_features.insert::<NrcSupported>()`, on failure remove it and log. Inserts `NrcLibraryDir(PathBuf)` into the main world.
- `SolarikNrcSceneBounds { min: Vec3, max: Vec3 }`: a main-world resource the application sets (every surface a ray can hit lies inside), extracted with `ExtractResourcePlugin`. Absent: ±1000 m and one warning.
- `NrcSupported`: the marker type for `AdditionalVulkanFeatures`.
- `NrcState` (render world resource): `Arc<Nrc>`, `NrcContext`, the `ContextSettings` last configured, `configured_frame: Option<u32>` (the `FrameCount` at the last configure), `constants: Buffer` (96 bytes, `UNIFORM | COPY_DST`), `counters: Buffer` (8 bytes, `STORAGE | COPY_SRC | COPY_DST`; the device-local counters the shaders add to; see "Spike results"), `bind_group_layout: BindGroupLayoutDescriptor`, `bind_group: Option<BindGroup>` (rebuilt on configure, the buffers may be new), `frame_settings: FrameSettings`, `training_dimensions: [u32; 2]`, `warmup_frames: u32`, `recorded_this_frame: bool`.
- `init_nrc` (RenderStartup, chained before `init_solari_lighting_pipelines`): if `AdditionalVulkanFeatures::has::<NrcSupported>()` and the library dir resource exists: `Nrc::initialize(&NrcInitSettings::new(dir))`, `NrcContext::new(&nrc, render_device.wgpu_device())`, the two buffers, the layout; insert `NrcState`. Any error: log at error level, no resource, the renderer runs without NRC.
- `prepare_nrc` (Render, `PrepareResources`, after `prepare_solari_lighting_resources`): for the view with `SolarikLightingResources`, the wanted `ContextSettings { learn_irradiance: true, include_direct_lighting: true, request_reset: bounds changed, scene_bounds from SolarikNrcSceneBounds, smallest_resolvable_feature_size: 0.01, frame_dimensions: view_size, training_dimensions: compute_ideal_training_dimensions(view_size, 0, 0.0), samples_per_pixel: 1, max_path_vertices: 8 }`. When it differs from the last (ignoring `request_reset`), or the bounds changed: `context.configure(&settings)` (hitches: allowed), rebuild the bind group, set `configured_frame`. One view only (the first); more views log a warning once.
- `nrc_end_frame` (Render, after `RenderSystems::Render`, i.e. after the graph's command buffers were submitted): if `recorded_this_frame` → `context.end_frame(&render_queue)`; clear the flag. (`RenderContextState::queue` appends each render system's command buffers to `PendingCommandBuffers`; the submit is in `render_system`, in the `Render` set.)
- `frame_settings`: the defaults, `max_expected_average_radiance_value` 1.0 (the shaders hand NRC exposed radiance, see below), `train_the_cache: true`, `termination_heuristic_threshold` and `training_termination_heuristic_threshold` 0.1, `proportion_unbiased` 1/16, `num_training_iterations` 4.

#### The NRC bind group (group 2 of the pipelines that use it)

| binding | WGSL | buffer |
|---|---|---|
| 0 | `var<uniform> nrc_constants: NrcConstants` | `NrcState::constants`, written each frame after `BeginFrame` from `PopulateShaderConstants`, with `pad0 = 1` when NRC answers are to be used (frames since configure ≥ warm-up) else 0 |
| 1 | `var<storage, read_write> nrc_query_path_info: array<u32>` | `BufferIdx::QueryPathInfo` |
| 2 | `var<storage, read_write> nrc_training_path_info: array<u32>` | `BufferIdx::TrainingPathInfo` |
| 3 | `var<storage, read_write> nrc_training_path_vertices: array<u32>` | `BufferIdx::TrainingPathVertices` |
| 4 | `var<storage, read_write> nrc_query_radiance_params: array<u32>` | `BufferIdx::QueryRadianceParams` |
| 5 | `var<storage, read_write> nrc_counters: array<atomic<u32>>` | `NrcState::counters` (ours, device local) |
| 6 | `var<storage, read> nrc_query_radiance: array<f32>` | `BufferIdx::QueryRadiance` |

All compute stage, `storage_buffer_sized(false, None)` / `storage_buffer_read_only_sized(false, None)` / `uniform_buffer::<NrcConstants>(false)` (no dynamic offset).

#### Pipelines (in `init_solari_lighting_pipelines`, when `NrcState` exists)

- `gi_initial_and_temporal_pipeline`: the same entry (`initial_and_temporal`, restir_gi.wesl) with the extra def `NRC` and the NRC layout as its third bind group layout; with `NRC` the entry does the initial sampling only.
- `nrc_training_pipeline`: `nrc_training.wesl`, entry `nrc_train`, defs `NRC`, `FOLIAGE_TRANSMISSION`, NRC layout at group 2. Dispatched `(training_dimensions.x.div_ceil(8), training_dimensions.y.div_ceil(8), 1)`.
- `nrc_resolve_pipeline`: restir_gi.wesl, entry `nrc_resolve_and_temporal`, def `NRC`, NRC layout at group 2. Dispatched `(dx, dy, 1)`.
- `prepare_primary_glass` waits for them too.

#### The node, per frame (the non-primary `solarik_lighting`), with NRC

```
sky distribution, presample light tiles, world cache (unchanged)
drop(pass)
  encoder A (its own, raw only): context.begin_frame(&frame_settings); ctx.add_command_buffer(A.finish())
  queue.write_buffer(constants, PopulateShaderConstants with pad0)   (after BeginFrame: the multiplier follows the frame settings)
  ctx.command_encoder().clear_buffer(counters)
pass = begin_compute_pass; bind groups 0 scene, 1 lighting (+offsets), 2 nrc
  training pass            span "solarik_lighting/nrc_training"
  direct lighting          (unchanged; group 2 stays bound, DI's layouts do not reach it)
  gi initial               (the NRC variant)
drop(pass)
  ctx.command_encoder().copy_buffer_to_buffer(counters → BufferIdx::Counter buffer, 8 bytes)
  encoder B (its own, raw only): context.query_and_train(false); ctx.add_command_buffer(B.finish())
pass = begin_compute_pass; bind groups 0, 1, 2 nrc
  nrc resolve + temporal   span "solarik_lighting/nrc_resolve"
  gi spatial and shade
specular GI, lit fog (unchanged; specular rebinds group 2 to the DLSS textures when present)
drop(pass) ... (unchanged)
recorded_this_frame = true
```

`ctx.add_command_buffer` finishes the current wgpu encoder, queues it, queues the raw one, and the next `ctx.command_encoder()` starts a fresh encoder, so the submit order is preserved. The library's commands never share an encoder with wgpu's (wgpu panics otherwise).

### Shaders

#### `bevy_solarik/src/realtime/nrc.wesl` (`load_shader_library!`, module `bevy_solarik::realtime::nrc`)

A real file in the source tree (the shader test harness in `tests/support/shader_graph.rs` loads every `.wesl` under `src`): a short header with the seven bindings above (group 2), then `crates/nrc_records/shaders/nrc_records.wgsl` and `nrc_buffers.wgsl` verbatim. A test (`bevy_solarik/tests/nrc_module.rs`) reads the three files and asserts the module is header + the two sources, so they cannot drift. If wesl-rs rejects the hex float literal `0x1.0001p-16f`, change the constant in `nrc_records.wgsl` to `bitcast<f32>(0x37800080u)` (the same bits) and rerun that crate's tests.

Everything NRC is under `@if(NRC)` in the existing shaders, so the default build compiles the same WGSL as before.

#### Units

NRC is handed exposed radiance: every radiance written into a training vertex or the sky term is multiplied by `view.exposure`, and the resolve divides the query result by `view.exposure`. Then `max_expected_average_radiance_value = 1` is about right (exposed scene radiance averages 0.1 to 1) and the LogLuv range is used well. Positions are Bevy world positions (meters), encoded with `nrc_encode_position(p, nrc_constants)`.

#### Surface attributes (both passes, the same way)

- position: the hit's `world_position`;
- normal: the shading normal (`world_normal`), for a leaf the side facing the viewer (restir_gi already flips `sample_point_world_normal` for `diffuse_transmission > 0`);
- view direction: the unit vector from the surface toward where the ray came from (`wo`), as NVIDIA's `viewVector`;
- roughness: `material.roughness` (linear);
- albedo: `calculate_diffuse_color(material.base_color, material.metallic, 0.0, 0.0)` (bevy_pbr's; brdf.wesl imports it);
- specular F0: `calculate_F0(material.base_color, material.metallic, vec3(material.reflectance))`;
- delta lobe: `material.roughness <= MIRROR_ROUGHNESS_THRESHOLD && material.metallic > 0.9999`.

#### `nrc_training.wesl` (`embedded_asset!`), entry `nrc_train`, workgroup 8x8

One thread per training path over `nrc_constants.training_dimensions`. A port of NVIDIA's update-mode flow (`NrcCreatePathState`, `NrcUpdateOnHit`, `NrcSetBrdfPdf`, `NrcUpdateOnMiss`, `NrcWriteFinalPathInfo` of `vendor/NRC/Include/Nrc.hlsli`) around a bounce loop built from Solarik's pieces (`surface_path.wesl` is the model: `sample_random_light_transmitted`, `evaluate_brdf`, `evaluate_brdf_pdf`, `evaluate_and_sample_brdf`, `power_heuristic`, `random_emissive_light_solid_angle_pdf`, `emitted_radiance`, `trace_ray`, `resolve_ray_hit_full`, `sample_sky`).

- The path's first vertex is a camera pixel: `pixel = min(vec2u((vec2f(id.xy) + rand2) * viewport / vec2f(training_dimensions)), viewport - 1)`, its surface from the G-buffer (`gpixel_resolve`), `wo` toward the camera, `hit_distance` the distance to the camera. `rng` from the pixel index and `constants.frame_index` (a stream of its own, e.g. `^ 0x5bd1e995u`), `forest_set_seed` likewise. A sky pixel (depth 0) writes a training path info with vertex count 0 and `NRC_PATH_FLAG_HAS_EXITED_SCENE`, query index `0xffffffff`, and returns.
- Segment accounting, as NVIDIA's: `radiance` and `throughput` are reset to 0 and 1 when a vertex is recorded; what accumulates between two recorded vertices (NEE at the vertex, emission met by the scatter ray, the sky on a miss) finalizes the earlier vertex (`nrc_update_training_path_vertex`) when the next one is recorded, or at the end (`NrcWriteFinalPathInfo`). The library reconstructs each vertex's target as `radiance_i + throughput_i * target_{i+1}`, the last one's from its self-training query.
- Per hit, in order: spread radius update (primary from `hit_distance / sqrt(cos_gamma / 4π)`, then `cumul += hit_distance / sqrt(cos_gamma * brdf_pdf)` unless the previous hit was delta; the previous-was-delta flag); a delta vertex is not recorded (NVIDIA skips it with the heuristic on: no NEE, bounce on); otherwise finalize and write the previous vertex if any, `vertex_count++`, reset accumulators, make the new packed vertex (kept in a `var`, written when finalized); `terminate = bounce == max_path_vertices - 1`, and for a path without `NRC_PATH_FLAG_IS_UNBIASED`: if `nrc_evaluate_termination_heuristic(state, training_termination_heuristic_threshold)` then `terminate |= HEURISTIC_RESET flag set`, set the flag, `cumul = 0` (run to the heuristic twice). `terminate` → break (TerminateImmediately: the cache includes direct light, so the last vertex gets no NEE; its target is the self-training query).
- NEE at a recorded vertex as `surface_path.wesl` does (the power heuristic against the BRDF pdf times the continuation probability when `brdf_rays_can_hit`), times `view.exposure`, into `radiance` (throughput is 1 here).
- Scatter: `evaluate_and_sample_brdf`; `pdf <= 0` → `throughput = 0`, break. `throughput *= next.throughput`, `state.brdf_pdf = next.pdf`. `trace_ray(offset origin, wi, RAY_T_MIN, ray_max_distance(), RAY_FLAG_NONE)` (thin panes skipped, as the GI sample rays do). Miss: `radiance += throughput * sample_sky(wi) * view.exposure`, `HAS_EXITED_SCENE`, break. Hit: `resolve_ray_hit_full`; emission `radiance += throughput * emitted_radiance(material, -wi) * view.exposure * power_heuristic(next.pdf, random_emissive_light_solid_angle_pdf(candidate, hit.world_position))` (weight 1 after a delta bounce); then `hit = candidate`, `wo = -wi`, `hit_distance = ray.t`.
- At the end (`NrcWriteFinalPathInfo`, update mode): if `vertex_count > 0`: finalize and write the current vertex with (`radiance`, `throughput`); if not exited and `max_path_vertices > 1`: `query_index = nrc_increment_counter(NRC_COUNTER_QUERIES)` and `nrc_write_radiance_params(query_index, nrc_create_radiance_params(nrc_unpack_path_vertex_with(vertex, radiance, throughput)))`. Then `nrc_write_training_path_info(nrc_calculate_training_path_index(training_dimensions, id.xy), {packed_data, query_index})`.
- `0xffffffff` is "no query" (NVIDIA's `NrcCreatePathState`). Positions in the spherical encodings: `nrc_safe_cartesian_to_spherical_unorm` as is (the NaN at the poles is the HLSL's too; a normal never has |z| rounding past 1 after `normalize`... it can; clamp `p.z` to [-1, 1] before `acos` in `nrc_records.wgsl` is allowed, with a comment, and the crate's tests rerun).

#### `restir_gi.wesl` under `@if(NRC)`

- `generate_initial_reservoir`: where the world cache is queried (the `!(NO_WORLD_CACHE)` branch), when `nrc_constants.pad0 != 0u`: take a query index (`nrc_increment_counter`), write `NrcRadianceParams` for the sample point (view direction `-ray_direction`, the attributes above), remember the index and `nrc_encode_log_luv_hdr(max(albedo + F0, 0))` in two `var<private>`s, leave `reservoir.radiance = 0` and `unbiased_contribution_weight = direction_sample.inverse_pdf`, and return without the `base_color / PI` factor (the query answers the full outgoing radiance toward the receiver, albedo included). Otherwise (warm-up) the world cache path as before. With `REGIONAL_HISTORY`, `support_radius = ray_max_distance()` for an NRC sample.
- `initial_and_temporal`: with `NRC`, after `generate_initial_reservoir`, `nrc_write_query_path_info(pixel_index, NrcPackedQueryPathInfo(prefix, query_index))` (`0xffffffff` and 0 when there was no query, also on the early returns for depth 0 and mirrors), then `gi_reservoirs_b[pixel_index] = initial_reservoir; return;` (no temporal step).
- New entry `nrc_resolve_and_temporal` (8x8): the same early returns; `var reservoir = gi_reservoirs_b[pixel_index]`; `let info = nrc_read_query_path_info(pixel_index)`; if `info.query_buffer_index != 0xffffffffu`: `reservoir.radiance = max(vec3(0), nrc_decode_log_luv_hdr(info.prefix_throughput) * nrc_unpack_query_radiance(nrc_constants, nrc_read_query_radiance(info.query_buffer_index)) / view.exposure)`; with `VISUALIZE_NRC` store that radiance times exposure to `view_output` and return. Then the temporal step exactly as `initial_and_temporal` does it today (`load_temporal_reservoir`, `merge_reservoirs`), `gi_reservoirs_b[pixel_index] = merged`.

### Scene bounds from the game

`load_level` knows every BSP it loads (`halo_map::bsp::Bsp::world_bounds`): the union over the level's BSPs, converted with `halo_to_bevy` (axes change: take min/max per component after conversion), plus a 50 m margin for objects and the sky's water, becomes `SolarikNrcSceneBounds`. (A level change replaces it; the bounds change makes `prepare_nrc` reconfigure with `request_reset`.)

### Checks

- `cargo test -p nrc_records --release -- --ignored` still passes if `nrc_records.wgsl` changed.
- The WESL compiles: a test over `tests/support/shader_graph.rs` validating `bevy_solarik::realtime::restir_gi` with and without `NRC` (and `REGIONAL_HISTORY`, `FOLIAGE_TRANSMISSION`, `WORLD_CACHE_FIRST_BOUNCE_LIGHT_LEAK_PREVENTION`), and `bevy_solarik::realtime::nrc_training` with `NRC`, `FOLIAGE_TRANSMISSION`; `scene_shader_variants` still passes.
- `cargo build --release --features dlss,nrc` builds the game; `cargo build --release --features dlss` (no nrc) is unchanged.
- In the game, `SOLARIK_NRC=1` with `HALO_RT_GPU_LOG=1`: no validation errors, the `nrc_training` and `nrc_resolve` spans in the log, the NRC log (target `nrc`) quiet; `SOLARIK_SHADER_DEFS=VISUALIZE_NRC` shows a plausible lit scene after the warm-up.
- Then `scripts/boil/run.sh nrc` against the `default` table (the plan's step 6).

## Integration results (5 November)

The design above is built and runs in the game (`cargo build --release --features dlss,nrc`, then `SOLARIK_NRC=1`). Two Opus agents wrote it, the shaders and the Rust side in parallel against the design, and it was reviewed and measured here. The default build compiles the same WGSL as before (checked by dumping the compiled modules with and without the change).

What was checked before the harness:
- `SOLARIK_SHADER_DEFS=VISUALIZE_NRC` (with `HALO_RT_NO_DLSS=1`) at b30's beach shows the answers at the sample points: black where the sample ray hit the sky (no query), the sand's radiance elsewhere. In b40's shaft the same view is near black, because that shaft's diffuse GI is a few percent of the picture (the strips' direct light is the rest), not because the answers are missing.
- `HALO_RT_CACHE_LOG=1` prints an `nrc N:` line every 40 frames (the game's `log_world_cache`): the frame's query count (745,000 in the shaft, 141,000 at the beach where most sample rays reach the sky), the first answer, the first query record's position and albedo, the first training path's packed data (3 vertices, heuristic stop) and its first vertex's radiance and throughput.
- The NRC render against the default capture at the same spot: the beach within noise (mean 124.0 against 123.7 of 255), the shaft 3% darker (30.8 against 31.9), the left wall and the floor alike.

`scripts/boil/run.sh nrc` against the `default` table (the same build, NRC on by the environment; the world cache still running, its GI side the warm-up fallback):

```
spot             gpu direct  cache diffGI specGI   dlss | stillWdr stillP90 still3s |   noise noiseP95 | moveWdr  move3s
b40_shaft      10.13   2.68   1.26   0.76   1.38   1.32 |   0.019   0.052   0.074 |   0.148    0.46 |   0.034   0.154
b40_canyon      8.28   1.94   1.28   0.68   0.63   1.31 |   0.004   0.005   0.013 |   0.072    0.24 |   0.005   0.020
c20_hall        6.74   1.86   1.10   0.43   0.44   1.31 |   0.003   0.005   0.014 |   0.086    0.30 |   0.005   0.027
a10_tube        7.21   1.84   1.05   0.51   0.91   1.31 |   0.002   0.003   0.006 |   0.011    0.05 |   0.003   0.011
a10_corridor    7.82   1.91   1.12   0.52   1.26   1.31 |   0.006   0.014   0.019 |   0.101    0.26 |   0.015   0.043
b30_passage     9.29   2.08   1.15   0.81   1.76   1.32 |   0.010   0.018   0.015 |   0.061    0.21 |   0.008   0.032
b30_beach       6.46   1.16   1.09   0.44   0.44   1.32 |   0.005   0.012   0.032 |   0.187    0.50 |   0.007   0.048
b40_canyon_fast 8.45   1.94   1.26   0.68   0.64   1.32 |   0.004   0.006   0.013 |   0.073    0.24 |   0.006   0.023
```

Reading it against `default` (the table in `boiling-plan.md`; `diffGI` is now the initial pass alone, the spatial pass is a span of its own):

| | default | nrc | note |
|---|---|---|---|
| shaft: diffuse GI passes | 1.71 | 0.76 + 0.62 + 0.24 training + 0.21 resolve = 1.83 | the initial pass no longer hashes into the cache |
| shaft: timed total | 10.06 | 10.13 | |
| `QueryAndTrain` | | not in the total | the library's commands carry no timestamps; the spike measured 0.77 ms at this query count, 0.9 with 8-vertex paths |
| real cost | | about +0.9 ms a frame | with `sample_gi` still running (0.4 to 0.6 ms to take back when the fallback goes) |
| still three-shot spread, shaft | 0.053 | 0.074 | worse |
| still P90, corridor / passage | 0.007 / 0.005 | 0.014 / 0.018 | worse |
| noise, shaft / tube | 0.126 / 0.014 | 0.148 / 0.011 | mixed |
| motion three-shot, shaft | 0.128 | 0.154 | worse |
| everything else | | within the run-to-run spread | |

So the first NRC frame costs about what was estimated and is as steady as the world cache at five spots and a little less steady at three (the shaft, the corridor, the passage), in stills above all: the network keeps training on 21,867 noisy paths a frame and its answer at a fixed point breathes (the drift the plan warned of; the spike measured 0.05% a frame on a synthetic scene, a real scene with one light sample per vertex trains noisier). The cache's boiling in motion (the shaft's 0.128) is not improved yet either: the ReSTIR temporal step still resamples a new sample point every frame, and the answer's smoothness only helps once the drift is below the cache's cell noise.

Knobs to try next, in order, each one a harness run:
1. `self_training_attenuation` below 1 and `learning_rate` below 1e-2 (the breathing), and `num_training_iterations` 2 (half the training per frame).
2. More signal per training vertex: two light samples at each vertex (the cache takes four per update), and `proportion_primary_segments_to_train_on` above 0.05 (the visible surfaces are what the queries ask about).
3. Then the step the design deferred: drop `sample_gi` once the warm-up is over (the cache's DI slots stay), and move specular GI's rough hits onto the query.

Diagnostics left in: the `nrc N:` log line, `VISUALIZE_NRC`, and the `nrc_training` / `nrc_resolve` spans. Bundling the DLLs is still to do (`src/bundle.rs`, 120 MB).

## Tuning and diagnosis (5 November, later)

The knobs of the results section are now read from the environment (`nrc.rs`, `env_or`): `SOLARIK_NRC_ITERATIONS`, `SOLARIK_NRC_LEARNING_RATE`, `SOLARIK_NRC_ATTENUATION`, `SOLARIK_NRC_PRIMARY`, `SOLARIK_NRC_LIGHT_SAMPLES` (light samples at each training vertex, averaged; `NrcConstants.pad1`), `SOLARIK_NRC_FEATURE_SIZE` (the hash grid's finest level, metres) and `SOLARIK_NRC_FREEZE` (no training after the warm-up). Each run below is `scripts/boil/run.sh` at the three spots the results section found less steady, or the shaft alone; the `default` and `nrc` lines are the earlier tables.

Knob 1, the training damped (2 iterations, learning rate 5e-3, self-training attenuation 0.5; now the defaults):

```
spot            still: Wdr   P90   3s     noise   move: Wdr  3s
a10_corridor  default 0.004 0.007 0.018   0.098   0.006 0.040
              nrc     0.006 0.014 0.019   0.101   0.015 0.043
              damped  0.004 0.008 0.018   0.099   0.009 0.041
b30_passage   default 0.003 0.005 0.012   0.056   0.005 0.030
              nrc     0.010 0.018 0.015   0.061   0.008 0.032
              damped  0.004 0.006 0.014   0.061   0.006 0.030
b40_shaft     default 0.015 0.038 0.053   0.126   0.029 0.128
              nrc     0.019 0.052 0.074   0.148   0.034 0.154
              damped  0.019 0.051 0.075   0.154   0.033 0.152
```

The corridor and the passage are back at the default's stills: that breathing was the network drifting, and half the training per frame is enough against it. The shaft did not move, and it did not move for any of the runs after either, all to the third decimal (still P90 / three-shot / motion three-shot):

| shaft run | still P90 | still 3s | move 3s | what it tells |
|---|---|---|---|---|
| default | 0.038 | 0.053 | 0.128 | |
| nrc, damped, 4 light samples a vertex + primary 0.25 | 0.052 | 0.075 | 0.153 | not the training rate, not the light sample count |
| feature size 10 cm, 40 cm (1 cm before) | 0.053 | 0.075 | 0.152 | not the hash grid's resolution |
| frozen after the warm-up | 0.052 | 0.075 | 0.154 | not drift: a network that no longer changes wanders as much |
| the cache's answers through the split pipeline (`SOLARIK_NRC_WARMUP=1000000`) | 0.039 | 0.052 | 0.128 | the initial / resolve split and the second rng stream are not it |
| a flat answer (`SOLARIK_SHADER_DEFS=NRC_CONSTANT_ANSWER`) | 0.040 | 0.053 | 0.125 | the default again: the wander is in the answers' own values |

So the shaft's extra wander is the variation of NRC's answers from one sample point to the next (the ReSTIR GI draws a new sample point every frame; the answers at those points scatter more than the cache's cell values did, and the temporal and spatial reuse and DLSS-RR carry that scatter into the picture). Two views of the same sample points, both times 32 (`VISUALIZE_NRC` with `VISUALIZE_NRC_GAIN` under NRC; the new `VISUALIZE_GI_INITIAL` under the cache; `HALO_RT_NO_DLSS=1`; the mean of ten captures 40 frames apart over the view without the HUD rows):

| | world cache | NRC |
|---|---|---|
| mean level | 59.2 | 16.4 |
| frame-to-frame std of a 19-pixel box, relative | 0.038 | 0.119 |
| share of pixels above four times their 19-pixel mean | 0.26% | 0.81% |
| the ratio cache / NRC by cache level | | 1.6 (dim), 3.2 (middle), 4.3 (bright) |

NRC's answers are 3.6 times lower than the cache's at the shaft, everywhere (the far wall 3.4, the ledge 3.2 to 4.5), and three times rougher. The final image is only 3% darker because the shaft's GI is a few percent of it. (The answers that the per-pixel view shows are noisy by construction, one random hemisphere direction a pixel, for both.)

The first concrete difference found between the two estimators: the cache update samples its lights with `sample_random_light_ris` (128 candidates from the light tiles plus 16 from the cell's light slots, resampled, shadow-tested; `world_cache_update.wesl`), while the training pass picks a light with `sample_random_light_transmitted`, uniformly-ish from all 464 emissive meshes plus sun and sky. Almost every training vertex then records zero direct light and a rare one a huge value; the network (Adam on a relative L2 loss) under-fits such a target toward the dark side and what it fits is rough. The fix under way is the cache's sampler in the training pass (an Opus agent is doing it; its result goes below). Four uniform samples a vertex did not raise the level (the shaft still 33.0 against the default's 34.1), which fits: four draws from 466 lights still mostly miss.

Not settled: which level is right. `--pathtracer` at the shaft spot renders near black (a mean of 0.85 against the real-time 31 after 3000 accumulated frames; `scripts/boil/out/ref/b40_shaft/`), so the reference mode needs a look of its own before it can arbitrate (the emissive scale or the light sampling of the strips, probably).

A trap met on the way: `restir_gi.wesl` and this document are CRLF files; a Python replacement written against LF text matched nothing, the build then had nothing to do, and two harness runs measured the unchanged shader before `grep -c` caught it.

### The cache's light sampler in the training pass

Done (commit `549d4f04`, an Opus agent against the spec above, reviewed): `sample_random_light_ris` moved to `light_ris.wesl` with a variant that hands back the pick's direction and its radiance without the receiver cosine; the training applies the full BRDF to it, takes the sun's own shadow-tested sample as the update does, reads the vertex's cell slots (`find_world_cache_cell`, read only, atomic loads) and after such a vertex leaves out the emission its scatter ray meets (weight 0: the NEE covers every emissive light, there is no single density to weigh against). Leaves (the resampling's target is zero behind the surface, which a leaf transmits) and vertices without a cell keep the uniform sampler with its MIS. The cache update's compiled WGSL is unchanged (dumped and compared for three def sets). Then the tile candidate count became a knob (`SOLARIK_NRC_TILE_CANDIDATES`, `NrcConstants.pad2`, default 32; the update's 128).

| shaft | answers' view mean (cache 59.1) | still mean (default 34.1) | nrc_training ms | gpu total | still P90 / 3s | motion 3s |
|---|---|---|---|---|---|---|
| uniform light pick (`nrc_damped`) | 16.4 | 33.0 | 0.24 | 10.12 | 0.051 / 0.075 | 0.152 |
| resampled, 128 candidates (`nrc_ris`) | 70.7 | 34.2 | 1.50 | 11.31 | 0.048 / 0.068 | 0.142 |
| resampled, 32 candidates (`nrc_ris32`) | | 34.2 | 0.93 | 11.04 | 0.048 / 0.066 | 0.142 |
| default | | 34.1 | | 10.06 | 0.038 / 0.053 | 0.128 |

So the dark answers were the training's light sampling: the level is now 20% above the cache's in the answers' view, and the final picture matches the default to 0.3% (the GI's share there is small). The corridor and the passage match the default in every column with it. The shaft is steadier than with the uniform pick but still not the default's (0.066 against 0.053 in stills, 0.142 against 0.128 in motion): the answers vary more from point to point than the cache's cell values, which is partly the thing itself (a continuous field against 30 cm cells with a half-cell jitter) and partly the network's own roughness; which of the two is not separated yet, and which level is right is not either (`--pathtracer` renders the shaft black, above).

The cost, b40's shaft, timed spans and the untimed `QueryAndTrain` from the spike's measurement:

| diffuse GI | default | NRC (32 candidates) |
|---|---|---|
| initial + spatial | 1.71 | 0.76 + 0.86 |
| training | | 0.93 (the cell lookup and two shadow rays a vertex; 0.24 with the uniform pick) |
| resolve | | 0.21 |
| QueryAndTrain | | about 0.9 |
| the cache's GI side, to drop after the warm-up | 0.4 to 0.6 | 0.4 to 0.6 for now |
| total | 1.71 | about 3.7, about 3.2 once `sample_gi` goes |

Where that leaves NRC as the diffuse GI's source: correct in level, about 1.5 ms a frame dearer than the world cache, as steady as it at seven spots and a little less at the shaft. On its own it is not a boiling fix. What it still offers that the cache cannot is the multi-bounce answer and a single query for specular GI's rough hits (the deferred step), and the training pass's cost has room (the two shadow rays a vertex could be one; the primary vertex's NEE could come from the DI pass's reservoir).

Knobs left to try, cheap ones first: `SOLARIK_NRC_LIGHT_SAMPLES=2` with 16 candidates (the same cost, two picks), `SOLARIK_NRC_PRIMARY` up (the visible surfaces are what the queries ask about), and querying one bounce later where the spread radius is small (NVIDIA's own rule; it trades a ray for a smoother answer at near sample points, which is the shaft's case).

### The reference, and the strips' animation (6 November)

`--pathtracer` lacked the real-time path's lighting systems (`apply_lighting`, `apply_level_exposure`, `follow_camera_sky`: the emissive, sun and sky scales, the level's lamps factor and the exposure), fixed. With them the shaft's reference is still eight times darker than the real-time picture in the walls and the floor (raw 8-bit means: the left wall 2.3 against 37.6, the floor 1.6 against 24.8, the directly lit ledge side 5.3 against 27.0) while the strips themselves match (98 against 109). The gap is in the transport of the strips' light, not in the output pipeline. And the real-time level is not fixed either: `HALO_RT_LEVEL_LIGHT=0,1` (the sun's and the sky's share zero) makes the shaft 60% brighter (the whole 50 against 31), which says the direct lighting's candidate budget, with the sun's fixed share in it, under-samples the strips in the default picture. So neither picture is a ground truth for the GI level yet; this is a bug hunt of its own (the pathtracer's NEE and MIS against emissive meshes, the DI's sun share indoors) and is parked. For the boiling question it does not matter: the NRC and the cache agree with the default picture to 0.3%.

The shaft's strips animate (`animate_self_illumination`, a periodic function of time; `HALO_RT_FREEZE_ANIMATION=1` holds them). Their effect on the harness is nil: frozen, the default's shaft is 0.039 / 0.053 / 0.126 (still P90 / three-shot / motion three-shot) against 0.038 / 0.053 / 0.128 animated, and NRC's 0.046 / 0.064 / 0.140 against 0.048 / 0.066 / 0.142. The lit ledge does wobble about 3% with the same phase in every run, but the measures are local and relative and a slow change of the whole passes through them.

### Querying one bounce later where the spread is small (6 November)

NVIDIA's rule, built behind `SOLARIK_SHADER_DEFS=NRC_QUERY_AT_SPREAD` (an Opus agent against a spec, reviewed; `nrc_query_at_spread` in `restir_gi.wesl`): the path's spread at the sample point against the primary spread by `nrc_evaluate_termination_heuristic` (the threshold `SOLARIK_NRC_QUERY_THRESHOLD`, 0.1); where it is too small, the sample point's direct light by the cache's resampled sampler and the sun's sample, one BRDF bounce, and the query at that hit with the prefix throughput, the resolve adding the answer to the reservoir's radiance. `VISUALIZE_NRC_BOUNCED` shows which pixels bounced on. At the shaft 17% of the querying pixels do, on the floor strip and the near ledge.

| shaft | still P90 / 3s | noise | move 3s | diffuse GI initial | gpu |
|---|---|---|---|---|---|
| default | 0.038 / 0.053 | 0.126 | 0.128 | 1.71 (with spatial) | 10.06 |
| NRC, resampled training (`nrc_ris32`) | 0.048 / 0.066 | 0.144 | 0.142 | 0.76 | 11.04 |
| with the rule (`nrc_spread`) | 0.047 / 0.065 | 0.197 | 0.141 | 2.34 | 12.35 |

Nothing moved in the stills or in motion, the per-pixel noise rose (one light sample baked into those pixels' radiance) and the initial pass costs 1.6 ms more (wave divergence around the resampling and two more rays for 17% of the pixels); the corridor and the passage are unchanged. The answers' view sits at 62 (the cache's 59), the picture at the default's level. So the shaft's remaining point-to-point variation is not the network answering with too small a footprint: it is in the radiance field the ReSTIR GI samples, which the cache's cells blurred and NRC resolves. The def stays for the record, off.

Where this leaves NRC, the decision point of the plan: as the diffuse GI's source it is right in level, matches the default picture, is as steady as the world cache at seven of eight spots and a little less steady at the shaft, and costs about 1.5 ms a frame more. It does not fix the boiling. What remains worth having from it is what the cache cannot do, the multi-bounce answer and a single query for specular GI, and that is a cost question now, not a steadiness one. The boiling work goes back to the estimator itself (the plan's remaining items): the cache's blur was doing the smoothing at the shaft, and a smoother ReSTIR GI estimate, whatever answers it, is what the shaft needs.

## Specular GI on the query, and the cache's GI side dropped (6 November): design

The boiling plan is through Phase 4 (`docs/boiling-plan.md`), and the user's intent for NRC is what the decision point left it: not the diffuse GI's steadiness, which the cache now has, but the two things the cache cannot give, a multi-bounce answer (NRC's training paths carry every bounce; the cache's cells carry their own one ray into other cells, blurred) and one view-dependent query where the specular path today ends in the cache. This step builds that, behind the same `nrc` feature and `SOLARIK_NRC=1`, with its own switches, so the user's A/B stays.

What the specular pass does today (`specular_gi.wesl`, `trace_unfogged_glossy_path`): a glossy path from the pixel's surface; at the third vertex, or at a hit past a cell's size where the path's spread or the hit's roughness is above `DIFFUSE_GI_REUSE_ROUGHNESS_THRESHOLD` (`cache_rough_hits`), it adds `throughput * base_color / PI * query_world_cache(hit)` and ends. The same path code serves `primary_glass.wesl` (reflections in glass), and the pass runs after the diffuse GI's spatial pass; its PSR variant (`DLSS_RR_GUIDE_BUFFERS`) binds the DLSS guide textures at group 2, where `nrc.wesl` also puts NRC's buffers.

The design:

1. **The query at the termination** (`NRC_SPECULAR`, a definition the specular pipelines alone get, so `primary_glass`'s compile of the same path is unchanged; `SOLARIK_NRC_SPECULAR=0` leaves it off with NRC on). Where the path would read the cache, with `NRC_SPECULAR` the main lobe's path writes a query instead: radiance params at the hit as the GI's `write_nrc_query_with_throughput` writes them (position, the hit's normal, view direction `-wi`, roughness, albedo, F0), a query index from the counter, and the path info in the pixel's **second slot**, index `pixel_count + pixel_index` (`samples_per_pixel: 2` in `nrc.rs`; the library sizes its query buffers by it), with the prefix throughput `throughput * base_color / PI` and, when fog is active, the first segment's transmittance (the immediate radiance keeps the in-scatter; `fog_ray_segment` has both). The specular entry writes an empty path info (query index `0xffffffff`) to its slot for every pixel first, early returns included, since the resolve reads every slot each frame. The clear coat's path and any second termination in a pixel keep the world cache (one specular slot a pixel; the coat is a small share). The query writer shared by the two passes moves out of `restir_gi.wesl` into a module both import (the GI keeps its private prefix and index for its own path-info write at the end).
2. **The order.** Training, DI, GI initial (slot 1), **then specular GI** (slot 2, its own radiance added to the view as now), then `QueryAndTrain`, then the resolve, then the GI spatial pass. The specular pass moving ahead of the resolve is safe for the GI: `replace_primary_surface` writes the DLSS guide textures and the specular motion vectors only, which no GI pass reads, and both passes add to `view_output`, which commutes. Without `NRC_SPECULAR` the order is as today.
3. **The resolve.** `nrc_resolve_and_temporal` (`restir_gi.wesl`) adds the specular slot's answer first, before its own early returns (mirror pixels are the ones with specular paths): `view_output += decode(prefix) * nrc_unpack_query_radiance(...)`, exposed radiance as the view holds it (the GI divides by the exposure because its reservoir holds unexposed radiance; the specular pass multiplies by it before adding, so the resolve adds the unpacked value as it is).
4. **The groups.** The specular pipelines with `NRC_SPECULAR` bind NRC at group 2 as every NRC pipeline does; the PSR variant's DLSS guide textures move to group 3 under `NRC_SPECULAR` (`realtime_bindings.wesl`, `@if` on the five declarations' group attribute), and its layout lists NRC's then the textures'. The default build's compiled WGSL is unchanged (dump and compare, as the earlier steps did).
5. **The cache's GI side dropped** (`SOLARIK_NRC_DROP_CACHE_GI`, on with NRC once the warm-up has passed, with `NRC_SPECULAR` on): `sample_gi` is not dispatched; the cells then hold their direct light alone (the slots and the DI are untouched; `primary_glass`'s grime reads a direct-only irradiance; should NRC fail mid-session the fallback's bounce light returns with the pass). 0.4 to 0.6 ms back.
6. **Checks.** `tests/nrc_shaders.rs` gains the `NRC_SPECULAR` variants of `specular_gi` (with and without `DLSS_RR_GUIDE_BUFFERS`) and of `restir_gi`'s resolve; `tests/nrc_module.rs` if the header moves; the three shader tests pass; `VISUALIZE_NRC` still shows the GI's answers; a new `VISUALIZE_NRC_SPECULAR` shows the specular slot's answer alone (black where the path ended elsewhere) for the shaft and the beach; `HALO_RT_CACHE_LOG`'s `nrc N:` line counts both slots' queries. Then the harness (`scripts/boil/run.sh`, run here, not by the agent): `nrc_spec` against `p3_final` and the `nrc` columns; the gate is the plan's (no boiling column worse than the NRC run before it, the specular pass's time with the cache read gone, the cache pass with `sample_gi` gone).

Not in this step: making NRC the default build (the DLLs' bundling in `src/bundle.rs`, 120 MB), which waits on the measurement; `NRC_QUERY_AT_SPREAD` stays off.

### Built (6 November)

As designed, by an agent, with two corrections. The query writer is `nrc_query.wesl`'s `write_nrc_query_record` (registered with the `nrc` feature), returning the index and a linear prefix; `restir_gi` encodes it as before (its NRC WGSL differs only by the call). The specular prefix is not `throughput * base_color / PI`: the network learns radiance over albedo + F0 (`learn_irradiance`), so the prefix is the path's throughput to the hit times the hit's albedo + F0, as the GI's; and it gathers the factors the path applies after the termination, the first segment's fog transmittance (`ray_fog.wesl`'s new `fog_ray_segment_transmittance`, under `NRC_SPECULAR`: `fog_ray_segment` is affine in the radiance, so the factor separates cleanly; the in-scatter stays in the radiance returned), the lobe's 1 / pdf in `glossy_lobe`, and `evaluate_base_specular_brdf` in the entry, where the second slot's path info is written. The query waits for the warm-up (`pad0`) as the GI's does; `var<private> nrc_specular_slot_open` is true only around the main lobe's `glossy_lobe`, so the clear coat keeps the cache. `specular_gi` writes the empty second-slot info first, before the sky return. In `nrc.rs`: `SOLARIK_NRC_SPECULAR` (default 1), `SOLARIK_NRC_DROP_CACHE_GI` (default 1, needs the specular query), `samples_per_pixel` 2 with it, `NrcPipelineIds::specular` / `specular_psr`, `NrcFrame::specular_pipeline(psr)` and `drops_cache_gi(frame)`; the init log line says which are on. In `node.rs`: `create_pipeline_with_layouts` (several extra groups; `create_pipeline` wraps it), the resolve with `NRC_SPECULAR`, the specular pass in two stages (below), and `sample_gi` not dispatched once warm (it only adds to `sample_di`'s pending radiance, so the blend and the indirect dispatch are untouched). `VISUALIZE_NRC_SPECULAR` stores the specular answer in the resolve and returns; `spatial_and_shade` returns under it. The default build's WGSL was dumped for 14 modules over 32 def combinations before and after: identical up to the compiler's own run-to-run variation (declaration order, repeated `enable` lines), the NRC `restir_gi` differing by the writer's move only. The four shader tests pass with the new `nrc_shaders` cases; both game builds build.

The pass in two stages. Moved whole ahead of the resolve, the rough surfaces' lobe (`roughness > DIFFUSE_GI_REUSE_ROUGHNESS_THRESHOLD`, which reads `gi_reservoirs_a` instead of tracing) read a reservoir `spatial_and_shade` had not written yet this frame. So with the specular query on: stage A (`NRC_SPECULAR`, span `specular_indirect_lighting`, between the GI's initial pass and `QueryAndTrain`, NRC at group 2) writes every pixel's empty second slot and then does the tracing surfaces only; stage B (`NRC_SPECULAR_REUSE`, span `specular_indirect_reuse`, at the old place after the spatial pass, bound as the plain pipelines, no NRC code) does the reusing surfaces only. Two more pipelines (`specular_reuse`, `specular_reuse_psr` in `NrcPipelineIds`); the game's overlay has a "Specular reuse" row. At the shaft the two stages take 0.56 + 0.83 ms against the single pass's 1.35.

The split did not fix the darkening. Raw (`HALO_RT_NO_DLSS=1`) at b40's shaft, frame 9000, PNG means (rows:cols; walls 100:600, 0:400, floor 400:600, 450:850, strips 290:632, 892:1275; the HUD left out of the picture's):

| run | picture | walls | floor | strips | live cells |
|---|---|---|---|---|---|
| default | 26.14 | 37.86 | 26.71 | 29.78 | 262,954 |
| NRC, `SOLARIK_NRC_SPECULAR=0` | 25.79 (0.99) | 37.79 (1.00) | 27.10 (1.01) | 28.21 (0.95) | 131,319 |
| NRC, `SOLARIK_NRC_DROP_CACHE_GI=0` | 20.71 (0.79) | 31.81 (0.84) | 21.95 (0.82) | 18.87 (0.63) | 99,895 |
| NRC (both on) | 18.00 (0.69) | 28.02 (0.74) | 18.89 (0.71) | 15.75 (0.53) | 16,929 |

`samples_per_pixel: 2` is not it (2 with the specular query off: walls 1.00). The cause is the cache's cells, not their radiance. A cell is made, and its life renewed, only by `query_world_cache` / `query_two_sided_world_cache` (`world_cache_query.wesl`); ReSTIR DI (`restir_di.wesl`, `find_world_cache_cell` for its `DI_CELL_LIGHTS` candidates) and the training pass's resampled light sample only look cells up. With NRC answering the GI, the cells at the shaft's walls were kept by the specular paths' cache terminations and `sample_gi`'s ray hits (which query the cell they hit). The specular query removes the first; dropping `sample_gi` the second. `VISUALIZE_CELL_LIGHTS` on the walls: pixels with no cell 4.1% (default), 4.1% (specular off), 6.2% (keep GI), 69.6% (both on); pixels whose cell has no light yet 0%, 0.2%, 32.8%, 11.8% (the surviving cells churn, young, their slots empty). The GI's answers (`VISUALIZE_NRC`) also come out 12% lower on the walls with the specular query (keep GI), consistent with the training's light sample losing the same slots. So the cells needed keeping alive another way.

The touch. Where the default read the cache and NRC now queries, the cache is still queried and its value discarded (`_ = query_two_sided_world_cache(...)` at the GI's sample point in `generate_initial_reservoir`'s NRC branch, with the arguments of the cache read below it; `_ = query_world_cache(...)` at `specular_gi`'s `NRC_SPECULAR` termination, with the cache read's arguments): the cell's birth, warm start and life renewal are the query's own. The cells then live for their DI slots, and `SOLARIK_NRC_DROP_CACHE_GI` stays. Same runs as above:

| run | picture | walls | floor | strips | live cells |
|---|---|---|---|---|---|
| default | 25.99 | 37.97 | 26.57 | 28.95 | 262,357 |
| NRC (both on) | 26.33 (1.01) | 38.11 (1.00) | 27.28 (1.03) | 29.22 (1.01) | 189,576 |
| NRC, `SOLARIK_NRC_DROP_CACHE_GI=0` | 26.17 (1.01) | 37.97 (1.00) | 27.24 (1.03) | 28.48 (0.98) | 262,241 |

`VISUALIZE_CELL_LIGHTS` on the walls with NRC (both on): no cell 4.1%, no lights 0% (the default's numbers). GPU times (`HALO_RT_GPU_LOG`, ms), default against NRC with both on: world cache 1.32 / 1.21 (`sample_gi` gone, but more cells selected for update now that they live); direct 2.82 / 2.79; diffuse 1.71 (initial with spatial) / 0.80 initial + 0.85 spatial + 0.96 training + 0.22 resolve; specular 1.35 / 0.58 (stage A) + 0.84 (stage B); total 8.83 / 9.91.

Earlier smoke runs (first build, single pass): about 9,000 specular queries a frame at the shaft (752,956 against 743,995 with the query off; a glossy path reaches the termination only at its second or third hit), `VISUALIZE_NRC_SPECULAR` showing them on the metal ceiling and the far ledge (0.6% of the pixels above 8/255); the PSR variant runs without validation or `nrc` errors.

### Measured (6 November)

`SOLARIK_NRC=1 scripts/boil/run.sh nrc_spec` (the specular query on and `sample_gi` dropped, as built above) against `p3_final`, the boiling plan's current baseline (the same build, NRC off); `specGI` is now the sum of the two stages:

```
spot             gpu direct  cache diffGI specGI   dlss | stillWdr stillP90 still3s |   noise noiseP95 | moveWdr  move3s
b40_shaft      11.31   2.81   1.21   0.80   1.44   1.31 |   0.009   0.018   0.028 |   0.098    0.32 |   0.018   0.078
b40_canyon      9.66   2.26   1.22   0.78   0.76   1.31 |   0.005   0.007   0.012 |   0.070    0.24 |   0.004   0.018
c20_hall        7.18   1.76   1.00   0.46   0.51   1.31 |   0.003   0.005   0.013 |   0.087    0.30 |   0.005   0.025
a10_tube        7.77   1.75   0.95   0.53   0.98   1.31 |   0.001   0.002   0.006 |   0.012    0.07 |   0.002   0.010
a10_corridor    8.58   1.83   1.06   0.56   1.34   1.31 |   0.004   0.007   0.018 |   0.097    0.28 |   0.006   0.038
b30_passage     9.94   1.97   1.08   0.85   1.84   1.31 |   0.003   0.005   0.012 |   0.058    0.22 |   0.005   0.030
b30_beach       7.46   1.43   1.11   0.49   0.53   1.31 |   0.005   0.013   0.033 |   0.190    0.51 |   0.007   0.049
b40_canyon_fast 9.65   2.24   1.23   0.78   0.76   1.31 |   0.003   0.005   0.012 |   0.068    0.24 |   0.005   0.022
```

Boiling: every column is within the run-to-run spread of `p3_final` (the shaft 0.028 / 0.078 against 0.029 / 0.080 in the still and motion three-shots; the only movers are the canyon's still P90, 0.007 against 0.004, and its noise, 0.070 against 0.061). The earlier NRC runs' shaft (0.074 / 0.154 in the results section, 0.066 / 0.142 with the resampled training lights) was measured before the boiling plan's Phase 3; its DI candidates and lookup probes took the cache's shaft to 0.029 / 0.080 and NRC's with it. So NRC is now as steady as the cache at all eight spots, and the specular query changed nothing the harness sees.

Cost, timed spans at the shaft, `p3_final` against `nrc_spec`: direct 2.84 / 2.81, cache 1.27 / 1.21, diffuse GI 1.45 (initial with spatial) / 0.80 + 0.85 + 0.96 training + 0.24 resolve, specular 1.37 / 0.60 (stage A) + 0.84 (stage B), total 9.92 / 11.31. The other spots are 0.8 to 1.0 ms dearer timed. `QueryAndTrain` is still untimed (the spike's 0.9 ms at this query count; the second slot adds about 9,000 queries at the shaft). So NRC costs about 2.3 ms a frame at the shaft and about 1.8 elsewhere, against the 1.5 estimated before the specular step; dropping `sample_gi` took back 0.06 ms here, not the 0.4 to 0.6 expected (the cache pass's cost is its update's selection and shade, not the GI rays).

The picture, the mean of the last five stills (rows 10% to 90%), `nrc_spec` over `p3_final`: shaft 1.015, canyon 1.134, hall 1.010, tube 1.039, corridor 1.004, passage 1.053, beach 1.002. The canyon's is the GI answer, not this step: the 5 November `nrc` run's canyon still was 22.57 against the default's 19.64 already (the brightening is largest in the darkest pixels, 1.22 below 8/255, so it is bounce light the cache's cells did not carry there, or the network's level; no ground truth, the reference mode is parked). The passage's and the tube's are this step's (17.30 and 4.73 in the `nrc` run): the specular answer at the rough hits is above the cache's value the path used to terminate in, as the training's answers measured 20% above the cache's at the shaft.

The drop of `sample_gi` is off by default after this (`SOLARIK_NRC_DROP_CACHE_GI=1` to try it). `scripts/boil/run.sh nrc_keepgi b40_shaft b30_passage` with it off: the cache pass 1.30 at the shaft (1.21 dropped, 1.27 default) and 1.31 at the passage (1.08 dropped, 1.23 default), every boiling column and the still levels the same as `nrc_spec`. So it saves 0.1 to 0.2 ms, and the cells' bounce light still has two readers off the NRC path, the glass's grime (`primary_glass.wesl`, lit by the cache's irradiance) and the clear coat's termination in `specular_gi`, which would both go direct-only.

Where that leaves it: NRC stays opt-in (`SOLARIK_NRC=1`, feature `nrc`); the DLL bundling waits on the user's call. The harness's gate ("no spot +0.3 ms") it fails on cost alone, with no boiling change to pay for it; what it buys is what the cache cannot do, the multi-bounce answer and a single query for specular GI's rough hits, which show as level (the canyon, the passage), and that is the user's A/B in play. A cost to look at if it goes further: stage B (the reusing surfaces, no rays) takes 0.84 ms of the specular's 1.44, a full dispatch of the whole specular shader returning early on the tracing pixels.
