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
#include "items/items.h"
#include "items/weapons.h"
#include "items/garbage.h"
#include "objects/scenery.h"
#include "units/unit_definitions.h"
#include "physics/point_physics.h"
#include "physics/collisions.h"
#include "items/projectiles.h"
#include "devices/device_definitions.h"
#include "items/projectiles_callbacks.h"
#include "objects/damage.h"
#include "devices/devices.h"
#include "units/unit_definitions.h"
#include "game/game_globals.h"
#include "game/game_engine.h"
#include "ai/actors.h"
#include "networking/network_game_globals.h"
#include "objects/damage_effect_definitions.h"
#include "structures/structures.h"
#include "scenario/scenario.h"
#include "tag_files/tag_groups.h"
#include "bevy_bridge.h"
#include "halo_sim.h"

#include <stdio.h>
#include <string.h>


/* ---------- constants */

/* the interface version this side speaks (crates/halo_sim's VERSION) */
#define HALO_SIM_VERSION 22

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

/* crates/halo_map's UnitUpdateState and UnitGlobals */
struct halo_sim_unit_update_state
{
	unsigned long flags;
	unsigned long control_flags;
	long persistent_control_timer;
	unsigned long persistent_control_flags;
	real_vector3d desired_facing_vector;
	real_vector3d desired_aiming_vector;
	real_vector3d aiming_vector;
	real_vector3d aiming_velocity;
	real_vector3d desired_looking_vector;
	real_vector3d looking_vector;
	real_vector3d looking_velocity;
	real_vector3d throttle;
	real primary_trigger;
	short timer;
	short grenade_throw_ticks;
	char weapon_drop_delay_ticks;
	char grenade_throw_state;
	char current_grenade_index;
	char desired_grenade_index;
	char grenade_counts[2];
	char current_zoom_level;
	char desired_zoom_level;
	char gunner_inactive_ticks;
	unsigned char aiming_change;
	unsigned char flaming_death_delay;
	char aiming_speed;
	short cause_for_camo_regrowth;
	short body_stun_ticks;
	short feign_death_timer;
	short delayed_damage_timer;
	short last_damage_category;
	short owner_team_index;
	real delayed_damage_peak;
	long delayed_damage_attacker_object_index;
	real body_stun;
	real seat_power[2];
	real integrated_light_power;
	real integrated_light_battery;
	real integrated_night_vision_power;
	real_vector3d seat_acceleration;
	real_vector3d seat_desired_acceleration;
	real active_camouflage;
	real active_camouflage_super_amount;
	real mouth_aperture;
	real run_blindly_angle;
	real run_blindly_angle_delta;
	short current_weapon_index;
	short desired_weapon_index;
	unsigned short animation_flags;
	char animation_state;
	unsigned char aiming_with_euler_screen;
	unsigned char looking_with_euler_screen;
	unsigned char pad[3];
	real_rectangle2d aiming_screen_bounds;
	real_rectangle2d looking_screen_bounds;
	long player_index;
	long driver_object_index;
	long gunner_object_index;
	long parent_object_index;
	unsigned long object_flags;
	unsigned long damage_flags;
	real body_vitality;
	short animation_frame_index;
	char player_melee_ticks;
	char pad2;
	real_vector3d forward;
	real_vector3d up;
	real_vector3d object_forward;
};

typedef char halo_sim_unit_update_state_size_assert[sizeof(struct halo_sim_unit_update_state) == 332 ? 1 : -1];

struct halo_sim_unit_globals
{
	short next_timer;
	short highest_timer;
	unsigned char used_time;
	unsigned char pad[3];
};

/* crates/halo_sim's ControllerC, UnitUpdateEventC and UnitUpdateCall */
struct halo_sim_controller
{
	short owner_team_index;
	short pad;
	unsigned long controls;
	unsigned long control_flags;
	real_vector3d desired_facing_vector;
	real_vector3d desired_aiming_vector;
	real_vector3d throttle;
	real primary_trigger;
};

struct halo_sim_unit_update_event
{
	long kind;
	long a;
	real b;
	long result;
	unsigned long seed;
	struct halo_sim_unit_update_state before;
	struct halo_sim_unit_update_state after;
};

struct halo_sim_unit_update_call
{
	unsigned long unit_index;
	unsigned long definition_index;
	struct halo_sim_unit_update_state state;
	struct halo_sim_unit_globals globals;
	unsigned long allow_integrated_lights;
	unsigned long game_engine_running;
	unsigned long cheat_jetpack;
	unsigned long cheat_infinite_ammo;
	unsigned long has_driver;
	struct halo_sim_controller driver;
	unsigned long has_gunner;
	struct halo_sim_controller gunner;
	unsigned long weapon_definition_index;
	struct halo_sim_unit_update_event const *events;
	unsigned long event_count;
	long difference_event;
	long difference_reason;
};

/* crates/halo_map's RestState */
struct halo_sim_rest_state
{
	real_vector3d throttle;
	real_vector3d translational_velocity;
	real_point3d position;
	real_point3d origin;
	unsigned long control_flags;
	unsigned long unit_flags;
	unsigned long object_flags;
	unsigned long damage_flags;
	long actor_index;
	long parent_object_index;
	long last_falling_communication_time;
	unsigned long animation_graph_index;
	short animation_index;
	short animation_frame_index;
	short location_cluster;
	char stop_ticks;
	char biped_state;
	char animation_state;
	char base_seat_index;
	char airborne_ticks;
	char state_desired;
	real ground_z;
};

typedef char halo_sim_rest_state_size_assert[sizeof(struct halo_sim_rest_state) == 96 ? 1 : -1];

/* crates/halo_sim's RestEventC, RestCall and RestCallbacks */
struct halo_sim_rest_event
{
	long kind;
	long a;
	long b;
	long result;
	struct halo_sim_rest_state before;
	struct halo_sim_rest_state after;
};

struct halo_sim_rest_call
{
	unsigned long biped_index;
	unsigned long definition_index;
	unsigned long riding;
	struct halo_sim_rest_state state;
	long game_time;
	unsigned long game_engine_running;
	real maximum_damage_velocity;
	long parent_kind;
	real vehicle_up_k;
	unsigned long vehicle_on_ground;
	unsigned long parent_dead;
	unsigned long rider_ejection;
	struct halo_sim_rest_event const *events;
	unsigned long event_count;
	long difference_event;
	long difference_reason;
	unsigned long still_there;
};

struct halo_sim_rest_callbacks
{
	long (*call)(unsigned long biped_index, struct halo_sim_rest_state *state, long kind, long a, long b);
};

/* crates/halo_map's DamageData (struct damage_data, its padding zeroed) */
struct halo_sim_damage_data
{
	long definition_index;
	unsigned long flags;
	long owner_player_index;
	long owner_object_index;
	short owner_team_index;
	short pad0;
	long location_leaf;
	short location_cluster;
	word location_bonus;
	real_point3d origin;
	real_point3d epicenter;
	real_vector3d direction;
	real scale;
	real multiplier;
	real material_effect_scale;
	short material_type;
	short pad1;
	void const *material_response;
};

typedef char halo_sim_damage_data_size_assert[sizeof(struct halo_sim_damage_data) == 84 ? 1 : -1];
typedef char halo_sim_damage_data_game_size_assert[sizeof(struct damage_data) == 84 ? 1 : -1];

/* crates/halo_map's ObjectDamageState */
struct halo_sim_object_damage_state
{
	unsigned long object_index;
	unsigned long definition_index;
	short object_type;
	short owner_team_index;
	real maximum_body_vitality;
	real maximum_shield_vitality;
	real body_vitality;
	real shield_vitality;
	real current_shield_damage;
	real current_body_damage;
	real recent_shield_damage;
	real recent_body_damage;
	long shield_damage_decay_timer;
	long body_damage_decay_timer;
	long umbrella_shield_object_index;
	long next_object_index;
	long first_child_object_index;
	long parent_object_index;
	short shield_stun_ticks;
	word damage_flags;
	word regions_destroyed_flags;
	word pad;
	byte region_damage[8];
	unsigned long unit;
	long player_index;
	long driver_object_index;
};

typedef char halo_sim_object_damage_state_size_assert[sizeof(struct halo_sim_object_damage_state) == 96 ? 1 : -1];

/* crates/halo_map's DamageCall, crates/halo_sim's DamageEventC, DamageCallC
and DamageCallbacks */
struct halo_sim_damage_call
{
	long kind;
	long object_index;
	long a;
	long b;
	real f[3];
	real out;
};

struct halo_sim_damage_event
{
	struct halo_sim_damage_call call;
	long result;
	unsigned long seed;
	struct halo_sim_damage_data damage_before;
	struct halo_sim_damage_data damage_after;
	struct halo_sim_object_damage_state object_before;
	struct halo_sim_object_damage_state object_after;
};

struct halo_sim_damage_call_c
{
	struct halo_sim_damage_data damage;
	unsigned long object_index;
	short node_index;
	short region_index;
	short material_index;
	word has_normal;
	unsigned long game_engine_running;
	unsigned long distributed_damage_authorized;
	unsigned long cheat_omnipotent;
	unsigned long cheat_deathless_player;
	unsigned long random_seed;
	struct halo_sim_object_damage_state const *snapshot;
	unsigned long snapshot_count;
	struct halo_sim_damage_event const *events;
	unsigned long event_count;
	long difference_event;
	long difference_reason;
};

/* crates/halo_map's AreaObject and AreaCall, crates/halo_sim's AreaEventC,
AreaCallC and AreaDamageCallbacks */
struct halo_sim_area_object
{
	unsigned long object_index;
	unsigned long definition_index;
	short object_type;
	short owner_team_index;
	unsigned long flags;
	real_point3d bounding_center;
	real bounding_radius;
	long parent_object_index;
	long first_child_object_index;
	long next_object_index;
};

typedef char halo_sim_area_object_size_assert[sizeof(struct halo_sim_area_object) == 44 ? 1 : -1];

struct halo_sim_area_call
{
	long kind;
	long object_index;
	long a;
	long b;
	real f[6];
	long result;
	real out[3];
};

struct halo_sim_area_event
{
	struct halo_sim_area_call call;
	unsigned long seed;
	struct halo_sim_damage_data damage_before;
	struct halo_sim_damage_data damage_after;
	unsigned long update_first;
	unsigned long update_count;
};

struct halo_sim_area_call_c
{
	struct halo_sim_damage_data damage;
	long structure_bsp_index;
	unsigned char const *breakable_surfaces;
	unsigned long random_seed;
	short const *clusters;
	unsigned long cluster_count;
	unsigned long const *partition_counts;
	unsigned long const *partition_objects;
	struct halo_sim_area_object const *snapshot;
	unsigned long snapshot_count;
	struct halo_sim_area_event const *events;
	unsigned long event_count;
	struct halo_sim_area_object const *updates;
	unsigned long update_count;
	long difference_event;
	long difference_reason;
};

struct halo_sim_area_damage_callbacks
{
	unsigned long (*cluster_objects)(short cluster, unsigned long partition, unsigned long *out, unsigned long maximum);
	long (*object)(unsigned long index, struct halo_sim_area_object *out);
	long (*call)(struct halo_sim_damage_data *damage, struct halo_sim_area_call *call);
};

/* crates/halo_sim's DamageUpdateEventC, DamageUpdateCallC and
DamageUpdateCallbacks */
struct halo_sim_damage_update_event
{
	long kind;
	real f;
	long result;
	real out;
	struct halo_sim_object_damage_state before;
	struct halo_sim_object_damage_state after;
};

struct halo_sim_damage_update_call
{
	struct halo_sim_object_damage_state state;
	unsigned long update_shields;
	unsigned long game_engine_running;
	struct halo_sim_damage_update_event const *events;
	unsigned long event_count;
	long difference_event;
	long difference_reason;
	unsigned long still_there;
};

struct halo_sim_damage_update_callbacks
{
	long (*call)(struct halo_sim_object_damage_state *state, long kind, real f, real *out);
};

/* crates/halo_map's DeviceState, DeviceBiped and DeviceCall, crates/
halo_sim's DeviceEventC, DeviceCallC and DeviceCallbacks */
struct halo_sim_device_state
{
	unsigned long object_index;
	unsigned long definition_index;
	short location_cluster;
	short power_group_index;
	unsigned long device_flags;
	real power;
	real power_velocity;
	short position_group_index;
	short delay_ticks;
	real position;
	real position_velocity;
	unsigned long machine_flags;
	long door_open_ticks;
	real_point3d elevator_position;
	real_point3d object_position;
	real_vector3d forward;
	real_point3d bounding_center;
	real bounding_radius;
};

typedef char halo_sim_device_state_size_assert[sizeof(struct halo_sim_device_state) == 96 ? 1 : -1];

struct halo_sim_device_biped
{
	unsigned long object_index;
	short owner_team_index;
	word cannot_open_doors;
	real_point3d bounding_center;
	real_point3d position;
	long elevator_object_index;
};

typedef char halo_sim_device_biped_size_assert[sizeof(struct halo_sim_device_biped) == 36 ? 1 : -1];

struct halo_sim_device_call
{
	long kind;
	long object_index;
	long a;
	long b;
	real f[3];
};

struct halo_sim_device_event
{
	struct halo_sim_device_call call;
	real radius;
	long result;
	struct halo_sim_device_state before;
	struct halo_sim_device_state after;
	unsigned long biped_first;
	unsigned long biped_count;
};

struct halo_sim_device_call_c
{
	struct halo_sim_device_state state;
	unsigned long part;
	long structure_bsp_index;
	unsigned char const *breakable_surfaces;
	real power_group_value;
	real position_group_value;
	long game_time;
	unsigned long has_elevator_node;
	real_point3d elevator_node_position;
	struct halo_sim_device_event const *events;
	unsigned long event_count;
	struct halo_sim_device_biped const *bipeds;
	unsigned long biped_count;
	long difference_event;
	long difference_reason;
	unsigned long changed;
};

struct halo_sim_device_callbacks
{
	long (*call)(struct halo_sim_device_state *state, struct halo_sim_device_call const *call);
	long (*biped)(unsigned long index, struct halo_sim_device_biped *out);
};

/* crates/halo_map's ItemState and ItemCall, crates/halo_sim's ItemEventC,
ItemCallC and ItemCallbacks */
struct halo_sim_item_state
{
	unsigned long object_index;
	unsigned long definition_index;
	unsigned long object_flags;
	long owner_player_index;
	long parent_object_index;
	real_point3d position;
	real_vector3d forward;
	real_vector3d up;
	real_vector3d translational_velocity;
	real_vector3d angular_velocity;
	byte region_permutations[8];
	unsigned long item_flags;
	short detonation_ticks;
	short rested_surface_index;
	short bsp_index;
	short pad;
	long ignore_object_index;
	long last_owned_time;
	long item_on_rest_object_index;
	real_point3d item_rest_object_offset;
	real_vector3d rotation_axis;
	real rotation_sine;
	real rotation_cosine;
};

typedef char halo_sim_item_state_size_assert[sizeof(struct halo_sim_item_state) == 144 ? 1 : -1];

struct halo_sim_item_call
{
	long kind;
	long a;
	long b;
	long c;
	real f[10];
};

struct halo_sim_item_event
{
	struct halo_sim_item_call call;
	long result;
	struct halo_sim_item_state before;
	struct halo_sim_item_state after;
};

struct halo_sim_item_call_c
{
	struct halo_sim_item_state state;
	long structure_bsp_index;
	unsigned char const *breakable_surfaces;
	real global_gravity;
	unsigned long game_engine_running;
	long game_time;
	real const *node_matrices;
	unsigned long node_count;
	struct halo_sim_item_event const *events;
	unsigned long event_count;
	long difference_event;
	long difference_reason;
	unsigned long still_there;
};

struct halo_sim_item_callbacks
{
	long (*call)(struct halo_sim_item_state *state, struct halo_sim_item_call const *call);
};

/* crates/halo_map's WeaponState, crates/halo_sim's WeaponEventC,
WeaponCallC and WeaponCallbacks */
struct halo_sim_weapon_state
{
	unsigned long object_index;
	unsigned long definition_index;
	long parent_object_index;
	unsigned long item_flags;
	unsigned long flags;
	word control_flags;
	word pad0;
	real primary_trigger;
	char state;
	char pad1;
	short state_timer;
	real heat;
	real age;
	real overcharged;
	real integrated_light_power;
	long tracked_object_index;
	short alternate_shots_loaded;
	short animation_index;
	short animation_frame_index;
	short pad2;
	struct weapon_trigger triggers[2];
	struct weapon_magazine magazines[2];
	long overheated_effect_index;
};

typedef char halo_sim_weapon_trigger_size_assert[sizeof(struct weapon_trigger) == 36 ? 1 : -1];
typedef char halo_sim_weapon_magazine_size_assert[sizeof(struct weapon_magazine) == 12 ? 1 : -1];
typedef char halo_sim_weapon_state_size_assert[sizeof(struct halo_sim_weapon_state) == 160 ? 1 : -1];

struct halo_sim_weapon_event
{
	long kind;
	long a;
	long b;
	unsigned long seed;
	struct halo_sim_weapon_state before;
	struct halo_sim_weapon_state after;
};

