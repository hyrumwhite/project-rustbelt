/*
HALO_SIM.C

Runs bevy-rt-test's Rust ports of the game's simulation (crates/halo_sim,
a DLL: halo_sim.dll beside the game, or HALO_SIM_DLL) in place of the
game's own C, one system at a time: the object simulation handing off from
the game to Rust (ROADMAP phase 7).

Off unless game.rust_simulation names systems (HALO_RUST_SIMULATION, e.g.
"point_physics" or "point_physics:check"). The DLL reads the level from
the game's own map file at its first call in the level. In check mode both
run: the game keeps its own result, and the Rust result is compared with it
bit for bit; differences are counted and the first few logged.
*/

/* ---------- headers */

#include "cseries.h"
#include "cache/cache_files.h"
#include "cache/physical_memory_map.h"
#include "physics/breakable_surfaces.h"
#include "game/game.h"
#include "models/model_definitions.h"
#include "objects/objects.h"
#include "units/units.h"
#include "units/bipeds.h"
#include "ai/ai.h"
#include "units/vehicle_datum.h"
#include "physics/physics.h"
#include "structures/structure_bsp_definitions.h"
#include "physics/mass_point_datum.h"
#include "physics/powered_mass_point_datum.h"
#include "devices/devices.h"
#include "physics/point_physics.h"
#include "physics/collisions.h"
#include "items/projectiles.h"
#include "devices/device_definitions.h"
#include "items/projectiles_callbacks.h"
#include "scenario/scenario.h"
#include "tag_files/tag_groups.h"
#include "bevy_bridge.h"
#include "halo_sim.h"

#include <stdio.h>
#include <string.h>


/* ---------- constants */

/* the interface version this side speaks (crates/halo_sim's VERSION) */
#define HALO_SIM_VERSION 12

/* ---------- structures */

/* crates/halo_sim's PointPhysicsCall and PointPhysicsResult */
struct halo_sim_point_physics_call
{
	long structure_bsp_index;
	unsigned long flags;
	struct point_physics_definition const *definition;
	unsigned char const *breakable_surfaces;
	real_point3d position;
	real_vector3d velocity;
	long location_leaf;
	short location_cluster;
	short underwater;
	unsigned long has_force;
	real_vector3d force;
	real_vector3d wind;
	real radius;
	real dt;
};

struct halo_sim_point_physics_result
{
	unsigned long flags;
	real_point3d position;
	real_vector3d velocity;
	long location_leaf;
	short location_cluster;
	short collided;
	real_vector3d collision_normal;
	long collision_material_type;
};

/* crates/halo_sim's PoseOrientationsCall and PoseMatricesCall */
struct halo_sim_pose_orientations_call
{
	unsigned long object_index;
	unsigned long definition_index;
	long animation_graph_index;
	short animation_index;
	short frame_index;
	unsigned long object_flags;
	long game_time;
	real scale;
	real function_values[4];
	short node_count;
	short pad;
	real_orientation *orientations;
};

struct halo_sim_pose_matrices_call
{
	unsigned long definition_index;
	short node_count;
	short interpolation_frame_index;
	short interpolation_frame_count;
	short world_relative;
	unsigned long object_flags;
	real_point3d position;
	real_vector3d forward;
	real_vector3d up;
	unsigned long has_parent;
	unsigned long parent_mirrored;
	real_matrix4x3 parent_node_matrix;
	real_orientation const *original_orientations;
	real_orientation *orientations;
	real_matrix4x3 *matrices;
};

/* crates/halo_sim's PosePreprocessCall */
struct halo_sim_pose_preprocess_call
{
	unsigned long object_index;
	unsigned long definition_index;
	short object_type;
	short node_count;
	real_orientation *orientations;
	unsigned long enhanced;
	unsigned long airborne;
	unsigned long parented;
	unsigned long has_weapon;
	unsigned long player;
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
	real mouth_aperture;
	real_vector3d seat_acceleration;
	real_vector3d aiming_vector;
	real_vector3d looking_vector;
	real_vector3d orientation_forward;
	real_vector3d orientation_up;
	real_vector3d forward;
	real_vector3d up;
	real_vector3d translational_velocity;
	real leg_weight;
	long leg_time;
	real air_weight;
	long air_time;
	unsigned long aiming_with_euler_screen;
	unsigned long looking_with_euler_screen;
	unsigned long aiming_bounds_set;
	unsigned long looking_bounds_set;
	real_rectangle2d aiming_screen_bounds;
	real_rectangle2d looking_screen_bounds;
	real vehicle_turn;
	real vehicle_speed;
	real vehicle_wheel;
	unsigned char vehicle_suspension[8];
	unsigned long device_flags;
	real device_position;
	real device_power;
};

/* crates/halo_sim's HostObject and Host: the running game's objects, as
Rust's ray test reads them */
struct halo_sim_host_object
{
	unsigned long object_index;
	unsigned long definition_index;
	short object_type;
	short location_cluster;
	unsigned long flags;
	real_point3d bounding_center;
	real bounding_radius;
	unsigned long parent;
	unsigned char region_permutations[8];
	unsigned long node_count;
	real_matrix4x3 const *matrices;
	real_point3d origin;
	real_vector3d forward;
	real_vector3d up;
	unsigned long projectile_flags;
	unsigned long dead;
	real_vector3d velocity;
	unsigned long elevator;
	unsigned long biped_passes_through;
	long player_index;
	long parent_seat_index;
	real_point3d pill_base;
	real pill_height;
	real pill_width;
};

/* bipeds.c's struct biped_physics (crates/halo_map's BipedPhysics) */
struct halo_sim_biped_physics
{
	long biped_index;
	unsigned short in_flags;
	unsigned short pad6;
	real_point3d position;
	real_vector3d forward;
	real_vector3d aiming;
	real_vector3d velocity;
	real crouch_velocity;
	real_vector3d movement_desired;
	real movement_constants[14]; /* movement_penalty through uphill_velocity_scale */
	real_plane3d ground_plane;
	long existing_support_surface_index;
	real gravity;
	long bumped_object_index;
	long elevator_object_index;
	unsigned short out_flags;
	unsigned short pad_a2;
	long support_surface_index;
	long stick_surface_index;
	real_point3d new_position;
	real_vector3d new_velocity;
	real landing_velocity;
	real collision_velocity;
};

typedef char halo_sim_biped_physics_size_assert[sizeof(struct halo_sim_biped_physics) == 0xcc ? 1 : -1];

/* crates/halo_map's VehiclePhysicsState */
struct halo_sim_vehicle_physics_state
{
	real_point3d position;
	real_vector3d forward;
	real_vector3d up;
	real_vector3d translational_velocity;
	real_vector3d angular_velocity;
	unsigned long object_flags;
	long location_leaf;
	short location_cluster;
	short pad;
	real_vector3d collision_force;
	real_vector3d collision_torque;
	unsigned long stuck_mass_point_flags;
};

/* crates/halo_sim's OtherVehicleC and VehiclePhysicsCall */
struct halo_sim_other_vehicle
{
	unsigned long object_index;
	unsigned long definition_index;
	unsigned long at_rest;
	real_point3d origin;
	real_vector3d forward;
	real_vector3d up;
};

#define MAXIMUM_HALO_SIM_OTHER_VEHICLES 32

struct halo_sim_vehicle_physics_call
{
	unsigned long vehicle_index;
	unsigned long definition_index;
	long structure_bsp_index;
	unsigned char const *breakable_surfaces;
	struct halo_sim_vehicle_physics_state state;
	struct powered_mass_point_datum const *powered;
	long powered_count;
	unsigned long has_magic;
	real_vector3d magic_force;
	real_vector3d magic_torque;
	real_point3d bounding_center_after;
	struct halo_sim_other_vehicle const *others;
	unsigned long other_count;
	struct mass_point_datum *mass_points;
	unsigned long mass_point_count;
};

/* crates/halo_map's VehicleUpdateState */
struct halo_sim_vehicle_update_state
{
	real_point3d position;
	real_vector3d forward;
	real_vector3d up;
	real_vector3d translational_velocity;
	real_vector3d angular_velocity;
	unsigned long object_flags;
	long location_leaf;
	short location_cluster;
	short pad;
	struct _vehicle_datum vehicle;
};

/* crates/halo_sim's VehicleUpdateCall */
struct halo_sim_vehicle_update_call
{
	unsigned long vehicle_index;
	unsigned long definition_index;
	long structure_bsp_index;
	unsigned char const *breakable_surfaces;
	struct halo_sim_vehicle_update_state state;
	real_vector3d throttle;
	unsigned long control_flags;
	real_vector3d desired_facing;
	real seat_power[2];
	unsigned long driven_by_ai;
	real vehicle_floor;
	real vehicle_ceiling;
	real_point3d bounding_center_after;
	struct powered_mass_point_datum const *powered_base;
	long powered_base_count;
	struct halo_sim_other_vehicle const *others;
	unsigned long other_count;
};

/* crates/halo_sim's VehicleUpdateCallbacks */
struct halo_sim_vehicle_update_callbacks
{
	void (*set_position)(unsigned long vehicle_index, struct halo_sim_vehicle_update_state *state,
		real_point3d const *position, real_vector3d const *forward, real_vector3d const *up);
	long (*suspension_shift)(unsigned long vehicle_index, struct halo_sim_vehicle_update_state *state, real maximum_shift);
	void (*slipping_effects)(unsigned long vehicle_index, struct halo_sim_vehicle_update_state *state,
		struct mass_point_datum const *mass_points, unsigned long count);
	void (*crashing_effects)(unsigned long vehicle_index, struct halo_sim_vehicle_update_state *state,
		real_vector3d const *previous_velocity, struct mass_point_datum const *mass_points, unsigned long count);
	void (*thruster_effect)(unsigned long vehicle_index, struct halo_sim_vehicle_update_state *state, long pelican);
};

/* crates/halo_map's MovingState (136 bytes) */
struct halo_sim_moving_state
{
	real_point3d position;
	real_vector3d forward;
	real_vector3d up;
	real_vector3d translational_velocity;
	real_vector3d angular_velocity;
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
	real_point3d pathfinding_point;
	long bump_object_index;
	real crouch;
	real bank;
	real_plane3d ground_plane;
};

typedef char halo_sim_moving_state_size_assert[sizeof(struct halo_sim_moving_state) == 136 ? 1 : -1];

/* crates/halo_sim's MovingCall */
struct halo_sim_moving_call
{
	unsigned long biped_index;
	unsigned long definition_index;
	long structure_bsp_index;
	unsigned char const *breakable_surfaces;
	struct halo_sim_moving_state state;
	unsigned long dead;
	unsigned long ignore_translation;
	unsigned long suspended;
	unsigned long control_flags;
	char unit_animation_state;
	char base_seat_index;
	char aiming_speed;
	char actor;
	long player_index;
	real_vector3d throttle;
	real body_stun;
	real recent_body_damage;
	real_vector3d desired_facing_vector;
	real_vector3d aiming_vector;
	real_point3d bounding_center;
	long animation_graph_index;
	short animation_index;
	short animation_frame_index;
	real difficulty_infection_forms;
	real speed_multiplier;
	unsigned long cinematic;
	unsigned long double_speed;
	unsigned long coop_player_collisions;
	unsigned long actor_leaping;
	unsigned long recorded_animation;
	unsigned long actor_jump_asked;
	unsigned long actor_jumped;
	real_vector3d actor_jump_velocity;
	unsigned long aborted_animation;
};

/* crates/halo_map's UpdateState (164 bytes) */
struct halo_sim_update_state
{
	struct halo_sim_moving_state moving;
	real_vector3d throttle;
	real_vector3d desired_facing_vector;
	char biped_state;
	char slipping_ticks;
	short pad;
};

typedef char halo_sim_update_state_size_assert[sizeof(struct halo_sim_update_state) == 164 ? 1 : -1];

/* crates/halo_sim's UpdateCall */
struct halo_sim_update_call
{
	struct halo_sim_moving_call moving;
	struct halo_sim_update_state state;
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
};

/* crates/halo_map's UnitAnimationState (40 bytes) */
struct halo_sim_unit_animation_state
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
	unsigned long object_animation_graph_index;
};

typedef char halo_sim_unit_animation_state_size_assert[sizeof(struct halo_sim_unit_animation_state) == 40 ? 1 : -1];

/* crates/halo_sim's UnitAnimationCall */
struct halo_sim_unit_animation_call
{
	unsigned long unit_index;
	long unit_graph_index;
	struct halo_sim_unit_animation_state state;
	char state_desired;
	char crouching;
	char desired_state;
	char magic_seat_index;
	unsigned long dead;
	unsigned long parented;
	unsigned long player;
	unsigned long force_alert;
	unsigned long flaming;
	short magic_base_seat_index;
	short pad;
	unsigned long at_rest;
	unsigned long biped;
	unsigned long airborne;
	unsigned long destroyed_after_dying;
	unsigned long has_no_dying_airborne;
	char const *weapon_label;
	unsigned long random_seed;
	short result;
	short pad2;
	unsigned long effects;
};

/* crates/halo_sim's UnitAnimationCallbacks */
struct halo_sim_unit_animation_callbacks
{
	unsigned long (*random_seed)(void);
	void (*set_random_seed)(unsigned long seed);
	long (*effect)(unsigned long unit_index, struct halo_sim_unit_animation_state *state, unsigned long effect);
};

/* crates/halo_sim's MeleeCall and MeleeCallbacks */
struct halo_sim_melee_call
{
	unsigned long biped_index;
	long unit_graph_index;
	unsigned long enhanced_animation;
	unsigned long use_equipment;
	unsigned long in_seat;
	unsigned long zoomed;
	long weapon_index;
	unsigned long weapon_definition_index;
	unsigned long weapon_prevents;
	char ticks;
	char attack_tick;
	short pad;
	struct halo_sim_unit_animation_state animation;
	unsigned long random_seed;
	unsigned long started;
	unsigned long damaged;
};

struct halo_sim_melee_callbacks
{
	void (*started)(unsigned long biped_index, long weapon_index, struct halo_sim_unit_animation_state *state);
	void (*damage)(unsigned long biped_index);
};

/* crates/halo_sim's UpdateCallbacks */
struct halo_sim_update_callbacks
{
	void (*moving_begins)(unsigned long biped_index, struct halo_sim_update_state *state);
	void (*moving_ends)(unsigned long biped_index, struct halo_sim_update_state *state);
	void (*limp_body)(unsigned long biped_index, struct halo_sim_moving_state *state);
	unsigned long (*random_seed)(void);
	void (*set_random_seed)(unsigned long seed);
	void (*landing_footsteps)(unsigned long biped_index, struct halo_sim_moving_state *state);
	void (*slipping_footsteps)(unsigned long biped_index, struct halo_sim_moving_state *state);
};

/* crates/halo_sim's MovingCallbacks */
struct halo_sim_moving_callbacks
{
	void (*abort_animation)(unsigned long biped_index, struct halo_sim_moving_state *state);
	void (*translate)(unsigned long biped_index, struct halo_sim_moving_state *state, real_point3d const *position);
	long (*actor_aim_jump)(unsigned long biped_index, struct halo_sim_moving_state *state, real magnitude, real_vector3d *velocity);
	void (*jumped)(unsigned long biped_index, struct halo_sim_moving_state *state);
	void (*melee_impact)(unsigned long biped_index, struct halo_sim_moving_state *state, real_point3d const *position,
		real_point3d const *new_position);
	void (*bump_notice)(unsigned long biped_index, struct halo_sim_moving_state *state, long object_index,
		real_vector3d const *velocity);
	void (*falling_damage)(unsigned long biped_index, struct halo_sim_moving_state *state, real landing_velocity);
	void (*physics_begin)(unsigned long biped_index, void const *physics, long stand_test, real height, real radius);
	void (*physics_end)(unsigned long biped_index, void const *physics);
};

/* crates/halo_sim's VehiclePhysicsCallbacks */
struct halo_sim_vehicle_physics_callbacks
{
	void (*set_position)(unsigned long vehicle_index, struct halo_sim_vehicle_physics_state *state,
		real_point3d const *position, real_vector3d const *forward, real_vector3d const *up);
	void (*translate)(unsigned long vehicle_index, struct halo_sim_vehicle_physics_state *state, real_point3d const *new_position);
	void (*unit_collisions)(unsigned long vehicle_index, struct halo_sim_vehicle_physics_state *state);
	void (*begin)(unsigned long vehicle_index, struct halo_sim_vehicle_physics_state *state,
		struct powered_mass_point_datum const *powered, long powered_count, real_vector3d const *magic, long old);
	void (*end)(unsigned long vehicle_index, struct halo_sim_vehicle_physics_state *state,
		struct mass_point_datum const *mass_points, unsigned long count);
};

/* crates/halo_sim's BipedPhysicsCall */
struct halo_sim_biped_physics_call
{
	long structure_bsp_index;
	unsigned char const *breakable_surfaces;
	unsigned long stand_test;
	real collision_height_standing;
	real collision_radius;
};

/* crates/halo_map's ProjectileState: what a projectile update reads and
changes of a projectile */
struct halo_sim_projectile_state
{
	real_point3d position;
	real_vector3d velocity;
	unsigned long flags;
	unsigned long object_flags;
	short action;
	short hit_material_type;
	long ignore_object_index;
	long target_object_index;
	long parent_object_index;
	real detonation_timer;
	real detonation_timer_delta;
	real arming_time;
	real arming_time_delta;
	real odometer;
	real deceleration_timer;
	real deceleration_timer_delta;
	real deceleration;
	real maximum_damage_distance;
	long location_leaf;
	short location_cluster;
	short pad;
};

/* crates/halo_sim's GuidanceC, ProjectileEventC and ProjectileUpdateCall */
struct halo_sim_guidance
{
	real_point3d projectile_center;
	real_point3d target_center;
	real_point3d target_center_of_mass;
	real guidance_scale;
};

struct halo_sim_projectile_event
{
	short kind;
	short material_type;
	unsigned long seed_before;
	unsigned long seed_after;
	long object_index;
	long potential;
	real scale;
	real material_effect_scale;
	struct halo_sim_guidance guidance;
	long game_time;
	struct halo_sim_projectile_state state;
};

struct halo_sim_projectile_call
{
	unsigned long projectile_index;
	unsigned long definition_index;
	long structure_bsp_index;
	unsigned char const *breakable_surfaces;
	struct halo_sim_projectile_state state;
	unsigned long *random_seed;
	real_vector3d const *directions;
	long direction_count;
	unsigned char const *wander_table;
	long game_time;
	struct halo_sim_projectile_event const *events;
	unsigned long event_count;
	long collision_count;
	real time_remaining;
	long difference_event;
	long difference_reason;
};

/* crates/halo_sim's ProjectileCallbacks */
struct halo_sim_projectile_callbacks
{
	void (*timers_updated)(unsigned long projectile_index, struct halo_sim_projectile_state *state);
	void (*guidance)(unsigned long projectile_index, struct halo_sim_projectile_state *state, struct halo_sim_guidance *out);
	void (*collision_begin)(unsigned long projectile_index, struct halo_sim_projectile_state *state);
	void (*impact_damage)(unsigned long projectile_index, struct halo_sim_projectile_state *state,
		struct collision_result const *collision, real damage_scale, real_vector3d const *velocity,
		short *material_type, real *material_effect_scale);
	void (*breakable_surface_damage)(unsigned long projectile_index, struct halo_sim_projectile_state *state,
		struct collision_result const *collision, real_vector3d const *velocity);
	void (*effect)(unsigned long projectile_index, struct halo_sim_projectile_state *state, long effect,
		long material_type, long potential, struct collision_result const *collision, real_vector3d const *direction,
		real scale, real material_effect_scale);
	void (*combining_reset)(unsigned long child_index);
	void (*attach)(unsigned long projectile_index, struct halo_sim_projectile_state *state,
		real_point3d const *new_position, struct collision_result const *collision);
	void (*impact_noise)(unsigned long projectile_index, struct halo_sim_projectile_state *state, real_point3d const *point);
	void (*moved)(unsigned long projectile_index, struct halo_sim_projectile_state *state, real_point3d const *new_position,
		real_vector3d const *new_velocity, long location_leaf, long location_cluster, real time_remaining,
		long collision_count, long ignore_object_index);
};

