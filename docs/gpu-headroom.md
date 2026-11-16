# GPU headroom: where time could be bought back

Written 11 November 2026, when the boiling work (docs/boiling-plan.md) was wrapped up. Nothing here was done or scheduled then: it is the list to come back to if the frame needs the room. The user opened it on 16 November; that round is the last section, and it measures at the user's setting (2560x1440, DLSS Quality), not at the tables' 1280x720 DLAA. The last two lighting changes were kept for their stability and cost GPU time; this records what they cost and what could pay for it.

## Where the time goes now

The harness's table of the default build (`scripts/boil/out/cards_default/table.txt`: the level viewer, 1280x720 DLAA, ms of GPU time; 60 frames a second is 16.7 ms):

| spot | GPU | direct | cache | diffuse GI | specular GI | DLSS |
|---|---|---|---|---|---|---|
| `a50_muster` | 8.69 | 2.34 | 1.04 | 1.19 | 1.90 | 1.32 |
| `b40_shaft` | 8.09 | 2.37 | 0.89 | 1.37 | 0.68 | 1.31 |
| `b40_hall_in` | 7.86 | 2.31 | 0.91 | 1.32 | 0.68 | 1.32 |
| `b30_passage` | 7.79 | 2.08 | 1.01 | 1.41 | 0.65 | 1.31 |
| `b40_canyon` | 7.73 | 2.14 | 1.04 | 1.34 | 0.44 | 1.31 |
| `c20_strips` | 7.11 | 2.13 | 0.87 | 1.06 | 0.78 | 1.31 |
| `c20_index` | 6.90 | 2.19 | 0.84 | 1.19 | 0.45 | 1.31 |
| `b30_beach` | 6.35 | 1.34 | 0.93 | 0.95 | 0.39 | 1.49 |
| `c20_hall` | 5.78 | 1.71 | 0.82 | 0.77 | 0.26 | 1.51 |

The game in play costs more than the viewer at the same place (its objects, effects, the HUD, the bridge): the last play measure, b40's corridor on 8 November, was 8.0 to 8.4 ms at 100 to 102 frames a second, before the changes below.

## What 10 and 11 November added

Against `glossy_mis`, the table before them:

| spot | before | now | added |
|---|---|---|---|
| `c20_index` | 6.40 | 6.90 | 0.50 |
| `c20_strips` | 6.64 | 7.11 | 0.47 |
| `b40_canyon` | 7.30 | 7.73 | 0.43 |
| `b30_passage` | 7.37 | 7.79 | 0.42 |
| `a50_muster` | 8.27 | 8.69 | 0.42 |
| `c20_hall` | 5.39 | 5.78 | 0.39 |
| `b40_shaft` | 7.71 | 8.09 | 0.38 |
| `a10_corridor` | 6.08 | 6.32 | 0.24 |
| `a10_tube` | 5.62 | 5.85 | 0.23 |
| `b30_beach` | 6.20 | 6.35 | 0.15 |

Two parts, both the user's choice after play:

- The near-field light sampling (commit d0b62eed): 0.18 to 0.43 ms, in the direct pass and the cache's. It is what ended the flashes in b40's hall and shaft and c20's strip halls. `SOLARIK_SHADER_DEFS=LIGHT_NEAR_AREA` turns it off and gets all of it back, with the flashes.
- Every card patterned (commit 461a285f): nothing to 0.22 ms, in the direct pass. `HALO_RT_EMITTER_MAPS=listed` is the look before.

Neither switch is a candidate here: they are the look and the stability that were chosen.

## Candidates, by how ready they are

### Ready to try: one switch, measured before

