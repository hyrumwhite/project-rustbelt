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
#define BEVY_BRIDGE_VERSION 19
#define BEVY_BRIDGE_BIPED_MOVING_RECORDS 1024
#define BEVY_BRIDGE_FEATURE_OBJECTS 16384
#define BEVY_BRIDGE_FEATURE_MATRICES 65536
#define BEVY_BRIDGE_FEATURE_OBJECT_MAXIMUM_NODES 128
#define BEVY_BRIDGE_BIPED_RECORDS 1024
#define BEVY_BRIDGE_PROJECTILE_RECORDS 1024
#define BEVY_BRIDGE_RANDOM_DIRECTIONS 4096
#define BEVY_BRIDGE_RAY_TESTS 16
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
	/* how camouflaged it is (0..1): a unit's active camouflage, which what
	it holds shares */
	float active_camouflage;
	/* the state the collision broad phase reads: the bounding sphere (as
	of the last tick), the object's flags (bit 0 invisible, 11 connected
	to the map, 25 with a collision model), its parent's object index (all
	ones for none) and its location's cluster (NONE outside the map) */
	float bounding_center[3];
	float bounding_radius;
	unsigned long object_flags;
	unsigned long parent_index;
	short location_cluster;
	short pad;
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

/* a ray the game tested against its structure's collision geometry
(collision_bsp_test_vector, front-facing surfaces), for a renderer to check
its own collision code against */
struct bevy_bridge_ray_test
{
	float point[3];
	float vector[3];
	/* nonzero if it hit; then how far along `vector` (0..1) and which
	collision surface */
	unsigned long hit;
	float t;
	long surface_index;
	/* and a sphere tested (collision_bsp_test_sphere) half its radius
	short of where it hit: what it touched, counted, and an FNV-1a checksum
	of the surface, edge, vertex and leaf indices in the order found */
	float sphere_center[3];
	float sphere_radius;
	unsigned short sphere_counts[4];
	unsigned long sphere_checksum;
	/* and a sphere swept from the ray's start (collision_bsp_test_pill,
	as collision_test_pill calls it): along `pill_vector`, and where it
	first touched */
	float pill_vector[3];
	float pill_radius;
	unsigned long pill_hit;
	float pill_t;
	float pill_normal[3];
	long pill_surface_index;
	/* and the game's whole ray test (collision_test_vector: the structure
	and every object type, front-facing surfaces): what it hit (object_index
	all ones for the structure or nothing, surface_index NONE for nothing),
	the collision node, bsp and surface, and how far along `vector` */
	unsigned long object_index;
	short object_node_index;
	short object_bsp_index;
	float object_t;
	long object_surface_index;
	/* and a point (point_physics_update, the map's first point physics with
	structure collisions) launched from the ray's start along it at 15
	world units a second, with no wind, under water every fourth, of radius
	`point_radius`: where it was after POINT_PHYSICS_STEPS steps of 1/30
	of a second, its velocity, every step's result flags or'd, and how many
	steps collided */
	float point_radius;
	unsigned long point_underwater;
	float point_position[3];
	float point_velocity[3];
	unsigned long point_result_flags;
	unsigned long point_collisions;
};

#define BEVY_BRIDGE_POINT_PHYSICS_STEPS 90

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
	/* rays fanned out from the camera (only with game.bevy_bridge_ray_tests) */
	unsigned long ray_test_count;
	struct bevy_bridge_ray_test ray_tests[BEVY_BRIDGE_RAY_TESTS];
	/* the point physics definition the ray tests' points use (its datum
	index; all ones for none) */
	unsigned long point_physics_definition;
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

/* a projectile's state, as projectile_update reads and leaves it */
struct bevy_bridge_projectile_state
{
	float position[3];
	float velocity[3];
	unsigned long projectile_flags;
	unsigned long object_flags;
	short action;
	short hit_material_type;
	long ignore_object_index;
	long target_object_index;
	long parent_object_index;
	float detonation_timer;
	float detonation_timer_delta;
	float arming_time;
	float arming_time_delta;
	float odometer;
	float deceleration_timer;
	float deceleration_timer_delta;
	float deceleration;
	float maximum_damage_distance;
	long location_leaf;
	short location_cluster;
	short pad;
};

/* one projectile_update (with game.bevy_bridge_ray_tests): the state
before and after (before it detonates or disappears), the global random
seed as the update began and as each collision response began, and
whether a collision touched an object */
struct bevy_bridge_projectile_record
{
	unsigned long sequence;
	unsigned long projectile_index;
	unsigned long definition_index;
	long structure_bsp_index;
	unsigned long random_seed;
	unsigned long collision_count;
	unsigned long collision_seeds[4];
	unsigned long touched_object;
	struct bevy_bridge_projectile_state before;
	struct bevy_bridge_projectile_state after;
};

/* bipeds.c's struct biped_physics, as it is laid out (0xCC bytes) */
struct bevy_bridge_biped_physics
{
	unsigned char bytes[0xCC];
};

