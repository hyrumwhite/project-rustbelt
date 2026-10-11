/*
Effect layouts (see probe.c): what an effect's update reads of its definition,
its events, their parts and their particles (effects.c).
*/

#include "cseries.h"
#include "math/real_math.h"
#include "tag_files/tag_files.h"
#include "effects/effect_definitions.h"

#include <stddef.h>

#define LAYOUT_OFFSET(s, f) const int layout__##s##__##f = offsetof(struct s, f);
#define LAYOUT_SIZE(s) const int layout__##s##__SIZEOF = sizeof(struct s);

/* effects.c's own, mirrored */
struct effect_particles_definition
{
	short environment;
	short disposition;
	short camera_mode;
	short unused006;
	short location_index;
	short unused00a;
	real_euler_angles2d direction;
	real_vector3d offset;
	real_vector3d runtime_direction;
	long unused02c[10];
	struct tag_reference particle;
	unsigned long flags;
	short distribution_function;
	short unused06a;
	short count_lower_bound;
	short count_upper_bound;
	real distribution_radius_lower_bound;
	real distribution_radius_upper_bound;
	long unused078[3];
	real velocity_lower_bound;
	real velocity_upper_bound;
	real velocity_cone_angle;
	real angular_velocity_lower_bound;
	real angular_velocity_upper_bound;
	long unused098[2];
	real radius_lower_bound;
	real radius_upper_bound;
	long unused0a8[2];
	real_argb_color tint_lower_bound;
	real_argb_color tint_upper_bound;
	long unused0d0[4];
	unsigned long scale_a_flags;
	unsigned long scale_b_flags;
};

struct effect_location_definition
{
	char marker_name[32];
};

LAYOUT_OFFSET(effect_definition, flags)
LAYOUT_OFFSET(effect_definition, loop_start_index)
LAYOUT_OFFSET(effect_definition, loop_stop_index)
LAYOUT_OFFSET(effect_definition, runtime_danger_radius)
LAYOUT_OFFSET(effect_definition, locations)
LAYOUT_OFFSET(effect_definition, events)

LAYOUT_SIZE(effect_location_definition)

LAYOUT_SIZE(effect_event_definition)
LAYOUT_OFFSET(effect_event_definition, skip_fraction)
LAYOUT_OFFSET(effect_event_definition, delay_lower_bound)
LAYOUT_OFFSET(effect_event_definition, delay_upper_bound)
LAYOUT_OFFSET(effect_event_definition, duration_lower_bound)
LAYOUT_OFFSET(effect_event_definition, duration_upper_bound)
LAYOUT_OFFSET(effect_event_definition, parts)
LAYOUT_OFFSET(effect_event_definition, particles)

LAYOUT_SIZE(effect_part_definition)
LAYOUT_OFFSET(effect_part_definition, environment)
LAYOUT_OFFSET(effect_part_definition, disposition)
LAYOUT_OFFSET(effect_part_definition, location_index)
LAYOUT_OFFSET(effect_part_definition, flags)
LAYOUT_OFFSET(effect_part_definition, runtime_base_class_tag)
LAYOUT_OFFSET(effect_part_definition, reference)
LAYOUT_OFFSET(effect_part_definition, velocity_lower_bound)
LAYOUT_OFFSET(effect_part_definition, velocity_upper_bound)
LAYOUT_OFFSET(effect_part_definition, velocity_cone_angle)
LAYOUT_OFFSET(effect_part_definition, angular_velocity_lower_bound)
LAYOUT_OFFSET(effect_part_definition, angular_velocity_upper_bound)
LAYOUT_OFFSET(effect_part_definition, radius_modifier_lower_bound)
LAYOUT_OFFSET(effect_part_definition, radius_modifier_upper_bound)
LAYOUT_OFFSET(effect_part_definition, scale_a_flags)
LAYOUT_OFFSET(effect_part_definition, scale_b_flags)

LAYOUT_SIZE(effect_particles_definition)
LAYOUT_OFFSET(effect_particles_definition, environment)
LAYOUT_OFFSET(effect_particles_definition, disposition)
LAYOUT_OFFSET(effect_particles_definition, camera_mode)
LAYOUT_OFFSET(effect_particles_definition, location_index)
LAYOUT_OFFSET(effect_particles_definition, offset)
LAYOUT_OFFSET(effect_particles_definition, runtime_direction)
LAYOUT_OFFSET(effect_particles_definition, particle)
LAYOUT_OFFSET(effect_particles_definition, flags)
LAYOUT_OFFSET(effect_particles_definition, distribution_function)
LAYOUT_OFFSET(effect_particles_definition, count_lower_bound)
LAYOUT_OFFSET(effect_particles_definition, count_upper_bound)
LAYOUT_OFFSET(effect_particles_definition, distribution_radius_lower_bound)
LAYOUT_OFFSET(effect_particles_definition, distribution_radius_upper_bound)
LAYOUT_OFFSET(effect_particles_definition, velocity_lower_bound)
LAYOUT_OFFSET(effect_particles_definition, velocity_upper_bound)
LAYOUT_OFFSET(effect_particles_definition, velocity_cone_angle)
LAYOUT_OFFSET(effect_particles_definition, angular_velocity_lower_bound)
LAYOUT_OFFSET(effect_particles_definition, angular_velocity_upper_bound)
LAYOUT_OFFSET(effect_particles_definition, radius_lower_bound)
LAYOUT_OFFSET(effect_particles_definition, radius_upper_bound)
LAYOUT_OFFSET(effect_particles_definition, tint_lower_bound)
LAYOUT_OFFSET(effect_particles_definition, tint_upper_bound)
LAYOUT_OFFSET(effect_particles_definition, scale_a_flags)
LAYOUT_OFFSET(effect_particles_definition, scale_b_flags)