1. **Specular reuse at roughness 0.2** (`SOLARIK_SPECULAR_REUSE_ROUGHNESS=20`; the default is 30). Surfaces rougher than the threshold take their reflection from the diffuse GI's reservoir and trace no glossy path.
   - Measured on 6 November against 30: specular GI 1.01 to 0.92 ms at the shaft, 0.73 to 0.51 at b30's passage, 2.25 to 1.74 at b40's smooth-floored corridor in play; every boiling column within 0.002; 0.1% of the shaft's pixels differ by more than 16 of 255.
   - Likely worth most at `a50_muster` (specular GI 1.90 ms, the largest single pass in the table), not measured there.
   - Open since then: the user's look at Covenant and vehicle metal, which was to settle it and never did.
   - 16 November: measured at the user's setting and through part of the gate with candidate 2 (the last section). Still off.
   - New risk since 10 November: the glossy highlight's sharing between the direct pass and the glossy ray (commit 25c27b75) covers roughness up to this threshold. Lowered, surfaces of roughness 0.2 to 0.3 lose the ray and their highlights go back to the direct pass alone, which is what flashed at c20's door rims. `scripts/boil/flash.sh` at `c20_strips` decides.

### Needs a measurement first: the saving is a guess

2. **Fewer tile candidates in the direct pass** (`SOLARIK_DI_TILE_CANDIDATES`, 16 now). Phase 3 found the cell candidates carry the stability and the tile count mostly pays: 24 to 16 saved 0.2 ms with every column held. 8 or 12 were never run until 16 November (the last section: 0.25 ms at 12, 0.5 at 8, at 2560x1440 Quality). The guess then was 0.1 to 0.15 ms, and it would also thin the near-field second pass's false alarms (item 4), since those are tile candidates. Pixels with no cache cell depend on the tile alone (2.9% of the shaft's after Phase 3), so the shaft and the two hall spots are the ones to watch.
3. **The world cache's update cap** (`WORLD_CACHE_CELL_UPDATES_SOFT_CAP`, 40000 cells a frame, a constant in `world_cache_query.wesl`). `HALO_RT_CACHE_LOG=1` says how often it binds; where it does, a lower cap takes time off the cache pass (0.8 to 1.0 ms now) in proportion. The plan's own working rule applies: the measures underrate what the cache does for stability, so this is the riskiest of the three and wants the user's play A/B, not the harness alone. It needs the constant made a tuning value first (a small code change). 16 November: the cap binds at every spot looked at, so there is no spot where lowering it is free.

### Needs code: ideas, not measured

