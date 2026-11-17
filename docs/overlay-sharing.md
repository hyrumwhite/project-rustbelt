# The overlay without the round trip

Planned and built 16 November 2026 (the user, after the first headroom round: "go ahead and plan out the overlay round trip fix and implement it with subagents"). docs/gpu-headroom.md, "What is left", has the measurement this started from. Protocol 56; both sides must be rebuilt.

## What it cost

With `--opence` the hidden game draws everything but the opaque world into its back buffer (the overlay: grass, particles, decals, water, the menus) and the viewer lays that picture over the ray-traced one. The picture crossed between the two processes through system memory:

1. the game read its render target back into a pixel pack buffer (`glReadPixels`, 14.7 MB at 2560x1440) and waited up to 4 ms for it (`overlay_share_async`, opence/port/linux/src/d3d8_gl.c);
2. mapped the buffer and copied it into the bridge's shared memory;
3. the viewer copied it out of the shared memory (`Bridge::read_overlay`, crates/halo_bridge);
4. and uploaded it to a texture (`upload_overlay`, src/overlay.rs: `write_texture`, a staging copy and a GPU copy).

Two transfers over the bus and three copies in memory a frame, for a picture that started on the same GPU it ends on. Measured in b40's corridor with the read back forced (`HALO_BEVY_OVERLAY_ALWAYS=1`), 2560x1440 at DLSS Quality: the frame 15.13 ms against 14.48, 0.65 ms, wherever anything is in the overlay; the game's own GPU time 1.50 ms a frame against 0.04. On a30's hillside (grass) the game's read back was 1.6 ms a frame and is what the stutter work of 15 November had to tame.

## What it is now: textures both sides hold

The viewer makes the overlay's textures in Vulkan with exportable memory, the game imports them into GL, copies its finished back buffer into one on the GPU and tells the viewer which. Nothing of the picture goes through system memory. (Nothing of it goes from the viewer to the game either: the viewer hands over empty memory, because Vulkan can export an allocation and GL can only import one. The pixels still flow from the game to the viewer.)

### The pieces

- **The viewer makes the slots** (src/overlay/shared.rs, Windows alone). `OVERLAY_SHARED_SLOTS` (8) Vulkan images of the overlay's size, `R8G8B8A8_SRGB`, 2D, one mip, optimal tiling, each with its own dedicated exportable allocation (`VkExternalMemoryImageCreateInfo`, `VkExportMemoryAllocateInfo`, `VkMemoryDedicatedAllocateInfo`, handle type `OPAQUE_WIN32`). wgpu-hal already enables `VK_KHR_external_memory_win32` when the driver has it; `capability` asks the driver whether such an image can be exported before anything is made. Each image is wrapped as a wgpu texture (`texture_from_raw`, `create_texture_from_hal`) and put into the render world as a `GpuImage` under an image id of its own, so the UI material samples it like any other picture. They are made in the render world after `prepare_windows` (`make_shared_textures`), since that is where the device's raw handle is at hand.
- **The handles cross by duplication.** The viewer gets each allocation's NT handle (`vkGetMemoryWin32HandleKHR`), duplicates it into the game's process (`OpenProcess(PROCESS_DUP_HANDLE)`, `DuplicateHandle`) and closes its own. It offers the set in the shared header (`struct bevy_bridge_overlay_shared`, at the end of the shared block: a serial written last, the game's process id, the size, the allocation's size, the handle type and the eight handle values as the game will see them).
- **The game imports them** (`overlay_slots_import`, d3d8_gl.c; `GL_EXT_memory_object` and `GL_EXT_memory_object_win32`: `glCreateMemoryObjectsEXT`, `glImportMemoryWin32HandleEXT`, `glTextureStorageMem2DEXT` with `GL_SRGB8_ALPHA8`; the functions are loaded as optional ones, `GL_OPTIONAL_FUNCTIONS` in gl.h), closes the duplicated handles and answers in the header: imported, or refused. A set offered to another process id is not touched.
- **A frame** (`overlay_slots_share`). Where the game read its target back, it copies it into a free slot on the GPU (`glCopyImageSubData` from the resolved back buffer, texel for texel, no colour conversion, the top row first as before), puts a fence after it and waits for the fence by the read back's rule: this frame's picture within `HALO_BEVY_OVERLAY_WAIT` (4 ms), else the newest older one that is done, else the one last named again. Then it names the slot in the overlay's header (`shared_serial`, `shared_slot`) in place of pixels.
- **The viewer shows the slot.** One UI node and material a slot beside the pixel picture's, only the named one visible (`Shown`). No copy, no upload.
- **Which slot is free.** The viewer writes a mask of the slots it may still be drawing from: the one it shows and those of its last frames (`HALO_RT_OVERLAY_SHARED_HOLD`, 4 by default, 1 to 6), for the frames still in flight on its side. `Bridge::read_overlay` adds the slot it is about to show to the mask and then looks at the header again, and the game reads the mask after it has begun its overlay, so one of the two always sees the other. The game never writes into a slot in the mask, nor into the last two it named. Eight slots leave at least one free at the default hold.
- **Sizes.** The slots follow the overlay's size: on a change (the game's Video Setup, a window resize in follow mode) the viewer makes a new set under a new serial, the game imports it, and the frames between go as pixels (about 23 of them). The old set is kept for 12 frames and then freed. A size the game refused is not offered again.

