/*
HALO_SIM.H

The game's side of halo_sim (bevy-rt-test's crates/halo_sim): Rust ports of
the game's simulation, loaded as a DLL, that run in place of the game's own
C one system at a time (the object simulation handing off to Rust).
*/

#ifndef __HALO_SIM_H
#define __HALO_SIM_H
#pragma once

/* ---------- constants */

enum
{
	_halo_sim_point_physics = 0,
	_halo_sim_poses,
	_halo_sim_collision,
	_halo_sim_projectiles,
	_halo_sim_biped_physics,
	_halo_sim_vehicle_physics,
	_halo_sim_vehicle_update,
	_halo_sim_biped_moving,
	_halo_sim_biped_update,
	_halo_sim_unit_animation,
	_halo_sim_player_melee,
	_halo_sim_unit_update,
	_halo_sim_biped_rest,
	_halo_sim_damage,
	_halo_sim_area_damage,
	_halo_sim_damage_update,
	_halo_sim_devices,
	_halo_sim_items,
	_halo_sim_weapons,
	_halo_sim_objects,
	_halo_sim_object_functions,
	_halo_sim_object_exports,
	_halo_sim_unit_ik,
	_halo_sim_lights,
	_halo_sim_effects,
	_halo_sim_particles,
	_halo_sim_contrails,
	_halo_sim_particle_systems,
	_halo_sim_player_effects,
	_halo_sim_game_tick,
	_halo_sim_hs,
	_halo_sim_ai_update,
	_halo_sim_actor_emotions,
	_halo_sim_actor_situation,
	_halo_sim_actor_upkeep,
	_halo_sim_actor_input,
	_halo_sim_actor_general,
	_halo_sim_actor_look_movement,
	_halo_sim_actor_communication,
	_halo_sim_actor_conversation,
	_halo_sim_actor_destination,
	_halo_sim_actor_unit_control,
	_halo_sim_actor_perception,
	_halo_sim_actor_look,
	_halo_sim_actor_move,
	_halo_sim_actor_combat,
	_halo_sim_actor_dispatch,
	_halo_sim_paths,
	_halo_sim_encounters,
	_halo_sim_conversations,
	_halo_sim_communication,
	/* the packet codec (memory/data_packet_groups.c), the game's messages' */
	_halo_sim_data_packets,
	/* the distributed netcode's wire format (network_distributed.c) */
	_halo_sim_distributed_wire,
	/* a client's applying of the host's state (network_objects.c) */
	_halo_sim_network_apply,
	/* the host's checking of clients' hit reports (network_damage.c) */
	_halo_sim_network_damage,
	/* the host's sending of its objects each tick (network_objects.c) */
	_halo_sim_network_host,
	/* the netcode's messages handled (network_distributed.c) */
	_halo_sim_network_messages,
	/* the netcode's input handled (network_distributed.c) */
	_halo_sim_network_input,
	/* the host's check of its clients' clocks (network_distributed.c) */
	_halo_sim_network_clock,
	/* network co-op (network_coop.c) */
	_halo_sim_network_coop,
	NUMBER_OF_HALO_SIM_SYSTEMS
};

/* how a system runs: the game's C, the Rust port, or both (the C's result
kept, the Rust's compared with it and differences counted and logged) */
enum
{
	_halo_sim_mode_c = 0,
	_halo_sim_mode_rust,
	_halo_sim_mode_check,
};

/* a projectile update's calls of the rest of the game, recorded for the
check (crates/halo_sim's projectiles::event) */
enum
{
	_halo_sim_projectile_event_guidance = 1,
	_halo_sim_projectile_event_collision,
	_halo_sim_projectile_event_impact_damage,
	_halo_sim_projectile_event_breakable_surface,
	_halo_sim_projectile_event_effect,
	_halo_sim_projectile_event_detonation_started,
	_halo_sim_projectile_event_combining_reset,
	_halo_sim_projectile_event_attach,
	_halo_sim_projectile_event_impact_noise,
	_halo_sim_projectile_event_moved,
};

/* a unit update's calls of the rest of the game (units.c's
unit_update_call; crates/halo_map's unit_update::call) */
enum
{
	_unit_update_call_running_blind_vector = 1,
	_unit_update_call_drop_current_weapon,
	_unit_update_call_resurrect,
	_unit_update_call_died,
	_unit_update_call_ready_desired_weapon,
	_unit_update_call_next_grenade,
	_unit_update_call_zoom_changed,
	_unit_update_call_grenade_throw_begin,
	_unit_update_call_grenade_move_to_hand,
	_unit_update_call_grenade_release,
	_unit_update_call_weapon_light_power,
	_unit_update_call_weapon_owner_update,
	_unit_update_call_seat_update,
	_unit_update_call_delayed_damage,
	_unit_update_call_continuous_melee_damage,
	_unit_update_call_dialogue_update,
	_unit_update_call_refresh_illumination,
	_unit_update_call_flame_to_death,
	_unit_update_call_night_vision_active,
	_unit_update_call_night_vision_effect,
	_unit_update_call_light_effect,
};

/* damage's calls of the rest of the game (damage.c's damage_call;
crates/halo_map's damage::call) */
enum
{
	_damage_call_network_deals = 1,
	_damage_call_ai_adjust,
	_damage_call_gametype_multiplier,
	_damage_call_team_is_enemy,
	_damage_call_difficulty_enemy_damage,
	_damage_call_difficulty_team_value,
	_damage_call_unsuspecting,
	_damage_call_friendly_damage,
	_damage_call_transition,
	_damage_call_player_effect,
	_damage_call_deplete_body,
	_damage_call_deplete_shield,
	_damage_call_destroy,
	_damage_call_destroy_region,
	_damage_call_shield_damaged_effect,
	_damage_call_body_damaged_effect,
	_damage_call_localized_effect,
	_damage_call_area_effect,
	_damage_call_aftermath,
	_damage_call_player_controlled,
};

/* area damage's calls of the rest of the game (damage.c's
area_damage_call; crates/halo_map's area_damage::call) */
enum
{
	_area_damage_call_ray_test = 1,
	_area_damage_call_team_is_enemy,
	_area_damage_call_difficulty_infection_forms,
	_area_damage_call_cause_damage,
	_area_damage_call_breakable_surfaces,
};

/* the damage update's calls of the rest of the game (damage.c's
damage_update_call; crates/halo_map's damage_update::call) */
enum
{
	_damage_update_call_act_of_god = 1,
	_damage_update_call_hud_tick_shield,
	_damage_update_call_difficulty_enemy_recharge,
	_damage_update_call_shield_recharging,
};

/* a device update's calls of the rest of the game (devices.c's
device_call; crates/halo_map's devices::call) */
enum
{
	_device_call_effect = 1,
	_device_call_set_group_actual,
	_device_call_team_is_enemy,
	_device_call_set_group_desired,
	_device_call_translate,
};

/* an item update's calls of the rest of the game (items.c's item_call;
crates/halo_map's items::call) */
enum
{
	_item_call_impact = 1,
	_item_call_set_garbage,
	_item_call_translate,
	_item_call_accelerate,
	_item_call_detonate,
};

/* a weapon's calls of the rest of the game (weapons.c's weapon_game_call;
crates/halo_map's weapons::call) */
enum
{
	_weapon_call_detonate_item = 1,
	_weapon_call_message,
	_weapon_call_looping_effect,
	_weapon_call_effect_stop,
	_weapon_call_permute_blurred,
	_weapon_call_effect,
	_weapon_call_underwater,
	_weapon_call_animation_time,
	_weapon_call_weapon_state_change,
	_weapon_call_weapon_fired,
	_weapon_call_ai_shooting,
	_weapon_call_self_damage,
	_weapon_call_delete,
	_weapon_call_firing_blindly,
	_weapon_call_projectile_ray,
	_weapon_call_player_aim,
	_weapon_call_actor_aim,
	_weapon_call_new_projectile,
};

/* crates/halo_map's WeaponCall: a weapon's call of the game, and its answer */
struct halo_sim_weapon_game_call
{
	long kind;
	long a;
	long b;
	long c;
	long d;
	float f[12];
	long result;
	float out[8];
};

/* an effect's calls of the rest of the game (effects.c's effect_call;
crates/halo_map's effects::call) */
enum
{
	_effect_call_object = 1,
	_effect_call_delete,
	_effect_call_detach_delete,
	_effect_call_function_value,
	_effect_call_change_color,
	_effect_call_visible,
	_effect_call_underwater,
	_effect_call_node_matrix,
	_effect_call_impulse,
	_effect_call_particle_system,
	_effect_call_sound,
	_effect_call_object_new,
	_effect_call_decal,
	_effect_call_damage,
	_effect_call_light,
	_effect_call_particle,
	_effect_call_allocate,
	_effect_call_impulse_field,
	_effect_call_corpse,
	_effect_call_local_index,
	_effect_call_location_from_point,
	_effect_call_markers,
	_effect_call_build_location,
	_effect_call_clear_locations,
	_effect_call_object_node_matrix,
};

/* which effect_new_* (crates/halo_map's effects::new_kind) */
enum
{
	_effect_new_looping = 0,
	_effect_new_from_object,
	_effect_new_attached_from_markers,
	_effect_new_unattached_from_markers,
};

/* crates/halo_map's EffectCall: an effect's call of the game, and its answer */
struct halo_sim_effect_call
{
	long kind;
	long a;
	long b;
	long c;
	long d;
	float f[20];
	long result;
	float out[16];
	long out_location_leaf;
	short out_location_cluster;
	unsigned short out_location_bonus;
};

/* crates/halo_map's EffectState: what an effect's update changes of the
effect, and the game's two random seeds */
struct halo_sim_effect_state
{
	unsigned long effect_index;
	unsigned long definition_index;
	unsigned short flags;
	short scale_a_function_index;
	short scale_b_function_index;
	short change_color_index;
	long location_leaf;
	short location_cluster;
	unsigned short location_bonus;
	float color[3];
	float velocity[3];
	long object_index;
	long owner_object_index;
	float scale_a;
	float scale_b;
	short local_player_index;
	short event_index;
	float event_time;
	float event_duration;
	float last_event_fraction;
	unsigned char particle_counts[32];
	unsigned long has_translational_impulse;
	unsigned long random_seed;
	unsigned long local_random_seed;
};

/* crates/halo_sim's EffectEventC */
struct halo_sim_effect_event
{
	struct halo_sim_effect_call call;
	unsigned long still_there;
	struct halo_sim_effect_state before;
	struct halo_sim_effect_state after;
};

/* crates/halo_sim's EffectNewC: what an effect_new_* is given, and the
effect it leaves */
struct halo_sim_effect_new_call
{
	long kind;
	long definition_index;
	long owner_object_index;
	long object_index;
	short node_index;
	short force_local_player_index;
	short scale_a_function_index;
	short scale_b_function_index;
	short change_color_index;
	short marker_count;
	float scale_a;
	float scale_b;
	unsigned long has_color;
	float color[3];
	unsigned long has_velocity;
	float velocity[3];
	unsigned long impulse_field;
	unsigned long can_be_deterministic;
	char const **marker_names;
	float const *marker_points;
	float const *marker_forwards;
	unsigned long game_in_editor;
	unsigned long corpse_nonviolent;
	long local_player_count;
	void const *directions;
	unsigned long direction_count;
	struct halo_sim_effect_state state;
	struct halo_sim_effect_event const *events;
	unsigned long event_count;
	long difference_event;
	long difference_reason;
	unsigned long still_there;
	long result;
};

/* crates/halo_map's EffectInstance: one of an effect's location instances,
`next` the next one's index among them */
struct halo_sim_effect_instance
{
	short node_designator;
	short pad;
	long next;
	float matrix[13];
};

/* a particle's calls of the rest of the game (particles.c's particle_call;
crates/halo_map's particles::call) */
enum
{
	_particle_call_delete = 1,
	_particle_call_effect,
	_particle_call_object_exists,
	_particle_call_physics,
	_particle_call_material_effect,
	_particle_call_node_point,
	_particle_call_location_from_point,
	_particle_call_visible,
	_particle_call_new,
	_particle_call_light,
};

/* crates/halo_map's ParticleCall: a particle's call of the game, and its
answer */
struct halo_sim_particle_call
{
	long kind;
	long a;
	long b;
	long c;
	long d;
	float f[4];
	long result;
	float out[6];
	long out_material_type;
	long out_location_leaf;
	short out_location_cluster;
	unsigned short out_location_bonus;
};

/* crates/halo_map's ParticleState: what a particle's update changes of the
particle, and the game's local random seed */
struct halo_sim_particle_state
{
	unsigned long particle_index;
	unsigned long definition_index;
	unsigned short flags;
	unsigned char state;
	unsigned char local_player_index;
	long object_index;
	short node_index;
	short sequence_index;
	short frame_index;
	short pad;
	long last_rendered_frame_index;
	float age;
	float lifespan;
	float frame_time;
	float frame_span;
	long location_leaf;
	short location_cluster;
	unsigned short location_bonus;
	float position[3];
	float direction[3];
	float translational_velocity[3];
	float rotation;
	float angular_velocity;
	float radius;
	float color[4];
	unsigned long local_random_seed;
};

/* a contrail's calls of the rest of the game (contrails.c's contrail_call;
crates/halo_map's contrails::call) */
enum
{
	_contrail_call_function_value = 1,
	_contrail_call_markers,
	_contrail_call_velocity,
	_contrail_call_point_new,
	_contrail_call_point_delete,
	_contrail_call_location_from_point,
	_contrail_call_location_from_line,
	_contrail_call_physics,
	_contrail_call_delete,
	_contrail_call_new,
};

/* crates/halo_map's ContrailCall: a contrail's call of the game, and its
answer */
struct halo_sim_contrail_call
{
	long kind;
	long a;
	long b;
	long c;
	long d;
	float f[8];
	long result;
	float out[6];
	long out_location_leaf;
	short out_location_cluster;
	unsigned short out_location_bonus;
};

/* crates/halo_map's ContrailState and ContrailPoint: what a contrail's update
changes of the contrail and its points, and the game's local random seed */
struct halo_sim_contrail_state
{
	unsigned long contrail_index;
	unsigned long definition_index;
	unsigned short flags;
	short attachment_index;
	long object_index;
	short density_function_index;
	short sequence_index;
	short frame_index;
	short pad;
	float density;
	float texture_offset_u;
	float texture_offset_v;
	float time_until_point;
	float frame_time;
	float expired_dt;
	short point_counts[4];
	long first_points[4];
	unsigned long local_random_seed;
};

struct halo_sim_contrail_point
{
	long index;
	long next;
	unsigned char flags;
	signed char state_index;
	unsigned short pad;
	float time;
	float delta;
	float density;
	float width;
	long location_leaf;
	short location_cluster;
	unsigned short location_bonus;
	float position[3];
	float velocity[3];
};

/* a particle system's calls of the rest of the game (particle_systems.c's
particle_system_call; crates/halo_map's particle_systems::call) */
enum
{
	_particle_system_call_object = 1,
	_particle_system_call_system_physics,
	_particle_system_call_markers,
	_particle_system_call_object_location,
	_particle_system_call_particle_new,
	_particle_system_call_location_from_point,
	_particle_system_call_particle_delete,
	_particle_system_call_particle_physics,
	_particle_system_call_delete,
	_particle_system_call_new,
	_particle_system_call_datum_delete,
	_particle_system_call_light,
	_particle_system_call_attachment,
	_particle_system_call_function_value,
};

/* crates/halo_map's ParticleSystemCall: a particle system's call of the
game, and its answer */
struct halo_sim_particle_system_call
{
	long kind;
	long a;
	long b;
	long c;
	long d;
	float f[9];
	long result;
	float out[9];
	long out_location_leaf;
	short out_location_cluster;
	unsigned short out_location_bonus;
};

