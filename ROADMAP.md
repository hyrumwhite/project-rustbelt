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

## Phase 4: Simulation bridge (playable) ✅ (done)

Deliverable: `cargo run --release --features dlss -- b30 --opence <OpenCE>\build\windows\halo.exe` starts the game hidden and plays it in the Bevy window, ray traced.

OpenCE side, branch `bevy-bridge` (worktree `../OpenCE-bevy`):
- [x] `port/linux/game/bevy_bridge.c`: with `game.bevy_bridge` on, `render_frame` writes the first player's camera and every object's interpolated node matrices, definition, region permutations and visibility to shared memory (`bevy_bridge_protocol.h`; two frame buffers with sequence locks; a named file mapping on Windows, POSIX shm elsewhere)
- [x] The first-person weapon and hands, recorded as `first_person_weapon_draw` poses them
- [x] Input from the renderer: `bevy_bridge_input.c` turns the forwarded keyboard and mouse into SDL events before the platform layer polls, so bindings, menus and aiming work unchanged
- [x] `game.bevy_bridge_map`: a level to load at start-up

Viewer side:
- [x] `crates/halo_bridge`: the protocol mirrored in Rust, snapshot reading and input writing; `bridge_probe` (with `walk`) checks it end to end without a window
- [x] `--bridge`: the game's objects, spawned as they appear, split by node and posed every frame at `node matrix × default inverse` (the tag's baked `runtime_default_inverse_matrix`), hidden as the game hides them, despawned when deleted; the camera follows the game's (F8 flies free)
- [x] Playing from the viewer: click to grab the mouse, F12 to let go; the viewer's hotkeys pause while playing
- [x] `--opence <halo.exe>`: starts the game hidden on the map's scenario and stops it with the viewer; `--show-game` keeps its window
- [x] Frame interpolation comes from the game (`render_interpolation.c`): it exports at display rate
- [ ] Moved to phase 5: true two-node skinning (bipeds use a rigid per-node split now, with small seams at joints); change colours; shader animation from object functions (the rifle's ammo digits); effects, decals and dynamic lights; showing only the active BSP

Checked: b30's opening cutscene (Pelicans, the troop bay landing), first-person gameplay with the assault rifle, a10's opening. A scripted walk through the bridge moves the player. Bridged starts are stable: 8 of 8 a10 starts clean with DLSS-RR running.

### What Phase 4 taught us

- OpenCE already blends node matrices and the camera between ticks for display-rate frames. Exporting from `render_frame` gets those blended poses for free.
- Halo's node default inverse matrices are baked into the cache file, so the renderer needs no skeleton maths of its own.
- Injecting SDL events is the narrowest input seam. Nothing in the game or the port's input code had to change.
- DLSS creates its feature with GPU work that wgpu doesn't track. If that happens during the window's first surface configure, wgpu's idle wait fails and Bevy quits. Starting the camera 30 frames in avoids the race. Adding DLSS to a camera that's already rendering doesn't work: it stays off.

## Phase 5: Effects, HUD and presentation ✅ (done)

The approach: the game draws everything that isn't opaque world, and the viewer composites it. With `game.bevy_bridge_overlay` (OpenCE branch `bevy-bridge-overlay`), `render_window` draws the opaque world only into the depth buffer. It draws the rest over a clear screen, keeping coverage in alpha: alpha blends add coverage, additive blends leave it. `Present` reads the picture back into the bridge (protocol 4). The viewer draws it with premultiplied blending, paired with the world frame it was drawn for. Effects are depth-tested against the game's own copy of the world, so they're occluded correctly by the geometry the viewer ray-traces.

- [x] The overlay: protocol 4, the depth-only world, coverage alpha, the readback; `src/overlay.rs` composites it and pairs frames; the viewer skips its own transparent surfaces; `--no-overlay` turns it off
- [x] Through the overlay: the HUD, menus and text; particles, contrails, decals, lens flares, weather; fog and water; transparent surfaces; cinematic letterboxing
- [x] Fixed on the way: the camera's field of view in cinematics. The game's frustum is wider than its camera's FOV there, because the window bounds sit inside the viewport. The bridge now shares the frustum's FOV, and cutscene objects no longer look 18% too close.
Carried over, then done in the rendering pass (branch `rendering-pass`, OpenCE `bevy-bridge-rendering`, protocol 9):

- [x] The rifle's ammo digits: they are transparent chicago shaders (numbers, compass, icons), drawn with the first-person weapon inside `render_objects`, so the overlay's depth-only world swallowed them. Its blended draws now go to the overlay. Change colours were done in phase 6. Opaque shaders animated by object functions (pulsing Covenant lights) are still static.
- [x] Two-node skinning: each model part is one skinned mesh (Halo's two nodes and weights per vertex), with the node entities as joints. Solari traces no skinned meshes, so the rays keep the rigid pieces, 2 cm inside the skinned surface to avoid self-shadowing blotches. The jagged shards under the first-person rifle are gone.
- [x] Only the active BSP is shown, along with its placed light fixtures. Solari traces every `RaytracingMesh3d` whether visible or not, so hidden meshes now leave the ray-traced scene (`TracedMesh`).
- [x] Active camouflage: drawn as a faint glassy shell, out of the ray-traced scene (no shadow). Under the overlay the game neither draws its shimmer, which refracts a picture it no longer has, nor the camouflaged model.
- [x] Dynamic lights: up to 64 a frame from the game's light system (muzzle flashes, plasma, explosions, the flashlight), each lighting an emissive proxy the rays see: a sphere, or a disc for a spotlight.
- [ ] Plasma and other opaque-world effects the overlay can't carry
- [ ] Multiply-blended effects: they darken what's under them, and coverage alpha can't represent that, so they are lost over the viewer's world

Checked: b30's opening cutscene (letterbox, engine glows, water, fog) lined up with the ray-traced Pelicans; b30 gameplay with the full HUD; a10's opening with the Pillar of Autumn's engine glows. 4 of 4 a10 overlay starts were clean.

### What Phase 5 taught us

- Letting the game draw its own effects over a depth-only world is far cheaper than porting each effect system, and it stays faithful: the effects are the game's own.
- Coverage alpha only works for blends that replace or add to what's under them. Multiplying blends need the world's colour, which the game no longer has.
- Compare the two renderers directly: a half-strength blend of the game's full picture over the viewer's found the cinematic field-of-view bug in one screenshot.

## Phase 6: Audio and multiplayer parity ✅ (done)

OpenCE branch `bevy-bridge-multiplayer` (protocol 6).

- [x] Sound: the hidden game already plays it (OpenCE's mixer: Xbox ADPCM, 3D positioning, occlusion, I3DL2 reverb). The bridge's input block now carries a listener, so with the viewer flying free (F8) the game hears from the viewer's camera.
- [x] Level changes: the viewer follows the game's level (the menus, the next campaign level, each multiplayer game). It unloads the old level (everything sits under one root entity) and loads the new map, with its sky and sun. `--menu` starts the game at its menus, which the overlay shows and the forwarded input drives.
- [x] A bridge per game (`game.bevy_bridge_name`): each viewer's game shares under its own name and writes its own log, so several viewers run side by side
- [x] System link through the bridge. Checked with OpenCE's network test harness on one machine: one viewer's game hosted Blood Gulch and the other's joined. Both games logged the same player positions every tick, and each viewer showed its own player's view, with the other player drawn.
- [x] Change colours (from phase 5): the bridge shares each object's four change colours, and the first-person arms' and weapon's. Model shaders tint their base maps where the multipurpose map's alpha says, as the Xbox shader does, so armour, team colours and Blood Gulch's autumn trees are right.
- [x] Options UI (F10): DLSS Ray Reconstruction and quality, ReSTIR, light levels, exposure and the stats text, saved between runs
- [ ] Internet play (OpenCE's invite links) through a viewer: untested

### What Phase 6 taught us

- The hidden game already did most of the work: its own mixer plays positioned sound, and its own menus drive multiplayer through the overlay. The viewer's job was to follow it: reloading levels, a bridge per game, and the camera as the listener.
- OpenCE's network test harness (`HALO_NETWORK_TEST`, loopback addresses per game) makes a two-machine system link check scriptable on one PC.

## Phase 7 (long term): Native Rust subsystems  ← *in progress*

Move subsystems out of the 32-bit process one at a time behind the bridge interface, ordered by payoff:

1. Camera and input ✅ (OpenCE branch `bevy-bridge-native-input`, protocol 7)
   - [x] Input is the viewer's: keyboard and mouse (phase 4), and now gamepads. Bevy reads the controllers and the bridge forwards up to four in SDL's layout. While the viewer owns them, the game's XInput layer takes them instead of physical pads, which a hidden window never receives. The game's rumble comes back and plays on the controller.
   - [x] Camera: measured, and kept in the game. The game's shared camera follows the mouse within one frame (median 7.5 ms, at most 9.1 ms, at 144 frames a second; `bridge_probe latency`). A Rust camera predicting the aim could save only that. It would also put the game's world-anchored overlay (water, effects, decals, drawn with the game's camera) out of step with the scene on fast turns.
2. Objects and physics  ← *in progress* (branch `phase-7-collision`, OpenCE `bevy-bridge-collision`, protocol 10)

   Each piece is a port of the game's own code, checked against the running game through the bridge before the next one is built on it.
   - [x] Collision BSP and the ray test (`halo_map::collision`, `collision_bsp_test_vector`): 483,000 rays through b30 and a10 agree with the game bit for bit (`bridge_probe <map file>` with `HALO_BEVY_BRIDGE_RAY_TESTS=1`)
   - [x] Sphere and pill tests (`collision_bsp_test_sphere`, `collision_bsp_test_pill`): the surfaces, edges and vertices near a point, and where a sweeping sphere first touches the structure, as movement uses them. The same fan of rays checks them: 414,000 spheres (34 million features touched) and 483,000 sweeps in b30 and a10 match the game exactly. Feature lists match in order; sweeps match bit for bit in t, surface and normal. One detail mattered: C's `fabs` is the double one, so the game computes a sweep's plane contact time in double.
   - [x] Object collision models (`collision_model_test_vector`): a ray against an object's collision BSPs, one per node and region permutation, in each node's space (the game's own `matrix4x3_inverse` and transforms). 141,000 rays that hit Pelicans, Marines, crew, doors and cryo tubes in b30 and a10 match the game bit for bit: object, node, BSP, surface and t.
   - [x] The game's whole ray test (`collision_test_vector`) with its object broad phase (`collision::World`). The structure ray's recorded leaves give the clusters to search. Each connected object with a collision model is registered in the clusters its bounding sphere reaches through portals (`structure_clusters_in_sphere`, `sphere_intersects_cluster_portal`, `convex_hull2d_test_circle`). Objects are tested once each, children inside their parent's sphere, nearest hit winning. 483,000 whole tests in b30 and a10 match the game bit for bit (134,000 of them object hits). The objects' bounding spheres and location clusters still come from the game (protocol 14), until the object simulation moves.
   - [x] Water in the whole ray test: each cluster's fog plane, moved by its fog's plane distance, as `collision_cluster_fog_plane` finds it; and the test's full result (what it hit, the plane facing the ray, the point, the location)
   - [x] Vehicles' physics shapes in the ray test (`halo_map::vehicle_physics`, `physics_instance_new` and `physics_test_vector`, through `collision::World` with `test_flags::USE_VEHICLE_PHYSICS`). A vehicle's physics tag's mass points, placed by its origin and axes and moved back by the centre of mass, are tested as spheres instead of its collision model. The game itself sets the bit only for debugging (`collision_debug_flag_use_vehicle_physics`), so the bridge tests rays from the camera at each vehicle with it and records the vehicles' placement (protocol 26). 378,600 such rays in b30 and a10 agree with the game bit for bit in what they hit, t and plane, 47,682 of them on b30's vehicles' mass points.
   - [x] Point physics (`halo_map::physics`, `point_physics_update`): the first simulation in Rust, which moves particles, contrails, antennas and flags. It covers gravity and buoyancy in air or water, friction towards the wind, and up to three bounces a step off the structure and water. Each frame the game launches 16 points from the camera, and Rust runs the same 90 steps. 552,000 points through 9.9 million bounces in b30 and a10 (11,000 of them into b30's water) end on exactly the game's position and velocity, bit for bit. Wind is an input; its runtime state is still the game's.
   - [x] Projectile flight (`halo_map::projectile`, `projectile_update`): timers, deceleration, gravity and range, flight through the structure and water, and each material's response (disappear, detonate, reflect, overpenetrate, attach) with its noise and friction, using the game's random numbers. The game records every projectile update along with the random seeds it began with, and Rust replays them from the same state. 6,700 updates of 1,400 projectiles in b30 (790 collisions) leave exactly the game's state, bit for bit.
   - [ ] Projectiles against objects (damage, attaching to a unit), guided projectiles, and detonation effects
   - [x] Biped movement physics (`halo_map::biped`, `biped_update_physics`, on `halo_map::features`, Halo's collision features and `collision_move_pill`): walking along the ground with its uphill and downhill speeds, falling, flying, sliding along up to three planes, being held to ledges, finding the ground, and the crouching player's stand test, with breakable surfaces as the game has them. The game records each update's physics, and Rust replays it. 131,000 updates of 95 bipeds in b30, a10, c10 and c20 (2,700 of them flying, 48 held to a ledge) leave exactly the game's physics, byte for byte.
   - [x] Bipeds against objects (`halo_map::features`, `object_get_features_in_sphere`): other bipeds' pills, and vehicles', scenery's, machines' and controls' collision models in their nodes' space, plus what a biped bumped, the elevator it rides, and volatile collisions. The game records each object its broad phase visited during an update, as that object was then; Rust works out the object's features and replays the whole update. 213,000 updates in b30 (two runs), c20, b40 and a30 agree byte for byte, 4,900 of them bumping an object; in the three runs that broke the bumps down, 627 were bipeds, 17 vehicles, 3,244 scenery and 3 machines. Not yet checked: riding an elevator, and vehicles' physics shapes (no biped test asks for them).
   - [x] Biped movement control (`halo_map::biped_moving`, `biped_update_moving`). It turns the throttle, crouch, the animation's root motion (read from the map's animation graphs, `halo_map::animation`) and the definition into the physics' input, through player speeds, animation-driven movement or flying. Afterwards it applies the result: the biped moves and finds its location, keeps what it stands on, starts landings, jumps and counts bumps. It also covers snap facing and turning by the animation. The game records each update's movement state and what it asked of the rest of the game (AI jump aim and leaping, difficulty, cinematics, aiming). 159,586 updates in b30 and c20 leave exactly the game's state, byte for byte, and every physics input Rust builds is the game's. That includes 5,367 with player physics, 2,671 flying, 2,027 turned by the animation, 130 landings and 4 jumps. Falling and melee damage, effects and the AI's notices remain the game's.
   - [x] The biped update up to its animation (`halo_map::biped_update`, `biped_update`). It covers snap facing, the airborne and slipping counters, turning (on the ground, climbing, and flying with banking through `unit_euler_aiming_update`'s acceleration planner), the movement, and the dead, airborne (tumbling, from the game's random numbers) and landing updates. 160,660 updates in b30 and c20 leave exactly the game's state, byte for byte: 23,193 dead, 3,487 airborne, 480 landing, 2,672 flying turns, 27,389 moving turns and 227 tumbles.
   - [x] Unit animation state (`halo_map::unit_animation`, `unit_update_animation` and `unit_animation_set_state`, with the graph's seats, weapon classes and weapon types read from the map). It picks the stance from the AI's state and the magic seats, finds animations by seat and weapon label, and advances the base, soft ping, action and overlay animations (key frames, loops, restarts on a permutation drawn from the game's random numbers). It enters the state asked for when the current one may be interrupted, with its aiming and looking screens and interpolation. Grenade releases, melee damage, destruction, limp bodies and seat changes are reported, not done. 171,934 updates of bipeds in b30 and c20 agree with the game, including 1,884 states entered, 356 seat or weapon changes, 3,135 restarts and 22,639 with side effects.
   - [x] Object poses (`halo_map::pose`). An object's node orientations come from its animation's frame (`animation_get_node_orientations`, the plain 8-byte quaternion frames and the keyframe-compressed ones with 6-byte quaternions) or its model's default pose, then its scale. Its node matrices come from the interpolation from the previous pose and the node hierarchy, placed by its position, axes, mirroring, centre of mass, origin offset and parent node (`object_compute_node_matrices`). The game records a sample of poses. 115,333 in b30, a10 and d40, of every kind of object (bipeds, vehicles, weapons, equipment, projectiles, scenery, machines, light fixtures), have exactly the game's orientations and node matrices, bit for bit, including 117 from compressed animations, 541 interpolating and 9,171 hanging from a parent.
   - [x] A unit's preprocess of its pose (`halo_map::unit_pose`, `unit_preprocess_node_orientations`, with the animation applies in `halo_map::pose`: replacement, overlay, scaled, continuous and aiming screens). Over the base animation go the action, the overlay action and the soft ping, the seat's emotion, mouth and acceleration overlays, the weapon class's aiming screen at the aim relative to the body, and the seat's looking screen at the look relative to the aim, setting the unit's euler screens and their bounds. The port's grenade throw on the move comes first: the legs kept in the walk (blended by speed and direction, eased a step a tick), floating in the air, crouched, or seated on a rider. The game records each sampled biped's inputs to it (protocol 23). 30,342 bipeds' preprocessed poses in b30 (twice, once throwing grenades on the move), a10 and d40 have exactly the game's orientations, euler screens and bounds, bit for bit, including 22,752 aiming, 22,187 looking, 172 actions, 1,500 overlay actions, 248 soft pings, 1,211 emotion, mouth or acceleration overlays and 22 grenade throws on the move (4 in the air); crouched throws and riders' throws were not exercised.
   - [x] Objects' overlay animations and the other types' preprocesses. An object's graph's overlays, driven by its outgoing function values, play a frame the value picks or a frame the time picks, scaled by the value (`halo_map::pose::object_overlays_apply`). A vehicle's preprocess, after the unit's, applies its steering screen at its turn, overlays for its sideways velocity, speed, forward velocity and wheel rotation, and one per suspension (`halo_map::vehicle_pose`). A device's applies its position overlay, interpolated or not, looping or not, reversed or not, and its power overlay (`halo_map::device_pose`). A weapon's does nothing. The game records the vehicle's and device's state with each sampled pose (protocol 24). In b30, a10 and d40, all 250,151 sampled poses' orientations agree with the game bit for bit, 1,950 of them with object overlays. Every type's preprocess agrees too: 15,771 bipeds, 6,434 vehicles (1,629 in a10 with wheels turning and suspensions compressed), 20,543 weapons, 6,038 devices (3,106 changed by position or power) and 201,365 others.
   - [x] A player's melee (`halo_map::biped_update::player_melee`, with `unit_animation_start_action` in `halo_map::unit_animation`). A player asking for one, with a weapon that allows it (not overcharged) or with none outside a seat, not zoomed, starts one. It lasts the weapon's first-person melee animation's frames with the hit at its key frame, both sped up by a quarter (`weapon_get_first_person_animation_time`). The unit's melee action plays a permutation of its weapon type's melee animation, drawn from the game's random numbers, and the damage lands at its tick. The game records each player's melee step (protocol 25). 6,156 steps in b30 agree with the game, including 60 melees started with an assault rifle and their 60 hits, with the action, its animation and the random seed after. Not exercised: melees with no weapon (a port-only gametype loadout). Not ported: the PAL maps' frame counts.
   - [ ] Vehicles (`physics.c` mass points), then the object simulation handing off from the game to Rust
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
