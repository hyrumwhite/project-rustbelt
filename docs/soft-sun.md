# The sun softened on cliffs and ground

Built 17 November 2026 (the user, of a capture of a30's first valley, `a30_1794916951800`: "Is there anything we can do from a pathtracing perspective to smooth out the harsh shadows on the cliff walls in the game? These are also prominent in Assault on the control room and two betrayals", and after the answer's four options: "Ok, let's try 1 and 2, keeping characters and actors sharp"). The blur is the default at 1.4 m since the user played it the same day (the last section); the terminator rule is opt-in. The viewer alone must be rebuilt.

## What was harsh

The level's cliffs are a few hundred large flat triangles. Traced exactly, the sun draws each facet's edge as a razor line: a facet that leans from the sun is black from edge to edge, and one facet's shadow on the next is a hard triangle.

The original game lit the same triangles. Its lightmaps were not baked from finer rock (the lightmap drawn alone shows the same triangular shadows in the same places); they are soft because a cliff holds about one lightmap texel a world unit (a30 1.08, b40 0.98 to 1.04; a world unit is 3.05 m), read bilinearly. Two things in the bake's softness, measured on the CPU against the bake with `crates/halo_map/examples/cliff_shading.rs`:

- **a30** (sun pitch 35): real cast and self shadows of the facets. A blur of about 1.2 world units across the surface fits the bake best (correlation 0.922 against 0.909 for the exact trace).
- **b40 and c40** (baked sun nearly overhead on near-vertical walls): 48 to 50% of the cliff area leans from the sun and half of that has smooth normals that face it, so facets flip whole between lit and black (the shadow terminator). There the bake fits the smooth normal best, and letting the sun reach such facets beats the blur (0.556 against 0.432 in b40's canyon).

## What it is now

Two rules for the sun's shadow in Solarik's direct lighting (ReSTIR DI), on terrain pixels alone, the first on by default and the second opt-in:

1. **The blur** (`SOLARIK_SUN_BLUR=<metres>`). A terrain pixel takes the terrain's shadow not at its own point but at a point of the terrain up to that many metres away across the surface, another each frame, so the denoiser's average is the shadow blurred over a disc of that radius in the world. It is the lightmap's softness, done by rays.
2. **The terminator rule** (`SOLARIK_SUN_TERMINATOR=1`). A terrain facet that leans from the sun no longer shadows itself: the sun's ray starts just inside it and only the outside faces of terrain stop it, so the smooth normal's shading shows where it was black. `SOLARIK_SUN_TERMINATOR_REACH=<metres>` limits how thick the rock the ray starts in may be (inside faces past the reach stop it too), so a real overhang still shades what is under it.

**Characters, vehicles, trees and structures stay sharp.** The sun's visibility at a terrain pixel is a product of two tests: the terrain's own shadow, at the moved point, and everything else's, from the pixel's own point as before. A marine's or a Warthog's shadow on blurred ground is traced exactly as it was, and so is a tree's on a cliff. Nothing that is not terrain receives a softened shadow.

### What counts as terrain

In the viewer (`LevelBuilder::soft_sun_shader`, src/level.rs): a BSP surface whose shader is an opaque, not alpha-tested `shader_environment` with a physics material type of dirt, sand, stone, snow or ice, on a plain surface (solid, emitting nothing, seen). Never water, glass, a glow or the metal of the Forerunner structures; never an object, even one with the same shader (the BSP gets its own copy of each terrain material, and the copy is marked). On a30: `cliff rock`, the ground shaders and the like marked, `trees_filler` and the metal shaders not.

Solarik is told twice: the material is listed in `RayMaterialMarks::soft_sun` (the binder sets `MATERIAL_FLAG_SOFT_SUN` on it, which is how a ray knows it has met terrain), and the drawn entity carries `NotShadowReceiver` (the G-buffer's shadow-receiver bit clear is how a pixel knows it is terrain). The ray masks are untouched.

### The rays (bevy_solarik/src/scene/sampling.wesl, `sun_soft_transmission`)

For a terrain pixel at p with facet normal g, when the sample is the sun:

1. **Where** (`sun_soft_landing`). A point d on a disc of the blur's radius around p, in the facet's plane, chosen from the sample's seed. A probe ray from a start above p (the radius up along g) through d, twice as far as d is (`SOLARIK_SUN_BLUR_DROP`), so that it finds the surface where it has bent away from the plane: the nearest outside face it meets, if that is terrain, is the landing q; anything else and the pixel keeps its own point.
2. **The start is kept under what is over the pixel.** A ray from p along g finds how much room there is. With less than the full height, the start comes down to 0.9 of the room, so that a probe from under an overhang or a ledge does not land on its sunlit top. With less than a quarter of a radius of room, the start stays at the full height and the landing is taken only if the pixel sees it (a ray from p to q that meets nothing). `SOLARIK_SHADER_DEFS=SUN_SOFT_APEX_UNCLAMPED` is the first form, no such care.
3. **Terrain's shadow at q.** If the facet at q faces the sun, a ray from just above it to the sun, stopped by terrain alone. If it leans from the sun: dark without the terminator rule; with it, a ray from just inside the facet, which only outside faces stop (and inside faces past the reach, the reach jittered between half and one and a half times itself so that rock of about that thickness gives a gradient, not a line).
4. **Everything else's shadow at p.** The ray the pixel always traced, from its own point, with terrain passed: the panes' tints, the alpha tests and the canopy as before.

The landing is a pure function of the seed, the point, the normal and the pixel's ray offset, and no random number is drawn for it, so the two places in the direct pass that test a sample (the candidate's and the final shade's) trace the same rays and the reservoirs' weights stay consistent. The other passes (the bounce, the reflections, the world cache) keep the exact test: they are too diffuse or too rare for the facets to show.

With `SOLARIK_SUN_BLUR=0` and the terminator off none of this is compiled (`SUN_SOFT` undefined) and the shaders are the ones of before.

## Switches

| switch | what it does |
|---|---|
| `SOLARIK_SUN_BLUR=<metres>` | the blur's radius, 1.4 by default (the user's choice in play); 0 off, the old. 3.66 (1.2 world units) is what fits a30's bake; any radius costs the same |
| `SOLARIK_SUN_TERMINATOR=1` | the terminator rule, off by default |
| `SOLARIK_SUN_TERMINATOR_REACH=<metres>` | the thickest rock the terminator's ray may start in; 0 (the default) no limit. 3.05 and 6.1 were tried |
| `SOLARIK_SUN_BLUR_DROP=<factor>` | how far the probe runs, in its start-to-disc distances (2; 1 to 8). 3 and 4 changed nothing seen |
| `HALO_RT_SOFT_SUN_SHADERS=part,part` | the viewer marks the BSP shaders whose names hold any of these instead of the rule above; `none` marks nothing |
| `HALO_RT_SOFT_SUN_LOG=1` | the viewer logs each BSP shader, its material type and whether it was marked |
| `SOLARIK_SHADER_DEFS=VISUALIZE_SUN_SOFT` | with `HALO_RT_NO_DLSS=1`: black not terrain, red the pixel kept its own point, green it landed (blue added where the facet at q leans from the sun), half brightness the start was lowered, quarter brightness the landing was checked by sight, grey it was refused |
| `SOLARIK_SHADER_DEFS=VISUALIZE_SUN_SOFT_FACES` | the facets as the rule sees them |
| `SOLARIK_SHADER_DEFS=SUN_SOFT_APEX_UNCLAMPED` | the probe's start always at full height (the first form) |
| `SOLARIK_SHADER_DEFS=SUN_SOFT_APEX_LEAST_OWN` | the second form: under a twentieth of a radius of room the pixel keeps its own point, no sight ray |
| `SOLARIK_SHADER_DEFS=SUN_TERMINATOR_REACH_FIXED` | the reach unjittered (a line where the rock's thickness crosses it) |

To try another radius or the terminator in play (PowerShell; from a bash prompt the variables go before the command and the game path in single quotes):

```
$env:SOLARIK_SUN_BLUR='3.66'; $env:SOLARIK_SUN_TERMINATOR='1'; $env:SOLARIK_SUN_TERMINATOR_REACH='6.1'
target\release\halo_rt.exe --opence opence\build\windows\halo.exe --menu --simulation all
```

## Measured

Everything in this section is from before the default was set: "off" is the build with nothing set, and the blur is 3.66 m unless said. All on the build of 17 November (the viewer alone; `cargo build --release --features dlss,nrc`), RTX 5080.

**The picture.** a30's cliff over the first valley (the user's capture's camera, harness spot `a30_cliff`): the black facets and the hard triangles between them become soft shading, the trees' shadows on the rock stay as sharp as they were. b40's canyon wall: the blur softens the wall's shadow edge; the terminator with a reach of 3.05 or 6.1 m lights the facets that had flipped black and leaves a gradient; with no reach limit b40's walls are brighter still, and on a30 the big overhang is lit from inside, which is wrong, so a reach belongs with it. b30's beach (sand in shade at the spot looked at): no change. Sheets in `target/release/captures/soft_sun/` (not in the repository).

**In the game** (a30 bridged, the player stood on the sunlit grass under the cliff where the capture had the Warthog, the camera detached at the capture's pose): the cliff softens as in the viewer, and the player's shadow on the grass is the same to within a few levels of 255 at every column of a profile across it, edges included. `HALO_RT_SOFT_SUN_LOG=1` in that run shows the bridged game's level load marking the terrain (of a30's 22 BSP shaders logged, 7 marked, four of dirt and three of stone, and 15 not).

**Cost at the user's setting** (the level viewer, borderless 2560x1440, DLSS Quality, one capture each, GPU ms a frame; off taken twice):

| spot | off | blur 3.66 | blur + terminator, reach 6.1 |
|---|---|---|---|
| a30 cliff | 14.68, 14.68 | 15.48 | 15.49 |
| b40 canyon | 12.33, 12.32 | 12.75 | 12.92 |
| b30 beach | 10.34, 10.31 | 10.61 | 10.62 |

All of it in the direct pass (a30 3.00 ms to 3.77: the candidates' pass 2.37 to 2.75, the shade 0.63 to 1.02), 0.3 to 0.8 ms a frame by how much of the screen is terrain: at a30's cliff 68 frames a second to 65. A terrain pixel whose sample is the sun traces four rays where it traced one (the room, the probe, terrain's shadow, the rest's). The room ray is 0.10 ms of it at 1280x720 (`SUN_SOFT_APEX_UNCLAMPED`: 9.89 ms against 9.98).

**The harness** (1280x720 DLAA, scripts/boil/run.sh, a scratch copy of the saved options at Auto):

| run | spot | gpu | direct | noise | noise p95 | still | moving |
|---|---|---|---|---|---|---|---|
| ss17_off | a30_cliff | 9.54 | 1.80 | 0.298 | 0.89 | 0.002 | 0.002 |
| ss17_blur | a30_cliff | 9.96 | 2.17 | 0.289 | 0.86 | 0.002 | 0.003 |
| ss17_blur_term | a30_cliff | 9.96 | 2.19 | 0.294 | 0.88 | 0.002 | 0.002 |
| ss17_off | b40_canyon | 7.78 | 2.17 | 0.208 | 0.53 | 0.004 | 0.005 |
| ss17_blur | b40_canyon | 8.04 | 2.43 | 0.197 | 0.47 | 0.003 | 0.005 |
| ss17_blur_term | b40_canyon | 8.12 | 2.50 | 0.217 | 0.53 | 0.003 | 0.005 |
| ss17_off | a10_corridor | 6.50 | 1.80 | 0.092 | 0.26 | 0.003 | 0.006 |
| ss17_blur | a10_corridor | 6.52 | 1.81 | 0.091 | 0.26 | 0.003 | 0.006 |

No more noise and no more boiling, still or moving, with either rule; indoors nothing changes. With the switches off the table is the last gate's (hr16: b40_canyon 7.80 ms and 0.207, a10_corridor 6.50 and 0.092).

## Checked

- Solarik's unit tests (86, `soft_sun_mark_sets_its_flag_alone` new), halo_map's (15), and the shader tests with the new variants (scene_shader_variants: the blur alone, the terminator alone, both with a reach, the unclamped and the second forms, the fixed reach, the two views; debug_views with the two views).
- The switches-off captures at 2560x1440 match those of 16 November at b30's beach (the mean of a region of its ground the same to the tenth of a level).
- A first build of the last revision used a reserved word of the shading language as a name and no lighting pass compiled, switches or no; it was renamed before anything here was measured. The shader tests are run on the final source.

## Limits

- **A thin dark line in a30's overhanging creases** (the V notch right of the capture's centre, the top edge of the block beside it), two to three pixels wide at 1280x720. Brightness profiles across it: the crease's darkest pixels are as dark with the feature off (32 and 21 of 255 on two rows) as with it (38 and 45), and as dark with the start's care as without (42 on both rows); what the care does is leave the shaded facet beside the crease brighter (71 against 50, 109 against 85), so the dark core reads as a crack where the first form shows a broader wedge. The crease is real shadow, and the care is what keeps sunlight from under thin ledges; three forms of the rule were tried for this sliver and it was left. `SUN_SOFT_APEX_UNCLAMPED` shows the other look.
- The probe misses the far facet of a steep convex crease (it meets it only where the start is high enough over the edge), so some sharp edges stay sharp. A longer probe (`SOLARIK_SUN_BLUR_DROP` 3 and 4) did not change it.
- The blur softens terrain's shadow on terrain and nothing else: a structure's shadow on the ground (a Forerunner wall's, a bridge's) stays a razor line beside softened rock. Not looked at in play.
- The terminator rule with no reach limit lights rock from inside (a30's overhang). With a reach it is a judgement by eye, on b40 alone so far; c40 was not looked at, nor a30's and b40's other BSPs, nor the snow levels' ground in the open.
- Bounced light, reflections and the world cache still take the exact shadow.
- The cost was read in the viewer, one capture a setting. The pathtracer (`--pathtracer`) has none of it.

## The default: the blur at 1.4 m (17 November)

The user played a30 and b40 with the blur at 3.66 m and the terminator at a reach of 6.1 m, then b40 with the blur alone ("ok, we can leave the terminator off. I'm liking the blur, and you've dialed it to nearly the base look. I want to play with it some though"), then at 1.8 (a30) and at 1.4, and chose: "let's make 1.4 the default and commit". So `SUN_BLUR_DEFAULT` is 1.4 (bevy_solarik/src/realtime/node.rs), a little under half a lightmap texel's softness, sharper than the bake; `SOLARIK_SUN_BLUR=0` is the picture of before, and the terminator rule stays opt-in. The game's log of each of the four sessions and the viewer's of the first three hold no error (the fourth's was not kept).

On the build with the default, nothing set against `SOLARIK_SUN_BLUR=0`, the level viewer at 2560x1440 Quality, one capture each (GPU ms a frame, and the direct pass):

| spot | `SOLARIK_SUN_BLUR=0` | the default | `SOLARIK_SUN_BLUR=1.4` |
|---|---|---|---|
| a30 cliff | 14.70 (3.00) | 15.42 (3.72) | 15.42 (3.72) |
| b40 canyon | 12.13 (3.80) | 12.68 (4.18) | 12.69 (4.18) |

The radius does not change the cost (3.66 m read 15.48 and 12.75). The default's picture differs from the explicit 1.4's by what two runs of one setting do (0.30 of 255 in the mean at a30, 0.48 at b40), and `SOLARIK_SUN_BLUR=0`'s from the captures taken before the default by the same (0.33 and 0.48), against 0.81 and 1.08 between the two settings.

The harness with nothing set (ss17_default) against the table from before (ss17_off), 1280x720 DLAA:

| run | spot | gpu | direct | noise | noise p95 | still | moving |
|---|---|---|---|---|---|---|---|
| ss17_off | a30_cliff | 9.54 | 1.80 | 0.298 | 0.89 | 0.002 | 0.002 |
| ss17_default | a30_cliff | 9.93 | 2.14 | 0.294 | 0.87 | 0.002 | 0.003 |
| ss17_off | b40_canyon | 7.78 | 2.17 | 0.208 | 0.53 | 0.004 | 0.005 |
| ss17_default | b40_canyon | 8.04 | 2.41 | 0.205 | 0.50 | 0.004 | 0.005 |
| ss17_off | a10_corridor | 6.50 | 1.80 | 0.092 | 0.26 | 0.003 | 0.006 |
| ss17_default | a10_corridor | 6.53 | 1.82 | 0.092 | 0.26 | 0.003 | 0.006 |

Outdoors the direct pass pays 0.24 to 0.34 ms there and nothing else moves; indoors nothing does. **Harness tables from now on carry the blur outdoors**: a gate against a table from before 17 November wants `SOLARIK_SUN_BLUR=0`, or ss17_default as the new baseline for these three spots.