### What stays

- The old path, whole: pack buffers, shared memory, upload. It is the fallback whenever sharing is not there (no extension, an import that fails, another driver, Linux and Android, a size change in flight, no free slot), and behind the switches below.
- An empty overlay is still shared as empty and costs nothing.
- The frame the picture belongs to, the pacing by the viewer's looks, the realtime GPU class, the frame traces' fields.

### Switches and log lines

| switch | what it does |
|---|---|
| `HALO_RT_OVERLAY_SHARED=0` | the viewer makes no slots: the pixels as before |
| `HALO_BEVY_OVERLAY_SHARED=0` | the game refuses every set (and closes its handles): the pixels as before |
| `HALO_RT_OVERLAY_SHARED_HOLD=<1..6>` | how many of its last frames' slots the viewer holds (4) |
| `HALO_BEVY_OVERLAY_SHARED_CHECK=1` | the tear detector: before each copy the game fills with magenta every slot the rule says the viewer is not drawing from. A picture drawn from one of them shows magenta |
| `HALO_BEVY_OVERLAY_SHARED_CHECK=2` | the detector made to fail: the slots held and the last two named are filled too |
| `HALO_BEVY_OVERLAY_SHARED_CHECK=3` | no copy at all (a slot is named with whatever it held): what the copy costs |
| `HALO_BEVY_OVERLAY_SHARED_DRAW=1` | the game draws each frame straight into a slot, no copy (17 November, the last section; not the default) |
| `HALO_RT_OVERLAY_DEBUG=3` | the overlay alone on screen, opaque: its bytes as shared, for comparing the two paths |
| `HALO_BEVY_DRAW_LOG=1` | the game's draw log, which now ends with the overlays shared by texture and by pixels of each 300 frames |

The viewer logs `overlay: shared textures, 8 slots of WxH, serial S` and then `overlay: the game imported the shared textures, serial S`, or once `overlay: the game refused the shared textures ...; the pixels as before` or `overlay: shared textures unavailable: <reason>; the pixels as before`. The game logs `overlay: shared textures imported, 8 slots of WxH, serial S (opaque handles, dedicated)` or the refusal with its reason and GL error.

## What was found on the way

- **Opaque handles work between a 64-bit Vulkan process and a 32-bit GL one** on this machine (RTX 5080). The plan's second route (a D3D11 texture as the shared object) was not needed and is not built; the header has the handle type for it.
- **A display change could end the viewer** with `Surface::configure: Failed to wait for GPU to come idle` once sharing was on (2 of 5 corridor runs, 3 of 4 on a30; none of 4 with sharing off). The cause was older than this work: DLSS makes its feature again at the new size and submits work in a system that ran in no fixed order against `prepare_windows`, which configures the surface and wants the GPU idle. `prepare_dlss` now runs after `prepare_windows` (bevy_solarik/vendor/bevy_anti_alias/src/dlss/mod.rs); none of ten display changes failed after.

## Measured

