# Halo: Combat Evolved on Bevy (Solari + DLSS) — Roadmap

Goal: run Halo: CE — from the [OpenCE](../OpenCE) decompilation (Xbox build 2342) and the retail
Xbox maps — with Bevy as the renderer, lit by Solari real-time ray tracing and DLSS Ray
Reconstruction.

## Starting point (what OpenCE is)

| Fact | Consequence |
| --- | --- |
| ~500k lines of decompiled C (AI 79k, rasterizer 44k, interface 42k, bitmaps 37k, game 32k, hs 28k, units 25k, …) plus a 95k-line port layer (`port/linux`). | A line-by-line Rust rewrite is years of work, so the port has to be incremental. |
| A **32-bit x86** program. Tag data is loaded at the fixed Xbox address `0x803A6000`, and every tag struct holds 32-bit pointers. | The game code can't be linked into a 64-bit process without rewriting every tag struct. The DLSS SDK only ships 64-bit libraries. |
| The port renders by **emulating Direct3D 8 + NV2A shaders on OpenGL** (`d3d8_gl.c`, `nv2a_psh.c`, `nv2a_vsh.c`). | That layer only sees triangles and textures, not a scene. Ray tracing needs scene data, so Bevy has to hook in *above* the rasterizer: game state → Bevy entities. |
| Retail maps are zlib-compressed Xbox cache files (`head` … `foot`, version 5). | The map format is well defined by OpenCE's headers. A native Rust loader is practical. |
| OpenCE's own code is CC0. The game data is Microsoft's. | We never ship or commit game data. The user supplies `maps/` (OpenCE extracts it from a disc image). |

## Architecture

```
            ┌──────────────────────── 64-bit Bevy process ────────────────────────┐
 maps/*.map │ halo_map (Rust)          halo_rt (Bevy app)                         │
 ─────────► │  cache file ─► tags ─►   BSP / models / bitmaps / shaders ─► meshes, │
            │  (layout generated       StandardMaterial, lights ─► Solari + DLSS   │
            │   from OpenCE headers)                                               │
            │                          ▲ per-frame game state (Phase 4)            │
            └──────────────────────────┼──────────────────────────────────────────┘
                                       │ shared memory: camera, objects, node
                                       │ matrices, effects   ◄── input
            ┌──────────────────────────┴───── 32-bit OpenCE process ──────────────┐
            │ OpenCE simulation (AI, physics, scripts, network) with rasterizer   │
            │ calls replaced by a render-state exporter                           │
            └──────────────────────────────────────────────────────────────────────┘
```

- **`crates/halo_map`**: a pure-Rust, Bevy-free reader for Xbox cache files.
  - Struct offsets are **generated from OpenCE's own C headers** by `tools/tag_layout` (clang, i686 MSVC ABI), so they are the compiler's numbers rather than hand-counted ones.
  - The generated file is committed, so OpenCE isn't needed to build.
- **`halo_rt`**: the Bevy app. It converts tags into Bevy assets and entities and owns rendering, the camera and the UI.
- **The simulation bridge** (Phase 4): OpenCE runs headless in its own 32-bit process and streams render state to Bevy. This gets the whole game playable without first rewriting 400k lines of game logic. Individual subsystems can still move to Rust later (Phase 7) behind the same interface.

Coordinates: Halo is Z-up and measured in world units (1 wu = 10 ft = 3.048 m). Bevy is Y-up and measured in metres. The mapping is `bevy = (x, z, -y) * 3.048`.

---

## Phase 1: Map loader + static level viewer ✅ (done)

Deliverable: `cargo run --release --features dlss -- b30` flies a free camera through The Silent Cartographer's level geometry, textured and lit by Solari with DLSS-RR.

- [x] Workspace layout: `halo_map` library, `halo_rt` app; the Solari demo moved to `examples/`
- [x] `tools/tag_layout`: generates struct offsets/sizes from OpenCE headers → `crates/halo_map/src/layout.rs`
- [x] Cache file: header check, zlib decompression, tag index, address → file-offset translation per image (tag data and each BSP)
- [x] Tag access: by datum, group or name; `tag_block` / `tag_reference` / `tag_data` readers with bounds checks
- [x] Scenario → structure BSP references → BSP header → `structure_bsp`
- [x] BSP geometry: lightmaps → materials → compressed vertices (11:11:10 normals) + surfaces, winding fixed for Bevy
- [x] Bitmaps: Xbox swizzle, DXT1/3/5 passthrough as BC1/2/3 with mips, A8R8G8B8 / X8R8G8B8 / R5G6B5 / A1R5G5B5 / A4R4G4B4 / A8 / Y8 / AY8 / A8Y8 → RGBA8 (P8 bump, cube and 3D textures moved to phase 3)
- [x] Shaders: base maps → `StandardMaterial`; shaders with radiosity emission → Solari emissive lights
- [x] Sky: the sun → directional light; the hemisphere sky light → environment light + skybox (a stand-in for skies without one)
- [x] Bevy viewer: meshes + `RaytracingMesh3d`, free camera at the player start, Solari, DLSS-RR, map/BSP selection, live light tuning, `--screenshot`
- [x] `halo_map` CLI: info / tags / bsp (stats + winding check) / shaders / sky / bitmaps (decode-all) / bitmap → PNG

