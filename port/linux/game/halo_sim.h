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

#endif // __HALO_SIM_H