struct halo_sim_host
{
	unsigned long (*cluster_objects)(short cluster, unsigned long *out, unsigned long maximum);
	long (*object)(unsigned long index, struct halo_sim_host_object *out, unsigned long purpose);
	unsigned long (*children)(unsigned long index, unsigned long *out, unsigned long maximum);
	struct halo_sim_projectile_callbacks projectile;
	void (*features_call)(void);
	struct halo_sim_vehicle_physics_callbacks vehicle_physics;
	struct halo_sim_vehicle_update_callbacks vehicle_update;
	struct halo_sim_moving_callbacks moving;
	struct halo_sim_update_callbacks biped_update;
	struct halo_sim_unit_animation_callbacks unit_animation;
	struct halo_sim_melee_callbacks melee;
};

/* crates/halo_sim's CollisionVectorCall and CollisionVectorResult */
struct halo_sim_collision_vector_call
{
	long structure_bsp_index;
	unsigned long flags;
	unsigned char const *breakable_surfaces;
	real_point3d point;
	real_vector3d vector;
	unsigned long ignore_object_index;
};

struct halo_sim_collision_vector_result
{
	long hit;
	short type;
	short node_index;
	real t;
	real_point3d point;
	real_plane3d plane;
	unsigned long object_index;
	short bsp_index;
	short material_type;
	long surface_index;
	long location_leaf;
	short location_cluster;
	unsigned short surface_flags;
	short region_index;
	short material_index;
	long plane_designator;
	unsigned long breakable_surface_index;
};

typedef unsigned long (*halo_sim_version_function)(void);
typedef long (*halo_sim_set_host_function)(struct halo_sim_host const *host);
typedef long (*halo_sim_projectile_function)(struct halo_sim_projectile_call *call);
typedef long (*halo_sim_biped_physics_function)(struct halo_sim_biped_physics_call const *call, void *physics);
typedef long (*halo_sim_vehicle_physics_function)(struct halo_sim_vehicle_physics_call *call);
typedef long (*halo_sim_vehicle_update_function)(struct halo_sim_vehicle_update_call *call);
typedef long (*halo_sim_moving_function)(struct halo_sim_moving_call *call);
typedef long (*halo_sim_update_function)(struct halo_sim_update_call *call);
typedef long (*halo_sim_unit_animation_function)(struct halo_sim_unit_animation_call *call);
typedef long (*halo_sim_melee_function)(struct halo_sim_melee_call *call);
typedef long (*halo_sim_collision_test_vector_function)(struct halo_sim_collision_vector_call const *call,
	struct halo_sim_collision_vector_result *result);
typedef long (*halo_sim_pose_preprocess_function)(struct halo_sim_pose_preprocess_call *call);
typedef long (*halo_sim_set_tag_cache_function)(void const *memory, unsigned long size, unsigned long base);
typedef long (*halo_sim_pose_orientations_function)(struct halo_sim_pose_orientations_call *call);
typedef long (*halo_sim_pose_matrices_function)(struct halo_sim_pose_matrices_call const *call);
typedef long (*halo_sim_load_map_function)(char const *path);
typedef long (*halo_sim_point_physics_function)(struct halo_sim_point_physics_call const *call,
	struct halo_sim_point_physics_result *result);

/* ---------- globals */

static struct
{
	boolean initialized;
	boolean loaded;
	short modes[NUMBER_OF_HALO_SIM_SYSTEMS];
	halo_sim_load_map_function load_map;
	halo_sim_point_physics_function point_physics;
	halo_sim_set_tag_cache_function set_tag_cache;
	halo_sim_pose_orientations_function pose_orientations;
	halo_sim_pose_matrices_function pose_matrices;
	halo_sim_pose_preprocess_function pose_preprocess;
	halo_sim_set_host_function set_host;
	halo_sim_collision_test_vector_function collision_test_vector;
	halo_sim_projectile_function projectile_update;
	halo_sim_projectile_function projectile_check;
	halo_sim_biped_physics_function biped_physics;
	halo_sim_vehicle_physics_function vehicle_physics_update;
	halo_sim_vehicle_physics_function vehicle_physics_check;
	halo_sim_vehicle_update_function vehicle_update;
	halo_sim_vehicle_update_function vehicle_update_check;
	halo_sim_moving_function biped_moving;
	halo_sim_moving_function biped_moving_check;
	halo_sim_update_function biped_update;
	halo_sim_update_function biped_update_check;
	halo_sim_unit_animation_function unit_animation;
	halo_sim_unit_animation_function unit_animation_check;
	halo_sim_melee_function player_melee;
	halo_sim_melee_function player_melee_check;
	long scenario_index;
	boolean level_loaded;
} halo_sim;

/* the check mode's comparisons */
static struct
{
	boolean pending;
	struct halo_sim_point_physics_call call;
	struct halo_sim_point_physics_result result;
	unsigned long compared;
	unsigned long differed;
} halo_sim_point_physics_checks;

static char const *const halo_sim_system_names[NUMBER_OF_HALO_SIM_SYSTEMS] =
{
	"point_physics",
	"poses",
	"collision",
	"projectiles",
	"biped_physics",
	"vehicle_physics",
	"vehicle_update",
	"biped_moving",
	"biped_update",
	"unit_animation",
	"player_melee",
};

/* the player melee checks: the step the game's began with, and the
comparisons */
static struct
{
	boolean recording;
	long biped_index;
	struct halo_sim_melee_call call;
	unsigned long compared;
	unsigned long differed;
	unsigned long started;
	unsigned long damaged;
} halo_sim_melee_checks;

/* the unit animation checks: the update the game's began with, and the
comparisons */
static struct
{
	boolean recording;
	long unit_index;
	struct halo_sim_unit_animation_call call;
	char weapon_label[64];
	unsigned long compared;
	unsigned long differed;
	unsigned long skipped;
	unsigned long effects;
} halo_sim_unit_animation_checks;

/* the biped update checks: the update the game's began with, what the
AI's jump aim and the random seed answered it, and the comparisons */
static struct
{
	boolean recording;
	long biped_index;
	struct halo_sim_update_call call;
	unsigned long compared;
	unsigned long differed;
	unsigned long skipped;
	unsigned long dead;
	unsigned long airborne;
	unsigned long tumbles;
} halo_sim_update_checks;

/* the biped movement checks: the update the game's began with, what the
AI's jump aim answered it, and the comparisons */
static struct
{
	boolean recording;
	long biped_index;
	struct halo_sim_moving_call call;
	unsigned long compared;
	unsigned long differed;
	unsigned long skipped;
	unsigned long aborted;
	unsigned long jumps;
} halo_sim_moving_checks;

/* the vehicle update checks: the state and inputs the game's update began
with, what its physics was handed and met, and the comparisons */
static struct
{
	boolean recording;
	long vehicle_index;
	struct halo_sim_vehicle_update_call call;
	struct powered_mass_point_datum powered[8];
	long powered_count;
	struct halo_sim_other_vehicle others[MAXIMUM_HALO_SIM_OTHER_VEHICLES];
	unsigned long other_count;
	boolean overflow;
	unsigned long compared;
	unsigned long differed;
	unsigned long skipped;
} halo_sim_vehicle_update_checks;

/* the vehicle physics checks: the state and inputs the game's update began
with, the vehicles its unit collisions considered, and the comparisons */
static struct
{
	boolean recording;
	long vehicle_index;
	struct halo_sim_vehicle_physics_state before;
	struct powered_mass_point_datum powered[8];
	long powered_count;
	boolean has_powered;
	unsigned long has_magic;
	real_vector3d magic_force;
	real_vector3d magic_torque;
	struct halo_sim_other_vehicle others[MAXIMUM_HALO_SIM_OTHER_VEHICLES];
	unsigned long other_count;
	boolean overflow;
	unsigned long compared;
	unsigned long differed;
	unsigned long skipped;
	unsigned long collided;
} halo_sim_vehicle_physics_checks;

/* the biped physics checks: Rust's result, and the comparisons */
static struct
{
	boolean pending;
	struct halo_sim_biped_physics before;
	struct halo_sim_biped_physics result;
	struct halo_sim_biped_physics_call call;
	unsigned long compared;
	unsigned long differed;
} halo_sim_biped_physics_checks;

/* the projectile checks: the state and random seed the game's update began
with, its calls, and the comparisons */
#define MAXIMUM_HALO_SIM_PROJECTILE_EVENTS 256
static struct
{
	boolean recording;
	boolean overflow;
	long projectile_index;
	unsigned long seed;
	struct halo_sim_projectile_state before;
	unsigned long event_count;
	struct halo_sim_projectile_event events[MAXIMUM_HALO_SIM_PROJECTILE_EVENTS];
	unsigned long compared;
	unsigned long differed;
	unsigned long skipped;
	unsigned long collisions;
	unsigned long events_compared;
} halo_sim_projectile_checks;

/* the collision checks' comparisons */
static struct
{
	unsigned long compared;
	unsigned long differed;
	/* what the game's hit: the structure, media, objects (vehicles' physics) */
	unsigned long structure_hits;
	unsigned long media_hits;
	unsigned long object_hits;
	unsigned long vehicle_physics_hits;
} halo_sim_collision_checks;

/* the pose checks: Rust's result, and the comparisons */
static struct
{
	boolean orientations_pending;
	long orientations_object;
	int world_relative;
	real_orientation orientations[MAXIMUM_NODES_PER_MODEL];
	boolean matrices_pending;
	long matrices_object;
	real_orientation interpolated[MAXIMUM_NODES_PER_MODEL];
	real_matrix4x3 matrices[MAXIMUM_NODES_PER_MODEL];
	boolean preprocess_pending;
	long preprocess_object;
	struct halo_sim_pose_preprocess_call preprocess;
	real_orientation preprocessed[MAXIMUM_NODES_PER_MODEL];
	unsigned long compared[3];
	unsigned long differed[3];
} halo_sim_pose_checks;

/* the platform layer's (port/linux/src) */
void platform_log(char const *format, ...);
void platform_translate_path(char const *xbox_path, char *host_path, unsigned long host_path_size);
char const *config_string(char const *name);

/* ---------- private code */

/* the platform layer's (win32_library.c, posix_library.c) */
void *platform_library_open(char const *path);
void *platform_library_symbol(void *library, char const *name);

static void *halo_sim_symbol(void *library, char const *name)
{
	return platform_library_symbol(library, name);
}

/* the host's callbacks: the running game's objects */
static unsigned long halo_sim_host_cluster_objects(short cluster, unsigned long *out, unsigned long maximum)
{
	unsigned long count = 0;
	long reference_index;
	long object_index;

	for (object_index = cluster_get_first_collideable_object(&reference_index, cluster);
		object_index != NONE;
		object_index = cluster_get_next_collideable_object(&reference_index))
	{
		if (count < maximum)
			out[count] = (unsigned long)object_index;
		count++;
	}

	return count;
}

/* whether a projectile update runs in Rust (the objects its ray tests
visit, recorded for the bridge as the game's own ray tests record them) */
static boolean halo_sim_projectile_updating;
/* and a biped's movement physics (the objects its features come from) */
static boolean halo_sim_biped_physics_updating;
/* and a vehicle's (its features' objects, and its ray tests') */
static boolean halo_sim_vehicle_physics_updating;

/* what the Rust port reads an object for (crates/halo_sim's purpose) */
enum
{
	_halo_sim_object_other = 0,
	_halo_sim_object_ray_test,
	_halo_sim_object_features,
};

static long halo_sim_host_object(unsigned long index, struct halo_sim_host_object *out, unsigned long purpose)
{
	struct object_datum *object = object_get((long)index);
	short region;

	/* (recorded for the bridge as the game's own ray tests and feature
	gatherings record what they visit) */
	if (purpose == _halo_sim_object_ray_test && (halo_sim_projectile_updating || halo_sim_vehicle_physics_updating))
		bevy_bridge_projectile_object((long)index);
	if (purpose == _halo_sim_object_features && (halo_sim_biped_physics_updating || halo_sim_vehicle_physics_updating))
		bevy_bridge_feature_object((long)index);

	out->object_index = index;
	out->definition_index = (unsigned long)object->definition_index;
	out->object_type = object->object.type;
	out->location_cluster = object->object.location.cluster_index;
	out->flags = object->object.flags;
	out->bounding_center = object->object.bounding_sphere_center;
	out->bounding_radius = object->object.bounding_sphere_radius;
	out->parent = (unsigned long)object->object.parent_object_index;
	for (region = 0; region < 8; region++)
	{
		out->region_permutations[region] = region < (short)sizeof(object->object.region_permutations) ?
			object->object.region_permutations[region] : 0;
	}
	out->node_count = (unsigned long)(object->object.node_matrices.size / (long)sizeof(real_matrix4x3));
	out->matrices = object_get_node_matrices((long)index);
	if (object->object.type == _object_type_vehicle)
	{
		object_get_origin((long)index, &out->origin);
		object_get_orientation((long)index, &out->forward, &out->up);
	}
	else
	{
		memset(&out->origin, 0, sizeof(out->origin));
		memset(&out->forward, 0, sizeof(out->forward));
		memset(&out->up, 0, sizeof(out->up));
	}
	out->projectile_flags = object->object.type == _object_type_projectile ?
		((struct projectile_datum *)object)->projectile.flags : 0;
	/* (as collision features see it: bevy_bridge.c's bevy_bridge_record_object) */
	out->dead = TEST_FLAG(object->object.damage_flags, _object_dead_bit) ? 1 : 0;
	out->velocity = object->object.translational_velocity;
	out->elevator = 0;
	out->biped_passes_through = 0;
	out->player_index = NONE;
	out->parent_seat_index = NONE;
	memset(&out->pill_base, 0, sizeof(out->pill_base));
	out->pill_height = 0.0f;
	out->pill_width = 0.0f;
	if (object->object.type == _object_type_machine)
	{
		struct machine_definition *machine_definition = machine_definition_get(object->definition_index);

		out->elevator = TEST_FLAG(machine_definition->machine.flags, _machine_never_appears_locked_bit) &&
			machine_definition->machine.elevator_node_index != NONE;
	}
	if (object->object.type == _object_type_biped)
	{
		struct biped_datum *biped = biped_get((long)index);

		out->biped_passes_through = TEST_FLAG(biped->biped.flags, _biped_movement_passes_through_bipeds_bit) ? 1 : 0;
		out->player_index = biped->unit.player_index;
		out->parent_seat_index = biped->unit.parent_seat_index;
		biped_get_physics_pill((long)index, &out->pill_base, &out->pill_height, &out->pill_width);
	}

	return TRUE;
}

/* an object's children: its first child, then that child's siblings */
static unsigned long halo_sim_host_children(unsigned long index, unsigned long *out, unsigned long maximum)
{
	unsigned long count = 0;
	long child_index;

	for (child_index = object_get((long)index)->object.first_child_object_index;
		child_index != NONE;
		child_index = object_get(child_index)->object.next_object_index)
	{
		if (count < maximum)
			out[count] = (unsigned long)child_index;
		count++;
	}

	return count;
}

/* a projectile's state, from the projectile */
static void halo_sim_projectile_state_load(long projectile_index, struct halo_sim_projectile_state *state)
{
	struct projectile_datum *projectile = projectile_get(projectile_index);

	state->position = projectile->object.position;
	state->velocity = projectile->object.translational_velocity;
	state->flags = projectile->projectile.flags;
	state->object_flags = projectile->object.flags;
	state->action = projectile->projectile.action;
	state->hit_material_type = projectile->projectile.hit_material_type;
	state->ignore_object_index = projectile->projectile.ignore_object_index;
	state->target_object_index = projectile->projectile.target_object_index;
	state->parent_object_index = projectile->object.parent_object_index;
	state->detonation_timer = projectile->projectile.detonation_timer;
	state->detonation_timer_delta = projectile->projectile.detonation_timer_delta;
	state->arming_time = projectile->projectile.arming_time;
	state->arming_time_delta = projectile->projectile.arming_time_delta;
	state->odometer = projectile->projectile.odometer;
	state->deceleration_timer = projectile->projectile.deceleration_timer;
	state->deceleration_timer_delta = projectile->projectile.deceleration_timer_delta;
	state->deceleration = projectile->projectile.deceleration;
	state->maximum_damage_distance = projectile->projectile.maximum_damage_distance;
	state->location_leaf = projectile->object.location.leaf_index;
	state->location_cluster = projectile->object.location.cluster_index;
	state->pad = 0;
}

/* and back to it: all but its place (position, location, parent), which
only the game's moves change */
static void halo_sim_projectile_state_store(long projectile_index, struct halo_sim_projectile_state const *state)
{
	struct projectile_datum *projectile = projectile_get(projectile_index);

	projectile->object.translational_velocity = state->velocity;
	projectile->projectile.flags = state->flags;
	projectile->object.flags = state->object_flags;
	projectile->projectile.action = state->action;
	projectile->projectile.hit_material_type = state->hit_material_type;
	projectile->projectile.ignore_object_index = state->ignore_object_index;
	projectile->projectile.target_object_index = state->target_object_index;
	projectile->projectile.detonation_timer = state->detonation_timer;
	projectile->projectile.detonation_timer_delta = state->detonation_timer_delta;
	projectile->projectile.arming_time = state->arming_time;
	projectile->projectile.arming_time_delta = state->arming_time_delta;
	projectile->projectile.odometer = state->odometer;
	projectile->projectile.deceleration_timer = state->deceleration_timer;
	projectile->projectile.deceleration_timer_delta = state->deceleration_timer_delta;
	projectile->projectile.deceleration = state->deceleration;
	projectile->projectile.maximum_damage_distance = state->maximum_damage_distance;
}

/* projectiles.c's, the game's side of a projectile update */
void projectile_rust_guidance(long projectile_index, real_point3d *projectile_center, real_point3d *target_center,
	real_point3d *target_center_of_mass, real *guidance_scale);
void projectile_rust_impact_damage(long projectile_index, struct collision_result const *collision, real damage_scale,
	real_vector3d const *new_velocity, short *material_type, real *material_effect_scale);
void projectile_rust_breakable_surface_damage(long projectile_index, struct collision_result const *collision,
	real_vector3d const *new_velocity);
void projectile_rust_effect(long projectile_index, short effect, short material_type, boolean potential,
	struct collision_result const *collision, real_vector3d const *direction, real effect_scale, real material_effect_scale);
void projectile_rust_combining_reset(long child_index);
void projectile_rust_attach(long projectile_index, real_point3d const *new_position, struct collision_result const *collision);
void projectile_rust_impact_noise(long projectile_index, real_point3d const *point);
void projectile_rust_moved(long projectile_index, real_point3d const *new_position, real_vector3d const *new_velocity,
	struct location const *location, real time_remaining, short collision_count, long ignore_object_index);

/* the Rust port's callbacks: the update's state written to the projectile,
the game's call, and the state read back */
static void halo_sim_projectile_timers_updated(unsigned long projectile_index, struct halo_sim_projectile_state *state)
{
	halo_sim_projectile_state_store((long)projectile_index, state);
	projectile_export_function_values((long)projectile_index);
	halo_sim_projectile_state_load((long)projectile_index, state);
}

static void halo_sim_projectile_guidance(unsigned long projectile_index, struct halo_sim_projectile_state *state,
	struct halo_sim_guidance *out)
{
	halo_sim_projectile_state_store((long)projectile_index, state);
	projectile_rust_guidance((long)projectile_index, &out->projectile_center, &out->target_center,
		&out->target_center_of_mass, &out->guidance_scale);
	halo_sim_projectile_state_load((long)projectile_index, state);
}

/* (for the bridge's recording: the seed a response begins with) */
static void halo_sim_projectile_collision_begin(unsigned long projectile_index, struct halo_sim_projectile_state *state)
{
	bevy_bridge_projectile_collision();
}

