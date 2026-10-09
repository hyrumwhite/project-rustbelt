# Halo RT: Halo: Combat Evolved with Bevy Solari ray tracing

This project ports Halo: Combat Evolved to the **Bevy 0.20** engine and renders it with:

- **[Bevy Solari](https://docs.rs/bevy_solari/0.20.0/bevy_solari/)**: Bevy's fully dynamic, real-time ray-traced lighting.
- **DLSS Ray Reconstruction**: NVIDIA's AI denoiser and upscaler.

It's based on the [OpenCE](../OpenCE) decompilation. See [ROADMAP.md](ROADMAP.md) for the architecture and phases.

**Status: Phase 2 done.** The viewer loads a retail Xbox map directly from the `.map` file: its level geometry, textures, and every object the scenario places (scenery, vehicles, weapons, devices). Halo's own sky model is drawn behind the level. Lighting comes from the level's own data:
- the sky tag's sun becomes a directional light;
- its hemisphere light becomes an environment light;
- shaders that emitted light in Halo's baked radiosity, and self-illuminated surfaces, become emissive light sources;
- light fixtures become ray-traced light sources.

## Game data

No game data is included. You need the retail Xbox maps (`a10.map`, `b30.map`, `bloodgulch.map`, …). OpenCE extracts them from a disc image on first run (see its README). Point the viewer at the folder that holds them:

```powershell
setx HALO_MAPS "C:\path\to\maps"      # once; open a new terminal afterwards
# or pass --maps-dir C:\path\to\maps each time
```

Without either, the viewer looks in `./maps`, a `maps` folder next to the executable, and `../OpenCE/build/windows/maps` (where OpenCE's Windows build extracts them). If it can't find the map, it lists the folders it tried.

## Run

```sh
cargo run --release --features dlss -- b30               # The Silent Cartographer
cargo run --release --features dlss -- a10               # Pillar of Autumn (interiors lit by emissive panels)
cargo run --release --features dlss -- bloodgulch --bsp 0
cargo run --release -- b30 --pathtracer                  # reference pathtracer (slow, converges over time)
cargo run --release -- b30 --screenshot shot.png         # render ~300 frames, save, exit
cargo run --release --features dlss -- bloodgulch --camera 24,-95,2.6,50,-10   # x,y,z (world units), yaw, pitch (degrees)
```

Other switches: `--no-objects`, `--no-fixture-lights`, `--show-light-proxies` (draw the light fixtures' emissive spheres).

The camera starts at the scenario's first player start. Without `--features dlss` there's no denoiser, so Solari's output will be noisy.

### Controls

| Key | Action |
| --- | --- |
| WASD + E/Q | Fly the camera (hold Left Shift to run) |
| Right mouse (hold) or M | Look around |
| `[` `]` | Emissive (Halo light-emitting shaders) brightness |
| `-` `=` | Sun brightness |
| `9` `0` | Sky brightness |
| `,` `.` | Exposure |
| P | Log the current view as a `--camera` argument |
| R | Toggle ReSTIR |
| 3 | Toggle DLSS Ray Reconstruction (`dlss` builds) |
| 4 | Cycle DLSS quality: Auto → DLAA → Quality → Balanced → Performance → Ultra Performance |

The top-right panel shows GPU timings for each Solari pass and for DLSS-RR.

## Layout

| Path | What it is |
| --- | --- |
| `crates/halo_map` | A Bevy-free reader for Xbox cache files: inflation, tag index, BSP geometry, models, object placements, bitmaps (swizzle, DXT/BC), shaders, skies, scenario. |
| `crates/halo_map/src/layout.rs` | Tag struct offsets, **generated** from OpenCE's C headers. |
| `tools/tag_layout` | The generator: `python tools/tag_layout/gen.py` compiles each `probe*.c` against `../OpenCE` with clang (i686 MSVC ABI) and reads the offsets back. Add a field to a probe and re-run it to expose it. |
| `src/` | The Bevy app: `level.rs` (BSPs, objects and the sky model → meshes and materials), `textures.rs` (bitmaps → images, emissive bakes), `sky.rs` (sun and sky light), `main.rs` (camera, controls, DLSS). |
| `examples/solari_demo.rs` | Bevy's Solari example (PICA PICA scene), kept as a reference: `cargo run --release --example solari_demo`. |

### `halo_map` CLI

```sh
cargo run --release -p halo_map -- info    maps/b30.map   # header, tag groups, player starts
cargo run --release -p halo_map -- bsp     maps/b30.map   # geometry stats + winding check
cargo run --release -p halo_map -- shaders maps/b30.map   # BSP shaders, emission, base maps
cargo run --release -p halo_map -- sky     maps/b30.map   # sky lights
cargo run --release -p halo_map -- bitmaps maps/b30.map   # decode every bitmap, report by format
cargo run --release -p halo_map -- objects maps/b30.map list   # placements; decode every placed model
cargo run --release -p halo_map -- bitmap  maps/b30.map "levels\b30\bitmaps\beach" beach.png
```

## DLSS setup

Building `dlss_wgpu` needs:

| Dependency | Install |
| --- | --- |
| Vulkan SDK (`VULKAN_SDK` env var) | `winget install KhronosGroup.VulkanSDK` |
| LLVM / clang (for bindgen) | `winget install LLVM.LLVM` |
| NVIDIA DLSS SDK v310.9.1 | `scripts/setup-dlss.ps1` clones it into `vendor/DLSS` and copies the DLLs into `target/` |

`.cargo/config.toml` points `DLSS_SDK` at `vendor/DLSS`. The app always uses the Vulkan backend, because DLSS needs it.

By downloading the DLSS SDK you agree to its [license](https://github.com/NVIDIA/DLSS/blob/v310.9.1/LICENSE.txt). To distribute a build, ship `nvngx_dlss.dll` and `nvngx_dlssd.dll` next to the exe, along with the license text from section 9.5 of the DLSS programming guide.

## Requirements

- A GPU with hardware ray tracing (NVIDIA RTX for DLSS)
- Rust 1.97.1+
- Windows or Linux with Vulkan