struct halo_sim_weapon_call
{
	struct halo_sim_weapon_state state;
	unsigned long random_seed;
	struct halo_sim_weapon_event const *events;
	unsigned long event_count;
	long difference_event;
	long difference_reason;
	unsigned long still_there;
};

struct halo_sim_weapon_callbacks
{
	long (*call)(struct halo_sim_weapon_state *state, long kind, long a, long b);
};

typedef char halo_sim_objects_call_size_assert[sizeof(struct halo_sim_objects_call) == 64 ? 1 : -1];

/* crates/halo_sim's ObjectsUpdateC, SceneryCallC and ObjectsCallbacks */
struct halo_sim_objects_update_call
{
	unsigned long dont_update_object;
	struct halo_sim_objects_call const *events;
	unsigned long event_count;
	long difference_event;
	long difference_reason;
};

struct halo_sim_scenery_call
{
	unsigned long animation_graph_index;
	unsigned long flags;
	unsigned long random_seed;
	short animation_index;
	short animation_frame_index;
};

struct halo_sim_objects_callbacks
{
	long (*call)(struct halo_sim_objects_call *call);
};

/* crates/halo_map's ObjectFunctionsState, crates/halo_sim's
ObjectFunctionsCallC */
struct halo_sim_object_functions_state
{
	unsigned long object_index;
	unsigned long definition_index;
	unsigned char functions_active_flags;
	unsigned char pad[3];
	real incoming_function_values[4];
	real outgoing_function_values[4];
	real_rgb_color outgoing_change_colors[4];
};

typedef char halo_sim_object_functions_state_size_assert[sizeof(struct halo_sim_object_functions_state) == 92 ? 1 : -1];

struct halo_sim_object_functions_call
{
	struct halo_sim_object_functions_state state;
	unsigned long part;
	long game_time;
	unsigned char const *periodic_tables[12];
	unsigned char const *transition_tables[6];
};

struct halo_sim_damage_callbacks
{
	long (*object)(unsigned long index, struct halo_sim_object_damage_state *out);
	long (*call)(struct halo_sim_damage_data *damage, struct halo_sim_object_damage_state *objects, unsigned long count,
		struct halo_sim_damage_call *call);
};

/* crates/halo_sim's UnitUpdateCallbacks */
struct halo_sim_unit_update_callbacks
{
	long (*call)(unsigned long unit_index, struct halo_sim_unit_update_state *state, long kind, long a, real b);
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
	struct halo_sim_unit_update_callbacks unit_update;
	struct halo_sim_rest_callbacks rest;
	struct halo_sim_damage_callbacks damage;
	struct halo_sim_area_damage_callbacks area_damage;
	struct halo_sim_damage_update_callbacks damage_update;
	struct halo_sim_device_callbacks devices;
	struct halo_sim_item_callbacks items;
	struct halo_sim_weapon_callbacks weapons;
	struct halo_sim_objects_callbacks objects;
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
typedef long (*halo_sim_unit_update_function)(struct halo_sim_unit_update_call *call);
typedef long (*halo_sim_rest_function)(struct halo_sim_rest_call *call);
typedef long (*halo_sim_damage_function)(struct halo_sim_damage_call_c *call);
typedef long (*halo_sim_area_damage_function)(struct halo_sim_area_call_c *call);
typedef long (*halo_sim_damage_update_function)(struct halo_sim_damage_update_call *call);
typedef long (*halo_sim_device_function)(struct halo_sim_device_call_c *call);
typedef long (*halo_sim_item_function)(struct halo_sim_item_call_c *call);
typedef long (*halo_sim_weapon_function)(struct halo_sim_weapon_call *call);
typedef long (*halo_sim_objects_function)(struct halo_sim_objects_update_call *call);
typedef long (*halo_sim_object_functions_function)(struct halo_sim_object_functions_call *call);
typedef long (*halo_sim_garbage_function)(short *lifetime_ticks);
typedef long (*halo_sim_scenery_function)(struct halo_sim_scenery_call *call);
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
	halo_sim_unit_update_function unit_update;
	halo_sim_unit_update_function unit_update_check;
	halo_sim_rest_function biped_rest;
	halo_sim_rest_function biped_rest_check;
	halo_sim_damage_function object_cause_damage;
	halo_sim_damage_function object_cause_damage_check;
	halo_sim_area_damage_function area_damage;
	halo_sim_area_damage_function area_damage_check;
	halo_sim_damage_update_function damage_update;
	halo_sim_damage_update_function damage_update_check;
	halo_sim_device_function device_update;
	halo_sim_device_function device_update_check;
	halo_sim_item_function item_update;
	halo_sim_item_function item_update_check;
	halo_sim_weapon_function weapon_update;
	halo_sim_weapon_function weapon_update_check;
	halo_sim_objects_function objects_update;
	halo_sim_objects_function objects_update_check;
	halo_sim_garbage_function garbage_update;
	halo_sim_scenery_function scenery_update;
	halo_sim_object_functions_function object_functions;
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
	"unit_update",
	"biped_rest",
	"damage",
	"area_damage",
	"damage_update",
	"devices",
	"items",
	"weapons",
	"objects",
	"object_functions",
};

/* the biped rest checks: what the game's began with, its calls, and the
comparisons */
#define MAXIMUM_HALO_SIM_REST_EVENTS 16
static struct
{
	boolean recording;
	boolean overflow;
	long biped_index;
	struct halo_sim_rest_call call;
	struct halo_sim_rest_event events[MAXIMUM_HALO_SIM_REST_EVENTS];
	unsigned long event_count;
	struct halo_sim_rest_event *open_event;
	struct unit_animation_update_data const *animation;
	unsigned long compared[2];
	unsigned long differed;
	unsigned long skipped;
	unsigned long calls;
} halo_sim_rest_checks;

/* the damage checks: the objects the game's began with, its calls, and the
comparisons */
#define MAXIMUM_HALO_SIM_DAMAGE_SNAPSHOT 64
#define MAXIMUM_HALO_SIM_DAMAGE_EVENTS 128
static struct
{
	boolean recording;
	boolean overflow;
	struct halo_sim_damage_call_c call;
	struct halo_sim_object_damage_state snapshot[MAXIMUM_HALO_SIM_DAMAGE_SNAPSHOT];
	unsigned long snapshot_count;
	struct halo_sim_damage_event events[MAXIMUM_HALO_SIM_DAMAGE_EVENTS];
	unsigned long event_count;
	struct halo_sim_damage_event *open_event;
	unsigned long compared;
	unsigned long differed;
	unsigned long skipped;
	unsigned long calls;
} halo_sim_damage_checks;

/* the area damage checks: the clusters, partitions and objects the game's
began with, its calls and what they changed, and the comparisons */
#define MAXIMUM_HALO_SIM_AREA_CLUSTERS 512
#define MAXIMUM_HALO_SIM_AREA_PARTITION_OBJECTS 8192
#define MAXIMUM_HALO_SIM_AREA_SNAPSHOT 2048
#define MAXIMUM_HALO_SIM_AREA_EVENTS 1024
#define MAXIMUM_HALO_SIM_AREA_UPDATES 4096
static struct
{
	boolean recording;
	boolean overflow;
	struct halo_sim_area_call_c call;
	short clusters[MAXIMUM_HALO_SIM_AREA_CLUSTERS];
	unsigned long cluster_count;
	unsigned long partition_counts[2 * MAXIMUM_HALO_SIM_AREA_CLUSTERS];
	unsigned long partition_objects[MAXIMUM_HALO_SIM_AREA_PARTITION_OBJECTS];
	unsigned long partition_object_count;
	struct halo_sim_area_object snapshot[MAXIMUM_HALO_SIM_AREA_SNAPSHOT];
	struct halo_sim_area_object current[MAXIMUM_HALO_SIM_AREA_SNAPSHOT];
	unsigned long snapshot_count;
	struct halo_sim_area_event events[MAXIMUM_HALO_SIM_AREA_EVENTS];
	unsigned long event_count;
	struct halo_sim_area_event *open_event;
	struct halo_sim_area_object updates[MAXIMUM_HALO_SIM_AREA_UPDATES];
	unsigned long update_count;
	unsigned long compared;
	unsigned long differed;
	unsigned long skipped;
	unsigned long calls;
	unsigned long rays;
	unsigned long dealt;
	unsigned long infection_forms;
} halo_sim_area_checks;

/* the damage update checks: the object the game's began with, its calls,
and the comparisons */
#define MAXIMUM_HALO_SIM_DAMAGE_UPDATE_EVENTS 8
static struct
{
	boolean recording;
	boolean overflow;
	long object_index;
	struct halo_sim_damage_update_call call;
	struct halo_sim_damage_update_event events[MAXIMUM_HALO_SIM_DAMAGE_UPDATE_EVENTS];
	unsigned long event_count;
	struct halo_sim_damage_update_event *open_event;
	unsigned long compared;
	unsigned long differed;
	unsigned long skipped;
	unsigned long calls;
	unsigned long recharging;
	unsigned long decaying;
} halo_sim_damage_update_checks;

/* the device update checks: the device the game's began with, its calls
and its spheres' bipeds, and the comparisons */
#define MAXIMUM_HALO_SIM_DEVICE_EVENTS 64
#define MAXIMUM_HALO_SIM_DEVICE_BIPEDS 256
static struct
{
	boolean recording;
	boolean overflow;
	long device_index;
	struct halo_sim_device_call_c call;
	struct halo_sim_device_event events[MAXIMUM_HALO_SIM_DEVICE_EVENTS];
	unsigned long event_count;
	struct halo_sim_device_event *open_event;
	struct halo_sim_device_biped bipeds[MAXIMUM_HALO_SIM_DEVICE_BIPEDS];
	unsigned long biped_count;
	unsigned long compared[2];
	unsigned long differed;
	unsigned long skipped;
	unsigned long calls;
	unsigned long moving;
	unsigned long spheres;
	unsigned long effects;
	unsigned long desired;
	unsigned long carried;
} halo_sim_device_checks;

/* the item update checks: the item the game's began with, its calls, and
the comparisons */
#define MAXIMUM_HALO_SIM_ITEM_EVENTS 8
static struct
{
	boolean recording;
	boolean overflow;
	long item_index;
	struct halo_sim_item_call_c call;
	struct halo_sim_item_event events[MAXIMUM_HALO_SIM_ITEM_EVENTS];
	unsigned long event_count;
	struct halo_sim_item_event *open_event;
	unsigned long compared;
	unsigned long differed;
	unsigned long skipped;
	unsigned long calls;
	unsigned long moving;
	unsigned long impacts;
	unsigned long rested;
	unsigned long spinning;
} halo_sim_item_checks;

/* the weapon update checks: the weapon and seed the game's began with, its
calls, and the comparisons */
#define MAXIMUM_HALO_SIM_WEAPON_EVENTS 16
static struct
{
	boolean recording;
	boolean overflow;
	long weapon_index;
	struct halo_sim_weapon_call call;
	struct halo_sim_weapon_event events[MAXIMUM_HALO_SIM_WEAPON_EVENTS];
	unsigned long event_count;
	struct halo_sim_weapon_event *open_event;
	unsigned long compared;
	unsigned long differed;
	unsigned long skipped;
	unsigned long calls;
	unsigned long firing;
	unsigned long reloading;
	unsigned long heating;
} halo_sim_weapon_checks;

/* the object update loop checks: the game's calls and what they read, and
the comparisons (and garbage's and scenery's) */
#define MAXIMUM_HALO_SIM_OBJECTS_EVENTS 131072
static struct
{
	boolean recording;
	boolean overflow;
	struct halo_sim_objects_call events[MAXIMUM_HALO_SIM_OBJECTS_EVENTS];
	unsigned long event_count;
	unsigned long compared;
	unsigned long differed;
	unsigned long skipped;
	unsigned long calls;
	unsigned long updates;
	short garbage_lifetime;
	boolean garbage_recording;
	struct halo_sim_scenery_call scenery_before;
	long scenery_index;
	boolean scenery_recording;
	unsigned long garbage_compared[2];
	unsigned long scenery_compared[2];
} halo_sim_objects_checks;

/* the function values' and change colours' checks: what the game's began
with, and the comparisons */
static struct
{
	boolean recording;
	long object_index;
	struct halo_sim_object_functions_call call;
	unsigned long compared[2];
	unsigned long differed[2];
	unsigned long active[2];
} halo_sim_object_functions_checks;

/* the unit update checks: the update the game's began with, its calls of
the rest of the game, and the comparisons */
#define MAXIMUM_HALO_SIM_UNIT_EVENTS 32
static struct
{
	boolean recording;
	boolean overflow;
	long unit_index;
	struct halo_sim_unit_globals *globals;
	struct halo_sim_unit_update_call call;
	struct halo_sim_unit_update_event events[MAXIMUM_HALO_SIM_UNIT_EVENTS];
	unsigned long event_count;
	struct halo_sim_unit_update_event *open_event;
	unsigned long compared;
	unsigned long differed;
	unsigned long skipped;
	unsigned long calls;
} halo_sim_unit_update_checks;

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

/* a unit's update state, from the unit */
static void halo_sim_unit_update_state_load(long unit_index, struct halo_sim_unit_update_state *state)
{
	struct unit_datum *unit = unit_get(unit_index);

	memset(state, 0, sizeof(*state));
	state->flags = unit->unit.flags;
	state->control_flags = unit->unit.control_flags;
	state->persistent_control_timer = unit->unit.persistent_control_timer;
	state->persistent_control_flags = unit->unit.persistent_control_flags;
	state->desired_facing_vector = unit->unit.desired_facing_vector;
	state->desired_aiming_vector = unit->unit.desired_aiming_vector;
	state->aiming_vector = unit->unit.aiming_vector;
	state->aiming_velocity = unit->unit.aiming_velocity;
	state->desired_looking_vector = unit->unit.desired_looking_vector;
	state->looking_vector = unit->unit.looking_vector;
	state->looking_velocity = unit->unit.looking_velocity;
	state->throttle = unit->unit.throttle;
	state->primary_trigger = unit->unit.primary_trigger;
	state->timer = unit->unit.timer;
	state->grenade_throw_ticks = unit->unit.grenade_throw_ticks;
	state->weapon_drop_delay_ticks = unit->unit.weapon_drop_delay_ticks;
	state->grenade_throw_state = unit->unit.grenade_throw_state;
	state->current_grenade_index = unit->unit.current_grenade_index;
	state->desired_grenade_index = unit->unit.desired_grenade_index;
	state->grenade_counts[0] = unit->unit.grenade_counts[0];
	state->grenade_counts[1] = unit->unit.grenade_counts[1];
	state->current_zoom_level = unit->unit.current_zoom_level;
	state->desired_zoom_level = unit->unit.desired_zoom_level;
	state->gunner_inactive_ticks = unit->unit.gunner_inactive_ticks;
	state->aiming_change = unit->unit.aiming_change;
	state->flaming_death_delay = unit->unit.flaming_death_delay;
	state->aiming_speed = unit->unit.aiming_speed;
	state->cause_for_camo_regrowth = unit->unit.cause_for_camo_regrowth;
	state->body_stun_ticks = unit->unit.body_stun_ticks;
	state->feign_death_timer = unit->unit.feign_death_timer;
	state->delayed_damage_timer = unit->unit.delayed_damage_timer;
	state->last_damage_category = unit->unit.last_damage_category;
	state->owner_team_index = unit->object.owner_team_index;
	state->delayed_damage_peak = unit->unit.delayed_damage_peak;
	state->delayed_damage_attacker_object_index = unit->unit.delayed_damage_attacker_object_index;
	state->body_stun = unit->unit.body_stun;
	state->seat_power[0] = unit->unit.seat_power[0];
	state->seat_power[1] = unit->unit.seat_power[1];
	state->integrated_light_power = unit->unit.integrated_light_power;
	state->integrated_light_battery = unit->unit.integrated_light_battery;
	state->integrated_night_vision_power = unit->unit.integrated_night_vision_power;
	state->seat_acceleration = unit->unit.seat_acceleration;
	state->seat_desired_acceleration = unit->unit.seat_desired_acceleration;
	state->active_camouflage = unit->unit.active_camouflage;
	state->active_camouflage_super_amount = unit->unit.active_camouflage_super_amount;
	state->mouth_aperture = unit->unit.mouth_aperture;
	state->run_blindly_angle = unit->unit.run_blindly_angle;
	state->run_blindly_angle_delta = unit->unit.run_blindly_angle_delta;
	state->current_weapon_index = unit->unit.current_weapon_index;
	state->desired_weapon_index = unit->unit.desired_weapon_index;
	state->animation_flags = unit->unit.animation.flags;
	state->animation_state = unit->unit.animation.state;
	state->aiming_with_euler_screen = unit->unit.animation.aiming_with_euler_screen ? 1 : 0;
	state->looking_with_euler_screen = unit->unit.animation.looking_with_euler_screen ? 1 : 0;
	state->aiming_screen_bounds = unit->unit.animation.aiming_screen_bounds;
	state->looking_screen_bounds = unit->unit.animation.looking_screen_bounds;
	state->player_index = unit->unit.player_index;
	state->driver_object_index = unit->unit.driver_object_index;
	state->gunner_object_index = unit->unit.gunner_object_index;
	state->parent_object_index = unit->object.parent_object_index;
	state->object_flags = unit->object.flags;
	state->damage_flags = unit->object.damage_flags;
	state->body_vitality = unit->object.body_vitality;
	state->animation_frame_index = unit->object.animation.state.frame_index;
	state->player_melee_ticks = unit->object.type == _object_type_biped ? biped_get(unit_index)->biped.player_melee_ticks : 0;
	object_get_orientation(unit_index, &state->forward, &state->up);
	state->object_forward = unit->object.forward;
}

