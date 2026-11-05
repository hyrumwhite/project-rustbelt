# Plan: steady lighting in dim interiors and big outdoor spaces, within budget

Drafted 4 November 2026, from the code on `phase-7-collision`, `docs/boiling.md`, the 3 and 4 November play findings and the session notes. Companion to [boiling.md](boiling.md), which records what was tried.

## Summary

Four structural causes remain, all visible in the code. Each phase below removes one, in order of effect per millisecond, and each has a switch for A/B in play and a measured gate. Phase 1 costs no GPU time and should take the largest bite out of the elevator shafts, the light strips and the Library halls. Phase 2 is a few shader lines and targets the canyon wall from a Banshee, the light collecting at wall-floor seams and the rocks that light up. Phases 3 and 4 spend small, bounded amounts of GPU time only where the first two leave noise, and Phase 5 pays for them.

Target (confirmed by the user, 4 November): 60 frames/s at 1280x720 DLAA. Budget rule for every phase: b40's canyon and b30's beach keep their current GPU total (9.3 ms at b40's fight spot), and no spot rises more than 0.3 ms over its own baseline below.

### The spots and their baselines

Each is a capture in `dist/captures`; the `.txt` holds the `--camera` line for the level viewer and `HALO_SIM_TELEPORT` for the game.

| Spot | Capture | Camera | GPU total | Direct | Cache | Diffuse GI | Specular GI |
|---|---|---|---|---|---|---|---|
| c20 dark hall (holo panels the only light) | `c20_1793811734634` | `61.16,-32.86,10.52,-144.5,29.7` | 6.46 ms | 1.84 | 1.20 | 1.04 | 0.76 |
| b40 elevator shaft (strip glyphs) | `b40_1793753064718` | `269.18,-588.08,23.09,177.4,-19.1` | 8.96 ms | 2.69 | 1.33 | 1.45 | 1.66 |
| b40 canyon wall from a Banshee (overcast, sky-lit, in motion) | `b40_1793753732789` | `277.89,-649.78,63.10,-80.3,22.2` | 8.16 ms | 2.40 | 1.32 | 1.54 | 0.99 |
| a10 Jefferies tube (guard: fixed) | `a10_1793827386171` | see its `.txt` | 6.25 ms | 1.83 | 1.14 | 0.79 | 0.94 |

The shaft's capture is the costliest interior (its specular pass is twice the tube's: the strips' glossy metal), so it has the least headroom. The canyon capture is one frame of a flight; the viewer reproduces the motion with `--dolly` along the Banshee's heading, and the game test is the user in a Banshee.

## What is still structurally wrong

### 1. A "light" is a lightmap page's material, not a fixture

`src/level.rs` `spawn_bsps` makes one mesh per (lightmap, material) of the BSP, and Solarik makes one light source per emissive mesh (`binder.rs`, `new_emissive_mesh_light`). A light sample picks a triangle of that mesh uniformly by index (`sampling.wesl` `resolve_light_sample`: `rand_range_u(triangle_count)`), wherever that triangle is. So one light can be every strip of a lightmap page, across several floors of a shaft, and most of its triangles are out of sight of any given wall.

The world cache's light slots hold a light id only (16 bits, `world_cache_slot`). A cell that has "found" its strip's material still draws, from that slot, any triangle of the page. The 8 cell candidates in `restir_di.wesl` therefore mostly land on far or hidden triangles, and the slot weighting by irradiance works against a whole page, not a fixture. This is why the cell candidates fixed the a10 tubes (a short page) and not the shafts. The 22 October "split by fixture did no better" test was in the tubes, where the page was already small, so it says nothing about b40 or c20.

### 2. Light selection ignores where the camera is

The alias table is rebuilt every frame on the CPU from each light's power to the 0.5 (`binder.rs` `prepare_raytracing_scene_bindings`). The viewer already knows the camera's cluster and the BSP's potentially visible set (`bridge.rs` `ClusterVisibility::cluster`, `sees`) and uses them to hide objects the game would not draw. None of that reaches the selection. `EMITTER_LIGHT_GAINS`, `LIGHT_DIMMING` and the flux exponent are all per-shader stand-ins for "give the room's lights the room's samples".

### 3. Cache cells are born in the wrong place, from one sample

`world_cache_query.wesl` `query_world_cache_with_support` jitters the query point by half a cell in the tangent plane for the lookup, then stores that jittered point as the new cell's position. At a floor-wall seam, that point can lie behind the wall, outside the sealed hull, where the sun and sky reach it, or inside a rock. That fits "light collecting where walls meet floors in dark rooms" and "rocks light up with no clear cause": the leak is the cell's own position, not the BSP's seams.

A new cell's first update replaces it wholesale (`cache_blend_amount` with `sample_count` 1): 4 shadow-tested light samples and one GI ray from one frame stand as the cell's value until later updates blend in at 1/n. Cell size follows camera distance (`get_cell_size`), and cells die after 10 unqueried frames, so a moving camera (a Banshee, or a walk down a dim hall) makes new cells all the time. ReSTIR GI reads the cache raw at its hit (`restir_gi.wesl`, `reservoir.radiance = query_two_sided_world_cache`), so the picture of a dim wall is a patchwork of one-sample cells, which DLSS-RR keeps as detail. This also explains why still-camera measurements understate what the user sees.

### 4. Every candidate pool draws from the same global table

ReSTIR DI takes 32 tile candidates and 8 cell candidates per pixel; the cache's search takes 128 tile candidates and 16 slot candidates per cell. All tile candidates come from the one global table. With causes 1 and 2 in place, few of a shaft pixel's 40 candidates have any contribution, and the reservoir's pick is effectively one of two or three lucky draws, which reuse then spreads over 30 pixels and 8 frames.

## Phase 0: the harness (half a day)

The measuring tools live in an old session scratchpad and can vanish. Bring them into the repo and add the three views that test the hypotheses above.

Done on 4 November, in `scripts/boil/`:

- `noise.py`, `blotch.py`, `three_shot.py`: the measures, brought in from the old scratchpad.
- `spots.txt`: the spots (name, map, BSP, camera, dolly reach) and the guards. The BSP of a capture's position comes from the new `halo_map` example `point_bsp <map> x,y,z`, which also gives its cluster (for Phase 1).
- `run.sh <label> [spot...]`: one table per label with the GPU pass times, still wander and three-shot, 4-frame noise, and in-motion wander and three-shot, under `scripts/boil/out/<label>/`.
- `views.sh <label> <spot> [view...]`: the diagnostic views at a spot, raw (`HALO_RT_NO_DLSS=1`), still and in motion.
- Three debug views behind `SOLARIK_SHADER_DEFS`, beside `VISUALIZE_CELL_LIGHTS`:
  - `VISUALIZE_CACHE_AGE` (`specular_gi.wesl`): the samples in the cell each pixel's diffuse bounce reads, red at one to green at the full history; magenta no cell. Tests cause 3 in motion.
  - `VISUALIZE_DI_PICK` (`restir_di.wesl`): red the share of the 32 tile candidates with any contribution, green the share of the 8 cell candidates, blue the pick was shadow-tested dark. The later passes leave the view alone. Tests causes 1 and 4.
  - `VISUALIZE_CELL_LEAK` (`specular_gi.wesl`): red where the cell's stored position is out of the surface's sight. Tests cause 3's leak half.

Gate: the table runs on every spot from one command, before any lighting change, and the three views show what the diagnosis predicts (young cells where it boils in motion, few live candidates in the shaft, leaked cells at the seams).

### Phase 0 findings (4 November, before any lighting change)

Raw (no DLSS), from `views.sh base <spot>` and `view_shares.py`, shares of the view:

| Fact | c20 hall still | c20 in motion | b40 shaft still | b40 shaft in motion | b40 canyon still | b40 canyon in motion |
|---|---|---|---|---|---|---|
| The initial DI pick was shadow-tested dark and dropped (`VISUALIZE_DI_PICK` blue) | 63% | 59% | **92%** | 93% | 57% | 57% |
| Mean share of the 32 tile candidates with any (unshadowed) contribution | 14% | 9% | 52% | 52% | 56% | 56% |
| Pixels where none of the 32 had any | 60% | 68% | 4% | 3% | 11% | 11% |
| Mean share of the 8 cell candidates with any | 22% | 19% | 50% | 50% | 40% | 39% |
| The pixel's surface finds **no cache cell** at all (`VISUALIZE_CELL_LEAK` magenta) | 73% | | 27% | 29% | 18% | 19% |
| Finds a cell whose stored position is out of the surface's sight (red) | 2% | | 2% | 2% | **38%** | 38% |
| Diffuse bounce reads a cell under 4 samples (`VISUALIZE_CACHE_AGE` red, first version) | | | 1% | 1% | 6% | 6% |

(The canyon's "in motion" is the dolly's 4 m sway, about 10 m/s, slower than a Banshee.)

The baseline table (`run.sh base`, level viewer, DLAA, the spot's BSP alone; GPU in ms, the rest the measures of `boiling.md`):

| spot | gpu | direct | cache | diffGI | specGI | still wander | still p90 | still 3-shot | 4-frame noise | noise p95 | motion wander | motion 3-shot |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| b40_shaft | 10.91 | 2.89 | 1.36 | 1.97 | 1.70 | 0.047 | 0.106 | 0.185 | 0.330 | 1.11 | 0.064 | 0.265 |
| b40_canyon | 8.76 | 2.39 | 1.28 | 1.52 | 0.71 | 0.003 | 0.005 | 0.012 | 0.065 | 0.22 | 0.005 | 0.021 |
| c20_hall | 6.52 | 1.67 | 1.20 | 0.90 | 0.56 | 0.028 | 0.047 | 0.094 | 0.249 | 0.67 | 0.041 | 0.141 |
| a10_tube (guard) | 6.84 | 1.74 | 1.08 | 0.86 | 0.93 | 0.002 | 0.004 | 0.007 | 0.011 | 0.05 | 0.005 | 0.022 |
| a10_corridor (guard) | 7.57 | 1.86 | 1.12 | 1.00 | 1.28 | 0.004 | 0.009 | 0.020 | 0.110 | 0.29 | 0.008 | 0.049 |
| b30_passage (guard) | 8.72 | 1.99 | 1.21 | 1.27 | 1.67 | 0.004 | 0.007 | 0.015 | 0.058 | 0.22 | 0.008 | 0.042 |
| b30_beach (guard) | 6.58 | 1.36 | 1.16 | 0.95 | 0.42 | 0.005 | 0.011 | 0.031 | 0.197 | 0.54 | 0.007 | 0.048 |

The shaft and the hall measure an order of magnitude above the guards on every stability column, so the harness sees the two reports. The canyon does not move at all in the viewer: with a still or slowly swaying camera the leaked cells stay where they are, a fixed patchwork that a change measure cannot see; in a Banshee the cells are reborn as the camera goes and the patchwork moves. A faster spot (`b40_canyon_fast`, 20 m each way, about 50 m/s) was added for that and is just as steady (0.006 wander, 0.025 three-shot in motion), so the viewer does not reproduce the report at flight speed either. What the viewer lacks there: the game's snow, the Banshee's own lights moving with the camera (compare the plasma pistol in the tubes, 22 October), and the lit fog. The canyon's test is therefore the user in a Banshee, A/B with `CACHE_CELL_UNJITTERED` (the leak fix) and, if it persists, `HALO_RT_LIGHT_LOG` for a light riding with the vehicle.

Three different failures, one estimator:

- **On the canyon wall the cache cells sit inside the rock.** 38% of the wall's pixels read a cell whose stored position the surface cannot see: the jittered query point, half a cell off in the tangent plane of a rough rock, lands inside it, and the cell's sun and bounce rays start there. Indoors on flat walls it is 2%. This is cause 3's leak half, and the "rocks light up" and seam reports. `CACHE_CELL_UNJITTERED` (`world_cache_query.wesl`) stores the query's own point instead; the leak view with the def is its gate (red should go to a few percent). **Result (4 November, `unjit` views): the canyon's leaked cells 38% → 5%.** To be made the default once the table shows no guard moving, and A/B'd by the user on the rocks that lit up.

- **In the shaft the pool is full of lights the pixel cannot see.** Half the candidates have unshadowed contribution, the resampling picks the brightest of them, and 92% of the time the shadow ray kills it. Nine pixels in ten get no fresh direct sample a frame and shade from reuse alone; whatever reuse carries wanders. The strips on the far side of a wall, and the level's other lights, win the candidates over the strip beside the pixel. That is exactly what Phase 1 (lights by cluster, selection by the camera's visible set) is for, and the view is its gate: the dropped share must fall.
- **In the hall the pool is nearly empty.** Six pixels in ten have no live tile candidate at all (the level's 710 emitters are mostly elsewhere), and three quarters never get the cell candidates: the whole lit wall is magenta, the pillars green. The lookup fails on the big faces and succeeds where the normal map's rings tilt the normal, which points at the key: the cells are keyed by the GI hits' geometric (triangle) normal, the camera's pixel looks them up by its shading normal (the interpolated vertex normal, normal-mapped), and on these faces the two quantise to different keys. The 22 October fix (trying both sides of a step) covered a wall's zero component, not this.

This moves a lookup fix ahead of Phase 1: `pixel_geometric_normal` (`gbuffer_utils.wesl`) rebuilds the surface's geometric normal from the depths round the pixel, and `CELL_LOOKUP_GEOMETRIC` keys the DI lookup and the views by it. Its test is the same leak view with the def: the magenta should go.

The first cache-age view keyed its lookup by the GI reservoir's shading normal and read half magenta for the same reason; it now records the sample count inside the real query (`world_cache_debug_samples`). Re-run, it reads 99% full-history cells at the still c20 hall, 75% at the shaft (23% of bounces read no cell: emitter or sky hits), and 61% at the canyon (4% young, 28% no cell).

**The geometric lookup is not the answer** (`geom` views): at the c20 hall the no-cell share went 73% to 65% only, at the shaft 27% to 32%, and at the canyon 18% to 40% (a rough rock's plane normals scatter over more keys than its smoothed vertex normals). The pillars' faces are now fully found and the wall's rings, which found cells by the shading normal, no longer do; the wall's body stays unfound either way. So the wall's cells sit under a normal key that is neither its plane's nor its flat shading normal's. The fact behind it: a ray hit's "geometric" normal in `raytracing_scene_bindings.wesl` is the interpolated vertex normal (`geometric_world_normal = world_normal`), not the triangle's, so the cells are keyed by the mesh's smoothed vertex normals, and the camera's G-buffer normal (the Halo raster material's own prepass, bump-mapped) evidently differs from it across this wall by a whole key step. `VISUALIZE_CELL_KEY_PROBE` (which neighbouring normal key holds the cell) and `VISUALIZE_NORMAL_KEY` (the pixel's own key) settled it: at the c20 hall 26% of pixels find the cell at their own key and the rest at a neighbouring key, with +1 in x or y and -1 in z in several combinations; the pixel's own key spreads over eight or more buckets across one flat slab. So the raster's bump-mapped G-buffer normal scatters over the keys while the slab's cells sit under its one smoothed vertex normal. At the shaft 70% find their own key.

The fix is to key the cells by the triangle's plane normal on both sides (`CELL_KEY_PLANE`: the hits' `triangle_world_normal`, turned to the hit's side, in every cache query; the pixel's from its depths with `CELL_LOOKUP_GEOMETRIC`), so a flat wall is one key whatever its bump map or smoothing does. Built; its gate is the leak view's magenta share at all three spots and the table against the guards. **Result (`plane` views): c20 hall no-cell 73% → 0.5%** (99.6% of pixels at their own key by the probe), its cell candidates with contribution 30% → 96%, its dropped picks 77% → 56% before the prior; **b40 shaft no-cell 27% → 9%**, dropped picks unchanged at 91% until the prior; **b40 canyon no-cell 18% → 2%**, its leaked cells 38% → 6% even without the unjittered position (the pixel now finds the cell keyed to its own facet), its cell candidates with contribution 40% → 80%, dropped picks 57% → 42%.

## Phase 1: lights by place, in the selection table (1 to 2 days, no GPU cost)

### 1a. Split emissive BSP meshes by cluster

In `spawn_bsps`, for a material whose surface has radiance, partition its triangles by the cluster of each triangle's centroid (`ClusterVisibility::cluster`, the collision BSP's leaf lookup) and spawn one traced mesh per cluster chunk. The raster mesh stays whole; only `RaytracingMesh3d` entities split, so the camera's picture, recolouring and the breakable-surface parents are untouched. Non-emissive materials are not split (no reason to add BLASes). Expect a10's 462 emissive meshes to become one to two thousand, well under the 65535 light limit; static BLASes build once, and the TLAS gains that many small instances.

After this, a cell slot names a fixture-sized light, the slot's irradiance weight means what it says, and a draw from the slot lands on a triangle the cell can see. `VISUALIZE_DI_PICK` should show the live-candidate count rise in the shaft.

### 1b. Weight the table by the camera's potentially visible set

Each frame in halo_rt: find the camera's cluster; mark every light whose cluster the camera's cluster sees (emissive chunks by their cluster from 1a; fixture and game lights by their position's cluster, or by range when they sit in no cluster). Pass the mark to Solarik as a component on the light's entity (for example `SolarikLightPlace { seen: bool }`), extracted with the rest.

In `prepare_raytracing_scene_bindings`, after the flux exponent: normalise the non-sun weights twice, once over all lights and once over the seen set, and take half of each. The seen set then holds half the non-sun share collectively, every unseen light keeps at least half its old share, and the sun's share is unchanged. Nothing in the shaders changes: `selection_probability` already carries each light's actual probability into the tiles and into both MIS denominators (`tile_probability` in `restir_di.wesl` and `world_cache_update.wesl`), so the estimators stay unbiased. The alias table is already rebuilt every frame, so this costs a few microseconds of CPU.

Switches: `HALO_RT_NO_CLUSTER_LIGHTS=1` (no split) and `HALO_RT_NO_PVS_LIGHTS=1` (no reweighting), for A/B in play.

Implemented 4 November (untested in the viewer until the Phase 0 runs release the binary):

- `src/clusters.rs`: `ClusterVisibility` moved out of the bridge (the viewer needs it without a game), `LevelClusters` (a resource, filled as the BSPs load), `LightPlace { bsp, cluster }` on each emissive chunk, and `mark_seen_lights`: every frame, the camera's cluster in the active BSP, then the chunks whose cluster it sees and the positioned lights (the game's, fixtures', objects' emitters) whose position's cluster it sees, into Solarik's `SolarikSeenLights`. A camera in no cluster sees none, and selection stays as it was.
- `src/level.rs` `spawn_bsps`: an emissive material's triangles grouped by the cluster of each one's centre (`emissive_chunks`); the camera's copy whole, one traced mesh a cluster for the rays, each with its `LightPlace`.
- `bevy_solarik` `SolarikSeenLights { entities, share }` (an extracted resource; the binder maps each light's render entity back to its main entity) and, in `prepare_raytracing_scene_bindings` after the flux exponent, the reweighting: the seen lights take `share` (0.5; `SOLARIK_SEEN_LIGHT_SHARE`) of the non-sun selection among themselves by power, every light keeps the rest by power. `SOLARIK_LIGHT_FLUX_LOG` now prints how many lights are seen.

Outdoors a canyon cluster sees many clusters, so the seen set is large and the reweighting is mild; the sun has its own sample and the sky is not a light, so nothing changes there. Cells in rooms the camera cannot see lose some tile share, but they keep their slots and are not on screen.

Gate: `SOLARIK_LIGHT_FLUX_LOG` at the shaft lists the shaft's strips in the top shares; the shaft, strips and c20 hall improve on all three metrics; the guards hold; GPU totals unchanged. Then the user A/Bs in play with the two switches. If 1b alone carries most of the gain, 1a can wait, but 1a is what makes the cell slots work, so do both.

Once this is in, revisit `EMITTER_LIGHT_GAINS`: keep the entries that were artistic (the ship's pink, the console strip), and try removing the ones added only so a weak emitter would be sampled (the monitors at 16, the holograms at 6).

### Phase 1 first result (4 November, `pvs` views)

At the shaft, with the cluster split and the visible-set weighting on: picks dropped 92% → 91%, tile candidates with contribution 52% → 61%. No change where it matters. The shaft is one open volume, so its cluster sees every strip in it; the bright strips on other floors are "seen" by the cluster and still win the resampling against the strip beside the pixel, and the shadow ray then finds the walkway in the way. Cluster visibility is too coarse for the pixel's question; the flux log's seen count (in the chain's log) will confirm the marking ran.

The table tells another story than the pick view (`pvs` table, level viewer, Phase 1 on, nothing else changed):

| spot | gpu base → Phase 1 | still 3-shot | 4-frame noise | motion wander | motion 3-shot |
|---|---|---|---|---|---|
| b40_shaft | 10.91 → 9.97 | 0.185 → 0.115 | 0.330 → 0.232 | 0.064 → 0.047 | 0.265 → 0.197 |
| c20_hall | 6.52 → 6.30 | 0.094 → 0.022 | 0.249 → 0.097 | 0.041 → 0.012 | 0.141 → 0.043 |
| b40_canyon | 8.76 → 8.69 | 0.012 → 0.012 | 0.065 → 0.068 | 0.005 → 0.005 | 0.021 → 0.021 |

| a10_tube (guard) | 6.84 → 6.87 | 0.007 → 0.007 | 0.011 → 0.011 | 0.005 → 0.004 | 0.022 → 0.017 |
| a10_corridor (guard) | 7.57 → 7.58 | 0.020 → 0.019 | 0.110 → 0.102 | 0.008 → 0.007 | 0.049 → 0.044 |
| b30_passage (guard) | 8.72 → 8.99 | 0.015 → 0.012 | 0.058 → 0.054 | 0.008 → 0.005 | 0.042 → 0.030 |
| b30_beach (guard) | 6.58 → 6.59 | 0.031 → 0.031 | 0.197 → 0.197 | 0.007 → 0.007 | 0.048 → 0.048 |

**Phase 1 passes its gate.** The hall is three to four times steadier and now measures like the a10 corridor guard; the shaft is a quarter to a third steadier; both are cheaper (fewer shadow rays wasted on lights that reach nothing); the canyon and the beach are untouched; every guard holds or improves. The one cost is b30's passage, 0.27 ms dearer in the diffuse GI pass, at the edge of the rule: to be re-measured with the later changes, since one run's GPU time varies by a tenth of a millisecond. At the shaft the flux log shows 52 of the BSP's 465 lights seen, holding a quarter of the selection. The dropped-pick share did not move because the pool's lights are now the right room's, which reuse then carries well, while the resampling's first choice is still the brightest unshadowed one.

What answers the pixel's question is already in the cache: a cell's light slots hold the lights that shadow-tested light actually reached it from. So, `DI_SLOT_VISIBILITY_PRIOR` (`restir_di.wesl`, and the cache's own search in `world_cache_update.wesl`): when the pixel's cell has slots, a tile candidate in none of them is weighed by a tenth of its unshadowed contribution (`SLOT_VISIBILITY_PRIOR`). The target function may be anything positive where the light contributes, so this changes variance alone: the pick's weight divides by this target and the shading multiplies by the real, traced contribution. It costs nothing. It needs the pixel to find its cell, which is what the plane-normal key is for; the two go together in the `prior` table.

### The plane key's table (`plane`: Phase 1 + `CELL_KEY_PLANE` + `CELL_LOOKUP_GEOMETRIC`)

| spot | gpu Phase 1 → plane | still 3-shot | motion wander | motion 3-shot |
|---|---|---|---|---|
| b40_shaft | 9.97 → 9.54 | 0.115 → 0.076 | 0.047 → 0.045 | 0.197 → 0.164 |

(The shaft's 4-frame series run died about 15 s after the level revealed, with nothing in its log, and from then on every viewer process compiled its pipelines cold for 40 s: the driver's shader cache, `%LOCALAPPDATA%\NVIDIA\DXCache`, holds 15.45 GB in 332 files and wrote nothing today, so it has stopped storing new pipelines, and captures before frame about 6000 were unlit or frozen. The rest of that chain was invalid and was stopped. `run.sh` now captures only past frame 9000, so a table takes about 45 minutes but no longer depends on the cache; the tables were relaunched, the full candidate first. The cache itself is the user's to clear or cap; the harness's numbers do not depend on it any more.)

### The full candidate (`prior` table: Phase 1 + `CELL_KEY_PLANE` + `CELL_LOOKUP_GEOMETRIC` + `CACHE_CELL_UNJITTERED` + `DI_SLOT_VISIBILITY_PRIOR`; captures past frame 9000)

The prior itself moved the dropped-pick share little (shaft 91% → 88%, hall 56% → 52%): the shaft's strips sit behind perforated glyph covers, so a random point on a strip is in sight of a pixel only through the slats, whatever picks it, and reuse carries the few that get through. The table is what counts:

| spot | gpu base → full | still 3-shot | 4-frame noise | motion wander | motion 3-shot |
|---|---|---|---|---|---|
| b40_shaft | 10.91 → 10.09 | 0.185 → 0.053 | 0.330 → 0.127 | 0.064 → 0.029 | 0.265 → 0.128 |
| c20_hall | 6.52 → 6.47 | 0.094 → 0.013 | 0.249 → 0.088 | 0.041 → 0.005 | 0.141 → 0.026 |
| b40_canyon | 8.76 → 8.67 | 0.012 → 0.012 | 0.065 → 0.063 | 0.005 → 0.004 | 0.021 → 0.019 |
| a10_tube (guard) | 6.84 → 6.92 | 0.007 → 0.006 | 0.011 → 0.013 | 0.005 → 0.002 | 0.022 → 0.009 |
| a10_corridor (guard) | 7.57 → 7.64 | 0.020 → 0.018 | 0.110 → 0.101 | 0.008 → 0.006 | 0.049 → 0.039 |
| b30_passage (guard) | 8.72 → 9.09 | 0.015 → 0.012 | 0.058 → 0.057 | 0.008 → 0.005 | 0.042 → 0.030 |
| b30_beach (guard) | 6.58 → 6.70 | 0.031 → 0.031 | 0.197 → 0.191 | 0.007 → 0.007 | 0.048 → 0.048 |

The shaft is half as unsteady in motion and a third as unsteady still; the hall is five times steadier and sits with the tube guard; every guard holds or improves. The one cost over the rule is b30's passage, +0.37 ms (diffuse GI and direct), present since Phase 1: b30 is one huge BSP, and the cluster split gave it many small emissive chunks, which the rays' traversal pays for. Phase 5's first payback is to merge chunks under a few dozen triangles into their neighbours. (The still measures of this table come from a longer-converged cache than the earlier tables', since the captures start later; the motion measures reset their history each sweep and compare directly.)

### What each part contributes (`plane` table: Phase 1 + plane key only, same late captures)

| spot | motion 3-shot: Phase 1 → + plane key → + unjittered + prior | still 3-shot |
|---|---|---|
| b40_shaft | 0.197 → 0.137 → 0.128 | 0.115 → 0.060 → 0.053 |
| c20_hall | 0.043 → 0.026 → 0.026 | 0.022 → 0.014 → 0.013 |

The plane key carries nearly all of the gain after Phase 1. The `plane_unjit` table (plane key + unjittered cells, no prior) reads the same as the full candidate at the shaft (0.126 in motion, 0.053 still), so the last 7% there is the unjittered cell position's and the slot prior adds nothing anywhere: it stays opt-in (`SOLARIK_SLOT_PRIOR=1`), an estimator change with no measured gain. The unjittered cell position removes the canyon's leaked cells (38% → 5%), costs nothing and changes no estimator, so it stays on.

**Defaults (node.rs, 4 November):** `CELL_KEY_PLANE` + `CELL_LOOKUP_GEOMETRIC` on (`SOLARIK_CELL_KEY_PLANE=0` for upstream's keys), `CACHE_CELL_UNJITTERED` on (`SOLARIK_CACHE_CELL_UNJITTERED=0`), the prior off (`SOLARIK_SLOT_PRIOR=1`), Phase 1 on (`HALO_RT_NO_CLUSTER_LIGHTS=1`, `HALO_RT_NO_PVS_LIGHTS=1`). The `default` table, with no definitions passed, confirmed the build: shaft 0.128 in motion and 0.053 still at 10.06 ms, hall 0.026 and 0.014 at 6.47 ms, canyon 0.019, tube 0.010, corridor 0.040, passage 0.030 at 8.98 ms, beach 0.048 at 6.66 ms.

## Phase 2: the world cache's birth (1 day, near zero GPU cost)

### 2a. Store the unjittered point as a new cell's position

One change in `query_world_cache_with_support`: keep the jitter for the key, store `world_position_in` in `world_cache_geometry_data[key].world_position`. The cell's updates then start on a real surface. Check with `VISUALIZE_CELL_LEAK` and at the two seam reports (b40 image 37 and 96) and the rocks (38 to 53). If the rocks persist, look at the sun's `offset_surface_ray` on their normal-mapped faces next, not at the cache.

### 2b. Warm-start new cells from the parent level of detail

When a cell is created, probe the cell one level of detail coarser at the same point and normal (quantise at twice the cell size; same key and checksum functions). If it exists with 4 or more samples, copy its radiance with a sample count of 4, so the first real update blends at 1/5 instead of replacing it. Keep the cell in the scheduler's first-light quota by marking it (the priority bucket currently reads `radiance.a == 0` as unlit; use `a < 4` or a flag bit). One extra hash probe per cell creation, nothing per frame.

### 2c. If motion still shows young cells: more samples on first light

The budget pass already counts unlit cells. For a cell on its first update, take 3 extra GI rays and twice the shadow-tested light samples. Cost is bounded by the unlit count (tens of thousands of rays in a Banshee, about 0.2 to 0.4 ms there, nothing when standing). Do this only if `VISUALIZE_CACHE_AGE` still shows red patches after 2b.

Switches: `SOLARIK_CACHE_WARM_START=0`, `SOLARIK_CACHE_FIRST_LIGHT=0`.

Gate: canyon wall in motion (viewer `--dolly` with a long reach along the canyon, and the user in a Banshee), c20 hall walking (`HALO_TEST_INPUT=walk:<seed>`): blotch wander in motion down, cache-age view mostly green; seam and rock reports gone or changed; b40 GPU total unchanged within 0.3 ms.

### The lookup's residual

With the plane key the grey-coded probe (`default` views) reads: shaft 87% of pixels at their own key, 5% one step off in x, 2% in y, the rest scattered, 0.1% in no neighbouring key at all. The misses are facets whose plane normal sits at a quantisation step, where the depth-derived normal's noise flips a component. A cheap follow-up: when the first lookup fails, probe the six single-component neighbours (six hash reads, on the 13% of pixels that need it).

## Phase 3: candidates and reuse, re-tuned on the better pools (1 to 2 days, bounded cost)

Only after Phases 1 and 2, because every number here was tuned against bad pools.

- Re-split DI candidates: try 24 tile and 16 cell, and 16 tile and 16 cell. If the live-candidate count stays high at 16 tile candidates, that is 0.3 ms back.
- Raise the DI confidence cap from 8 toward 12 or 16. It was cut because bad picks persisted; with good picks, more history is less boiling. Measure in motion, not still.
- Adaptive second spatial neighbour in `spatial_and_shade`: merge a second neighbour only when the pixel's reservoir confidence is under 4 or its cell is young (from 2b's count). Costs where it helps, nothing elsewhere; expect under 0.3 ms worst case.
- The cache's own search: with a place-weighted table, try 64 tile candidates per cell instead of 128 (about 0.3 ms back on the cache pass).

Gate per change: the full table, with the guards; keep only what moves a boiling spot without moving a guard.

## Phase 4: outdoors (1 to 2 days)

- The 46 frames/s dips are in fights, not in the lighting of the empty canyon (9.3 ms GPU, render thread 4.7 ms). Capture a fight with F9 (the `.txt` now carries CPU and GPU times by pass) and split the cost: BLAS refits, the 192 dynamic lights, particles, or the CPU. Fix what the capture names; do not pre-empt it with lighting cuts.
- Shade lit by the sky alone: after 2b, if sky-lit shade still wanders, give cells whose direct sample is zero a second GI ray per update (a cheap test inside `sample_gi`, cost proportional to such cells).
- Distant silhouettes and alpha-tested foliage (the 20 October leftovers) stay out of this plan; they are a different estimator (`primary_foliage`).

## Phase 5: paying for it

Everything above is either free (1, 2a, 2b) or bounded (2c, 3, 4). If the interior total rises past the 0.3 ms rule, these pay it back in order: the cache search at 64 candidates (Phase 3), 16 DI tile candidates (Phase 3), and the world cache update soft cap (40000 cells a frame; check `HALO_RT_CACHE_LOG` for how often it binds).

## Working rules (from boiling.md's lessons)

- Prefer changes that keep the estimator the same: 1b and 2a are reweighting and placement, the estimators are unchanged. Anything that routes more light through the cache is a stability risk the metrics underrate.
- Every change behind an env switch, and the user A/Bs in play after each phase. Three-shot spreads in motion matched the user's perception before; still-camera noise did not.
- Confirm with a view before tuning: the three Phase 0 views exist to be looked at before and after each phase.
- Captures right after a shader change may still be compiling; use the warm-up rule in the memory notes.

## Play test of 5 November (the default build, nothing to fix yet)

The user played b40 with everything above on by default: overall image stability is improved. A few places still boiled, and for the most part settled with time (consistent with young cache cells and fresh reservoirs rather than a wrong selection table). Noted, no fix applied yet; the F9 captures are in `dist/captures` with the camera in their `.txt`:

| Capture (b40) | Camera (`--camera`) | BSP, cluster | What it shows | GPU |
|---|---|---|---|---|
| `b40_1793880053940` to `055211` (7 shots, tick 2394 to 2432) | 46.06,27.58,48.62,178.8,-12.1 | bsp 3 b40_a1, cluster 7 | the hangar floor by a parked Ghost, lit by wall lamps and a lamp post | 11.6 ms |
| `b40_1793880122931` to `123471` (4 shots, tick 4463 to 4479) | 42.35,-2.69,48.72,-142.0,4.0 | bsp 3 b40_a1, cluster 1 | a dark structural pillar, purple floor glow behind | 9.5 ms |
| `b40_1793880320794` to `321913` (4 shots, tick 10395 to 10428, still camera) | 42.42,-106.67,0.62,20.5,-12.6 | bsp 4 b40_a2, cluster 2 | a slanted light strip (the b40 strip spot) | 9.1 ms |
| `b40_1793880817825` (tick 21729) | 241.17,-585.41,45.21,-51.1,24.3 | bsp 8 b40_b3, cluster 11 | looking up a shaft at its blue strip columns | 8.5 ms |

First three settle; the shaft view is the one spot still well above the others in the harness (3-shot 0.053 still, 0.128 in motion). Phase 2b/2c (cache warm start, more samples on first light) and the lookup residual are the candidates when a fix is wanted.

## What is needed from the user

- F9 capture `.txt` lines for the b40 shaft, a b40 light strip, the canyon wall spot (and roughly how fast the Banshee was moving), and the c20 hall. `dist/captures` has 206 b40 files from 3 November but no map from the findings' image numbers to them.
- Confirm the budget: 60 frames/s at 1280x720 DLAA, with the lighting passes under about 10 ms. If the target is a higher output resolution or the Performance mode, say so, because the interior reference numbers change.

## Order and time

| Phase | Days | GPU cost | Targets |
|---|---|---|---|
| 0 harness and views | 0.5 | 0 | measurement, hypotheses confirmed |
| 1 lights by place | 1 to 2 | 0 | shafts, strips, c20 halls, c10 floor lights |
| 2 cache birth | 1 | 0 to 0.4 ms in motion | canyon wall from a Banshee, seams, rocks, dim halls while walking |
| 3 re-tune | 1 to 2 | net zero or better | what remains, and the budget |
| 4 outdoors | 1 to 2 | 0 | fight dips, sky-lit shade |

About a week and a half, with a play A/B after Phases 1, 2 and 3.
