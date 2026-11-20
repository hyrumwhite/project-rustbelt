# Frame pacing traces

Scripts that read the frame traces of a game run (the viewer with the game hidden behind it) and say how even the motion on screen was. Used for the stutter of 15 November 2026 (ROADMAP.md, "Stutter in play at 2560x1440").

A traced run: start the viewer with both traces going into one folder,

    HALO_FRAME_TRACE=<dir>\game_trace.txt HALO_RT_FRAME_TRACE=<dir>\viewer_trace.txt HALO_RT_GPU_LOG=1 HALO_FRAME_TIMING_LOG=1

- `game_trace.txt` (the game, `frame_trace` in opence/port/linux/src/d3d8_gl.c): a `G` line a frame, with the frame's index and tick, when it was shared and when it ended (the performance counter, microseconds), the swap, the pace wait and, last, `late <n>`: which read back the overlay shared (0 this frame's, 1 the frame before's, 9 the picture last shared again, -1 an empty overlay or the synchronous read back).
- `viewer_trace.txt` (src/overlay.rs `trace_frame`): a `V` line a frame, the time of the look, the game frame shown, its tick and the camera.
- `viewer_trace.txt.cpu` (src/cpu_times.rs): `M` a main frame and `R` a render frame, when each ended and how long it took, the wait for the window apart.
- `viewer_trace.txt.gpu` (src/main.rs `trace_gpu_times`): `P` the GPU passes' times a frame.

A scripted run (`--screenshot`) has no keyboard focus and draws as an unfocused window does; `HALO_RT_TEST_FOCUS=1` makes it behave as in play. `HALO_BEVY_OVERLAY_ALWAYS=1` reads the overlay back on every frame, so the read back's cost shows on a spot whose overlay is empty.

The scripts take the folder:

- `stut.py <dir> [seconds]`: the viewer's frame interval, how many game frames each look stepped (0 is a repeat), how evenly the game time shown follows the viewer's time (the residual), the age of the frame shown, the game's own intervals.
- `waits.py <dir> [skip_tail_s] [span_s]`: the render frames' intervals and the hitches (intervals over 1.7 times the median) a minute.
- `slow.py <dir>`: the game's work per frame and the frames over 6 ms.
- `late.py <dir>`: the shares of the `late` values.
- `rep.py <dir> [n]`: where the viewer repeated a game frame or stepped three, with the timeline round the first n.
- `gpuh.py <dir>`: the GPU passes' sum and the frames far over its median.
- `vehicle.py <dir> [from tick]`: the vehicle the player rides against the chase camera, frame by frame (the Warthog seen to jump ahead for a few frames, 19 November 2026): where the vehicle's root sits along the camera's forward, the frames where that leaves a running median of its neighbours, whether the game drew the same step, and, from the interpolation trace, how often the camera's obstruction check popped (`cut reversals`: the check's own and what the camera was placed at).
- `press_replay.py <dir> [from tick]`: a traced drive's obstruction check run again through the camera's release ease (`observer_ease_press` in opence/source/camera/observer.c, `HALO_CAMERA_PRESS_EASE`) as built and under other settings (a longer hold, a hold begun again by a jump, a release that gathers speed): the pops each leaves and how far each holds the camera in under the check. The frame's time is taken from the tick and its fraction, so the built rule's replay is near the recorded distance, not equal to it.

For `vehicle.py` a third trace beside the two, the game's `HALO_INTERPOLATION_TRACE=<dir>\interp_trace.txt` (`bevy_bridge_interpolation_trace` in opence/port/linux/game/bevy_bridge.c): an `I` line a frame with the camera's blend (the branch, the observer's position, the camera drawn), the ridden vehicle's (blended or snapped, the latest tick's root, the root drawn) and the observer's focus distance as placed, its own, its timers and what the obstruction check itself allowed. The file is opened at the path as given (builds before 20 November 2026 took it for an Xbox path: `c:\x.txt` landed in `<the saves' folder>\c\x.txt`). A driver for a scripted run: `HALO_TEST_INPUT=drive:<seed>` (forward nine seconds of every twelve, back the rest, the look weaving, the handbrake now and then; opence/port/linux/src/xinput_sdl.c), the player seated by script, e.g. on b30 `(begin (sleep -1 mission_beach_lz) (object_create_anew jeep) (object_teleport jeep beach_lz_banshee_flag) (unit_enter_vehicle (unit (list_get (players) 0)) jeep "w-driver"))` (without the `sleep` the mission's script takes the jeep for the Pelican's delivery at a tick that differs run to run, and the trace from then on is of a camera under the island).