/* and back to it: what the update changes */
static void halo_sim_unit_update_state_store(long unit_index, struct halo_sim_unit_update_state const *state)
{
	struct unit_datum *unit = unit_get(unit_index);

	unit->unit.flags = state->flags;
	unit->unit.control_flags = state->control_flags;
	unit->unit.persistent_control_timer = state->persistent_control_timer;
	unit->unit.persistent_control_flags = state->persistent_control_flags;
	unit->unit.desired_facing_vector = state->desired_facing_vector;
	unit->unit.desired_aiming_vector = state->desired_aiming_vector;
	unit->unit.aiming_vector = state->aiming_vector;
	unit->unit.aiming_velocity = state->aiming_velocity;
	unit->unit.desired_looking_vector = state->desired_looking_vector;
	unit->unit.looking_vector = state->looking_vector;
	unit->unit.looking_velocity = state->looking_velocity;
	unit->unit.throttle = state->throttle;
	unit->unit.primary_trigger = state->primary_trigger;
	unit->unit.timer = state->timer;
	unit->unit.grenade_throw_ticks = state->grenade_throw_ticks;
	unit->unit.weapon_drop_delay_ticks = state->weapon_drop_delay_ticks;
	unit->unit.grenade_throw_state = state->grenade_throw_state;
	unit->unit.current_grenade_index = state->current_grenade_index;
	unit->unit.desired_grenade_index = state->desired_grenade_index;
	unit->unit.grenade_counts[0] = state->grenade_counts[0];
	unit->unit.grenade_counts[1] = state->grenade_counts[1];
	unit->unit.current_zoom_level = state->current_zoom_level;
	unit->unit.gunner_inactive_ticks = state->gunner_inactive_ticks;
	unit->unit.aiming_change = state->aiming_change;
	unit->unit.flaming_death_delay = state->flaming_death_delay;
	unit->unit.cause_for_camo_regrowth = state->cause_for_camo_regrowth;
	unit->unit.body_stun_ticks = state->body_stun_ticks;
	unit->unit.feign_death_timer = state->feign_death_timer;
	unit->unit.delayed_damage_timer = state->delayed_damage_timer;
	unit->unit.last_damage_category = state->last_damage_category;
	unit->object.owner_team_index = state->owner_team_index;
	unit->unit.delayed_damage_peak = state->delayed_damage_peak;
	unit->unit.delayed_damage_attacker_object_index = state->delayed_damage_attacker_object_index;
	unit->unit.body_stun = state->body_stun;
	unit->unit.seat_power[0] = state->seat_power[0];
	unit->unit.seat_power[1] = state->seat_power[1];
	unit->unit.integrated_light_power = state->integrated_light_power;
	unit->unit.integrated_light_battery = state->integrated_light_battery;
	unit->unit.integrated_night_vision_power = state->integrated_night_vision_power;
	unit->unit.seat_acceleration = state->seat_acceleration;
	unit->unit.active_camouflage = state->active_camouflage;
	unit->unit.active_camouflage_super_amount = state->active_camouflage_super_amount;
	unit->unit.mouth_aperture = state->mouth_aperture;
	unit->unit.run_blindly_angle = state->run_blindly_angle;
	unit->unit.run_blindly_angle_delta = state->run_blindly_angle_delta;
}

/* units.c's, a unit update's call of the rest of the game */
long unit_update_call(long unit_index, long kind, long a, real b);

static long halo_sim_unit_update_callback(unsigned long unit_index, struct halo_sim_unit_update_state *state, long kind,
	long a, real b)
{
	long result;

	halo_sim_unit_update_state_store((long)unit_index, state);
	result = unit_update_call((long)unit_index, kind, a, b);
	halo_sim_unit_update_state_load((long)unit_index, state);
	return result;
}

/* bipeds.c's: the ground a falling biped found, the falling damage's speed
that kills, and a call of the rest of the game */
real biped_rust_ground_z(void);
real biped_rust_maximum_damage_velocity(void);
long biped_rest_call(long biped_index, long kind, long a, long b);

/* the update's animation data, while the rest of a biped's update runs */
static struct unit_animation_update_data *halo_sim_rest_animation;

/* the rest of a biped's update's state, from the biped (and its update's
animation data) */
static void halo_sim_rest_state_load(long biped_index, struct unit_animation_update_data const *animation,
	struct halo_sim_rest_state *state)
{
	struct biped_datum *biped = biped_get(biped_index);

	memset(state, 0, sizeof(*state));
	state->throttle = biped->unit.throttle;
	state->translational_velocity = biped->object.translational_velocity;
	state->position = biped->object.position;
	object_get_origin(biped_index, &state->origin);
	state->control_flags = biped->unit.control_flags;
	state->unit_flags = biped->unit.flags;
	state->object_flags = biped->object.flags;
	state->damage_flags = biped->object.damage_flags;
	state->actor_index = biped->unit.actor_index;
	state->parent_object_index = biped->object.parent_object_index;
	state->last_falling_communication_time = biped->biped.last_falling_communication_time;
	state->animation_graph_index = biped->object.animation.animation_graph_index;
	state->animation_index = biped->object.animation.state.index;
	state->animation_frame_index = biped->object.animation.state.frame_index;
	state->location_cluster = biped->object.location.cluster_index;
	state->stop_ticks = biped->biped.stop_ticks;
	state->biped_state = biped->biped.state;
	state->animation_state = biped->unit.animation.state;
	state->base_seat_index = biped->unit.animation.base_seat_index;
	state->airborne_ticks = biped->biped.airborne_ticks;
	state->state_desired = animation ? animation->state_desired : 0;
	state->ground_z = biped_rust_ground_z();
}

/* and back to it: what these change */
static void halo_sim_rest_state_store(long biped_index, struct unit_animation_update_data *animation,
	struct halo_sim_rest_state const *state)
{
	struct biped_datum *biped = biped_get(biped_index);

	biped->biped.last_falling_communication_time = state->last_falling_communication_time;
	biped->biped.stop_ticks = state->stop_ticks;
	if (animation)
		animation->state_desired = state->state_desired;
}

static long halo_sim_rest_callback(unsigned long biped_index, struct halo_sim_rest_state *state, long kind, long a, long b)
{
	long result;

	halo_sim_rest_state_store((long)biped_index, halo_sim_rest_animation, state);
	result = biped_rest_call((long)biped_index, kind, a, b);
	if (object_try_and_get_and_verify_type((long)biped_index, _object_mask_biped))
		halo_sim_rest_state_load((long)biped_index, halo_sim_rest_animation, state);
	return result;
}

/* damage.c's, a call of the rest of the game */
long damage_call(struct damage_data *damage, long kind, long object_index, long a, long b, real const *f, real *out);

/* the damage Rust deals, while it runs (the game's caller's) */
static struct damage_data *halo_sim_damage_active;

static void halo_sim_damage_data_load(struct damage_data const *damage, struct halo_sim_damage_data *data)
{
	memset(data, 0, sizeof(*data));
	data->definition_index = damage->definition_index;
	data->flags = damage->flags;
	data->owner_player_index = damage->owner_player_index;
	data->owner_object_index = damage->owner_object_index;
	data->owner_team_index = damage->owner_team_index;
	data->location_leaf = damage->location.leaf_index;
	data->location_cluster = damage->location.cluster_index;
	data->location_bonus = damage->location.bonus;
	data->origin = damage->origin;
	data->epicenter = damage->epicenter;
	data->direction = damage->direction;
	data->scale = damage->scale;
	data->multiplier = damage->multiplier;
	data->material_effect_scale = damage->material_effect_scale;
	data->material_type = damage->material_type;
	data->material_response = damage->material_response;
}

static void halo_sim_damage_data_store(struct damage_data *damage, struct halo_sim_damage_data const *data)
{
	damage->definition_index = data->definition_index;
	damage->flags = data->flags;
	damage->owner_player_index = data->owner_player_index;
	damage->owner_object_index = data->owner_object_index;
	damage->owner_team_index = data->owner_team_index;
	damage->location.leaf_index = data->location_leaf;
	damage->location.cluster_index = data->location_cluster;
	damage->location.bonus = data->location_bonus;
	damage->origin = data->origin;
	damage->epicenter = data->epicenter;
	damage->direction = data->direction;
	damage->scale = data->scale;
	damage->multiplier = data->multiplier;
	damage->material_effect_scale = data->material_effect_scale;
	damage->material_type = data->material_type;
	damage->material_response = (struct projectile_material_response_definition const *)data->material_response;
}

/* an object's damage state, from the object: FALSE if it is gone */
static boolean halo_sim_object_damage_state_load(long object_index, struct halo_sim_object_damage_state *state)
{
	struct object_datum *object = object_try_and_get(object_index);

	memset(state, 0, sizeof(*state));
	state->object_index = (unsigned long)object_index;
	if (!object)
		return FALSE;
	state->definition_index = (unsigned long)object->definition_index;
	state->object_type = object->object.type;
	state->owner_team_index = object->object.owner_team_index;
	state->maximum_body_vitality = object->object.maximum_body_vitality;
	state->maximum_shield_vitality = object->object.maximum_shield_vitality;
	state->body_vitality = object->object.body_vitality;
	state->shield_vitality = object->object.shield_vitality;
	state->current_shield_damage = object->object.current_shield_damage;
	state->current_body_damage = object->object.current_body_damage;
	state->recent_shield_damage = object->object.recent_shield_damage;
	state->recent_body_damage = object->object.recent_body_damage;
	state->shield_damage_decay_timer = object->object.shield_damage_decay_timer;
	state->body_damage_decay_timer = object->object.body_damage_decay_timer;
	state->umbrella_shield_object_index = object->object.umbrella_shield_object_index;
	state->next_object_index = object->object.next_object_index;
	state->first_child_object_index = object->object.first_child_object_index;
	state->parent_object_index = object->object.parent_object_index;
	state->shield_stun_ticks = object->object.shield_stun_ticks;
	state->damage_flags = object->object.damage_flags;
	state->regions_destroyed_flags = object->object.regions_destroyed_flags;
	memcpy(state->region_damage, object->object.region_damage, sizeof(state->region_damage));
	if (TEST_FLAG(_object_mask_unit, object->object.type))
	{
		struct unit_datum *unit = (struct unit_datum *)object;

		state->unit = 1;
		state->player_index = unit->unit.player_index;
		state->driver_object_index = unit->unit.driver_object_index;
	}
	else
	{
		state->player_index = NONE;
		state->driver_object_index = NONE;
	}

	return TRUE;
}

/* and back to it: what damage changes */
static void halo_sim_object_damage_state_store(struct halo_sim_object_damage_state const *state)
{
	struct object_datum *object = object_try_and_get((long)state->object_index);

	if (!object)
		return;
	object->object.body_vitality = state->body_vitality;
	object->object.shield_vitality = state->shield_vitality;
	object->object.current_shield_damage = state->current_shield_damage;
	object->object.current_body_damage = state->current_body_damage;
	object->object.recent_shield_damage = state->recent_shield_damage;
	object->object.recent_body_damage = state->recent_body_damage;
	object->object.shield_damage_decay_timer = state->shield_damage_decay_timer;
	object->object.body_damage_decay_timer = state->body_damage_decay_timer;
	object->object.shield_stun_ticks = state->shield_stun_ticks;
	object->object.damage_flags = state->damage_flags;
	memcpy(object->object.region_damage, state->region_damage, sizeof(state->region_damage));
}

static long halo_sim_damage_object_callback(unsigned long index, struct halo_sim_object_damage_state *out)
{
	return halo_sim_object_damage_state_load((long)index, out) ? 1 : 0;
}

static long halo_sim_damage_call_callback(struct halo_sim_damage_data *damage, struct halo_sim_object_damage_state *objects,
	unsigned long count, struct halo_sim_damage_call *call)
{
	struct damage_data *active = halo_sim_damage_active;
	unsigned long object_number;
	long result;

	if (!active)
		return 0;
	halo_sim_damage_data_store(active, damage);
	for (object_number = 0; object_number < count; object_number++)
		halo_sim_object_damage_state_store(&objects[object_number]);
	result = damage_call(active, call->kind, call->object_index, call->a, call->b, call->f, &call->out);
	halo_sim_damage_data_load(active, damage);
	for (object_number = 0; object_number < count; object_number++)
	{
		struct halo_sim_object_damage_state state;

		if (halo_sim_object_damage_state_load((long)objects[object_number].object_index, &state))
			objects[object_number] = state;
	}
	return result;
}

/* damage.c's, area damage's call of the rest of the game */
long area_damage_call(struct damage_data *damage, long kind, long object_index, long a, long b, real const *f, real *out);

/* the area damage Rust deals, while it runs (the game's caller's) */
static struct damage_data *halo_sim_area_damage_active;

/* an object as area damage reads it: FALSE if it is gone */
static boolean halo_sim_area_object_load(long object_index, struct halo_sim_area_object *state)
{
	struct object_datum *object = object_try_and_get(object_index);

	memset(state, 0, sizeof(*state));
	state->object_index = (unsigned long)object_index;
	if (!object)
	{
		state->object_type = NONE;
		return FALSE;
	}
	state->definition_index = (unsigned long)object->definition_index;
	state->object_type = object->object.type;
	state->owner_team_index = object->object.owner_team_index;
	state->flags = object->object.flags;
	state->bounding_center = object->object.bounding_sphere_center;
	state->bounding_radius = object->object.bounding_sphere_radius;
	state->parent_object_index = object->object.parent_object_index;
	state->first_child_object_index = object->object.first_child_object_index;
	state->next_object_index = object->object.next_object_index;

	return TRUE;
}

/* a cluster's objects in a partition (0 collideable, 1 not), in order */
static unsigned long halo_sim_area_cluster_objects(short cluster, unsigned long partition, unsigned long *out,
	unsigned long maximum)
{
	unsigned long count = 0;
	long reference;
	long object_index;

	if (partition == 0)
	{
		for (object_index = cluster_get_first_collideable_object(&reference, cluster);
			object_index != NONE && count < maximum;
			object_index = cluster_get_next_collideable_object(&reference))
		{
			out[count++] = (unsigned long)object_index;
		}
	}
	else
	{
		for (object_index = cluster_get_first_noncollideable_object(&reference, cluster);
			object_index != NONE && count < maximum;
			object_index = cluster_get_next_noncollideable_object(&reference))
		{
			out[count++] = (unsigned long)object_index;
		}
	}
	return count;
}

static long halo_sim_area_object_callback(unsigned long index, struct halo_sim_area_object *out)
{
	return halo_sim_area_object_load((long)index, out) ? 1 : 0;
}

static long halo_sim_area_call_callback(struct halo_sim_damage_data *damage, struct halo_sim_area_call *call)
{
	struct damage_data *active = halo_sim_area_damage_active;
	long result;

	if (!active)
		return 0;
	halo_sim_damage_data_store(active, damage);
	result = area_damage_call(active, call->kind, call->object_index, call->a, call->b, call->f, call->out);
	call->result = result;
	halo_sim_damage_data_load(active, damage);
	return result;
}

/* damage.c's, the damage update's call of the rest of the game */
long damage_update_call(long object_index, long kind, real f, real *out);

static long halo_sim_damage_update_callback(struct halo_sim_object_damage_state *state, long kind, real f, real *out)
{
	long object_index = (long)state->object_index;
	long result;

	halo_sim_object_damage_state_store(state);
	result = damage_update_call(object_index, kind, f, out);
	if (!halo_sim_object_damage_state_load(object_index, state))
		return -1;
	return result;
}

/* devices.c's, a device update's call of the rest of the game */
long device_call(long device_index, long kind, long object_index, long a, long b, real const *f);

/* a device's state, from the device: FALSE if it is gone */
static boolean halo_sim_device_state_load(long device_index, struct halo_sim_device_state *state)
{
	struct device_datum *device = object_try_and_get_and_verify_type(device_index, _object_mask_device);

	memset(state, 0, sizeof(*state));
	state->object_index = (unsigned long)device_index;
	if (!device)
		return FALSE;
	state->definition_index = (unsigned long)device->definition_index;
	state->location_cluster = device->object.location.cluster_index;
	state->power_group_index = device->device.power_group_index;
	state->device_flags = device->device.flags;
	state->power = device->device.power;
	state->power_velocity = device->device.power_velocity;
	state->position_group_index = device->device.position_group_index;
	state->delay_ticks = device->device.delay_ticks;
	state->position = device->device.position;
	state->position_velocity = device->device.position_velocity;
	if (device->object.type == _object_type_machine)
	{
		struct machine_datum *machine = (struct machine_datum *)device;

		state->machine_flags = machine->machine.flags;
		state->door_open_ticks = machine->machine.door_open_ticks;
		state->elevator_position = machine->machine.elevator_position;
	}
	state->object_position = device->object.position;
	state->forward = device->object.forward;
	state->bounding_center = device->object.bounding_sphere_center;
	state->bounding_radius = device->object.bounding_sphere_radius;

	return TRUE;
}

