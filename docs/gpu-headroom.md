# GPU headroom: where time could be bought back

Written 11 November 2026, when the boiling work (docs/boiling-plan.md) was wrapped up. Nothing here is done or scheduled: it is the list to come back to if the frame needs the room. The last two lighting changes were kept for their stability and cost GPU time; this records what they cost and what could pay for it.

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
   - New risk since 10 November: the glossy highlight's sharing between the direct pass and the glossy ray (commit 25c27b75) covers roughness up to this threshold. Lowered, surfaces of roughness 0.2 to 0.3 lose the ray and their highlights go back to the direct pass alone, which is what flashed at c20's door rims. `scripts/boil/flash.sh` at `c20_strips` decides.

### Needs a measurement first: the saving is a guess

2. **Fewer tile candidates in the direct pass** (`SOLARIK_DI_TILE_CANDIDATES`, 16 now). Phase 3 found the cell candidates carry the stability and the tile count mostly pays: 24 to 16 saved 0.2 ms with every column held. 8 or 12 were never run. A guess of 0.1 to 0.15 ms, and it would also thin the near-field second pass's false alarms (item 4), since those are tile candidates. Pixels with no cache cell depend on the tile alone (2.9% of the shaft's after Phase 3), so the shaft and the two hall spots are the ones to watch.
3. **The world cache's update cap** (`WORLD_CACHE_CELL_UPDATES_SOFT_CAP`, 40000 cells a frame, a constant in `world_cache_query.wesl`). `HALO_RT_CACHE_LOG=1` says how often it binds; where it does, a lower cap takes time off the cache pass (0.8 to 1.0 ms now) in proportion. The plan's own working rule applies: the measures underrate what the cache does for stability, so this is the riskiest of the three and wants the user's play A/B, not the harness alone. It needs the constant made a tuning value first (a small code change).

### Needs code: ideas, not measured

4. **The near-field second pass's false alarms.** At a spot with nothing near (the canyon) the new sampling still costs 0.22 ms over the old: about 0.09 is the tests, and the rest is a second pass that most waves enter for a sparse tile candidate that then turns out not to be near (a tile's sample carries no triangle to test against, only a point and a farthest-corner distance). Up to about 0.1 ms at every spot if the tile's pre-test could be made as tight as the cell's (the triangle's plane and edges), for instance by carrying the triangle's index in the tile sample and reading its record only when the cheap test passes. Tried already and of no use: trimming the warp's arithmetic, a shorter reach (the flashes return), a centroid sphere.
5. **b30's small emissive chunks.** Splitting emissive BSP meshes by cluster (Phase 1) gave b30, one huge BSP, many small chunks, and the passage cost 0.37 ms more from then on (diffuse GI and direct). The plan's remedy, never built: merge chunks under a few dozen triangles into their neighbours. The light triangle table and the refined emitters have changed that code since, so measure first: `HALO_RT_NO_CLUSTER_LIGHTS=1` at `b30_passage` gives the most there is to win.
6. **The muster corridor's specular GI** (1.90 ms). Beyond item 1, nobody has looked at why this spot is twice the next one: which surfaces trace, and whether they need to.

### Not tuning: a different target

7. **A lower DLSS mode.** Every lighting pass scales with the pixels rendered, and DLSS itself is 1.3 to 1.5 ms. Quality or Balanced would buy more than everything above together. The target the user confirmed is DLAA at 1280x720, so this is a product decision, and the boiling measures would need their own baseline at that mode (the harness measures DLAA).

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