4. **The near-field second pass's false alarms.** At a spot with nothing near (the canyon) the new sampling still costs 0.22 ms over the old: about 0.09 is the tests, and the rest is a second pass that most waves enter for a sparse tile candidate that then turns out not to be near (a tile's sample carries no triangle to test against, only a point and a farthest-corner distance). Up to about 0.1 ms at every spot if the tile's pre-test could be made as tight as the cell's (the triangle's plane and edges), for instance by carrying the triangle's index in the tile sample and reading its record only when the cheap test passes. Tried already and of no use: trimming the warp's arithmetic, a shorter reach (the flashes return), a centroid sphere.
5. **b30's small emissive chunks.** Splitting emissive BSP meshes by cluster (Phase 1) gave b30, one huge BSP, many small chunks, and the passage cost 0.37 ms more from then on (diffuse GI and direct). The plan's remedy, never built: merge chunks under a few dozen triangles into their neighbours. The light triangle table and the refined emitters have changed that code since, so measure first: `HALO_RT_NO_CLUSTER_LIGHTS=1` at `b30_passage` gives the most there is to win.
6. **The muster corridor's specular GI** (1.90 ms). Beyond item 1, nobody has looked at why this spot is twice the next one: which surfaces trace, and whether they need to.

### Not tuning: a different target

7. **A lower DLSS mode.** Every lighting pass scales with the pixels rendered, and DLSS itself is 1.3 to 1.5 ms. Quality or Balanced would buy more than everything above together. The target the user confirmed then was DLAA at 1280x720, so this is a product decision, and the boiling measures would need their own baseline at that mode (the harness measures DLAA). Since the Video Setup menu drives the window (15 November) the user plays 2560x1440 at Quality: the last section has that setting's numbers.

## Looked at already and empty: do not retry

- The sun's shadow ray in the direct pass (9 November): its apparent 0.25 ms was an artifact of the timing switch.
- Cell candidates drawn from points presampled on each light, in two forms (9 November): 0.7 ms cheaper and rejected, shared points boil at any count.
- The confidence caps of the direct and GI reservoirs (Phase 3): no cost either way, no gain.
- An adaptive second spatial neighbour (Phase 3): roughens the stills.
- The world cache's search at 64 tile candidates and the direct pass at 16 tile and 16 cell candidates: already the defaults (0.34 and 0.1 ms taken).
- The light samples' resolve: already one record read from the light triangle table (0.3 to 0.9 ms taken on 9 November).

## The gate for any of these

- The full harness against `cards_default` (`scripts/boil/run.sh <label>`, about 1 h 50 min; the canyon's rows against `sun_b40`, taken after b40's sun moved on 12 November): no boiling column moves.
- The flash counts, which the harness's stills cannot rank: `scripts/boil/flash.sh` at `c20_strips`, `b40_hall_out`, `b40_hall_in` and `b40_shaft` (today, of 300 captures with a flash over 100%: 0 to 3).
- Then the user's A/B in play, each change behind its own switch.
- The harness and `flash.sh` start with the user's saved options, DLSS mode included (Quality since 16 November). To set a run beside the earlier tables give it a scratch copy at Auto: a directory holding `halo_rt/settings.txt`, and `APPDATA=<that directory>` on the script.

## 16 November: the first round

The user opened this list on 16 November, after the stutter fix was confirmed in play: "Lets see if we can get some more performance without impacting visuals". The round took only what changes no pixel; what changes the estimator was measured and left behind its switch.

### The setup measured

The user plays borderless 2560x1440 with DLSS Quality saved (the lighting at 1707x960), not the 1280x720 DLAA of the tables above. The level viewer at that setting before this round (ms of GPU time, frames a second in a scripted window):

| spot | GPU | direct | cache | diffuse GI | specular GI | DLSS | frames/s |
|---|---|---|---|---|---|---|---|
| `a50_muster` | 14.66 | 4.26 | 1.10 | 2.35 | 3.35 | 2.44 | 66 |
| `b40_shaft` | 13.89 | 4.35 | 0.97 | 2.85 | 1.20 | 2.44 | 70 |
| `b40_hall_in` | 13.45 | 4.23 | 0.98 | 2.74 | 1.22 | 2.44 | 72 |
| `b30_passage` | 13.37 | 3.79 | 1.12 | 3.08 | 1.15 | 2.44 | 72 |
| `b40_canyon` | 12.89 | 3.82 | 1.14 | 2.87 | 0.78 | 2.45 | 75 |
| `c20_strips` | 12.03 | 3.88 | 0.94 | 2.16 | 1.36 | 2.44 | 78 |

The game in play at the same setting: a30's hillside 13.5 ms of GPU and 15.1 ms a frame (66 frames a second), b40's corridor 13.7 and 14.8. The same six spots at DLSS Auto, which is Performance at this size (1280x720): 7.97 to 9.61 ms, 100 to 116 frames a second.

### Taken: three passes that worked where there was nothing for them

None of these changes a pixel; each has its switch for the old.

- **The glass pass's probe.** The camera's pane pass traced the whole scene for every pixel whenever the level held a pane anywhere (water counts), 0.27 to 0.72 ms. Each pixel now first sends a probe that meets pane instances alone and stops at its first hit, and returns the background at once where there is none (bevy_solarik/docs/glass.md): 0.12 to 0.14 ms with no pane in view, 0.35 to 0.50 with panes. `SOLARIK_SHADER_DEFS=GLASS_UNPROBED` is the old.
- **The foliage pass's probe.** The same for leaves when the scene has no canopy (every level but a30's trees, which keep the full trace): 0.23 to 0.45 ms to 0.08 to 0.10. `SOLARIK_SHADER_DEFS=FOLIAGE_UNPROBED` is the old.
- **The sky's sampling distribution** was rebuilt every frame from a cubemap that never changes after the level loads, 0.11 to 0.12 ms. It is rebuilt when its inputs change (`SolarikStaticSky`, bevy_solarik/docs/sky-sampling.md). `SOLARIK_SKY_REBUILD=1` is the old.

The viewer at 2560x1440 Quality, before and after (the old by the three switches on the same build):

