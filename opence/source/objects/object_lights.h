/*
OBJECT_LIGHTS.H

header included in hcex build.
*/

#ifndef __OBJECT_LIGHTS_H
#define __OBJECT_LIGHTS_H
#pragma once

/* ---------- constants */

/* ---------- macros */

/* ---------- structures */

struct render_lighting;

/* port: a light lit this frame (lights_port_list) */
struct lights_port_light
{
	real_point3d position;
	real_vector3d forward;
	/* and its up, which turns a gel round the forward */
	real_vector3d up;
	real_rgb_color color;
	real radius;
	/* a spotlight's (the flashlight's) cutoff angle, radians; pi or more
	for a light shining every way */
	real cutoff_angle;
	/* and the angle its light falls off from, to none at the cutoff */
	real falloff_angle;
	/* its light tag, that tag's flags, and the object it is attached to
	(NONE for none) */
	long definition_index;
	long definition_flags;
	long object_index;
};

/* ---------- prototypes/OBJECT_LIGHTS.C */

void lights_initialize(void);
void lights_dispose(void);
void lights_initialize_for_new_map(void);
void lights_dispose_from_old_map(void);
/* port: the lights made again, empty, if their array is out of order (object_lights.c) */
boolean lights_port_recover(void);
boolean lights_enable(boolean enable);
long light_new(long definition_index, long object_index, short object_attachment_index, short object_function_index, short object_change_color_index);
long light_new_unattached(
	long definition_index,
	long object_index,
	short object_node_index,
	real_point3d const *position,
	real_vector3d const *direction,
	real scale);
void light_delete(long light_index);
void lights_queue_lens_flare(
	long lens_flare_definition_index,
	real_point3d const *position,
	real_vector3d const *direction,
	real_vector3d const *up,
	real_rgb_color const *color,
	real scale);
void light_particle(
	real_point3d const *position,
	real_rgb_color *light,
	real_rgb_color *diffuse,
	boolean block);

real object_get_self_illumination(long object_index);
void lights_illumination_at_point(union real_point3d const *point, struct location const *location, union real_rgb_color *color);

void light_disconnect_from_map(long light_index);
void light_reconnect_to_map(long light_index);
void lights_disconnect_from_structure_bsp(
	void);
void lights_reconnect_to_structure_bsp(
	void);
void lights_prepare_for_object_dynamic(
	long object_index,
	struct render_lighting *lighting);
/* port: the dynamic lights lit by the last lights_preprocess_scene (muzzle
flashes, plasma, explosions, the flashlight), at most `maximum`, for a Bevy
renderer (port/linux/game/bevy_bridge.c) */
short lights_port_list(
	struct lights_port_light *lights,
	short maximum);

/* ---------- globals */

/* ---------- public code */

#endif // __OBJECT_LIGHTS_H
