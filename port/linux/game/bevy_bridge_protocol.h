/*
BEVY_BRIDGE_PROTOCOL.H

The memory the game shares with a Bevy renderer (bevy-rt-test's halo_rt):
each frame, the camera and every object's node matrices, so the renderer
can draw the game's world with ray tracing. The game writes; the renderer
only reads. See bevy_bridge.c.

The layout is fixed-size types only, the same for this 32-bit game and a
64-bit reader; bevy-rt-test mirrors it (src/bridge.rs) and checks
BEVY_BRIDGE_VERSION and the size.

Two frame buffers: the game fills the one `latest_frame` does not name and
then names it. Each frame is also a sequence lock: `sequence` is odd while
the frame is written, so a reader copies a frame, checks that the sequence
is even and unchanged, and otherwise reads again.

Positions and matrices are the game's: world units, Z up.

The other way, the renderer may forward its window's keyboard and mouse
(`input`, which only the renderer writes): the platform layer turns its
changes into the SDL events the game reads (bevy_bridge_input.c), so a
player can play from the renderer's window.

With game.bevy_bridge_overlay on, the game also shares its own picture
without the opaque world: effects, transparent surfaces, the HUD and menus,
drawn over a clear screen with premultiplied alpha (`overlay`), for the
renderer to draw over its own picture of the world. Its two buffers work as
the frames do.
*/

#ifndef __BEVY_BRIDGE_PROTOCOL_H
#define __BEVY_BRIDGE_PROTOCOL_H
#pragma once

/* the shared memory's name (game.bevy_bridge_name; letters, digits, '-' and
'_'), so that several games can each share with their own renderer: a file
mapping "Local\\<name>" on Windows, a POSIX shared memory object "/<name>"
elsewhere */
#define BEVY_BRIDGE_DEFAULT_NAME "HaloBevyBridge"
#define BEVY_BRIDGE_MAXIMUM_NAME_LENGTH 64
#define BEVY_BRIDGE_MAGIC 0x42524248UL /* "HBRB" */
#define BEVY_BRIDGE_VERSION 8
#define BEVY_BRIDGE_MAXIMUM_LIGHTS 64
#define BEVY_BRIDGE_MAXIMUM_OBJECTS 4096
#define BEVY_BRIDGE_MAXIMUM_NODES 49152
#define BEVY_BRIDGE_FRAMES 2
/* the largest overlay picture (pixels), and its buffers */
#define BEVY_BRIDGE_OVERLAY_MAXIMUM_PIXELS (2560 * 1440)
#define BEVY_BRIDGE_OVERLAYS 2

/* bevy_bridge_object.flags */
enum
{
	/* the game does not draw it: invisible, or the local player's unit (or
	something it holds) seen in first person */
	_bevy_bridge_object_hidden_bit = 0,
	/* the camera is this object's first-person view */
	_bevy_bridge_object_first_person_bit,
	/* not an object: a model the game draws by itself (the first-person
	weapon and hands); definition_index is the model tag's datum index */
	_bevy_bridge_object_model_bit,
};

struct bevy_bridge_camera
{
	float position[3];
	float forward[3];
	float up[3];
	/* radians: the vertical extent the picture covers (the frustum's,
	wider than the camera's own in cinematics), centred */
	float vertical_field_of_view;
};

struct bevy_bridge_object
{
	/* the object's datum index: stable while the object lives */
	unsigned long object_index;
	/* its definition tag's datum index (in the map's tag index) */
	unsigned long definition_index;
	unsigned long first_node;
	unsigned short node_count;
	short type;
	unsigned long flags;
	/* the permutation shown of each model region */
	unsigned char region_permutations[8];
	/* the object's change colours (A-D: team and armour colours), which
	model shaders tint their base maps with where their multipurpose map's
	alpha says; white for a model */
	float change_colors[4][3];
};

/* real_matrix4x3: a node's world transform */
struct bevy_bridge_node
{
	float scale;
	float forward[3];
	float left[3];
	float up[3];
	float position[3];
};

/* a dynamic light (a muzzle flash, plasma, an explosion, the flashlight),
as the game lit the frame before */
struct bevy_bridge_light
{
	float position[3];
	float forward[3];
	/* 0..1 a channel */
	float color[3];
	/* how far it reaches (world units) */
	float radius;
	/* a spotlight's cutoff angle (radians); pi or more shines every way */
	float cutoff_angle;
};