/* and back to it: what its update changes */
static void halo_sim_device_state_store(struct halo_sim_device_state const *state)
{
	struct device_datum *device = object_try_and_get_and_verify_type((long)state->object_index, _object_mask_device);

	if (!device)
		return;
	device->device.flags = state->device_flags;
	device->device.power = state->power;
	device->device.power_velocity = state->power_velocity;
	device->device.delay_ticks = state->delay_ticks;
	device->device.position = state->position;
	device->device.position_velocity = state->position_velocity;
	if (device->object.type == _object_type_machine)
	{
		struct machine_datum *machine = (struct machine_datum *)device;

		machine->machine.door_open_ticks = state->door_open_ticks;
		machine->machine.elevator_position = state->elevator_position;
	}
}

/* a biped near a machine, as a door or an elevator reads it */
static boolean halo_sim_device_biped_load(long biped_index, struct halo_sim_device_biped *out)
{
	struct biped_datum *biped = object_try_and_get_and_verify_type(biped_index, _object_mask_biped);

	memset(out, 0, sizeof(*out));
	out->object_index = (unsigned long)biped_index;
	if (!biped)
		return FALSE;
	{
		struct unit_definition *unit_definition = unit_definition_get(biped->definition_index);

		out->cannot_open_doors =
			/* (device_machines.c's: _object_function2_active_bit 2, and
			_object_runtime_unit_cannot_open_doors_automatically_bit 14) */
			(TEST_FLAG(*((byte *)biped + 0xb6), 2) ||
				TEST_FLAG(*(unsigned long *)((byte *)unit_definition + 0x17c), 14)) ? 1 : 0;
	}
	out->owner_team_index = biped->object.owner_team_index;
	out->bounding_center = biped->object.bounding_sphere_center;
	out->position = biped->object.position;
	out->elevator_object_index = biped->biped.elevator_object_index;

	return TRUE;
}

static long halo_sim_device_call_callback(struct halo_sim_device_state *state, struct halo_sim_device_call const *call)
{
	long device_index = (long)state->object_index;
	long result;

	halo_sim_device_state_store(state);
	result = device_call(device_index, call->kind, call->object_index, call->a, call->b, call->f);
	halo_sim_device_state_load(device_index, state);
	return result;
}

static long halo_sim_device_biped_callback(unsigned long index, struct halo_sim_device_biped *out)
{
	return halo_sim_device_biped_load((long)index, out) ? 1 : 0;
}

/* items.c's, an item update's call of the rest of the game */
long item_call(long item_index, long kind, long a, long b, long c, real const *f);

/* an item's state, from the item: FALSE if it is gone */
static boolean halo_sim_item_state_load(long item_index, struct halo_sim_item_state *state)
{
	struct item_datum *item = item_try_and_get(item_index);

	memset(state, 0, sizeof(*state));
	state->object_index = (unsigned long)item_index;
	if (!item)
		return FALSE;
	state->definition_index = (unsigned long)item->definition_index;
	state->object_flags = item->object.flags;
	state->owner_player_index = item->object.owner_player_index;
	state->parent_object_index = item->object.parent_object_index;
	state->position = item->object.position;
	state->forward = item->object.forward;
	state->up = item->object.up;
	state->translational_velocity = item->object.translational_velocity;
	state->angular_velocity = item->object.angular_velocity;
	memcpy(state->region_permutations, item->object.region_permutations, sizeof(state->region_permutations));
	state->item_flags = item->item.flags;
	state->detonation_ticks = item->item.detonation_ticks;
	state->rested_surface_index = item->item.rested_surface_index;
	state->bsp_index = item->item.bsp_index;
	state->ignore_object_index = item->item.ignore_object_index;
	state->last_owned_time = item->item.last_owned_time;
	state->item_on_rest_object_index = item->item.item_on_rest_object_index;
	state->item_rest_object_offset = item->item.item_rest_object_offset;
	state->rotation_axis = item->item.rotation_axis;
	state->rotation_sine = item->item.rotation_sine;
	state->rotation_cosine = item->item.rotation_cosine;

	return TRUE;
}

/* and back to it: what its update changes */
static void halo_sim_item_state_store(struct halo_sim_item_state const *state)
{
	struct item_datum *item = item_try_and_get((long)state->object_index);

	if (!item)
		return;
	item->object.flags = state->object_flags;
	item->object.position = state->position;
	item->object.forward = state->forward;
	item->object.up = state->up;
	item->object.translational_velocity = state->translational_velocity;
	item->object.angular_velocity = state->angular_velocity;
	item->item.flags = state->item_flags;
	item->item.detonation_ticks = state->detonation_ticks;
	item->item.rested_surface_index = state->rested_surface_index;
	item->item.bsp_index = state->bsp_index;
	item->item.ignore_object_index = state->ignore_object_index;
	item->item.last_owned_time = state->last_owned_time;
	item->item.item_on_rest_object_index = state->item_on_rest_object_index;
	item->item.item_rest_object_offset = state->item_rest_object_offset;
	item->item.rotation_axis = state->rotation_axis;
	item->item.rotation_sine = state->rotation_sine;
	item->item.rotation_cosine = state->rotation_cosine;
}

static long halo_sim_item_call_callback(struct halo_sim_item_state *state, struct halo_sim_item_call const *call)
{
	long item_index = (long)state->object_index;
	long result;

	halo_sim_item_state_store(state);
	result = item_call(item_index, call->kind, call->a, call->b, call->c, call->f);
	if (result)
		halo_sim_item_state_load(item_index, state);
	return result;
}

/* weapons.c's, a weapon update's call of its helpers */
long weapon_update_call(long weapon_index, long kind, long a, long b);

/* a weapon's state, from the weapon: FALSE if it is gone */
static boolean halo_sim_weapon_state_load(long weapon_index, struct halo_sim_weapon_state *state)
{
	struct weapon_datum *weapon = weapon_try_and_get(weapon_index);

	memset(state, 0, sizeof(*state));
	state->object_index = (unsigned long)weapon_index;
	if (!weapon)
		return FALSE;
	state->definition_index = (unsigned long)weapon->definition_index;
	state->parent_object_index = weapon->object.parent_object_index;
	state->item_flags = weapon->item.flags;
	state->flags = weapon->weapon.flags;
	state->control_flags = weapon->weapon.control_flags;
	state->primary_trigger = weapon->weapon.primary_trigger;
	state->state = weapon->weapon.state;
	state->state_timer = weapon->weapon.state_timer;
	state->heat = weapon->weapon.heat;
	state->age = weapon->weapon.age;
	state->overcharged = weapon->weapon.overcharged;
	state->integrated_light_power = weapon->weapon.integrated_light_power;
	state->tracked_object_index = weapon->weapon.tracked_object_index;
	state->alternate_shots_loaded = weapon->weapon.alternate_shots_loaded;
	state->animation_index = weapon->object.animation.state.index;
	state->animation_frame_index = weapon->object.animation.state.frame_index;
	memcpy(state->triggers, weapon->weapon.triggers, sizeof(state->triggers));
	memcpy(state->magazines, weapon->weapon.magazines, sizeof(state->magazines));
	state->overheated_effect_index = weapon->weapon.overheated_effect_index;

	return TRUE;
}

/* and back to it: what its update changes */
static void halo_sim_weapon_state_store(struct halo_sim_weapon_state const *state)
{
	struct weapon_datum *weapon = weapon_try_and_get((long)state->object_index);

	if (!weapon)
		return;
	weapon->weapon.flags = state->flags;
	weapon->weapon.state_timer = state->state_timer;
	weapon->weapon.heat = state->heat;
	weapon->weapon.overcharged = state->overcharged;
	weapon->weapon.integrated_light_power = state->integrated_light_power;
	weapon->weapon.tracked_object_index = state->tracked_object_index;
	weapon->object.animation.state.index = state->animation_index;
	weapon->object.animation.state.frame_index = state->animation_frame_index;
	memcpy(weapon->weapon.triggers, state->triggers, sizeof(state->triggers));
	memcpy(weapon->weapon.magazines, state->magazines, sizeof(state->magazines));
}

static long halo_sim_weapon_call_callback(struct halo_sim_weapon_state *state, long kind, long a, long b)
{
	long weapon_index = (long)state->object_index;
	long result;

	halo_sim_weapon_state_store(state);
	result = weapon_update_call(weapon_index, kind, a, b);
	if (result)
		halo_sim_weapon_state_load(weapon_index, state);
	return result;
}

