# Halo RT: Halo: Combat Evolved with Bevy Solari ray tracing

This project ports Halo: Combat Evolved to the **Bevy 0.20** engine and renders it with:

- **[Bevy Solari](https://docs.rs/bevy_solari/0.20.0/bevy_solari/)**: Bevy's fully dynamic, real-time ray-traced lighting.
- **DLSS Ray Reconstruction**: NVIDIA's AI denoiser and upscaler.

It's based on the [OpenCE](../OpenCE) decompilation. See [ROADMAP.md](ROADMAP.md) for the architecture and phases.

**Status: Phases 1–6 done; Phase 7 (native Rust subsystems) under way: input is the viewer's, gamepads included.** With `--opence`, the viewer runs the actual game ([OpenCE](../OpenCE), hidden in the background) and you play it in the ray-traced window, with the game's HUD and effects drawn over it (see *Play*). Without it, the viewer loads a retail Xbox map directly from the `.map` file: its level geometry and every object the scenario places (scenery, vehicles, weapons, devices), with Halo's materials:
- detail maps and bump maps on level surfaces;
- alpha-tested grates and fences;
- reflections and shininess from Halo's specular settings;
- holograms, glows, shields, glass and water, drawn by Halo's own register-combiner programs;
- the sky model.

Lighting comes from the level's own data:
- the sky tag's sun becomes a directional light;
- the sky model, baked into a cubemap, becomes the environment light;
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

Other switches: `--no-objects`, `--no-fixture-lights`, `--show-light-proxies` (draw the light fixtures' emissive spheres), `--matte` (ignore Halo's reflection settings).

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
| F10 | Options: DLSS Ray Reconstruction and quality, ReSTIR, light levels, exposure, the stats text. They are saved (`%APPDATA%\halo_rt\settings.txt`) and used again next time. Turning DLSS back on may need a restart. |

The top-right panel shows GPU timings for each Solari pass and for DLSS-RR.

## Play

### One executable

```powershell
python tools/package.py
dist\halo_rt.exe          # the game's menus
dist\halo_rt.exe b30      # or straight into a level
```

`tools/package.py` builds OpenCE (`../OpenCE-bevy`), the Rust simulation (`halo_sim`, 32-bit) and the viewer with the `dlss` and `bundle` features. `dist\halo_rt.exe` then carries the game, the simulation and the DLSS runtime. Run, it unpacks them into `%LOCALAPPDATA%\halo_rt\runtime-<hash>` (once per build), starts the game hidden with every simulation system in Rust (`--simulation` names others, or `none` for the game's own C), and draws it path traced. The game's data isn't in it: the maps come from `HALO_MAPS`, `--maps-dir`, or a `maps` folder beside the executable. `--viewer` only views the map, as a build without the game does. Under the hood the game still runs in its own hidden 32-bit process: the C engine (its main loop, memory, menus, HUD and sound) runs the Rust simulation, and the viewer draws what it shares.

### From the source

Playing needs OpenCE built from its `bevy-bridge` branch, which shares the game's world with the viewer and takes input from it. The branch is checked out at `../OpenCE-bevy`. Build it as OpenCE's Windows README says (`python configure.py`, then `ninja windows`). Then:

```powershell
cargo run --release --features dlss -- b30 --opence ..\OpenCE-bevy\build\windows\halo.exe
```

The game starts hidden on the level (the map's folder's parent is its data root) and stops when you close the viewer. With `--menu` it starts at its own menus instead (`cargo run --release --features dlss -- --opence ..\OpenCE-bevy\build\windows\halo.exe --menu`), shown in the viewer. Pick a campaign level or a multiplayer game there. The viewer follows the game from level to level: the next campaign level, the menus, each game of a multiplayer session. The game's log is `halo_rt_opence-<viewer process id>.log` in the temporary folder; the viewer prints its path.

| Key | Action |
| --- | --- |
| Click | Play: the mouse and keyboard go to the game (Halo's own bindings) |
| Gamepad | Plays the game while the viewer has the focus (up to four, with rumble) |
| F12 | Let go of the mouse |
| F8 | Leave the game's camera and fly free (WASD/QE, right mouse to look); F8 again follows. While you fly, you hear the game from the flying camera. |
| F10 | The viewer's options (the mouse is the viewer's while they're open) |

The viewer's lighting hotkeys work while you're not playing. `--show-game` also shows the game's own window. `--bridge` attaches to a game you started yourself with `HALO_BEVY_BRIDGE=1` (and `--bridge-name` to one started with `HALO_BEVY_BRIDGE_NAME`).

Sound is the game's own: OpenCE plays it with 3D positioning, occlusion and reverb.

**Multiplayer.** Each viewer's game shares the world under its own name, so several viewers can run at once. System link works through them, with the armour and team colours drawn. To try it on one machine, give each game its own loopback address (OpenCE's `HALO_NET_ADDRESS` and `HALO_NET_BROADCAST`). Then host and join from the menus. OpenCE's test harness can also do it without the menus:

```powershell
$env:HALO_NET_ONLINE = "0"
# one terminal: host Blood Gulch
$env:HALO_NETWORK_TEST = "host:bloodgulch"; $env:HALO_NET_ADDRESS = "127.0.0.200"; $env:HALO_NET_BROADCAST = "127.0.0.201"
cargo run --release --features dlss -- --opence ..\OpenCE-bevy\build\windows\halo.exe --menu
# another: join it
$env:HALO_NETWORK_TEST = "join"; $env:HALO_NET_ADDRESS = "127.0.0.201"; $env:HALO_NET_BROADCAST = "127.0.0.200"
cargo run --release --features dlss -- --opence ..\OpenCE-bevy\build\windows\halo.exe --menu
```

The game also shares its own picture without the opaque world: the HUD, menus, cutscene letterboxing, particles, decals, fog, water and every transparent surface, drawn over a clear screen and composited over the ray-traced frame (the *game overlay*, `src/overlay.rs`). The viewer pairs each overlay picture with the world frame it was drawn for. The window is then a fixed 1280×720, the size the hidden game draws at, so the two pictures share a shape. `--no-overlay` leaves the overlay out and draws the transparent surfaces in Bevy, as before. For debugging, `HALO_RT_OVERLAY_DEBUG=1` tints the overlay's coverage red. With `HALO_RT_OVERLAY_DEBUG=2` and `HALO_BEVY_OVERLAY_DEBUG_WORLD=1` (which makes the game draw its opaque world too), the game's whole picture shows at half strength over the viewer's, so you can compare the two.

## Layout

| Path | What it is |
| --- | --- |
| `crates/halo_bridge` | The bridge to OpenCE: its shared-memory protocol mirrored, frame snapshots, input forwarding; `bridge_probe` to inspect a running game. |
| `crates/halo_map` | A Bevy-free reader for Xbox cache files: inflation, tag index, BSP geometry, models, object placements, bitmaps (swizzle, DXT/BC, P8 bump), shaders, transparent-shader combiner programs (built and evaluated), skies (and a CPU sky renderer), scenario. |
| `crates/halo_map/src/layout.rs` | Tag struct offsets, **generated** from OpenCE's C headers. |
| `tools/tag_layout` | The generator: `python tools/tag_layout/gen.py` compiles each `probe*.c` against `../OpenCE` with clang (i686 MSVC ABI) and reads the offsets back. Add a field to a probe and re-run it to expose it. |
| `src/` | The Bevy app: `level.rs` (BSPs, objects and the sky model → meshes and materials), `material.rs` + `shaders/` (the Halo environment material extension and the transparent combiner material), `textures.rs` (bitmaps → images, emissive and roughness bakes), `sky.rs` (sun, sky light and sky cubemap), `bridge.rs` (`--bridge`/`--opence`: the game's objects, camera and input), `overlay.rs` (the game's HUD, effects and transparent surfaces composited over the frame), `options.rs` (the F10 options, saved between runs), `main.rs` (camera, controls, DLSS, loading and following levels). |
| `examples/solari_demo.rs` | Bevy's Solari example (PICA PICA scene), kept as a reference: `cargo run --release --example solari_demo`. |

### `halo_map` CLI

```sh
cargo run --release -p halo_map -- info    maps/b30.map   # header, tag groups, player starts
cargo run --release -p halo_map -- bsp     maps/b30.map   # geometry stats + winding check
cargo run --release -p halo_map -- shaders maps/b30.map   # BSP shaders, emission, base maps
cargo run --release -p halo_map -- sky     maps/b30.map   # sky lights
cargo run --release -p halo_map -- bitmaps maps/b30.map   # decode every bitmap, report by format
cargo run --release -p halo_map -- objects maps/b30.map list   # placements; decode every placed model
cargo run --release -p halo_map -- skypano maps/b30.map sky.png # the sky model composited as a panorama
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