Checked on b30 (2 BSPs, 58k triangles), a10 (9 BSPs, 364k triangles) and bloodgulch. All of their 2D bitmaps decode. Frames take about 2 ms of GPU time at 1280×720 with DLSS-RR on an RTX 5080.

### What Phase 1 taught us (format notes)

- Built maps have the `_bitmap_cached_bit` set, which means `pixels_offset` is already absolute. Adding the group's `pixel_data.file_offset` again (as `texture_cache_bitmap_new` does at build time) breaks every texture.
- BSP triangles are clockwise when seen from the front (100% of triangles in every BSP checked), so the viewer swaps two indices per triangle.
- The decompiled `sky_light` leaves colour, power and direction in `unused30`. They sit at the Halo Editing Kit offsets 0x50, 0x5C and 0x68.
- The Xbox build has no `shader_transparent_chicago_extended`, so the shader types run: environment 3, model 4, generic 5, chicago 6, water 7, glass 8, meter 9, plasma 10.
- Halo's light fixtures are mostly transparent-generic (`sotr`) shaders with radiosity emission. Drawn opaque and emissive, they light interiors on their own.

## Phase 2: World content and real lighting ✅ (done)

Deliverable: levels populated with their scenery, vehicles, weapons and devices; Halo's own sky behind them; light fixtures and self-illuminated surfaces lighting the scene.

- [x] Scenario placements for scenery, bipeds, vehicles, equipment, weapons, machines, controls and light fixtures, filtered as the game does (not-automatic flag, `on_bsp_flags`; multiplayer vehicles with no BSP flags are shown)
- [x] Models (`mode`): Xbox compressed vertices read through the tag header's D3D vertex buffers, triangle strips unrolled, regions/permutations (the scenario's choice), highest LOD, nodes; meshes made once per (model, permutation) and instanced
- [x] Object placement: the game's yaw/pitch/roll matrix (`matrix4x3_rotation_from_angles`) converted to Bevy
- [x] `shader_model` base map with the model's and shader's UV scales
- [x] Self-illumination: model shaders (multipurpose green mask × base map × colour) and environment shaders (self-illumination map R/G/B × primary/secondary/plasma colours × base map) baked into emissive textures; radiosity emitters keep their radiosity power
- [x] The sky model as camera-centred, raster-only, unlit, alpha-blended layers in Halo's order (each layer's main map for now)
- [x] Light fixtures (scenario colour × intensity) as invisible emissive spheres in the ray-traced scene, since Solari has no point lights
- [x] Tooling: `--camera` / P to reproduce views, `--no-objects`, `--no-fixture-lights`, `--show-light-proxies`; CLI `objects [list]` decodes every placed model
- [ ] Moved to later phases: sky layers captured into the environment cubemap (phase 3); `ligh` lights on objects and effects (phase 4/5, where they move with objects); the "baked" lightmap comparison mode (optional, phase 3); physically calibrated light units (phase 3)

Checked on b30 (916 objects, 170k triangles), a10 (466 objects, 245k triangles) and bloodgulch. Every placed model decodes, normals are unit length and 99.97% of triangles wind clockwise like the BSPs. GPU time is about 2 ms per frame with DLSS-RR.

### What Phase 2 taught us

- Xbox model parts leave their tag blocks empty. The geometry is reached through `vertex_buffer.hardware_format`, a D3D resource `{Common, Data, Lock}` in the tag header's array whose `Data` is a tag-space address (the game registers it with base `NULL`). `triangle_buffer.base_address` is the strip itself.
- Model vertices are in model space at the default pose (`render_model` multiplies node matrices by `runtime_default_inverse_matrix`), so static objects need only their object transform.
- On Xbox the multipurpose map's channels are R specular, G self-illumination, B colour change, A auxiliary. Halo PC moves them, and the port swizzles Custom Edition maps to match.
- Environment self-illumination adds to the lightmap, so it lights the base map rather than replacing it.
- Many light fixtures and glowing panels emit radiosity *and* self-illuminate. The radiosity power is their real light output; using the self-illumination colour instead made a10 go dark.

## Phase 3: Material fidelity ✅ (done)

Deliverable: Halo's materials as the game draws them. Solari lights the surfaces; the camera sees Halo's own shading layers.

The architecture: Solari traces rays against `StandardMaterial` only, but takes each pixel's material from the G-buffer the rasterizer writes. So a surface with a Halo material is two entities. A raster-only one carries a material extension or custom material (what the camera sees). A ray-traced-only one carries the plain `StandardMaterial` (what bounces, shadows and reflections see).

