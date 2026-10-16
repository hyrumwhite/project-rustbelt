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
#define BEVY_BRIDGE_VERSION 37
#define BEVY_BRIDGE_SPRITE_BATCHES 512
#define BEVY_BRIDGE_SPRITE_VERTICES 32768
#define BEVY_BRIDGE_DECALS 2048
#define BEVY_BRIDGE_DECAL_VERTICES 10240
#define BEVY_BRIDGE_VEHICLE_UPDATE_RECORDS 512
#define BEVY_BRIDGE_VEHICLE_PHYSICS_RECORDS 256
#define BEVY_BRIDGE_MASS_POINTS 8192
#define BEVY_BRIDGE_PHYSICS_VEHICLES 128
#define BEVY_BRIDGE_VEHICLE_RAY_TESTS 16
#define BEVY_BRIDGE_MELEE_RECORDS 512
#define BEVY_BRIDGE_POSE_RECORDS 2048
#define BEVY_BRIDGE_POSE_NODES 65536
#define BEVY_BRIDGE_UNIT_ANIMATION_RECORDS 1024
#define BEVY_BRIDGE_BIPED_UPDATE_RECORDS 1024
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

/* (protocol 32) the fog the first player's window was drawn with
(rasterizer_window_set_fog's, as it left it: off, each has no density), in
the game's coordinates. Atmospheric fog thickens with the view depth from
the minimum to the maximum distance; planar fog with the view depth over its
maximum distance and the depth under its plane (whose normal points out of
the fog) over its maximum depth (rasterizer_xbox_environment_fog.c) */
struct bevy_bridge_fog
{
	/* nonzero once written */
	unsigned long valid;
	/* fog_definition flags (bit 0 water, 1 atmosphere dominant, 2 screen
	effect only) and render_fog.planar_mode (0 off, 1 normal, 2 fully
	fogged) */
	unsigned short flags;
	short planar_mode;
	float atmospheric_color[3];
	float atmospheric_maximum_density;
	float atmospheric_minimum_distance;
	float atmospheric_maximum_distance;
	/* normal (i, j, k) and d: n.p = d on the plane */
	float plane[4];
	float planar_color[3];
	float planar_maximum_density;
	float planar_maximum_distance;
	float planar_maximum_depth;
};

/* (protocol 33) the fog screen the first player's window would have drawn
over its fog (rasterizer_xbox_environment_fog.c): up to four layers of a
bitmap's frames over the screen, moved on by the game (wind, strafing,
rotation, zoom), each a density: the dot product of the frame's colour and
the layer's weights (its animation and depth). They cover 1 - the product of
(1 - each), in the fog screen's colour; with `mode` 1 that is added to the
picture, times the surface's density: near_density at near_distance up to 1
at far_distance along the planar fog density's smoothstep (1 where nothing
is drawn); with 2, blended over the whole picture. */
struct bevy_bridge_fog_screen_layer
{
	/* the layer's texture coordinates from the screen's, x right and y up,
	-1 to 1 across the picture: u = t0 x + t1 y + t2, v = t3 x + t4 y + t5 */
	float transform[6];
	/* the frame's red, green and blue's weights */
	float weights[3];
	/* the frame: the bitmap of `map` */
	unsigned long bitmap_index;
};

struct bevy_bridge_fog_screen
{
	/* 0 none, 1 added by depth, 2 blended over */
	unsigned short mode;
	/* fog_screen flags (bit 2: no texture: its depth's density is flat) */
	unsigned short flags;
	/* the bitmap tag's datum index */
	unsigned long map;
	short layer_count;
	short pad;
	float color[3];
	/* world units, and the near density over the far */
	float near_distance;
	float far_distance;
	float near_density;
	struct bevy_bridge_fog_screen_layer layers[4];
};