static void halo_sim_projectile_impact_damage(unsigned long projectile_index, struct halo_sim_projectile_state *state,
	struct collision_result const *collision, real damage_scale, real_vector3d const *velocity,
	short *material_type, real *material_effect_scale)
{
	halo_sim_projectile_state_store((long)projectile_index, state);
	projectile_rust_impact_damage((long)projectile_index, collision, damage_scale, velocity, material_type, material_effect_scale);
	halo_sim_projectile_state_load((long)projectile_index, state);
}

static void halo_sim_projectile_breakable_surface_damage(unsigned long projectile_index, struct halo_sim_projectile_state *state,
	struct collision_result const *collision, real_vector3d const *velocity)
{
	halo_sim_projectile_state_store((long)projectile_index, state);
	projectile_rust_breakable_surface_damage((long)projectile_index, collision, velocity);
	halo_sim_projectile_state_load((long)projectile_index, state);
}

static void halo_sim_projectile_effect(unsigned long projectile_index, struct halo_sim_projectile_state *state, long effect,
	long material_type, long potential, struct collision_result const *collision, real_vector3d const *direction,
	real scale, real material_effect_scale)
{
	halo_sim_projectile_state_store((long)projectile_index, state);
	projectile_rust_effect((long)projectile_index, (short)effect, (short)material_type, potential != 0, collision, direction,
		scale, material_effect_scale);
	halo_sim_projectile_state_load((long)projectile_index, state);
}

static void halo_sim_projectile_combining_reset(unsigned long child_index)
{
	projectile_rust_combining_reset((long)child_index);
}

static void halo_sim_projectile_attach(unsigned long projectile_index, struct halo_sim_projectile_state *state,
	real_point3d const *new_position, struct collision_result const *collision)
{
	halo_sim_projectile_state_store((long)projectile_index, state);
	projectile_rust_attach((long)projectile_index, new_position, collision);
	halo_sim_projectile_state_load((long)projectile_index, state);
}

static void halo_sim_projectile_impact_noise(unsigned long projectile_index, struct halo_sim_projectile_state *state,
	real_point3d const *point)
{
	halo_sim_projectile_state_store((long)projectile_index, state);
	projectile_rust_impact_noise((long)projectile_index, point);
	halo_sim_projectile_state_load((long)projectile_index, state);
}

static void halo_sim_projectile_moved(unsigned long projectile_index, struct halo_sim_projectile_state *state,
	real_point3d const *new_position, real_vector3d const *new_velocity, long location_leaf, long location_cluster,
	real time_remaining, long collision_count, long ignore_object_index)
{
	struct location location;

	location.leaf_index = location_leaf;
	location.cluster_index = (short)location_cluster;
	location.bonus = 0;
	halo_sim_projectile_state_store((long)projectile_index, state);
	projectile_rust_moved((long)projectile_index, new_position, new_velocity, &location, time_remaining,
		(short)collision_count, ignore_object_index);
	halo_sim_projectile_state_load((long)projectile_index, state);
}

/* a vehicle's physics state, from the vehicle */
static void halo_sim_vehicle_physics_state_load(long vehicle_index, struct halo_sim_vehicle_physics_state *state)
{
	struct vehicle_datum *vehicle = vehicle_datum_get(vehicle_index);

	state->position = vehicle->object.position;
	state->forward = vehicle->object.forward;
	state->up = vehicle->object.up;
	state->translational_velocity = vehicle->object.translational_velocity;
	state->angular_velocity = vehicle->object.angular_velocity;
	state->object_flags = vehicle->object.flags;
	state->location_leaf = vehicle->object.location.leaf_index;
	state->location_cluster = vehicle->object.location.cluster_index;
	state->pad = 0;
	state->collision_force = vehicle->vehicle.collision_force;
	state->collision_torque = vehicle->vehicle.collision_torque;
	state->stuck_mass_point_flags = vehicle->vehicle.stuck_mass_point_flags;
}

/* and back to it: all but its position and location, which only the
game's moves change */
static void halo_sim_vehicle_physics_state_store(long vehicle_index, struct halo_sim_vehicle_physics_state const *state)
{
	struct vehicle_datum *vehicle = vehicle_datum_get(vehicle_index);

	vehicle->object.forward = state->forward;
	vehicle->object.up = state->up;
	vehicle->object.translational_velocity = state->translational_velocity;
	vehicle->object.angular_velocity = state->angular_velocity;
	vehicle->object.flags = state->object_flags;
	vehicle->vehicle.collision_force = state->collision_force;
	vehicle->vehicle.collision_torque = state->collision_torque;
	vehicle->vehicle.stuck_mass_point_flags = state->stuck_mass_point_flags;
}

/* physics.c's (the game's unit collisions) */
void physics_rust_compute_unit_collisions(long object_index);

/* the Rust port's callbacks: the update's state written to the vehicle,
the game's call, and the state read back */
static void halo_sim_vehicle_physics_set_position(unsigned long vehicle_index, struct halo_sim_vehicle_physics_state *state,
	real_point3d const *position, real_vector3d const *forward, real_vector3d const *up)
{
	halo_sim_vehicle_physics_state_store((long)vehicle_index, state);
	object_set_position((long)vehicle_index, position, forward, up);
	halo_sim_vehicle_physics_state_load((long)vehicle_index, state);
}

static void halo_sim_vehicle_physics_translate(unsigned long vehicle_index, struct halo_sim_vehicle_physics_state *state,
	real_point3d const *new_position)
{
	struct object_datum *object = object_get((long)vehicle_index);
	struct location new_location;

	halo_sim_vehicle_physics_state_store((long)vehicle_index, state);
	scenario_location_from_line(&new_location, &object->object.location, &object->object.position, new_position);
	object_translate((long)vehicle_index, new_position, &new_location);
	halo_sim_vehicle_physics_state_load((long)vehicle_index, state);
}

static void halo_sim_vehicle_physics_unit_collisions(unsigned long vehicle_index, struct halo_sim_vehicle_physics_state *state)
{
	halo_sim_vehicle_physics_state_store((long)vehicle_index, state);
	physics_rust_compute_unit_collisions((long)vehicle_index);
	halo_sim_vehicle_physics_state_load((long)vehicle_index, state);
}

/* whether a vehicle's update runs in Rust (its physics, recorded for the
bridge by Rust's calls; the game's own physics_update records itself) */
static boolean halo_sim_vehicle_update_running;

static void halo_sim_vehicle_physics_begin(unsigned long vehicle_index, struct halo_sim_vehicle_physics_state *state,
	struct powered_mass_point_datum const *powered, long powered_count, real_vector3d const *magic, long old)
{
	if (!halo_sim_vehicle_update_running)
		return;
	halo_sim_vehicle_physics_state_store((long)vehicle_index, state);
	bevy_bridge_vehicle_physics_begin((long)vehicle_index, powered, (short)powered_count,
		magic, magic ? magic + 1 : NULL, old != 0);
}

static void halo_sim_vehicle_physics_end(unsigned long vehicle_index, struct halo_sim_vehicle_physics_state *state,
	struct mass_point_datum const *mass_points, unsigned long count)
{
	if (!halo_sim_vehicle_update_running)
		return;
	halo_sim_vehicle_physics_state_store((long)vehicle_index, state);
	bevy_bridge_vehicle_physics_end((long)vehicle_index, mass_points, (short)count);
}

/* a vehicle's update state, from the vehicle */
static void halo_sim_vehicle_update_state_load(long vehicle_index, struct halo_sim_vehicle_update_state *state)
{
	struct vehicle_datum *vehicle = vehicle_datum_get(vehicle_index);

	state->position = vehicle->object.position;
	state->forward = vehicle->object.forward;
	state->up = vehicle->object.up;
	state->translational_velocity = vehicle->object.translational_velocity;
	state->angular_velocity = vehicle->object.angular_velocity;
	state->object_flags = vehicle->object.flags;
	state->location_leaf = vehicle->object.location.leaf_index;
	state->location_cluster = vehicle->object.location.cluster_index;
	state->pad = 0;
	state->vehicle = vehicle->vehicle;
}

/* and back to it: all but its position and location, which only the
game's moves change */
static void halo_sim_vehicle_update_state_store(long vehicle_index, struct halo_sim_vehicle_update_state const *state)
{
	struct vehicle_datum *vehicle = vehicle_datum_get(vehicle_index);

	vehicle->object.forward = state->forward;
	vehicle->object.up = state->up;
	vehicle->object.translational_velocity = state->translational_velocity;
	vehicle->object.angular_velocity = state->angular_velocity;
	vehicle->object.flags = state->object_flags;
	vehicle->vehicle = state->vehicle;
}

/* vehicles.c's, the game's side of a vehicle update */
boolean vehicle_rust_suspension_sound(long vehicle_index, real maximum_shift);
void vehicle_rust_slipping_effects(long vehicle_index, struct mass_point_datum const *mass_points);
void vehicle_rust_crashing_effects(long vehicle_index, real_vector3d const *previous_velocity,
	struct mass_point_datum const *mass_points);
void vehicle_rust_thruster_effect(long vehicle_index, boolean pelican);

static void halo_sim_vehicle_update_set_position(unsigned long vehicle_index, struct halo_sim_vehicle_update_state *state,
	real_point3d const *position, real_vector3d const *forward, real_vector3d const *up)
{
	halo_sim_vehicle_update_state_store((long)vehicle_index, state);
	object_set_position((long)vehicle_index, position, forward, up);
	halo_sim_vehicle_update_state_load((long)vehicle_index, state);
}

static long halo_sim_vehicle_update_suspension_shift(unsigned long vehicle_index, struct halo_sim_vehicle_update_state *state,
	real maximum_shift)
{
	boolean played;

	halo_sim_vehicle_update_state_store((long)vehicle_index, state);
	played = vehicle_rust_suspension_sound((long)vehicle_index, maximum_shift);
	halo_sim_vehicle_update_state_load((long)vehicle_index, state);
	return played ? 1 : 0;
}

static void halo_sim_vehicle_update_slipping_effects(unsigned long vehicle_index, struct halo_sim_vehicle_update_state *state,
	struct mass_point_datum const *mass_points, unsigned long count)
{
	halo_sim_vehicle_update_state_store((long)vehicle_index, state);
	vehicle_rust_slipping_effects((long)vehicle_index, mass_points);
	halo_sim_vehicle_update_state_load((long)vehicle_index, state);
}

static void halo_sim_vehicle_update_crashing_effects(unsigned long vehicle_index, struct halo_sim_vehicle_update_state *state,
	real_vector3d const *previous_velocity, struct mass_point_datum const *mass_points, unsigned long count)
{
	halo_sim_vehicle_update_state_store((long)vehicle_index, state);
	vehicle_rust_crashing_effects((long)vehicle_index, previous_velocity, mass_points);
	halo_sim_vehicle_update_state_load((long)vehicle_index, state);
}

static void halo_sim_vehicle_update_thruster_effect(unsigned long vehicle_index, struct halo_sim_vehicle_update_state *state,
	long pelican)
{
	halo_sim_vehicle_update_state_store((long)vehicle_index, state);
	vehicle_rust_thruster_effect((long)vehicle_index, pelican != 0);
	halo_sim_vehicle_update_state_load((long)vehicle_index, state);
}

/* the update's animation data, while a biped's movement runs in Rust */
static struct unit_animation_update_data *halo_sim_moving_animation;

/* a biped's movement state, from the biped (and the update's animation data) */
static void halo_sim_moving_state_load(long biped_index, struct unit_animation_update_data const *animation,
	struct halo_sim_moving_state *state)
{
	struct biped_datum *biped = biped_get(biped_index);

	state->position = biped->object.position;
	state->forward = biped->object.forward;
	state->up = biped->object.up;
	state->translational_velocity = biped->object.translational_velocity;
	state->angular_velocity = biped->object.angular_velocity;
	state->object_flags = biped->object.flags;
	state->location_leaf = biped->object.location.leaf_index;
	state->location_cluster = biped->object.location.cluster_index;
	state->landing = biped->biped.landing;
	state->biped_flags = biped->biped.flags;
	state->landing_recovery_counter = biped->biped.landing_recovery_counter;
	state->landing_recovery_time = biped->biped.landing_recovery_time;
	state->elevator_ticks = biped->biped.elevator_ticks;
	state->bump_ticks = biped->biped.bump_ticks;
	state->jump_recovery_timer = biped->biped.jump_recovery_timer;
	state->airborne_ticks = biped->biped.airborne_ticks;
	state->animation_state_desired = animation->state_desired;
	state->animation_crouching = animation->crouching ? 1 : 0;
	state->elevator_object_index = biped->biped.elevator_object_index;
	state->support_surface_index = biped->biped.support_surface_index;
	state->pathfinding_surface_index = biped->biped.pathfinding_surface_index;
	state->pathfinding_point = biped->biped.pathfinding_point;
	state->bump_object_index = biped->biped.bump_object_index;
	state->crouch = biped->biped.crouch;
	state->bank = biped->biped.bank;
	state->ground_plane = biped->biped.ground_plane;
}

/* and back to it: all but its position and location, which only the
game's moves change */
static void halo_sim_moving_state_store(long biped_index, struct unit_animation_update_data *animation,
	struct halo_sim_moving_state const *state)
{
	struct biped_datum *biped = biped_get(biped_index);

	biped->object.forward = state->forward;
	biped->object.up = state->up;
	biped->object.translational_velocity = state->translational_velocity;
	biped->object.angular_velocity = state->angular_velocity;
	biped->object.flags = state->object_flags;
	biped->biped.landing = state->landing;
	biped->biped.flags = state->biped_flags;
	biped->biped.landing_recovery_counter = state->landing_recovery_counter;
	biped->biped.landing_recovery_time = state->landing_recovery_time;
	biped->biped.elevator_ticks = state->elevator_ticks;
	biped->biped.bump_ticks = state->bump_ticks;
	biped->biped.jump_recovery_timer = state->jump_recovery_timer;
	biped->biped.airborne_ticks = state->airborne_ticks;
	animation->state_desired = state->animation_state_desired;
	animation->crouching = state->animation_crouching != 0;
	biped->biped.elevator_object_index = state->elevator_object_index;
	biped->biped.support_surface_index = state->support_surface_index;
	biped->biped.pathfinding_surface_index = state->pathfinding_surface_index;
	biped->biped.pathfinding_point = state->pathfinding_point;
	biped->biped.bump_object_index = state->bump_object_index;
	biped->biped.crouch = state->crouch;
	biped->biped.bank = state->bank;
	biped->biped.ground_plane = state->ground_plane;
}

/* bipeds.c's, the game's side of a biped's movement */
void biped_rust_jumped(long biped_index);
void biped_rust_melee_impact(long biped_index, real_point3d const *position, real_point3d const *new_position);
void biped_rust_falling_damage(long biped_index, real landing_velocity);

static void halo_sim_moving_abort_animation(unsigned long biped_index, struct halo_sim_moving_state *state)
{
	halo_sim_moving_state_store((long)biped_index, halo_sim_moving_animation, state);
	unit_abort_animation((long)biped_index);
	bevy_bridge_biped_aborted_animation();
	halo_sim_moving_state_load((long)biped_index, halo_sim_moving_animation, state);
}

static void halo_sim_moving_translate(unsigned long biped_index, struct halo_sim_moving_state *state,
	real_point3d const *position)
{
	halo_sim_moving_state_store((long)biped_index, halo_sim_moving_animation, state);
	object_translate((long)biped_index, position, NULL);
	halo_sim_moving_state_load((long)biped_index, halo_sim_moving_animation, state);
}

static long halo_sim_moving_actor_aim_jump(unsigned long biped_index, struct halo_sim_moving_state *state, real magnitude,
	real_vector3d *velocity)
{
	struct biped_datum *biped = biped_get((long)biped_index);
	long actor_index = biped->unit.swarm_actor_index != NONE ? biped->unit.swarm_actor_index : biped->unit.actor_index;
	boolean jumped = FALSE;

	halo_sim_moving_state_store((long)biped_index, halo_sim_moving_animation, state);
	if (actor_index != NONE)
	{
		boolean leap = biped->unit.animation.state == _unit_state_leap_start ||
			biped->unit.animation.state == _unit_state_leap_airborne;
		float recorded[3];

		jumped = actor_aim_jump(actor_index, (long)biped_index, leap, magnitude, velocity);
		recorded[0] = velocity->i;
		recorded[1] = velocity->j;
		recorded[2] = velocity->k;
		bevy_bridge_biped_actor_jump(jumped, recorded);
	}
	halo_sim_moving_state_load((long)biped_index, halo_sim_moving_animation, state);
	return jumped ? 1 : 0;
}

static void halo_sim_moving_jumped(unsigned long biped_index, struct halo_sim_moving_state *state)
{
	halo_sim_moving_state_store((long)biped_index, halo_sim_moving_animation, state);
	biped_rust_jumped((long)biped_index);
	halo_sim_moving_state_load((long)biped_index, halo_sim_moving_animation, state);
}

static void halo_sim_moving_melee_impact(unsigned long biped_index, struct halo_sim_moving_state *state,
	real_point3d const *position, real_point3d const *new_position)
{
	halo_sim_moving_state_store((long)biped_index, halo_sim_moving_animation, state);
	biped_rust_melee_impact((long)biped_index, position, new_position);
	halo_sim_moving_state_load((long)biped_index, halo_sim_moving_animation, state);
}

static void halo_sim_moving_bump_notice(unsigned long biped_index, struct halo_sim_moving_state *state, long object_index,
	real_vector3d const *velocity)
{
	halo_sim_moving_state_store((long)biped_index, halo_sim_moving_animation, state);
	ai_handle_bump((long)biped_index, object_index, velocity);
	halo_sim_moving_state_load((long)biped_index, halo_sim_moving_animation, state);
}

static void halo_sim_moving_falling_damage(unsigned long biped_index, struct halo_sim_moving_state *state,
	real landing_velocity)
{
	halo_sim_moving_state_store((long)biped_index, halo_sim_moving_animation, state);
	biped_rust_falling_damage((long)biped_index, landing_velocity);
	halo_sim_moving_state_load((long)biped_index, halo_sim_moving_animation, state);
}

static void halo_sim_moving_physics_begin(unsigned long biped_index, void const *physics, long stand_test, real height,
	real radius)
{
	bevy_bridge_biped_physics_begin((long)biped_index, physics, sizeof(struct halo_sim_biped_physics),
		stand_test != 0, height, radius);
}

static void halo_sim_moving_physics_end(unsigned long biped_index, void const *physics)
{
	bevy_bridge_biped_physics_end(physics, sizeof(struct halo_sim_biped_physics));
}

/* a biped's update state, from the biped (and the update's animation data) */
static void halo_sim_update_state_load(long biped_index, struct unit_animation_update_data const *animation,
	struct halo_sim_update_state *state)
{
	struct biped_datum *biped = biped_get(biped_index);

	halo_sim_moving_state_load(biped_index, animation, &state->moving);
	state->throttle = biped->unit.throttle;
	state->desired_facing_vector = biped->unit.desired_facing_vector;
	state->biped_state = biped->biped.state;
	state->slipping_ticks = biped->biped.slipping_ticks;
	state->pad = 0;
}

static void halo_sim_update_state_store(long biped_index, struct unit_animation_update_data *animation,
	struct halo_sim_update_state const *state)
{
	struct biped_datum *biped = biped_get(biped_index);

	halo_sim_moving_state_store(biped_index, animation, &state->moving);
	biped->unit.throttle = state->throttle;
	biped->unit.desired_facing_vector = state->desired_facing_vector;
	biped->biped.state = state->biped_state;
	biped->biped.slipping_ticks = state->slipping_ticks;
}

/* what the rest of the game answered the movement, while a biped's update
runs in Rust (for the bridge's recording of the movement) */
static struct halo_sim_moving_call const *halo_sim_update_moving_call;

/* bipeds.c's, the game's side of a biped's update */
void biped_rust_limp_body(long biped_index);
void biped_rust_landing_footsteps(long biped_index);
void biped_rust_slipping_footsteps(long biped_index);

