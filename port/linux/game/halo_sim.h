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

#endif // __HALO_SIM_H