Everything at the user's setting, borderless 2560x1440 with DLSS Quality, scripted (the stutter work's runs and its traces, scripts/pace), old against new by `HALO_RT_OVERLAY_SHARED=0` on one build. A single run's frame wanders 0.1 to 0.3 ms, so every figure is from repeats.

### The frame

The viewer's render interval, median of a run, in ms:

| spot | pixels (the old) | shared | saved | empty overlay |
|---|---|---|---|---|
| b40's corridor, overlay forced (`HALO_BEVY_OVERLAY_ALWAYS=1`) | 15.08, 15.13, 15.19 | 14.82, 14.81, 14.68, 14.74 | 0.37 | 14.44, 14.43, 14.30 |
| a30's hillside (grass in the overlay) | 14.96, 14.59, 14.65 | 14.58, 14.26, 14.35, 14.29 | 0.30 to 0.38 by pairs | |

The corridor with the game's own switch (`HALO_BEVY_OVERLAY_SHARED=0`) reads 15.04, as the viewer's. About 66.1 frames a second to 67.7 in the corridor and 67.9 to 69.6 on the hillside.

So sharing takes half of what an overlay with something in it cost: 0.37 ms of 0.74, and a frame with a picture in the overlay is still 0.37 ms dearer than one without. Of that, the copy itself is about 0.16 (the corridor with `HALO_BEVY_OVERLAY_SHARED_CHECK=3`, no copy: 14.67 and 14.53; two runs, and too much by a factor of six: measured on 17 November with the copy really gone it is 0.025 ms of the game's GPU time and nothing of the frame, the last section) and the rest is not taken apart: the game's fence and flush each frame and the viewer laying a full picture over its own.

### The game's side

| | pixels | shared | empty overlay |
|---|---|---|---|
| the game's GPU time a frame (its own timer), corridor | 1.57 to 1.58 ms | 0.27 to 0.28 | 0.085 |
| the same, hillside | 1.51 to 1.53 | 0.25 | |
| the game's work a frame, median, corridor | 2.00 to 2.09 | 0.71 to 0.74 | 0.37 |
| sharing the picture, mean (longest), corridor | 1.54 to 1.58 (1.98 to 2.05) | 0.26 to 0.27 (0.56 to 0.73) | |
| the same, hillside | 1.57 to 1.60 (2.01 to 2.21) | 0.31 to 0.33 (0.58 to 0.68) | |

The game's draw log says "300 by shared texture, 0 by pixels" in every line of every run; no frame fell back for want of a free slot.

### Pacing

The stutter work's measures, shared against pixels: drawn frames that repeat a game frame 0.0 to 0.1% against 0.0%; the game time shown against the viewer's time, a deviation of 0.63 to 0.77 ms against 0.66 to 0.83 (0.51 to 0.52 with an empty overlay); every frame showed its own picture, none an older one; no render interval over 1.7 times the median in any traced run. One measure moved: the viewer steps two game frames on 8.4 to 11.4% of its own, against 0.0 to 0.6% with the pixels and 16.1 to 18.1% with an empty overlay. The game's frame was 2 ms with the read back and is 0.7 now, so it runs ahead of the viewer as it always has where the overlay is empty.

### The picture is the same to the bit

a30's hillside with the game frozen (`(game_speed 0)` by script), the overlay alone on screen (`HALO_RT_OVERLAY_DEBUG=3`) and the camera the script's, so that neither the HUD nor the first-person weapon is in it (`(camera_control 1)`): of the picture's pixels outside the viewer's own text, none of 4,752,000 differs between a shared run and a pixels run, nor between two shared runs (212,789 of them not black, the grass).

With the weapon in the picture the same holds but along the weapon's outline, where it hides the grass: 223 to 254 pixels differ between two runs of one path and 1046 to 1300 between the paths, a line one to three pixels wide. The weapon's idle pose is frozen where the frames before left it, and the game's frame rate is not the same on the two paths. That is the game's state, not the sharing, as the weaponless runs show.

### No slot is written while the viewer draws from it

`HALO_BEVY_OVERLAY_SHARED_CHECK=1` fills with magenta, before each copy, every slot the rule says the viewer is not drawing from. A picture the viewer drew from one of those would show magenta. On the hillside with the player turning: none of 40 captures taken 50 frames apart, and none of the 41 of an earlier, weaker form of the check. `=2` breaks the rule on purpose (the held slots and the last two named are filled too), to see the check fail: 40 of 40 captures magenta from edge to edge.

### A change of size

In follow mode with the display changed three times in a run (the overlay 2560x1440, then 1600x900, 1920x1080 and 1280x720): four sets, each made, imported and used, in both logs; no error.

### Two things tried beside it

- **`HALO_BEVY_HIDDEN_PRESENT=2`**, the first round's flush in place of the hidden window's swap, which had hitched in one run of four then. With the shared overlay: the corridor 14.24 and 14.29 ms against 14.76 (13.92 against 14.39 with an empty overlay), the hillside 14.13, 14.06, 14.05, 14.27, 14.27 and 14.02 against 14.37: 0.25 to 0.5 ms, more than the sharing itself saves, on any frame, and it steps two game frames on 4.3 to 5.7% of drawn frames in place of 8 to 11%. None of the nine runs hitched. It was left opt-in on 16 November, the one bad run never having been explained, and became the default on 17 November after the user's play with it (the last section).
- **`HALO_BEVY_OVERLAY_WAIT=0`** (never wait for the frame's own copy): 14.51 ms in the corridor, and every picture a game frame old. Not taken.

## What is left

- **The other half.** A frame with something in its overlay still costs 0.37 ms more than one with nothing. The copy is not it: the game drawing straight into the slot was built and measured on 17 November (the last section) and the frame is no shorter for it. What a picture in the overlay costs is the fence, the flush and the viewer's own blend of a full picture, 0.22 ms a frame in b40's corridor since the flush became the default (14.10 ms against 13.88 with the overlay empty).
- **Semaphores and a layout hand-off** (`GL_EXT_semaphore`, a Vulkan semaphore each way), if another driver ever needs them.
- **Linux**, by file descriptors (`GL_EXT_memory_object_fd`): the header's handle type has room for it.

## Limits

- **Proven on one driver.** NVIDIA on Windows, a 64-bit viewer and a 32-bit game. Another vendor's driver may refuse the import (the pixels then, with one log line) or, worse, accept it and misbehave: `HALO_RT_OVERLAY_SHARED=0` is the way back.
- **No semaphores and no layout hand-off between the two APIs.** The fence says the game's copy is done before a slot is named and the mask keeps the game off what the viewer may read; nothing tells Vulkan that GL wrote the image. wgpu moves each image from an undefined layout to a read-only one at its first use, which the specification lets a driver treat as throwing the contents away (this one does not), and GL writes while Vulkan holds the layout read-only. It works here; it is the first thing to suspect if a shared overlay ever shows garbage or stale pictures on another machine.
- **The game runs a little ahead of the viewer** when its frame is this cheap: the viewer steps two game frames on 8.4 to 11.4% of its own (0.0 to 0.6% with the pixels, 16 to 18% with an empty overlay, which is how most of play has always been). The evenness of motion measures the same.
- **Windows alone.** The Linux and Android builds of the game keep the pixels; they were not compiled for this change.
- A fixed-size run's `HALO_RT_TEST_RESIZE` changes no overlay size (the game stays at 1280x720), so size changes were checked in follow mode.

## 17 November: the flush for the hidden swap, the default

The user played the build above with `HALO_BEVY_HIDDEN_PRESENT=2` set by hand on 17 November (a30 among the levels; the session's logs show the shared textures made and imported at 2560x1440 and no error; the F9 capture's note 69 drawn frames a second against 70 of the game's): "ok, performane was great, it felt smooth, lets keep HALO_BEVY_HIDDEN_PRESENT=2 and I will keep an eye out for hitching in future playthroughs". So the hidden game no longer swaps the window nobody sees: it flushes and keeps the frame's pacing (`hidden_present` in opence/port/linux/src/d3d8_gl.c). `HALO_BEVY_HIDDEN_PRESENT=1` is the swap, the default until now, and `=0` the clear, the blit and the swap of before the first headroom round.

On the rebuilt game, with nothing set, against `HALO_BEVY_HIDDEN_PRESENT=1` (2560x1440 Quality, scripted, one run each):

| spot | swap (`=1`) | flush (the default) |
|---|---|---|
| a30's hillside | 14.47 ms | 14.02 |
| b40's corridor, overlay forced | 14.76 (yesterday's four) | 14.12 |

Repeats 0.0% on the hillside and 0.2% in the corridor, a deviation of 0.61 and 0.85 ms, the viewer stepping two game frames on 5.6% of its own (9.2% with the swap), no render interval over 1.7 times the median in any of the three runs.

One thing the flush does that the swap does not, seen in these runs and, looked for, in yesterday's with the switch too: now and then a frame's wait for its copy's fence runs 3.4 to 4.0 ms, where the swap's longest is under 1.0. It is the longest of about a third of the game's five-second timing windows; by the game's trace 12 to 14 frames of a run (of 13,400 on the hillside, of 7,900 in the corridor) spend over 3 ms from the frame's start to its picture shared, against 3 to 5 with the swap. Each still shares its own picture (the wait's bound is 4 ms) and none of the pacing measures shows it. If hitching is ever met in play, this is the first place to look: `HALO_BEVY_HIDDEN_PRESENT=1` is the way back, and the game's `frame timing:` line gives the share step's longest.

The one run of the first round that hitched six times a second with the flush has not come back in eleven scripted runs and one play session since. It is still unexplained.

## 17 November: the frame drawn straight into the slot, tried and left opt-in

The user, after the flush became the default: "Ok, next let's try your "draw straight to the shared texture" suggestion". It was the first thing under "What is left": the game copies its finished back buffer into a slot every frame (`glCopyImageSubData`, 14.7 MB at 2560x1440), and the copy would go if the frame were drawn into the slot to begin with. It was built, it gives the same picture to the bit, and it gains nothing that can be measured on the frame. It stays behind `HALO_BEVY_OVERLAY_SHARED_DRAW=1`; the copy is still the default.

### What was built

All of it in the game (opence/port/linux/src/d3d8_gl.c, Windows alone, `glTextureView` added to the optional functions in gl.h); no change to the protocol or the viewer.

- **A view of each slot.** The game's back buffer is `GL_RGBA8` and the slots are `GL_SRGB8_ALPHA8` (the copy is a copy of bytes, so the difference never mattered). At import each slot gets a `GL_RGBA8` view of its texture (`overlay_slots_make_views`), so that drawing, blending, reading and sampling see the format they always did. The driver made the views without complaint; had it not, one log line says why and the copy stays.
- **The redirect.** At the end of each present, after the share, the flush and the pace, the game chooses the slot for the next frame by the copy's own rule (not held by the viewer, not one of the last two named; one with no picture waiting first, else the oldest waiting, whose fence is deleted so that it is never named) and makes the back buffer's texture that slot's view (`overlay_slots_redirect`). The framebuffers are found by texture, so everything that draws into or reads the back buffer follows. At the frame's end there is no choice and no copy, only the fence, the flush, the wait and the naming as before (`overlay_slots_share`). A frame shared empty keeps its slot for the next.
- **What a frame does not clear.** With one texture of its own the back buffer kept whatever a frame left alone; with another slot each frame it would show a picture several frames old there. So the first use of the back buffer in a frame decides (`back_buffer_used`): a clear of every channel over the whole target needs nothing, anything else (a draw, a sample, a partial clear, reaching the present untouched) first copies the picture of the frame before in, counted in the draw log as carried. Under the overlay the game clears the whole target as each frame begins, so no frame carried in any run here, the menus and the loading screens among them; a split screen's clear is partial and, by the code, would (not run).
- **Giving it back.** Before a set is deleted (a new size, a refusal), when the back buffer is another (the screen's size changed) and when no slot is free as a frame begins, the back buffer has its own texture back with the picture copied into it (`overlay_slots_unredirect`), and the frame goes by the copy. The framebuffers cached for a deleted view are deleted with it (`framebuffers_forget`; the cache never forgot one before, and a texture made later can be given the same name).
- **The log.** `overlay: drawn into the shared textures, serial S (rgba8 views)` once a set; `HALO_BEVY_DRAW_LOG=1` ends with "overlays 300 by shared texture (300 drawn in place, 0 carried), 0 by pixels".

### Measured

2560x1440 Quality, scripted, the two on one build by the switch, runs in turn; the render interval's median in ms.

| spot | the copy | drawn in place |
|---|---|---|
| b40's corridor, overlay forced | 14.09, 14.13, 14.10, 14.10 | 14.12, 14.15, 14.05, 14.08 |
| a30's hillside | 14.19, 14.13, 14.06, 14.10 | 14.03, 14.03, 14.08, 14.29 |
| b40's corridor, overlay empty | 13.83, 13.92 | |

The means are 14.11 against 14.10 in the corridor and 14.12 against 14.11 on the hillside: nothing, the runs of one setting lying up to 0.26 ms apart.

| the game's side | the copy | drawn in place |
|---|---|---|
| its GPU time a frame, corridor | 0.278 to 0.281 ms | 0.252 to 0.257 |
| its GPU time a frame, hillside | 0.253 to 0.259 | 0.230 to 0.233 |
| its work a frame (median), corridor | 0.71 to 0.72 | 0.68 to 0.70 |
| its work a frame (median), hillside | 0.73 to 0.74 | 0.70 to 0.72 |
| the share step, corridor | 0.26 to 0.28 | 0.25 to 0.26 |
| the share step, hillside | 0.31 to 0.33 | 0.29 to 0.30 |

So the copy is 0.025 ms of the game's GPU time a frame, the same at both spots and in every run, and that is all of it. The 0.16 ms written down for it on 16 November came from two runs with `HALO_BEVY_OVERLAY_SHARED_CHECK=3` against one, inside what two runs of one setting differ by; a GPU moves 14.7 MB in a few hundredths of a millisecond.

Pacing as it was on both: repeats 0.0 to 0.1% in place and 0.0 to 0.2% with the copy, a deviation of 0.61 to 0.77 ms and 0.64 to 0.89, the viewer stepping two game frames on 3.7 to 6.5% of its own and 4.6 to 6.3%, no render interval over 1.7 times the median in any of the sixteen runs. The flush's occasional long wait for the fence (3.1 to 3.8 ms as a five-second window's longest) is there on both.

### Checked

- **The same to the bit.** The frozen recipe above (a30, the game stopped by script, the overlay alone on screen, the weapon out of the picture): none of 4,752,000 pixels outside the viewer's own text differs between a run drawn in place and a run with the copy, nor between two runs drawn in place, in each of three captures (212,789 of them not black).
- **No slot written while the viewer draws from it.** `HALO_BEVY_OVERLAY_SHARED_CHECK=1` fills the free slots with magenta as the next frame's slot is chosen, that slot among them, so it also shows a frame that neither cleared nor carried: none in 40 captures on a30 with the player turning. `=2`, the rule broken: 40 of 40.
- **The menus** with the check on: no magenta in the captures, the frames drawn in place from the set's second on, none carried.
- **A change of size** in follow mode through 1600x900, 1920x1080 and 1280x720: a new set each time, the back buffer given back and redirected again, "drawn into the shared textures" under each of the four serials, the frames between as pixels as before, no error on either side.
- The new C checked for implicit declarations; the game alone rebuilt.

### Why it is not the default

It buys 0.025 ms of GPU work and no time on the frame, and it asks more of the driver than the copy does: the game renders the whole frame into memory that Vulkan owns, through a view, where the copy writes it once at the end. That works on this machine, as the copy does, with the same lack of a layout hand-off between the two APIs. Not exercised: the carry (no frame needed it), split screen, any anti-aliasing mode but the user's "off" (multisampling's resolve would land in the slot by itself; FXAA, SMAA and supersampling were only read), another driver. With nothing gained there is no reason to put it under every play session. `HALO_BEVY_OVERLAY_SHARED_DRAW=1` turns it on.

What this settles: the 0.22 ms a picture in the overlay still costs is not the copy. It is the fence and the flush on the game's side (its GPU time is 0.085 ms with an empty overlay and 0.255 with a picture drawn in place) and the viewer's blend of a full picture, and the next step there would be to measure those two apart, not to build more.