static void halo_sim_update_moving_begins(unsigned long biped_index, struct halo_sim_update_state *state)
{
	struct halo_sim_moving_call const *call = halo_sim_update_moving_call;

	halo_sim_update_state_store((long)biped_index, halo_sim_moving_animation, state);
	if (call && bevy_bridge_biped_moving_recording())
	{
		float aiming_vector[3];

		aiming_vector[0] = call->aiming_vector.i;
		aiming_vector[1] = call->aiming_vector.j;
		aiming_vector[2] = call->aiming_vector.k;
		bevy_bridge_biped_moving_begin((long)biped_index, halo_sim_moving_animation->state_desired,
			halo_sim_moving_animation->crouching, aiming_vector, call->difficulty_infection_forms,
			call->speed_multiplier, call->cinematic, call->double_speed, call->coop_player_collisions,
			call->actor_leaping, call->recorded_animation);
	}
	halo_sim_update_state_load((long)biped_index, halo_sim_moving_animation, state);
}

static void halo_sim_update_moving_ends(unsigned long biped_index, struct halo_sim_update_state *state)
{
	halo_sim_update_state_store((long)biped_index, halo_sim_moving_animation, state);
	bevy_bridge_biped_moving_end((long)biped_index, halo_sim_moving_animation->state_desired,
		halo_sim_moving_animation->crouching);
	halo_sim_update_state_load((long)biped_index, halo_sim_moving_animation, state);
}

static void halo_sim_update_limp_body(unsigned long biped_index, struct halo_sim_moving_state *state)
{
	halo_sim_moving_state_store((long)biped_index, halo_sim_moving_animation, state);
	biped_rust_limp_body((long)biped_index);
	halo_sim_moving_state_load((long)biped_index, halo_sim_moving_animation, state);
}

unsigned long get_random_seed(void);
unsigned long *get_global_random_seed_address(void);

static unsigned long halo_sim_update_random_seed(void)
{
	bevy_bridge_biped_update_airborne();
	return get_random_seed();
}

static void halo_sim_update_set_random_seed(unsigned long seed)
{
	*get_global_random_seed_address() = seed;
}

static unsigned long halo_sim_update_random_seed_plain(void)
{
	return get_random_seed();
}

/* a unit's animation state, from the unit */
static void halo_sim_unit_animation_state_load(long unit_index, struct halo_sim_unit_animation_state *state)
{
	struct unit_datum *unit = unit_get(unit_index);

	memset(state, 0, sizeof(*state));
	state->flags = unit->unit.animation.flags;
	state->aiming_screen_index = unit->unit.animation.aiming_screen_index;
	state->looking_screen_index = unit->unit.animation.looking_screen_index;
	state->action_animation_index = unit->unit.animation.action_animation.index;
	state->action_animation_frame_index = unit->unit.animation.action_animation.frame_index;
	state->overlay_action_animation_index = unit->unit.animation.overlay_action_animation.index;
	state->overlay_action_animation_frame_index = unit->unit.animation.overlay_action_animation.frame_index;
	state->soft_ping_animation_index = unit->unit.animation.soft_ping_animation.index;
	state->soft_ping_animation_frame_index = unit->unit.animation.soft_ping_animation.frame_index;
	state->object_animation_index = unit->object.animation.state.index;
	state->object_animation_frame_index = unit->object.animation.state.frame_index;
	state->interpolation_frame_index = unit->object.animation.interpolation_frame_index;
	state->interpolation_frame_count = unit->object.animation.interpolation_frame_count;
	state->seat_index = unit->unit.animation.seat_index;
	state->weapon_index = unit->unit.animation.weapon_index;
	state->weapon_type_index = unit->unit.animation.weapon_type_index;
	state->state = unit->unit.animation.state;
	state->action = unit->unit.animation.action;
	state->overlay_action = unit->unit.animation.overlay_action;
	state->base_seat_index = unit->unit.animation.base_seat_index;
	state->object_animation_graph_index = unit->object.animation.animation_graph_index;
}

/* and back to it */
static void halo_sim_unit_animation_state_store(long unit_index, struct halo_sim_unit_animation_state const *state)
{
	struct unit_datum *unit = unit_get(unit_index);

	unit->unit.animation.flags = state->flags;
	unit->unit.animation.aiming_screen_index = state->aiming_screen_index;
	unit->unit.animation.looking_screen_index = state->looking_screen_index;
	unit->unit.animation.action_animation.index = state->action_animation_index;
	unit->unit.animation.action_animation.frame_index = state->action_animation_frame_index;
	unit->unit.animation.overlay_action_animation.index = state->overlay_action_animation_index;
	unit->unit.animation.overlay_action_animation.frame_index = state->overlay_action_animation_frame_index;
	unit->unit.animation.soft_ping_animation.index = state->soft_ping_animation_index;
	unit->unit.animation.soft_ping_animation.frame_index = state->soft_ping_animation_frame_index;
	unit->object.animation.state.index = state->object_animation_index;
	unit->object.animation.state.frame_index = state->object_animation_frame_index;
	unit->object.animation.interpolation_frame_index = state->interpolation_frame_index;
	unit->object.animation.interpolation_frame_count = state->interpolation_frame_count;
	unit->unit.animation.seat_index = state->seat_index;
	unit->unit.animation.weapon_index = state->weapon_index;
	unit->unit.animation.weapon_type_index = state->weapon_type_index;
	unit->unit.animation.state = state->state;
	unit->unit.animation.action = state->action;
	unit->unit.animation.overlay_action = state->overlay_action;
	unit->unit.animation.base_seat_index = state->base_seat_index;
	unit->object.animation.animation_graph_index = state->object_animation_graph_index;
}

/* bipeds.c's, a player melee's side effects */
void biped_rust_melee_started(long biped_index, long weapon_index);
void biped_rust_melee_damage(long biped_index);

static void halo_sim_melee_started(unsigned long biped_index, long weapon_index, struct halo_sim_unit_animation_state *state)
{
	halo_sim_unit_animation_state_store((long)biped_index, state);
	biped_rust_melee_started((long)biped_index, weapon_index);
	halo_sim_unit_animation_state_load((long)biped_index, state);
}

static void halo_sim_melee_damage(unsigned long biped_index)
{
	biped_rust_melee_damage((long)biped_index);
}

/* units.c's, a unit animation's side effect */
void unit_rust_animation_effect(long unit_index, unsigned long effect);

static long halo_sim_unit_animation_effect(unsigned long unit_index, struct halo_sim_unit_animation_state *state,
	unsigned long effect)
{
	halo_sim_unit_animation_state_store((long)unit_index, state);
	unit_rust_animation_effect((long)unit_index, effect);
	/* (destroying a dead unit deletes it) */
	if (!object_try_and_get_and_verify_type((long)unit_index, _object_mask_unit))
		return 0;
	halo_sim_unit_animation_state_load((long)unit_index, state);
	return 1;
}

static void halo_sim_update_landing_footsteps(unsigned long biped_index, struct halo_sim_moving_state *state)
{
	halo_sim_moving_state_store((long)biped_index, halo_sim_moving_animation, state);
	biped_rust_landing_footsteps((long)biped_index);
	halo_sim_moving_state_load((long)biped_index, halo_sim_moving_animation, state);
}

static void halo_sim_update_slipping_footsteps(unsigned long biped_index, struct halo_sim_moving_state *state)
{
	halo_sim_moving_state_store((long)biped_index, halo_sim_moving_animation, state);
	biped_rust_slipping_footsteps((long)biped_index);
	halo_sim_moving_state_load((long)biped_index, halo_sim_moving_animation, state);
}

/* (for the bridge's recording: a gathering of features) */
static void halo_sim_features_call(void)
{
	if (halo_sim_biped_physics_updating || halo_sim_vehicle_physics_updating)
		bevy_bridge_features_call();
}

static struct halo_sim_host const halo_sim_host_callbacks =
{
	halo_sim_host_cluster_objects,
	halo_sim_host_object,
	halo_sim_host_children,
	{
		halo_sim_projectile_timers_updated,
		halo_sim_projectile_guidance,
		halo_sim_projectile_collision_begin,
		halo_sim_projectile_impact_damage,
		halo_sim_projectile_breakable_surface_damage,
		halo_sim_projectile_effect,
		halo_sim_projectile_combining_reset,
		halo_sim_projectile_attach,
		halo_sim_projectile_impact_noise,
		halo_sim_projectile_moved,
	},
	halo_sim_features_call,
	{
		halo_sim_vehicle_physics_set_position,
		halo_sim_vehicle_physics_translate,
		halo_sim_vehicle_physics_unit_collisions,
		halo_sim_vehicle_physics_begin,
		halo_sim_vehicle_physics_end,
	},
	{
		halo_sim_vehicle_update_set_position,
		halo_sim_vehicle_update_suspension_shift,
		halo_sim_vehicle_update_slipping_effects,
		halo_sim_vehicle_update_crashing_effects,
		halo_sim_vehicle_update_thruster_effect,
	},
	{
		halo_sim_moving_abort_animation,
		halo_sim_moving_translate,
		halo_sim_moving_actor_aim_jump,
		halo_sim_moving_jumped,
		halo_sim_moving_melee_impact,
		halo_sim_moving_bump_notice,
		halo_sim_moving_falling_damage,
		halo_sim_moving_physics_begin,
		halo_sim_moving_physics_end,
	},
	{
		halo_sim_update_moving_begins,
		halo_sim_update_moving_ends,
		halo_sim_update_limp_body,
		halo_sim_update_random_seed,
		halo_sim_update_set_random_seed,
		halo_sim_update_landing_footsteps,
		halo_sim_update_slipping_footsteps,
	},
	{
		halo_sim_update_random_seed_plain,
		halo_sim_update_set_random_seed,
		halo_sim_unit_animation_effect,
	},
	{
		halo_sim_melee_started,
		halo_sim_melee_damage,
	},
};

static void halo_sim_initialize(void)
{
	char const *systems;
	char const *path;
	void *library;
	short system;

	halo_sim.initialized = TRUE;
	halo_sim.scenario_index = NONE;
	systems = config_string("game.rust_simulation");
	if (!systems || !systems[0])
		return;

	path = getenv("HALO_SIM_DLL");
	if (!path || !path[0])
		path = "halo_sim.dll";
	library = platform_library_open(path);
	if (!library)
	{
		platform_log("halo_sim: %s isn't loaded; the game's own simulation runs", path);
		return;
	}
	{
		halo_sim_version_function version = (halo_sim_version_function)halo_sim_symbol(library, "halo_sim_version");

		if (!version || version() != HALO_SIM_VERSION)
		{
			platform_log("halo_sim: %s speaks another version; the game's own simulation runs", path);
			return;
		}
	}
	halo_sim.load_map = (halo_sim_load_map_function)halo_sim_symbol(library, "halo_sim_load_map");
	halo_sim.point_physics = (halo_sim_point_physics_function)halo_sim_symbol(library, "halo_sim_point_physics_update");
	halo_sim.set_tag_cache = (halo_sim_set_tag_cache_function)halo_sim_symbol(library, "halo_sim_set_tag_cache");
	halo_sim.pose_orientations = (halo_sim_pose_orientations_function)halo_sim_symbol(library, "halo_sim_pose_orientations");
	halo_sim.pose_matrices = (halo_sim_pose_matrices_function)halo_sim_symbol(library, "halo_sim_pose_matrices");
	halo_sim.pose_preprocess = (halo_sim_pose_preprocess_function)halo_sim_symbol(library, "halo_sim_pose_preprocess");
	halo_sim.set_host = (halo_sim_set_host_function)halo_sim_symbol(library, "halo_sim_set_host");
	halo_sim.collision_test_vector = (halo_sim_collision_test_vector_function)halo_sim_symbol(library, "halo_sim_collision_test_vector");
	halo_sim.projectile_update = (halo_sim_projectile_function)halo_sim_symbol(library, "halo_sim_projectile_update");
	halo_sim.projectile_check = (halo_sim_projectile_function)halo_sim_symbol(library, "halo_sim_projectile_check");
	halo_sim.biped_physics = (halo_sim_biped_physics_function)halo_sim_symbol(library, "halo_sim_biped_physics_update");
	halo_sim.vehicle_physics_update = (halo_sim_vehicle_physics_function)halo_sim_symbol(library, "halo_sim_vehicle_physics_update");
	halo_sim.vehicle_physics_check = (halo_sim_vehicle_physics_function)halo_sim_symbol(library, "halo_sim_vehicle_physics_check");
	halo_sim.vehicle_update = (halo_sim_vehicle_update_function)halo_sim_symbol(library, "halo_sim_vehicle_update");
	halo_sim.vehicle_update_check = (halo_sim_vehicle_update_function)halo_sim_symbol(library, "halo_sim_vehicle_update_check");
	halo_sim.biped_moving = (halo_sim_moving_function)halo_sim_symbol(library, "halo_sim_biped_moving_update");
	halo_sim.biped_moving_check = (halo_sim_moving_function)halo_sim_symbol(library, "halo_sim_biped_moving_check");
	halo_sim.biped_update = (halo_sim_update_function)halo_sim_symbol(library, "halo_sim_biped_update");
	halo_sim.biped_update_check = (halo_sim_update_function)halo_sim_symbol(library, "halo_sim_biped_update_check");
	halo_sim.unit_animation = (halo_sim_unit_animation_function)halo_sim_symbol(library, "halo_sim_unit_animation_update");
	halo_sim.unit_animation_check = (halo_sim_unit_animation_function)halo_sim_symbol(library, "halo_sim_unit_animation_check");
	halo_sim.player_melee = (halo_sim_melee_function)halo_sim_symbol(library, "halo_sim_player_melee");
	halo_sim.player_melee_check = (halo_sim_melee_function)halo_sim_symbol(library, "halo_sim_player_melee_check");
	if (!halo_sim.load_map || !halo_sim.point_physics || !halo_sim.set_tag_cache || !halo_sim.pose_orientations ||
		!halo_sim.pose_matrices || !halo_sim.pose_preprocess || !halo_sim.set_host || !halo_sim.collision_test_vector ||
		!halo_sim.projectile_update || !halo_sim.projectile_check || !halo_sim.biped_physics ||
		!halo_sim.vehicle_physics_update || !halo_sim.vehicle_physics_check ||
		!halo_sim.vehicle_update || !halo_sim.vehicle_update_check ||
		!halo_sim.biped_moving || !halo_sim.biped_moving_check ||
		!halo_sim.biped_update || !halo_sim.biped_update_check ||
		!halo_sim.unit_animation || !halo_sim.unit_animation_check ||
		!halo_sim.player_melee || !halo_sim.player_melee_check)
	{
		platform_log("halo_sim: %s lacks an entry point; the game's own simulation runs", path);
		return;
	}
	halo_sim.loaded = TRUE;
	halo_sim.set_host(&halo_sim_host_callbacks);

	for (system = 0; system < NUMBER_OF_HALO_SIM_SYSTEMS; system++)
	{
		char const *name = halo_sim_system_names[system];
		char const *found = strstr(systems, name);
		size_t length = strlen(name);

		if (found && (found == systems || found[-1] == ',') &&
			(found[length] == 0 || found[length] == ',' || found[length] == ':'))
		{
			halo_sim.modes[system] = strncmp(found + length, ":check", 6) == 0 ? _halo_sim_mode_check : _halo_sim_mode_rust;
			platform_log("halo_sim: %s runs in Rust%s", name,
				halo_sim.modes[system] == _halo_sim_mode_check ? ", checked against the game's" : "");
		}
	}
}

/* the level's map file, read by the DLL once a level */
static boolean halo_sim_level_loaded(void)
{
	if (global_scenario_index != halo_sim.scenario_index)
	{
		char xbox_path[256] = "";
		char path[1024] = "";
		long count = -1;

		halo_sim.scenario_index = global_scenario_index;
		halo_sim.level_loaded = FALSE;
		if (global_scenario_index != NONE &&
			snprintf(xbox_path, sizeof(xbox_path), "%s%s.map", cache_files_map_directory(),
				tag_name_strip_path(tag_get_name(global_scenario_index))) < (int)sizeof(xbox_path))
		{
			/* (the game's d:\maps\, on the host) */
			platform_translate_path(xbox_path, path, sizeof(path));
			count = halo_sim.load_map(path);
		}
		if (count > 0)
		{
			/* the level's tags, where the game holds them (its pointers
			address them from where they are) */
			void *tag_cache = physical_memory_get_tag_cache_base_address();
			long tags = halo_sim.set_tag_cache(tag_cache, TAG_CACHE_SIZE, (unsigned long)tag_cache);

			platform_log("halo_sim: reads the game's %ld tags at %p", tags, tag_cache);
			if (tags <= 0)
				count = -1;
		}
		halo_sim.level_loaded = count > 0;
		platform_log("halo_sim: %s the level %s (%ld structure bsps)", halo_sim.level_loaded ? "loaded" : "couldn't load",
			global_scenario_index != NONE ? path : "(none)", count);
	}

	return halo_sim.level_loaded;
}

/* ---------- public code */

short halo_sim_mode(short system)
{
	if (!halo_sim.initialized)
		halo_sim_initialize();
	if (!halo_sim.loaded || system < 0 || system >= NUMBER_OF_HALO_SIM_SYSTEMS ||
		halo_sim.modes[system] == _halo_sim_mode_c || !halo_sim_level_loaded())
	{
		return _halo_sim_mode_c;
	}

	return halo_sim.modes[system];
}

int halo_sim_point_physics_update(
	unsigned long flags,
	void const *definition,
	void *location,
	void *position,
	void *velocity,
	void const *force,
	void const *wind,
	int underwater,
	float radius,
	float dt,
	void *collision_normal,
	short *collision_material_type,
	unsigned long *result)
{
	short mode = halo_sim_mode(_halo_sim_point_physics);
	struct halo_sim_point_physics_call call;
	struct halo_sim_point_physics_result rust;
	struct location *point_location = (struct location *)location;

	halo_sim_point_physics_checks.pending = FALSE;
	if (mode == _halo_sim_mode_c)
		return FALSE;

	call.structure_bsp_index = global_structure_bsp_index;
	call.flags = flags;
	call.definition = (struct point_physics_definition const *)definition;
	call.breakable_surfaces = (unsigned char const *)breakable_surface_flags_get();
	call.position = *(real_point3d *)position;
	call.velocity = *(real_vector3d *)velocity;
	call.location_leaf = point_location->leaf_index;
	call.location_cluster = point_location->cluster_index;
	call.underwater = underwater ? 1 : 0;
	call.has_force = force != NULL;
	if (force)
		call.force = *(real_vector3d const *)force;
	else
		memset(&call.force, 0, sizeof(call.force));
	call.wind = *(real_vector3d const *)wind;
	call.radius = radius;
	call.dt = dt;
	if (halo_sim.point_physics(&call, &rust) != 0)
		return FALSE;

	if (mode == _halo_sim_mode_check)
	{
		halo_sim_point_physics_checks.pending = TRUE;
		halo_sim_point_physics_checks.call = call;
		halo_sim_point_physics_checks.result = rust;
		return FALSE;
	}

	*result = rust.flags;
	*(real_point3d *)position = rust.position;
	*(real_vector3d *)velocity = rust.velocity;
	point_location->leaf_index = rust.location_leaf;
	point_location->cluster_index = rust.location_cluster;
	if (rust.collided)
	{
		if (collision_normal)
			*(real_vector3d *)collision_normal = rust.collision_normal;
		if (collision_material_type)
			*collision_material_type = (short)rust.collision_material_type;
	}

	return TRUE;
}

