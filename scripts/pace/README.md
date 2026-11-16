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