| spot | before | after | saved | frames/s |
|---|---|---|---|---|
| `b30_passage` | 13.37 | 12.71 | 0.66 | 72 to 76 |
| `b40_shaft` | 13.90 | 13.28 | 0.62 | 70 to 73 |
| `b40_hall_in` | 13.45 | 12.91 | 0.54 | 72 to 75 |
| `b40_canyon` | 12.89 | 12.38 | 0.51 | 75 to 78 |
| `a30_trees` | 13.33 | 12.88 | 0.45 | 72 to 75 |
| `c20_strips` | 12.03 | 11.67 | 0.36 | 78 to 80 |
| `b30_beach` | 10.72 | 10.39 | 0.33 | 90 to 93 |
| `a10_tube` | 10.03 | 9.77 | 0.26 | 95 to 97 |
| `a50_muster` | 14.66 | 14.43 | 0.23 | 66 to 67 |

How "no pixel" was checked. `SOLARIK_SHADER_DEFS=PROBE_CHECK` runs the old trace beside the probe and paints a pixel white where the trace met a pane or a leaf although the probe missed: none at seven spots (b30_beach, a30_trees, a10_tube, b40_shaft, a50_muster, b40_canyon, b30_passage). `PROBE_CHECK,PROBE_CHECK_WRONG` gives each pass the other's probe, to show the check can fail: a fifth of the beach's pixels (its water), a tenth of the shaft's, 7.7% of `b40_hall_in`'s, the canyon's one tree (838 pixels). Pictures new against old on one build differ by what two runs of one setting differ by (mean absolute difference of 255: the shaft 0.294 against 0.290, the passage 0.166 against 0.163, the beach 0.439 against 0.437). Two things to know for such pairs: Solarik's water ripples drift by wall time, so a faster build draws another phase of them at the same frame (`SOLARIK_FREEZE_TIME=1` holds them, with `HALO_RT_FREEZE_ANIMATION=1` for the viewer's own); and pictures from two builds differ by more than that floor whatever the switches say (0.38 to 0.40 at the shaft, cause not found), so compare switches on one build and builds by the harness.

The gate. The harness on the final build, on a scratch copy of the saved options at Auto so that it is 1280x720 DLAA like every earlier table (`scripts/boil/out/hr16`: b40_canyon, a10_corridor, a50_muster, b40_hall_in), against the last gate's (`hl15`): the GPU 7.80, 6.50, 8.97 and 7.98 ms against 8.10, 6.74, 9.03 and 8.34; the noise column 0.207, 0.092, 0.111 and 0.078 against 0.208, 0.092, 0.100 and 0.080 (the muster's wanders with its hologram, 0.105 and 0.117 in the two tables before; its diffuse GI pass reads 1.49 ms here and in those two and 1.34 in `hl15` alone, so its GPU is better set against their 9.17 and 9.09); the still and moving columns within 0.001. The corridor's cache pass reads 0.14 ms more and its TLAS build 0.17 less, in this table and the tunables' alike, the two's sum the same (1.16 against 1.19); not looked into. The passes taken read as in the viewer: the glass pass 0.17 to 0.32 ms to 0.07 to 0.22, the foliage pass 0.23 to 0.05 in the hall and 0.46 to 0.37 in the canyon (its tree in view), the sky's distribution 0.11 to 0.12 to nothing; the direct, GI and specular passes within 0.01 but for that one. Solarik's unit tests and its scene_hit, scene_shader_variants (the check's variants among them), debug_views, nrc_shaders and pathtracer_layout tests pass.

### Taken on the game's side: work nobody sees

Under a viewer the hidden game draws only its overlay, but still ran the whole frame's draw calls. Three changes (opence/port/linux/src/d3d8_gl.c), each with its switch:

- A draw that writes no colour, depth or stencil and that no visibility test counts is left out (`HALO_BEVY_OVERLAY_NOOP_DRAWS=1` the old): in b40's corridor 60 of a frame's 84.
- The objects' shadows are not drawn under the overlay, where nothing of them is seen (`HALO_BEVY_OVERLAY_SHADOWS=1` the old).
- The hidden window's present is the swap alone, without the clear and the blit of a picture nobody sees (`HALO_BEVY_HIDDEN_PRESENT=0` the old).

