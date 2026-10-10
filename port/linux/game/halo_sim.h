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

#endif // __HALO_SIM_H