void halo_sim_point_physics_check(
	unsigned long result,
	void const *location,
	void const *position,
	void const *velocity,
	void const *collision_normal,
	short const *collision_material_type)
{
	struct halo_sim_point_physics_result const *rust = &halo_sim_point_physics_checks.result;
	struct location const *point_location = (struct location const *)location;
	boolean same;

	if (!halo_sim_point_physics_checks.pending)
		return;
	halo_sim_point_physics_checks.pending = FALSE;
	same = rust->flags == result &&
		memcmp(&rust->position, position, sizeof(real_point3d)) == 0 &&
		memcmp(&rust->velocity, velocity, sizeof(real_vector3d)) == 0 &&
		rust->location_leaf == point_location->leaf_index &&
		rust->location_cluster == point_location->cluster_index &&
		(!rust->collided ||
			((!collision_normal || memcmp(&rust->collision_normal, collision_normal, sizeof(real_vector3d)) == 0) &&
			(!collision_material_type || (short)rust->collision_material_type == *collision_material_type)));
	halo_sim_point_physics_checks.compared++;
	if (!same)
	{
		halo_sim_point_physics_checks.differed++;
		if (halo_sim_point_physics_checks.differed <= 8)
		{
			real_point3d const *p = (real_point3d const *)position;

			platform_log("halo_sim: point physics differs: game %lx (%f %f %f) leaf %ld, rust %lx (%f %f %f) leaf %ld",
				result, p->x, p->y, p->z, point_location->leaf_index,
				rust->flags, rust->position.x, rust->position.y, rust->position.z, rust->location_leaf);
			{
				struct halo_sim_point_physics_call const *c = &halo_sim_point_physics_checks.call;

				platform_log("halo_sim:   from bsp %ld flags %lx position %08lx %08lx %08lx velocity %08lx %08lx %08lx leaf %ld wind %08lx %08lx %08lx underwater %d radius %08lx dt %08lx force %lu",
					c->structure_bsp_index, c->flags,
					*(unsigned long const *)&c->position.x, *(unsigned long const *)&c->position.y, *(unsigned long const *)&c->position.z,
					*(unsigned long const *)&c->velocity.i, *(unsigned long const *)&c->velocity.j, *(unsigned long const *)&c->velocity.k,
					c->location_leaf,
					*(unsigned long const *)&c->wind.i, *(unsigned long const *)&c->wind.j, *(unsigned long const *)&c->wind.k,
					c->underwater, *(unsigned long const *)&c->radius, *(unsigned long const *)&c->dt, c->has_force);
			}
		}
	}
	if ((halo_sim_point_physics_checks.compared & 0xffff) == 0)
	{
		platform_log("halo_sim: point physics: %lu of %lu agree with the game",
			halo_sim_point_physics_checks.compared - halo_sim_point_physics_checks.differed,
			halo_sim_point_physics_checks.compared);
	}
}

/* ---------- poses */

int halo_sim_pose_orientations(long object_index, short node_count, void *orientations,
	int *world_relative, short *frame_index)
{
	short mode = halo_sim_mode(_halo_sim_poses);
	struct object_datum *object;
	struct halo_sim_pose_orientations_call call;
	long result;

	halo_sim_pose_checks.orientations_pending = FALSE;
	if (mode == _halo_sim_mode_c || node_count <= 0 || node_count > MAXIMUM_NODES_PER_MODEL)
		return FALSE;
	object = object_get(object_index);
	call.object_index = (unsigned long)object_index;
	call.definition_index = (unsigned long)object->definition_index;
	call.animation_graph_index = object->object.animation.animation_graph_index;
	call.animation_index = object->object.animation.state.index;
	call.frame_index = object->object.animation.state.frame_index;
	call.object_flags = object->object.flags;
	call.game_time = game_time_get();
	call.scale = object->object.scale;
	memcpy(call.function_values, object->object.outgoing_function_values, sizeof(call.function_values));
	call.node_count = node_count;
	call.pad = 0;
	call.orientations = mode == _halo_sim_mode_check ? halo_sim_pose_checks.orientations : (real_orientation *)orientations;
	result = halo_sim.pose_orientations(&call);
	if (result < 0)
		return FALSE;
	if (mode == _halo_sim_mode_check)
	{
		halo_sim_pose_checks.orientations_pending = TRUE;
		halo_sim_pose_checks.orientations_object = object_index;
		halo_sim_pose_checks.world_relative = (int)result;
		return FALSE;
	}
	*world_relative = (int)result;
	*frame_index = call.frame_index;

	return TRUE;
}

static char const *const halo_sim_pose_stage_names[3] = { "orientations", "matrices", "preprocess" };

static void halo_sim_pose_report(short stage, long object_index, short node_count, short node_index)
{
	halo_sim_pose_checks.differed[stage]++;
	if (halo_sim_pose_checks.differed[stage] <= 8)
	{
		platform_log("halo_sim: pose %s of %s (object %08lx) differs at node %d of %d",
			halo_sim_pose_stage_names[stage], tag_get_name(object_get(object_index)->definition_index),
			(unsigned long)object_index, node_index, node_count);
	}
}

static void halo_sim_pose_tally(short stage)
{
	halo_sim_pose_checks.compared[stage]++;
	if ((halo_sim_pose_checks.compared[stage] & 0x3fff) == 0 || halo_sim_pose_checks.compared[stage] == 1)
	{
		platform_log("halo_sim: pose %s: %lu of %lu agree with the game", halo_sim_pose_stage_names[stage],
			halo_sim_pose_checks.compared[stage] - halo_sim_pose_checks.differed[stage], halo_sim_pose_checks.compared[stage]);
	}
}

void halo_sim_pose_orientations_check(long object_index, short node_count, void const *orientations,
	int world_relative)
{
	real_orientation const *game = (real_orientation const *)orientations;
	short node_index;

	if (!halo_sim_pose_checks.orientations_pending || halo_sim_pose_checks.orientations_object != object_index)
		return;
	halo_sim_pose_checks.orientations_pending = FALSE;
	for (node_index = 0; node_index < node_count; node_index++)
	{
		if (memcmp(&game[node_index], &halo_sim_pose_checks.orientations[node_index], sizeof(real_orientation)) != 0)
			break;
	}
	if (node_index < node_count || world_relative != halo_sim_pose_checks.world_relative)
		halo_sim_pose_report(0, object_index, node_count, node_index);
	halo_sim_pose_tally(0);
}

int halo_sim_pose_matrices(long object_index, short node_count, int world_relative,
	void const *parent_node_matrix, void *orientations, void *matrices)
{
	short mode = halo_sim_mode(_halo_sim_poses);
	struct object_datum *object;
	struct halo_sim_pose_matrices_call call;

	halo_sim_pose_checks.matrices_pending = FALSE;
	if (mode == _halo_sim_mode_c || node_count <= 0 || node_count > MAXIMUM_NODES_PER_MODEL)
		return FALSE;
	object = object_get(object_index);
	call.definition_index = (unsigned long)object->definition_index;
	call.node_count = node_count;
	call.interpolation_frame_index = object->object.animation.interpolation_frame_index;
	call.interpolation_frame_count = object->object.animation.interpolation_frame_count;
	call.world_relative = world_relative ? 1 : 0;
	call.object_flags = object->object.flags;
	call.position = object->object.position;
	call.forward = object->object.forward;
	call.up = object->object.up;
	call.has_parent = parent_node_matrix != NULL;
	call.parent_mirrored = parent_node_matrix &&
		TEST_FLAG(object_get(object->object.parent_object_index)->object.flags, _object_mirrored_bit);
	if (parent_node_matrix)
		call.parent_node_matrix = *(real_matrix4x3 const *)parent_node_matrix;
	else
		memset(&call.parent_node_matrix, 0, sizeof(call.parent_node_matrix));
	call.original_orientations = call.interpolation_frame_count > 0 ?
		(real_orientation const *)object_header_block_get(object_index, &object->object.original_node_orientations) :
		NULL;
	if (mode == _halo_sim_mode_check)
	{
		memcpy(halo_sim_pose_checks.interpolated, orientations, sizeof(real_orientation) * node_count);
		call.orientations = halo_sim_pose_checks.interpolated;
		call.matrices = halo_sim_pose_checks.matrices;
	}
	else
	{
		call.orientations = (real_orientation *)orientations;
		call.matrices = (real_matrix4x3 *)matrices;
	}
	if (halo_sim.pose_matrices(&call) != 0)
		return FALSE;
	if (mode == _halo_sim_mode_check)
	{
		halo_sim_pose_checks.matrices_pending = TRUE;
		halo_sim_pose_checks.matrices_object = object_index;
		return FALSE;
	}

	return TRUE;
}

void halo_sim_pose_matrices_check(long object_index, short node_count, void const *orientations,
	void const *matrices)
{
	real_orientation const *game_orientations = (real_orientation const *)orientations;
	real_matrix4x3 const *game_matrices = (real_matrix4x3 const *)matrices;
	short node_index;

	if (!halo_sim_pose_checks.matrices_pending || halo_sim_pose_checks.matrices_object != object_index)
		return;
	halo_sim_pose_checks.matrices_pending = FALSE;
	for (node_index = 0; node_index < node_count; node_index++)
	{
		if (memcmp(&game_orientations[node_index], &halo_sim_pose_checks.interpolated[node_index], sizeof(real_orientation)) != 0 ||
			memcmp(&game_matrices[node_index], &halo_sim_pose_checks.matrices[node_index], sizeof(real_matrix4x3)) != 0)
		{
			break;
		}
	}
	if (node_index < node_count)
		halo_sim_pose_report(1, object_index, node_count, node_index);
	halo_sim_pose_tally(1);
}

/* ---------- the object types' preprocess */

boolean unit_animation_enhanced(long unit_index); /* units.c */

static void halo_sim_pose_preprocess_gather(long object_index, short node_count, struct halo_sim_pose_preprocess_call *call)
{
	struct object_datum *object = object_get(object_index);

	memset(call, 0, sizeof(*call));
	call->object_index = (unsigned long)object_index;
	call->definition_index = (unsigned long)object->definition_index;
	call->object_type = object->object.type;
	call->node_count = node_count;
	call->forward = object->object.forward;
	call->up = object->object.up;
	call->translational_velocity = object->object.translational_velocity;
	call->game_time = game_time_get();
	if (TEST_FLAG(_object_mask_unit, object->object.type))
	{
		struct unit_datum *unit = unit_get(object_index);

		call->enhanced = unit_animation_enhanced(object_index) ? 1 : 0;
		call->airborne = unit_flying_through_air(object_index) ? 1 : 0;
		call->parented = object->object.parent_object_index != NONE;
		call->has_weapon = unit->unit.current_weapon_index != NONE;
		call->player = unit->unit.player_index != NONE;
		call->unit_animation_flags = unit->unit.animation.flags;
		call->aiming_screen_index = unit->unit.animation.aiming_screen_index;
		call->looking_screen_index = unit->unit.animation.looking_screen_index;
		call->override_emotion_animation_index = unit->unit.override_emotion_animation_index;
		call->seat_index = unit->unit.animation.seat_index;
		call->weapon_index = unit->unit.animation.weapon_index;
		call->state = unit->unit.animation.state;
		call->action = unit->unit.animation.action;
		call->base_seat_index = unit->unit.animation.base_seat_index;
		call->emotion_index = unit->unit.animation.emotion_index;
		call->action_animation[0] = unit->unit.animation.action_animation.index;
		call->action_animation[1] = unit->unit.animation.action_animation.frame_index;
		call->overlay_action_animation[0] = unit->unit.animation.overlay_action_animation.index;
		call->overlay_action_animation[1] = unit->unit.animation.overlay_action_animation.frame_index;
		call->soft_ping_animation[0] = unit->unit.animation.soft_ping_animation.index;
		call->soft_ping_animation[1] = unit->unit.animation.soft_ping_animation.frame_index;
		call->mouth_aperture = unit->unit.mouth_aperture;
		call->seat_acceleration = unit->unit.seat_acceleration;
		call->aiming_vector = unit->unit.aiming_vector;
		call->looking_vector = unit->unit.looking_vector;
		object_get_orientation(object_index, &call->orientation_forward, &call->orientation_up);
		unit_grenade_throw_leg_weights(object_index, &call->leg_weight, &call->leg_time, &call->air_weight, &call->air_time);
	}
	if (object->object.type == _object_type_vehicle)
	{
		struct vehicle_datum *vehicle = vehicle_datum_get(object_index);

		call->vehicle_turn = vehicle->vehicle.turn;
		call->vehicle_speed = vehicle->vehicle.speed;
		call->vehicle_wheel = vehicle->vehicle.wheel;
		memcpy(call->vehicle_suspension, vehicle->vehicle.suspension, sizeof(call->vehicle_suspension));
	}
	if (TEST_FLAG(_object_mask_device, object->object.type))
	{
		struct device_datum *device = device_get(object_index);

		call->device_flags = device->device.flags;
		call->device_position = device->device.position;
		call->device_power = device->device.power;
	}
}

int halo_sim_pose_preprocess(long object_index, short node_count, void *orientations)
{
	short mode = halo_sim_mode(_halo_sim_poses);
	struct halo_sim_pose_preprocess_call *call = &halo_sim_pose_checks.preprocess;

	halo_sim_pose_checks.preprocess_pending = FALSE;
	if (mode == _halo_sim_mode_c || node_count <= 0 || node_count > MAXIMUM_NODES_PER_MODEL)
		return FALSE;
	halo_sim_pose_preprocess_gather(object_index, node_count, call);
	if (mode == _halo_sim_mode_check)
	{
		memcpy(halo_sim_pose_checks.preprocessed, orientations, sizeof(real_orientation) * node_count);
		call->orientations = halo_sim_pose_checks.preprocessed;
	}
	else
	{
		call->orientations = (real_orientation *)orientations;
	}
	if (halo_sim.pose_preprocess(call) != 0)
	{
		static long failures = 0;

		if (failures++ < 5)
		{
			platform_log("halo_sim: the preprocess of %s (type %d) couldn't run in Rust",
				tag_get_name(object_get(object_index)->definition_index), call->object_type);
		}
		return FALSE;
	}
	if (mode == _halo_sim_mode_check)
	{
		halo_sim_pose_checks.preprocess_pending = TRUE;
		halo_sim_pose_checks.preprocess_object = object_index;
		return FALSE;
	}
	if (TEST_FLAG(_object_mask_unit, call->object_type))
	{
		struct unit_datum *unit = unit_get(object_index);

		unit->unit.animation.aiming_with_euler_screen = call->aiming_with_euler_screen ? TRUE : FALSE;
		unit->unit.animation.looking_with_euler_screen = call->looking_with_euler_screen ? TRUE : FALSE;
		if (call->aiming_bounds_set)
			unit->unit.animation.aiming_screen_bounds = call->aiming_screen_bounds;
		if (call->looking_bounds_set)
			unit->unit.animation.looking_screen_bounds = call->looking_screen_bounds;
		unit_grenade_throw_set_leg_weights(object_index, call->leg_weight, call->leg_time, call->air_weight, call->air_time);
	}

	return TRUE;
}

void halo_sim_pose_preprocess_check(long object_index, short node_count, void const *orientations)
{
	struct halo_sim_pose_preprocess_call const *call = &halo_sim_pose_checks.preprocess;
	real_orientation const *game = (real_orientation const *)orientations;
	short node_index;
	boolean same;

	if (!halo_sim_pose_checks.preprocess_pending || halo_sim_pose_checks.preprocess_object != object_index)
		return;
	halo_sim_pose_checks.preprocess_pending = FALSE;
	for (node_index = 0; node_index < node_count; node_index++)
	{
		if (memcmp(&game[node_index], &halo_sim_pose_checks.preprocessed[node_index], sizeof(real_orientation)) != 0)
			break;
	}
	same = node_index == node_count;
	if (same && TEST_FLAG(_object_mask_unit, call->object_type))
	{
		struct unit_datum *unit = unit_get(object_index);
		real leg_weight, air_weight;
		long leg_time, air_time;

		unit_grenade_throw_leg_weights(object_index, &leg_weight, &leg_time, &air_weight, &air_time);
		same = (unit->unit.animation.aiming_with_euler_screen != 0) == (call->aiming_with_euler_screen != 0) &&
			(unit->unit.animation.looking_with_euler_screen != 0) == (call->looking_with_euler_screen != 0) &&
			(!call->aiming_bounds_set ||
				memcmp(&unit->unit.animation.aiming_screen_bounds, &call->aiming_screen_bounds, sizeof(real_rectangle2d)) == 0) &&
			(!call->looking_bounds_set ||
				memcmp(&unit->unit.animation.looking_screen_bounds, &call->looking_screen_bounds, sizeof(real_rectangle2d)) == 0) &&
			memcmp(&leg_weight, &call->leg_weight, sizeof(real)) == 0 && leg_time == call->leg_time &&
			memcmp(&air_weight, &call->air_weight, sizeof(real)) == 0 && air_time == call->air_time;
	}
	if (!same)
		halo_sim_pose_report(2, object_index, node_count, node_index);
	halo_sim_pose_tally(2);
}

/* ---------- collision */

void halo_sim_collision_test_vector_check(unsigned long flags, void const *point, void const *vector,
	long ignore_object_index, int hit, void const *collision)
{
	struct collision_result const *game = (struct collision_result const *)collision;
	struct halo_sim_collision_vector_call call;
	struct halo_sim_collision_vector_result rust;
	boolean same;

	if (halo_sim_mode(_halo_sim_collision) == _halo_sim_mode_c)
		return;
	call.structure_bsp_index = global_structure_bsp_index;
	call.flags = flags;
	call.breakable_surfaces = (unsigned char const *)breakable_surface_flags_get();
	call.point = *(real_point3d const *)point;
	call.vector = *(real_vector3d const *)vector;
	call.ignore_object_index = (unsigned long)ignore_object_index;
	if (halo_sim.collision_test_vector(&call, &rust) != 0)
		return;

	same = (rust.hit != 0) == (hit != 0) &&
		rust.type == game->type &&
		memcmp(&rust.t, &game->t, sizeof(real)) == 0 &&
		memcmp(&rust.point, &game->point, sizeof(real_point3d)) == 0 &&
		rust.location_leaf == game->location.leaf_index &&
		rust.location_cluster == game->location.cluster_index;
	if (same && hit)
	{
		same = memcmp(&rust.plane, &game->plane, sizeof(real_plane3d)) == 0 &&
			rust.material_type == game->material_type;
		if (game->type == _collision_result_structure || game->type == _collision_result_object)
		{
			same = same && rust.surface_index == game->surface_index &&
				rust.surface_flags == game->flags &&
				rust.material_index == game->material_index &&
				rust.plane_designator == game->plane_designator &&
				rust.breakable_surface_index == game->breakable_surface_index;
		}
		if (game->type == _collision_result_object)
		{
			same = same && rust.object_index == (unsigned long)game->object_index &&
				rust.node_index == game->node_index &&
				rust.bsp_index == game->bsp_index &&
				rust.region_index == game->region_index;
		}
	}
	halo_sim_collision_checks.compared++;
	if (hit)
	{
		if (game->type == _collision_result_structure)
			halo_sim_collision_checks.structure_hits++;
		else if (game->type == _collision_result_media)
			halo_sim_collision_checks.media_hits++;
		else if (game->type == _collision_result_object)
		{
			halo_sim_collision_checks.object_hits++;
			if (game->node_index == NONE)
				halo_sim_collision_checks.vehicle_physics_hits++;
		}
	}
	if (!same)
	{
		halo_sim_collision_checks.differed++;
		if (halo_sim_collision_checks.differed <= 8)
		{
			platform_log("halo_sim: collision differs: flags %lx point %08lx %08lx %08lx vector %08lx %08lx %08lx ignore %lx",
				flags,
				*(unsigned long const *)&call.point.x, *(unsigned long const *)&call.point.y, *(unsigned long const *)&call.point.z,
				*(unsigned long const *)&call.vector.i, *(unsigned long const *)&call.vector.j, *(unsigned long const *)&call.vector.k,
				(unsigned long)ignore_object_index);
			platform_log("halo_sim:   game %d type %d t %08lx object %lx node %d bsp %d surface %ld material %d flags %x leaf %ld",
				hit, game->type, *(unsigned long const *)&game->t,
				hit && game->type == _collision_result_object ? (unsigned long)game->object_index : 0xfffffffful,
				game->node_index, game->bsp_index, game->surface_index, game->material_type, game->flags,
				game->location.leaf_index);
			platform_log("halo_sim:   rust %ld type %d t %08lx object %lx node %d bsp %d surface %ld material %d flags %x leaf %ld",
				rust.hit, rust.type, *(unsigned long const *)&rust.t, rust.object_index,
				rust.node_index, rust.bsp_index, rust.surface_index, rust.material_type, rust.surface_flags,
				rust.location_leaf);
		}
	}
	if ((halo_sim_collision_checks.compared & 0xffff) == 0)
	{
		platform_log("halo_sim: collision: %lu of %lu agree with the game (hits: %lu structure, %lu media, %lu objects, %lu of them vehicles' physics)",
			halo_sim_collision_checks.compared - halo_sim_collision_checks.differed,
			halo_sim_collision_checks.compared,
			halo_sim_collision_checks.structure_hits, halo_sim_collision_checks.media_hits,
			halo_sim_collision_checks.object_hits, halo_sim_collision_checks.vehicle_physics_hits);
	}
}