- [x] `shader_environment` detail layers: the Xbox pixel shader reproduced in WGSL (primary/secondary detail mixed by secondary or base alpha, the three detail functions, micro detail, gamma-space combine, rescaled detail maps)
- [x] Bump maps: palettized (P8) bump maps decoded through the renderer's vector palette into tangent-space normal maps, used at their own scale on the camera's surfaces and on the ray-traced ones where the UVs line up
- [x] Alpha test: environment shaders cut out by the bump map's alpha (the palette's zero-alpha entries), as the Xbox alpha-tests the bump pass
- [x] Specular and reflection: reflection brightness and specular masks (environment base alpha, model multipurpose red) become roughness and metalness, baked into a metallic-roughness texture where masked (`--matte` turns this off)
- [x] Transparent chicago and generic shaders: the Xbox preprocessors ported to build each shader's NV2A register combiner program; a WGSL interpreter evaluates it with the port's semantics. Covers holograms, glows, shields, decal-like surfaces, waves and the sky
- [x] Glass and water: blended, smooth surfaces with their tint and base maps that reflect the sky
- [x] The sky: its layers composited in order with Halo's combiners, and baked on the CPU (a Rust combiner evaluator plus ray casts) into the environment cubemap that lights the level
- [ ] Moved later: texture animation (UV scrolling, chicago map animation); reflection cube maps on transparent and environment shaders; meter and plasma shaders; detail maps on model shaders; light-unit calibration and a baked-lightmap comparison mode; 3D textures

On b30, every transparent surface is drawn except one meter and one plasma shader; on a10, all of them are. GPU time is still about 2 ms per frame with DLSS-RR.

### What Phase 3 taught us

- The environment shader's diffuse is `f(base, mix(secondary, primary, w))`, then the micro detail, with `f` one of `2ab`, `ab` or `a + 2b - 1`. A missing detail map is the game's 50% grey default, which is neutral for the "biased" functions.
- Halo's alpha test isn't the base map's alpha. The Xbox alpha-tests the pass where texture 0 is the bump map, and the P8 vector palette has seven zero-alpha entries for the cut-outs.
- The combiner programs are small and fully data-driven. Porting the two preprocessors and interpreting NV2A combiners covers every chicago and generic shader at once.
- Bevy sorts transparent meshes by the centres of their bounds, not their origins. Sky layers need a large negative depth bias (so they draw before the level's transparents) plus a step per layer bigger than the sky.
- A custom `Material` culls back faces by default. Sky layers are seen from inside, so they must be double-sided.

## Phase 4: Simulation bridge (playable)  ← *next*

- Build OpenCE headless (`HALO_BEVY_BRIDGE`), with the D3D8 rasterizer replaced by a render-state exporter at the `render_*` / `rasterizer_*` scene level
- Shared-memory protocol (versioned, double-buffered): camera, loaded BSP, visible objects (tag index, node matrices, permutation, colour change, shader parameters), dynamic lights, decals, particles/contrails, weather, first-person model
- Input and window events from Bevy → OpenCE input layer (`xinput_sdl.c` equivalent)
- Process management: spawn OpenCE, handshake, map loading, crash reporting
- Frame interpolation between sim ticks (30 Hz sim → display rate; see `render_interpolation.c`)

## Phase 5: Effects, HUD and presentation

- Particles, contrails, decals, lightning and lens flares (raster passes on top of Solari)
- HUD, menus and text (Bevy UI drawing OpenCE's widget output, or the bridge sending 2D draw lists)
- Active camouflage, plasma, screen effects, fog, water
- Cutscenes and cinematic letterboxing

## Phase 6: Audio and multiplayer parity

- Sound playback from the bridge (or a Rust Xbox ADPCM decoder + Bevy audio), with 3D positioning and sound environments
- Check that OpenCE's netcode/system link works through the bridge
- Options UI for ray-tracing quality and DLSS modes

## Phase 7 (long term): Native Rust subsystems

Move subsystems out of the 32-bit process one at a time behind the bridge interface, ordered by payoff:

1. Camera and input
2. Objects and physics
3. Effects
4. Scripts (`hs`)
5. AI
6. Networking

When nothing is left in the 32-bit process, the bridge goes away.

## Known risks

- **Solari is experimental.** It needs hardware ray tracing and supports opaque `StandardMaterial` only. Transparent and alpha-tested content needs a fallback.
- **Interior lighting.** Halo's interiors are lit almost entirely by baked radiosity. To look right under Solari, they need emissive light sources recovered from shaders, lens-flare markers and `ligh` tags (Phase 2).
- **32/64-bit split.** The shared-memory bridge has to stay cheap. A frame's state is small (hundreds of objects × nodes), so this should be fine.
- **Legal.** Never commit or redistribute game data or DLSS binaries.