`HALO_BEVY_DRAW_LOG=1` logs the draws, those left out and the frame's GPU time every 300 frames. The game's own GPU time with an empty overlay went from 0.16 ms a frame to 0.04; with the grass in it (a30) from 1.55 to 1.52, since there the read back is the cost. The pacing measures of the stutter work are as they were in every variant (a30: no repeats, a deviation of 0.63 to 0.67 ms).

In play at the user's setting, everything together (scripted, the frame's interval): a30's hillside 15.12 ms to 14.75, 14.66 and 14.65 over three runs (66 frames a second to 68), b40's corridor 14.83 to 14.69 and 14.48. Less than the viewer alone saves: the game's side moved the viewer's frame by 0.1 to 0.25 ms at most, near what two runs differ by, and the corridor's probes save less than the viewer's spots do (not looked into).

`HALO_BEVY_HIDDEN_PRESENT=2` (a flush in place of the swap) is not the default: 14.45, 14.44 and 14.72 ms on a30 against the default's 14.75, 14.66 and 14.65, 13.87 against 14.69 and 14.48 in the corridor, and a fourth a30 run with it hitched six times a second (intervals of 25 to 100 ms) for a reason not found. Unproven either way.

### Measured and left: they change the estimator

At 2560x1440 Quality, the six spots of the first table (the stills differ from the default's by about what two runs do, 0.17 to 0.54 of 255 against a floor of 0.16 to 0.31, which cannot rank them: the gate below is still theirs to pass):

- Candidate 1, `SOLARIK_SPECULAR_REUSE_ROUGHNESS=20`: the muster corridor 14.66 to 14.12 ms (specular GI 3.35 to 2.78), b30's passage 13.37 to 13.06, the rest 0.1 or less.
- Candidate 2, `SOLARIK_DI_TILE_CANDIDATES=12`: the direct pass 0.24 to 0.26 ms less at every spot; `=8`: 0.48 to 0.52.
- Candidate 3: the cache's update cap binds at every spot looked at (`HALO_RT_CACHE_LOG=1`), so a lower cap would take time off everywhere and change what the cache knows everywhere. Not made a tuning value.

Both together (`SOLARIK_SPECULAR_REUSE_ROUGHNESS=20 SOLARIK_DI_TILE_CANDIDATES=12`) went through part of the gate, at 1280x720 DLAA: the harness's four spots (`hr16_tune` against `hr16`) 7.67, 6.17, 8.52 and 7.85 ms against 7.80, 6.50, 8.97 and 7.98; the noise column 0.207, 0.091, 0.113 and 0.079 against 0.207, 0.092, 0.111 and 0.078; the still and moving columns within 0.002 (the hall's stillP90 0.014 against 0.012); the flash counts of 300 captures at `c20_strips` 8 over 50% and none over 100% against 9 and none, at `b40_shaft` none against none. Not run: the full harness's other spots, the flash runs at the two hall spots, and the user's look at Covenant and vehicle metal in play, which candidate 1 has waited on since 6 November. So both stay as they were; for a play A/B, set the two on the launch (at 2560x1440 Quality about 0.3 to 0.8 ms a frame).

### What is left, largest first

- **The DLSS mode** (candidate 7) is now the user's own setting and by far the largest lever: Quality costs 12.0 to 14.7 ms where Performance (what Auto picks at this size) costs 8.0 to 9.6. Balanced was not measured in this round.
- **The overlay's round trip.** A frame whose overlay is not empty (grass, particles, decals, water, a menu; the HUD is the viewer's own pass) is read back by the game (14.7 MB at 2560x1440), copied through shared memory and uploaded again by the viewer. In b40's corridor with the read back forced (`HALO_BEVY_OVERLAY_ALWAYS=1`) the frame is 15.13 ms against 14.48: 0.65 ms, 3 frames a second, wherever anything is in the overlay. Sharing the texture between the game's GL and the viewer's Vulkan would end it; reading and uploading only the rows drawn into would cut it. Neither is small.
- **The game-only passes** the viewer's GPU log times in play: the HUD 0.26 to 0.29 ms, the objects' acceleration structures 0.24 to 0.26, the fog 0.12 to 0.15, the UI 0.12, and about 1.2 to 1.6 ms a frame that no pass's timer covers (0.45 in the viewer alone). Not looked into.
- Candidates 4, 5 and 6 above, untouched.