/* ---------- projectiles */

/* random_math.c's and periodic_functions.c's */
unsigned long get_random_seed(void);
unsigned long *get_global_random_seed_address(void);
short random_direction_table_get(real_vector3d const **table);
unsigned char const *periodic_function_table_get(short function_type);

/* projectiles.c's _periodic_function_wander */
#define HALO_SIM_PERIODIC_FUNCTION_WANDER 10

static void halo_sim_projectile_call_new(long projectile_index, struct halo_sim_projectile_call *call, unsigned long *random_seed)
{
	real_vector3d const *directions = NULL;

	memset(call, 0, sizeof(*call));
	call->projectile_index = (unsigned long)projectile_index;
	call->definition_index = (unsigned long)object_get(projectile_index)->definition_index;
	call->structure_bsp_index = global_structure_bsp_index;
	call->breakable_surfaces = (unsigned char const *)breakable_surface_flags_get();
	halo_sim_projectile_state_load(projectile_index, &call->state);
	call->random_seed = random_seed;
	call->direction_count = random_direction_table_get(&directions);
	call->directions = directions;
	call->wander_table = periodic_function_table_get(HALO_SIM_PERIODIC_FUNCTION_WANDER);
	call->game_time = game_time_get();
	call->difference_event = NONE;
}

int halo_sim_projectile_update(long projectile_index, short *collision_count, float *time_remaining)
{
	short mode = halo_sim_mode(_halo_sim_projectiles);
	struct halo_sim_projectile_call call;
	long result;

	halo_sim_projectile_checks.recording = FALSE;
	if (mode == _halo_sim_mode_c)
		return FALSE;
	if (mode == _halo_sim_mode_check)
	{
		halo_sim_projectile_state_load(projectile_index, &halo_sim_projectile_checks.before);
		halo_sim_projectile_checks.seed = get_random_seed();
		halo_sim_projectile_checks.projectile_index = projectile_index;
		halo_sim_projectile_checks.event_count = 0;
		halo_sim_projectile_checks.overflow = FALSE;
		halo_sim_projectile_checks.recording = TRUE;
		return FALSE;
	}

	halo_sim_projectile_call_new(projectile_index, &call, get_global_random_seed_address());
	halo_sim_projectile_updating = TRUE;
	result = halo_sim.projectile_update(&call);
	halo_sim_projectile_updating = FALSE;
	if (result == -1)
		return FALSE;
	halo_sim_projectile_state_store(projectile_index, &call.state);
	if (result != 0)
	{
		static long stops = 0;

		if (stops++ < 8)
			platform_log("halo_sim: projectile %lx's update stopped for an input it couldn't have", (unsigned long)projectile_index);
	}
	*collision_count = (short)call.collision_count;
	*time_remaining = call.time_remaining;

	return TRUE;
}

unsigned long halo_sim_projectile_seed(void)
{
	return halo_sim_projectile_checks.recording ? get_random_seed() : 0;
}

static struct halo_sim_projectile_event *halo_sim_projectile_event_new(short kind, unsigned long seed_before)
{
	struct halo_sim_projectile_event *event;

	if (!halo_sim_projectile_checks.recording)
		return NULL;
	if (halo_sim_projectile_checks.event_count >= MAXIMUM_HALO_SIM_PROJECTILE_EVENTS)
	{
		halo_sim_projectile_checks.overflow = TRUE;
		return NULL;
	}
	event = &halo_sim_projectile_checks.events[halo_sim_projectile_checks.event_count++];
	memset(event, 0, sizeof(*event));
	event->kind = kind;
	event->seed_before = seed_before;
	event->seed_after = get_random_seed();
	event->object_index = NONE;
	event->material_type = NONE;

	return event;
}

void halo_sim_projectile_event(short kind, unsigned long seed_before, long object_index, short material_type,
	int potential, float scale, float material_effect_scale)
{
	struct halo_sim_projectile_event *event = halo_sim_projectile_event_new(kind, seed_before);

	if (!event)
		return;
	event->object_index = object_index;
	event->material_type = material_type;
	event->potential = potential ? 1 : 0;
	event->scale = scale;
	event->material_effect_scale = material_effect_scale;
}

void halo_sim_projectile_event_guidance(void const *projectile_center, void const *target_center,
	void const *target_center_of_mass, float guidance_scale, long game_time)
{
	struct halo_sim_projectile_event *event = halo_sim_projectile_event_new(_halo_sim_projectile_event_guidance,
		halo_sim_projectile_seed());

	if (!event)
		return;
	event->guidance.projectile_center = *(real_point3d const *)projectile_center;
	event->guidance.target_center = *(real_point3d const *)target_center;
	event->guidance.target_center_of_mass = *(real_point3d const *)target_center_of_mass;
	event->guidance.guidance_scale = guidance_scale;
	event->game_time = game_time;
}

void halo_sim_projectile_event_state(short kind, unsigned long seed_before, long projectile_index)
{
	struct halo_sim_projectile_event *event = halo_sim_projectile_event_new(kind, seed_before);

	if (event)
		halo_sim_projectile_state_load(projectile_index, &event->state);
}

static boolean halo_sim_projectile_states_agree(struct halo_sim_projectile_state const *a, struct halo_sim_projectile_state const *b)
{
	return memcmp(&a->position, &b->position, sizeof(a->position)) == 0 &&
		memcmp(&a->velocity, &b->velocity, sizeof(a->velocity)) == 0 &&
		a->flags == b->flags &&
		a->object_flags == b->object_flags &&
		a->action == b->action &&
		a->hit_material_type == b->hit_material_type &&
		a->ignore_object_index == b->ignore_object_index &&
		a->target_object_index == b->target_object_index &&
		a->parent_object_index == b->parent_object_index &&
		memcmp(&a->detonation_timer, &b->detonation_timer, 9 * sizeof(real)) == 0 &&
		a->location_leaf == b->location_leaf &&
		a->location_cluster == b->location_cluster;
}

void halo_sim_projectile_check(long projectile_index, short collision_count, float time_remaining)
{
	struct halo_sim_projectile_call call;
	struct halo_sim_projectile_state game;
	unsigned long seed;
	boolean same;

	if (!halo_sim_projectile_checks.recording)
		return;
	halo_sim_projectile_checks.recording = FALSE;
	if (halo_sim_projectile_checks.projectile_index != projectile_index || halo_sim_projectile_checks.overflow)
	{
		halo_sim_projectile_checks.skipped++;
		return;
	}

	seed = halo_sim_projectile_checks.seed;
	halo_sim_projectile_call_new(projectile_index, &call, &seed);
	call.state = halo_sim_projectile_checks.before;
	call.events = halo_sim_projectile_checks.events;
	call.event_count = halo_sim_projectile_checks.event_count;
	if (halo_sim.projectile_check(&call) != 0)
	{
		halo_sim_projectile_checks.skipped++;
		return;
	}

	halo_sim_projectile_state_load(projectile_index, &game);
	same = halo_sim_projectile_states_agree(&call.state, &game) &&
		call.collision_count == collision_count &&
		memcmp(&call.time_remaining, &time_remaining, sizeof(real)) == 0 &&
		call.difference_event == NONE &&
		seed == get_random_seed();
	halo_sim_projectile_checks.compared++;
	halo_sim_projectile_checks.collisions += (unsigned long)collision_count;
	halo_sim_projectile_checks.events_compared += halo_sim_projectile_checks.event_count;
	if (!same)
	{
		halo_sim_projectile_checks.differed++;
		if (halo_sim_projectile_checks.differed <= 8)
		{
			struct halo_sim_projectile_state const *b = &halo_sim_projectile_checks.before;

			platform_log("halo_sim: projectile %lx (definition %lx) differs: call %ld of %lu (kind %d) reason %ld; collisions game %d rust %ld; time %08lx %08lx; seed game %08lx rust %08lx",
				(unsigned long)projectile_index, call.definition_index, call.difference_event,
				halo_sim_projectile_checks.event_count,
				call.difference_event >= 0 && call.difference_event < (long)halo_sim_projectile_checks.event_count ?
					halo_sim_projectile_checks.events[call.difference_event].kind : -1,
				call.difference_reason, collision_count, call.collision_count,
				*(unsigned long const *)&time_remaining, *(unsigned long const *)&call.time_remaining,
				get_random_seed(), seed);
			platform_log("halo_sim:   from (%f %f %f) velocity (%f %f %f) flags %lx action %d target %lx",
				b->position.x, b->position.y, b->position.z, b->velocity.i, b->velocity.j, b->velocity.k,
				b->flags, b->action, (unsigned long)b->target_object_index);
			platform_log("halo_sim:   game (%f %f %f) velocity (%f %f %f) flags %lx/%lx action %d leaf %ld parent %lx",
				game.position.x, game.position.y, game.position.z, game.velocity.i, game.velocity.j, game.velocity.k,
				game.flags, game.object_flags, game.action, game.location_leaf, (unsigned long)game.parent_object_index);
			platform_log("halo_sim:   rust (%f %f %f) velocity (%f %f %f) flags %lx/%lx action %d leaf %ld parent %lx",
				call.state.position.x, call.state.position.y, call.state.position.z,
				call.state.velocity.i, call.state.velocity.j, call.state.velocity.k,
				call.state.flags, call.state.object_flags, call.state.action, call.state.location_leaf,
				(unsigned long)call.state.parent_object_index);
		}
	}
	if ((halo_sim_projectile_checks.compared & 0x3ff) == 0 ||
		(halo_sim_projectile_checks.compared & (halo_sim_projectile_checks.compared - 1)) == 0)
	{
		platform_log("halo_sim: projectiles: %lu of %lu updates agree with the game (%lu collisions, %lu calls of the game; %lu left out)",
			halo_sim_projectile_checks.compared - halo_sim_projectile_checks.differed,
			halo_sim_projectile_checks.compared, halo_sim_projectile_checks.collisions,
			halo_sim_projectile_checks.events_compared, halo_sim_projectile_checks.skipped);
	}
}

/* ---------- biped physics */

int halo_sim_biped_physics_update(void *physics, int stand_test, float collision_height_standing,
	float collision_radius)
{
	short mode = halo_sim_mode(_halo_sim_biped_physics);
	struct halo_sim_biped_physics_call call;

	halo_sim_biped_physics_checks.pending = FALSE;
	if (mode == _halo_sim_mode_c)
		return FALSE;
	call.structure_bsp_index = global_structure_bsp_index;
	call.breakable_surfaces = (unsigned char const *)breakable_surface_flags_get();
	call.stand_test = stand_test ? 1 : 0;
	call.collision_height_standing = collision_height_standing;
	call.collision_radius = collision_radius;
	if (mode == _halo_sim_mode_check)
	{
		halo_sim_biped_physics_checks.before = *(struct halo_sim_biped_physics *)physics;
		halo_sim_biped_physics_checks.result = *(struct halo_sim_biped_physics *)physics;
		halo_sim_biped_physics_checks.call = call;
		if (halo_sim.biped_physics(&call, &halo_sim_biped_physics_checks.result) == 0)
			halo_sim_biped_physics_checks.pending = TRUE;
		return FALSE;
	}

	{
		long result;

		halo_sim_biped_physics_updating = TRUE;
		result = halo_sim.biped_physics(&call, physics);
		halo_sim_biped_physics_updating = FALSE;
		return result == 0;
	}
}

void halo_sim_biped_physics_check(void const *physics)
{
	struct halo_sim_biped_physics const *game = (struct halo_sim_biped_physics const *)physics;
	struct halo_sim_biped_physics const *rust = &halo_sim_biped_physics_checks.result;

	if (!halo_sim_biped_physics_checks.pending)
		return;
	halo_sim_biped_physics_checks.pending = FALSE;
	halo_sim_biped_physics_checks.compared++;
	if (memcmp(game, rust, sizeof(struct halo_sim_biped_physics)) != 0)
	{
		halo_sim_biped_physics_checks.differed++;
		if (halo_sim_biped_physics_checks.differed <= 8)
		{
			struct halo_sim_biped_physics const *b = &halo_sim_biped_physics_checks.before;

			platform_log("halo_sim: biped physics differs: biped %lx in %x stand %lu from (%f %f %f) velocity (%f %f %f)",
				(unsigned long)b->biped_index, b->in_flags, halo_sim_biped_physics_checks.call.stand_test,
				b->position.x, b->position.y, b->position.z,
				b->new_velocity.i, b->new_velocity.j, b->new_velocity.k);
			platform_log("halo_sim:   game out %x (%f %f %f) velocity (%f %f %f) support %ld bumped %lx elevator %lx",
				game->out_flags, game->new_position.x, game->new_position.y, game->new_position.z,
				game->new_velocity.i, game->new_velocity.j, game->new_velocity.k,
				game->support_surface_index, (unsigned long)game->bumped_object_index, (unsigned long)game->elevator_object_index);
			platform_log("halo_sim:   rust out %x (%f %f %f) velocity (%f %f %f) support %ld bumped %lx elevator %lx",
				rust->out_flags, rust->new_position.x, rust->new_position.y, rust->new_position.z,
				rust->new_velocity.i, rust->new_velocity.j, rust->new_velocity.k,
				rust->support_surface_index, (unsigned long)rust->bumped_object_index, (unsigned long)rust->elevator_object_index);
		}
	}
	if ((halo_sim_biped_physics_checks.compared & 0x3fff) == 0 ||
		(halo_sim_biped_physics_checks.compared & (halo_sim_biped_physics_checks.compared - 1)) == 0)
	{
		platform_log("halo_sim: biped physics: %lu of %lu agree with the game",
			halo_sim_biped_physics_checks.compared - halo_sim_biped_physics_checks.differed,
			halo_sim_biped_physics_checks.compared);
	}
}

/* ---------- vehicle physics */

static void halo_sim_vehicle_physics_call_new(long vehicle_index, struct halo_sim_vehicle_physics_call *call)
{
	memset(call, 0, sizeof(*call));
	call->vehicle_index = (unsigned long)vehicle_index;
	call->definition_index = (unsigned long)object_get(vehicle_index)->definition_index;
	call->structure_bsp_index = global_structure_bsp_index;
	call->breakable_surfaces = (unsigned char const *)breakable_surface_flags_get();
	halo_sim_vehicle_physics_state_load(vehicle_index, &call->state);
}

int halo_sim_vehicle_physics_update(long vehicle_index, void *powered_mass_points, short powered_count,
	void *mass_points, short mass_point_count, void const *magic_force, void const *magic_torque)
{
	short mode = halo_sim_mode(_halo_sim_vehicle_physics);
	struct halo_sim_vehicle_physics_call call;
	long result;

	halo_sim_vehicle_physics_checks.recording = FALSE;
	/* (the vehicle update check's: what its control code handed the physics) */
	if (halo_sim_vehicle_update_checks.recording && halo_sim_vehicle_update_checks.vehicle_index == vehicle_index)
	{
		short index;

		for (index = 0; powered_mass_points && index < powered_count && index < 8; index++)
			halo_sim_vehicle_update_checks.powered[index] = ((struct powered_mass_point_datum const *)powered_mass_points)[index];
		halo_sim_vehicle_update_checks.powered_count = powered_mass_points ? index : 0;
		if (powered_mass_points && powered_count > 8)
			halo_sim_vehicle_update_checks.overflow = TRUE;
	}
	if (mode == _halo_sim_mode_c)
		return FALSE;
	if (mode == _halo_sim_mode_check)
	{
		short index;

		halo_sim_vehicle_physics_state_load(vehicle_index, &halo_sim_vehicle_physics_checks.before);
		halo_sim_vehicle_physics_checks.vehicle_index = vehicle_index;
		halo_sim_vehicle_physics_checks.has_powered = powered_mass_points != NULL;
		halo_sim_vehicle_physics_checks.powered_count = 0;
		for (index = 0; powered_mass_points && index < powered_count && index < 8; index++)
			halo_sim_vehicle_physics_checks.powered[index] = ((struct powered_mass_point_datum const *)powered_mass_points)[index];
		halo_sim_vehicle_physics_checks.powered_count = index;
		halo_sim_vehicle_physics_checks.has_magic = magic_force && magic_torque;
		if (magic_force && magic_torque)
		{
			halo_sim_vehicle_physics_checks.magic_force = *(real_vector3d const *)magic_force;
			halo_sim_vehicle_physics_checks.magic_torque = *(real_vector3d const *)magic_torque;
		}
		halo_sim_vehicle_physics_checks.other_count = 0;
		halo_sim_vehicle_physics_checks.overflow = powered_mass_points && powered_count > 8;
		halo_sim_vehicle_physics_checks.recording = TRUE;
		return FALSE;
	}

	halo_sim_vehicle_physics_call_new(vehicle_index, &call);
	call.powered = (struct powered_mass_point_datum const *)powered_mass_points;
	call.powered_count = powered_count;
	call.has_magic = magic_force && magic_torque;
	if (magic_force && magic_torque)
	{
		call.magic_force = *(real_vector3d const *)magic_force;
		call.magic_torque = *(real_vector3d const *)magic_torque;
	}
	call.mass_points = (struct mass_point_datum *)mass_points;
	call.mass_point_count = (unsigned long)mass_point_count;
	halo_sim_vehicle_physics_updating = TRUE;
	result = halo_sim.vehicle_physics_update(&call);
	halo_sim_vehicle_physics_updating = FALSE;
	if (result != 0)
		return FALSE;
	halo_sim_vehicle_physics_state_store(vehicle_index, &call.state);

	return TRUE;
}

void halo_sim_vehicle_physics_other(long object_index)
{
	struct halo_sim_other_vehicle *other;
	struct object_datum *object;

	if (halo_sim_vehicle_update_checks.recording)
	{
		if (halo_sim_vehicle_update_checks.other_count >= MAXIMUM_HALO_SIM_OTHER_VEHICLES)
		{
			halo_sim_vehicle_update_checks.overflow = TRUE;
		}
		else
		{
			other = &halo_sim_vehicle_update_checks.others[halo_sim_vehicle_update_checks.other_count++];
			object = object_get(object_index);
			other->object_index = (unsigned long)object_index;
			other->definition_index = (unsigned long)object->definition_index;
			other->at_rest = TEST_FLAG(object->object.flags, _object_at_rest_bit) ? 1 : 0;
			object_get_origin(object_index, &other->origin);
			object_get_orientation(object_index, &other->forward, &other->up);
		}
	}
	if (!halo_sim_vehicle_physics_checks.recording)
		return;
	if (halo_sim_vehicle_physics_checks.other_count >= MAXIMUM_HALO_SIM_OTHER_VEHICLES)
	{
		halo_sim_vehicle_physics_checks.overflow = TRUE;
		return;
	}
	other = &halo_sim_vehicle_physics_checks.others[halo_sim_vehicle_physics_checks.other_count++];
	object = object_get(object_index);
	other->object_index = (unsigned long)object_index;
	other->definition_index = (unsigned long)object->definition_index;
	other->at_rest = TEST_FLAG(object->object.flags, _object_at_rest_bit) ? 1 : 0;
	object_get_origin(object_index, &other->origin);
	object_get_orientation(object_index, &other->forward, &other->up);
}