/* crates/halo_map's ParticleType, ParticleSystemState and SystemParticle:
what a particle system's update changes of the system and its particles,
and the game's local random seed */
struct halo_sim_particle_type
{
	short state_index;
	short transition_state_index;
	float time_left_in_state;
	float state_length;
	float variables[10];
	float fractional_particle_count;
	unsigned char states_moving_forward;
	unsigned char reserved;
	short particle_count;
	long first_particle_index;
};

struct halo_sim_particle_system_state
{
	unsigned long system_index;
	unsigned long definition_index;
	unsigned long flags;
	long object_index;
	short attachment_index;
	short function_index;
	float scale;
	long location_leaf;
	short location_cluster;
	unsigned short location_bonus;
	float position[3];
	float velocity[3];
	float color[4];
	float lighting[3];
	unsigned char grounded;
	unsigned char pad[3];
	struct halo_sim_particle_type types[4];
	unsigned long local_random_seed;
};

struct halo_sim_system_particle
{
	long index;
	unsigned char states_moving_forward;
	unsigned char valid;
	unsigned short pad;
	long next;
	short state_index;
	short transition_state_index;
	float time_left_in_state;
	float state_length;
	long location_leaf;
	short location_cluster;
	unsigned short location_bonus;
	float position[3];
	float velocity[3];
	float axis[3];
	float rotation;
	float sprite_index;
	float randomized_variables[7];
	float transition_randomized_variables[7];
};

/* a player effect's calls of the rest of the game (player_effects.c's
player_effect_call; crates/halo_map's player_effects::call) */
enum
{
	_player_effect_call_facing_angles = 1,
	_player_effect_call_facing_direction,
	_player_effect_call_permanent_impulse,
	_player_effect_call_rumble_impulse,
	_player_effect_call_sound,
	_player_effect_call_indicator,
	_player_effect_call_rumble_continuous,
	_player_effect_call_rumble_set_scale,
};

/* the operations (crates/halo_sim's player_effects::op) */
enum
{
	_player_effect_op_start = 1,
	_player_effect_op_screen_flash,
	_player_effect_op_telefrag,
	_player_effect_op_continuous,
	_player_effect_op_camera_matrix,
	_player_effect_op_get_screen_flash,
};

/* crates/halo_map's PlayerEffectCall */
struct halo_sim_player_effect_call
{
	long kind;
	long a;
	long b;
	float f[2];
	long result;
	float out[12];
};

/* crates/halo_map's PlayerEffectState: a local player's effect (the game's
datum), the screen fade, the scripted effect, the flags and the local seed */
struct halo_sim_player_effect_state
{
	long local_player_index;
	unsigned char datum[0xEC];
	unsigned char screen_fade[20];
	unsigned char scripted_effect[32];
	unsigned long global_flags;
	unsigned long local_random_seed;
};

/* what an operation is given, and where it leaves its answer */
struct screen_flash_definition;
struct render_screen_flash;
struct halo_sim_player_effect_args
{
	long op;
	short local_player_index;
	long definition_index;
	long owner_object_index;
	real_vector3d direction;
	float scale;
	float total_damage;
	float intensity;
	float distance;
	struct screen_flash_definition const *screen_flash;
	struct render_screen_flash *render_screen_flash;
	real_matrix4x3 *matrix;
};

/* the tick's steps and questions (game.c's game_tick_call; crates/halo_map's
game_tick::call) */
enum
{
	_game_tick_call_begin = 1,
	_game_tick_call_data_check,
	_game_tick_call_cheats_enforce,
	_game_tick_call_remove_quitting_players,
	_game_tick_call_allegiance,
	_game_tick_call_units,
	_game_tick_call_distributed_client,
	_game_tick_call_ai,
	_game_tick_call_network_actors,
	_game_tick_call_players_before,
	_game_tick_call_double_speed,
	_game_tick_call_effects,
	_game_tick_call_antennas,
	_game_tick_call_first_person,
	_game_tick_call_game_engine,
	_game_tick_call_editor,
	_game_tick_call_coop_active,
	_game_tick_call_scripts,
	_game_tick_call_recorded_animations,
	_game_tick_call_objects,
	_game_tick_call_players_after,
	_game_tick_call_hud,
	_game_tick_call_player_effects,
	_game_tick_call_end,
};

/* the script runtime's calls of the rest of the game (hs_runtime.c's
hs_runtime_call; crates/halo_map's hs_runtime::call) */
enum
{
	_hs_runtime_call_external = 1,
	_hs_runtime_call_script_disabled,
	_hs_runtime_call_game_in_progress,
	_hs_runtime_call_object_from_name,
	_hs_runtime_call_object_list_from_ai,
	_hs_runtime_call_object_list_of_object,
	_hs_runtime_call_object_list_add_reference,
	_hs_runtime_call_object_list_remove_reference,
	_hs_runtime_call_object_type,
	_hs_runtime_call_error,
	_hs_runtime_call_inspect,
	_hs_runtime_call_unused12,
	_hs_runtime_call_object_list_gc,
	_hs_runtime_call_node_gc,
	_hs_runtime_call_unused15, /* (a console command's thread deleted: the runtime's own now) */
	_hs_runtime_call_global_initializer_disabled,
};

/* the runtime's operations the Rust port can run */
enum
{
	_halo_sim_hs_update,
	_halo_sim_hs_initialize_for_new_map,
	_halo_sim_hs_evaluate,
	NUMBER_OF_HALO_SIM_HS_OPERATIONS
};

/* the runtime's messages */
enum
{
	_hs_runtime_error_stack_overflow = 1,
	_hs_runtime_error_syntax,
	_hs_runtime_error_unexpected_parameters,
	_hs_runtime_error_object_cast,
	_hs_runtime_error_out_of_script_threads,
	_hs_runtime_error_out_of_command_threads,
	_hs_runtime_error_global_initialization_slept,
};

/* crates/halo_map's HsCall */
struct halo_sim_hs_call
{
	long kind;
	long a;
	long b;
	long c;
	long result;
};

/* crates/halo_sim's HsContextC: where the script runtime's state and tables
are */
struct halo_sim_hs_context
{
	unsigned long thread_data;
	unsigned long syntax_data;
	unsigned long global_data;
	unsigned long functions;
	long function_count;
	unsigned long external_globals;
	long external_global_count;
	unsigned long scripts;
	long script_count;
	unsigned long globals;
	long global_count;
	unsigned long runtime_globals;
	unsigned long random_seed;
	long game_time;
	unsigned long string_default;
	long scenario_loaded;
};

/* the AI's update's steps and questions (ai.c's ai_update_call, actors.c's
actors_port_call; crates/halo_map's ai_update::call) */
enum
{
	_ai_update_call_state = 1,
	_ai_update_call_set_control_data,
	_ai_update_call_profile_enter,
	_ai_update_call_profile_exit,
	_ai_update_call_debug_update,
	_ai_update_call_profile_update,
	_ai_update_call_place_pending_mounted_weapons,
	_ai_update_call_move_randomly,
	_ai_update_call_conversation_update,
	_ai_update_call_encounters_update,
	_ai_update_call_freeze,
	_ai_update_call_actors_begin,
	_ai_update_call_encounter_next,
	_ai_update_call_first_encounterless,
	_ai_update_call_actor,
	_ai_update_call_actor_erase,
	_ai_update_call_actor_dormancy,
	_ai_update_call_update_begin,
	_ai_update_call_general_update,
	_ai_update_call_timeslice,
	_ai_update_call_input,
	_ai_update_call_perception,
	_ai_update_call_situation,
	_ai_update_call_emotion,
	_ai_update_call_clear_orders,
	_ai_update_call_decision_loop,
	_ai_update_call_action_update,
	_ai_update_call_swarm_control,
	_ai_update_call_clear_output,
	_ai_update_call_action_control,
	_ai_update_call_communication,
	_ai_update_call_conversation_control,
	_ai_update_call_destination,
	_ai_update_call_look_affect_movement,
	_ai_update_call_move_update,
	_ai_update_call_look_update,
	_ai_update_call_combat_update,
	_ai_update_call_unit_control,
	_ai_update_call_update_end,
};

/* _ai_update_call_state's and _actor's bits */
enum
{
	_ai_update_state_initialized = 1<<0,
	_ai_update_state_profile_disabled = 1<<1,
	_ai_update_state_move_randomly = 1<<2,
	_ai_update_state_active = 1<<3,
	_ai_update_state_has_control_data = 1<<4,
};
enum
{
	_ai_update_actor_active = 1<<0,
	_ai_update_actor_erase_next_tick = 1<<1,
	_ai_update_actor_thinking = 1<<2,
};

/* crates/halo_map's AiUpdateCall */
struct halo_sim_ai_update_call
{
	long kind;
	long a;
	long result;
	long result2;
	long result3;
	long result4;
};

/* the AI's steps of an actor the Rust port runs (crates/halo_sim's
ai_steps), and their systems */
enum
{
	_halo_sim_ai_step_emotion,
	_halo_sim_ai_step_situation,
	_halo_sim_ai_step_timeslice,
	_halo_sim_ai_step_clear_orders,
	_halo_sim_ai_step_clear_output,
	_halo_sim_ai_step_input,
	_halo_sim_ai_step_general,
	_halo_sim_ai_step_look_movement,
	_halo_sim_ai_step_communication,
	_halo_sim_ai_step_conversation,
	_halo_sim_ai_step_destination,
	_halo_sim_ai_step_unit_control,
	_halo_sim_ai_step_perception,
	_halo_sim_ai_step_look,
	_halo_sim_ai_step_move,
	_halo_sim_ai_step_combat,
	_halo_sim_ai_step_decision_loop,
	_halo_sim_ai_step_action_update,
	_halo_sim_ai_step_action_control,
	_halo_sim_ai_step_swarm_control,
	NUMBER_OF_HALO_SIM_AI_STEPS
};

#define HALO_SIM_ACTOR_DATUM_SIZE 0x724
#define HALO_SIM_PROP_DATUM_SIZE 0x138
#define HALO_SIM_DATA_ARRAY_SIZE 0x38
/* the head of an actor's debug info the AI step checks save and compare
(the firing decision, the burst's tracking and alignment; the rest, its
histories, is 0x657C bytes in all) */
#define HALO_SIM_ACTOR_DEBUG_HEAD_SIZE 0x1A0

/* crates/halo_map's AiCall: what an actor's step asks of the game */
struct halo_sim_ai_call
{
	long kind;
	long actor;
	long a;
	long b;
	float input[9];
	float output[6];
	long result;
	long result2;
};

/* crates/halo_map's AiStepContext */
struct halo_sim_ai_step_context
{
	long actor_index;
	unsigned long actor;
	unsigned long definition;
	unsigned long prop_data;
	long game_time;
	unsigned long actor_data;
	unsigned long object_header_data;
	/* its debug info (ai_debug.h's actor_debug_array) */
	unsigned long debug_info;
	unsigned long variant_definition;
	unsigned long scenario;
	/* global_structure_bsp_get and breakable_surface_flags_get */
	unsigned long structure;
	unsigned long breakable_surface_flags;
	long structure_bsp_index;
	unsigned long encounter_data;
	unsigned long pursuit_data;
	unsigned long squad_array;
	/* the loaded tags' table and its count (cache_files_tag_instances) */
	unsigned long tag_instances;
	long tag_count;
};

/* the emotion update's calls (actor_perception.c's actor_emotion_call;
crates/halo_map's actor_emotions::call) */
enum
{
	_actor_emotion_call_berserk_end = 1,
	_actor_emotion_call_communication_event,
	_actor_emotion_call_friend_attacking,
	_actor_emotion_call_discard_firing_position,
	_actor_emotion_call_flood_shamble,
	_actor_emotion_call_unopposable_retreat,
	_actor_emotion_call_unit_busy,
	_actor_emotion_call_has_path,
	_actor_emotion_call_firing_variant,
	_actor_emotion_call_unit_shamble_state,
	_actor_emotion_call_has_ranged_weapon,
	_actor_emotion_call_random,
	_actor_emotion_call_random_range,
};

/* the situation update's calls (actor_perception.c's actor_situation_call;
crates/halo_map's actor_situation::call) */
enum
{
	_actor_situation_call_unit_actor = 1,
	_actor_situation_call_unit_player,
	_actor_situation_call_unit_active_region_flags,
	_actor_situation_call_target_weight,
};

/* the upkeep's calls (actors.c's actor_upkeep_call; crates/halo_map's
actor_upkeep::call) */
enum
{
	_actor_upkeep_call_time_given = 1,
	_actor_upkeep_call_last_highest_service_timer,
	_actor_upkeep_call_current_highest_service_timer,
	_actor_upkeep_call_give_time,
	_actor_upkeep_call_set_current_highest_service_timer,
};

/* the input update's calls (actors.c's actor_input_call; crates/halo_map's
actor_input::call) */
enum
{
	_actor_input_call_swarm_refresh = 1,
	_actor_input_call_sample_position,
	_actor_input_call_underwater,
	_actor_input_call_vehicle_flags,
	_actor_input_call_gunner_bombardment,
	_actor_input_call_keep_squad,
	_actor_input_call_mark_prevehicle_encounter,
	_actor_input_call_change_encounter,
	_actor_input_call_firing_position_type,
	_actor_input_call_team_is_enemy,
	_actor_input_call_facing,
};

/* the general update's calls (actors.c's actor_general_call; crates/halo_map's
actor_general::call) */
enum
{
	_actor_general_call_profile = 1,
	_actor_general_call_erase,
	_actor_general_call_encounter_force_active,
	_actor_general_call_set_dormant,
	_actor_general_call_action_class,
	_actor_general_call_has_path,
	_actor_general_call_encounter_follows_players,
};

/* the look's effect on movement's calls (actor_looking.c's
actor_look_movement_call), the idle vocalizations' (ai_communication.c's
actor_communication_call) and the conversation's (action_converse.c's
actor_conversation_call); crates/halo_map's actor_attention */
enum
{
	_actor_look_movement_call_has_path = 1,
	_actor_look_movement_call_decode_direction,
};

enum
{
	_actor_communication_call_dialogue_triggers_enabled = 1,
	_actor_communication_call_in_combat,
	_actor_communication_call_reset_idle_vocalization_timer,
	_actor_communication_call_idle_speech,
};

enum
{
	_actor_conversation_call_active_prop = 1,
	_actor_conversation_call_head_position,
	_actor_conversation_call_look,
};

/* the destination update's calls (actor_moving.c's actor_destination_call;
crates/halo_map's actor_destination::call) */
enum
{
	_actor_destination_call_path_refresh = 1,
	_actor_destination_call_test_destination,
	_actor_destination_call_path_clear,
	_actor_destination_call_unfinished_path,
	_actor_destination_call_tau_ceti,
};

/* the unit control's calls (actors.c's actor_unit_control_call;
crates/halo_map's actor_unit_control::call) */
enum
{
	_actor_unit_control_call_player_input_enabled = 1,
	_actor_unit_control_call_set_actively_controlled,
	_actor_unit_control_call_control,
	_actor_unit_control_call_start_animation_impulse,
	_actor_unit_control_call_persistent_control,
};

