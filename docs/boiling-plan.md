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

### Phase 2b result (6 November): built, on by default, not measurable in the viewer

`CACHE_WARM_START` (`SOLARIK_CACHE_WARM_START=0` to switch off; an Opus agent against a spec, reviewed): at a cell's birth the cell one level of detail coarser at the same jittered point and normal is probed read-only; with 4 or more samples its radiance is copied in as 3.999 samples (a mark no ordinary count reaches: the first real update makes it 4.999 and blends at 1/5; the priority pass keeps the marked cell in the first-light quota), its support carried over, and a parent stored more than six child cells away is refused (the quantised coordinates hash alike at every level). The off path compiles to the same WGSL; `world_cache_priority` is now validated by the shader tests, which it was not before.

The full `warm` table equals `default` in every column (canyon in motion 0.019, the fast canyon 0.023, the hall 0.027 against 0.026; no spot more than 0.10 ms dearer), and `warm_off` reproduces `default` at the three A/B spots (the shaft reads 9.8 ms and noise 0.139 in both, against the `default` run's 10.06 and 0.126: drift of that spot since 4 November, the switch is not it). The cache-age views say why nothing moved: the warm start fires, young cells on the canyon wall in motion halve (red 4.8% to 2.3%, newborn 1.3% to 0.7%), but young cells were only 5% of that wall in the harness's sway (20 m each way every two seconds keeps the cache warm) and 0% in the hall's. A longer sway was tried (`HALO_RT_DOLLY_PERIOD`, `HALO_RT_DOLLY_LEVEL`, a sixth column in `spots.txt`, kept): the viewer and the scripts take it, but from the canyon spot, which looks at the wall, a 100 m level flight along the view ends inside the rock, so no flight spot is defined; a Banshee path in the open is needed for one.

So 2b costs nothing, passes every guard, and the viewer cannot show its benefit; it stays on, and the user's Banshee A/B (`SOLARIK_CACHE_WARM_START=0` against the default) is its test. 2c has the same problem: with 5% young cells in the sway there is nothing for it to move in the harness, so it waits for the same path. The shaft is the one spot still above the others, and the NRC work (`docs/nrc-integration.md`, "Tuning and diagnosis") showed its remaining wander is in the radiance field the ReSTIR GI samples, which the cache's cell blur smooths: that is Phase 3's reuse items (the confidence cap, the adaptive second spatial neighbour), which come next, with the lookup residual.

## Phase 3: candidates and reuse, re-tuned on the better pools (1 to 2 days, bounded cost)

Only after Phases 1 and 2, because every number here was tuned against bad pools.

- Re-split DI candidates: try 24 tile and 16 cell, and 16 tile and 16 cell. If the live-candidate count stays high at 16 tile candidates, that is 0.3 ms back.
- Raise the DI confidence cap from 8 toward 12 or 16. It was cut because bad picks persisted; with good picks, more history is less boiling. Measure in motion, not still.
- Adaptive second spatial neighbour in `spatial_and_shade`: merge a second neighbour only when the pixel's reservoir confidence is under 4 or its cell is young (from 2b's count). Costs where it helps, nothing elsewhere; expect under 0.3 ms worst case.
- The cache's own search: with a place-weighted table, try 64 tile candidates per cell instead of 128 (about 0.3 ms back on the cache pass).

Gate per change: the full table, with the guards; keep only what moves a boiling spot without moving a guard.

### Phase 3 result (6 November): the DI candidates re-split, the caps left alone

The five constants became shader values read from the environment (`bevy_solarik/src/realtime/tuning.wesl`; `node.rs` reads `SOLARIK_DI_TILE_CANDIDATES`, `SOLARIK_DI_CELL_CANDIDATES`, `SOLARIK_DI_CONFIDENCE_CAP`, `SOLARIK_GI_CONFIDENCE_CAP`, `SOLARIK_CACHE_TILE_CANDIDATES`, and `restir_di`, `restir_gi` and `world_cache_query` take them from there), so the whole sweep ran on one build (`scripts/boil/out/phase3.sh`, `phase3b.sh`). The baseline is `warm` (the fresh table; the shaft had drifted since `default`, 9.8 ms and noise 0.138 against 10.06 and 0.126). The shaft and the three guards:

| variant | shaft still P90 / 3s | shaft noise | shaft motion 3s | shaft direct ms | hall / corridor / fast canyon motion 3s | direct ms at the guards |
|---|---|---|---|---|---|---|
| `warm` (32 tile, 8 cell, caps 8, cache 128) | 0.039 / 0.054 | 0.138 | 0.126 | 2.71 | 0.027 / 0.040 / 0.023 | 1.85 / 1.89 / 2.33 |
| 24 tile, 16 cell | 0.034 / 0.043 | 0.122 | 0.102 | 3.01 | 0.027 / 0.039 / 0.023 | +0.11 each |
| 16 tile, 16 cell | 0.033 / 0.045 | 0.115 | 0.106 | 2.80 | 0.025 / 0.039 / 0.023 | -0.09 each |
| 16 tile, 32 cell | 0.029 / 0.039 | 0.113 | 0.092 | 3.80 | 0.024 / 0.039 / 0.023 | +0.55 each |
| DI confidence cap 16 | 0.038 / 0.053 | 0.121 | 0.117 | 2.69 | 0.032 / 0.040 / 0.023 | same |
| GI confidence cap 16 | 0.039 / 0.054 | 0.139 | 0.126 | 2.69 | 0.028 / 0.040 / 0.024 | same |
| cache 64 tile candidates | 0.039 / 0.054 | 0.141 | 0.125 | 2.68 | 0.027 / 0.040 / 0.023 | cache pass -0.34 each |
| 16 tile, 16 cell, cache 64 | 0.035 / 0.045 | 0.115 | 0.106 | 2.74 | 0.025 / 0.039 / 0.023 | cache pass -0.34 each |

What it says:

