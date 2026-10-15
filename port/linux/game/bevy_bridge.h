/*
BEVY_BRIDGE.H

The world, each frame, for a Bevy renderer (bevy_bridge.c).
*/

#ifndef __BEVY_BRIDGE_H
#define __BEVY_BRIDGE_H
#pragma once

struct render_window;
struct real_matrix4x3;

/* render_frame, once a frame: the windows it draws */
void bevy_bridge_export_frame(
	struct render_window const *windows,
	short window_count);

/* a model the game draws by itself, posed and coloured (its change colours,
or NULL; first_person_weapons.c): shared with the next frame */
void bevy_bridge_record_model(
	long model_index,
	struct real_matrix4x3 const *node_matrices,
	short node_count,
	real_rgb_color const *change_colors,
	real active_camouflage);

/* sound_manager.c: the first player's listener, which the renderer may
move to its own camera; TRUE if it did */
boolean bevy_bridge_listener(
	real_matrix4x3 *matrix);

/* with game.bevy_bridge_overlay on: TRUE (the renderer draws the opaque
world itself, so render_window draws it only into the depth buffer) */
boolean bevy_bridge_overlay_enabled(void);
/* with game.bevy_bridge_viewer_glass on too: TRUE (the renderer draws glass
and water itself, ray traced, so the game leaves them out of its overlay) */
boolean bevy_bridge_viewer_draws_glass(void);
/* with game.bevy_bridge_viewer_fog on too: TRUE (the renderer fogs the world
itself, from the fog each frame shares, so the game draws no environment
fog into its overlay) */
boolean bevy_bridge_viewer_draws_fog(void);
/* rasterizer_window_set_fog: the fog the first player's window is drawn
with (struct render_fog), for the frame's export */
void bevy_bridge_note_fog(void const *fog);
/* rasterizer_environment_fog_screen_share: the fog screen this frame's
window would have drawn (struct bevy_bridge_fog_screen), for its export */
void bevy_bridge_note_fog_screen(void const *screen);
/* with game.bevy_bridge_viewer_decals on too: TRUE (the renderer draws the
decals itself, from those each frame shares, so the game draws none) */
boolean bevy_bridge_viewer_draws_decals(void);
/* rasterizer_xbox_decals.c: a decal the first player's window would have
drawn, its quads' vertices as the game keeps them (struct
bevy_bridge_decal_vertex), for the frame's export */
void bevy_bridge_note_decal(long decal_index, long map, short bitmap_index, short layer,
	short blend_function, unsigned long color, unsigned long intensity,
	void const *vertices, long quad_count);

/* the overlay buffer to fill with a width x height picture
(bevy_bridge_protocol.h), or NULL; bevy_bridge_overlay_commit shares it */
unsigned char *bevy_bridge_overlay_begin(
	unsigned long width,
	unsigned long height);
void bevy_bridge_overlay_commit(void);
/* (before the commit) whether the multiply's pixels follow the picture's
(bevy_bridge_protocol.h) */
void bevy_bridge_overlay_multiply(boolean multiply);

/* projectiles.c, with game.bevy_bridge_ray_tests: each projectile_update
recorded for a renderer to replay (bevy_bridge_projectile_record) */
void bevy_bridge_projectile_begin(long projectile_index);
void bevy_bridge_projectile_collision(void);
void bevy_bridge_projectile_touched_object(void);
/* collisions.c: an object a projectile's ray test visited; projectiles.c:
what its impact damage reported, and its guided steering */
void bevy_bridge_projectile_object(long object_index);
void bevy_bridge_projectile_damage(short material_type);
void bevy_bridge_projectile_guidance(long projectile_index, long target_index, void const *target_center_of_mass);
void bevy_bridge_projectile_end(long projectile_index);

/* bipeds.c, with game.bevy_bridge_ray_tests: each biped_update_physics
recorded for a renderer to replay (bevy_bridge_biped_record); `physics` is
bipeds.c's struct biped_physics */
void bevy_bridge_biped_physics_begin(long biped_index, void const *physics, unsigned long size,
	int stand_test, float stand_height, float stand_radius);
void bevy_bridge_biped_physics_end(void const *physics, unsigned long size);
/* bipeds.c, with game.bevy_bridge_ray_tests: each biped_update_moving
recorded for a renderer to replay (bevy_bridge_biped_moving_record), with
what it asks of the rest of the game; and, during it, what the actor's jump
aim answered and whether it aborted the unit's animation */
int bevy_bridge_biped_moving_recording(void);
void bevy_bridge_biped_moving_begin(long biped_index, char state_desired, int crouching,
	float const *aiming_vector, float difficulty_infection_forms, float speed_multiplier,
	int cinematic, int double_speed, int coop_player_collisions, int actor_leaping, int recorded_animation);
void bevy_bridge_biped_moving_end(long biped_index, char state_desired, int crouching);
void bevy_bridge_biped_actor_jump(int jumped, float const *velocity);
void bevy_bridge_biped_aborted_animation(void);
/* bipeds.c: each biped_update of an unparented biped recorded up to its
animation update (bevy_bridge_biped_update_record), and the random seed as
its airborne update begins */
void bevy_bridge_biped_update_begin(long biped_index, int enhanced_animation);
void bevy_bridge_biped_update_end(long biped_index, char state_desired, int crouching);
void bevy_bridge_biped_update_airborne(void);
/* units.c: each unit_update_animation of a biped recorded
(bevy_bridge_unit_animation_record), with the side effects it had */
void bevy_bridge_unit_animation_begin(long unit_index, char state_desired, int crouching,
	short magic_base_seat_index, char const *weapon_label);
void bevy_bridge_unit_animation_end(long unit_index, short result);
void bevy_bridge_unit_animation_side_effect(unsigned long effect);
/* objects.c: a sample of object_compute_node_matrices recorded
(bevy_bridge_pose_record): the orientations before and after the object
type's preprocess, and the matrices */
void bevy_bridge_pose_begin(long object_index, void const *node_orientations, short node_count,
	short frame_index, int world_relative, void const *parent_node_matrix);
void bevy_bridge_pose_preprocessed(long object_index, void const *node_orientations);
void bevy_bridge_pose_end(long object_index, void const *node_matrices);
/* bipeds.c: a player's melee step recorded (bevy_bridge_melee_record):
begun with its weapon (NONE for none), a melee started or its damage caused
(1, 2), and ended */
void bevy_bridge_melee_begin(long biped_index, long weapon_index);
/* physics.c: a vehicle's physics_update recorded
(bevy_bridge_vehicle_physics_record): begun with what its vehicle code gave
it, each other vehicle its unit collisions consider, and ended with its
mass points */
void bevy_bridge_vehicle_physics_begin(long vehicle_index, void const *powered_mass_points, short powered_count,
	void const *magic_force, void const *magic_torque, int old_physics);
void bevy_bridge_vehicle_physics_other(long object_index);
/* vehicles.c: a vehicle_update recorded (bevy_bridge_vehicle_update_record),
to its animation */
void bevy_bridge_vehicle_update_begin(long vehicle_index);
void bevy_bridge_vehicle_update_end(long vehicle_index);
void bevy_bridge_vehicle_physics_end(long vehicle_index, void const *mass_points, short mass_point_count);
void bevy_bridge_melee_event(long biped_index, unsigned long event);
void bevy_bridge_melee_end(long biped_index);

/* collisions.c: collision_get_features_in_sphere's calls, and each object
object_get_features_in_sphere visits, during a recorded biped update */
void bevy_bridge_features_call(void);
void bevy_bridge_feature_object(long object_index);

#endif /* __BEVY_BRIDGE_H */