/* the perception update's calls (actor_perception.c's actor_perception_call;
crates/halo_map's actor_perception::call) */
enum
{
	_actor_perception_call_refresh = 1,
	_actor_perception_call_refresh_danger_zone,
	_actor_perception_call_random,
	_actor_perception_call_noticed_danger,
	_actor_perception_call_refresh_prop,
	_actor_perception_call_debug_start_aware,
	_actor_perception_call_debug_stop_aware,
	_actor_perception_call_knowledge,
	_actor_perception_call_debug_awareness,
	_actor_perception_call_debug_become_aware,
	_actor_perception_call_desire_orphan,
	_actor_perception_call_orphan_transition,
	_actor_perception_call_switch_props,
	_actor_perception_call_become_acknowledged,
	_actor_perception_call_unopposable,
	_actor_perception_call_target_weight,
	_actor_perception_call_prop_delete,
	_actor_perception_call_stimulus_just_killed,
	_actor_perception_call_stimulus_sighted,
	_actor_perception_call_sighted_friend_player,
	_actor_perception_call_team_is_enemy,
	_actor_perception_call_allegiance_changed,
	_actor_perception_call_stimulus_found_body,
	_actor_perception_call_stimulus_perceived_enemy,
	_actor_perception_call_encounter_enemy_unseen,
	_actor_perception_call_alert_noncombat,
	_actor_perception_call_in_combat,
	_actor_perception_call_is_fighting,
	_actor_perception_call_profile,
	_actor_perception_call_dead_prop_state,
	_actor_perception_call_has_ranged_weapon,
	_actor_perception_call_weapon_definition,
	_actor_perception_call_firing_variant,
	_actor_perception_call_stimulus_acknowledged,
	_actor_perception_call_corpse_stopped,
	_actor_perception_call_unit_from_swarm,
	_actor_perception_call_sample_unit,
	_actor_perception_call_sense_position,
	_actor_perception_call_status_refresh,
	_actor_perception_call_status_sample,
	_actor_perception_call_look_started_moving,
	_actor_perception_call_line_of_sight, /* now Rust's (sensing) */
	_actor_perception_call_visibility, /* now Rust's (sensing) */
	_actor_perception_call_combat_state,
	_actor_perception_call_audibility, /* now Rust's (sensing) */
	_actor_perception_call_vehicle_danger,
	_actor_perception_call_suicide_danger,
	_actor_perception_call_unreachable,
	_actor_perception_call_look_interest,
	_actor_perception_call_refresh_pvs,
	_actor_perception_call_pvs_has_unit,
	_actor_perception_call_next_pvs_cluster,
	_actor_perception_call_cluster_first_object,
	_actor_perception_call_cluster_next_object,
	_actor_perception_call_marker_begin,
	_actor_perception_call_mark,
	_actor_perception_call_marker_end,
	_actor_perception_call_mark_swarm_units,
	_actor_perception_call_biped_sense,
	_actor_perception_call_unit_from_swarm_sensed,
	_actor_perception_call_biped_status,
	_actor_perception_call_suicide_danger_at,
	_actor_perception_call_new_prop,
	_actor_perception_call_vehicle_danger_undriven,
	_actor_perception_call_projectile_sample,
	_actor_perception_call_projectile_owner,
	_actor_perception_call_overflow_warning,
	_actor_perception_call_danger_sense,
	_actor_perception_call_danger_details,
	_actor_perception_call_active_prop_of_unit,
	_actor_perception_call_danger_line_of_sight,
	_actor_perception_call_danger_visibility,
	_actor_perception_call_danger_audibility,
	_actor_perception_call_look_danger,
	_actor_perception_call_find_pathfinding_location,
	_actor_perception_call_communicate_grenade_sighted,
	_actor_perception_call_killer_prop,
	_actor_perception_call_look_acknowledged,
	_actor_perception_call_surprise_debug,
	_actor_perception_call_communicate_unexpected_enemy,
	_actor_perception_call_medusa,
	_actor_perception_call_communicate_sighted_enemy,
	_actor_perception_call_prop_unit_details,
	_actor_perception_call_props_full_warning,
	_actor_perception_call_switch_swarm_targets,
	_actor_perception_call_object_origin,
	_actor_perception_call_vehicle_danger_sample,
	_actor_perception_call_attacker,
	_actor_perception_call_sense_detail,
};

/* the look update's calls (actor_looking.c's actor_look_call; crates/halo_map's
actor_looking::call), and the directions it decodes */
enum
{
	_actor_look_call_has_weapon = 1,
	_actor_look_call_firing_burst,
	_actor_look_call_decode,
	_actor_look_call_action_class,
	_actor_look_call_look_timer_expired,
	_actor_look_call_move_force_stop,
	_actor_look_call_idle_timer,
	_actor_look_call_idle_new_major,
	_actor_look_call_idle_new_minor,
	_actor_look_call_unit_is_busy,
	_actor_look_call_abandon_stationary_facing,
	_actor_look_call_exact_facing,
};

enum
{
	_actor_look_direction_target = 0,
	_actor_look_direction_primary,
	_actor_look_direction_secondary,
	_actor_look_direction_idle_major,
	_actor_look_direction_idle_minor,
};

/* the move update's calls (actor_moving.c's actor_move_call; crates/halo_map's
actor_moving::call) */
enum
{
	_actor_move_call_vector_avoidance = 1,
	_actor_move_call_has_path,
	_actor_move_call_vehicle_definition,
	_actor_move_call_vehicle_stuck,
	_actor_move_call_calculate_movement,
	_actor_move_call_clear_discarded_firing_positions,
	_actor_move_call_force_crouch,
	_actor_move_call_unit_control_crouch,
	_actor_move_call_animation_busy,
	_actor_move_call_berserk,
	_actor_move_call_unit_control_jump,
	_actor_move_call_leap_begin,
	_actor_move_call_leap_shout,
	_actor_move_call_find_pathfinding_location,
	_actor_move_call_destination_tolerance,
	_actor_move_call_stopping_distances,
	_actor_move_call_oversteer_disable,
	_actor_move_call_avoidance_setup,
};

/* the combat update's calls (actor_combat.c's actor_combat_call;
crates/halo_map's actor_combat::call), the difficulty values and gun offsets
they take */
enum
{
	_actor_combat_call_firing_variant = 1,
	_actor_combat_call_weapon_definition,
	_actor_combat_call_weapon,
	_actor_combat_call_has_ranged_weapon,
	_actor_combat_call_grenade_count,
	_actor_combat_call_add_grenade,
	_actor_combat_call_throw_grenade,
	_actor_combat_call_weapon_triggers,
	_actor_combat_call_difficulty,
	_actor_combat_call_enable_special_fire,
	_actor_combat_call_random_range,
	_actor_combat_call_random,
	_actor_combat_call_allow_special_fire,
	_actor_combat_call_players_see_cluster,
	_actor_combat_call_manual_line_of_sight, /* now Rust's (sensing) */
	_actor_combat_call_aim_at_target,
	_actor_combat_call_animation_busy,
	_actor_combat_call_weapon_vector,
	_actor_combat_call_first_burst_delay,
	_actor_combat_call_start_burst,
	_actor_combat_call_start_pause,
	_actor_combat_call_time_to_target,
	_actor_combat_call_camera_position,
	_actor_combat_call_gun_position,
	_actor_combat_call_aim_burst,
	_actor_combat_call_line_of_fire,
	_actor_combat_call_blocked_shout,
	_actor_combat_call_firing_pattern,
	_actor_combat_call_primary_trigger,
	_actor_combat_call_secondary_trigger,
};

enum
{
	_actor_combat_difficulty_target_tracking = 0,
	_actor_combat_difficulty_target_leading,
	_actor_combat_difficulty_rate_of_fire,
};

enum
{
	_actor_combat_gun_offset_variant_crouch = 0,
	_actor_combat_gun_offset_definition_crouch,
	_actor_combat_gun_offset_variant_stand,
	_actor_combat_gun_offset_definition_stand,
};

/* the dispatch's calls (actors.c's actor_dispatch_call; crates/halo_map's
actor_dispatch::call) */
enum
{
	_actor_dispatch_call_decide_action = 1,
	_actor_dispatch_call_stimulus_clear,
	_actor_dispatch_call_action_perform,
	_actor_dispatch_call_loop_failed,
	_actor_dispatch_call_action_update,
	_actor_dispatch_call_action_control,
	_actor_dispatch_call_swarm_control,
};

/* what the handlers ask (actions.c's actions_port_handler_call; crates/halo_map's
actor_transitions::call) */
enum
{
	_actions_port_handler_call_set_default_state = 0x1040 | 1,
	_actions_port_handler_call_obey_command_list,
	_actions_port_handler_call_flee_always,
	_actions_port_handler_call_recent_damaging_prop,
	_actions_port_handler_call_attached_projectile_prop,
	_actions_port_handler_call_burning_prop,
	_actions_port_handler_call_squad_delayed, /* now Rust's */
	_actions_port_handler_call_squad_timer_expire, /* now Rust's */
	_actions_port_handler_call_conversation_flags,
	_actions_port_handler_call_animation_impulse,
	_actions_port_handler_call_communication,
	_actions_port_handler_call_fire_wildly,
	_actions_port_handler_call_disable_bursts,
	_actions_port_handler_call_was_surprised,
	_actions_port_handler_call_try_new_target,
	_actions_port_handler_call_firing_variant,
	_actions_port_handler_call_random,
	_actions_port_handler_call_try_to_panic,
	_actions_port_handler_call_combat_selection,
	_actions_port_handler_call_lost_contact,
	_actions_port_handler_call_guard_from_fleeing,
	_actions_port_handler_call_berserk,
	_actions_port_handler_call_guard_from_combat_transition,
	_actions_port_handler_call_tried_to_uncover,
	_actions_port_handler_call_tried_to_search,
	_actions_port_handler_call_abandoned_search,
	_actions_port_handler_call_try_to_seek_cover,
	_actions_port_handler_call_has_ranged_weapon,
	_actions_port_handler_call_difficulty,
	_actions_port_handler_call_charge,
	_actions_port_handler_call_vehicle_definition,
	_actions_port_handler_call_fight,
	_actions_port_handler_call_encounter_stand_down, /* now Rust's */
	_actions_port_handler_call_pursuit_desires,
	_actions_port_handler_call_pursuit_availability,
	_actions_port_handler_call_find_prop_pathfinding_location,
	_actions_port_handler_call_nearby_firing_positions,
	_actions_port_handler_call_uncover_target,
	_actions_port_handler_call_search_target,
	_actions_port_handler_call_target_unit,
	_actions_port_handler_call_search_undirected,
	_actions_port_handler_call_select_pursuit_position,
	_actions_port_handler_call_uncover_pursuit,
	_actions_port_handler_call_search_pursuit,
	_actions_port_handler_call_mark_examined, /* now Rust's (pursuits) */
	_actions_port_handler_call_wait,
	_actions_port_handler_call_guard_postcombat,
	_actions_port_handler_call_guard_find_position,
	_actions_port_handler_call_allow_cover_seeking,
	_actions_port_handler_call_exit_seat,
	_actions_port_handler_call_consider_grenade,
	_actions_port_handler_call_try_to_evade,
	_actions_port_handler_call_animation_busy,
	_actions_port_handler_call_has_path,
	_actions_port_handler_call_discard_firing_position,
	_actions_port_handler_call_find_escape,
	_actions_port_handler_call_try_to_dive,
	_actions_port_handler_call_at_destination,
	_actions_port_handler_call_avoid,
	_actions_port_handler_call_grenades_enabled,
	_actions_port_handler_call_grenade_chance_scale,
	_actions_port_handler_call_test_grenade,
	_actions_port_handler_call_try_to_throw_grenade,
	_actions_port_handler_call_allowed_to_enter_vehicle,
	_actions_port_handler_call_vehicle_driver,
	_actions_port_handler_call_object_origin,
	_actions_port_handler_call_enterable_vehicles,
	_actions_port_handler_call_vehicle_exists,
	_actions_port_handler_call_vehicle_setup,
	_actions_port_handler_call_unit_ready_to_throw,
	_actions_port_handler_call_mark_grenade_thrown, /* now Rust's */
	_actions_port_handler_call_encounter_grenade_timer,
	_actions_port_handler_call_find_grenade_target,
	_actions_port_handler_call_collateral_damage,
	_actions_port_handler_call_plan_grenade_trajectory,
	_actions_port_handler_call_unit_evade_distance,
	_actions_port_handler_call_try_evasion_direction,
	_actions_port_handler_call_test_animation_impulse,
	_actions_port_handler_call_guard_current_position,
	_actions_port_handler_call_guard_find_position_if,
	_actions_port_handler_call_alert,
	_actions_port_handler_call_sleep,
	_actions_port_handler_call_unit_dive_distance,
	_actions_port_handler_call_try_evasion_vector,
	_actions_port_handler_call_evasion_direction,
	_actions_port_handler_call_action_end,
	_actions_port_handler_call_clear_discarded_firing_positions,
	_actions_port_handler_call_action_begin,
	_actions_port_handler_call_seed_random_range,
	_actions_port_handler_call_nearby_firing_positions_at,
	_actions_port_handler_call_real_seed_random_range,
	_actions_port_handler_call_find_flee_position,
	_actions_port_handler_call_melee_inhibited,
	_actions_port_handler_call_melee_range_and_ticks,
	_actions_port_handler_call_valid_melee_destination,
	_actions_port_handler_call_move_to_prop,
	_actions_port_handler_call_keep_moving,
	_actions_port_handler_call_vehicle_desirable,
	_actions_port_handler_call_find_impromptu_seat,
	_actions_port_handler_call_has_enter_animation,
	_actions_port_handler_call_find_vehicle_destination,
	_actions_port_handler_call_move_to_point,
	_actions_port_handler_call_unit_stop_running_blindly,
	_actions_port_handler_call_unit_start_running_blindly,
	_actions_port_handler_call_conversation_finish,
	_actions_port_handler_call_uncover_debug,
	_actions_port_handler_call_forget_recent_damage,
	_actions_port_handler_call_retreat_successful,
	_actions_port_handler_call_playfight_nearby_target,
	_actions_port_handler_call_pursuit_find_nearby_actors,
	_actions_port_handler_call_squad_magic_sight,
};

/* the actions' functions (actions.c's actions_port_ported) */
enum
{
	_actions_port_perform = 0,
	_actions_port_update,
	_actions_port_control,
};

/* the actions' calls ((action << 8) | n: action_wait.c's action_wait_call and
so on; crates/halo_map's actions) */
enum
{
	_action_wait_call_find_nearby_actors = (8 << 8) | 1,
	_action_wait_call_move_to_prop,
	_action_wait_call_move_halt,
	_action_wait_call_communication,
	_action_wait_call_query_time,
};

enum
{
	_action_avoid_call_change_firing_position = (13 << 8) | 1,
};

enum
{
	_action_alert_call_at_destination = (2 << 8) | 1,
	_action_alert_call_play_animation,
	_action_alert_call_has_path,
	_action_alert_call_tolerance,
	_action_alert_call_too_many_positions,
	_action_alert_call_choose_random_position,
	_action_alert_call_random_range,
	_action_alert_call_move_to_move_position,
};

enum
{
	_action_search_call_communication = (7 << 8) | 1,
	_action_search_call_sees_pursuit_location,
	_action_search_call_searching_same_position,
	_action_search_call_mark_examined, /* now Rust's (pursuits) */
	_action_search_call_move_halt,
	_action_search_call_move_to_prop,
	_action_search_call_move_to_firing_position,
};

enum
{
	_action_uncover_call_select_firing_position = (5 << 8) | 1,
	_action_uncover_call_tolerance,
	_action_uncover_call_debug,
	_action_uncover_call_change_firing_position,
	_action_uncover_call_discard_firing_position,
	_action_uncover_call_done,
};

enum
{
	_action_guard_call_team_is_ally = (6 << 8) | 1,
	_action_guard_call_communication,
	_action_guard_call_wake,
	_action_guard_call_communication_target,
	_action_guard_call_discard_firing_position,
	_action_guard_call_change_firing_position,
	_action_guard_call_random_range,
	_action_guard_call_move_halt,
	_action_guard_call_move_to_guard_point,
	_action_guard_call_move_to_firing_position,
	_action_guard_call_shout_dead_friend,
};