void halo_sim_vehicle_physics_check(long vehicle_index, void const *mass_points, short mass_point_count)
{
	static struct mass_point_datum rust_mass_points[32];
	struct halo_sim_vehicle_physics_call call;
	struct halo_sim_vehicle_physics_state game;
	struct halo_sim_vehicle_physics_state const *rust = &call.state;
	boolean same;

	if (!halo_sim_vehicle_physics_checks.recording)
		return;
	halo_sim_vehicle_physics_checks.recording = FALSE;
	if (halo_sim_vehicle_physics_checks.vehicle_index != vehicle_index || halo_sim_vehicle_physics_checks.overflow ||
		mass_point_count > 32)
	{
		halo_sim_vehicle_physics_checks.skipped++;
		return;
	}

	halo_sim_vehicle_physics_call_new(vehicle_index, &call);
	call.state = halo_sim_vehicle_physics_checks.before;
	call.powered = halo_sim_vehicle_physics_checks.has_powered ? halo_sim_vehicle_physics_checks.powered : NULL;
	call.powered_count = halo_sim_vehicle_physics_checks.powered_count;
	call.has_magic = halo_sim_vehicle_physics_checks.has_magic;
	call.magic_force = halo_sim_vehicle_physics_checks.magic_force;
	call.magic_torque = halo_sim_vehicle_physics_checks.magic_torque;
	call.bounding_center_after = object_get(vehicle_index)->object.bounding_sphere_center;
	call.others = halo_sim_vehicle_physics_checks.others;
	call.other_count = halo_sim_vehicle_physics_checks.other_count;
	call.mass_points = rust_mass_points;
	call.mass_point_count = (unsigned long)mass_point_count;
	memset(rust_mass_points, 0, sizeof(rust_mass_points));
	if (halo_sim.vehicle_physics_check(&call) != 0)
	{
		halo_sim_vehicle_physics_checks.skipped++;
		return;
	}

	halo_sim_vehicle_physics_state_load(vehicle_index, &game);
	same = memcmp(&rust->position, &game.position, 5 * sizeof(real_vector3d)) == 0 &&
		rust->object_flags == game.object_flags &&
		rust->location_leaf == game.location_leaf &&
		rust->location_cluster == game.location_cluster &&
		memcmp(&rust->collision_force, &game.collision_force, 2 * sizeof(real_vector3d)) == 0 &&
		rust->stuck_mass_point_flags == game.stuck_mass_point_flags &&
		(!mass_points || memcmp(rust_mass_points, mass_points, mass_point_count * sizeof(struct mass_point_datum)) == 0);
	halo_sim_vehicle_physics_checks.compared++;
	halo_sim_vehicle_physics_checks.collided += halo_sim_vehicle_physics_checks.other_count > 0;
	if (!same)
	{
		halo_sim_vehicle_physics_checks.differed++;
		if (halo_sim_vehicle_physics_checks.differed <= 8)
		{
			struct halo_sim_vehicle_physics_state const *b = &halo_sim_vehicle_physics_checks.before;
			short index = 0;

			while (mass_points && index < mass_point_count &&
				memcmp(&rust_mass_points[index], (struct mass_point_datum const *)mass_points + index, sizeof(struct mass_point_datum)) == 0)
			{
				index++;
			}
			platform_log("halo_sim: vehicle physics differs: vehicle %lx from (%f %f %f) velocity (%f %f %f) flags %lx; first differing mass point %d of %d; %lu others",
				(unsigned long)vehicle_index, b->position.x, b->position.y, b->position.z,
				b->translational_velocity.i, b->translational_velocity.j, b->translational_velocity.k, b->object_flags,
				index, mass_point_count, halo_sim_vehicle_physics_checks.other_count);
			platform_log("halo_sim:   game (%f %f %f) velocity (%f %f %f) flags %lx leaf %ld force (%f %f %f) stuck %lx",
				game.position.x, game.position.y, game.position.z,
				game.translational_velocity.i, game.translational_velocity.j, game.translational_velocity.k,
				game.object_flags, game.location_leaf, game.collision_force.i, game.collision_force.j, game.collision_force.k,
				game.stuck_mass_point_flags);
			platform_log("halo_sim:   rust (%f %f %f) velocity (%f %f %f) flags %lx leaf %ld force (%f %f %f) stuck %lx",
				rust->position.x, rust->position.y, rust->position.z,
				rust->translational_velocity.i, rust->translational_velocity.j, rust->translational_velocity.k,
				rust->object_flags, rust->location_leaf, rust->collision_force.i, rust->collision_force.j, rust->collision_force.k,
				rust->stuck_mass_point_flags);
		}
	}
	if ((halo_sim_vehicle_physics_checks.compared & 0x3fff) == 0 ||
		(halo_sim_vehicle_physics_checks.compared & (halo_sim_vehicle_physics_checks.compared - 1)) == 0)
	{
		platform_log("halo_sim: vehicle physics: %lu of %lu agree with the game (%lu meeting other vehicles; %lu left out)",
			halo_sim_vehicle_physics_checks.compared - halo_sim_vehicle_physics_checks.differed,
			halo_sim_vehicle_physics_checks.compared,
			halo_sim_vehicle_physics_checks.collided, halo_sim_vehicle_physics_checks.skipped);
	}
}

/* ---------- vehicle updates */

boolean unit_driven_by_ai(long unit_index); /* units.c */

static void halo_sim_vehicle_update_call_new(long vehicle_index, struct halo_sim_vehicle_update_call *call)
{
	struct vehicle_datum *vehicle = vehicle_datum_get(vehicle_index);

	memset(call, 0, sizeof(*call));
	call->vehicle_index = (unsigned long)vehicle_index;
	call->definition_index = (unsigned long)vehicle->definition_index;
	call->structure_bsp_index = global_structure_bsp_index;
	call->breakable_surfaces = (unsigned char const *)breakable_surface_flags_get();
	halo_sim_vehicle_update_state_load(vehicle_index, &call->state);
	call->throttle = vehicle->unit.throttle;
	call->control_flags = vehicle->unit.control_flags;
	call->desired_facing = vehicle->unit.desired_facing_vector;
	call->seat_power[0] = vehicle->unit.seat_power[0];
	call->seat_power[1] = vehicle->unit.seat_power[1];
	call->driven_by_ai = unit_driven_by_ai(vehicle_index) ? 1 : 0;
	call->vehicle_floor = global_structure_bsp_get()->vehicle_floor;
	call->vehicle_ceiling = global_structure_bsp_get()->vehicle_ceiling;
}

int halo_sim_vehicle_update(long vehicle_index)
{
	short mode = halo_sim_mode(_halo_sim_vehicle_update);
	struct halo_sim_vehicle_update_call call;
	long result;

	halo_sim_vehicle_update_checks.recording = FALSE;
	if (mode == _halo_sim_mode_c)
		return FALSE;
	if (mode == _halo_sim_mode_check)
	{
		halo_sim_vehicle_update_call_new(vehicle_index, &halo_sim_vehicle_update_checks.call);
		halo_sim_vehicle_update_checks.vehicle_index = vehicle_index;
		halo_sim_vehicle_update_checks.powered_count = 0;
		halo_sim_vehicle_update_checks.other_count = 0;
		halo_sim_vehicle_update_checks.overflow = FALSE;
		halo_sim_vehicle_update_checks.recording = TRUE;
		return FALSE;
	}

	halo_sim_vehicle_update_call_new(vehicle_index, &call);
	halo_sim_vehicle_physics_updating = TRUE;
	halo_sim_vehicle_update_running = TRUE;
	result = halo_sim.vehicle_update(&call);
	halo_sim_vehicle_update_running = FALSE;
	halo_sim_vehicle_physics_updating = FALSE;
	if (result != 0)
		return FALSE;
	halo_sim_vehicle_update_state_store(vehicle_index, &call.state);

	return TRUE;
}

void halo_sim_vehicle_update_check(long vehicle_index)
{
	struct halo_sim_vehicle_update_call call;
	struct halo_sim_vehicle_update_state game;
	struct halo_sim_vehicle_update_state const *rust = &call.state;
	boolean same;

	if (!halo_sim_vehicle_update_checks.recording)
		return;
	halo_sim_vehicle_update_checks.recording = FALSE;
	if (halo_sim_vehicle_update_checks.vehicle_index != vehicle_index || halo_sim_vehicle_update_checks.overflow)
	{
		halo_sim_vehicle_update_checks.skipped++;
		return;
	}

	call = halo_sim_vehicle_update_checks.call;
	call.breakable_surfaces = (unsigned char const *)breakable_surface_flags_get();
	call.bounding_center_after = object_get(vehicle_index)->object.bounding_sphere_center;
	call.powered_base = halo_sim_vehicle_update_checks.powered;
	call.powered_base_count = halo_sim_vehicle_update_checks.powered_count;
	call.others = halo_sim_vehicle_update_checks.others;
	call.other_count = halo_sim_vehicle_update_checks.other_count;
	if (halo_sim.vehicle_update_check(&call) != 0)
	{
		halo_sim_vehicle_update_checks.skipped++;
		return;
	}

	halo_sim_vehicle_update_state_load(vehicle_index, &game);
	same = memcmp(&rust->position, &game.position, 5 * sizeof(real_vector3d)) == 0 &&
		rust->object_flags == game.object_flags &&
		rust->location_leaf == game.location_leaf &&
		rust->location_cluster == game.location_cluster &&
		memcmp(&rust->vehicle, &game.vehicle, sizeof(game.vehicle)) == 0;
	halo_sim_vehicle_update_checks.compared++;
	if (!same)
	{
		halo_sim_vehicle_update_checks.differed++;
		if (halo_sim_vehicle_update_checks.differed <= 8)
		{
			struct halo_sim_vehicle_update_state const *b = &halo_sim_vehicle_update_checks.call.state;

			platform_log("halo_sim: vehicle update differs: vehicle %lx from (%f %f %f) velocity (%f %f %f) flags %lx speed %f turn %f; %ld powered, %lu others",
				(unsigned long)vehicle_index, b->position.x, b->position.y, b->position.z,
				b->translational_velocity.i, b->translational_velocity.j, b->translational_velocity.k, b->object_flags,
				b->vehicle.speed, b->vehicle.turn, halo_sim_vehicle_update_checks.powered_count,
				halo_sim_vehicle_update_checks.other_count);
			platform_log("halo_sim:   game (%f %f %f) velocity (%f %f %f) flags %lx leaf %ld speed %f turn %f stop %d airborne %d",
				game.position.x, game.position.y, game.position.z,
				game.translational_velocity.i, game.translational_velocity.j, game.translational_velocity.k,
				game.object_flags, game.location_leaf, game.vehicle.speed, game.vehicle.turn, game.vehicle.stop_time,
				game.vehicle.airborne_ticks);
			platform_log("halo_sim:   rust (%f %f %f) velocity (%f %f %f) flags %lx leaf %ld speed %f turn %f stop %d airborne %d",
				rust->position.x, rust->position.y, rust->position.z,
				rust->translational_velocity.i, rust->translational_velocity.j, rust->translational_velocity.k,
				rust->object_flags, rust->location_leaf, rust->vehicle.speed, rust->vehicle.turn, rust->vehicle.stop_time,
				rust->vehicle.airborne_ticks);
		}
	}
	if ((halo_sim_vehicle_update_checks.compared & 0x3fff) == 0 ||
		(halo_sim_vehicle_update_checks.compared & (halo_sim_vehicle_update_checks.compared - 1)) == 0)
	{
		platform_log("halo_sim: vehicle updates: %lu of %lu agree with the game (%lu left out)",
			halo_sim_vehicle_update_checks.compared - halo_sim_vehicle_update_checks.differed,
			halo_sim_vehicle_update_checks.compared, halo_sim_vehicle_update_checks.skipped);
	}
}

/* ---------- biped movement */

static void halo_sim_moving_call_new(long biped_index, struct unit_animation_update_data const *animation,
	float const *aiming_vector, float difficulty_infection_forms, float speed_multiplier, int cinematic, int double_speed,
	int coop_player_collisions, int actor_leaping, int recorded_animation, struct halo_sim_moving_call *call)
{
	struct biped_datum *biped = biped_get(biped_index);

	memset(call, 0, sizeof(*call));
	call->biped_index = (unsigned long)biped_index;
	call->definition_index = (unsigned long)biped->definition_index;
	call->structure_bsp_index = global_structure_bsp_index;
	call->breakable_surfaces = (unsigned char const *)breakable_surface_flags_get();
	halo_sim_moving_state_load(biped_index, animation, &call->state);
	call->dead = TEST_FLAG(biped->object.damage_flags, _object_dead_bit) ? 1 : 0;
	call->ignore_translation = TEST_FLAG(biped->unit.animation.flags, _unit_animation_ignore_translation_bit) ? 1 : 0;
	call->suspended = TEST_FLAG(biped->unit.flags, _unit_suspended_bit) ? 1 : 0;
	call->control_flags = biped->unit.control_flags;
	call->unit_animation_state = biped->unit.animation.state;
	call->base_seat_index = biped->unit.animation.base_seat_index;
	call->aiming_speed = biped->unit.aiming_speed;
	call->actor = (biped->unit.actor_index != NONE || biped->unit.swarm_actor_index != NONE) ? 1 : 0;
	call->player_index = biped->unit.player_index;
	call->throttle = biped->unit.throttle;
	call->body_stun = biped->unit.body_stun;
	call->recent_body_damage = biped->object.recent_body_damage;
	call->desired_facing_vector = biped->unit.desired_facing_vector;
	call->aiming_vector.i = aiming_vector[0];
	call->aiming_vector.j = aiming_vector[1];
	call->aiming_vector.k = aiming_vector[2];
	call->bounding_center = biped->object.bounding_sphere_center;
	call->animation_graph_index = biped->object.animation.animation_graph_index;
	call->animation_index = biped->object.animation.state.index;
	call->animation_frame_index = biped->object.animation.state.frame_index;
	call->difficulty_infection_forms = difficulty_infection_forms;
	call->speed_multiplier = speed_multiplier;
	call->cinematic = cinematic ? 1 : 0;
	call->double_speed = double_speed ? 1 : 0;
	call->coop_player_collisions = coop_player_collisions ? 1 : 0;
	call->actor_leaping = actor_leaping ? 1 : 0;
	call->recorded_animation = recorded_animation ? 1 : 0;
}

int halo_sim_biped_moving_update(long biped_index, void *animation, float const *aiming_vector,
	float difficulty_infection_forms, float speed_multiplier, int cinematic, int double_speed,
	int coop_player_collisions, int actor_leaping, int recorded_animation)
{
	short mode = halo_sim_mode(_halo_sim_biped_moving);
	struct unit_animation_update_data *data = (struct unit_animation_update_data *)animation;
	struct halo_sim_moving_call call;
	long result;

	halo_sim_moving_checks.recording = FALSE;
	if (mode == _halo_sim_mode_c)
		return FALSE;
	halo_sim_moving_call_new(biped_index, data, aiming_vector, difficulty_infection_forms, speed_multiplier, cinematic,
		double_speed, coop_player_collisions, actor_leaping, recorded_animation,
		mode == _halo_sim_mode_check ? &halo_sim_moving_checks.call : &call);
	if (mode == _halo_sim_mode_check)
	{
		halo_sim_moving_checks.biped_index = biped_index;
		halo_sim_moving_checks.recording = TRUE;
		return FALSE;
	}

	halo_sim_moving_animation = data;
	halo_sim_biped_physics_updating = TRUE;
	result = halo_sim.biped_moving(&call);
	halo_sim_biped_physics_updating = FALSE;
	halo_sim_moving_animation = NULL;
	if (result != 0)
		return FALSE;
	halo_sim_moving_state_store(biped_index, data, &call.state);

	return TRUE;
}

void halo_sim_biped_actor_jump(int jumped, void const *velocity)
{
	if (halo_sim_update_checks.recording)
	{
		halo_sim_update_checks.call.moving.actor_jump_asked = 1;
		halo_sim_update_checks.call.moving.actor_jumped = jumped ? 1 : 0;
		halo_sim_update_checks.call.moving.actor_jump_velocity = *(real_vector3d const *)velocity;
	}
	if (!halo_sim_moving_checks.recording)
		return;
	halo_sim_moving_checks.call.actor_jump_asked = 1;
	halo_sim_moving_checks.call.actor_jumped = jumped ? 1 : 0;
	halo_sim_moving_checks.call.actor_jump_velocity = *(real_vector3d const *)velocity;
}

static boolean halo_sim_moving_states_agree(struct halo_sim_moving_state const *a, struct halo_sim_moving_state const *b)
{
	return memcmp(&a->position, &b->position, 5 * sizeof(real_vector3d)) == 0 &&
		a->object_flags == b->object_flags &&
		a->location_leaf == b->location_leaf &&
		a->location_cluster == b->location_cluster &&
		a->landing == b->landing &&
		a->biped_flags == b->biped_flags &&
		memcmp(&a->landing_recovery_counter, &b->landing_recovery_counter, 8) == 0 &&
		a->elevator_object_index == b->elevator_object_index &&
		a->support_surface_index == b->support_surface_index &&
		a->pathfinding_surface_index == b->pathfinding_surface_index &&
		memcmp(&a->pathfinding_point, &b->pathfinding_point, sizeof(a->pathfinding_point)) == 0 &&
		a->bump_object_index == b->bump_object_index &&
		memcmp(&a->crouch, &b->crouch, 2 * sizeof(real) + sizeof(real_plane3d)) == 0;
}

void halo_sim_biped_moving_check(long biped_index, void const *animation)
{
	struct halo_sim_moving_call call;
	struct halo_sim_moving_state game;
	struct halo_sim_moving_state const *rust = &call.state;

	if (!halo_sim_moving_checks.recording)
		return;
	halo_sim_moving_checks.recording = FALSE;
	if (halo_sim_moving_checks.biped_index != biped_index)
	{
		halo_sim_moving_checks.skipped++;
		return;
	}
	call = halo_sim_moving_checks.call;
	call.breakable_surfaces = (unsigned char const *)breakable_surface_flags_get();
	if (halo_sim.biped_moving_check(&call) != 0)
	{
		halo_sim_moving_checks.skipped++;
		return;
	}

	halo_sim_moving_state_load(biped_index, (struct unit_animation_update_data const *)animation, &game);
	halo_sim_moving_checks.compared++;
	halo_sim_moving_checks.aborted += call.aborted_animation;
	halo_sim_moving_checks.jumps += call.actor_jump_asked;
	if (!halo_sim_moving_states_agree(rust, &game))
	{
		halo_sim_moving_checks.differed++;
		if (halo_sim_moving_checks.differed <= 8)
		{
			struct halo_sim_moving_state const *b = &halo_sim_moving_checks.call.state;

			platform_log("halo_sim: biped movement differs: biped %lx from (%f %f %f) velocity (%f %f %f) flags %lx/%lx; animation %d frame %d",
				(unsigned long)biped_index, b->position.x, b->position.y, b->position.z,
				b->translational_velocity.i, b->translational_velocity.j, b->translational_velocity.k,
				b->object_flags, b->biped_flags, halo_sim_moving_checks.call.animation_index,
				halo_sim_moving_checks.call.animation_frame_index);
			platform_log("halo_sim:   game (%f %f %f) velocity (%f %f %f) flags %lx/%lx leaf %ld landing %d bump %d/%lx elevator %lx",
				game.position.x, game.position.y, game.position.z,
				game.translational_velocity.i, game.translational_velocity.j, game.translational_velocity.k,
				game.object_flags, game.biped_flags, game.location_leaf, game.landing, game.bump_ticks,
				(unsigned long)game.bump_object_index, (unsigned long)game.elevator_object_index);
			platform_log("halo_sim:   rust (%f %f %f) velocity (%f %f %f) flags %lx/%lx leaf %ld landing %d bump %d/%lx elevator %lx",
				rust->position.x, rust->position.y, rust->position.z,
				rust->translational_velocity.i, rust->translational_velocity.j, rust->translational_velocity.k,
				rust->object_flags, rust->biped_flags, rust->location_leaf, rust->landing, rust->bump_ticks,
				(unsigned long)rust->bump_object_index, (unsigned long)rust->elevator_object_index);
		}
	}
	if ((halo_sim_moving_checks.compared & 0x3fff) == 0 ||
		(halo_sim_moving_checks.compared & (halo_sim_moving_checks.compared - 1)) == 0)
	{
		platform_log("halo_sim: biped movement: %lu of %lu agree with the game (%lu aborting the animation, %lu asking the AI to jump; %lu left out)",
			halo_sim_moving_checks.compared - halo_sim_moving_checks.differed, halo_sim_moving_checks.compared,
			halo_sim_moving_checks.aborted, halo_sim_moving_checks.jumps, halo_sim_moving_checks.skipped);
	}
}