/* an object object_get_features_in_sphere visited, as it was then: during
the call `call` (counted from 0) of collision_get_features_in_sphere in the
biped update recording it. A biped's pill is biped_get_physics_pill's; an
object's node matrices (node_count of them from first_matrix, in the
feature matrix ring) and region permutations are its collision model's;
`elevator` is whether it is a machine that is an elevator a biped standing
on it rides */
struct bevy_bridge_feature_object
{
	unsigned long sequence;
	unsigned long object_index;
	unsigned long definition_index;
	unsigned long call;
	short type;
	short dead;
	unsigned long flags;
	float bounding_center[3];
	float bounding_radius;
	long parent_object_index;
	float translational_velocity[3];
	unsigned long elevator;
	unsigned long biped_passes_through;
	long player_index;
	long parent_seat_index;
	float pill_base[3];
	float pill_height;
	float pill_width;
	unsigned char region_permutations[8];
	unsigned long node_count;
	unsigned long first_matrix;
};

/* one biped_update_physics (with game.bevy_bridge_ray_tests): the biped's
physics before and after, which breakable surfaces stood (bit i of byte
i / 8), and what the crouching player's stand test reads of the biped's
definition (stand_test: whether it is made at all) */
struct bevy_bridge_biped_record
{
	unsigned long sequence;
	unsigned long biped_index;
	long structure_bsp_index;
	unsigned long stand_test;
	float stand_height;
	float stand_radius;
	unsigned char breakable_surfaces[32];
	/* the objects collision_get_features_in_sphere visited during the update
	(feature_object_count of them from first_feature_object, in the feature
	object ring), and how many times it was called */
	unsigned long first_feature_object;
	unsigned long feature_object_count;
	unsigned long feature_calls;
	struct bevy_bridge_biped_physics before;
	struct bevy_bridge_biped_physics after;
};

/* what biped_update_moving changes of a biped (and of the animation update
it is given) */
struct bevy_bridge_biped_moving_state
{
	float position[3];
	float forward[3];
	float up[3];
	float translational_velocity[3];
	float angular_velocity[3];
	unsigned long object_flags;
	long location_leaf;
	short location_cluster;
	short landing;
	unsigned long biped_flags;
	char landing_recovery_counter;
	char landing_recovery_time;
	char elevator_ticks;
	char bump_ticks;
	char jump_recovery_timer;
	char airborne_ticks;
	char animation_state_desired;
	char animation_crouching;
	long elevator_object_index;
	long support_surface_index;
	long pathfinding_surface_index;
	float pathfinding_point[3];
	long bump_object_index;
	float crouch;
	float bank;
	float ground_plane[4];
};

/* one biped_update_moving (with game.bevy_bridge_ray_tests): the biped's
movement state before and after, what it reads of the biped, unit and
animation, what it asks of the rest of the game (the aiming vector, the
difficulty's infection forms value, the player's speed multiplier, whether
a cinematic plays, players are double speed, co-op players collide, its
actor leaps, a recorded animation controls it), what its actor's jump aim
answered (if asked), whether it aborted the unit's animation, and the biped
physics record it made (NONE for none) */
struct bevy_bridge_biped_moving_record
{
	unsigned long sequence;
	unsigned long biped_index;
	unsigned long definition_index;
	long structure_bsp_index;
	long physics_record;
	unsigned long dead;
	unsigned long ignore_translation;
	unsigned long suspended;
	unsigned long control_flags;
	char unit_animation_state;
	char base_seat_index;
	char aiming_speed;
	char actor;
	long player_index;
	float throttle[3];
	float body_stun;
	float recent_body_damage;
	float desired_facing_vector[3];
	float aiming_vector[3];
	float bounding_center[3];
	long animation_graph_index;
	short animation_index;
	short animation_frame_index;
	float difficulty_infection_forms;
	float speed_multiplier;
	unsigned long cinematic;
	unsigned long double_speed;
	unsigned long coop_player_collisions;
	unsigned long actor_leaping;
	unsigned long recorded_animation;
	unsigned long actor_jump_asked;
	unsigned long actor_jumped;
	float actor_jump_velocity[3];
	unsigned long aborted_animation;
	struct bevy_bridge_biped_moving_state before;
	struct bevy_bridge_biped_moving_state after;
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
	/* projectile updates (a ring: the record `count` goes in place
	count % BEVY_BRIDGE_PROJECTILE_RECORDS), and the random direction table
	random_vector_in_cone3d draws from */
	unsigned long projectile_record_count;
	unsigned long random_direction_count;
	struct bevy_bridge_projectile_record projectile_records[BEVY_BRIDGE_PROJECTILE_RECORDS];
	float random_directions[BEVY_BRIDGE_RANDOM_DIRECTIONS][3];
	/* biped physics updates (a ring, as the projectile records) */
	unsigned long biped_record_count;
	unsigned long biped_pad;
	struct bevy_bridge_biped_record biped_records[BEVY_BRIDGE_BIPED_RECORDS];
	/* the objects biped updates' collision features came from (a ring: the
	object `count` goes in place count % BEVY_BRIDGE_FEATURE_OBJECTS), and
	their node matrices (a ring of scale, forward, left, up, position) */
	unsigned long feature_object_count;
	unsigned long feature_matrix_count;
	struct bevy_bridge_feature_object feature_objects[BEVY_BRIDGE_FEATURE_OBJECTS];
	float feature_matrices[BEVY_BRIDGE_FEATURE_MATRICES][13];
	/* biped movement updates (a ring, as the projectile records) */
	unsigned long biped_moving_record_count;
	unsigned long biped_moving_pad;
	struct bevy_bridge_biped_moving_record biped_moving_records[BEVY_BRIDGE_BIPED_MOVING_RECORDS];
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