enum
{
	_action_flee_call_run_blindly = (4 << 8) | 1,
	_action_flee_call_move_halt,
	_action_flee_call_move_to_firing_position,
	_action_flee_call_discard_firing_position,
	_action_flee_call_at_destination,
	_action_flee_call_tolerance,
	_action_flee_call_situation_status,
	_action_flee_call_position_exposed,
	_action_flee_call_find_flee_position,
	_action_flee_call_unit_is_speaking,
	_action_flee_call_scream,
	_action_flee_call_shout,
};

enum
{
	_action_converse_call_base_prop = (12 << 8) | 1,
	_action_converse_call_move_halt,
	_action_converse_call_move_to_prop,
	_action_converse_call_player_prop,
};

enum
{
	_action_vehicle_call_has_path = (9 << 8) | 1,
	_action_vehicle_call_vehicle_exists,
	_action_vehicle_call_desirable,
	_action_vehicle_call_evaluate_seat,
	_action_vehicle_call_enter_seat,
	_action_vehicle_call_move_halt,
	_action_vehicle_call_find_destination,
	_action_vehicle_call_move_to_destination,
};

enum
{
	_action_charge_call_has_path = (10 << 8) | 1,
	_action_charge_call_firing_variant,
	_action_charge_call_has_ranged_weapon,
	_action_charge_call_weapon_definition,
	_action_charge_call_unit_is_busy,
	_action_charge_call_aim_leap,
	_action_charge_call_melee_attack,
	_action_charge_call_move_to_prop,
	_action_charge_call_keep_moving,
	_action_charge_call_move_halt,
	_action_charge_call_perception_unreachable,
};

enum
{
	_action_obey_call_swarm_update = (11 << 8) | 1,
	_action_obey_call_swarm_perform,
	_action_obey_call_command_perform,
	_action_obey_call_command_end,
	_action_obey_call_command_begin,
	_action_obey_call_at_destination,
	_action_obey_call_animation_busy,
	_action_obey_call_animation_impulse,
	_action_obey_call_communication,
	_action_obey_call_unit_is_busy,
	_action_obey_call_tolerance,
	_action_obey_call_move_halt,
	_action_obey_call_plan_grenade,
	_action_obey_call_recording,
	_action_obey_call_loop_error,
	_action_obey_call_biped_release,
	_action_obey_call_look_secondary_stop,
	_action_obey_call_move_to_destination,
	_action_obey_call_keep_moving,
	_action_obey_call_random_range_short,
	_action_obey_call_random_range,
	_action_obey_call_nearest_player_unit,
	_action_obey_call_object_unit,
	_action_obey_call_active_prop,
	_action_obey_call_head_position,
	_action_obey_call_look_secondary,
	_action_obey_call_add_grenade,
	_action_obey_call_enter_vehicle,
	_action_obey_call_animate,
	_action_obey_call_play_recording,
	_action_obey_call_wake_script,
	_action_obey_call_die,
	_action_obey_call_vocalize,
	_action_obey_call_teleport,
	_action_obey_call_debug_command,
};

enum
{
	_action_fight_call_discard_firing_position = (3 << 8) | 1,
	_action_fight_call_firing_variant,
	_action_fight_call_find_pathfinding_location,
	_action_fight_call_nearby_firing_positions,
	_action_fight_call_near_current_firing_position,
	_action_fight_call_move_halt,
	_action_fight_call_change_firing_position,
	_action_fight_call_random_range,
	_action_fight_call_vehicle_minimum_firing_position_time,
	_action_fight_call_has_ranged_weapon,
	_action_fight_call_firing_position_reaches,
	_action_fight_call_perception_unreachable,
};

/* the firing position selection's calls ((NUMBER_OF_ACTOR_ACTIONS << 8) | n:
actor_firing_position.c's actor_firing_position_call; crates/halo_map's
firing_positions::call) */
enum
{
	_actor_firing_position_call_owners = (14 << 8) | 1,
	_actor_firing_position_call_owner,
	_actor_firing_position_call_estimate_position,
	_actor_firing_position_call_find_prop_pathfinding_location,
	_actor_firing_position_call_friend_attacking,
	_actor_firing_position_call_unit_aiming_vector,
	_actor_firing_position_call_nearby_firing_positions,
	_actor_firing_position_call_path_3d_available,
	_actor_firing_position_call_target_path,
	_actor_firing_position_call_target_distance,
	_actor_firing_position_call_area_path,
	_actor_firing_position_call_area_distance,
	_actor_firing_position_call_random_range,
	_actor_firing_position_call_firing_variant,
	_actor_firing_position_call_aiming_blockage,
	_actor_firing_position_call_weapon_definition,
	_actor_firing_position_call_vehicle,
	_actor_firing_position_call_actor_path_3d_available,
	_actor_firing_position_call_mark_examined, /* now Rust's (pursuits) */
	_actor_firing_position_call_already_examined, /* now Rust's (pursuits) */
	_actor_firing_position_call_line_of_sight, /* now Rust's (sensing) */
	_actor_firing_position_call_change,
	_actor_firing_position_call_flee_approach,
};

/* the object update loop's calls (objects.c's objects_update_call;
crates/halo_map's objects_update::call) */
enum
{
	_objects_call_header = 1,
	_objects_call_begin,
	_objects_call_activation,
	_objects_call_object,
	_objects_call_count_garbage,
	_objects_call_set_interpolation,
	_objects_call_type_update,
	_objects_call_damage_update,
	_objects_call_export_function_values,
	_objects_call_compute_node_matrices,
	_objects_call_compute_function_values,
	_objects_call_compute_change_colors,
	_objects_call_connect_lights,
	_objects_call_postprocess_node_matrices,
	_objects_call_clear_do_not_update,
	_objects_call_clear_being_created,
	_objects_call_delete,
	_objects_call_garbage_collection,
};

/* crates/halo_map's ObjectHeader, ObjectUpdateState and ObjectsCall: a
call of the object update loop, and what it read */
struct halo_sim_objects_header
{
	long count;
	short identifier;
	unsigned char flags;
	unsigned char object_type;
	long unit_player_index;
};

struct halo_sim_objects_state
{
	unsigned long exists;
	unsigned char header_flags;
	unsigned char pad0;
	short interpolation_frame_index;
	short interpolation_frame_count;
	short pad1;
	unsigned long object_flags;
	long first_child_object_index;
	long next_object_index;
	long parent_object_index;
	unsigned long has_collision_model;
	unsigned long has_model;
};

struct halo_sim_objects_call
{
	long kind;
	long object_index;
	long a;
	long b;
	struct halo_sim_objects_header header;
	struct halo_sim_objects_state state;
};

/* the rest of a biped update's calls of the rest of the game (bipeds.c's
biped_rest_call; crates/halo_map's biped_rest::call) */
enum
{
	_biped_rest_call_footstep_shuffle = 1,
	_biped_rest_call_footstep,
	_biped_rest_call_find_ground,
	_biped_rest_call_scream,
	_biped_rest_call_discard,
	_biped_rest_call_vehicle_speech,
	_biped_rest_call_try_to_exit_seat,
	_biped_rest_call_exit_seat_end,
};

/* ---------- prototypes/HALO_SIM.C */

/* a system's mode (game.rust_simulation, HALO_RUST_SIMULATION: a comma
separated list of systems, each optionally with ":check"; the C where the
DLL isn't loaded or the level isn't) */
short halo_sim_mode(short system);

/* point_physics.c: one update after the wind or current and whether the
point is under water are known. TRUE when the Rust port ran and its result
(the flags, place, velocity, location and collision) is written; in check
mode it runs and keeps its result for halo_sim_point_physics_check */
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
	unsigned long *result);
/* point_physics.c, in check mode: the C's result, compared with the Rust's */
void halo_sim_point_physics_check(
	unsigned long result,
	void const *location,
	void const *position,
	void const *velocity,
	void const *collision_normal,
	short const *collision_material_type);

/* objects.c's object_compute_node_matrices: a pose's orientations (the
animation's frame or the model's pose, the object's overlays and scale).
TRUE when the Rust port wrote them (and world_relative and frame_index); in
check mode it runs and keeps its result for halo_sim_pose_orientations_check */
int halo_sim_pose_orientations(long object_index, short node_count, void *orientations,
	int *world_relative, short *frame_index);
void halo_sim_pose_orientations_check(long object_index, short node_count, void const *orientations,
	int world_relative);
/* and its interpolation and node matrices, from the orientations the object
type's preprocess left: TRUE when the Rust port wrote them (the orientations
interpolated in place); in check mode it runs on copies, compared by
halo_sim_pose_matrices_check once the game's own have run */
int halo_sim_pose_matrices(long object_index, short node_count, int world_relative,
	void const *parent_node_matrix, void *orientations, void *matrices);
void halo_sim_pose_matrices_check(long object_index, short node_count, void const *orientations,
	void const *matrices);

/* and the object type's preprocess between them (a unit's, then a
vehicle's; a device's): TRUE when the Rust port ran it (the orientations
changed in place, the unit's euler screens and grenade throw legs set); in
check mode it runs on a copy, compared by halo_sim_pose_preprocess_check
once the game's own has run */
int halo_sim_pose_preprocess(long object_index, short node_count, void *orientations);
void halo_sim_pose_preprocess_check(long object_index, short node_count, void const *orientations);

/* collisions.c's collision_test_vector, once the game's has run: in check
mode the Rust port's ray test, on the running game's objects (through the
host's callbacks), compared with the game's result (flags as the caller
passed them) */
/* crates/halo_sim's DataPacketCall: a packet's encoding or decoding by a
data_packet_group_definition (memory/data_packet_groups.c) */
struct halo_sim_data_packet_call
{
	void const *group;
	void *decoded;
	void *encoded;
	short encoded_size;
	short packet_type;
	short packet_version;
	short expected_packet_class;
	long version;
	/* data_packet_groups.c's error's index, 0 for none */
	long error;
	/* decoding's: the caller's size, written (as the game does) before the
	struct is (a struct shorter than its packet's definition would overrun
	into what lies after it) */
	short *encoded_size_reference;
};

/* the packet codec in Rust (Rust's mode): whether it ran, the call then
holding its results */
int halo_sim_data_packet_encode(struct halo_sim_data_packet_call *call);
int halo_sim_data_packet_decode(struct halo_sim_data_packet_call *call);
/* in check mode: the game's encoding (its result in the call) against
Rust's of the same packet */
void halo_sim_data_packet_encode_check(struct halo_sim_data_packet_call const *game);
/* in check mode: the decoding's inputs kept before the game's runs, then the
game's results (in the call, its buffers as it left them) against Rust's of
the kept inputs */
void halo_sim_data_packet_decode_begin(struct halo_sim_data_packet_call const *call);
void halo_sim_data_packet_decode_check(struct halo_sim_data_packet_call const *game);

/* crates/halo_sim's WireCall: one of network_distributed.c's packers,
writers or readers (crates/halo_map's distributed_wire) */
enum
{
	_halo_sim_wire_vector_pack = 1,
	_halo_sim_wire_vector_unpack,
	_halo_sim_wire_unit_vector_unpack,
	_halo_sim_wire_vitality_pack,
	_halo_sim_wire_vitality_unpack,
	_halo_sim_wire_angle_pack,
	_halo_sim_wire_angle_unpack,
	_halo_sim_wire_unit_state_write,
	_halo_sim_wire_unit_state_read,
	_halo_sim_wire_relayed_action_write,
	_halo_sim_wire_relayed_action_read,
	_halo_sim_wire_relayed_action_from,
	_halo_sim_wire_axes_make_valid,
	_halo_sim_wire_transform_valid,
	_halo_sim_wire_vector_clamp,
	_halo_sim_wire_checksum,
	_halo_sim_wire_object_state_unpack,
	_halo_sim_wire_state_from_player,
	_halo_sim_wire_state_from_object,
	_halo_sim_wire_inventory_from_unit,
	NUMBER_OF_HALO_SIM_WIRE_KINDS
};

/* crates/halo_sim's WireTransform: a transform's parts (the velocities
NULL for none) */
struct halo_sim_wire_transform
{
	void const *position;
	void const *forward;
	void const *up;
	void const *velocity;
	void const *angular_velocity;
};

/* crates/halo_map's distributed_wire::Game: where the samplers find the
objects and players */
struct halo_sim_wire_game
{
	unsigned long object_header_data;
	unsigned long player_data;
};

/* crates/halo_map's distributed_wire::PlayerExtras: what a player's state
takes of the netcode's own tables */
struct halo_sim_wire_player_extras
{
	long game_time;
	long server;
	long tracked;
	long prediction_taken_time;
	long prediction_taken_host_time;
	long death_valid;
	long death_killing_player_index;
	long death_friendly_fire;
	long death_killed_by_vehicle;
};

struct halo_sim_wire_call
{
	long kind;
	void const *input;
	void const *end;
	void *output;
	long a;
	long b;
	float value;
	long result;
	float real_result;
};

/* Rust's (Rust's mode): whether it ran, the call then holding its results */
int halo_sim_distributed_wire(struct halo_sim_wire_call *call);
/* in check mode: the game's (its results in the call, output_size bytes
of them at its output) against Rust's of the same inputs */
void halo_sim_distributed_wire_check(struct halo_sim_wire_call const *game, long output_size);

void halo_sim_collision_test_vector_check(unsigned long flags, void const *point, void const *vector,
	long ignore_object_index, int hit, void const *collision);

/* projectiles.c's projectile_update, after its tracer: TRUE when the Rust
port ran the update in its place (its timers, flight and collisions, the
game called back for what it does to the world) and left the collision
count and time remaining; in check mode it notes the state the game's
update begins with, and the game's update records its calls */
int halo_sim_projectile_update(long projectile_index, short *collision_count, float *time_remaining);
/* and before it detonates or disappears, in check mode: the Rust port's
replay of the game's update, compared with where the game left it */
void halo_sim_projectile_check(long projectile_index, short collision_count, float time_remaining);
/* the game's random seed, as a recorded call begins */
unsigned long halo_sim_projectile_seed(void);
/* a call of the game's update, recorded in check mode: what it was given
(the object; the material type, or breakable surface; whether the potential
response; the scales) and gave back, and the seed it began with */
void halo_sim_projectile_event(short kind, unsigned long seed_before, long object_index, short material_type,
	int potential, float scale, float material_effect_scale);
void halo_sim_projectile_event_guidance(void const *projectile_center, void const *target_center,
	void const *target_center_of_mass, float guidance_scale, long game_time);
/* a move or attaching, and where it left the projectile */
void halo_sim_projectile_event_state(short kind, unsigned long seed_before, long projectile_index);

/* bipeds.c's biped_update_physics, on its physics (the game's struct
biped_physics): TRUE when the Rust port moved it (the game's own then
doesn't run); in check mode it runs on a copy, compared by
halo_sim_biped_physics_check once the game's own has run. stand_test: the
crouching player's stand test applies, with the definition's standing
height and radius */
int halo_sim_biped_physics_update(void *physics, int stand_test, float collision_height_standing,
	float collision_radius);
void halo_sim_biped_physics_check(void const *physics);

/* physics.c's physics_update: TRUE when the Rust port ran it (the vehicle
moved and its unit collisions done by the game, called back; its mass
points written); in check mode it notes the state the game's update begins
with, the game's unit collisions note the vehicles they consider
(halo_sim_vehicle_physics_other), and halo_sim_vehicle_physics_check, once
the game's update is done, compares Rust's replay of it */
int halo_sim_vehicle_physics_update(long vehicle_index, void *powered_mass_points, short powered_count,
	void *mass_points, short mass_point_count, void const *magic_force, void const *magic_torque);
void halo_sim_vehicle_physics_other(long object_index);
void halo_sim_vehicle_physics_check(long vehicle_index, void const *mass_points, short mass_point_count);

/* vehicles.c's vehicle_update, once a vehicle not riding a parent is
past its parent check: TRUE when the Rust port ran it up to its falling
damage (its physics, effects and sounds the game's, called back); in check
mode it notes the state the game's update begins with, and
halo_sim_vehicle_update_check, before its animation, compares Rust's
replay of it */
int halo_sim_vehicle_update(long vehicle_index);
void halo_sim_vehicle_update_check(long vehicle_index);

