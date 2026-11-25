# Playtest plan: scripts in Rust, and the viewer's HUD

What to play to exercise the script library's Rust bodies and the HUD the viewer now draws (25 October). `dist/halo_rt.exe` runs every simulation system in Rust by default, the scripts included, so a normal launch covers all of it.

## What a failure looks like

A wrong script body rarely crashes. It stalls:

- enemies that never spawn, or a wave that never starts
- a door or elevator that doesn't move
- a cutscene that never ends, or a screen stuck black after a fade
- dialogue or an objective that never fires
- characters left frozen after a cutscene

## Levels, most exercised first

| Level | What runs in Rust there | What to do |
|---|---|---|
| **c10** (343 Guilty Spark) | the most of any: player lists, trigger volume tests, the structure BSP (about 18,000 calls in the first 70 s) | push through the swamp into the facility and past the Flood reveal; each area change and wave start is a volume test |
| **c20** (The Library) | door and elevator positions (`device_set_position`), all-players volume tests, `ai_disregard` | ride the lifts; doors should close behind you and floors advance |
| **d20** (Keyes) | device positions, single-object volume tests | doors and elevators again |
| **a50** (Truth and Reconciliation) | encounter counts (`ai_living_count`), which decide when waves and checkpoints advance | clear rooms; the next event should follow |
| **b40** (Assault on the Control Room) | the seat test (`vehicle_test_seat_list`) | wherever the script waits for you or marines to board a vehicle, board, and see it move on. The least exercised body: only a contrived test has made it return true so far |
| **c40** (Two Betrayals) | `ai_command_list_status`, for scripted AI sequences | watch scripted marines and Covenant finish their moves and the scene go on |
| **any cutscene** (a10's cryo opening is quick) | `fade_in`, `fade_out`, `unit_suspended` | fades should end; characters should move again after the scene |

## The HUD (any level)

The viewer now draws the HUD, menus and text, the motion sensor, and the screen flash (damage, pickups, scripted fades). The game's own picture is skipped when nothing was drawn into it, which should be most frames, so play may feel slightly smoother.

To find anything still left to the game's picture, run with `HALO_RT_OVERLAY_DEBUG=1`: whatever the game still draws shows tinted red. Only b30's gameplay and a10's opening have been checked (empty every frame). Likely places to look: the menus, other levels' cutscenes, the Flood, Sentinels, the Library's lighting.

## If something stalls

1. Note the level and roughly where you were.
2. Replay that spot with everything in C: `dist/halo_rt.exe --simulation none`. If it plays fine that way, the problem is likely on the Rust side. (Not yet tried on the packaged build; the flag is passed through.)
3. Pass on where it happened. With that, check mode (`--simulation hs:check`, the scripts only, the other systems in C) logs the exact library function that first disagrees with the game.

## Optional: lit fog

`dist/halo_rt.exe --lit-fog 1` (or `HALO_RT_LIT_FOG=1`) lights the game's fog with the scene's lights: muzzle flashes, plasma and glowing surfaces should light the haze around them, path traced. It's off by default, and F10's panel has it too (`Lit fog: on|off` and `Lit fog strength`, saved). Things to try:

- **Where:** foggy levels, such as c10's swamp, b30's sea fog and d20.
- **Strength:** 1 is physical. Try 3 to 10 for a stronger look (10 is strong on c10).
- **The sun:** add `,sun` to let it light the fog too (`--lit-fog 3,sun`). The game's fog colour already stands for daylight, so this is off by default.
- **See the term alone:** add `,only`.
- **What to look for:**
  - grain or shimmer in the glow, above all at the higher strengths (there was, crawling, at every strength over 0.5; filtered wider and over up to 96 frames on 20 November, `SOLARIK_LIT_FOG_LEVELS=0 SOLARIK_LIT_FOG_FRAMES=24 SOLARIK_LIT_FOG_CLAMP=0` the old)
  - soft blotches that crawl in dark far fog, a corridor's far end or a hangar door (there were, on a50 at strength 3; the history's clamp widened by the light's own noise on 22 November, `SOLARIK_LIT_FOG_CLAMP_NOISE=0` the old)
  - bright lines along silhouettes in thick fog, far things pale against a darker sky, patches over glass (there were, on c10 at strength 3; the light laid over the depth fog since 22 November, in the pass before the upscaler since the 23rd; the lift's glass not reproduced by script, look there)
  - a shield, a bolt or a glow cut off where it stands against far fog, or far trunks showing as streaks in lit fog (there were, on c10; the fog before the upscaler and the light's reach on 23 November, `SOLARIK_LIT_FOG_REACH=0` the old of the second)
  - the Covenant torches on b40 and c40 light the snow and their poles (since 23 November, share 0.02; `HALO_RT_STATIC_LIGHTS=0` the old, `HALO_RT_STATIC_LIGHTS="ctorch=<share>"` to try another): too bright, too pale, a white cup, a light popping in or out
  - the fog is laid before the upscaler since 23 November (the fog after the upscaler, its curtain, raise and edge rule removed on the 24th; the commit before that has them): look for any outline left on things against fog, fog darker or blotchier when moving than standing (c10), rock or ground texture too clear through mid fog, b30's shore foam and c10's near water (both show more now), lit fog's glow against what it was, b40's sky and dome, c40's snow against the sky, a shield or a pane against far fog
  - the sky model's cubemap is no longer mirrored (since 24 November; `HALO_RT_SKY_CUBE_HANDED=0` the old): one gas giant on the main menu, not two; on levels with a sky (b30's beach and sea, a30, d20) the sky's light from the wrong side, a reflection of the sky in water or glass that does not match the sky above it, a planet or the ring seen twice or missing
  - the Pelican's bay lamps cast no more light than the game's (off again on 24 November: the bay boiled in flight; `HALO_RT_EMITTER_LIGHT=pelican_bay=32` the lamps as played): the bay as dark as before
  - lit fog while moving (since 24 November; `SOLARIK_LIT_FOG_LOCAL=0` with `SOLARIK_LIT_FOG_NOISE_STILL=0` and `SOLARIK_LIT_FOG_DISOCCLUSION=0` the old): a small blotchy pattern or dark specks in the flashlight's cloud after a turn, an afterimage of a rock's or a fern's edge in the fog, the fog by the flashlight and the lamps brighter than it was at the same strength, blotches come back in dark far fog when walking slowly
  - a glow that trails a moving light or hangs after a light goes out (it should not: the history is clamped to each frame's light)
  - a soft mottle while firing in thick fog (known: a muzzle flash is one frame's light)
  - glows that look too strong right in front of glowing surfaces
  - squares in the glow (there were, at every strength; smoothed on 20 November, `SOLARIK_SHADER_DEFS=LIT_FOG_COVERAGE_POINT,LIT_FOG_BOX` the old)
  - the cost: about 1.9 ms a frame at 2560x1440 with DLSS Quality, 1.1 ms with DLSS Auto

## Fixes from the c10 pass to check

- **Starting a level from the menus:** c10 crashed as it loaded (a bitmap kept from the menus' map was a particle there); any level could have.
- **c10's door lights:** the coloured lights beside the doors are at a tenth of their old brightness.
- **c10's swamps:** the water's pass is about 8.5 ms cheaper (the frame went from 28 to 20 ms on the opening's view). The stats panel now lists "Glass and water", "Foliage" and "Lit fog", which it used to leave out of its total.
- **The Flood reveal's helmet recording:** the game's screen effects are drawn now: the video look (green scanlines, noise) and light enhancement (night vision), and desaturation. It was black.
## Fixes from the 25 October findings to check (c10)

- **Blood that came and went with the view:** the game now shares every decal, not only those of the areas in front of the camera.
- **Windows black beyond a circle round you, and the hatching through the yellow structures:** the glass's reflections now show the room instead of black, and the stripes of noise seen through tinted glass are gone. The glass pass there is about twice as fast (5.4 to 2.75 ms).
- **Shot-out glass:** the viewer now drops a pane the game has broken. The yellow cast inside the centre structures was the glass, so it goes with it.
- **Helmet recording:** seen from the marine's eyes, not from inside his head or chest.
- **Lights:** the door lights are at half their last brightness, the batons (glow sticks) at half, without the dark disc round them, and all of c10 is a tenth darker.
- **Camouflage:** your own arms and weapon are glass, like camouflaged enemies.
- **Canopy fog:** above the trees there is only fog, as in MCC.
- **Not changed:** the boiling floor light near the windows (findings 6 and 8) and Chief's dull visor. c10's jittering scenery lights flicker by design, which keeps the denoiser from settling; they're dimmer now, so see whether it's still bad. The pillar strips' glow looks animated by their shader.

## Known gaps (not bugs to report)

Drawn by neither the game nor the viewer yet:


- the sun glow around the sun
- active camouflage's shimmer (the viewer draws a faint glassy shell instead)
- texture animation on character and vehicle shaders (level surfaces do animate)
- lens flares hidden behind the first-person weapon
