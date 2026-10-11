/*
Object function layouts (see probe.c): what an object's function values and
change colours read of its definition (objects.c's
object_compute_function_values and object_compute_change_colors).
*/

#include "cseries.h"
#include "tag_files/tag_files.h"
#include "objects/object_definitions.h"

#include <stddef.h>

#define LAYOUT_OFFSET(s, f) const int layout__##s##__##f = offsetof(struct s, f);
#define LAYOUT_SIZE(s) const int layout__##s##__SIZEOF = sizeof(struct s);

LAYOUT_OFFSET(_object_definition, runtime_flags)
LAYOUT_OFFSET(_object_definition, functions)
LAYOUT_OFFSET(_object_definition, change_colors)

LAYOUT_SIZE(object_function_definition)
LAYOUT_OFFSET(object_function_definition, flags)
LAYOUT_OFFSET(object_function_definition, scale_period_by_function_index)
LAYOUT_OFFSET(object_function_definition, function_type)
LAYOUT_OFFSET(object_function_definition, scale_function_by_function_index)
LAYOUT_OFFSET(object_function_definition, wobble_function_type)
LAYOUT_OFFSET(object_function_definition, wobble_period)
LAYOUT_OFFSET(object_function_definition, wobble_magnitude)
LAYOUT_OFFSET(object_function_definition, square_wave_threshold)
LAYOUT_OFFSET(object_function_definition, step_count)
LAYOUT_OFFSET(object_function_definition, map_result_to_transition_function)
LAYOUT_OFFSET(object_function_definition, add_function_index)
LAYOUT_OFFSET(object_function_definition, scale_result_by_function_index)
LAYOUT_OFFSET(object_function_definition, bounds_mode)
LAYOUT_OFFSET(object_function_definition, lower_bound)
LAYOUT_OFFSET(object_function_definition, upper_bound)
LAYOUT_OFFSET(object_function_definition, turn_off_with_function_index)
LAYOUT_OFFSET(object_function_definition, scale_by)
LAYOUT_OFFSET(object_function_definition, runtime_reciprocal_bounds_range)
LAYOUT_OFFSET(object_function_definition, runtime_reciprocal_sawtooth_count)
LAYOUT_OFFSET(object_function_definition, runtime_reciprocal_step_count)
LAYOUT_OFFSET(object_function_definition, runtime_one_over_period)

LAYOUT_SIZE(object_change_color_definition)
LAYOUT_OFFSET(object_change_color_definition, darken_by)
LAYOUT_OFFSET(object_change_color_definition, scaled_by)
LAYOUT_OFFSET(object_change_color_definition, scale_flags)
LAYOUT_OFFSET(object_change_color_definition, color_lower_bound)
LAYOUT_OFFSET(object_change_color_definition, color_upper_bound)