/* bipeds.c's biped_update_moving, with the update's animation data and
what the rest of the game answers it (bevy_bridge_biped_moving_begin's):
TRUE when the Rust port moved the biped (what it asks of the game called
back); in check mode it notes the state the game's update begins with, the
game's jump notes the AI's answer (halo_sim_biped_actor_jump), and
halo_sim_biped_moving_check compares Rust's replay of it */
int halo_sim_biped_moving_update(long biped_index, void *animation, float const *aiming_vector,
	float difficulty_infection_forms, float speed_multiplier, int cinematic, int double_speed,
	int coop_player_collisions, int actor_leaping, int recorded_animation);
void halo_sim_biped_moving_check(long biped_index, void const *animation);
void halo_sim_biped_actor_jump(int jumped, void const *velocity);

/* bipeds.c's biped_update of an unparented biped, up to its player melee
(facing, turning, the movement, the dead, airborne, landing and slipping
updates), with the update's animation data, what the rest of the game
answers its movement, and unit_animation_enhanced: TRUE when the Rust port
ran it (what it asks of the game called back); in check mode it notes the
state the game's update begins with, the game's airborne update notes the
random seed (halo_sim_biped_update_airborne), and
halo_sim_biped_update_check compares Rust's replay of it */
int halo_sim_biped_update(long biped_index, void *animation, float const *aiming_vector,
	float difficulty_infection_forms, float speed_multiplier, int cinematic, int double_speed,
	int coop_player_collisions, int actor_leaping, int recorded_animation, int enhanced_animation);
void halo_sim_biped_update_check(long biped_index, void const *animation);
void halo_sim_biped_update_airborne(void);

/* units.c's unit_update_animation, with its animation update data, the
current weapon's label, the magic base seat, the unit definition's graph
and "destroyed after dying" flag, and the biped definition's "has no dying
airborne" flag: TRUE when the Rust port ran it (its side effects the
game's, called back: units.c's unit_rust_animation_effect) and left what
it returns in *result; in check mode it notes the state and random seed
the game's update begins with, and halo_sim_unit_animation_check compares
Rust's replay of it */
int halo_sim_unit_animation_update(long unit_index, void *animation, char const *weapon_label,
	short magic_base_seat_index, long unit_graph_index, int destroyed_after_dying, int has_no_dying_airborne,
	short *result);
void halo_sim_unit_animation_check(long unit_index, short result);

/* bipeds.c's player melee step, with its weapon (NONE for none) and whether
the weapon prevents a melee, the unit definition's graph and
unit_animation_enhanced: TRUE when the Rust port ran it (the reload's
stopping, the first-person message and the damage the game's, called
back); in check mode it notes what the game's step begins with, and
halo_sim_player_melee_check compares Rust's replay of it */
int halo_sim_player_melee(long biped_index, long weapon_index, int weapon_prevents, long unit_graph_index,
	int enhanced_animation);
void halo_sim_player_melee_check(long biped_index);

/* units.c's unit_update, with its unit globals, whether integrated lights
are allowed, whether the game engine runs, and the jetpack and infinite
ammo cheats: TRUE when the Rust port ran it (its calls of the game through
unit_update_call); in check mode it notes the state the game's update
begins with, unit_update_call records each call (halo_sim_unit_event_begin
and _end), and halo_sim_unit_update_check compares Rust's replay of it */
int halo_sim_unit_update(long unit_index, void *unit_globals, int allow_integrated_lights, int game_engine_running,
	int cheat_jetpack, int cheat_infinite_ammo);
void halo_sim_unit_update_check(long unit_index);
void halo_sim_unit_event_begin(long unit_index, long kind, long a, float b);
void halo_sim_unit_event_end(long unit_index, long result);

/* bipeds.c's biped_update: the rest of a biped's update, riding (the
parented branch) or after the update (footsteps, falling danger,
discarding), with the update's animation data: TRUE when the Rust port ran
it (its calls of the game through biped_rest_call); in check mode it notes
the state the game's begins with, biped_rest_call records each call
(halo_sim_biped_rest_event_begin and _end), and halo_sim_biped_rest_check
compares Rust's replay of it */
int halo_sim_biped_rest(long biped_index, void *animation, int riding);
void halo_sim_biped_rest_check(long biped_index, void const *animation);
void halo_sim_biped_rest_event_begin(long biped_index, long kind, long a, long b);
void halo_sim_biped_rest_event_end(long biped_index, long result);

/* damage.c's object_cause_damage (the network's authority over it already
the game's): TRUE when the Rust port dealt it (its calls of the game
through damage_call); in check mode it notes the objects the game's begins
with, damage_call records each call (halo_sim_damage_event_begin and _end),
and halo_sim_object_cause_damage_check compares Rust's replay of it */
struct damage_data;
int halo_sim_object_cause_damage(struct damage_data *damage, long object_index, short node_index, short region_index,
	short material_index, int has_normal, int distributed_damage_authorized, int cheat_omnipotent,
	int cheat_deathless_player);
void halo_sim_object_cause_damage_check(struct damage_data const *damage);
void halo_sim_damage_event_begin(struct damage_data const *damage, long kind, long object_index, long a, long b,
	float const *f);
void halo_sim_damage_event_end(struct damage_data const *damage, long object_index, long result, float value);

/* damage.c's area_of_effect_cause_damage: TRUE when the Rust port dealt it
(its calls of the game through area_damage_call, its rays its own); in
check mode it notes the clusters the sphere reaches with their objects and
the objects, area_damage_call records each call (rays included) with what a
dealing of damage changed (halo_sim_area_damage_event_begin and _end), and
halo_sim_area_of_effect_cause_damage_check compares Rust's replay of it */
int halo_sim_area_of_effect_cause_damage(struct damage_data *damage);
void halo_sim_area_of_effect_cause_damage_check(struct damage_data const *damage);
void halo_sim_area_damage_event_begin(struct damage_data const *damage, long kind, long object_index, long a, long b,
	float const *f);
void halo_sim_area_damage_event_end(struct damage_data const *damage, long result, float const *out);

/* damage.c's object_damage_update, for an object with a collision model:
TRUE when the Rust port ran it (its calls of the game through
damage_update_call); in check mode it notes the object the game's begins
with, damage_update_call records each call (halo_sim_damage_update_event_begin
and _end), and halo_sim_object_damage_update_check compares Rust's replay */
int halo_sim_object_damage_update(long object_index);
void halo_sim_object_damage_update_check(long object_index);
void halo_sim_damage_update_event_begin(long object_index, long kind, float f);
void halo_sim_damage_update_event_end(long object_index, long result, float value);

/* devices.c's device_update (part 0) and device_machines.c's
machine_update (part 1): TRUE when the Rust port ran it (`changed` its
result; its calls of the game through device_call); in check mode it notes
the device the game's begins with, device_call records each call
(halo_sim_device_event_begin and _end) and device_bipeds_in_sphere each
sphere's bipeds, and halo_sim_device_update_check compares Rust's replay */
int halo_sim_device_update(long device_index, int part, unsigned char *changed);
void halo_sim_device_update_check(long device_index, int part, unsigned char changed);
void halo_sim_device_event_begin(long device_index, long kind, long object_index, long a, long b, float const *f);
void halo_sim_device_event_end(long device_index, long result);
void halo_sim_device_bipeds_in_sphere(long machine_index, short cluster, void const *center, float radius,
	short maximum_count, long const *object_indices, short object_count);

/* items.c's item_update: TRUE when the Rust port ran it (its calls of the
game through item_call); in check mode it notes the item the game's begins
with, item_call records each call (halo_sim_item_event_begin and _end), and
halo_sim_item_update_check compares Rust's replay */
int halo_sim_item_update(long item_index);
void halo_sim_item_update_check(long item_index);
void halo_sim_item_event_begin(long item_index, long kind, long a, long b, long c, float const *f);
void halo_sim_item_event_end(long item_index, long result);

/* weapons.c's weapon_update: TRUE when the Rust port ran it (its calls of
the game through weapon_game_call); in check mode it notes the weapon and
random seed the game's begins with, weapon_game_call records each call
(halo_sim_weapon_event_begin and _end), and
halo_sim_weapon_update_check compares Rust's replay */
int halo_sim_weapon_update(long weapon_index);
void halo_sim_weapon_update_check(long weapon_index);
void halo_sim_weapon_event_begin(long weapon_index, struct halo_sim_weapon_game_call const *call);
void halo_sim_weapon_event_end(long weapon_index, struct halo_sim_weapon_game_call const *call, long still_there);
long weapon_game_call(long weapon_index, struct halo_sim_weapon_game_call *call);

/* objects.c's objects_update: TRUE when the Rust port ran the tick's loop
(its calls through objects_update_call); in check mode objects_update_call
records each call and what it read (halo_sim_objects_event), and
halo_sim_objects_update_check compares Rust's replay */
int halo_sim_objects_update(int dont_update_object);
void halo_sim_objects_update_check(int dont_update_object);
void halo_sim_objects_event(struct halo_sim_objects_call const *call);
void objects_update_call(struct halo_sim_objects_call *call);

/* garbage.c's garbage_update and scenery.c's scenery_update, under
`objects`: TRUE when the Rust port ran them; in check mode the game's runs
and the _check compares Rust's from the state it began with */
int halo_sim_garbage_lifetime(short *lifetime_ticks, unsigned char *active);
void halo_sim_garbage_lifetime_check(short lifetime_ticks, unsigned char active);
int halo_sim_scenery_animation(long scenery_index);
void halo_sim_scenery_animation_check(long scenery_index);

/* objects.c's object_compute_function_values (part 0) and
object_compute_change_colors (part 1): TRUE when the Rust port computed
them; in check mode it notes the values the game's begins with, and
halo_sim_object_functions_check compares Rust's from them with the game's */
int halo_sim_object_functions(long object_index, int part);
void halo_sim_object_functions_check(long object_index, int part);

/* object_types.c's object_type_export_function_values: TRUE when the Rust
port exported them; in check mode it notes the state the game's begins
with, and halo_sim_object_exports_check compares Rust's from it */
int halo_sim_object_exports(long object_index);
void halo_sim_object_exports_check(long object_index);

/* units.c's unit_postprocess_node_matrices: TRUE when the Rust port
adjusted the unit's node matrices; in check mode it notes the matrices the
game's begins with, halo_sim_unit_ik_note_grenade the game's answer to
whether a grenade throw releases the vehicle's grip, and
halo_sim_unit_ik_check compares Rust's */
int halo_sim_unit_ik(long unit_index, void *node_matrices, long current_weapon_object_index);
void halo_sim_unit_ik_check(long unit_index, void const *node_matrices);
void halo_sim_unit_ik_note_grenade(long unit_index, int releases);

/* objects.c's object_connect_lights: TRUE when the Rust port chose the
lights (`lights`, `light_count`); in check mode the game's list is compared */
int halo_sim_object_lights(long object_index, long *lights, short *light_count);
void halo_sim_object_lights_check(long object_index, long const *lights, short light_count);

/* effects.c's effect_update: TRUE when the Rust port ran it (its calls of
the game through effect_call); in check mode `checking` is set when this
update is the one checked (a nested one isn't): it notes the effect and seeds
the game's begins with, effect_call records each call
(halo_sim_effect_event_begin and _end), and halo_sim_effect_update_check
compares Rust's replay */
int halo_sim_effect_update(long effect_index, float dt, int *checking);
void halo_sim_effect_update_check(long effect_index);
/* (begin's result: whether it opened an event, which only that call's end
closes) */
int halo_sim_effect_event_begin(long effect_index, struct halo_sim_effect_call const *call);
void halo_sim_effect_event_end(long effect_index, struct halo_sim_effect_call const *call, long still_there,
	int opened);
long effect_call(long effect_index, struct halo_sim_effect_call *call);
/* effects.c's effect_new_*: TRUE when the Rust port created the effect
(`call->result`); in check mode `checking` is set when this creation is the
one checked, and halo_sim_effect_new_check compares Rust's replay with the
effect the game's made */
int halo_sim_effect_new(struct halo_sim_effect_new_call *call, int *checking);
void halo_sim_effect_new_check(long effect_index);
/* effects.c's: an effect's state (FALSE if it is gone: the seeds are still
read), and back (the seeds always); its location instances (FALSE if they
don't fit) */
int effect_halo_sim_state_load(long effect_index, struct halo_sim_effect_state *state);
void effect_halo_sim_state_store(struct halo_sim_effect_state const *state);
int effect_halo_sim_instances(long effect_index, long *first_instances, struct halo_sim_effect_instance *instances,
	unsigned long maximum_instance_count, unsigned long *instance_count);

/* particles.c's particles_update, each particle's: TRUE when the Rust port
ran it (its calls of the game through particle_call); in check mode
`checking` is set, particle_call records each call
(halo_sim_particle_event_begin and _end), and halo_sim_particle_update_check
compares Rust's replay */
int halo_sim_particle_update(long particle_index, float dt, long frame_index, long oldest_tick_frame_index,
	long update_ticks, int *checking);
void halo_sim_particle_update_check(long particle_index);
int halo_sim_particle_event_begin(long particle_index, struct halo_sim_particle_call const *call);
void halo_sim_particle_event_end(long particle_index, struct halo_sim_particle_call const *call, long still_there,
	int opened);
long particle_call(long particle_index, struct halo_sim_particle_call *call);
/* particles.c's particle_new: TRUE when the Rust port made the particle; in
check mode `checking` is set when this one is checked, and
halo_sim_particle_new_check compares Rust's replay with the particle the
game's made (NONE for none) */
struct new_particle_data;
int halo_sim_particle_new(struct new_particle_data const *data, int *checking);
void halo_sim_particle_new_check(long particle_index);
/* particles.c's: a particle's state (FALSE if it is gone: the seed is still
read), and back (the seed always) */
int particle_halo_sim_state_load(long particle_index, struct halo_sim_particle_state *state);
void particle_halo_sim_state_store(struct halo_sim_particle_state const *state);

/* contrails.c's contrails_update, each contrail's: TRUE when the Rust port
ran it (its calls of the game through contrail_call); in check mode
`checking` is set, contrail_call records each call
(halo_sim_contrail_event_begin and _end), and halo_sim_contrail_update_check
compares Rust's replay with the contrail and points the game's left */
int halo_sim_contrail_update(long contrail_index, float dt, float point_dt, int *checking);
void halo_sim_contrail_update_check(long contrail_index);
int halo_sim_contrail_event_begin(long contrail_index, struct halo_sim_contrail_call const *call);
void halo_sim_contrail_event_end(long contrail_index, struct halo_sim_contrail_call const *call, int opened);
void contrail_call(long contrail_index, struct halo_sim_contrail_call *call);
/* contrails.c's contrail_new: TRUE when the Rust port made the contrail
(`contrail_index`); in check mode `checking` is set when this one is
checked, and halo_sim_contrail_new_check compares Rust's replay */
int halo_sim_contrail_new(long definition_index, long object_index, short attachment_index, long *contrail_index,
	int *checking);
void halo_sim_contrail_new_check(long contrail_index);
/* contrails.c's: a contrail and its points in the order its lists walk
(FALSE if it is gone, or they don't fit: the seed is still read), and back
(the seed always) */
int contrail_halo_sim_load(long contrail_index, struct halo_sim_contrail_state *state,
	struct halo_sim_contrail_point *points, unsigned long maximum_point_count, unsigned long *point_count);
void contrail_halo_sim_store(struct halo_sim_contrail_state const *state, struct halo_sim_contrail_point const *points,
	unsigned long point_count);