struct bevy_bridge_frame
{
	unsigned long sequence;
	unsigned long frame_index;
	unsigned long game_tick;
	long structure_bsp_index;
	/* the scenario tag's name (levels\b30\b30): its map is the last part */
	char scenario_name[64];
	unsigned long camera_valid;
	struct bevy_bridge_camera camera;
	unsigned long light_count;
	struct bevy_bridge_light lights[BEVY_BRIDGE_MAXIMUM_LIGHTS];
	unsigned long object_count;
	unsigned long node_count;
	struct bevy_bridge_object objects[BEVY_BRIDGE_MAXIMUM_OBJECTS];
	struct bevy_bridge_node nodes[BEVY_BRIDGE_MAXIMUM_NODES];
};

/* what the renderer's window receives, for the game to read as its own */
struct bevy_bridge_input
{
	/* nonzero while the renderer forwards input (its window has the
	focus and the player is playing, not flying its free camera) */
	unsigned long active;
	/* the keys held, a bit per SDL scancode (USB HID usage) */
	unsigned long keys[16];
	/* cumulative relative mouse motion (pixels) and wheel steps: the game
	acts on their change */
	long mouse_x;
	long mouse_y;
	long wheel;
	/* the mouse buttons held, a bit per SDL button number */
	unsigned long mouse_buttons;
	/* nonzero while the renderer's camera is its own (flying free), not
	the game's: the first player hears from it (the game's coordinates) */
	unsigned long listener_valid;
	float listener_position[3];
	float listener_forward[3];
	float listener_up[3];
	unsigned long reserved[1];
};

/* an overlay picture: width x height pixels, RGBA, 8 bits a channel, the
top row first; the colours premultiplied by alpha (what covers the world)
and in the display's encoding (sRGB) */
struct bevy_bridge_overlay
{
	unsigned long sequence;
	/* the bevy_bridge_frame.frame_index of the frame it shows */
	unsigned long frame_index;
	unsigned long width;
	unsigned long height;
	unsigned long reserved[4];
};

/* a gamepad the renderer forwards */
struct bevy_bridge_gamepad
{
	/* the buttons held, a bit per SDL_GamepadButton */
	unsigned long buttons;
	/* SDL_GamepadAxis order and ranges: left x, left y, right x, right y
	(-32768..32767, y down), left trigger, right trigger (0..32767) */
	short axes[6];
	/* the game's rumble for it (0..65535: the low and high frequency
	motors), which only the game writes */
	unsigned short rumble[2];
};

#define BEVY_BRIDGE_GAMEPADS 4

/* the renderer's gamepads: while `active`, the game takes these instead of
the ones it finds itself (whose input a hidden window would not get), the
first sharing port 0 with the keyboard as a gamepad of its own would */
struct bevy_bridge_gamepads
{
	unsigned long active;
	unsigned long count;
	struct bevy_bridge_gamepad pads[BEVY_BRIDGE_GAMEPADS];
};

struct bevy_bridge_shared
{
	unsigned long magic;
	unsigned long version;
	/* sizeof(struct bevy_bridge_shared) */
	unsigned long size;
	/* the frame buffer last completed */
	unsigned long latest_frame;
	unsigned long writer_process_id;
	unsigned long reserved[3];
	struct bevy_bridge_input input;
	struct bevy_bridge_gamepads gamepads;
	struct bevy_bridge_frame frames[BEVY_BRIDGE_FRAMES];
	/* nonzero when the game shares overlays (game.bevy_bridge_overlay) */
	unsigned long overlay_enabled;
	/* the overlay buffer last completed */
	unsigned long latest_overlay;
	unsigned long overlay_reserved[2];
	struct bevy_bridge_overlay overlays[BEVY_BRIDGE_OVERLAYS];
	unsigned char overlay_pixels[BEVY_BRIDGE_OVERLAYS][BEVY_BRIDGE_OVERLAY_MAXIMUM_PIXELS * 4];
};

/* ---------- prototypes/the platform layer (posix_bevy_bridge.c, win32_bevy_bridge.c) */

/* the shared memory named `name` (BEVY_BRIDGE_DEFAULT_NAME), created at
`size` bytes and zeroed, or NULL */
void *bevy_bridge_platform_map(char const *name, unsigned long size);
/* the memory bevy_bridge_platform_map made, or NULL before then */
void *bevy_bridge_platform_shared(void);
unsigned long bevy_bridge_platform_process_id(void);

#endif /* __BEVY_BRIDGE_PROTOCOL_H */