- The cell candidates carry the shaft. 24+16 and 16+16 land within 0.004 of each other in every column, so doubling the draws from the cell's four light slots is the gain and the tile count only pays for it: a cell's slots already name the lights that matter, and each extra draw is another point on those strips, a candidate for the one shadow ray, where the tile's draws are mostly lights that do not reach the pixel. 32 cell candidates steady the shaft further (0.092 in motion) but the resolve of each candidate costs: 1.0 ms at the shaft, 0.55 at the guards, past the 0.3 ms rule. 16 and 16 are the new defaults (`node.rs`); the full `p3_new` table with them equals `warm` at the other seven spots (within 0.002 everywhere, the direct pass 0.1 ms cheaper, the shaft's total 9.83 against 9.80).
- The confidence caps are not the shaft's problem. GI cap 16 changes nothing there; DI cap 16 takes 0.009 off the shaft's motion but adds 0.005 to the hall's (the longer history lags the sway), and nothing in the stills. The plan's expectation, that more history is less boiling once the picks are good, was wrong for these spots: the wander is in the sample points, not the history length. Both stay at 8 (the knobs remain).
- Cache 64 tile candidates saves 0.34 ms at every spot and moves no boiling column, alone or with the new DI split (the hall's stills in the lone run were run noise; the combined run reads 0.004 / 0.013 as `warm`). It is Phase 5's first payback, ready as `SOLARIK_CACHE_TILE_CANDIDATES=64`, and the default stays at 128 until the interior total needs it: the working rules say the metrics underrate what the cache's search does for stability, and nothing has to be paid for yet.

The two reuse items, each alone against `p3_new` (the 16 and 16 defaults) at the same four spots:

| | shaft still P90 / 3s | shaft noise | shaft motion 3s | shaft direct ms | hall stills P90 / 3s | hall / corridor / fast canyon motion 3s |
|---|---|---|---|---|---|---|
| `p3_new` | 0.033 / 0.044 | 0.113 | 0.107 | 2.75 | 0.004 / 0.013 | 0.025 / 0.039 / 0.023 |
| adaptive second spatial neighbour (`GI_ADAPTIVE_SPATIAL`) | 0.034 / 0.045 | 0.114 | 0.106 | 2.79 | 0.009 / 0.017 | 0.025 / 0.039 / 0.023 |
| the lookup's residual (`CELL_LOOKUP_NEIGHBOURS`) | 0.018 / 0.028 | 0.101 | 0.079 | 2.83 | 0.005 / 0.013 | 0.025 / 0.039 / 0.023 |

- The adaptive neighbour (a pixel whose input reservoir's confidence is under `SOLARIK_GI_ADAPTIVE_CONFIDENCE`, 4, merges a second spatial neighbour in `spatial_and_shade`) does nothing at the shaft and roughens the hall's stills: the GI reservoir's age limit (16 frames) empties a pixel's history on schedule, so a short history is a still's ordinary state for three frames in sixteen, and the second neighbour there is a different sample where the picture was settling. Off (`SOLARIK_GI_ADAPTIVE_SPATIAL=1` to try it); the "young cell" half of the plan's condition was not built, since the confidence half already showed the idea does not reach the shaft's wander.
- The lookup's residual is the largest single gain of the plan so far. When the four normal tries of `find_world_cache_cell` fail, the six keys one quantised normal component away are probed (`world_cache_query.wesl`; the probes run on the missing pixels only, 0.08 ms in the shaft's direct pass, 0.02 at the guards). The shaft's stills drop from 0.044 to 0.028, motion from 0.107 to 0.079, noise 0.113 to 0.101, and the guards do not move. On by default (`SOLARIK_CELL_LOOKUP_NEIGHBOURS=0` off). The 13% of the shaft's pixels that found no cell were drawing their light from the tile alone, 16 candidates from the whole level's table, and those were the picks that wandered: the cell's four slots, reached by the neighbouring key, are what steadied them, the same lever as the cell candidates above.

The no-cell view confirms where it acts (`VISUALIZE_CELL_LIGHTS` at the shaft, `p3_final` and `p3_lookup_off` views): pixels without a cell fall from 11.2% to 2.9%, and the DI pick view's pixels with no live cell candidate from 1.6% to 0.9%. The full table with Phase 3's defaults (`p3_final`: DI 16 tile and 16 cell candidates, the lookup's six probes, caps 8, cache 128; `scripts/boil/out/p3_final/table.txt`, the table to compare against from here):

```
spot             gpu direct  cache diffGI specGI   dlss | stillWdr stillP90 still3s |   noise noiseP95 | moveWdr  move3s
b40_shaft       9.92   2.84   1.27   1.45   1.37   1.31 |   0.009   0.019   0.029 |   0.099    0.32 |   0.018   0.080
b40_canyon      8.64   2.26   1.29   1.53   0.71   1.31 |   0.003   0.004   0.012 |   0.061    0.24 |   0.004   0.019
c20_hall        6.38   1.77   1.15   0.80   0.48   1.31 |   0.003   0.004   0.013 |   0.085    0.30 |   0.005   0.026
a10_tube        6.85   1.76   1.07   0.86   0.91   1.31 |   0.002   0.003   0.007 |   0.012    0.08 |   0.002   0.010
a10_corridor    7.54   1.83   1.13   1.01   1.27   1.32 |   0.003   0.007   0.018 |   0.099    0.28 |   0.006   0.039
b30_passage     8.99   1.97   1.23   1.44   1.74   1.31 |   0.003   0.005   0.012 |   0.054    0.22 |   0.005   0.030
b30_beach       6.69   1.43   1.17   0.96   0.43   1.31 |   0.004   0.011   0.031 |   0.192    0.51 |   0.007   0.048
b40_canyon_fast   8.65   2.27   1.30   1.53   0.71   1.31 |   0.003   0.004   0.012 |   0.060    0.24 |   0.005   0.023
```

Against `warm` (the morning's table): the shaft's stills 0.054 to 0.029, its motion 0.126 to 0.080, its noise 0.138 to 0.099, for 0.12 ms on its total (9.80 to 9.92, the direct pass 2.71 to 2.84); every other spot within 0.004 in every boiling column and within 0.05 ms, the hall 0.08 ms cheaper. The shaft is now in the range of the other interiors in stills and within a factor of two in motion, where it was four. Phase 3 is done; what the shaft still has (0.080 in motion against the corridor's 0.039) is the next candidate pool question: 32 cell candidates showed the cell's sample points are still the lever, at a resolve cost, so a cheaper way to more samples on the cell's lights (resolve the four slots' lights once and draw points on them, rather than resolve each draw) is the item to carry into Phase 5's accounting.

## Phase 4: outdoors (1 to 2 days)

- The 46 frames/s dips are in fights, not in the lighting of the empty canyon (9.3 ms GPU, render thread 4.7 ms). Capture a fight with F9 (the `.txt` now carries CPU and GPU times by pass) and split the cost: BLAS refits, the 192 dynamic lights, particles, or the CPU. Fix what the capture names; do not pre-empt it with lighting cuts.
- Shade lit by the sky alone: after 2b, if sky-lit shade still wanders, give cells whose direct sample is zero a second GI ray per update (a cheap test inside `sample_gi`, cost proportional to such cells).
- Distant silhouettes and alpha-tested foliage (the 20 October leftovers) stay out of this plan; they are a different estimator (`primary_foliage`).

### Phase 4 (6 November): what the captures name, and the shade's second ray

The cost split first. Of the 1110 F9 captures in `dist/captures`, 29 carry times (the ones since the capture gained its `cpu:` and `gpu` lines), b40 and c20 of 3 November, none of a fight below 60 frames/s (median 88, the lowest 67). The slow ones among them are of two kinds, and neither is the lighting of the empty canyon:

| frames | frames/s | CPU main / render ms | GPU ms | what stands out |
|---|---|---|---|---|
| c20 at tick 18911 (three captures) | 67 | 2.0 / 14.5 | 8.8 | the render thread is the frame: 14.5 ms against a GPU of 8.8, every pass at its median |
| b40 at tick 2394 (seven captures, camera 46, 28, 49 looking back) | 76 | 1.2 / 5.5 | 11.7 | the GPU is the frame: specular GI 2.8 (its median 1.35), diffuse GI 1.7 (1.2), direct 2.8 (2.4) |
| the rest | 78 to 116 | about 2 / 6 | 7 to 9.1 | |

So the capture line could not yet say what the render thread does in its 14.5 ms (the plan's suspects: BLAS refits, the dynamic lights, particles), only that it is the thread, not the GPU. The capture now splits it (`src/cpu_times.rs`): `render 11.44 ms (views 1.2, prepare 6.0, graph 4.1; and 0.04 ms waiting for the window)`, where views is the assets, meshes, views and pipelines (ExtractCommands to PrepareViews), prepare is the queue and the prepare sets (the scene's acceleration structures and uploads live there), and graph is the render graph's encoding and submit. The next fight capture names the stretch; the plan's rule stands: fix what it names, not a lighting cut pre-empting it. The b40 GPU dip is a view question (specular GI doubled where the view had more rough metal in it, the Pelican's hull at the drop, probably): a capture at that camera in the game, with `HALO_RT_GPU_LOG=1`, would say whether it is the pass's ray count or its hits' shading.

The shade's second ray (`CACHE_SHADE_SECOND_RAY`, `world_cache_update.wesl`): a cell whose direct sample found nothing this update (no sun, no emitter in reach) traces two cosine rays instead of one and takes their mean, so the sky's irradiance in a canyon's shade is estimated from two directions an update rather than one; the cost is a ray on the cells in shade only. The full table against `p3_final` (`p4_shade`): every boiling column within 0.003 at every spot, the cache pass 0.03 to 0.08 ms dearer (the passage the most, 1.23 to 1.31; the shaft's total 9.92 to 9.94). The harness cannot see what it is for, for the reason 2b's result gave (the sway keeps the cells warm and the viewer has no Banshee flight through shade), so it stays on behind its switch, `SOLARIK_CACHE_SHADE_SECOND_RAY=0`, as the user's A/B on the canyon's shaded walls, with 2b's. The two remaining Phase 4 items wait on the user: a fight capture with the new render-thread split, and the specular GI's doubling at b40's drop.

### Phase 4, the fight (6 November, later): the render thread split, and three fixes in it

The user's b30 beach fight (nine F9 captures, ticks 1226 to 2180, 73 to 79 frames/s) was render-thread bound: the thread 10.1 to 10.8 ms against a GPU of 8.2 to 9.8, every GPU pass at its median, and the capture's split named views 3.8 to 4.0 and prepare 3.8 to 4.3 (a quiet a10 1 and 2.5). So the lighting is not what a fight costs; the scene's CPU side is. The split was taken one level finer (`src/cpu_times.rs`: extract, assets with the BLAS inside, meshes, specialize, views, queue, resources with the scene bindings inside, batches, bind groups, graph), and the renderer's own CPU scopes, which nothing consumed, are now summed and logged with it (`BEVY_RENDER_PROFILE=1`, the `cpu scopes:` line, with the counters it keeps). A scripted b30 run (the game's own camera at the landing, the beach fight going on in front of it, 100 s with `HALO_RT_GPU_LOG=1`) reproduces the captures' thread at 9.8 to 10.4 ms, so the rest needed no captures.

What the frame held, 170 to 210 skinned parts a frame (every marine's and Covenant's bending parts, posed again each frame the nodes move):

| stretch | ms | what |
|---|---|---|
| assets | 3.1 to 3.5 | the mesh allocator freeing and allocating every posed part again (0.2) and copying its data in with one `write_buffer_with` an element, 635 a frame, each of which allocates and maps a staging buffer in wgpu (1.8 ms) besides the packing (0.5); the BLAS system 0.9 (the triangle edges of every part walked again for the light sampling's area, 0.4; its own submit 0.4) |
| resources | 3.0 to 3.2 | the scene bindings 2.9: the dependencies observed over all 1180 instances (0.9, every frame, since a refit bumps the BLAS generation and a transform the instance inputs), the rows uploaded (1.0, 850 to 1010 geometry rows dirty a frame), the instances 0.35, the TLAS build and its submit |
| graph | 2.3 to 2.6 | the render graph's encoding and submit |
| main thread | 3.2 to 3.7 | the posing itself and the bridge |

Done, each measured in the scripted run at a like number of parts:

- The mesh allocator stages the frame's copies through a `StagingBelt` into one encoder and one submit (`copy_element_data_staged`, vendored `bevy_render`): the copies 2.55 to 1.0 ms (the packing 0.47, the submit 0.26).
- A modified mesh whose data has the size and layout of its allocation keeps it and is written over in place (`can_reuse_allocation`, `mesh_reusable`; the allocator's own TODO): 168 of 172 parts a frame reuse, the free and the staging 0.2 to 0.02, and their rows' buffer offsets no longer change (geometry rows dirty 1010 to 850).
- A deformable mesh keeps its first pose's triangle edges (`prepare_raytracing_blas`): 0.42 to 0.10.
- The bridge writes an object node's transform only when it moved (`src/bridge.rs`): fewer changed instances for the binder, though most of the fight's 280 a frame do move.
- Tried and reverted: the four prepare-time submits (mesh copies, BLAS builds, row uploads, TLAS build) collected into one before the graph. The bindings stretch fell 0.8 ms and the graph stretch rose 0.9: a submit's cost is what it carries (wgpu's tracking of the builds' resources), not the call.

The render thread in the scripted fight: 9.8 to 10.4 ms before, 8.4 to 9.1 after (at 185 to 215 parts). Left in it, by size: the scene bindings (dependencies 0.86 over all instances each frame, the row upload 0.86, its TLAS submit), the graph 2.1 to 2.4, the main thread's posing 3.4, the BLAS submit 0.36 and the mesh copies' 0.26. The fight is no longer the frame at this spot (GPU 8.7 to 9.8), but a c20-sized fight would still be; the bindings' all-instance observe is the next thing to make incremental.

### Phase 4, the b40 corridor's specular GI (6 November, later)

The 3 November b40 capture with specular GI at 2.8 ms (tick 2394, camera 46.06, 27.58, 48.62, 178.8, -12.1) is not the Pelican drop but the corridor after it, a Ghost and a plasma sword on a wide smooth metal floor. A scripted b40 run with the camera detached there (`game.sh`, `--camera` with `--opence`) reproduces it from the first frames and holds it: specular 2.95 to 3.07 ms, direct 3.0, the GPU 11.6 to 12.2 against the harness's b40 spots' 8.7 to 9.9. It is the view, not a transient: the floor fills half the picture, its roughness is under the reuse threshold of 0.4, so every one of its pixels traces a glossy path (a bounce to the far wall, a light sample and its shadow ray, the cache at the rough hit), where at the harness spots most pixels are rough and reuse the diffuse GI's reservoir instead.

The threshold is now a knob (`SOLARIK_SPECULAR_REUSE_ROUGHNESS`, hundredths, 40 until now; `tuning.wesl` as the Phase 3 ones), and at that camera:

| reuse roughness | specular GI ms | GPU ms | picture (floor region mean) |
|---|---|---|---|
| 0.4 (the default until now) | 3.05 | 12.0 | 55.3 |
| 0.3 | 2.25 | 11.2 | 56.5 |
| 0.2 | 1.74 | 10.6 | 56.6 |

So the floor's roughness lies between 0.2 and 0.4, and its traced reflection is not visible in the picture at any of the three (the stacked crops in the scratchpad: the same floor, the reused GI 2% brighter). The harness at the two spots with the most mid-rough metal (`spec30`, `spec20` against `p3_final`):

| | specular GI ms, shaft / passage | GPU ms | boiling columns | still mean, shaft / passage | pixels differing from 0.4 by more than 16/255 |
|---|---|---|---|---|---|
| 0.4 | 1.37 / 1.74 | 9.92 / 8.99 | | 33.87 / 17.51 | |
| 0.3 | 1.01 / 0.73 | 9.54 / 8.08 | all within 0.002 | 33.95 / 17.50 | |
| 0.2 | 0.92 / 0.51 | 9.43 / 7.82 | all within 0.002 | 33.93 / 17.51 | 0.1% / 0.03% |

The glossy paths traced on surfaces of roughness 0.2 to 0.4 give the picture nothing the diffuse GI's reservoir does not at these spots (the difference maps show the shaft's far strip edge and nothing in the passage), for 0.4 to 1.3 ms a frame. The default is now 0.3, the conservative half (`SOLARIK_SPECULAR_REUSE_ROUGHNESS=40` is the old look, `=20` the rest of the saving), the user's A/B on Covenant and vehicle metal to settle it; the gate here is the look, not the boiling, which did not move.

### Phase 4 in play (6 November, evening): the A/Bs, and the library

The user's two sessions, b40 and c20 (the defaults, then `SOLARIK_SPECULAR_REUSE_ROUGHNESS=20 SOLARIK_CACHE_WARM_START=0 SOLARIK_CACHE_SHADE_SECOND_RAY=0`):

- Specular reuse: at 20 the frozen lake "looks more dull and marble-like"; 30 stays the default. The lake on foot is where the knob shows (`b40_1794005139544`, camera 190.98,-540.08,12.97: specular GI 4.0 ms of 12 at 30), not the corridor.
- Warm start and the shade's second ray: on a Banshee canyon flight the bounce light on the far wall "looked better on the first run" (the defaults, both on), "but that might be entirely placebo". Both stay on.
- Found on the way: a Warthog on the frozen lake lit the ice with a flashing white light. The game's ice chips (`effects\particles\solid\ice`, its one alpha-multiply-add particle: glass shards added for their glint) counted as glowing sprites in `src/sprite_lights.rs`; their bitmap's share is now 0.

The captures with times (b40 fights at 1794005053..743, c20's library fights at 1794005862..6124) are render-thread bound as the b30 beach was, and further:

| | render thread ms | GPU ms | frames/s | assets [blas] | resources [bindings] | graph |
|---|---|---|---|---|---|---|
| b40, fights | 14.2 to 17.5 | 7.7 to 9.5 | 55 to 70 | 3.7 to 7.0 [1.8 to 4.4] | 2.4 to 3.8 [2.2 to 3.6] | 3.6 to 4.8 |
| c20, the library | 16.6 to 20.1 | 8.0 to 9.2 | 48 to 58 | 4.8 to 7.2 [4.7 to 7.2] | 3.3 to 5.2 [3.1 to 4.9] | 5.2 to 6.2 |

So the library's frame looked like the BLAS stretch first (4.7 to 7.2 ms, against the beach's 0.9 after its edges fix), then the bindings, then the graph.

**The BLAS stretch was the materials.** A scripted c20 run (`game.sh c20`, the game's own camera, 110 s, `BEVY_RENDER_PROFILE=1`) has the thread at 8.9 to 9.5 ms with the stretch at 3.4, but the BLAS systems inside it at 0.3: the stretch's span ran from before the mesh allocator to after `compact_raytracing_blas`, which takes `&mut World`, so the executor drains every system in flight before it, and the span took their time. The capture line now spans the BLAS system alone and the compaction wait as `compact` (0.3 and 2.7 here), and the renderer's erased assets (the materials) have CPU scopes: `MeshMaterial3d<HaloTransparentMaterial>` 3.2 ms a frame, 522 materials prepared a frame. They are the transparent surfaces' animated materials (`animate_transparent`: every scrolling map and cycling colour writes its material's params each frame; the library's walls are full of them, and each object's copy is its own), and each prepare made a uniform buffer and a bind group (`bind groups` stretch 1.5 ms, 466 bind groups a frame), because:

- Bevy's `RenderMaterialBindings::prepare_material` has a fast path for a changed material that binds the same resources (`try_update_data`), but only for bindless materials; for non-bindless ones it was a TODO. It is now implemented in the vendored `bevy_render` (`MaterialBindGroupNonBindlessAllocator::try_update_data`, `write_buffers`): the same buffers, textures and samplers and data of the same sizes keep the bind group, the new bytes are written over its uniform buffers in `PrepareBindGroups`.
- The `#[uniform]` derive makes a new GPU buffer in `build_bind_group` (a `Buffer` binding, different every time), so no two prepares could match. The `#[data]` attribute keeps the bytes on the CPU (the same uniform buffer in the shader); `HaloTransparentMaterial` and `HaloEnvironment` now use it (`src/material.rs`), their params converted by `AsBindGroupShaderType`.

| scripted c20 | render thread ms | assets [blas] [compact] | bind groups | the transparent materials' prepare | updated in place / reallocated a frame |
|---|---|---|---|---|---|
| before | 8.9 to 9.5 | 3.4 [0.3] [2.7] | 1.5 | 3.25 | 57 / 465 |
| after | 5.5 to 5.6 | 0.96 [0.28] [0.18] | 0.75 | 0.76 | 522 / 0.3 |

The stills of the two runs show the same room with its animated strips (`scratchpad/game/lib_scopes`, `lib_data`). Left in the library's thread: the scene bindings 1.8, the graph 1.3, the bind groups 0.75 and the materials' 0.76 (522 `build_bind_group`s at 0.2, the rest the erased-asset loop). The b40 fights' 14 to 17.5 ms thread had the same shape (assets 3.7 to 7.0) and should fall by as much: for the user's next session.

**The library played again (6 November, night).** The user's captures, the same ground as the earlier session's:

| library, F9 captures | render thread ms | GPU ms | fps |
|---|---|---|---|
| earlier session, the opening fights | 16.6 to 20.1 | | 48 to 58 |
| this build, the opening fights | 6.8 | 8.2 to 8.4 | 90 to 92 |
| this build, the second floor's fights | 9.6 to 10.8 | 8.6 to 8.9 | 75 to 91 |
| this build, the big late fights (third and fourth floors) | 12.7 to 15.8 | 9.0 to 9.7 | 61 to 76 |

The dips the user felt with many enemies together are the CPU: the GPU holds at 9 to 9.7 ms while the thread climbs to 15.8 and the window wait falls to nothing. The scene is twice the opening's by then (the heartbeat: 9.4k entities and 3.5k meshes at the start, 18 to 19.8k entities and 6.0 to 6.4k meshes on the upper floors; the bridge draws 717 of 849 objects: the bodies stay). Three brackets grow with it: the graph 1.6 in the quiet opening, 4.0 to 7.4 in the big fights; the compaction wait, 0.1 to 0.4 on the lower floors, 1.7 to 3.2 up there; the bindings 2.5 to 3.3.

Done for it, awaiting the user's next captures:

- The capture `.txt` carries the renderer's CPU scopes (`BEVY_RENDER_PROFILE=1` at launch) and a `nodes cpu` line, the render graph's nodes timed on the CPU as the diagnostics already timed them (`elapsed_cpu`, beside `elapsed_gpu`). The graph stretch is split in the scopes: `graph.schedule` (the graph's systems), `graph.submit_set` (the command buffers finished, `graph.finish_buffers`, and submitted: `bevy_core_pipeline`'s system, so timed around its set), `graph.final_submit`, `graph.present`. In a scripted c20 the graph's 1.3 ms is: schedule 1.1 (the timed nodes 0.3 of it, the submit set 0.6, of which finishing the buffers 0.3), present 0.1.
- The binder's light probability left the geometry rows (`scene.geometry_dirty_rows` 1069 a frame against 97 transform-dirty rows): the row carries the instance's index in `light_sources` (`light_index`, stable until the lights change), and the hit shader reads the selection probability there (`raytracing_scene_bindings.wesl`), where the buffer is rewritten every frame anyway. Geometry-dirty rows now equal transform-dirty rows (98 or 241 a frame in the scripted runs); `scene.upload` 0.54 to 0.47.

Scripted fights did not reproduce the user's: `ai_place` of the first floor's encounters from the start room (scratchpad `fight.sh`: the flood stay beyond the door; 11.6k entities, the thread 8.1 ms with `mesh.system` 1.6 for 271 skinned meshes a frame, 3.7 MB of vertices) and `HALO_SIM_TELEPORT` to the user's spots (`fight2.sh`, `fight3.sh` by env: the teleport at tick 1200 restarts the level, the tick count back to 0, on the same floor as well as another; the opening cutscene, perhaps). The user's own captures with the scopes on are the next data.

**The scoped captures (7 November).** The user's library and b40 captures with the scopes on. In the library the graph's growth is wgpu's: `graph.finish_buffers` (wgpu 30 records a command buffer's commands into the backend when the encoder is finished, so this is the frame's encode) 0.4 ms in the quiet opening to 2.4 in the upper floors' fights, and the submit set less it (the queue's submit) 0.3 to 1.4; the graph's other systems 0.9 to 1.4; the timed nodes 0.3 to 0.6. In b40 the big fight was the lake's battle (ticks 10 to 12k), and its 15 to 18.7 ms thread was the decals: each fading decal's material changed every frame, and as a uniform binding each change was a new buffer and bind group (`material.reallocated` 331 a frame, `DecalMaterial` 6.7 ms a frame, the compaction wait 5.8).

Done:

- The decal material's params are a `data` binding (as the transparent material's): a change is an upload.
- wgpu-core counts its work a frame (`wgpu.*` in the scopes and the `.txt`, from the vendored crate's `stats` module): the encode by kind (`encode_render` with `render_pass_setup` and `render_draw`, `encode_compute` with `compute_flush`, `encode_as`, `encode_other`), the submit by part (`submit_check`, `submit_as`, `submit_trackers`, `submit_hal`, `submit_cleanup`), and counts (draws, dispatches, passes, submits, bind groups set and their resources merged). In the scripted library (quiet, 11.6k entities): encode render 0.7 (15 passes, 190 draws: setup 0.2, draws 0.13), compute 0.9 (10 passes, 330 dispatches, their binding flushes 0.37), acceleration structures 0.6, copies 0.5; submits 6 a frame with 32 command buffers (hal 0.2, cleanup 0.4). The scene bind group is small here (250 images), so the bindless walk is not the library's cost; what grows in the fights is to be read from the user's next captures with these counters.
- The transparent materials' params moved out of the materials into one storage buffer, the combiner table (`src/combiners.rs`; the material's data binding is its slot, `HaloCombinerSlot`, and `combiners[slot]` in `halo_transparent.wesl`). The library's 606 animated surfaces changed their material every frame, and each change was the asset pipeline's (extracted, prepared, its bind group built, uploaded): now `animate_transparent` writes the table and one buffer is rewritten in place (`ShaderBuffer`, the same size). Scripted library: thread 8.30 to 7.42 ms, `material.updated_in_place` 606 to 140 a frame (the 140 left are the self-illumination's `StandardMaterial`s and their raster copies, which also dirty 346 of the binder's material rows a frame: next). The frames match the old build's.
- The self-illumination's glow moves in steps (`HALO_RT_SELF_ILLUMINATION_STEPS`, 32; 0 for the game's every change of 255): each change was two materials (the traced `StandardMaterial`, the raster copy) through the asset pipeline, 140 a frame in the library (`StandardMaterial` 0.5 ms a frame in the user's fights), now 80; scripted thread 7.42 to 7.26.
- The user's captures with the `wgpu.*` counters (7 November, evening): the library's fights 9 to 11.5 ms on the thread (from 12 to 15.8), b40's lake battle 10 to 12 against a 10 to 12.5 GPU (from 15 to 18.7). What grows now is the passes' setup: `render_pass_setup` 0.26 ms a frame at the start of the library to 4.7 late (15 passes), `compute_pass_setup` 0.14 to 3.7 (11 passes), `compute_flush` 0.9 to 4, with the same dispatches, draws and bind group entries. Each pass makes a usage scope and (compute) an intermediate tracker whose tables are sized to the device's highest resource index ever given (a high-water mark: `TrackerIndexAllocator::size`), and cleared and refilled over their whole length each pass. Done: the scope's and tracker's clears touch only the entries they own (`ResourceMetadata::clear`, `BufferUsageScope::clear`, `TextureUsageScope::clear`, `BufferTracker::reset_sparse`, `TextureTracker::reset_sparse`), the tables keep their length, and the compute passes' intermediate trackers are pooled on the device (`pooled_tracker`, `recycle_tracker`). The scripted library (peak 1018 buffers, 294 textures: `wgpu.tracker_buffers_peak/live`, `_textures_`) is unchanged by it, as it should be; the user's play, where the peak grows through a level (what grows it is the next question: the gauges are in the `.txt`), is the test.
- The plasma weapon's light that stayed where the weapon was put away (the user's, 7 November): the game disconnects a holstered weapon's lights from the map (`object_connect_lights`), so its own renderer, which finds lights by their clusters, neither draws nor updates them; the port's `lights_port_list` walked the whole table. It now skips a light that connects to the map but is not connected.
- Scripted fights: a teleport before the invulnerability kills the player (the level reverts in a loop, the tick back near 1000); `switch_bsp N` before a teleport into another BSP (c20's third floor is BSP 2). Neither brought the enemies to the player yet (`fight2.sh`, `b40fight.sh`): the user's captures remain the fight data.

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