/* particle_systems.c's particle_systems_update, each system's: TRUE when
the Rust port ran it (its calls of the game through particle_system_call);
in check mode `checking` is set, particle_system_call records each call
(halo_sim_particle_system_event_begin and _end), and
halo_sim_particle_system_update_check compares Rust's replay with the system
and particles the game's left */
int halo_sim_particle_system_update(long system_index, float dt, int *checking);
void halo_sim_particle_system_update_check(long system_index);
int halo_sim_particle_system_event_begin(long system_index, struct halo_sim_particle_system_call const *call);
void halo_sim_particle_system_event_end(long system_index, struct halo_sim_particle_system_call const *call,
	int opened);
void particle_system_call(long system_index, struct halo_sim_particle_system_call *call);
/* particle_systems.c's particle_system_new_unattached (`attached` FALSE) and
_attached: TRUE when the Rust port made the system (`system_index`); in
check mode `checking` is set when this one is checked, and
halo_sim_particle_system_new_check compares Rust's replay */
int halo_sim_particle_system_new(int attached, long definition_index, real_point3d const *position,
	real_vector3d const *velocity, real_argb_color const *color, float scale, long object_index,
	short attachment_index, long *system_index, int *checking);
void halo_sim_particle_system_new_check(long system_index);
/* particle_systems.c's: a system and its particles in the order its lists
walk (FALSE if it is gone, or they don't fit: the seed is still read), and
back (the seed always) */
int particle_system_halo_sim_load(long system_index, struct halo_sim_particle_system_state *state,
	struct halo_sim_system_particle *particles, unsigned long maximum_particle_count, unsigned long *particle_count);
void particle_system_halo_sim_store(struct halo_sim_particle_system_state const *state,
	struct halo_sim_system_particle const *particles, unsigned long particle_count);

/* player_effects.c's player effects: TRUE when the Rust port ran the
operation (its answers written where `args` says); in check mode `checking`
is set, player_effect_call records each call
(halo_sim_player_effect_event_begin and _end), and
halo_sim_player_effect_check compares Rust's replay */
int halo_sim_player_effect(struct halo_sim_player_effect_args *args, int *checking);
void halo_sim_player_effect_check(struct halo_sim_player_effect_args const *args);
int halo_sim_player_effect_event_begin(struct halo_sim_player_effect_call const *call);
void halo_sim_player_effect_event_end(struct halo_sim_player_effect_call const *call, int opened);
void player_effect_call(short local_player_index, struct halo_sim_player_effect_call *call);
void player_effect_halo_sim_load(short local_player_index, struct halo_sim_player_effect_state *state);
void player_effect_halo_sim_store(struct halo_sim_player_effect_state const *state);

/* game.c's game_tick: TRUE when the Rust port ran the tick (its steps
through game_tick_call); in check mode `checking` is set, game_tick_call
notes each step (halo_sim_game_tick_note), and halo_sim_game_tick_check
compares Rust's replay */
int halo_sim_game_tick(int *checking);
void halo_sim_game_tick_check(void);
void halo_sim_game_tick_note(long kind, float f, long result);
long game_tick_call(long kind, float f);

/* hs_runtime.c's update, level start and evaluation (the operation, with its
expression): TRUE when the Rust port ran it (`result` its value); in check
mode `checking` is set, hs_runtime_call records each call
(halo_sim_hs_event_begin and _end), and halo_sim_hs_check (with the game's
value) puts the script memory back as the operation began, runs Rust's
replay on it, compares, and leaves the game's */
int halo_sim_hs_begin(long operation, long expression, long *result, int *checking);
void halo_sim_hs_check(long result);
int halo_sim_hs_event_begin(struct halo_sim_hs_call const *call);
void halo_sim_hs_event_end(struct halo_sim_hs_call const *call, int opened);
void hs_runtime_call(struct halo_sim_hs_call *call);
void hs_runtime_port_context(struct halo_sim_hs_context *context);
void hs_runtime_port_thread(long thread_index, void **thread_address, long **destination);

/* ai.c's ai_update: TRUE when the Rust port ran it (its steps through
ai_update_call); in check mode `checking` is set, ai_update_call notes each
call (halo_sim_ai_update_note), and halo_sim_ai_update_check compares
Rust's replay */
int halo_sim_ai_update(int *checking);

/* the AI's path search (path.c's path_state_find and
path_state_estimated_distance; crates/halo_map's path): the game's state
(HALO_SIM_PATH_STATE_SIZE bytes, its debug storage HALO_SIM_PATH_DEBUG_SIZE),
searched by the game's `game` or Rust's as the paths system runs */
#define HALO_SIM_PATH_STATE_SIZE 0x1408C
#define HALO_SIM_PATH_DEBUG_SIZE 0x1CA7C
int halo_sim_path_find(void *state, void *debug, unsigned long breakable_surface_flags, long structure_bsp_index,
	int (*game)(void *state));
int halo_sim_path_estimated_distance(void *state, float const *end_point, long end_surface_index,
	float *distance, float *closest_approach, float *direction,
	int (*game)(void *state, float const *end_point, long end_surface_index, float *distance, float *closest_approach,
		float *direction));

/* path_structure_bsp.c's 2D tests, path_smoothing.c's smoothing and path.c's
approach point, the same way (a path_collision_result is 28 bytes, a
path_step 16) */
typedef int (*halo_sim_path_line2d_function)(void const *structure, int ignore_broken_surfaces, float const *p0,
	long p0_surface_index, float const *p1, long p1_surface_index, void *result);
typedef int (*halo_sim_path_pill2d_function)(void const *structure, int ignore_broken_surfaces, float const *p0,
	long p0_surface_index, float const *p1, long p1_surface_index, float radius, unsigned long flags, void *result);
typedef void (*halo_sim_path_smooth_function)(void *state, short raw_step_count, void const *raw_steps,
	short *smoothed_step_count, void *smoothed_steps, int *steps_finish_path);
typedef int (*halo_sim_path_approach_function)(void *state, float const *end_point, long end_surface_index,
	int *straight_line, float *approach_point);
int halo_sim_path_test_line2d(void const *structure, int ignore_broken_surfaces, float const *p0,
	long p0_surface_index, float const *p1, long p1_surface_index, void *result, unsigned long breakable_surface_flags,
	halo_sim_path_line2d_function game);
int halo_sim_path_test_pill2d(void const *structure, int ignore_broken_surfaces, float const *p0,
	long p0_surface_index, float const *p1, long p1_surface_index, float radius, unsigned long flags, void *result,
	unsigned long breakable_surface_flags, halo_sim_path_pill2d_function game);
void halo_sim_path_smooth(void *state, short raw_step_count, void const *raw_steps, short *smoothed_step_count,
	void *smoothed_steps, int *steps_finish_path, unsigned long breakable_surface_flags,
	halo_sim_path_smooth_function game);
int halo_sim_path_approach_point(void *state, float const *end_point, long end_surface_index, int *straight_line,
	float *approach_point, unsigned long breakable_surface_flags, halo_sim_path_approach_function game);

/* path.c's path_state_build_path (a path_result is HALO_SIM_PATH_RESULT_SIZE
bytes): the game's or Rust's, which gathers the discs of the objects in the
way through the game's `discs_in_sphere` (obstacles_get_discs_in_sphere);
`build_flags` bit 0 no smoothing, bit 1 no obstacle avoidance (a local
game's debug settings) */
#define HALO_SIM_PATH_RESULT_SIZE 0x5C
typedef void (*halo_sim_path_discs_function)(void *obstacles, float const *center, float radius,
	float const *movement_direction, long ignore_source_object_index, long ignore_target_object_index);
int halo_sim_path_build(void *state, void *debug, void *path, unsigned long structure,
	unsigned long breakable_surface_flags, long build_flags, halo_sim_path_discs_function discs_in_sphere,
	int (*game)(void *state, void *path));
void halo_sim_ai_update_check(void);
void halo_sim_ai_update_note(struct halo_sim_ai_update_call const *call);
void ai_update_call(struct halo_sim_ai_update_call *call);
long ai_update_call_simple(long kind, long a);
void actors_port_call(struct halo_sim_ai_update_call *call);

/* an actor's step (_halo_sim_ai_step_*): TRUE when the Rust port ran it; in
check mode `checking` is set, the step's call function records each call
(halo_sim_ai_event_begin and _end), and halo_sim_ai_step_check puts the
actor back as the step began, runs Rust's replay on it, compares, and leaves
the game's */
int halo_sim_ai_step_begin(long step, long actor_index, int *checking);
long halo_sim_ai_step_result(void);
void halo_sim_ai_step_check(long step, long result);
int halo_sim_ai_event_begin(struct halo_sim_ai_call const *call);
void halo_sim_ai_event_end(struct halo_sim_ai_call const *call, int opened);
void actors_port_step_context(long actor_index, struct halo_sim_ai_step_context *context);
void actor_emotion_call(struct halo_sim_ai_call *call);
void actor_situation_call(struct halo_sim_ai_call *call);
void actor_upkeep_call(struct halo_sim_ai_call *call);
void actor_input_call(struct halo_sim_ai_call *call);
void actor_general_call(struct halo_sim_ai_call *call);
void actor_look_movement_call(struct halo_sim_ai_call *call);
void actor_communication_call(struct halo_sim_ai_call *call);
void actor_conversation_call(struct halo_sim_ai_call *call);
void actor_destination_call(struct halo_sim_ai_call *call);
void actor_unit_control_call(struct halo_sim_ai_call *call);
void actor_perception_call(struct halo_sim_ai_call *call);
void actor_look_call(struct halo_sim_ai_call *call);
void actor_move_call(struct halo_sim_ai_call *call);
void actor_combat_call(struct halo_sim_ai_call *call);
void actor_dispatch_call(struct halo_sim_ai_call *call);
void actor_firing_position_call(struct halo_sim_ai_call *call);

/* an actor's path's calls (actor_moving.c's actor_path_call; crates/halo_map's
actor_path::call), in a range of their own that any step's call function
passes on */
enum
{
	_actor_path_call_set_awake = (15 << 8) | 1,
	_actor_path_call_find_prop_pathfinding_location,
	_actor_path_call_path_3d_available,
	_actor_path_call_destination_tolerance,
	_actor_path_call_build_3d,
	_actor_path_call_build,
	/* (the game's alone: a cached path state, `a`) */
	_actor_path_call_build_cached,
	_actor_path_call_find_pathfinding_location,
	_actor_path_call_seed_random,
};

void actor_path_call(struct halo_sim_ai_call *call);

/* an actor's choice of a vehicle and its seat's calls (action_vehicle.c's
vehicle_choice_call; crates/halo_map's vehicle_choice::call), in a range of
their own the dispatch step's call function passes on */
enum
{
	_vehicle_choice_call_seat_status = (17 << 8) | 1,
	_vehicle_choice_call_seat_entrance,
	_vehicle_choice_call_seat_hint,
	_vehicle_choice_call_vehicle_shape,
	_vehicle_choice_call_object_origin,
};

void vehicle_choice_call(struct halo_sim_ai_call *call);

/* ---------- the world steps: the AI's updates over the game's arrays, not
one actor (halo_sim.c; crates/halo_sim's world_steps) */

enum
{
	_halo_sim_world_step_encounters = 0,
	_halo_sim_world_step_conversations,
	_halo_sim_world_step_communication,
	_halo_sim_world_step_network_apply_state,
	_halo_sim_world_step_network_object_correct,
	_halo_sim_world_step_network_object_reconcile,
	_halo_sim_world_step_network_correct_own_unit,
	_halo_sim_world_step_network_unit_state,
	_halo_sim_world_step_network_object_state,
	_halo_sim_world_step_network_inventory,
	_halo_sim_world_step_network_object_change,
	_halo_sim_world_step_network_hit_report,
	_halo_sim_world_step_network_damage_bookkeeping,
	_halo_sim_world_step_network_damage_replay,
	_halo_sim_world_step_network_damage_deals,
	_halo_sim_world_step_network_damage_player_effect,
	_halo_sim_world_step_network_damage_aftermath,
	_halo_sim_world_step_network_host_tick,
	_halo_sim_world_step_network_vehicle_predictions,
	_halo_sim_world_step_network_plan_players,
	_halo_sim_world_step_network_send_players,
	_halo_sim_world_step_network_client_tick,
	_halo_sim_world_step_network_client_asked,
	_halo_sim_world_step_network_handle_message,
	_halo_sim_world_step_network_apply_predictions,
	_halo_sim_world_step_network_handle_inputs,
	_halo_sim_world_step_network_handle_actions,
	_halo_sim_world_step_network_statistics,
	_halo_sim_world_step_network_pickups,
	_halo_sim_world_step_network_client_sends,
	_halo_sim_world_step_network_client_clock,
	_halo_sim_world_step_network_coop_world,
	_halo_sim_world_step_network_coop_device_groups,
	_halo_sim_world_step_network_coop_queue,
	_halo_sim_world_step_network_coop_object_names,
	_halo_sim_world_step_network_coop_screen_effect,
	_halo_sim_world_step_network_coop_presentation,
	_halo_sim_world_step_network_coop_resend,
	_halo_sim_world_step_coop_enemies,
	_halo_sim_world_step_network_coop_client_world,
	_halo_sim_world_step_network_coop_client_devices,
	_halo_sim_world_step_network_coop_client_names,
	NUMBER_OF_HALO_SIM_WORLD_STEPS
};

/* memory a world step works on (an array, a data array's header) */
struct halo_sim_world_region
{
	unsigned long address;
	unsigned long size;
};

/* the game's arrays an actor's step's check follows besides the actor and its
props (the encounters and pursuits, for the dispatch steps); returns their
count */
long actors_port_step_regions(
	struct halo_sim_world_region *regions,
	long maximum);

/* crates/halo_map's WorldContext */
struct halo_sim_world_context
{
	long game_time;
	unsigned long scenario;
	unsigned long actor_data;
	unsigned long prop_data;
	unsigned long object_header_data;
	unsigned long encounter_data;
	unsigned long squad_array;
	unsigned long platoon_array;
	unsigned long pursuit_data;
	unsigned long player_data;
	unsigned long ai_globals;
	unsigned long encounters_updated_meter;
	unsigned long structure;
	unsigned long breakable_surface_flags;
	long structure_bsp_index;
	unsigned long conversation_data;
	unsigned long combined_pvs;
	long structure_cluster_count;
	unsigned long activation_flags;
	long communication[6];
	unsigned long communication_table_indices;
	unsigned long dialogue_table;
	unsigned long dialogue_events;
	unsigned long communication_selected;
	unsigned long communication_flags;
	/* the step's own arguments (a struct; 0 for none) */
	unsigned long arguments;
};

/* crates/halo_map's network_apply::Arguments: a client's applying of the
host's state, its arguments (by value) and result */
struct halo_sim_network_apply
{
	long object_index;
	float position[3];
	float forward[3];
	float up[3];
	float velocity[3];
	float angular_velocity[3];
	long has_velocity;
	long has_angular_velocity;
	float tolerance;
	float blend_distance;
	float maximum_speed;
	float maximum_fall_speed;
	/* (distributed_apply_state's: network_distributed.c's struct
	distributed_unit_state) */
	unsigned char state[64];
	/* network_distributed.c's corrections count, 0 for none */
	unsigned long corrections;
	/* the local players' noted positions (0 for none), the smoothed round
	trip, and (distributed_correct_own_unit's) the local player's index */
	unsigned long own_positions;
	unsigned long round_trip;
	long local_player_index;
	/* distributed_handle_unit_state's: the player's death and seat
	disagreement (0 for one not tracked), and its datum */
	unsigned long death;
	unsigned long seat_disagreement;
	unsigned long player;
	/* network_objects.c's: the local players' noted vehicles (0 for none),
	and the object's entry of the dead bipeds (0 for none) */
	unsigned long own_vehicles;
	unsigned long dead;
	/* the local players' own inventories (0 for none), and the loaded tags'
	table and count */
	unsigned long own_inventories;
	unsigned long tag_instances;
	long tag_count;
	/* an object change's: the change, its entry of the objects the client
	has, and the objects' statistics */
	unsigned long change;
	unsigned long has;
	unsigned long statistics;
	long result;
};

