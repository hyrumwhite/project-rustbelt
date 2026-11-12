# Boiling under dim and small lights: what we've done and tried

As of 4 November 2026. Gathered from ROADMAP.md (the "Indoor flicker" passes and the play feedback sections), commit messages and the session's notes. Numbers are the measures we used at the time (see [Measuring](#measuring)); lower is steadier.

## Where it stands

- **Fixed, or acceptable in play:** a10's corridors and Jefferies tubes, b30's Shaft A passage, the a10 fly-in cabin, the receding hull, the first-person weapon while walking, and the a30 drop pod. Outdoors was called "fantastic, great image stability" on 20 October.
- **Still open:** dim rooms lit by many small, weak emitters. These are b40's elevator shafts, its light strips and the canyon wall seen from a Banshee (3 November), and c20's dark Library halls (4 November, deferred). They fit the same pattern as the c10 floor lights (25 October).
- **The common cause of what's left:** Solarik picks lights by power across the whole level. A weak light (a strip of power 10 beside fluorescents of 80, or one of hundreds of emissive meshes) gets few samples. Its surroundings are then lit by whichever pixels happened to draw it, and the denoiser keeps the blotches. Each fix below either gives weak lights more samples near where they shine, keeps more history, or removes a light that stole samples.

## Why dim lights boil

