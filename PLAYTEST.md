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

`dist/halo_rt.exe --lit-fog 1` (or `HALO_RT_LIT_FOG=1`) lights the game's fog with the scene's lights: muzzle flashes, plasma and glowing surfaces should light the haze around them, path traced. It's off by default. Things to try:

- **Where:** foggy levels, such as c10's swamp, b30's sea fog and d20.
- **Strength:** 1 is physical. Try 3 to 10 for a stronger look (10 is strong on c10).
- **The sun:** add `,sun` to let it light the fog too (`--lit-fog 3,sun`). The game's fog colour already stands for daylight, so this is off by default.
- **See the term alone:** add `,only`.
- **What to look for:**
  - smearing behind a moving light (the light is averaged over up to 24 frames)
  - glows that look too strong right in front of glowing surfaces
  - the cost: about 0.75 ms a frame at 1280x720

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