typedef char halo_sim_network_apply_size_assert[sizeof(struct halo_sim_network_apply) == 216 ? 1 : -1];

/* network_objects.c's calls (crates/halo_map's network_apply::call) */
enum
{
	_network_apply_call_set_position = 1,
	_network_apply_call_interpolation,
	_network_apply_call_coop_state,
	_network_apply_call_leaf,
	_network_apply_call_client_has,
	_network_apply_call_detach_unit,
	_network_apply_call_attach_unit,
	_network_apply_call_unit_kill,
	_network_apply_call_unit_kill_no_statistics,
	_network_apply_call_set_seat,
	_network_apply_call_set_damage,
	_network_apply_call_teleporter,
	_network_apply_call_unit_kill_silent,
	_network_apply_call_damage_update,
	_network_apply_call_reconnect,
	_network_apply_call_set_visibility,
	_network_apply_call_drop_weapon,
	_network_apply_call_release_item,
	_network_apply_call_add_weapon,
	_network_apply_call_ctf_game,
	_network_apply_call_create,
	_network_apply_call_create_failed,
	_network_apply_call_delete_immediately,
	_network_apply_call_release_item_deleting,
	_network_apply_call_exit_seat_end,
	_network_apply_call_set_garbage,
	_network_apply_call_disconnect,
	/* (the client's tick) the message in network_objects.c's
	objects_client_message, its type b & 0xff, count (b >> 8) & 0xff and size
	b >> 16, where a says (distributed_send) */
	_network_apply_call_client_send,
	/* player_control_set_desired_weapon(a, b) */
	_network_apply_call_set_desired_weapon,
	/* distributed_client_send_identity() */
	_network_apply_call_send_identity,
};

/* crates/halo_map's network_apply::ClientTickArguments: a client's tick of
the host's objects: its records (network_objects.c and
network_objects_records.h), the message it sends, the messages' types and
destinations, and its local players as the tick began */
struct halo_sim_network_client_tick
{
	unsigned long state;
	unsigned long new_objects;
	unsigned long has;
	unsigned long picked_up_weapons;
	unsigned long own_vehicles;
	unsigned long own_inventories;
	unsigned long statistics;
	unsigned long message;
	short ready_type;
	short prediction_type;
	short to_host;
	short to_host_reliably;
	long local_players[4];
};

typedef char halo_sim_network_client_tick_size_assert[sizeof(struct halo_sim_network_client_tick) == 8 * 4 + 4 * 2 + 16 ? 1 : -1];

/* network_distributed.c's smoothed round trip */
unsigned long network_distributed_round_trip_address(void);

/* crates/halo_map's network_damage::Arguments: a client's hit report the
host checks, the host's records of its player, and the damage it deals */
struct halo_sim_network_report
{
	unsigned char report[0x6C];
	long machine_index;
	unsigned long player_record;
	unsigned long explosions;
	unsigned long trail;
	unsigned long history;
	unsigned long game_globals;
	float gravity;
	unsigned long tag_instances;
	long tag_count;
	unsigned char damage[0x54];
	unsigned long rejected;
	unsigned long dealt;
};

typedef char halo_sim_network_report_size_assert[sizeof(struct halo_sim_network_report) == 0x6C + 36 + 0x54 + 8 ? 1 : -1];

/* network_damage.c's calls (crates/halo_map's network_damage::call) */
enum
{
	_network_damage_call_machine_has_player = 1,
	_network_damage_call_maximum_body_vitality,
	_network_damage_call_has_node,
	_network_damage_call_round_trip,
	_network_damage_call_origin,
	_network_damage_call_velocity,
	_network_damage_call_location,
	_network_damage_call_deal,
	_network_damage_call_unarmed_melee_damage,
	_network_damage_call_player_machine,
	_network_damage_call_replay_player_effect,
	_network_damage_call_replay_aftermath,
	_network_damage_call_set_death,
	_network_damage_call_replay_kill,
	_network_damage_call_telefragged,
	_network_damage_call_replay_static,
	_network_damage_call_get_death,
	_network_damage_call_score,
	_network_damage_call_client_has,
};

/* crates/halo_map's network_damage::HookArguments: network_damage.c's other
steps (the host's bookkeeping, a client's replay of an event, and damage.c's
hooks), and the records they keep */
struct halo_sim_network_damage_hook
{
	unsigned char entry[0x6C];
	unsigned char damage[0x54];
	unsigned long damage_address;
	long object_index;
	short node_index;
	short region_index;
	short material_index;
	short has_normal;
	float normal[3];
	long authorized;
	unsigned long being_damaged_flags;
	float shield_damage;
	float body_damage;
	float body_damage_multiplier;
	float total_damage;
	short body_part;
	short pad;
	long player_index;
	long connection;
	long host_time;
	long dealing_report;
	unsigned long players;
	unsigned long history;
	unsigned long trails;
	unsigned long events;
	unsigned long event_count;
	unsigned long destinations;
	unsigned long reports;
	unsigned long report_count;
	unsigned long replayed;
	unsigned long game_globals;
	unsigned long tag_instances;
	long tag_count;
	long result;
};

typedef char halo_sim_network_damage_hook_size_assert[
	sizeof(struct halo_sim_network_damage_hook) == 0x6C + 0x54 + 8 + 8 + 12 + 24 + 4 + 16 + 48 + 4 ? 1 : -1];

void network_damage_call(struct halo_sim_ai_call *call);

/* crates/halo_map's network_host::Arguments: the host's objects sent a
tick, its records (network_objects.c and network_objects_records.h), the
messages' types and how many entries each takes, the client machines and
their players as the tick began, and whether a co-op cutscene films through
the host's camera */
struct halo_sim_network_host
{
	unsigned long told;
	unsigned long told_count;
	unsigned long resting_cursor;
	unsigned long state_moving;
	unsigned long rest_times;
	unsigned long inventories;
	unsigned long viewers;
	unsigned long vehicle_predictions;
	unsigned long statistics;
	unsigned long damage_animations;
	unsigned long damage_animation_count;
	/* the message being sent (network_objects.c's objects_host_message) */
	unsigned long message;
	short change_type;
	short state_type;
	short inventory_type;
	short damage_animation_type;
	short change_limit;
	short state_limit;
	short inventory_limit;
	short to_clients;
	short to_clients_reliably;
	short pad;
	long machine_count;
	long machine_indices[128];
	long player_lists[128][4];
	long coop_cinematic;
	float camera[3];
	/* (a client's asking for the objects, network_objects_client_asked)
	the machines told all (network_objects_records.h), the word that that
	is all, and the machine asking, whether again, and its players */
	unsigned long host_machines;
	short synchronized_type;
	short pad2;
	long asked_machine;
	long asked_again;
	long asked_players[4];
};

typedef char halo_sim_network_host_size_assert[sizeof(struct halo_sim_network_host) ==
	12 * 4 + 10 * 2 + 4 + 128 * 4 + 128 * 16 + 16 + 4 + 4 + 8 + 16 ? 1 : -1];

/* network_objects.c's host calls (crates/halo_map's network_host::call) */
enum
{
	/* the message, of its type b & 0xff, count (b >> 8) & 0xff and size
	b >> 16: to the machine a (reliably if actor), or with a NONE where
	actor says */
	_network_host_call_send = 1,
	_network_host_call_origin,
	_network_host_call_machine_has_player,
	_network_host_call_coop_has_structure_bsp,
	_network_host_call_round_trip,
	_network_host_call_vehicle_definition,
	_network_host_call_reconcile,
};

/* crates/halo_map's network_host::VehicleArguments: the host taking its
clients' predictions of the vehicles they drive: the records
(network_objects_records.h), the gravity, and the prediction being taken
(network_objects_reconcile's, network_host_call's) */
struct halo_sim_network_vehicle_predictions
{
	unsigned long predictions;
	float gravity;
	float position[3];
	float forward[3];
	float up[3];
	float velocity[3];
	float angular_velocity[3];
};

typedef char halo_sim_network_vehicle_predictions_size_assert[
	sizeof(struct halo_sim_network_vehicle_predictions) == 8 + 15 * 4 ? 1 : -1];

void network_host_call(struct halo_sim_ai_call *call);

/* crates/halo_map's network_host::PlanArguments: the host's plan of what
each client is sent of each player a tick: its records
(network_distributed.c, network_distributed_records.h), the update the
tick ran and how many players' input it has (NONE: none), the map's cluster
count, and the client machines and their players as the tick began */
struct halo_sim_network_plan
{
	unsigned long sends;
	unsigned long seen;
	unsigned long viewers;
	unsigned long sent_units;
	unsigned long states;
	unsigned long player_machines;
	unsigned long update_number;
	long update_number_value;
	long recent_count;
	long cluster_count;
	long machine_count;
	long machine_indices[128];
	long player_lists[128][4];
};

typedef char halo_sim_network_plan_size_assert[sizeof(struct halo_sim_network_plan) == 11 * 4 + 128 * 4 + 128 * 16 ? 1 : -1];

/* network_distributed.c's plan calls (crates/halo_map's network_host::plan_call) */
enum
{
	/* distributed_state_from_player(a) into the plan's states */
	_network_plan_call_state = 1,
	/* object_get_origin(a), output[0..3] */
	_network_plan_call_origin,
	/* distributed_relayed_action_from(a) into the plan's actions: whether
	it changed */
	_network_plan_call_action,
	/* whether the cluster a sees the cluster b (the map's potentially
	visible set) */
	_network_plan_call_sees,
	/* the host's packer begun for the machine a's messages of type b,
	with the update number first if actor */
	_network_plan_call_packer_begin,
	/* the send's entry of b bytes into the packer */
	_network_plan_call_packer_add,
	/* the packer's message so far into the batch */
	_network_plan_call_packer_flush,
};

/* crates/halo_map's network_host::SendArguments: the host's sending of what
it planned each client is sent of each player: the plan's records, the
messages' types, the client machines, and the entry being packed */
struct halo_sim_network_send_players
{
	unsigned long sends;
	unsigned long states;
	unsigned long actions;
	unsigned long player_machines;
	short relayed_type;
	short unit_type;
	long machine_count;
	long machine_indices[128];
	unsigned char entry[64];
};

typedef char halo_sim_network_send_players_size_assert[
	sizeof(struct halo_sim_network_send_players) == 4 * 4 + 4 + 4 + 128 * 4 + 64 ? 1 : -1];

void network_plan_call(struct halo_sim_ai_call *call);

/* crates/halo_map's network_messages::Arguments: a message of the netcode
handled: its sender (NONE: the host), where it is and its size, whether a
game is on and this machine's connection, the records it writes, the
message kinds' numbers, and each kind's least entry and what it is
(_network_message_kind_*); and a batch's message being handled */
struct halo_sim_network_message
{
	long machine_index;
	unsigned long message;
	long size;
	long in_progress;
	long connection;
	long server_connection;
	long client_connection;
	unsigned long host_time;
	unsigned long received;
	unsigned long handling_batch;
	unsigned long received_times;
	short batch_type;
	short message_types;
	short damage_events_type;
	short player_prediction_type;
	short vehicle_prediction_type;
	short message_header_size;
	short senders;
	short host_sender;
	short buffer_size;
	short pad;
	short entry_sizes[96];
	unsigned char kinds[96];
	unsigned char buffer[1204];
};

typedef char halo_sim_network_message_size_assert[sizeof(struct halo_sim_network_message) ==
	11 * 4 + 10 * 2 + 96 * 2 + 96 + 1204 ? 1 : -1];

enum
{
	_network_message_kind_from_client = 1 << 0,
	_network_message_kind_needs_entry = 1 << 1,
	_network_message_kind_stale = 1 << 2,
};

/* network_distributed.c's message calls (crates/halo_map's
network_messages::call): b names the message (0 the one handled, 1 the
batch's in the buffer), actor its size */
enum
{
	/* network_voice_handles_message, and if so handled: the result */
	_network_message_call_voice = 1,
	/* distributed_note_client_clock(a, b): whether its game runs fast */
	_network_message_call_clock,
	/* the message to its handler, from the machine a */
	_network_message_call_dispatch,
};

void network_message_call(struct halo_sim_ai_call *call);

/* crates/halo_map's network_host::PredictionArguments: the host taking its
clients' predictions of their players on foot: the records
(network_distributed_records.h) and the gravity */
struct halo_sim_network_predictions
{
	unsigned long predictions;
	unsigned long accepted;
	unsigned long host_speeds;
	float gravity;
};

/* network_distributed.c's prediction calls (crates/halo_map's
network_host::prediction_call) */
enum
{
	/* distributed_machine_round_trip_ticks of the player a's machine,
	output[0] */
	_network_predictions_call_round_trip = 1,
	/* network_coop_player_has_structure_bsp(a) */
	_network_predictions_call_coop_has_structure_bsp,
	/* distributed_apply_state of the unit a to the player b's prediction,
	its speeds input[0..2]: whether taken */
	_network_predictions_call_apply_state,
	/* the game globals' player speeds forward, back, sideways and the
	double speed multiplier, output[0..4]; the result 1 if there are any,
	and 2 if players are double speed */
	_network_predictions_call_player_speeds,
	/* the biped tag a's jump velocity and standing and crouching heights,
	output[0..3] */
	_network_predictions_call_biped_definition,
};

void network_predictions_call(struct halo_sim_ai_call *call);

/* crates/halo_map's network_input::Arguments: the host taking a client's
players' input (the inputs at data, count of them, from the machine), or a
client the host's relayed input (its bytes data to end, count entries);
the records (network_distributed_records.h), whether co-op is on, and the
action (and its ticks' buttons) handed on */
struct halo_sim_network_input
{
	long machine_index;
	unsigned long data;
	unsigned long end;
	long count;
	unsigned long facings;
	unsigned long round_trips;
	long coop_active;
	unsigned char action[0x20];
	unsigned short control_flags[4];
};

typedef char halo_sim_network_input_size_assert[sizeof(struct halo_sim_network_input) == 7 * 4 + 0x20 + 8 ? 1 : -1];

/* network_distributed.c's input calls (crates/halo_map's network_input::call) */
enum
{
	/* distributed_machine_has_player(a, b) */
	_network_input_call_machine_has_player = 1,
	/* player_control_action_test_note(a, the turn input[0..2], the throttle
	input[2..4], the trigger input[4]) */
	_network_input_call_note_actions,
	/* network_coop_note_player_structure_bsp(a, b) */
	_network_input_call_note_structure_bsp,
	/* update_server_handle_distributed_input of the player a, the input b's
	tick and buttons, the arguments' action */
	_network_input_call_hand_on,
	/* update_client_handle_relayed_action of the player a, the update b,
	the arguments' action and buttons */
	_network_input_call_relayed,
};

void network_input_call(struct halo_sim_ai_call *call);

/* crates/halo_map's network_host::SendsArguments: the host's periodic sends
(statistics, pings, pickups): their records (network_distributed.c), the
players' machines, the message, its kinds and how many entries each takes */
struct halo_sim_network_sends
{
	unsigned long sent_statistics;
	unsigned long statistics_cursor;
	unsigned long statistics_due;
	unsigned long pickups;
	unsigned long pickup_count;
	unsigned long player_machines;
	unsigned long message;
	short statistics_type;
	short pings_type;
	short pickups_type;
	short statistics_limit;
	short pings_limit;
	short to_clients;
};

/* network_distributed.c's sends' calls (crates/halo_map's network_host::sends_call) */
enum
{
	/* the message, of its type b & 0xff, count (b >> 8) & 0xff and size
	b >> 16: to every client as actor says (a NONE), or to the machine a
	reliably */
	_network_sends_call_send = 1,
	/* distributed_player_ping(a) */
	_network_sends_call_ping,
};