/* ---------- biped updates */

static void halo_sim_update_call_new(long biped_index, struct unit_animation_update_data const *animation,
	float const *aiming_vector, float difficulty_infection_forms, float speed_multiplier, int cinematic, int double_speed,
	int coop_player_collisions, int actor_leaping, int recorded_animation, int enhanced_animation,
	struct halo_sim_update_call *call)
{
	struct biped_datum *biped = biped_get(biped_index);

	memset(call, 0, sizeof(*call));
	halo_sim_moving_call_new(biped_index, animation, aiming_vector, difficulty_infection_forms, speed_multiplier, cinematic,
		double_speed, coop_player_collisions, actor_leaping, recorded_animation, &call->moving);
	halo_sim_update_state_load(biped_index, animation, &call->state);
	call->dead = TEST_FLAG(biped->object.damage_flags, _object_dead_bit) ? 1 : 0;
	call->control_flags = biped->unit.control_flags;
	call->aim_without_turning = TEST_FLAG(biped->unit.flags, _unit_aim_without_turning_bit) ? 1 : 0;
	call->unit_animation_state = biped->unit.animation.state;
	call->base_seat_index = biped->unit.animation.base_seat_index;
	call->enhanced_animation = enhanced_animation ? 1 : 0;
	call->limp_body = TEST_FLAG(biped->biped.flags, _biped_limp_body_physics_active_bit) &&
		biped->biped.limp_body_current_relaxation_iterations < biped->biped.limp_body_max_relaxation_iterations;
	call->player_index = biped->unit.player_index;
}

int halo_sim_biped_update(long biped_index, void *animation, float const *aiming_vector,
	float difficulty_infection_forms, float speed_multiplier, int cinematic, int double_speed,
	int coop_player_collisions, int actor_leaping, int recorded_animation, int enhanced_animation)
{
	short mode = halo_sim_mode(_halo_sim_biped_update);
	struct unit_animation_update_data *data = (struct unit_animation_update_data *)animation;
	struct halo_sim_update_call call;
	long result;

	halo_sim_update_checks.recording = FALSE;
	if (mode == _halo_sim_mode_c)
		return FALSE;
	halo_sim_update_call_new(biped_index, data, aiming_vector, difficulty_infection_forms, speed_multiplier, cinematic,
		double_speed, coop_player_collisions, actor_leaping, recorded_animation, enhanced_animation,
		mode == _halo_sim_mode_check ? &halo_sim_update_checks.call : &call);
	if (mode == _halo_sim_mode_check)
	{
		halo_sim_update_checks.biped_index = biped_index;
		halo_sim_update_checks.recording = TRUE;
		return FALSE;
	}

	halo_sim_moving_animation = data;
	halo_sim_update_moving_call = &call.moving;
	halo_sim_biped_physics_updating = TRUE;
	result = halo_sim.biped_update(&call);
	halo_sim_biped_physics_updating = FALSE;
	halo_sim_update_moving_call = NULL;
	halo_sim_moving_animation = NULL;
	if (result != 0)
		return FALSE;
	halo_sim_update_state_store(biped_index, data, &call.state);

	return TRUE;
}

void halo_sim_biped_update_airborne(void)
{
	if (!halo_sim_update_checks.recording || halo_sim_update_checks.call.airborne_seed_taken)
		return;
	halo_sim_update_checks.call.airborne_seed_taken = 1;
	halo_sim_update_checks.call.airborne_seed = get_random_seed();
}

void halo_sim_biped_update_check(long biped_index, void const *animation)
{
	struct halo_sim_update_call call;
	struct halo_sim_update_state game;
	struct halo_sim_update_state const *rust = &call.state;
	boolean same;

	if (!halo_sim_update_checks.recording)
		return;
	halo_sim_update_checks.recording = FALSE;
	if (halo_sim_update_checks.biped_index != biped_index)
	{
		halo_sim_update_checks.skipped++;
		return;
	}
	call = halo_sim_update_checks.call;
	call.moving.breakable_surfaces = (unsigned char const *)breakable_surface_flags_get();
	if (halo_sim.biped_update_check(&call) != 0)
	{
		halo_sim_update_checks.skipped++;
		return;
	}

	halo_sim_update_state_load(biped_index, (struct unit_animation_update_data const *)animation, &game);
	same = halo_sim_moving_states_agree(&rust->moving, &game.moving) &&
		rust->moving.animation_state_desired == game.moving.animation_state_desired &&
		rust->moving.animation_crouching == game.moving.animation_crouching &&
		memcmp(&rust->throttle, &game.throttle, 2 * sizeof(real_vector3d)) == 0 &&
		rust->biped_state == game.biped_state &&
		rust->slipping_ticks == game.slipping_ticks;
	halo_sim_update_checks.compared++;
	halo_sim_update_checks.dead += halo_sim_update_checks.call.dead;
	halo_sim_update_checks.airborne += (game.moving.biped_flags & 1) != 0;
	halo_sim_update_checks.tumbles += halo_sim_update_checks.call.airborne_seed_taken;
	if (!same)
	{
		halo_sim_update_checks.differed++;
		if (halo_sim_update_checks.differed <= 8)
		{
			struct halo_sim_update_state const *b = &halo_sim_update_checks.call.state;

			platform_log("halo_sim: biped update differs: biped %lx dead %lu from (%f %f %f) forward (%f %f %f) flags %lx/%lx state %d",
				(unsigned long)biped_index, halo_sim_update_checks.call.dead, b->moving.position.x, b->moving.position.y,
				b->moving.position.z, b->moving.forward.i, b->moving.forward.j, b->moving.forward.k,
				b->moving.object_flags, b->moving.biped_flags, halo_sim_update_checks.call.unit_animation_state);
			platform_log("halo_sim:   game (%f %f %f) forward (%f %f %f) angular (%f %f %f) desired %d crouch %d flags %lx/%lx",
				game.moving.position.x, game.moving.position.y, game.moving.position.z,
				game.moving.forward.i, game.moving.forward.j, game.moving.forward.k,
				game.moving.angular_velocity.i, game.moving.angular_velocity.j, game.moving.angular_velocity.k,
				game.moving.animation_state_desired, game.moving.animation_crouching,
				game.moving.object_flags, game.moving.biped_flags);
			platform_log("halo_sim:   rust (%f %f %f) forward (%f %f %f) angular (%f %f %f) desired %d crouch %d flags %lx/%lx",
				rust->moving.position.x, rust->moving.position.y, rust->moving.position.z,
				rust->moving.forward.i, rust->moving.forward.j, rust->moving.forward.k,
				rust->moving.angular_velocity.i, rust->moving.angular_velocity.j, rust->moving.angular_velocity.k,
				rust->moving.animation_state_desired, rust->moving.animation_crouching,
				rust->moving.object_flags, rust->moving.biped_flags);
		}
	}
	if ((halo_sim_update_checks.compared & 0x3fff) == 0 ||
		(halo_sim_update_checks.compared & (halo_sim_update_checks.compared - 1)) == 0)
	{
		platform_log("halo_sim: biped updates: %lu of %lu agree with the game (%lu dead, %lu ending airborne, %lu with an airborne update; %lu left out)",
			halo_sim_update_checks.compared - halo_sim_update_checks.differed, halo_sim_update_checks.compared,
			halo_sim_update_checks.dead, halo_sim_update_checks.airborne, halo_sim_update_checks.tumbles,
			halo_sim_update_checks.skipped);
	}
}

/* ---------- unit animation */

static void halo_sim_unit_animation_call_new(long unit_index, struct unit_animation_update_data const *data,
	char const *weapon_label, short magic_base_seat_index, long unit_graph_index, int destroyed_after_dying,
	int has_no_dying_airborne, struct halo_sim_unit_animation_call *call)
{
	struct unit_datum *unit = unit_get(unit_index);

	memset(call, 0, sizeof(*call));
	call->unit_index = (unsigned long)unit_index;
	call->unit_graph_index = unit_graph_index;
	halo_sim_unit_animation_state_load(unit_index, &call->state);
	call->state_desired = data->state_desired;
	call->crouching = data->crouching ? 1 : 0;
	call->desired_state = unit->unit.animation.desired_state;
	call->magic_seat_index = unit->unit.magic_seat_index;
	call->dead = TEST_FLAG(unit->object.damage_flags, _object_dead_bit) ? 1 : 0;
	call->parented = unit->object.parent_object_index != NONE;
	call->player = unit->unit.player_index != NONE;
	call->force_alert = TEST_FLAG(unit->unit.control_flags, _unit_control_force_alert_bit) ? 1 : 0;
	call->flaming = unit->unit.flaming_death_delay > 0;
	call->magic_base_seat_index = magic_base_seat_index;
	call->at_rest = TEST_FLAG(unit->object.flags, _object_at_rest_bit) ? 1 : 0;
	call->biped = unit->object.type == _object_type_biped;
	call->airborne = call->biped && TEST_FLAG(biped_get(unit_index)->biped.flags, _biped_airborne_bit) ? 1 : 0;
	call->destroyed_after_dying = destroyed_after_dying ? 1 : 0;
	call->has_no_dying_airborne = has_no_dying_airborne ? 1 : 0;
	call->weapon_label = weapon_label;
	call->random_seed = get_random_seed();
}

int halo_sim_unit_animation_update(long unit_index, void *animation, char const *weapon_label,
	short magic_base_seat_index, long unit_graph_index, int destroyed_after_dying, int has_no_dying_airborne,
	short *result)
{
	short mode = halo_sim_mode(_halo_sim_unit_animation);
	struct unit_animation_update_data *data = (struct unit_animation_update_data *)animation;
	struct halo_sim_unit_animation_call call;

	halo_sim_unit_animation_checks.recording = FALSE;
	if (mode == _halo_sim_mode_c)
		return FALSE;
	if (mode == _halo_sim_mode_check)
	{
		struct halo_sim_unit_animation_call *recorded = &halo_sim_unit_animation_checks.call;

		halo_sim_unit_animation_call_new(unit_index, data, weapon_label, magic_base_seat_index, unit_graph_index,
			destroyed_after_dying, has_no_dying_airborne, recorded);
		/* (the label kept: the game's may change) */
		halo_sim_unit_animation_checks.weapon_label[0] = 0;
		if (weapon_label)
		{
			strncpy(halo_sim_unit_animation_checks.weapon_label, weapon_label,
				sizeof(halo_sim_unit_animation_checks.weapon_label) - 1);
			halo_sim_unit_animation_checks.weapon_label[sizeof(halo_sim_unit_animation_checks.weapon_label) - 1] = 0;
		}
		recorded->weapon_label = weapon_label ? halo_sim_unit_animation_checks.weapon_label : NULL;
		halo_sim_unit_animation_checks.unit_index = unit_index;
		halo_sim_unit_animation_checks.recording = TRUE;
		return FALSE;
	}

	halo_sim_unit_animation_call_new(unit_index, data, weapon_label, magic_base_seat_index, unit_graph_index,
		destroyed_after_dying, has_no_dying_airborne, &call);
	if (halo_sim.unit_animation(&call) != 0)
		return FALSE;
	/* (a destroyed unit is gone) */
	if (object_try_and_get_and_verify_type(unit_index, _object_mask_unit))
		halo_sim_unit_animation_state_store(unit_index, &call.state);
	*result = call.result;

	return TRUE;
}

void halo_sim_unit_animation_check(long unit_index, short result)
{
	struct halo_sim_unit_animation_call call;
	struct halo_sim_unit_animation_state game;

	if (!halo_sim_unit_animation_checks.recording)
		return;
	halo_sim_unit_animation_checks.recording = FALSE;
	if (halo_sim_unit_animation_checks.unit_index != unit_index ||
		!object_try_and_get_and_verify_type(unit_index, _object_mask_unit))
	{
		halo_sim_unit_animation_checks.skipped++;
		return;
	}
	call = halo_sim_unit_animation_checks.call;
	if (halo_sim.unit_animation_check(&call) != 0)
	{
		halo_sim_unit_animation_checks.skipped++;
		return;
	}

	halo_sim_unit_animation_state_load(unit_index, &game);
	halo_sim_unit_animation_checks.compared++;
	halo_sim_unit_animation_checks.effects += call.effects != 0;
	if (memcmp(&call.state, &game, sizeof(game)) != 0 || call.result != result)
	{
		halo_sim_unit_animation_checks.differed++;
		if (halo_sim_unit_animation_checks.differed <= 8)
		{
			struct halo_sim_unit_animation_state const *b = &halo_sim_unit_animation_checks.call.state;

			platform_log("halo_sim: unit animation differs: unit %lx state %d desired %d (seat %d weapon %d animation %d/%d) effects %lx result game %d rust %d",
				(unsigned long)unit_index, b->state, halo_sim_unit_animation_checks.call.state_desired, b->seat_index,
				b->weapon_index, b->object_animation_index, b->object_animation_frame_index, call.effects, result, call.result);
			platform_log("halo_sim:   game state %d seat %d/%d animation %d/%d action %d/%d overlay %d/%d ping %d/%d interpolation %d/%d flags %x",
				game.state, game.seat_index, game.base_seat_index, game.object_animation_index, game.object_animation_frame_index,
				game.action_animation_index, game.action_animation_frame_index, game.overlay_action_animation_index,
				game.overlay_action_animation_frame_index, game.soft_ping_animation_index, game.soft_ping_animation_frame_index,
				game.interpolation_frame_index, game.interpolation_frame_count, game.flags);
			platform_log("halo_sim:   rust state %d seat %d/%d animation %d/%d action %d/%d overlay %d/%d ping %d/%d interpolation %d/%d flags %x",
				call.state.state, call.state.seat_index, call.state.base_seat_index, call.state.object_animation_index,
				call.state.object_animation_frame_index, call.state.action_animation_index, call.state.action_animation_frame_index,
				call.state.overlay_action_animation_index, call.state.overlay_action_animation_frame_index,
				call.state.soft_ping_animation_index, call.state.soft_ping_animation_frame_index,
				call.state.interpolation_frame_index, call.state.interpolation_frame_count, call.state.flags);
		}
	}
	if ((halo_sim_unit_animation_checks.compared & 0x3fff) == 0 ||
		(halo_sim_unit_animation_checks.compared & (halo_sim_unit_animation_checks.compared - 1)) == 0)
	{
		platform_log("halo_sim: unit animation: %lu of %lu agree with the game (%lu with side effects; %lu left out)",
			halo_sim_unit_animation_checks.compared - halo_sim_unit_animation_checks.differed,
			halo_sim_unit_animation_checks.compared, halo_sim_unit_animation_checks.effects,
			halo_sim_unit_animation_checks.skipped);
	}
}

/* ---------- player melee */

static void halo_sim_melee_call_new(long biped_index, long weapon_index, int weapon_prevents, long unit_graph_index,
	int enhanced_animation, struct halo_sim_melee_call *call)
{
	struct biped_datum *biped = biped_get(biped_index);

	memset(call, 0, sizeof(*call));
	call->biped_index = (unsigned long)biped_index;
	call->unit_graph_index = unit_graph_index;
	call->enhanced_animation = enhanced_animation ? 1 : 0;
	call->use_equipment = biped->unit.player_index != NONE &&
		TEST_FLAG(biped->unit.control_flags, _unit_control_use_equipment_bit);
	call->in_seat = biped->unit.parent_seat_index != NONE;
	call->zoomed = biped->unit.current_zoom_level != NONE;
	call->weapon_index = weapon_index;
	call->weapon_definition_index = weapon_index != NONE ? (unsigned long)object_get(weapon_index)->definition_index : NONE;
	call->weapon_prevents = weapon_prevents ? 1 : 0;
	call->ticks = biped->biped.player_melee_ticks;
	call->attack_tick = biped->biped.player_melee_attack_tick;
	halo_sim_unit_animation_state_load(biped_index, &call->animation);
	call->random_seed = get_random_seed();
}

int halo_sim_player_melee(long biped_index, long weapon_index, int weapon_prevents, long unit_graph_index,
	int enhanced_animation)
{
	short mode = halo_sim_mode(_halo_sim_player_melee);
	struct halo_sim_melee_call call;
	struct biped_datum *biped;

	halo_sim_melee_checks.recording = FALSE;
	if (mode == _halo_sim_mode_c)
		return FALSE;
	halo_sim_melee_call_new(biped_index, weapon_index, weapon_prevents, unit_graph_index, enhanced_animation,
		mode == _halo_sim_mode_check ? &halo_sim_melee_checks.call : &call);
	if (mode == _halo_sim_mode_check)
	{
		halo_sim_melee_checks.biped_index = biped_index;
		halo_sim_melee_checks.recording = TRUE;
		return FALSE;
	}

	if (halo_sim.player_melee(&call) != 0)
		return FALSE;
	biped = biped_get(biped_index);
	biped->biped.player_melee_ticks = call.ticks;
	biped->biped.player_melee_attack_tick = call.attack_tick;
	halo_sim_unit_animation_state_store(biped_index, &call.animation);

	return TRUE;
}

void halo_sim_player_melee_check(long biped_index)
{
	struct halo_sim_melee_call call;
	struct halo_sim_unit_animation_state game;
	struct biped_datum *biped;

	if (!halo_sim_melee_checks.recording)
		return;
	halo_sim_melee_checks.recording = FALSE;
	if (halo_sim_melee_checks.biped_index != biped_index)
		return;
	call = halo_sim_melee_checks.call;
	if (halo_sim.player_melee_check(&call) != 0)
		return;

	biped = biped_get(biped_index);
	halo_sim_unit_animation_state_load(biped_index, &game);
	halo_sim_melee_checks.compared++;
	halo_sim_melee_checks.started += call.started;
	halo_sim_melee_checks.damaged += call.damaged;
	if (call.ticks != biped->biped.player_melee_ticks || call.attack_tick != biped->biped.player_melee_attack_tick ||
		memcmp(&call.animation, &game, sizeof(game)) != 0)
	{
		halo_sim_melee_checks.differed++;
		if (halo_sim_melee_checks.differed <= 8)
		{
			platform_log("halo_sim: player melee differs: biped %lx weapon %lx from ticks %d/%d: game %d/%d action %d/%d, rust %d/%d action %d/%d (started %lu)",
				(unsigned long)biped_index, (unsigned long)halo_sim_melee_checks.call.weapon_index,
				halo_sim_melee_checks.call.ticks, halo_sim_melee_checks.call.attack_tick,
				biped->biped.player_melee_ticks, biped->biped.player_melee_attack_tick,
				game.action, game.action_animation_index, call.ticks, call.attack_tick,
				call.animation.action, call.animation.action_animation_index, call.started);
		}
	}
	if ((halo_sim_melee_checks.compared & 0x3fff) == 0 ||
		(halo_sim_melee_checks.compared & (halo_sim_melee_checks.compared - 1)) == 0)
	{
		platform_log("halo_sim: player melee: %lu of %lu agree with the game (%lu started, %lu landing)",
			halo_sim_melee_checks.compared - halo_sim_melee_checks.differed, halo_sim_melee_checks.compared,
			halo_sim_melee_checks.started, halo_sim_melee_checks.damaged);
	}
}
