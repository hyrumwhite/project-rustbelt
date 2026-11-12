# Glass: steadier reflections, bump maps, grime

Written 11 November 2026 from three reports of the same day: glass reflections show sampling patterns (five captures of a10, the bridge, from `a10_1794277861308.png`); most glass hides its grime and other textures unless it is directly lit; the glass of b40 (Assault on the Control Room) in the transition hallways has a circuit pattern in MCC that this build does not show at all. Solarik's side of glass is described in bevy_solarik/docs/glass.md; this is the plan, what was found on the way, and what was measured. All four parts below are in and on by default, each with a switch.

## What was found

### The sampling patterns are world cache cells seen one at a time

Measured on a10's bridge with the game frozen at tick 2167 (`HALO_SIM_HS_COMMAND="(game_speed 0)"`, `HALO_SIM_HS_COMMAND_TIME=2167`, the camera of the captures: `--camera 0.33,-0.13,0.62,6.2,-0.6`), with three diagnostic views added to `primary_glass.wesl` (`SOLARIK_SHADER_DEFS=GLASS_VIEW_REFLECTION`, `GLASS_VIEW_MIRROR`, `GLASS_NO_REFLECTION`):

- Without the reflection the panes are steady. The pattern is all in the reflected light, none of it in the grime.
- The first reading was the reflected path's own noise: the path (`trace_unfogged_glossy_path`) ended in the world cache only at a rough hit farther than a cell, and at every other hit took one light sample and bounced on. Part A ended that. It took the black pixels of the raw reflection from 1.2% to 0.2% and 0.2 ms off the pass, and the blobs stayed.
- What was left is the cache itself. A reflected path that ends in the cache reads one cell (about 0.15 m at the finest level), and each cell's value wanders slowly on its own. Seen in a mirror the cells are a mosaic; DLSS, which has no guide for a reflection (its guides on a pane are the background's), smooths the mosaic into soft blobs that come and go over seconds. The map of each pixel's deviation over 22 frozen captures shows them all over the reflection, and none with the reflection off. Part D is the cure.
- The bridge is the worst case: its `monitor huge` glass reflects strongly (0.125 head-on), the camera looks through many panes at once, and the room behind is dark, so the reflection is most of the picture.

### b40's hallway pattern is the glass shader's bump map

Halo's glass shader has three passes (OpenCE `rasterizer_xbox_transparent_geometry.c`): the tint, a cube-map reflection added over it, and the diffuse map blended over. The reflection is bumped when the reflection type is 0 (bumped cube map), a bump map is present and the flag "bump map is specular mask" is clear: the pane's normal is bent by the bump map, tiled at `reflection bump map scale`. b40's `b40_ctrlglass` has `tech dense circuits bump` (flat plates between grooves) at scale 3, `b40_ctrlglassb` at 5: the circuit pattern of the MCC capture is that bumped reflection. This build reflected about the flat pane and never read the map. In ten campaign maps three shaders are of this kind: those two and a10's `burn monitor huge`.