/* (protocol 34) a decal the first player's window would have drawn, with
game.bevy_bridge_viewer_decals (rasterizer_xbox_decals.c): its quads (four
vertices each, from first_vertex of the frame's decal_vertices), its
bitmap, and how it blends, by its shader's framebuffer blend function
(0 alpha blend, 1 multiply, 2 double multiply, 3 add, 4 reverse subtract,
5 min, 6 max, 7 alpha multiply add) with its colour and intensity, as the
game's combiners take them: the bitmap's colour times the decal's colour,
by the intensity toward nothing (multiply's white, double multiply's
grey, add's black) */
struct bevy_bridge_decal
{
	/* its datum index: the same decal from frame to frame */
	unsigned long decal_index;
	/* the bitmap tag's datum index, and the bitmap of it */
	unsigned long map;
	unsigned char bitmap_index;
	/* 0 primary, 1 secondary, 2 light, 3 alpha tested, 4 water */
	unsigned char layer;
	unsigned char blend_function;
	/* 0..255 */
	unsigned char intensity;
	/* red, green, blue (0..255) */
	unsigned char color[4];
	unsigned long first_vertex;
	unsigned long quad_count;
};

/* (protocol 35) a batch of sprites the first player's window would have
drawn, with game.bevy_bridge_viewer_effects (render_sprite.c's
build_sprites_end: particles, particle systems, weather, glows, antennas):
quads (four vertices each, from first_vertex of the frame's
sprite_vertices) of one bitmap of `map`, with an effect shader
(shader_effect_definition). Its combiners take the bitmap's colour times
the vertex's (with nonlinear tint, (1 - v) t^4 + v t), and blend it by the
framebuffer blend function (as a decal's), faded by the vertex's alpha
toward nothing: an alpha blend's alpha is the bitmap's times it, a
multiply's colour goes toward white, a double multiply's toward grey, an
add's toward black */
struct bevy_bridge_sprite_batch
{
	unsigned long map;
	unsigned short bitmap_index;
	/* shader_effect flags (bit 0 sort bias, 1 nonlinear tint, 2 don't
	overdraw the first-person weapon) and its framebuffer blend function */
	unsigned short shader_flags;
	unsigned short blend_function;
	/* primary map flags (bit 0 point sampled, 1 u clamped, 2 v clamped) */
	unsigned short map_flags;
	/* 1 drawn with the first-person weapon */
	unsigned long first_person;
	/* the secondary map's bitmap tag (all ones for none), its anchor (0
	with the primary, 1 screen space, 2 zsprite) and flags */
	unsigned long secondary_map;
	unsigned short secondary_anchor;
	unsigned short secondary_flags;
	/* where the game sorts the batch from (the game's coordinates) */
	float centroid[3];
	unsigned long first_vertex;
	unsigned long quad_count;
	/* (protocol 36) what it is: BEVY_BRIDGE_SPRITE_EFFECT, or
	BEVY_BRIDGE_SPRITE_LENS_FLARE, a lens flare's reflection: drawn over
	everything (the game tests its occlusion itself), added by the vertex's
	alpha, its colour (1 - v) tint t^6 + v t (rasterizer_xbox_widgets.c's
	internal sprite), shader_flags and blend_function unused */
	unsigned long kind;
	float tint;
};

enum
{
	BEVY_BRIDGE_SPRITE_EFFECT = 0,
	BEVY_BRIDGE_SPRITE_LENS_FLARE = 1
};

/* a sprite's vertex: the game's coordinates, the bitmap's texture
coordinates and the colour (a8r8g8b8: alpha the fade) */
struct bevy_bridge_sprite_vertex
{
	float position[3];
	float texcoord[2];
	unsigned long color;
};