void network_sends_call(struct halo_sim_ai_call *call);

/* crates/halo_map's network_input::ClientSendsArguments: a client's sends
each tick: the noted positions, the host's latest tick it had, its
structure BSP, the tests' lie's period (HALO_NETWORK_TEST_LIE), its local
players, the messages' kinds; and the local input, the player's state, the
entry packed and the inputs' message the calls pass */
struct halo_sim_network_client_sends
{
	unsigned long own_positions;
	long host_time;
	long structure_bsp_index;
	long lie_ticks;
	long local_players[4];
	short inputs_type;
	short predictions_type;
	short to_host;
	short host_sender;
	long tick;
	unsigned char action[0x20];
	unsigned short control_flags[4];
	unsigned char state[64];
	unsigned char entry[64];
	unsigned char message[8 + 4 * 52];
};

typedef char halo_sim_network_client_sends_size_assert[
	sizeof(struct halo_sim_network_client_sends) == 4 * 4 + 16 + 8 + 4 + 0x20 + 8 + 64 + 64 + 216 ? 1 : -1];

/* network_distributed.c's client sends' calls (crates/halo_map's
network_input::sends_call) */
enum
{
	/* update_client_distributed_input(a) into the arguments' tick, action
	and buttons: whether there is one */
	_network_client_sends_call_local_input = 1,
	/* the arguments' message, its type b & 0xff, count (b >> 8) & 0xff and
	size b >> 16, where actor says */
	_network_client_sends_call_send,
	/* distributed_state_from_player(a) into the arguments' state */
	_network_client_sends_call_state,
	/* the packer begun for the sender a's messages of type b */
	_network_client_sends_call_packer_begin,
	/* the arguments' entry, of b bytes, into the packer */
	_network_client_sends_call_packer_add,
	/* the packer's message so far into the batch */
	_network_client_sends_call_packer_flush,
};

void network_client_sends_call(struct halo_sim_ai_call *call);

/* crates/halo_map's network_messages::ClockArguments: the host's check of a
client machine's clock, by the tick its message was stamped with: the
machine, the tick, the time (milliseconds), whether the message came over
its stream, and the clocks (network_distributed_records.h) */
struct halo_sim_network_clock
{
	long machine_index;
	long tick;
	unsigned long now;
	long stream;
	unsigned long clocks;
};

/* network_distributed.c's clock calls (crates/halo_map's
network_messages::clock_call): of the machine a, its rate input[0] and its
ticks ahead b */
enum
{
	/* its game runs fast: logged */
	_network_clock_call_log_fast = 1,
	/* ... for long, its stream says so too: told, logged a cheater, dropped */
	_network_clock_call_drop,
	/* ... its datagrams alone say so: logged, unverified */
	_network_clock_call_log_unverified,
};

void network_clock_call(struct halo_sim_ai_call *call);

/* crates/halo_map's network_coop::WorldArguments: co-op's host sending the
world's changes each tick: whether it resends what ever changed, its records
(network_coop_records.h), the message and its kinds */
struct halo_sim_network_coop_world
{
	long refresh;
	unsigned long sent_transforms;
	unsigned long sent_devices;
	unsigned long sent_looks;
	unsigned long message;
	short transforms_type;
	short device_states_type;
	short looks_type;
	short pad;
};

/* network_coop.c's world calls (crates/halo_map's network_coop::call) */
enum
{
	/* the message, of the type a, b entries of actor bytes, to every client */
	_network_coop_world_call_send = 1,
	/* the device groups' message, of b entries, to every client */
	_network_coop_world_call_send_device_groups,
	/* the events' message, of the first b events queued, to every client */
	_network_coop_world_call_send_events,
	/* whether the looping sound (a, a tag) is no longer playing: result */
	_network_coop_world_call_looping_sound_stopped,
	/* whether the breakable surface a of the structure is broken: result,
	and its centroid: output[0..2] */
	_network_coop_world_call_surface_broken,
	/* logs that the event queue is full (an event of kind a not sent) */
	_network_coop_world_call_log_events_full,
	/* the object names' message to every client */
	_network_coop_world_call_send_object_names,
	/* the screen effect's message to every client */
	_network_coop_world_call_send_screen_effect,
	/* logs that the cutscene can be skipped */
	_network_coop_world_call_log_skippable,
	/* skips the cinematic, a of b machines voting (main_skip_cinematic) */
	_network_coop_world_call_skip_cinematic,
	/* the presentation's message to every client */
	_network_coop_world_call_send_presentation,
	/* logs that a machine joined */
	_network_coop_world_call_log_joined,
	/* sets the unit's (a) maximum vitality to the players' (object_initialize_vitality) */
	_network_coop_world_call_initialize_vitality,
};

/* network_coop.c's client world calls (crates/halo_map's network_coop) */
enum
{
	/* the device (actor) set to the power input[0], and moved to the
	position input[1] if a (device_port_set_state) */
	_network_coop_client_call_device_state = 1,
	/* the object (actor) placed at input[0..2] facing input[3..5] with up
	input[6..8] (object_set_position) */
	_network_coop_client_call_set_position,
	/* the object's (actor) region a's permutation set to b */
	_network_coop_client_call_set_permutation,
	/* the object (actor) scaled to input[0] (objects_scripting_set_scale) */
	_network_coop_client_call_set_scale,
	/* the device group (actor) set to input[0] with the flags a, jumping
	there if b (device_group_network_set) */
	_network_coop_client_call_set_device_group,
	/* the object of the scenario's name actor made (object_new_by_name) */
	_network_coop_client_call_create_named,
	/* the object (actor) deleted (object_delete) */
	_network_coop_client_call_delete,
};

/* crates/halo_map's network_coop::ClientNamesArguments: a co-op client
matching its host's named objects: the host's names (struct
distributed_coop_object_names), the differences seen last time (a bit per
name), the game's object of each name, the scenario's names (struct
scenario_object_name) and their count, and the structure BSP */
struct halo_sim_network_coop_client_names
{
	unsigned long names;
	unsigned long differing;
	unsigned long object_name_list;
	unsigned long scenario_names;
	long scenario_name_count;
	long structure_bsp_index;
};

/* crates/halo_map's network_coop::ClientDevicesArguments: a co-op client
applying its host's device groups: the count and the entries
(network_coop_records.h's), the snap counts seen (struct
coop_client_devices), the game's device groups (a data array), the
scenario's count of them, the flags a group's network state carries and the
runtime bit, and the game's object of each name */
struct halo_sim_network_coop_client_devices
{
	long count;
	unsigned long entries;
	unsigned long client_devices;
	unsigned long device_groups;
	long scenario_group_count;
	long network_flags;
	long runtime_bit;
	unsigned long object_name_list;
};

/* which of the host's world changes a client applies */
enum
{
	_coop_client_world_device_states,
	_coop_client_world_object_transforms,
	_coop_client_world_object_looks,
};

void network_coop_client_call(struct halo_sim_ai_call *call);

/* crates/halo_map's network_coop::ClientWorldArguments: a co-op client
applying its host's world changes: which (_coop_client_world_*), the count
and the entries (network_coop_records.h's), the game's object of each name
(object_name_list_port_get) and the loaded tags (cache_files_tag_instances) */
struct halo_sim_network_coop_client_world
{
	short kind;
	short count;
	unsigned long entries;
	unsigned long object_name_list;
	unsigned long tag_instances;
	long tag_count;
};

/* coop_enemies.c's update's calls (crates/halo_map's coop_enemies::call) */
enum
{
	/* free ground about input[0..2] for the a'th enemy
	(coop_enemies_spread_position): result, and output[0..2] */
	_coop_enemies_call_spread_position = 1,
	/* the vehicle's (a) seat's (b) marker: result, and output[0..2] */
	_coop_enemies_call_seat_marker,
	/* places a kept rider of the variant a, the encounter b and the squad
	actor at input[0..2] facing input[3] (actor_place) */
	_coop_enemies_call_place_rider,
};

void coop_enemies_world_call(struct halo_sim_ai_call *call);

/* crates/halo_map's coop_enemies::Arguments: co-op's host placing the
riders kept for want of a dropship's seat: the state (struct
coop_enemies_state, coop_enemies_records.h) */
struct halo_sim_coop_enemies
{
	unsigned long state;
};

/* crates/halo_map's network_coop::ResendArguments: co-op's host noticing a
machine joining (and so resending) and keeping its players' units at the
scripts' vitality: the machines it has (struct coop_resend_inputs), the
resend's records (struct coop_host_resend) and the players' vitality (struct
coop_players_vitality) */
struct halo_sim_network_coop_resend
{
	unsigned long inputs;
	unsigned long resend;
	unsigned long vitality;
};

/* crates/halo_map's network_coop::PresentationArguments: co-op's host
counting the skip vote and sending its presentation: what they read of the
game (struct coop_presentation_inputs), the skip vote (struct coop_skip_vote),
the message (network_coop_records.h) and the game time */
struct halo_sim_network_coop_presentation
{
	unsigned long inputs;
	unsigned long skip_vote;
	unsigned long message;
	long now;
};

/* crates/halo_map's network_coop::ObjectNamesArguments: co-op's host
sending which scenario object names it has: the game's object of each name
(object_name_list_port_get), the scenario's count of names, the structure BSP
and the message (network_coop_records.h) */
struct halo_sim_network_coop_object_names
{
	unsigned long object_name_list;
	long name_count;
	long structure_bsp_index;
	unsigned long message;
};

/* crates/halo_map's network_coop::ScreenEffectArguments: co-op's host
sending the scripts' screen effect: whether to resend, the state the
rasterizer has, the state last sent, the message (network_coop_records.h) and
the state's size */
struct halo_sim_network_coop_screen_effect
{
	long refresh;
	unsigned long state;
	unsigned long sent;
	unsigned long message;
	long state_size;
};

/* crates/halo_map's network_coop::QueueArguments: co-op's host resending
the scripts' state and sending its events: whether to resend and whether a
machine joined, the events (struct coop_events) and the host's records
(struct coop_host_queue), the structure BSP and its count of breakable
surfaces */
struct halo_sim_network_coop_queue
{
	long refresh;
	long joined;
	unsigned long events;
	unsigned long queue;
	long structure_bsp_index;
	long surface_count;
};

/* crates/halo_map's network_coop::DeviceGroupsArguments: co-op's host
sending its device groups: whether a machine joined, the groups as last sent
(network_coop_records.h), the game's device groups (a data array), the
scenario's count, and the groups' flags a client takes and the runtime bit */
struct halo_sim_network_coop_device_groups
{
	long joined;
	unsigned long host_devices;
	unsigned long device_groups;
	long scenario_group_count;
	long network_flags;
	long runtime_bit;
};

void network_coop_world_call(struct halo_sim_ai_call *call);

/* network_distributed.c's corrections count */
unsigned long network_distributed_corrections_address(void);

/* network_objects.c's: the calls the applying's steps share */
long network_apply_coop_state(void);
long network_apply_leaf(void const *point);
long network_apply_simple(long kind, long a, long b, float const *input, short input_count);

/* network_objects.c's: the applying's call function, and its world (the
object's datum, the arguments, and the corrections count); returns the
regions' count */
void network_apply_call(struct halo_sim_ai_call *call);
long network_apply_world(struct halo_sim_world_context *context, struct halo_sim_network_apply *arguments,
	struct halo_sim_world_region *regions);

/* the world step's update in Rust when asked (returns TRUE, done), or the
game's (FALSE; in check mode `checking`, the game's then runs, its calls
going through halo_sim_world_event_begin/end, and halo_sim_world_step_check
compares) */
int halo_sim_world_step_begin(long step, struct halo_sim_world_context const *context,
	struct halo_sim_world_region const *regions, long region_count, void (*call)(struct halo_sim_ai_call *call),
	int *checking);
int halo_sim_world_event_begin(struct halo_sim_ai_call const *call);
void halo_sim_world_event_end(struct halo_sim_ai_call const *call, int opened);
void halo_sim_world_step_check(long step);

/* the encounters' update's calls (encounters.c's encounter_call;
crates/halo_map's encounters::call) */
enum
{
	_encounter_call_test_activation = 1,
	_encounter_call_stand_down,
	_encounter_call_post_combat,
	_encounter_call_team_is_ally,
	_encounter_call_spawn_actor,
	_encounter_call_random_range,
	_encounter_call_squad_timer_expire, /* now Rust's but its magic sight */
	_encounter_call_follow_target_count,
	_encounter_call_follow_target,
	_encounter_call_object_origin,
	_encounter_call_actor_change_encounter,
	_encounter_call_stimulus_maneuvering,
	_encounter_call_player_rating,
	_encounter_call_random_real,
	_encounter_call_communication_team,
	_encounter_call_prop_active_by_unit,
	_encounter_call_no_corpse_shooting,
	_encounter_call_clear_pursuit,
	_encounter_call_drop_prop,
	_encounter_call_set_actor_active,
	_encounter_call_verify_activation,
	_encounter_call_swarm_in_pvs,
	_encounter_call_squad_magic_sight,
};

void encounter_call(struct halo_sim_ai_call *call);

/* the conversations' update's calls (ai_communication.c's conversation_call;
crates/halo_map's conversations::call) */
enum
{
	_conversation_call_begin = 1,
	_conversation_call_finish,
	_conversation_call_scripted_dialog_playing,
	_conversation_call_scripted_sound_new,
	_conversation_call_test_speech,
	_conversation_call_speak,
	_conversation_call_scripted_sound_time,
};

void conversation_call(struct halo_sim_ai_call *call);

/* a communication event's calls (ai_communication.c's communication_call;
crates/halo_map's communication::call); several pass integers in `input` as
their bits */
enum
{
	_communication_call_team_is_ally = 1,
	_communication_call_team_is_enemy,
	_communication_call_actor_type_race,
	_communication_call_find_global_actor,
	_communication_call_find_specific_actor,
	_communication_call_incident,
	_communication_call_scripted_dialog_playing,
	_communication_call_find_reply_actor,
	_communication_call_player_rating,
	_communication_call_communication_team,
	_communication_call_consider_speech,
	_communication_call_test_animation_impulse,
	_communication_call_action_class,
	_communication_call_random_real,
	_communication_call_reply,
	_communication_call_speak,
};

void communication_call(struct halo_sim_ai_call *call);
short actor_firing_position_port_change(long actor_index, short firing_position_index, long previous_owner,
	int cached_path_available);
short actor_firing_position_port_change_to_active_selection(long actor_index, short evaluation_mode);
short actor_firing_position_port_change_to_guard_selection(long actor_index);
short actor_firing_position_port_select_pursuit_position(long actor_index, int tenacious, long last_perceived_time);
struct path_state *actor_firing_position_port_area_path_state(void);
int actions_port_ported(short action, long function);
void actions_port_call(struct halo_sim_ai_call *call);
void actions_port_handler_call(struct halo_sim_ai_call *call);
void action_wait_call(struct halo_sim_ai_call *call);
void action_alert_call(struct halo_sim_ai_call *call);
void action_search_call(struct halo_sim_ai_call *call);
void action_uncover_call(struct halo_sim_ai_call *call);
void action_guard_call(struct halo_sim_ai_call *call);
void action_flee_call(struct halo_sim_ai_call *call);
void action_converse_call(struct halo_sim_ai_call *call);
void action_vehicle_call(struct halo_sim_ai_call *call);
void action_charge_call(struct halo_sim_ai_call *call);
void action_obey_call(struct halo_sim_ai_call *call);
void action_avoid_call(struct halo_sim_ai_call *call);
void action_fight_call(struct halo_sim_ai_call *call);
long actors_port_step_props(long actor_index, unsigned long *addresses, long maximum);

#endif // __HALO_SIM_H