The corridor itself (BSP 3, Halo's coordinates): it runs along y from about 15.5 to 26 between x 47.07 and 49.07, narrowing toward the ceiling at z 51.7, floor at z 48.0. The glass panels (0.75 m wide, 2.9 m tall, at y 19.24, 21.49, 17.0 and 23.73 on both walls) cover niches half a metre deep; the glyph floats in the niche behind the glass. Between the panels are the light-strip niches. A viewer camera must stand inside the hull: `--camera 48.7,18.6,48.65,148,38` is the MCC capture's view, `48.6,19.6,48.65,180,27` faces a panel. (Cameras outside the wall see the niches' back faces and no glass, and cost a wrong reading on the way: a black band where nothing bends.)

The panel's own diffuse map is black with no alpha: there is no grime on it to show. Everything MCC shows on it is the reflection.

### Grime shows only by the light on the side looked at

A glass's diffuse map is Solarik's grime layer, lit by the world cache's irradiance at the pane on the side facing the camera. Halo blends the map over the pane unlit, at full brightness, so it shows everywhere. Lit from the viewer's side alone, a grimy pane in a dim room with a bright space behind shows almost nothing, though a real dirty film scatters the light that comes through it too.

## The plan, as built

Every change has a switch for an A/B in play; the old behaviour is one environment variable away.

### A. Pane reflections end in the world cache (Solarik, `specular_gi.wesl`)

A pane's reflected path (`trace_unfogged_glossy_path` with `pane_reflection` set: the camera's pane pass, glass and water) ends at its first hit of an opaque surface that is not a mirror, whatever its roughness and distance: the surface's emission as before, plus its base colour times the cache's irradiance there. Mirrors, further panes and leaves are followed as before. The sky and emitters are seen sharp; lit surfaces are seen as the cache has them. Glossy highlights on the reflected surfaces are lost, which a pane's faint reflection does not need. `SOLARIK_SHADER_DEFS=GLASS_REFLECTION_PATHS` is the old path.

### D. The reflection reads four cells, not one (Solarik, `world_cache_query.wesl`, `specular_gi.wesl`)

Added after A was measured. `query_world_cache_spread`: four lookups on a circle of 1.25 cells round the hit, in the surface's plane at a random angle, each an ordinary jittered query; the brightest and the darkest are dropped and the two between averaged. A cell that spikes or sags no longer shows, and the mosaic's edges are gone, at the cost of three more lookups for each pane pixel. `SOLARIK_SHADER_DEFS=GLASS_REFLECTION_ONE_CELL` is the single cell.

The grime's reads can take the same spread (`SOLARIK_SHADER_DEFS=GLASS_GRIME_SPREAD`). It is off: the film was already steady by one cell, and the spread cost 0.29 ms at the bridge for nothing seen.

### B. Glass bump maps (halo side, `halo_map`, `src/level.rs`)

`halo_map::shader::Glass` gained the shader's flags, reflection type, bump map and its scale (and the diffuse detail map and scale, unused so far). A glass whose reflection is Halo's bumped kind registers its bump map as the pane's ripple in Solarik's `WaterRippleMaterials`: one still map, no drift, no fade. The camera's pane pass already bends its reflection by that registry (it is how the sea ripples), so there are no shader changes. `HALO_RT_NO_GLASS_BUMP=1` leaves the panes flat.

### C. Grime lit from both sides (Solarik, `primary_glass.wesl`)

The grime layer takes the irradiance of both sides of the pane: what falls on the side looked at, and what falls on the far side and scatters through the film. `SOLARIK_SHADER_DEFS=GLASS_GRIME_ONE_SIDED` is the old.

## Results

### The bridge (a10, frozen, 22 captures 8 frames apart)

| build | blobs in the deviation map | 99th percentile of a 16 px block's step between captures (of 255) | `primary_glass` |
|---|---|---|---|
| before any of this | all over the reflection | not measured this way | 2.05 ms |
| A and C | all over the reflection | 5.9 | 1.84 ms |
| A, C, D (the default; two runs) | none | 1.1 and 1.5 | 2.16 and 2.15 ms |
| the same build, `GLASS_REFLECTION_ONE_CELL` | back | 3.4 | (1.87 ms with the grime's single cell) |
| A, C, D and the grime's spread | none | 1.1 | 2.45 ms |

The numbers of the last column are the pass at the bridge, where panes in several layers fill 62% of the frame: the reflection's spread costs 0.3 ms there and A had saved 0.2. Elsewhere the pass is a fraction of that (0.6 ms in b40's corridor).

The reflection before DLSS (`GLASS_VIEW_MIRROR`, `HALO_RT_NO_DLSS=1`, a region of the main pane): a 16 px block's deviation over time 3.16 to 2.03, the 99th percentile step 12.7 to 9.0, the largest step 63 to 30, the mean the same (47.3 and 47.2 of 255): the spread does not darken or brighten the reflection.

The block statistics over the denoised frames move from run to run with the monitors' animation, which the freeze stops at a different frame each time; the deviation map (`stdmap.py` in the scratch tools: the first capture, each pixel's deviation times 8, its largest step times 4) is the judge, and the masked measure (`unrest.py`, the mask from a `GLASS_NO_REFLECTION` run) only supports it.

### b40's corridor

From the MCC capture's view the panel now shows the circuit pattern wherever something bright reflects: the light strip of the opposite wall, a clean bar in the flat pane, is broken into plates with dark grooves between, and elsewhere the grooves catch the strips and the ceiling lights as thin bright lines. At rest it is steady over 22 captures; in a run along the corridor (`--dolly 3`, consecutive frames) the lines hold from frame to frame.

It is not MCC's look exactly, and the difference is a choice. Halo adds an authored cube map (`cube map forerunner interiors`, bright all round) at about 9% head-on, so its whole panel has a navy sheen with dark lines. Here the pane reflects the corridor itself at about 11% head-on, and the corridor is dim: the plates, which mirror the opposite wall, stay near black, and the pattern lives where the lights are. With the reflection unweighted (`GLASS_VIEW_MIRROR`) the whole panel shows the pattern, so it is all there, only dark. A stand-in for the cube map's sheen (a floor under the reflection scaled by the light at the pane) would bring MCC's look and was not built: it is light that is not in the room.

### Grime

b40's dome from below (the level viewer, BSP 3, `--camera 45,50,80,90,45`), the same build with and without `GLASS_GRIME_ONE_SIDED`: one-sided, the dome's panes are a clear dark window on the sky's clouds, their streaks faint; two-sided, the sky's light on the far side lights the streaks and the panes are a pale, mottled film over the sky, 28% of the frame's pixels different by more than 8 of 255. The pass costs the same (0.67 and 0.69 ms there). b30's sea from the beach (no grime; the reflection's new ending and spread) against `GLASS_REFLECTION_PATHS`: 0.2% of the frame's pixels differ by more than 8 of 255. The sum of both sides is not energy conserving for a pane lit strongly from both (a film cannot scatter all of both); it errs bright, by at most the dimmer side.

## Left open

- The diffuse detail map (b40's floor glass multiplies its diffuse map by `detail tech panels` at 15): parsed, not applied.
- The bump bends the camera's view of the pane only. A pane seen in another reflection, or lit through, is flat.
- A reflection guide for DLSS (the reflected surface's motion and normal on panes that are mostly reflection): moving reflections may still ghost. Large, not started.
- The cube-map sheen above, if MCC's look is wanted over the room's own light.
- Glass built by the game bridge for objects gets no bump registry (`src/bridge.rs` passes none). No object's glass has a bumped reflection in the maps checked.
- The pane pass's cache reads do not use the first-bounce light-leak prevention the other passes may.
