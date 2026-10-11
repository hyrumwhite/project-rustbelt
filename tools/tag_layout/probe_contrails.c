/*
Contrail layouts (see probe.c): what a contrail's update reads of its
definition, its points' states and its bitmap's sequences (contrails.c).
*/

#include "cseries.h"
#include "math/real_math.h"
#include "tag_files/tag_files.h"
#include "effects/contrail_definitions.h"
#include "bitmaps/bitmap_group.h"

#include <stddef.h>

#define LAYOUT_OFFSET(s, f) const int layout__##s##__##f = offsetof(struct s, f);
#define LAYOUT_SIZE(s) const int layout__##s##__SIZEOF = sizeof(struct s);

LAYOUT_OFFSET(contrail_definition, flags)
LAYOUT_OFFSET(contrail_definition, scale_flags)
LAYOUT_OFFSET(contrail_definition, point_generation_rate)
LAYOUT_OFFSET(contrail_definition, point_velocity_lower_bound)
LAYOUT_OFFSET(contrail_definition, point_velocity_upper_bound)
LAYOUT_OFFSET(contrail_definition, point_velocity_cone_angle)
LAYOUT_OFFSET(contrail_definition, point_inherited_velocity_fraction)
LAYOUT_OFFSET(contrail_definition, texture_animation_u)
LAYOUT_OFFSET(contrail_definition, texture_animation_v)
LAYOUT_OFFSET(contrail_definition, frames_per_second)
LAYOUT_OFFSET(contrail_definition, bitmap)
LAYOUT_OFFSET(contrail_definition, first_sequence_index)
LAYOUT_OFFSET(contrail_definition, sequence_count)
LAYOUT_OFFSET(contrail_definition, states)

LAYOUT_SIZE(contrail_point_state)
LAYOUT_OFFSET(contrail_point_state, duration_lower_bound)
LAYOUT_OFFSET(contrail_point_state, duration_upper_bound)
LAYOUT_OFFSET(contrail_point_state, transition_duration_lower_bound)
LAYOUT_OFFSET(contrail_point_state, transition_duration_upper_bound)
LAYOUT_OFFSET(contrail_point_state, physics)
LAYOUT_OFFSET(contrail_point_state, width)
LAYOUT_OFFSET(contrail_point_state, scale_flags)

LAYOUT_OFFSET(bitmap_group_sequence, bitmap_count)