/* a decal's vertex, as the game keeps it: the game's coordinates, and the
bitmap's texture coordinates in 32768ths */
struct bevy_bridge_decal_vertex
{
	float position[3];
	short texcoord[2];
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
	/* (protocol 37) the angle its light falls off from, to none at the
	cutoff (radians) */
	float falloff_angle;
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

/* (protocol 26) a vehicle as its physics shape is placed
(physics_instance_new: object_get_origin and object_get_orientation) */
struct bevy_bridge_physics_vehicle
{
	unsigned long object_index;
	float origin[3];
	float forward[3];
	float up[3];
};

/* (protocol 26) a ray from the camera at a vehicle (through its bounding
sphere's centre, and above it), tested as collision_test_vector does with
_collision_test_use_vehicle_physics_bit (the structure and every object
type, front-facing surfaces; vehicles by their physics' mass points): the
result's type (NONE for nothing), object, t, plane, node and surface */
struct bevy_bridge_vehicle_ray_test
{
	float point[3];
	float vector[3];
	short type;
	short node_index;
	unsigned long object_index;
	float t;
	float plane[4];
	long surface_index;
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
	struct bevy_bridge_fog fog;
	struct bevy_bridge_fog_screen fog_screen;
	unsigned long decal_count;
	unsigned long decal_vertex_count;
	struct bevy_bridge_decal decals[BEVY_BRIDGE_DECALS];
	struct bevy_bridge_decal_vertex decal_vertices[BEVY_BRIDGE_DECAL_VERTICES];
	unsigned long sprite_batch_count;
	unsigned long sprite_vertex_count;
	struct bevy_bridge_sprite_batch sprite_batches[BEVY_BRIDGE_SPRITE_BATCHES];
	struct bevy_bridge_sprite_vertex sprite_vertices[BEVY_BRIDGE_SPRITE_VERTICES];
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
	/* (protocol 26; only with game.bevy_bridge_ray_tests) the vehicles'
	physics placement, and rays at them tested with their physics shapes */
	unsigned long physics_vehicle_count;
	struct bevy_bridge_physics_vehicle physics_vehicles[BEVY_BRIDGE_PHYSICS_VEHICLES];
	unsigned long vehicle_ray_test_count;
	struct bevy_bridge_vehicle_ray_test vehicle_ray_tests[BEVY_BRIDGE_VEHICLE_RAY_TESTS];
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
	/* the renderer's cursor while the game's menus take a pointer
	(bevy_bridge_shared.menus_pointer) and it is over the window: bit 30
	set, and the fractions of the window across (bits 0-14) and down (bits
	15-29) from the top left, each of 32767; 0 when there is none */
	unsigned long pointer;
};

/* an overlay picture: width x height pixels, RGBA, 8 bits a channel, the
top row first; the colours premultiplied by alpha (what covers the world)
and in the display's encoding (sRGB). With `multiply` (protocol 31), as
many pixels again follow it: what the multiplying blends (decals that darken
or brighten what is under them) do to the world, as half the factor (0.5:
nothing), in the display's encoding: the renderer multiplies its world by
twice it, under the picture. */
struct bevy_bridge_overlay
{
	unsigned long sequence;
	/* the bevy_bridge_frame.frame_index of the frame it shows */
	unsigned long frame_index;
	unsigned long width;
	unsigned long height;
	unsigned long multiply;
	unsigned long reserved[3];
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

/* (protocol 27) a guided projectile's steering in an update's step: its
bounding sphere's centre, its target's and the target's centre of mass
(unit_get_center_of_mass), and the difficulty's guidance scale when the
target is a player's unit (1 otherwise) */
struct bevy_bridge_projectile_guidance
{
	float projectile_center[3];
	float target_center[3];
	float target_center_of_mass[3];
	float guidance_scale;
	long game_time;
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
	/* (protocol 27) the objects its ray tests visited, as they were then
	(object_count from first_object in the feature object ring); what each
	collision's impact damage reported (object_cause_damage's material
	type, -2 for no damage done) and the random seed it left; and its
	guided steering, step by step */
	unsigned long first_object;
	unsigned long object_count;
	short damage_material_types[4];
	/* and the random seed the damage left (the response draws from it) */
	unsigned long damage_seeds[4];
	unsigned long guidance_count;
	struct bevy_bridge_projectile_guidance guidance[4];
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
	/* (protocol 27) its location, where the broad phase registers it */
	long location_leaf;
	short location_cluster;
	short pad;
	/* a projectile's own flags (projectile_flags), 0 for other objects */
	unsigned long projectile_flags;
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

/* what biped_update changes of an unparented biped before its animation
updates: its movement state, and its throttle, desired facing, state and
slipping ticks */
struct bevy_bridge_biped_update_state
{
	struct bevy_bridge_biped_moving_state moving;
	float throttle[3];
	float desired_facing_vector[3];
	char biped_state;
	char slipping_ticks;
	short pad;
};

/* one biped_update of an unparented biped (with
game.bevy_bridge_ray_tests), up to its animation update: the state before
and after (taken before its melee, footsteps, falling danger and discard),
what it reads, the global random seed as its airborne update began (if it
had one), and the movement record it made (NONE for none) */
struct bevy_bridge_biped_update_record
{
	unsigned long sequence;
	unsigned long biped_index;
	unsigned long definition_index;
	long structure_bsp_index;
	long moving_record;
	unsigned long dead;
	unsigned long control_flags;
	unsigned long aim_without_turning;
	char unit_animation_state;
	char base_seat_index;
	char enhanced_animation;
	char limp_body;
	long player_index;
	unsigned long airborne_seed_taken;
	unsigned long airborne_seed;
	struct bevy_bridge_biped_update_state before;
	struct bevy_bridge_biped_update_state after;
};

/* what unit_update_animation changes of a unit: its unit animation, and its
object animation and interpolation */
struct bevy_bridge_unit_animation_state
{
	unsigned short flags;
	short aiming_screen_index;
	short looking_screen_index;
	short action_animation_index;
	short action_animation_frame_index;
	short overlay_action_animation_index;
	short overlay_action_animation_frame_index;
	short soft_ping_animation_index;
	short soft_ping_animation_frame_index;
	short object_animation_index;
	short object_animation_frame_index;
	short interpolation_frame_index;
	short interpolation_frame_count;
	char seat_index;
	char weapon_index;
	char weapon_type_index;
	char state;
	char action;
	char overlay_action;
	char base_seat_index;
	char pad;
	short pad2;
	long object_animation_graph_index;
};

/* one unit_update_animation of a biped (with game.bevy_bridge_ray_tests):
the state before and after, what it reads (the animation update it is
given, the AI's animation state, the magic seats, force alert, flaming, the
current weapon's label, the global random seed), the side effects it had
(bits: 1 grenade released, 2 melee damage, 4 destroyed, 8 limp body
started, 16 seat exited, 32 seat entered, 64 grenade released on changing
state) and what it returned */
struct bevy_bridge_unit_animation_record
{
	unsigned long sequence;
	unsigned long unit_index;
	unsigned long definition_index;
	unsigned long random_seed;
	char state_desired;
	char crouching;
	char desired_state;
	char dead;
	char parented;
	char player;
	char force_alert;
	char flaming;
	short magic_base_seat_index;
	char magic_seat_index;
	char at_rest;
	char airborne;
	char pad[3];
	char weapon_label[32];
	unsigned long side_effects;
	short result;
	short pad2;
	struct bevy_bridge_unit_animation_state before;
	struct bevy_bridge_unit_animation_state after;
};

/* a node of a recorded pose: its orientation (rotation i j k w,
translation, scale) as the animation, overlays and scale left it, after
its object type's preprocess, the original it interpolates from, and its
matrix (scale, forward, left, up, position) */
struct bevy_bridge_pose_node
{
	float animated[8];
	float preprocessed[8];
	float original[8];
	float matrix[13];
};

/* one object_compute_node_matrices (with game.bevy_bridge_ray_tests; a
sample: objects whose index plus the game time is a multiple of 8): what it
reads (the animation frame it used, NONE for the model's pose, its world
relative flag, the object's place, scale, flags, interpolation, outgoing
function values and parent node matrix) and its nodes (node_count from
first_node in the pose node ring) */
struct bevy_bridge_pose_record
{
	unsigned long sequence;
	unsigned long object_index;
	unsigned long definition_index;
	short type;
	short node_count;
	unsigned long first_node;
	unsigned long object_flags;
	long animation_graph_index;
	short animation_index;
	short frame_index;
	short interpolation_frame_index;
	short interpolation_frame_count;
	unsigned long world_relative;
	float scale;
	float position[3];
	float forward[3];
	float up[3];
	float function_values[4];
	unsigned long has_parent;
	unsigned long parent_mirrored;
	float parent_node_matrix[13];
	/* (protocol 23) a unit's (unit_recorded): what its type's preprocess
	reads (unit_preprocess_node_orientations: its animation state, its
	emotion, mouth, seat acceleration, aiming and looking, its orientation
	as object_get_orientation gives it, and for a grenade throw on the move
	unit_animation_enhanced, unit_flying_through_air, its velocity, the game
	time and the legs' eased weights before), and what it sets (whether it
	aims and looks with euler screens, and their bounds) */
	unsigned long unit_recorded;
	unsigned long unit_enhanced;
	unsigned long unit_airborne;
	unsigned long unit_parented;
	unsigned long unit_has_weapon;
	unsigned long unit_player;
	long game_time;
	unsigned short unit_animation_flags;
	short aiming_screen_index;
	short looking_screen_index;
	short override_emotion_animation_index;
	char seat_index;
	char weapon_index;
	char state;
	char action;
	char base_seat_index;
	char emotion_index;
	short pad;
	short action_animation[2];
	short overlay_action_animation[2];
	short soft_ping_animation[2];
	float mouth_aperture;
	float seat_acceleration[3];
	float aiming_vector[3];
	float looking_vector[3];
	float orientation_forward[3];
	float orientation_up[3];
	float translational_velocity[3];
	float grenade_leg_weight;
	long grenade_leg_time;
	float grenade_air_weight;
	long grenade_air_time;
	unsigned long aiming_with_euler_screen;
	unsigned long looking_with_euler_screen;
	float aiming_screen_bounds[4];
	float looking_screen_bounds[4];
	/* (protocol 24; the game time above is every pose's, for its overlays,
	and the unit's fields every unit's) a vehicle's (vehicle_recorded): its
	turn, speed, wheel and suspensions (vehicle_preprocess_node_orientations
	reads them with its velocity and axes above); a device's
	(device_recorded): its flags, position and power
	(device_preprocess_node_orientations) */
	unsigned long vehicle_recorded;
	float vehicle_turn;
	float vehicle_speed;
	float vehicle_wheel;
	unsigned char vehicle_suspension[8];
	unsigned long device_recorded;
	unsigned long device_flags;
	float device_position;
	float device_power;
};

/* (protocol 25) a player biped's melee step in biped_update (with
game.bevy_bridge_ray_tests): what it reads (its melee ticks, whether the
player asks to melee, its weapon (NONE for none) and whether the weapon is
overcharged, whether it rides in a seat or zooms, its unit animation's
seat, weapon class and type and action, the random seed), whether it
started a melee (unit_animation_start_action: the action, its animation
and the random seed after) or caused its damage, and its melee ticks
after */
struct bevy_bridge_melee_record
{
	unsigned long sequence;
	unsigned long biped_index;
	unsigned long definition_index;
	unsigned long weapon_definition_index;
	unsigned long weapon_overcharged;
	unsigned long use_equipment;
	unsigned long in_seat;
	unsigned long zoomed;
	unsigned long enhanced;
	unsigned long random_seed_before;
	unsigned long random_seed_after_start;
	unsigned long started;
	unsigned long damaged;
	char ticks_before;
	char attack_tick_before;
	char ticks_after;
	char attack_tick_after;
	char seat_index;
	char weapon_index;
	char weapon_type_index;
	char action_before;
	char action_after;
	char pad[3];
	short action_animation_before[2];
	short action_animation_after[2];
};

/* (protocol 28) a vehicle as its physics reads and leaves it: its place,
axes, velocities, flags, location, the collision force and torque other
vehicles gave it, the mass points that stuck, and (after) the bounding
sphere centre its new pose gave it (object_compute_node_matrices) */
struct bevy_bridge_vehicle_physics_state
{
	float position[3];
	float forward[3];
	float up[3];
	float translational_velocity[3];
	float angular_velocity[3];
	unsigned long object_flags;
	long location_leaf;
	short location_cluster;
	short pad;
	float collision_force[3];
	float collision_torque[3];
	unsigned long stuck_mass_point_flags;
	float bounding_center[3];
};

/* a powered mass point's controls (powered_mass_point_datum: ground, water
and air friction velocities, water and air lift ratios, thrust and antigrav
fractions, rotation i j k w) */
struct bevy_bridge_powered_mass_point
{
	float values[7];
	float rotation[4];
};

/* a vehicle the unit collisions considered (objects_in_sphere): its
physics placement and whether it was at rest */
struct bevy_bridge_vehicle_physics_other
{
	unsigned long object_index;
	unsigned long definition_index;
	unsigned long at_rest;
	float origin[3];
	float forward[3];
	float up[3];
};

/* a mass point as physics_compute_new left it (mass_point_datum) */
struct bevy_bridge_mass_point
{
	unsigned char bytes[304];
};

/* (protocol 28) one physics_update of a vehicle (with
game.bevy_bridge_ray_tests): its state before and after, what its vehicle
code gave it (powered mass points, the magic force and torque), which
breakable surfaces stood, its mass points after (mass_point_count from
first_mass_point in the mass point ring), the objects its collision tests
visited (in the feature object ring, feature_calls calls of
collision_get_features_in_sphere), the other vehicles its unit collisions
considered, and whether it took the old physics (physics radius > 0) */
struct bevy_bridge_vehicle_physics_record
{
	unsigned long sequence;
	unsigned long vehicle_index;
	unsigned long definition_index;
	long structure_bsp_index;
	unsigned long old_physics;
	unsigned long powered_count;
	struct bevy_bridge_powered_mass_point powered[8];
	unsigned long has_magic;
	float magic_force[3];
	float magic_torque[3];
	unsigned char breakable_surfaces[32];
	struct bevy_bridge_vehicle_physics_state before;
	struct bevy_bridge_vehicle_physics_state after;
	unsigned long mass_point_count;
	unsigned long first_mass_point;
	unsigned long first_object;
	unsigned long object_count;
	unsigned long feature_calls;
	unsigned long other_count;
	struct bevy_bridge_vehicle_physics_other others[8];
};

/* (protocol 30) a vehicle as vehicle_update reads and leaves it: its
place, axes, velocities, flags, location, the bounding sphere centre its
pose gave it, and its vehicle datum (vehicles.c's _vehicle_datum, 88
bytes: flags, stop time, airborne, upending and on-ground ticks, speed,
slide, turn, wheel, treads, hover, thrust, suspensions, hover position,
collision force and torque, stuck mass points) */
struct bevy_bridge_vehicle_update_state
{
	float position[3];
	float forward[3];
	float up[3];
	float translational_velocity[3];
	float angular_velocity[3];
	unsigned long object_flags;
	long location_leaf;
	short location_cluster;
	short pad;
	float bounding_center[3];
	unsigned char vehicle[88];
};

/* (protocol 30) one vehicle_update, up to its animation (with
game.bevy_bridge_ray_tests): its state before and after, its driver's
controls (throttle, control flags, desired facing, seat power, whether an
AI drives), whether it has a parent, the physics record it made (its
sequence + 1, 0 for none), which breakable surfaces stood, and the objects
its other ray tests visited (in the feature object ring) */
struct bevy_bridge_vehicle_update_record
{
	unsigned long sequence;
	unsigned long vehicle_index;
	unsigned long definition_index;
	long structure_bsp_index;
	float throttle[3];
	unsigned long control_flags;
	float desired_facing[3];
	float seat_power[2];
	unsigned long driven_by_ai;
	unsigned long parented;
	unsigned long physics_record;
	unsigned char breakable_surfaces[32];
	struct bevy_bridge_vehicle_update_state before;
	struct bevy_bridge_vehicle_update_state after;
	unsigned long first_object;
	unsigned long object_count;
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
	/* nonzero while the game's menus are up and take a mouse pointer: the
	renderer then frees its cursor and forwards its place (input.pointer) */
	unsigned long menus_pointer;
	unsigned long reserved[2];
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
	/* (protocol 27) the periodic function tables the game built at startup
	(periodic_functions.c: 1024 bytes each, by function type), once a
	projectile has been recorded; nonzero once written */
	unsigned long periodic_tables_valid;
	unsigned char periodic_tables[12][1024];
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
	/* biped updates (a ring, as the projectile records) */
	unsigned long biped_update_record_count;
	unsigned long biped_update_pad;
	struct bevy_bridge_biped_update_record biped_update_records[BEVY_BRIDGE_BIPED_UPDATE_RECORDS];
	/* unit animation updates (a ring, as the projectile records) */
	unsigned long unit_animation_record_count;
	unsigned long unit_animation_pad;
	struct bevy_bridge_unit_animation_record unit_animation_records[BEVY_BRIDGE_UNIT_ANIMATION_RECORDS];
	/* sampled poses (rings, as the feature objects) */
	unsigned long pose_record_count;
	unsigned long pose_node_count;
	struct bevy_bridge_pose_record pose_records[BEVY_BRIDGE_POSE_RECORDS];
	struct bevy_bridge_pose_node pose_nodes[BEVY_BRIDGE_POSE_NODES];
	/* players' melee steps (a ring, as the projectiles) */
	unsigned long melee_record_count;
	struct bevy_bridge_melee_record melee_records[BEVY_BRIDGE_MELEE_RECORDS];
	/* vehicles' physics updates (rings, as the poses) */
	unsigned long vehicle_physics_record_count;
	unsigned long mass_point_count;
	struct bevy_bridge_vehicle_physics_record vehicle_physics_records[BEVY_BRIDGE_VEHICLE_PHYSICS_RECORDS];
	struct bevy_bridge_mass_point mass_points[BEVY_BRIDGE_MASS_POINTS];
	/* vehicle updates (a ring, as the projectiles) */
	unsigned long vehicle_update_record_count;
	struct bevy_bridge_vehicle_update_record vehicle_update_records[BEVY_BRIDGE_VEHICLE_UPDATE_RECORDS];
	/* (each overlay's picture, then its multiply's) */
	unsigned char overlay_pixels[BEVY_BRIDGE_OVERLAYS][BEVY_BRIDGE_OVERLAY_MAXIMUM_PIXELS * 4 * 2];
};

/* ---------- prototypes/the platform layer (posix_bevy_bridge.c, win32_bevy_bridge.c) */

/* the shared memory named `name` (BEVY_BRIDGE_DEFAULT_NAME), created at
`size` bytes and zeroed, or NULL */
void *bevy_bridge_platform_map(char const *name, unsigned long size);
/* the memory bevy_bridge_platform_map made, or NULL before then */
void *bevy_bridge_platform_shared(void);
unsigned long bevy_bridge_platform_process_id(void);

#endif /* __BEVY_BRIDGE_PROTOCOL_H */