static long halo_sim_objects_call_callback(struct halo_sim_objects_call *call)
{
	objects_update_call(call);
	return 0;
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
	{
		halo_sim_unit_update_callback,
	},
	{
		halo_sim_rest_callback,
	},
	{
		halo_sim_damage_object_callback,
		halo_sim_damage_call_callback,
	},
	{
		halo_sim_area_cluster_objects,
		halo_sim_area_object_callback,
		halo_sim_area_call_callback,
	},
	{
		halo_sim_damage_update_callback,
	},
	{
		halo_sim_device_call_callback,
		halo_sim_device_biped_callback,
	},
	{
		halo_sim_item_call_callback,
	},
	{
		halo_sim_weapon_call_callback,
	},
	{
		halo_sim_objects_call_callback,
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
	halo_sim.unit_update = (halo_sim_unit_update_function)halo_sim_symbol(library, "halo_sim_unit_update");
	halo_sim.unit_update_check = (halo_sim_unit_update_function)halo_sim_symbol(library, "halo_sim_unit_update_check");
	halo_sim.biped_rest = (halo_sim_rest_function)halo_sim_symbol(library, "halo_sim_biped_rest");
	halo_sim.biped_rest_check = (halo_sim_rest_function)halo_sim_symbol(library, "halo_sim_biped_rest_check");
	halo_sim.object_cause_damage = (halo_sim_damage_function)halo_sim_symbol(library, "halo_sim_object_cause_damage");
	halo_sim.object_cause_damage_check = (halo_sim_damage_function)halo_sim_symbol(library, "halo_sim_object_cause_damage_check");
	halo_sim.area_damage = (halo_sim_area_damage_function)halo_sim_symbol(library, "halo_sim_area_of_effect_cause_damage");
	halo_sim.area_damage_check = (halo_sim_area_damage_function)halo_sim_symbol(library, "halo_sim_area_of_effect_cause_damage_check");
	halo_sim.damage_update = (halo_sim_damage_update_function)halo_sim_symbol(library, "halo_sim_object_damage_update");
	halo_sim.damage_update_check = (halo_sim_damage_update_function)halo_sim_symbol(library, "halo_sim_object_damage_update_check");
	halo_sim.device_update = (halo_sim_device_function)halo_sim_symbol(library, "halo_sim_device_update");
	halo_sim.device_update_check = (halo_sim_device_function)halo_sim_symbol(library, "halo_sim_device_update_check");
	halo_sim.item_update = (halo_sim_item_function)halo_sim_symbol(library, "halo_sim_item_update");
	halo_sim.item_update_check = (halo_sim_item_function)halo_sim_symbol(library, "halo_sim_item_update_check");
	halo_sim.weapon_update = (halo_sim_weapon_function)halo_sim_symbol(library, "halo_sim_weapon_update");
	halo_sim.weapon_update_check = (halo_sim_weapon_function)halo_sim_symbol(library, "halo_sim_weapon_update_check");
	halo_sim.objects_update = (halo_sim_objects_function)halo_sim_symbol(library, "halo_sim_objects_update");
	halo_sim.objects_update_check = (halo_sim_objects_function)halo_sim_symbol(library, "halo_sim_objects_update_check");
	halo_sim.garbage_update = (halo_sim_garbage_function)halo_sim_symbol(library, "halo_sim_garbage_update");
	halo_sim.scenery_update = (halo_sim_scenery_function)halo_sim_symbol(library, "halo_sim_scenery_update");
	halo_sim.object_functions = (halo_sim_object_functions_function)halo_sim_symbol(library, "halo_sim_object_functions");
	if (!halo_sim.load_map || !halo_sim.point_physics || !halo_sim.set_tag_cache || !halo_sim.pose_orientations ||
		!halo_sim.pose_matrices || !halo_sim.pose_preprocess || !halo_sim.set_host || !halo_sim.collision_test_vector ||
		!halo_sim.projectile_update || !halo_sim.projectile_check || !halo_sim.biped_physics ||
		!halo_sim.vehicle_physics_update || !halo_sim.vehicle_physics_check ||
		!halo_sim.vehicle_update || !halo_sim.vehicle_update_check ||
		!halo_sim.biped_moving || !halo_sim.biped_moving_check ||
		!halo_sim.biped_update || !halo_sim.biped_update_check ||
		!halo_sim.unit_animation || !halo_sim.unit_animation_check ||
		!halo_sim.player_melee || !halo_sim.player_melee_check ||
		!halo_sim.unit_update || !halo_sim.unit_update_check ||
		!halo_sim.biped_rest || !halo_sim.biped_rest_check ||
		!halo_sim.object_cause_damage || !halo_sim.object_cause_damage_check ||
		!halo_sim.area_damage || !halo_sim.area_damage_check ||
		!halo_sim.damage_update || !halo_sim.damage_update_check ||
		!halo_sim.device_update || !halo_sim.device_update_check ||
		!halo_sim.item_update || !halo_sim.item_update_check ||
		!halo_sim.weapon_update || !halo_sim.weapon_update_check ||
		!halo_sim.objects_update || !halo_sim.objects_update_check || !halo_sim.garbage_update || !halo_sim.scenery_update ||
		!halo_sim.object_functions)
	{
		platform_log("halo_sim: %s lacks an entry point; the game's own simulation runs", path);
		return;
	}
	halo_sim.loaded = TRUE;
	halo_sim.set_host(&halo_sim_host_callbacks);

	for (system = 0; system < NUMBER_OF_HALO_SIM_SYSTEMS; system++)
	{
		char const *name = halo_sim_system_names[system];
		size_t length = strlen(name);
		char const *found = strstr(systems, name);

		/* (the name itself, not within another: damage in area_damage) */
		while (found && !((found == systems || found[-1] == ',') &&
			(found[length] == 0 || found[length] == ',' || found[length] == ':')))
		{
			found = strstr(found + 1, name);
		}
		if (found)
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

/* ---------- unit updates */

static void halo_sim_controller_load(long unit_index, boolean gunner, struct halo_sim_controller *controller)
{
	struct unit_datum *unit = unit_get(unit_index);

	memset(controller, 0, sizeof(*controller));
	controller->owner_team_index = unit->object.owner_team_index;
	controller->controls = unit->unit.player_index != NONE ||
		(unit->unit.animation.state != _unit_state_exiting_seat && unit->unit.animation.state != _unit_state_entering_seat);
	controller->control_flags = unit->unit.control_flags;
	controller->desired_facing_vector = unit->unit.desired_facing_vector;
	controller->desired_aiming_vector = unit->unit.desired_aiming_vector;
	controller->throttle = unit->unit.throttle;
	controller->primary_trigger = unit->unit.primary_trigger;
}

static void halo_sim_unit_update_call_new(long unit_index, void *unit_globals, int allow_integrated_lights,
	int game_engine_running, int cheat_jetpack, int cheat_infinite_ammo, struct halo_sim_unit_update_call *call)
{
	struct unit_datum *unit = unit_get(unit_index);
	long weapon_index;

	memset(call, 0, sizeof(*call));
	call->unit_index = (unsigned long)unit_index;
	call->definition_index = (unsigned long)unit->definition_index;
	halo_sim_unit_update_state_load(unit_index, &call->state);
	call->globals = *(struct halo_sim_unit_globals const *)unit_globals;
	call->allow_integrated_lights = allow_integrated_lights ? 1 : 0;
	call->game_engine_running = game_engine_running ? 1 : 0;
	call->cheat_jetpack = cheat_jetpack ? 1 : 0;
	call->cheat_infinite_ammo = cheat_infinite_ammo ? 1 : 0;
	if (unit->unit.driver_object_index != NONE)
	{
		call->has_driver = 1;
		halo_sim_controller_load(unit->unit.driver_object_index, FALSE, &call->driver);
	}
	if (unit->unit.gunner_object_index != NONE)
	{
		call->has_gunner = 1;
		halo_sim_controller_load(unit->unit.gunner_object_index, TRUE, &call->gunner);
	}
	weapon_index = unit->unit.current_weapon_index != NONE ?
		unit->unit.weapon_object_indices[unit->unit.current_weapon_index] : NONE;
	call->weapon_definition_index = weapon_index != NONE ? (unsigned long)object_get(weapon_index)->definition_index : NONE;
	call->difference_event = NONE;
}

int halo_sim_unit_update(long unit_index, void *unit_globals, int allow_integrated_lights, int game_engine_running,
	int cheat_jetpack, int cheat_infinite_ammo)
{
	short mode = halo_sim_mode(_halo_sim_unit_update);
	struct halo_sim_unit_update_call call;

	halo_sim_unit_update_checks.recording = FALSE;
	if (mode == _halo_sim_mode_c)
		return FALSE;
	halo_sim_unit_update_call_new(unit_index, unit_globals, allow_integrated_lights, game_engine_running, cheat_jetpack,
		cheat_infinite_ammo, mode == _halo_sim_mode_check ? &halo_sim_unit_update_checks.call : &call);
	if (mode == _halo_sim_mode_check)
	{
		halo_sim_unit_update_checks.unit_index = unit_index;
		halo_sim_unit_update_checks.globals = (struct halo_sim_unit_globals *)unit_globals;
		halo_sim_unit_update_checks.event_count = 0;
		halo_sim_unit_update_checks.open_event = NULL;
		halo_sim_unit_update_checks.overflow = FALSE;
		halo_sim_unit_update_checks.recording = TRUE;
		return FALSE;
	}

	if (halo_sim.unit_update(&call) != 0)
		return FALSE;
	halo_sim_unit_update_state_store(unit_index, &call.state);
	*(struct halo_sim_unit_globals *)unit_globals = call.globals;

	return TRUE;
}

void halo_sim_unit_event_begin(long unit_index, long kind, long a, float b)
{
	struct halo_sim_unit_update_event *event;

	if (!halo_sim_unit_update_checks.recording || halo_sim_unit_update_checks.unit_index != unit_index ||
		halo_sim_unit_update_checks.open_event)
	{
		return;
	}
	if (halo_sim_unit_update_checks.event_count >= MAXIMUM_HALO_SIM_UNIT_EVENTS)
	{
		halo_sim_unit_update_checks.overflow = TRUE;
		return;
	}
	event = &halo_sim_unit_update_checks.events[halo_sim_unit_update_checks.event_count++];
	memset(event, 0, sizeof(*event));
	event->kind = kind;
	event->a = a;
	event->b = b;
	halo_sim_unit_update_state_load(unit_index, &event->before);
	halo_sim_unit_update_checks.open_event = event;
}

void halo_sim_unit_event_end(long unit_index, long result)
{
	struct halo_sim_unit_update_event *event = halo_sim_unit_update_checks.open_event;

	if (!event || halo_sim_unit_update_checks.unit_index != unit_index)
		return;
	halo_sim_unit_update_checks.open_event = NULL;
	event->result = result;
	event->seed = get_random_seed();
	/* (a call may delete the unit: died, flamed to death) */
	if (object_try_and_get_and_verify_type(unit_index, _object_mask_unit))
		halo_sim_unit_update_state_load(unit_index, &event->after);
	else
		halo_sim_unit_update_checks.overflow = TRUE;
}

void halo_sim_unit_update_check(long unit_index)
{
	struct halo_sim_unit_update_call call;
	struct halo_sim_unit_update_state game;

	if (!halo_sim_unit_update_checks.recording)
		return;
	halo_sim_unit_update_checks.recording = FALSE;
	if (halo_sim_unit_update_checks.unit_index != unit_index || halo_sim_unit_update_checks.overflow ||
		!object_try_and_get_and_verify_type(unit_index, _object_mask_unit))
	{
		halo_sim_unit_update_checks.skipped++;
		return;
	}
	call = halo_sim_unit_update_checks.call;
	call.events = halo_sim_unit_update_checks.events;
	call.event_count = halo_sim_unit_update_checks.event_count;
	if (halo_sim.unit_update_check(&call) != 0)
	{
		halo_sim_unit_update_checks.skipped++;
		return;
	}

	halo_sim_unit_update_state_load(unit_index, &game);
	halo_sim_unit_update_checks.compared++;
	halo_sim_unit_update_checks.calls += halo_sim_unit_update_checks.event_count;
	if (memcmp(&call.state, &game, sizeof(game)) != 0 || call.difference_event != NONE ||
		memcmp(&call.globals, halo_sim_unit_update_checks.globals, 5) != 0)
	{
		halo_sim_unit_update_checks.differed++;
		if (halo_sim_unit_update_checks.differed <= 8)
		{
			unsigned char const *r = (unsigned char const *)&call.state;
			unsigned char const *g = (unsigned char const *)&game;
			unsigned long offset = 0;

			while (offset < sizeof(game) && r[offset] == g[offset])
				offset++;
			platform_log("halo_sim: unit update differs: unit %lx (definition %lx) call %ld of %lu (kind %ld) reason %ld; first differing byte %lu of %lu; timer game %d rust %d",
				(unsigned long)unit_index, call.definition_index, call.difference_event, halo_sim_unit_update_checks.event_count,
				call.difference_event >= 0 && call.difference_event < (long)halo_sim_unit_update_checks.event_count ?
					halo_sim_unit_update_checks.events[call.difference_event].kind : NONE,
				call.difference_reason, offset, (unsigned long)sizeof(game), game.timer, call.state.timer);
			platform_log("halo_sim:   game aiming (%f %f %f) looking (%f %f %f) flags %lx controls %lx",
				game.aiming_vector.i, game.aiming_vector.j, game.aiming_vector.k,
				game.looking_vector.i, game.looking_vector.j, game.looking_vector.k, game.flags, game.control_flags);
			platform_log("halo_sim:   rust aiming (%f %f %f) looking (%f %f %f) flags %lx controls %lx",
				call.state.aiming_vector.i, call.state.aiming_vector.j, call.state.aiming_vector.k,
				call.state.looking_vector.i, call.state.looking_vector.j, call.state.looking_vector.k,
				call.state.flags, call.state.control_flags);
		}
	}
	if ((halo_sim_unit_update_checks.compared & 0x3fff) == 0 ||
		(halo_sim_unit_update_checks.compared & (halo_sim_unit_update_checks.compared - 1)) == 0)
	{
		platform_log("halo_sim: unit updates: %lu of %lu agree with the game (%lu calls of the game; %lu left out)",
			halo_sim_unit_update_checks.compared - halo_sim_unit_update_checks.differed, halo_sim_unit_update_checks.compared,
			halo_sim_unit_update_checks.calls, halo_sim_unit_update_checks.skipped);
	}
}

/* ---------- the rest of biped updates */

extern boolean rider_ejection; /* bipeds.c */

static void halo_sim_rest_call_new(long biped_index, struct unit_animation_update_data const *animation, int riding,
	struct halo_sim_rest_call *call)
{
	struct biped_datum *biped = biped_get(biped_index);

	memset(call, 0, sizeof(*call));
	call->biped_index = (unsigned long)biped_index;
	call->definition_index = (unsigned long)biped->definition_index;
	call->riding = riding ? 1 : 0;
	halo_sim_rest_state_load(biped_index, animation, &call->state);
	call->game_time = game_time_get();
	call->game_engine_running = game_engine_running() ? 1 : 0;
	call->maximum_damage_velocity = biped_rust_maximum_damage_velocity();
	call->rider_ejection = rider_ejection ? 1 : 0;
	if (biped->object.parent_object_index != NONE)
	{
		struct object_datum *parent = object_get(biped->object.parent_object_index);

		if (parent->object.type == _object_type_vehicle)
		{
			call->parent_kind = 1;
			call->vehicle_up_k = parent->object.up.k;
			call->vehicle_on_ground = TEST_FLAG(parent->object.flags, _object_on_ground_bit) ? 1 : 0;
		}
		else if (parent->object.type == _object_type_biped)
		{
			call->parent_kind = 2;
			call->parent_dead = TEST_FLAG(parent->object.damage_flags, _object_dead_bit) ? 1 : 0;
		}
		else
		{
			call->parent_kind = 3;
		}
	}
	call->difference_event = NONE;
}

int halo_sim_biped_rest(long biped_index, void *animation, int riding)
{
	short mode = halo_sim_mode(_halo_sim_biped_rest);
	struct unit_animation_update_data *data = (struct unit_animation_update_data *)animation;
	struct halo_sim_rest_call call;
	long result;

	halo_sim_rest_checks.recording = FALSE;
	if (mode == _halo_sim_mode_c)
		return FALSE;
	halo_sim_rest_call_new(biped_index, data, riding, mode == _halo_sim_mode_check ? &halo_sim_rest_checks.call : &call);
	if (mode == _halo_sim_mode_check)
	{
		halo_sim_rest_checks.biped_index = biped_index;
		halo_sim_rest_checks.animation = data;
		halo_sim_rest_checks.event_count = 0;
		halo_sim_rest_checks.open_event = NULL;
		halo_sim_rest_checks.overflow = FALSE;
		halo_sim_rest_checks.recording = TRUE;
		return FALSE;
	}

	halo_sim_rest_animation = data;
	result = halo_sim.biped_rest(&call);
	halo_sim_rest_animation = NULL;
	if (result != 0)
		return FALSE;
	if (call.still_there && object_try_and_get_and_verify_type(biped_index, _object_mask_biped))
		halo_sim_rest_state_store(biped_index, data, &call.state);

	return TRUE;
}

void halo_sim_biped_rest_event_begin(long biped_index, long kind, long a, long b)
{
	struct halo_sim_rest_event *event;

	if (!halo_sim_rest_checks.recording || halo_sim_rest_checks.biped_index != biped_index || halo_sim_rest_checks.open_event)
		return;
	if (halo_sim_rest_checks.event_count >= MAXIMUM_HALO_SIM_REST_EVENTS)
	{
		halo_sim_rest_checks.overflow = TRUE;
		return;
	}
	event = &halo_sim_rest_checks.events[halo_sim_rest_checks.event_count++];
	memset(event, 0, sizeof(*event));
	event->kind = kind;
	event->a = a;
	event->b = b;
	halo_sim_rest_state_load(biped_index, halo_sim_rest_checks.animation, &event->before);
	halo_sim_rest_checks.open_event = event;
}

void halo_sim_biped_rest_event_end(long biped_index, long result)
{
	struct halo_sim_rest_event *event = halo_sim_rest_checks.open_event;

	if (!event || halo_sim_rest_checks.biped_index != biped_index)
		return;
	halo_sim_rest_checks.open_event = NULL;
	event->result = result;
	if (object_try_and_get_and_verify_type(biped_index, _object_mask_biped))
		halo_sim_rest_state_load(biped_index, halo_sim_rest_checks.animation, &event->after);
	else
		halo_sim_rest_checks.overflow = TRUE;
}

void halo_sim_biped_rest_check(long biped_index, void const *animation)
{
	struct halo_sim_rest_call call;
	struct halo_sim_rest_state game;

	if (!halo_sim_rest_checks.recording)
		return;
	halo_sim_rest_checks.recording = FALSE;
	if (halo_sim_rest_checks.biped_index != biped_index || halo_sim_rest_checks.overflow ||
		!object_try_and_get_and_verify_type(biped_index, _object_mask_biped))
	{
		halo_sim_rest_checks.skipped++;
		return;
	}
	call = halo_sim_rest_checks.call;
	call.events = halo_sim_rest_checks.events;
	call.event_count = halo_sim_rest_checks.event_count;
	if (halo_sim.biped_rest_check(&call) != 0)
	{
		halo_sim_rest_checks.skipped++;
		return;
	}

	halo_sim_rest_state_load(biped_index, (struct unit_animation_update_data const *)animation, &game);
	halo_sim_rest_checks.compared[call.riding ? 1 : 0]++;
	halo_sim_rest_checks.calls += halo_sim_rest_checks.event_count;
	if (memcmp(&call.state, &game, sizeof(game)) != 0 || call.difference_event != NONE)
	{
		halo_sim_rest_checks.differed++;
		if (halo_sim_rest_checks.differed <= 8)
		{
			platform_log("halo_sim: biped rest differs: biped %lx riding %lu call %ld of %lu (kind %ld) reason %ld; stop ticks game %d rust %d; falling time game %ld rust %ld; desired game %d rust %d",
				(unsigned long)biped_index, call.riding, call.difference_event, halo_sim_rest_checks.event_count,
				call.difference_event >= 0 && call.difference_event < (long)halo_sim_rest_checks.event_count ?
					halo_sim_rest_checks.events[call.difference_event].kind : NONE,
				call.difference_reason, game.stop_ticks, call.state.stop_ticks, game.last_falling_communication_time,
				call.state.last_falling_communication_time, game.state_desired, call.state.state_desired);
		}
	}
	{
		unsigned long compared = halo_sim_rest_checks.compared[0] + halo_sim_rest_checks.compared[1];

		if ((compared & 0x3fff) == 0 || (compared & (compared - 1)) == 0)
		{
			platform_log("halo_sim: biped rest: %lu of %lu agree with the game (%lu riding; %lu calls of the game; %lu left out)",
				compared - halo_sim_rest_checks.differed, compared, halo_sim_rest_checks.compared[1],
				halo_sim_rest_checks.calls, halo_sim_rest_checks.skipped);
		}
	}
}

/* ---------- damage */

/* the check's snapshot: an object, if it is there and not yet in it */
static void halo_sim_damage_snapshot_add(long object_index)
{
	unsigned long object_number;

	if (object_index == NONE || !object_try_and_get(object_index))
		return;
	for (object_number = 0; object_number < halo_sim_damage_checks.snapshot_count; object_number++)
	{
		if (halo_sim_damage_checks.snapshot[object_number].object_index == (unsigned long)object_index)
			return;
	}
	if (halo_sim_damage_checks.snapshot_count >= MAXIMUM_HALO_SIM_DAMAGE_SNAPSHOT)
	{
		halo_sim_damage_checks.overflow = TRUE;
		return;
	}
	halo_sim_object_damage_state_load(object_index,
		&halo_sim_damage_checks.snapshot[halo_sim_damage_checks.snapshot_count++]);
}

/* what damage may read: the object, up its parents, its children (a
vehicle's riders), and each one's umbrella shield */
static void halo_sim_damage_snapshot(long object_index)
{
	unsigned long object_number;
	long index;

	halo_sim_damage_checks.snapshot_count = 0;
	for (index = object_index; index != NONE; index = object_get(index)->object.parent_object_index)
	{
		halo_sim_damage_snapshot_add(index);
		if (halo_sim_damage_checks.snapshot_count >= 16)
			break;
	}
	for (index = object_get(object_index)->object.first_child_object_index; index != NONE;
		index = object_get(index)->object.next_object_index)
	{
		halo_sim_damage_snapshot_add(index);
	}
	for (object_number = 0; object_number < halo_sim_damage_checks.snapshot_count; object_number++)
	{
		halo_sim_damage_snapshot_add(halo_sim_damage_checks.snapshot[object_number].umbrella_shield_object_index);
	}
}

int halo_sim_object_cause_damage(struct damage_data *damage, long object_index, short node_index, short region_index,
	short material_index, int has_normal, int distributed_damage_authorized, int cheat_omnipotent,
	int cheat_deathless_player)
{
	short mode = halo_sim_mode(_halo_sim_damage);
	struct halo_sim_damage_call_c call;
	struct halo_sim_damage_call_c *target;
	struct damage_data *saved;
	long result;

	if (mode == _halo_sim_mode_c || !object_try_and_get(object_index))
		return FALSE;
	target = mode == _halo_sim_mode_check ? &halo_sim_damage_checks.call : &call;
	memset(target, 0, sizeof(*target));
	halo_sim_damage_data_load(damage, &target->damage);
	target->object_index = (unsigned long)object_index;
	target->node_index = node_index;
	target->region_index = region_index;
	target->material_index = material_index;
	target->has_normal = has_normal ? 1 : 0;
	target->game_engine_running = game_engine_running() ? 1 : 0;
	target->distributed_damage_authorized = distributed_damage_authorized ? 1 : 0;
	target->cheat_omnipotent = cheat_omnipotent ? 1 : 0;
	target->cheat_deathless_player = cheat_deathless_player ? 1 : 0;
	target->random_seed = get_random_seed();
	target->difference_event = NONE;
	if (mode == _halo_sim_mode_check)
	{
		/* (a damage nested in a call of the game's own damage isn't checked) */
		if (halo_sim_damage_checks.recording)
			return FALSE;
		halo_sim_damage_checks.overflow = FALSE;
		halo_sim_damage_snapshot(object_index);
		halo_sim_damage_checks.event_count = 0;
		halo_sim_damage_checks.open_event = NULL;
		halo_sim_damage_checks.recording = TRUE;
		return FALSE;
	}

	/* (a damage dealt from one of Rust's calls of the game is its own) */
	saved = halo_sim_damage_active;
	halo_sim_damage_active = damage;
	result = halo_sim.object_cause_damage(&call);
	halo_sim_damage_active = saved;
	if (result != 0)
		return FALSE;
	halo_sim_damage_data_store(damage, &call.damage);

	return TRUE;
}

void halo_sim_damage_event_begin(struct damage_data const *damage, long kind, long object_index, long a, long b,
	float const *f)
{
	struct halo_sim_damage_event *event;

	if (!halo_sim_damage_checks.recording || halo_sim_damage_checks.open_event)
		return;
	if (halo_sim_damage_checks.event_count >= MAXIMUM_HALO_SIM_DAMAGE_EVENTS)
	{
		halo_sim_damage_checks.overflow = TRUE;
		return;
	}
	event = &halo_sim_damage_checks.events[halo_sim_damage_checks.event_count++];
	memset(event, 0, sizeof(*event));
	event->call.kind = kind;
	event->call.object_index = object_index;
	event->call.a = a;
	event->call.b = b;
	if (f)
	{
		event->call.f[0] = f[0];
		event->call.f[1] = f[1];
		event->call.f[2] = f[2];
	}
	halo_sim_damage_data_load(damage, &event->damage_before);
	if (object_index != NONE)
		halo_sim_object_damage_state_load(object_index, &event->object_before);
	halo_sim_damage_checks.open_event = event;
}

void halo_sim_damage_event_end(struct damage_data const *damage, long object_index, long result, float value)
{
	struct halo_sim_damage_event *event = halo_sim_damage_checks.open_event;

	if (!event)
		return;
	halo_sim_damage_checks.open_event = NULL;
	event->result = result;
	event->call.out = value;
	event->seed = get_random_seed();
	halo_sim_damage_data_load(damage, &event->damage_after);
	if (object_index != NONE)
	{
		/* (an object deleted keeps its state before) */
		if (!halo_sim_object_damage_state_load(object_index, &event->object_after))
			event->object_after = event->object_before;
	}
}

void halo_sim_object_cause_damage_check(struct damage_data const *damage)
{
	struct halo_sim_damage_call_c call;
	struct halo_sim_damage_data game;

	if (!halo_sim_damage_checks.recording)
		return;
	halo_sim_damage_checks.recording = FALSE;
	if (halo_sim_damage_checks.overflow)
	{
		halo_sim_damage_checks.skipped++;
		return;
	}
	call = halo_sim_damage_checks.call;
	call.snapshot = halo_sim_damage_checks.snapshot;
	call.snapshot_count = halo_sim_damage_checks.snapshot_count;
	call.events = halo_sim_damage_checks.events;
	call.event_count = halo_sim_damage_checks.event_count;
	if (halo_sim.object_cause_damage_check(&call) != 0)
	{
		halo_sim_damage_checks.skipped++;
		return;
	}

	halo_sim_damage_data_load(damage, &game);
	halo_sim_damage_checks.compared++;
	halo_sim_damage_checks.calls += halo_sim_damage_checks.event_count;
	if (memcmp(&call.damage, &game, sizeof(game)) != 0 || call.difference_event != NONE)
	{
		halo_sim_damage_checks.differed++;
		if (halo_sim_damage_checks.differed <= 8)
		{
			long kind = call.difference_event >= 0 && call.difference_event < (long)halo_sim_damage_checks.event_count ?
				halo_sim_damage_checks.events[call.difference_event].call.kind : NONE;

			platform_log("halo_sim: damage differs: object %lx definition %lx call %ld of %lu (kind %ld) reason %ld; flags game %lx rust %lx; multiplier game %f rust %f",
				call.object_index, (unsigned long)call.damage.definition_index, call.difference_event,
				halo_sim_damage_checks.event_count, kind, call.difference_reason, game.flags, call.damage.flags,
				(double)game.multiplier, (double)call.damage.multiplier);
		}
	}
	{
		unsigned long compared = halo_sim_damage_checks.compared;

		if ((compared & 0x3fff) == 0 || (compared & (compared - 1)) == 0)
		{
			platform_log("halo_sim: damage: %lu of %lu agree with the game (%lu calls of the game; %lu left out)",
				compared - halo_sim_damage_checks.differed, compared, halo_sim_damage_checks.calls,
				halo_sim_damage_checks.skipped);
		}
	}
}

/* ---------- area damage */

static void halo_sim_area_call_new(struct damage_data const *damage, struct halo_sim_area_call_c *call)
{
	memset(call, 0, sizeof(*call));
	halo_sim_damage_data_load(damage, &call->damage);
	call->structure_bsp_index = global_structure_bsp_index;
	call->breakable_surfaces = (unsigned char const *)breakable_surface_flags_get();
	call->random_seed = get_random_seed();
	call->difference_event = NONE;
}

/* the check's snapshot: an object, if it is there and not yet in it */
static void halo_sim_area_snapshot_add(long object_index)
{
	unsigned long object_number;

	if (object_index == NONE || !object_try_and_get(object_index))
		return;
	for (object_number = 0; object_number < halo_sim_area_checks.snapshot_count; object_number++)
	{
		if (halo_sim_area_checks.snapshot[object_number].object_index == (unsigned long)object_index)
			return;
	}
	if (halo_sim_area_checks.snapshot_count >= MAXIMUM_HALO_SIM_AREA_SNAPSHOT)
	{
		halo_sim_area_checks.overflow = TRUE;
		return;
	}
	halo_sim_area_object_load(object_index, &halo_sim_area_checks.snapshot[halo_sim_area_checks.snapshot_count++]);
}

/* what area damage may read: the clusters its sphere reaches, each one's
objects, and those objects, their children (and theirs) and their parents */
static void halo_sim_area_snapshot(struct damage_data const *damage)
{
	struct damage_effect_definition *definition = damage_effect_definition_get(damage->definition_index);
	unsigned long cluster_number;
	unsigned long object_number;
	short clusters[MAXIMUM_HALO_SIM_AREA_CLUSTERS];
	short cluster_count;

	cluster_count = structure_clusters_in_sphere(damage->location.cluster_index, &damage->origin,
		definition->cutoff_radius, MAXIMUM_HALO_SIM_AREA_CLUSTERS, clusters);
	halo_sim_area_checks.cluster_count = 0;
	halo_sim_area_checks.partition_object_count = 0;
	halo_sim_area_checks.snapshot_count = 0;
	for (cluster_number = 0; cluster_number < (unsigned long)cluster_count; cluster_number++)
	{
		unsigned long partition;

		halo_sim_area_checks.clusters[halo_sim_area_checks.cluster_count] = clusters[cluster_number];
		for (partition = 0; partition < 2; partition++)
		{
			unsigned long room = MAXIMUM_HALO_SIM_AREA_PARTITION_OBJECTS - halo_sim_area_checks.partition_object_count;
			unsigned long count = halo_sim_area_cluster_objects(clusters[cluster_number], partition,
				&halo_sim_area_checks.partition_objects[halo_sim_area_checks.partition_object_count], room);

			if (count >= room)
				halo_sim_area_checks.overflow = TRUE;
			halo_sim_area_checks.partition_counts[2 * halo_sim_area_checks.cluster_count + partition] = count;
			halo_sim_area_checks.partition_object_count += count;
		}
		halo_sim_area_checks.cluster_count++;
	}
	for (object_number = 0; object_number < halo_sim_area_checks.partition_object_count; object_number++)
	{
		halo_sim_area_snapshot_add((long)halo_sim_area_checks.partition_objects[object_number]);
	}
	/* (their children and parents, as they are added) */
	for (object_number = 0; object_number < halo_sim_area_checks.snapshot_count; object_number++)
	{
		struct halo_sim_area_object const *object = &halo_sim_area_checks.snapshot[object_number];
		long child_index;

		halo_sim_area_snapshot_add(object->parent_object_index);
		for (child_index = object->first_child_object_index; child_index != NONE;
			child_index = object_get(child_index)->object.next_object_index)
		{
			halo_sim_area_snapshot_add(child_index);
		}
	}
	memcpy(halo_sim_area_checks.current, halo_sim_area_checks.snapshot,
		halo_sim_area_checks.snapshot_count * sizeof(halo_sim_area_checks.snapshot[0]));
}

int halo_sim_area_of_effect_cause_damage(struct damage_data *damage)
{
	short mode = halo_sim_mode(_halo_sim_area_damage);
	struct halo_sim_area_call_c call;
	struct damage_data *saved;
	long result;

	if (mode == _halo_sim_mode_c)
		return FALSE;
	if (mode == _halo_sim_mode_check)
	{
		/* (area damage within the game's own isn't checked) */
		if (halo_sim_area_checks.recording)
			return FALSE;
		halo_sim_area_call_new(damage, &halo_sim_area_checks.call);
		halo_sim_area_checks.overflow = FALSE;
		halo_sim_area_snapshot(damage);
		halo_sim_area_checks.event_count = 0;
		halo_sim_area_checks.update_count = 0;
		halo_sim_area_checks.open_event = NULL;
		halo_sim_area_checks.recording = TRUE;
		return FALSE;
	}

	halo_sim_area_call_new(damage, &call);
	/* (area damage dealt from one of Rust's calls of the game is its own) */
	saved = halo_sim_area_damage_active;
	halo_sim_area_damage_active = damage;
	result = halo_sim.area_damage(&call);
	halo_sim_area_damage_active = saved;
	if (result != 0)
		return FALSE;
	halo_sim_damage_data_store(damage, &call.damage);

	return TRUE;
}

void halo_sim_area_damage_event_begin(struct damage_data const *damage, long kind, long object_index, long a, long b,
	float const *f)
{
	struct halo_sim_area_event *event;

	if (!halo_sim_area_checks.recording || halo_sim_area_checks.open_event)
		return;
	if (halo_sim_area_checks.event_count >= MAXIMUM_HALO_SIM_AREA_EVENTS)
	{
		halo_sim_area_checks.overflow = TRUE;
		return;
	}
	event = &halo_sim_area_checks.events[halo_sim_area_checks.event_count++];
	memset(event, 0, sizeof(*event));
	event->call.kind = kind;
	event->call.object_index = object_index;
	event->call.a = a;
	event->call.b = b;
	if (f)
		memcpy(event->call.f, f, sizeof(event->call.f));
	halo_sim_damage_data_load(damage, &event->damage_before);
	halo_sim_area_checks.open_event = event;
}

void halo_sim_area_damage_event_end(struct damage_data const *damage, long result, float const *out)
{
	struct halo_sim_area_event *event = halo_sim_area_checks.open_event;

	if (!event)
		return;
	halo_sim_area_checks.open_event = NULL;
	event->call.result = result;
	if (out)
		memcpy(event->call.out, out, sizeof(event->call.out));
	event->seed = get_random_seed();
	halo_sim_damage_data_load(damage, &event->damage_after);
	event->update_first = halo_sim_area_checks.update_count;
	if (event->call.kind == _area_damage_call_cause_damage)
	{
		unsigned long object_number;

		/* (what the damage changed of the objects area damage may read) */
		for (object_number = 0; object_number < halo_sim_area_checks.snapshot_count; object_number++)
		{
			struct halo_sim_area_object *current = &halo_sim_area_checks.current[object_number];
			struct halo_sim_area_object now;

			halo_sim_area_object_load((long)current->object_index, &now);
			if (memcmp(&now, current, sizeof(now)) != 0)
			{
				if (halo_sim_area_checks.update_count >= MAXIMUM_HALO_SIM_AREA_UPDATES)
				{
					halo_sim_area_checks.overflow = TRUE;
					break;
				}
				*current = now;
				halo_sim_area_checks.updates[halo_sim_area_checks.update_count++] = now;
			}
		}
	}
	event->update_count = halo_sim_area_checks.update_count - event->update_first;
}

void halo_sim_area_of_effect_cause_damage_check(struct damage_data const *damage)
{
	struct halo_sim_area_call_c call;
	struct halo_sim_damage_data game;
	unsigned long event_number;

	if (!halo_sim_area_checks.recording)
		return;
	halo_sim_area_checks.recording = FALSE;
	if (halo_sim_area_checks.overflow)
	{
		halo_sim_area_checks.skipped++;
		return;
	}
	call = halo_sim_area_checks.call;
	call.clusters = halo_sim_area_checks.clusters;
	call.cluster_count = halo_sim_area_checks.cluster_count;
	call.partition_counts = halo_sim_area_checks.partition_counts;
	call.partition_objects = halo_sim_area_checks.partition_objects;
	call.snapshot = halo_sim_area_checks.snapshot;
	call.snapshot_count = halo_sim_area_checks.snapshot_count;
	call.events = halo_sim_area_checks.events;
	call.event_count = halo_sim_area_checks.event_count;
	call.updates = halo_sim_area_checks.updates;
	call.update_count = halo_sim_area_checks.update_count;
	if (halo_sim.area_damage_check(&call) != 0)
	{
		halo_sim_area_checks.skipped++;
		return;
	}

	halo_sim_damage_data_load(damage, &game);
	halo_sim_area_checks.compared++;
	halo_sim_area_checks.calls += halo_sim_area_checks.event_count;
	for (event_number = 0; event_number < halo_sim_area_checks.event_count; event_number++)
	{
		long kind = halo_sim_area_checks.events[event_number].call.kind;

		if (kind == _area_damage_call_ray_test)
			halo_sim_area_checks.rays++;
		else if (kind == _area_damage_call_cause_damage)
			halo_sim_area_checks.dealt++;
		else if (kind == _area_damage_call_difficulty_infection_forms)
			halo_sim_area_checks.infection_forms++;
	}
	if (memcmp(&call.damage, &game, sizeof(game)) != 0 || call.difference_event != NONE)
	{
		halo_sim_area_checks.differed++;
		if (halo_sim_area_checks.differed <= 8)
		{
			long kind = call.difference_event >= 0 && call.difference_event < (long)halo_sim_area_checks.event_count ?
				halo_sim_area_checks.events[call.difference_event].call.kind : NONE;

			platform_log("halo_sim: area damage differs: definition %lx call %ld of %lu (kind %ld) reason %ld; flags game %lx rust %lx; scale game %f rust %f",
				(unsigned long)call.damage.definition_index, call.difference_event, halo_sim_area_checks.event_count, kind,
				call.difference_reason, game.flags, call.damage.flags, (double)game.scale, (double)call.damage.scale);
		}
	}
	{
		unsigned long compared = halo_sim_area_checks.compared;

		if ((compared & 0x3fff) == 0 || (compared & (compared - 1)) == 0)
		{
			platform_log("halo_sim: area damage: %lu of %lu agree with the game (%lu calls of the game: %lu rays, %lu objects damaged, %lu infection forms popped at; %lu left out)",
				compared - halo_sim_area_checks.differed, compared, halo_sim_area_checks.calls, halo_sim_area_checks.rays,
				halo_sim_area_checks.dealt, halo_sim_area_checks.infection_forms, halo_sim_area_checks.skipped);
		}
	}
}

/* ---------- damage updates */

static void halo_sim_damage_update_call_new(long object_index, struct halo_sim_damage_update_call *call)
{
	memset(call, 0, sizeof(*call));
	halo_sim_object_damage_state_load(object_index, &call->state);
	call->update_shields = network_game_distributed_client() ? 0 : 1;
	call->game_engine_running = game_engine_running() ? 1 : 0;
	call->difference_event = NONE;
}

int halo_sim_object_damage_update(long object_index)
{
	short mode = halo_sim_mode(_halo_sim_damage_update);
	struct halo_sim_damage_update_call call;
	long result;

	halo_sim_damage_update_checks.recording = FALSE;
	if (mode == _halo_sim_mode_c)
		return FALSE;
	if (mode == _halo_sim_mode_check)
	{
		halo_sim_damage_update_call_new(object_index, &halo_sim_damage_update_checks.call);
		halo_sim_damage_update_checks.object_index = object_index;
		halo_sim_damage_update_checks.event_count = 0;
		halo_sim_damage_update_checks.open_event = NULL;
		halo_sim_damage_update_checks.overflow = FALSE;
		halo_sim_damage_update_checks.recording = TRUE;
		return FALSE;
	}

	halo_sim_damage_update_call_new(object_index, &call);
	result = halo_sim.damage_update(&call);
	if (result != 0)
		return FALSE;
	if (call.still_there)
		halo_sim_object_damage_state_store(&call.state);

	return TRUE;
}

void halo_sim_damage_update_event_begin(long object_index, long kind, float f)
{
	struct halo_sim_damage_update_event *event;

	if (!halo_sim_damage_update_checks.recording || halo_sim_damage_update_checks.object_index != object_index ||
		halo_sim_damage_update_checks.open_event)
	{
		return;
	}
	if (halo_sim_damage_update_checks.event_count >= MAXIMUM_HALO_SIM_DAMAGE_UPDATE_EVENTS)
	{
		halo_sim_damage_update_checks.overflow = TRUE;
		return;
	}
	event = &halo_sim_damage_update_checks.events[halo_sim_damage_update_checks.event_count++];
	memset(event, 0, sizeof(*event));
	event->kind = kind;
	event->f = f;
	halo_sim_object_damage_state_load(object_index, &event->before);
	halo_sim_damage_update_checks.open_event = event;
}

void halo_sim_damage_update_event_end(long object_index, long result, float value)
{
	struct halo_sim_damage_update_event *event = halo_sim_damage_update_checks.open_event;

	if (!event || halo_sim_damage_update_checks.object_index != object_index)
		return;
	halo_sim_damage_update_checks.open_event = NULL;
	event->result = result;
	event->out = value;
	if (!halo_sim_object_damage_state_load(object_index, &event->after))
		halo_sim_damage_update_checks.overflow = TRUE;
}

void halo_sim_object_damage_update_check(long object_index)
{
	struct halo_sim_damage_update_call call;
	struct halo_sim_object_damage_state game;

	if (!halo_sim_damage_update_checks.recording)
		return;
	halo_sim_damage_update_checks.recording = FALSE;
	if (halo_sim_damage_update_checks.object_index != object_index || halo_sim_damage_update_checks.overflow ||
		!halo_sim_object_damage_state_load(object_index, &game))
	{
		halo_sim_damage_update_checks.skipped++;
		return;
	}
	call = halo_sim_damage_update_checks.call;
	call.events = halo_sim_damage_update_checks.events;
	call.event_count = halo_sim_damage_update_checks.event_count;
	if (halo_sim.damage_update_check(&call) != 0)
	{
		halo_sim_damage_update_checks.skipped++;
		return;
	}

	halo_sim_damage_update_checks.compared++;
	halo_sim_damage_update_checks.calls += halo_sim_damage_update_checks.event_count;
	if (halo_sim_damage_update_checks.call.state.shield_vitality != game.shield_vitality)
		halo_sim_damage_update_checks.recharging++;
	if (halo_sim_damage_update_checks.call.state.body_damage_decay_timer != NONE ||
		halo_sim_damage_update_checks.call.state.shield_damage_decay_timer != NONE)
	{
		halo_sim_damage_update_checks.decaying++;
	}
	if (memcmp(&call.state, &game, sizeof(game)) != 0 || call.difference_event != NONE)
	{
		halo_sim_damage_update_checks.differed++;
		if (halo_sim_damage_update_checks.differed <= 8)
		{
			platform_log("halo_sim: damage update differs: object %lx call %ld of %lu reason %ld; shield game %f rust %f; flags game %x rust %x; body decay game %ld rust %ld",
				(unsigned long)object_index, call.difference_event, halo_sim_damage_update_checks.event_count,
				call.difference_reason, (double)game.shield_vitality, (double)call.state.shield_vitality,
				game.damage_flags, call.state.damage_flags, game.body_damage_decay_timer,
				call.state.body_damage_decay_timer);
		}
	}
	{
		unsigned long compared = halo_sim_damage_update_checks.compared;

		if ((compared & 0x3ffff) == 0 || (compared & (compared - 1)) == 0)
		{
			platform_log("halo_sim: damage update: %lu of %lu agree with the game (%lu with the shield changing, %lu decaying; %lu calls of the game; %lu left out)",
				compared - halo_sim_damage_update_checks.differed, compared, halo_sim_damage_update_checks.recharging,
				halo_sim_damage_update_checks.decaying, halo_sim_damage_update_checks.calls,
				halo_sim_damage_update_checks.skipped);
		}
	}
}

/* ---------- device updates */

static void halo_sim_device_call_new(long device_index, int part, struct halo_sim_device_call_c *call)
{
	struct device_datum *device = device_get(device_index);

	memset(call, 0, sizeof(*call));
	halo_sim_device_state_load(device_index, &call->state);
	call->part = (unsigned long)part;
	call->structure_bsp_index = global_structure_bsp_index;
	call->breakable_surfaces = (unsigned char const *)breakable_surface_flags_get();
	if (device->device.power_group_index != NONE)
		call->power_group_value = device_group_get_value(device->device.power_group_index);
	if (device->device.position_group_index != NONE)
		call->position_group_value = device_group_get_value(device->device.position_group_index);
	call->game_time = game_time_get();
	if (part == 1)
	{
		struct machine_definition *definition = machine_definition_get(device->definition_index);

		/* (the elevator node's position: its node matrix as it stands) */
		if (TEST_FLAG(definition->machine.flags, _machine_is_elevator_bit) &&
			definition->machine.elevator_node_index != NONE)
		{
			call->has_elevator_node = 1;
			call->elevator_node_position =
				object_get_node_matrix(device_index, (unsigned short)definition->machine.elevator_node_index)->position;
		}
	}
	call->difference_event = NONE;
}

int halo_sim_device_update(long device_index, int part, unsigned char *changed)
{
	short mode = halo_sim_mode(_halo_sim_devices);
	struct halo_sim_device_call_c call;
	long result;

	halo_sim_device_checks.recording = FALSE;
	if (mode == _halo_sim_mode_c)
		return FALSE;
	if (mode == _halo_sim_mode_check)
	{
		halo_sim_device_call_new(device_index, part, &halo_sim_device_checks.call);
		halo_sim_device_checks.device_index = device_index;
		halo_sim_device_checks.event_count = 0;
		halo_sim_device_checks.biped_count = 0;
		halo_sim_device_checks.open_event = NULL;
		halo_sim_device_checks.overflow = FALSE;
		halo_sim_device_checks.recording = TRUE;
		return FALSE;
	}

	halo_sim_device_call_new(device_index, part, &call);
	result = halo_sim.device_update(&call);
	if (result != 0)
		return FALSE;
	halo_sim_device_state_store(&call.state);
	*changed = call.changed ? TRUE : FALSE;

	return TRUE;
}

void halo_sim_device_event_begin(long device_index, long kind, long object_index, long a, long b, float const *f)
{
	struct halo_sim_device_event *event;

	if (!halo_sim_device_checks.recording || halo_sim_device_checks.device_index != device_index ||
		halo_sim_device_checks.open_event)
	{
		return;
	}
	if (halo_sim_device_checks.event_count >= MAXIMUM_HALO_SIM_DEVICE_EVENTS)
	{
		halo_sim_device_checks.overflow = TRUE;
		return;
	}
	event = &halo_sim_device_checks.events[halo_sim_device_checks.event_count++];
	memset(event, 0, sizeof(*event));
	event->call.kind = kind;
	event->call.object_index = object_index;
	event->call.a = a;
	event->call.b = b;
	memcpy(event->call.f, f, sizeof(event->call.f));
	halo_sim_device_state_load(device_index, &event->before);
	halo_sim_device_checks.open_event = event;
}

void halo_sim_device_event_end(long device_index, long result)
{
	struct halo_sim_device_event *event = halo_sim_device_checks.open_event;

	if (!event || halo_sim_device_checks.device_index != device_index)
		return;
	halo_sim_device_checks.open_event = NULL;
	event->result = result;
	if (!halo_sim_device_state_load(device_index, &event->after))
		halo_sim_device_checks.overflow = TRUE;
}

void halo_sim_device_bipeds_in_sphere(long machine_index, short cluster, void const *center, float radius,
	short maximum_count, long const *object_indices, short object_count)
{
	struct halo_sim_device_event *event;
	short object_number;

	if (!halo_sim_device_checks.recording || halo_sim_device_checks.device_index != machine_index ||
		halo_sim_device_checks.open_event)
	{
		return;
	}
	if (halo_sim_device_checks.event_count >= MAXIMUM_HALO_SIM_DEVICE_EVENTS ||
		halo_sim_device_checks.biped_count + (unsigned long)object_count > MAXIMUM_HALO_SIM_DEVICE_BIPEDS)
	{
		halo_sim_device_checks.overflow = TRUE;
		return;
	}
	event = &halo_sim_device_checks.events[halo_sim_device_checks.event_count++];
	memset(event, 0, sizeof(*event));
	event->call.kind = 6; /* (crates/halo_sim's BIPEDS_IN_SPHERE) */
	event->call.object_index = NONE;
	event->call.a = maximum_count;
	event->call.b = cluster;
	memcpy(event->call.f, center, sizeof(event->call.f));
	event->radius = radius;
	event->biped_first = halo_sim_device_checks.biped_count;
	for (object_number = 0; object_number < object_count; object_number++)
	{
		halo_sim_device_biped_load(object_indices[object_number],
			&halo_sim_device_checks.bipeds[halo_sim_device_checks.biped_count++]);
	}
	event->biped_count = (unsigned long)object_count;
	halo_sim_device_checks.spheres++;
}

void halo_sim_device_update_check(long device_index, int part, unsigned char changed)
{
	struct halo_sim_device_call_c call;
	struct halo_sim_device_state game;

	if (!halo_sim_device_checks.recording)
		return;
	halo_sim_device_checks.recording = FALSE;
	if (halo_sim_device_checks.device_index != device_index || halo_sim_device_checks.overflow ||
		!halo_sim_device_state_load(device_index, &game))
	{
		halo_sim_device_checks.skipped++;
		return;
	}
	call = halo_sim_device_checks.call;
	call.events = halo_sim_device_checks.events;
	call.event_count = halo_sim_device_checks.event_count;
	call.bipeds = halo_sim_device_checks.bipeds;
	call.biped_count = halo_sim_device_checks.biped_count;
	if (halo_sim.device_update_check(&call) != 0)
	{
		halo_sim_device_checks.skipped++;
		return;
	}

	halo_sim_device_checks.compared[part ? 1 : 0]++;
	halo_sim_device_checks.calls += halo_sim_device_checks.event_count;
	{
		unsigned long event_number;

		for (event_number = 0; event_number < halo_sim_device_checks.event_count; event_number++)
		{
			struct halo_sim_device_call const *event = &halo_sim_device_checks.events[event_number].call;

			if (event->kind == _device_call_effect && event->a != NONE)
				halo_sim_device_checks.effects++;
			else if (event->kind == _device_call_set_group_desired)
				halo_sim_device_checks.desired++;
			else if (event->kind == _device_call_translate && event->object_index != device_index)
				halo_sim_device_checks.carried++;
		}
	}
	if (halo_sim_device_checks.call.state.position != game.position || halo_sim_device_checks.call.state.power != game.power)
		halo_sim_device_checks.moving++;
	if (memcmp(&call.state, &game, sizeof(game)) != 0 || call.difference_event != NONE ||
		(part == 0 && (call.changed != 0) != (changed != 0)))
	{
		halo_sim_device_checks.differed++;
		if (halo_sim_device_checks.differed <= 8)
		{
			platform_log("halo_sim: device update differs: device %lx part %d call %ld of %lu reason %ld; position game %f rust %f; power game %f rust %f; flags game %lx rust %lx; changed game %d rust %lu",
				(unsigned long)device_index, part, call.difference_event, halo_sim_device_checks.event_count,
				call.difference_reason, (double)game.position, (double)call.state.position, (double)game.power,
				(double)call.state.power, game.device_flags, call.state.device_flags, changed, call.changed);
		}
	}
	{
		unsigned long compared = halo_sim_device_checks.compared[0] + halo_sim_device_checks.compared[1];

		if ((compared & 0x3ffff) == 0 || (compared & (compared - 1)) == 0)
		{
			platform_log("halo_sim: device update: %lu of %lu agree with the game (%lu machines; %lu moving; %lu calls of the game: %lu effects, %lu doors' groups set, %lu riders carried, %lu spheres of bipeds; %lu left out)",
				compared - halo_sim_device_checks.differed, compared, halo_sim_device_checks.compared[1],
				halo_sim_device_checks.moving, halo_sim_device_checks.calls, halo_sim_device_checks.effects,
				halo_sim_device_checks.desired, halo_sim_device_checks.carried, halo_sim_device_checks.spheres,
				halo_sim_device_checks.skipped);
		}
	}
}

/* ---------- item updates */

extern real global_gravity; /* physics.c */

static void halo_sim_item_call_new(long item_index, struct halo_sim_item_call_c *call)
{
	struct object_datum *object = object_get(item_index);

	memset(call, 0, sizeof(*call));
	halo_sim_item_state_load(item_index, &call->state);
	call->structure_bsp_index = global_structure_bsp_index;
	call->breakable_surfaces = (unsigned char const *)breakable_surface_flags_get();
	call->global_gravity = global_gravity;
	call->game_engine_running = game_engine_running() ? 1 : 0;
	call->game_time = game_time_get();
	call->node_matrices = (real const *)object_get_node_matrices(item_index);
	call->node_count = (unsigned long)(object->object.node_matrices.size / (long)sizeof(real_matrix4x3));
	call->difference_event = NONE;}

int halo_sim_item_update(long item_index)
{
	short mode = halo_sim_mode(_halo_sim_items);
	struct halo_sim_item_call_c call;
	long result;

	halo_sim_item_checks.recording = FALSE;
	if (mode == _halo_sim_mode_c)
		return FALSE;
	if (mode == _halo_sim_mode_check)
	{
		halo_sim_item_call_new(item_index, &halo_sim_item_checks.call);
		halo_sim_item_checks.item_index = item_index;
		halo_sim_item_checks.event_count = 0;
		halo_sim_item_checks.open_event = NULL;
		halo_sim_item_checks.overflow = FALSE;
		halo_sim_item_checks.recording = TRUE;
		return FALSE;
	}

	halo_sim_item_call_new(item_index, &call);
	result = halo_sim.item_update(&call);
	if (result != 0)
		return FALSE;
	if (call.still_there)
		halo_sim_item_state_store(&call.state);

	return TRUE;
}

void halo_sim_item_event_begin(long item_index, long kind, long a, long b, long c, float const *f)
{
	struct halo_sim_item_event *event;

	if (!halo_sim_item_checks.recording || halo_sim_item_checks.item_index != item_index ||
		halo_sim_item_checks.open_event)
	{
		return;
	}
	if (halo_sim_item_checks.event_count >= MAXIMUM_HALO_SIM_ITEM_EVENTS)
	{
		halo_sim_item_checks.overflow = TRUE;
		return;
	}
	event = &halo_sim_item_checks.events[halo_sim_item_checks.event_count++];
	memset(event, 0, sizeof(*event));
	event->call.kind = kind;
	event->call.a = a;
	event->call.b = b;
	event->call.c = c;
	memcpy(event->call.f, f, sizeof(event->call.f));
	halo_sim_item_state_load(item_index, &event->before);
	halo_sim_item_checks.open_event = event;
}

void halo_sim_item_event_end(long item_index, long result)
{
	struct halo_sim_item_event *event = halo_sim_item_checks.open_event;

	if (!event || halo_sim_item_checks.item_index != item_index)
		return;
	halo_sim_item_checks.open_event = NULL;
	event->result = result;
	if (result)
		halo_sim_item_state_load(item_index, &event->after);
	else
		event->after = event->before;
}

void halo_sim_item_update_check(long item_index)
{
	struct halo_sim_item_call_c call;
	struct halo_sim_item_state game;
	boolean still_there;
	unsigned long event_number;

	if (!halo_sim_item_checks.recording)
		return;
	halo_sim_item_checks.recording = FALSE;
	still_there = halo_sim_item_state_load(item_index, &game);
	if (halo_sim_item_checks.item_index != item_index || halo_sim_item_checks.overflow)
	{
		halo_sim_item_checks.skipped++;
		return;
	}
	call = halo_sim_item_checks.call;
	call.events = halo_sim_item_checks.events;
	call.event_count = halo_sim_item_checks.event_count;
	if (halo_sim.item_update_check(&call) != 0)
	{
		halo_sim_item_checks.skipped++;
		return;
	}

	halo_sim_item_checks.compared++;
	halo_sim_item_checks.calls += halo_sim_item_checks.event_count;
	for (event_number = 0; event_number < halo_sim_item_checks.event_count; event_number++)
	{
		long kind = halo_sim_item_checks.events[event_number].call.kind;

		if (kind == _item_call_impact)
			halo_sim_item_checks.impacts++;
		else if (kind == _item_call_translate)
			halo_sim_item_checks.moving++;
	}
	if (still_there && TEST_FLAG(game.object_flags, _object_at_rest_bit) &&
		!TEST_FLAG(halo_sim_item_checks.call.state.object_flags, _object_at_rest_bit))
	{
		halo_sim_item_checks.rested++;
	}
	if (TEST_FLAG(halo_sim_item_checks.call.state.item_flags, 2))
		halo_sim_item_checks.spinning++;
	/* (an item gone is compared by its calls alone) */
	if ((still_there && memcmp(&call.state, &game, sizeof(game)) != 0) || call.difference_event != NONE)
	{
		halo_sim_item_checks.differed++;
		if (halo_sim_item_checks.differed <= 8)
		{
			platform_log("halo_sim: item update differs: item %lx call %ld of %lu reason %ld; position game %f %f %f rust %f %f %f; flags game %lx/%lx rust %lx/%lx",
				(unsigned long)item_index, call.difference_event, halo_sim_item_checks.event_count, call.difference_reason,
				(double)game.position.x, (double)game.position.y, (double)game.position.z, (double)call.state.position.x,
				(double)call.state.position.y, (double)call.state.position.z, game.object_flags, game.item_flags,
				call.state.object_flags, call.state.item_flags);
		}
	}
	{
		unsigned long compared = halo_sim_item_checks.compared;

		if ((compared & 0x3ffff) == 0 || (compared & (compared - 1)) == 0)
		{
			platform_log("halo_sim: item update: %lu of %lu agree with the game (%lu moving, %lu impacts, %lu came to rest, %lu spinning; %lu calls of the game; %lu left out)",
				compared - halo_sim_item_checks.differed, compared, halo_sim_item_checks.moving, halo_sim_item_checks.impacts,
				halo_sim_item_checks.rested, halo_sim_item_checks.spinning, halo_sim_item_checks.calls,
				halo_sim_item_checks.skipped);
		}
	}
}

/* ---------- weapon updates */

int halo_sim_weapon_update(long weapon_index)
{
	short mode = halo_sim_mode(_halo_sim_weapons);
	struct halo_sim_weapon_call call;
	long result;

	halo_sim_weapon_checks.recording = FALSE;
	if (mode == _halo_sim_mode_c)
		return FALSE;
	if (mode == _halo_sim_mode_check)
	{
		memset(&halo_sim_weapon_checks.call, 0, sizeof(halo_sim_weapon_checks.call));
		halo_sim_weapon_state_load(weapon_index, &halo_sim_weapon_checks.call.state);
		halo_sim_weapon_checks.call.random_seed = get_random_seed();
		halo_sim_weapon_checks.weapon_index = weapon_index;
		halo_sim_weapon_checks.event_count = 0;
		halo_sim_weapon_checks.open_event = NULL;
		halo_sim_weapon_checks.overflow = FALSE;
		halo_sim_weapon_checks.recording = TRUE;
		return FALSE;
	}

	memset(&call, 0, sizeof(call));
	halo_sim_weapon_state_load(weapon_index, &call.state);
	result = halo_sim.weapon_update(&call);
	if (result != 0)
		return FALSE;
	if (call.still_there)
		halo_sim_weapon_state_store(&call.state);

	return TRUE;
}

void halo_sim_weapon_event_begin(long weapon_index, long kind, long a, long b)
{
	struct halo_sim_weapon_event *event;

	if (!halo_sim_weapon_checks.recording || halo_sim_weapon_checks.weapon_index != weapon_index ||
		halo_sim_weapon_checks.open_event)
	{
		return;
	}
	if (halo_sim_weapon_checks.event_count >= MAXIMUM_HALO_SIM_WEAPON_EVENTS)
	{
		halo_sim_weapon_checks.overflow = TRUE;
		return;
	}
	event = &halo_sim_weapon_checks.events[halo_sim_weapon_checks.event_count++];
	memset(event, 0, sizeof(*event));
	event->kind = kind;
	event->a = a;
	event->b = b;
	halo_sim_weapon_state_load(weapon_index, &event->before);
	halo_sim_weapon_checks.open_event = event;
}

void halo_sim_weapon_event_end(long weapon_index, long result)
{
	struct halo_sim_weapon_event *event = halo_sim_weapon_checks.open_event;

	if (!event || halo_sim_weapon_checks.weapon_index != weapon_index)
		return;
	halo_sim_weapon_checks.open_event = NULL;
	event->seed = get_random_seed();
	if (result)
		halo_sim_weapon_state_load(weapon_index, &event->after);
	else
		halo_sim_weapon_checks.overflow = TRUE;
}

void halo_sim_weapon_update_check(long weapon_index)
{
	struct halo_sim_weapon_call call;
	struct halo_sim_weapon_state game;
	unsigned long event_number;

	if (!halo_sim_weapon_checks.recording)
		return;
	halo_sim_weapon_checks.recording = FALSE;
	if (halo_sim_weapon_checks.weapon_index != weapon_index || halo_sim_weapon_checks.overflow ||
		!halo_sim_weapon_state_load(weapon_index, &game))
	{
		halo_sim_weapon_checks.skipped++;
		return;
	}
	call = halo_sim_weapon_checks.call;
	call.events = halo_sim_weapon_checks.events;
	call.event_count = halo_sim_weapon_checks.event_count;
	if (halo_sim.weapon_update_check(&call) != 0)
	{
		halo_sim_weapon_checks.skipped++;
		return;
	}

	halo_sim_weapon_checks.compared++;
	halo_sim_weapon_checks.calls += halo_sim_weapon_checks.event_count;
	for (event_number = 0; event_number < halo_sim_weapon_checks.event_count; event_number++)
	{
		long kind = halo_sim_weapon_checks.events[event_number].kind;

		if (kind == _weapon_update_call_trigger_begin_firing)
			halo_sim_weapon_checks.firing++;
		else if (kind == _weapon_update_call_magazine_start_reload || kind == _weapon_update_call_magazine_finish_reload)
			halo_sim_weapon_checks.reloading++;
	}
	if (halo_sim_weapon_checks.call.state.heat > 0.f)
		halo_sim_weapon_checks.heating++;
	if (memcmp(&call.state, &game, sizeof(game)) != 0 || call.difference_event != NONE)
	{
		halo_sim_weapon_checks.differed++;
		if (halo_sim_weapon_checks.differed <= 8)
		{
			long kind = call.difference_event >= 0 && call.difference_event < (long)halo_sim_weapon_checks.event_count ?
				halo_sim_weapon_checks.events[call.difference_event].kind : NONE;

			platform_log("halo_sim: weapon update differs: weapon %lx call %ld of %lu (kind %ld) reason %ld; heat game %f rust %f; flags game %lx rust %lx; rate of fire game %f rust %f",
				(unsigned long)weapon_index, call.difference_event, halo_sim_weapon_checks.event_count, kind,
				call.difference_reason, (double)game.heat, (double)call.state.heat, game.flags, call.state.flags,
				(double)game.triggers[0].rate_of_fire, (double)call.state.triggers[0].rate_of_fire);
		}
	}
	{
		unsigned long compared = halo_sim_weapon_checks.compared;

		if ((compared & 0x3ffff) == 0 || (compared & (compared - 1)) == 0)
		{
			platform_log("halo_sim: weapon update: %lu of %lu agree with the game (%lu triggers fired, %lu reloads, %lu with heat; %lu calls of the game; %lu left out)",
				compared - halo_sim_weapon_checks.differed, compared, halo_sim_weapon_checks.firing,
				halo_sim_weapon_checks.reloading, halo_sim_weapon_checks.heating, halo_sim_weapon_checks.calls,
				halo_sim_weapon_checks.skipped);
		}
	}
}

/* ---------- the object update loop */

int halo_sim_objects_update(int dont_update_object)
{
	short mode = halo_sim_mode(_halo_sim_objects);
	struct halo_sim_objects_update_call call;

	halo_sim_objects_checks.recording = FALSE;
	if (mode == _halo_sim_mode_c)
		return FALSE;
	if (mode == _halo_sim_mode_check)
	{
		halo_sim_objects_checks.event_count = 0;
		halo_sim_objects_checks.overflow = FALSE;
		halo_sim_objects_checks.recording = TRUE;
		return FALSE;
	}

	memset(&call, 0, sizeof(call));
	call.dont_update_object = dont_update_object ? 1 : 0;
	return halo_sim.objects_update(&call) == 0;
}

void halo_sim_objects_event(struct halo_sim_objects_call const *call)
{
	if (!halo_sim_objects_checks.recording)
		return;
	if (halo_sim_objects_checks.event_count >= MAXIMUM_HALO_SIM_OBJECTS_EVENTS)
	{
		halo_sim_objects_checks.overflow = TRUE;
		return;
	}
	halo_sim_objects_checks.events[halo_sim_objects_checks.event_count++] = *call;
	if (call->kind == _objects_call_type_update)
		halo_sim_objects_checks.updates++;
}

void halo_sim_objects_update_check(int dont_update_object)
{
	struct halo_sim_objects_update_call call;

	if (!halo_sim_objects_checks.recording)
		return;
	halo_sim_objects_checks.recording = FALSE;
	if (halo_sim_objects_checks.overflow)
	{
		halo_sim_objects_checks.skipped++;
		return;
	}
	memset(&call, 0, sizeof(call));
	call.dont_update_object = dont_update_object ? 1 : 0;
	call.events = halo_sim_objects_checks.events;
	call.event_count = halo_sim_objects_checks.event_count;
	if (halo_sim.objects_update_check(&call) != 0)
	{
		halo_sim_objects_checks.skipped++;
		return;
	}

	halo_sim_objects_checks.compared++;
	halo_sim_objects_checks.calls += halo_sim_objects_checks.event_count;
	if (call.difference_event != NONE)
	{
		halo_sim_objects_checks.differed++;
		if (halo_sim_objects_checks.differed <= 8)
		{
			struct halo_sim_objects_call const *event = call.difference_event >= 0 &&
				call.difference_event < (long)halo_sim_objects_checks.event_count ?
					&halo_sim_objects_checks.events[call.difference_event] : NULL;

			platform_log("halo_sim: object update loop differs: call %ld of %lu (kind %ld object %lx a %ld) reason %ld",
				call.difference_event, halo_sim_objects_checks.event_count, event ? event->kind : NONE,
				event ? (unsigned long)event->object_index : 0, event ? event->a : 0, call.difference_reason);
		}
	}
	{
		unsigned long compared = halo_sim_objects_checks.compared;

		if ((compared & 0x3fff) == 0 || (compared & (compared - 1)) == 0)
		{
			platform_log("halo_sim: object update loop: %lu of %lu ticks agree with the game (%lu object updates, %lu calls of the game; garbage %lu of %lu, scenery %lu of %lu; %lu left out)",
				compared - halo_sim_objects_checks.differed, compared, halo_sim_objects_checks.updates,
				halo_sim_objects_checks.calls, halo_sim_objects_checks.garbage_compared[0],
				halo_sim_objects_checks.garbage_compared[1], halo_sim_objects_checks.scenery_compared[0],
				halo_sim_objects_checks.scenery_compared[1], halo_sim_objects_checks.skipped);
		}
	}
}

int halo_sim_garbage_lifetime(short *lifetime_ticks, unsigned char *active)
{
	short mode = halo_sim_mode(_halo_sim_objects);

	halo_sim_objects_checks.garbage_recording = FALSE;
	if (mode == _halo_sim_mode_c)
		return FALSE;
	if (mode == _halo_sim_mode_check)
	{
		halo_sim_objects_checks.garbage_lifetime = *lifetime_ticks;
		halo_sim_objects_checks.garbage_recording = TRUE;
		return FALSE;
	}
	*active = halo_sim.garbage_update(lifetime_ticks) ? TRUE : FALSE;
	return TRUE;
}

void halo_sim_garbage_lifetime_check(short lifetime_ticks, unsigned char active)
{
	short rust_lifetime = halo_sim_objects_checks.garbage_lifetime;
	long rust_active;

	if (!halo_sim_objects_checks.garbage_recording)
		return;
	halo_sim_objects_checks.garbage_recording = FALSE;
	rust_active = halo_sim.garbage_update(&rust_lifetime);
	halo_sim_objects_checks.garbage_compared[1]++;
	if (rust_lifetime == lifetime_ticks && (rust_active != 0) == (active != 0))
		halo_sim_objects_checks.garbage_compared[0]++;
	else
		platform_log("halo_sim: garbage update differs: lifetime game %d rust %d", lifetime_ticks, rust_lifetime);
}

static void halo_sim_scenery_call_load(long scenery_index, struct halo_sim_scenery_call *call)
{
	struct scenery_datum *scenery = scenery_get(scenery_index);

	memset(call, 0, sizeof(*call));
	call->animation_graph_index = (unsigned long)scenery->object.object.animation.animation_graph_index;
	call->flags = scenery->flags;
	call->random_seed = get_random_seed();
	call->animation_index = scenery->object.object.animation.state.index;
	call->animation_frame_index = scenery->object.object.animation.state.frame_index;
}

int halo_sim_scenery_animation(long scenery_index)
{
	short mode = halo_sim_mode(_halo_sim_objects);
	struct halo_sim_scenery_call call;
	struct scenery_datum *scenery;

	halo_sim_objects_checks.scenery_recording = FALSE;
	if (mode == _halo_sim_mode_c)
		return FALSE;
	if (mode == _halo_sim_mode_check)
	{
		halo_sim_scenery_call_load(scenery_index, &halo_sim_objects_checks.scenery_before);
		halo_sim_objects_checks.scenery_index = scenery_index;
		halo_sim_objects_checks.scenery_recording = TRUE;
		return FALSE;
	}
	halo_sim_scenery_call_load(scenery_index, &call);
	if (halo_sim.scenery_update(&call) != 0)
		return FALSE;
	scenery = scenery_get(scenery_index);
	scenery->object.object.animation.state.index = call.animation_index;
	scenery->object.object.animation.state.frame_index = call.animation_frame_index;
	*get_global_random_seed_address() = call.random_seed;
	return TRUE;
}

void halo_sim_scenery_animation_check(long scenery_index)
{
	struct halo_sim_scenery_call rust;
	struct halo_sim_scenery_call game;

	if (!halo_sim_objects_checks.scenery_recording || halo_sim_objects_checks.scenery_index != scenery_index)
		return;
	halo_sim_objects_checks.scenery_recording = FALSE;
	rust = halo_sim_objects_checks.scenery_before;
	if (halo_sim.scenery_update(&rust) != 0)
		return;
	halo_sim_scenery_call_load(scenery_index, &game);
	halo_sim_objects_checks.scenery_compared[1]++;
	if (memcmp(&rust, &game, sizeof(game)) == 0)
		halo_sim_objects_checks.scenery_compared[0]++;
	else
		platform_log("halo_sim: scenery update differs: scenery %lx animation game %d/%d rust %d/%d; seed game %lx rust %lx",
			(unsigned long)scenery_index, game.animation_index, game.animation_frame_index, rust.animation_index,
			rust.animation_frame_index, game.random_seed, rust.random_seed);
}

/* ---------- function values and change colours */

unsigned char const *transition_function_table_get(short function_type); /* periodic_functions.c */

static void halo_sim_object_functions_load(long object_index, struct halo_sim_object_functions_state *state)
{
	struct object_datum *object = object_get(object_index);

	memset(state, 0, sizeof(*state));
	state->object_index = (unsigned long)object_index;
	state->definition_index = (unsigned long)object->definition_index;
	state->functions_active_flags = object->object.functions_active_flags;
	memcpy(state->incoming_function_values, object->object.incoming_function_values, sizeof(state->incoming_function_values));
	memcpy(state->outgoing_function_values, object->object.outgoing_function_values, sizeof(state->outgoing_function_values));
	memcpy(state->outgoing_change_colors, object->object.outgoing_change_colors, sizeof(state->outgoing_change_colors));
}

static void halo_sim_object_functions_call_new(long object_index, int part, struct halo_sim_object_functions_call *call)
{
	short function_type;

	memset(call, 0, sizeof(*call));
	halo_sim_object_functions_load(object_index, &call->state);
	call->part = (unsigned long)part;
	call->game_time = game_time_get();
	for (function_type = 0; function_type < 12; function_type++)
		call->periodic_tables[function_type] = periodic_function_table_get(function_type);
	for (function_type = 0; function_type < 6; function_type++)
		call->transition_tables[function_type] = transition_function_table_get(function_type);
}

int halo_sim_object_functions(long object_index, int part)
{
	short mode = halo_sim_mode(_halo_sim_object_functions);
	struct halo_sim_object_functions_call call;
	struct object_datum *object;

	halo_sim_object_functions_checks.recording = FALSE;
	if (mode == _halo_sim_mode_c)
		return FALSE;
	if (mode == _halo_sim_mode_check)
	{
		halo_sim_object_functions_call_new(object_index, part, &halo_sim_object_functions_checks.call);
		halo_sim_object_functions_checks.object_index = object_index;
		halo_sim_object_functions_checks.recording = TRUE;
		return FALSE;
	}

	halo_sim_object_functions_call_new(object_index, part, &call);
	if (halo_sim.object_functions(&call) != 0)
		return FALSE;
	object = object_get(object_index);
	object->object.functions_active_flags = call.state.functions_active_flags;
	memcpy(object->object.outgoing_function_values, call.state.outgoing_function_values, sizeof(call.state.outgoing_function_values));
	memcpy(object->object.outgoing_change_colors, call.state.outgoing_change_colors, sizeof(call.state.outgoing_change_colors));

	return TRUE;
}

void halo_sim_object_functions_check(long object_index, int part)
{
	struct halo_sim_object_functions_call call;
	struct halo_sim_object_functions_state game;

	if (!halo_sim_object_functions_checks.recording || halo_sim_object_functions_checks.object_index != object_index)
		return;
	halo_sim_object_functions_checks.recording = FALSE;
	call = halo_sim_object_functions_checks.call;
	if (halo_sim.object_functions(&call) != 0)
		return;
	halo_sim_object_functions_load(object_index, &game);
	part = part ? 1 : 0;
	halo_sim_object_functions_checks.compared[part]++;
	if (memcmp(&halo_sim_object_functions_checks.call.state, &game, sizeof(game)) != 0)
		halo_sim_object_functions_checks.active[part]++;
	if (memcmp(&call.state, &game, sizeof(game)) != 0)
	{
		halo_sim_object_functions_checks.differed[part]++;
		if (halo_sim_object_functions_checks.differed[0] + halo_sim_object_functions_checks.differed[1] <= 8)
		{
			platform_log("halo_sim: %s differ: object %lx definition %lx; outgoing game %f %f %f %f rust %f %f %f %f; active game %x rust %x; colour a game %f %f %f rust %f %f %f",
				part ? "change colours" : "function values", (unsigned long)object_index, game.definition_index,
				(double)game.outgoing_function_values[0], (double)game.outgoing_function_values[1],
				(double)game.outgoing_function_values[2], (double)game.outgoing_function_values[3],
				(double)call.state.outgoing_function_values[0], (double)call.state.outgoing_function_values[1],
				(double)call.state.outgoing_function_values[2], (double)call.state.outgoing_function_values[3],
				game.functions_active_flags, call.state.functions_active_flags,
				(double)game.outgoing_change_colors[0].red, (double)game.outgoing_change_colors[0].green,
				(double)game.outgoing_change_colors[0].blue, (double)call.state.outgoing_change_colors[0].red,
				(double)call.state.outgoing_change_colors[0].green, (double)call.state.outgoing_change_colors[0].blue);
		}
	}
	{
		unsigned long compared = halo_sim_object_functions_checks.compared[0] + halo_sim_object_functions_checks.compared[1];

		if ((compared & 0x3ffff) == 0 || (compared & (compared - 1)) == 0)
		{
			platform_log("halo_sim: object functions: %lu of %lu function values and %lu of %lu change colours agree with the game (%lu and %lu changing)",
				halo_sim_object_functions_checks.compared[0] - halo_sim_object_functions_checks.differed[0],
				halo_sim_object_functions_checks.compared[0],
				halo_sim_object_functions_checks.compared[1] - halo_sim_object_functions_checks.differed[1],
				halo_sim_object_functions_checks.compared[1], halo_sim_object_functions_checks.active[0],
				halo_sim_object_functions_checks.active[1]);
		}
	}
}