1. **Global light selection.** The light tiles draw candidates in proportion to each light's power, wherever it is. The sun took a fixed half. A level's few huge emitters (a10's hull, BSP-wide strip materials) take most of the rest. A room's own small lights end up as a fraction of a percent of the candidates.
2. **ReSTIR DI reuse spreads a bad pick.** A pixel's chosen light is reused over about 30 pixels and several frames. When the pick is poor (out of sight, or a rarely drawn light), whole patches come and go together.
3. **The world cache feeds itself.** A cell that updates from one sample of one of hundreds of emitters swings as a whole. Bounce light in dim rooms comes almost entirely from the cache, so the swing shows up as slow drift.
4. **Motion shortens history.** Much of what the user sees as boiling happens while moving. The denoiser's history is short then, and still-camera measurements understate it.

## What was done (in order)

| Date | Change | Effect |
|---|---|---|
| ~18 Oct | **`dynamic_scene`**: Solarik keeps its lighting history (DI/GI reservoirs, world cache) through scene changes instead of resetting whenever a light moves or enters the view | Dim rooms were no longer "smoky" in motion. In b30's shaft the cache stayed filled instead of going black and speckled after each reset |
| ~18 Oct | Fixture and game lights are never frustum culled | b30's dark room kept all 42 fixtures (3–6 before). Noise unchanged |
| ~18 Oct | Directional lights take a fixed half of selection | Fixed outdoor sun-shadow staircases (indoors this later became a problem, see 21 Oct) |
| 20 Oct | **Light tile seeding** (third pass): hashed seeds, and a fresh tile per workgroup per frame. Before this, frame f+1 drew frame f's samples shifted by one, so a region read the same tile for hundreds of frames | Drift: a10 strip 0.74→0.33, a10 corridor 0.44→0.26, b30 room 0.12→0.10. The slow fivefold fades were gone |
| 21 Oct | **World cache history** (fourth pass): 128 frames' worth instead of about 5 samples. History is cut in proportion to a cell's change, not by an eighth root that noise always triggered. 128 candidates and 4 shadow-tested samples per update | The cache's own view changed 46%→4% between captures |
| 21 Oct | **ReSTIR DI reuse reduced**: confidence cap 20→8, candidates 8→32 | Wander: b30 passage 0.049→0.027, a10 corridor 0.30→0.11, a10 second spot 0.25→0.056. Cost +0.85 ms |
| 21 Oct | **The sun sampled separately** (fifth pass): every pixel gets its own shadow-tested sun sample, the tiles hold no sun, the cache takes its own sun sample, and the chosen tile light is shadow-tested before reuse. 74% of corridor pixels had picked an out-of-sight light, mostly the sun | a10 corridor in motion 0.098→0.061, b30 passage 0.027→0.011. Cost +0.4 ms |
| 21–22 Oct | **Light candidates by place** (`SOLARIK_CELL_LIGHTS`, on by default): each world-cache cell keeps 4 light slots that are in sight from it. Slots are weighted by each light's shadow-tested irradiance there and mixed with the tiles by the balance heuristic. The cell lookup now tries both sides of a normal's zero component (cells found 48%→77%) | a10 corridor 0.061→0.049. The wall beside a dim red strip 0.35→0.01, and brighter, since the strip is found every frame. Cost about +1 ms in total. **This is the main tool for weak lights** |
| 22 Oct | A masked (hidden first-person) body's emission is no longer a light | The tubes holding a plasma pistol 0.23→0.02 |
| 22 Oct | The game's lights inside the drawn BSP's solid are dropped (`LightsInSolid`) | a10's alarm light from an overlapping BSP had lit the tube. User: "dramatically better" |
| 23 Oct | **Light power weighed by its square root** (`SOLARIK_FLUX_EXPONENT=0.5`) | Big emitters no longer starve small ones. Receding hull 0.45→0.24, the cabin's cells settled on its own lights, and the boarding craft's thin strips were kept (0.040→0.039) |
| 23 Oct | Animated self-illuminated objects recoloured in place instead of respawned | a30 pod: respawned 2573→0 times in 5000 frames. Its boiling and flashing stopped |
| 28 Oct | ReSTIR DI weighs lights with an albedo floor of 4% | Black surfaces pick lights, so they keep their highlights |
| 2 Nov | ReSTIR temporal reuse accepts a previous pixel in either frame's view (`temporal_pixel_dissimilar`) | First-person rifle black pixels while walking 32%→9% |

### Light-level tuning that changed the balance

These weren't boiling fixes. They do change which lights win samples:

- **Dimmed lights that flooded rooms or stole samples** (`LIGHT_DIMMING`, `SMALL_LIGHTS`): c10's coloured door lights to 5% and its batons to half. The user had wondered whether the strong red lights caused the c10 floor-light boiling. a50's door locks to 1/12, b40's door blinkers to 1/25 (12 November: a fifth of that again, in a cone about straight up, `UPWARD_LIGHTS`), sparks to a quarter, Guilty Spark to 0.15.
- **Per-level light shares** (`LEVEL_DIMMING`, `HALO_RT_LEVEL_LIGHT`): Forerunner levels keep 40% of their lamps, and c10 is a tenth darker overall. Darker rooms make the remaining noise more visible relative to the signal.
- **Raised emission for the rays only** (`EMITTER_LIGHT_GAINS`): Truth and Reconciliation's circuits and strips ×12 (later 3), a10's monitors ×16, c20's holograms ×6, its door cards ×4. Each of these gives weak emitters a larger share of samples. This is a per-shader workaround, not a general fix.

## Tried and taken out (or no help)

- **Distance culling of lights** (range + 10 m): no change in noise (0.322 vs 0.335), and Guilty Spark's light went out as he flew off.
- **A ReGIR light grid** (Solarik branches `light-grid`, `light-grid-2`, kept unmerged): no help before the seed fix, and slightly noisier after it.
- **Cache variants:** longer history alone, steadier change detection, history counted in cell updates, and cells living 60 frames unqueried. None changed the drift. Longer lives tripled the cells to 310k, most rarely touched.
- **Emitters weighted by their texture's mean** (`SolarikEmissionAverages`): fixed a10's cabin but starved the boarding craft's thin strips. The square-root weighting replaced it.
- **Splitting an emitting BSP material into per-fixture lights:** no better than the cell candidates.
- **More DI candidates (64, 128)** after the sun change: no difference. **A smaller spatial reuse radius:** worse.
- **A 10% sun share** (`SOLARIK_DIRECTIONAL_SHARE=0.1`): a little help indoors (a10 corridor 0.108→0.095), but it was left at the default for outdoors.
- **Turning off fixture lights, animated glow, or DLSS:** the drift stayed, so none of them was the cause.
- **`SOLARIK_CELL_LIGHTS=0`:** less stable elsewhere.
- **Ending blurry reflections in the world cache** (a9cc1642, a perf win of 1.1 ms): the user saw "dramatically" more boiling and it was reverted (262bb268). Our measurements showed only up to a fifth more noise. **Lesson:** anything that routes more lighting through the cache is a stability risk that the metrics underrate.

## Measuring

- **Reproduce the user's number first.** The level viewer draws every BSP at once. On a10 the nine BSPs overlap and shadow each other (0.11 wander with all, 0.012 with `--bsp 1`), so always pass `--bsp N`.
- **Test in motion.** Use `--dolly 1` with `HALO_RT_DOLLY_DURING=1`, capturing at one phase of each sweep (scratchpad `boil_move.sh`), and compare with the user's three-shot spread. The corridor matched at 0.098 against the user's 0.103.
- **Still-camera measures:** `noise.py` for frame-to-frame change (a10 craft: 0.04 good, 0.07 visibly boiling), and `blotch.py` for patch-scale wander (19-pixel boxes, std over 40 captures 40 frames apart).
- **For flicker, use a 4-frame time series** (`--screenshot-every 4`) and track a region's mean. Scripted runs repeat closely, so A/B runs line up by shape.
- **Diagnostics:**
  - `SOLARIK_SHADER_DEFS=VISUALIZE_WORLD_CACHE`.
  - `VISUALIZE_CELL_LIGHTS` (magenta = no cache cell, black = no light).
  - `SOLARIK_LIGHT_FLUX_LOG=1`: are the room's own lights in the top shares?
  - `HALO_RT_CACHE_LOG=1`.
  - `HALO_RT_LIGHT_LOG`: which game lights are live.
- **In the bridged game:** `HALO_SIM_TELEPORT` puts the player at the capture's spot. The a50 hangar recipe is in `game_boil.sh`. The game sets `dynamic_scene`, so cache-reset theories don't apply there.
- **Pitfalls:**
  - The viewer has no player, so it misses player-carried lights (the plasma pistol case).
  - Look-input turns swamp noise metrics.
  - Captures made right after a shader change may still be compiling.

## Open reports and likely next steps

- **b40 (3 November):** boiling in the elevator shafts, on the light strips, and on the canyon wall from a Banshee.
- **c20 (4 November):** the dark Library halls, where the holograms and door cards are now the only light.
- **c10 (25 October):** floor lights and other lights. The door lights were dimmed then, but no separate boiling fix is recorded.
- **Candidates for next time:**
  - Check with `VISUALIZE_CELL_LIGHTS` whether these rooms' cells find their strips at all.
  - Check with `SOLARIK_LIGHT_FLUX_LOG` what share the strips get.
  - If cells find them but their slots are weak, give the cell candidates more weight against the tiles there, or more slots.
  - If the strips' share is tiny, a per-level selection boost for small emitters would be a general version of `EMITTER_LIGHT_GAINS` that leaves their appearance alone.
  - Prefer changes that keep the estimator the same over ones that trade stability for speed, and have the user A/B in play.
