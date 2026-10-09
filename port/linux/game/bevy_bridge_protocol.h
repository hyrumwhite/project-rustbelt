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
*/

#ifndef __BEVY_BRIDGE_PROTOCOL_H
#define __BEVY_BRIDGE_PROTOCOL_H
#pragma once

#define BEVY_BRIDGE_MAPPING_NAME "Local\\HaloBevyBridge"
#define BEVY_BRIDGE_POSIX_NAME "/halo_bevy_bridge"
#define BEVY_BRIDGE_MAGIC 0x42524248UL /* "HBRB" */
#define BEVY_BRIDGE_VERSION 3
#define BEVY_BRIDGE_MAXIMUM_OBJECTS 4096
#define BEVY_BRIDGE_MAXIMUM_NODES 49152
#define BEVY_BRIDGE_FRAMES 2

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
	/* radians */
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
	unsigned long reserved[11];
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
	struct bevy_bridge_frame frames[BEVY_BRIDGE_FRAMES];
};

/* ---------- prototypes/the platform layer (posix_bevy_bridge.c, win32_bevy_bridge.c) */

/* the shared memory, created at `size` bytes and zeroed, or NULL */
void *bevy_bridge_platform_map(unsigned long size);
/* the memory bevy_bridge_platform_map made, or NULL before then */
void *bevy_bridge_platform_shared(void);
unsigned long bevy_bridge_platform_process_id(void);

#endif /* __BEVY_BRIDGE_PROTOCOL_H */
