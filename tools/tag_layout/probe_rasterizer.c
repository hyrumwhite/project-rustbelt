/*
Layouts the Xbox rasterizer defines for itself (see probe.c): the
environment shader's specular and reflection properties, which the port's
tag schema leaves as reserved space.
*/

#include "cseries.h"
#include "rasterizer/xbox/rasterizer_xbox_environment.c"

#include <stddef.h>

#define LAYOUT_OFFSET(s, f) const int layout__##s##__##f = offsetof(struct s, f);
#define LAYOUT_SIZE(s) const int layout__##s##__SIZEOF = sizeof(struct s);

LAYOUT_OFFSET(shader_environment_properties, specular)
LAYOUT_OFFSET(shader_environment_properties, reflection)
LAYOUT_OFFSET(shader_environment_specular_properties, flags)
LAYOUT_OFFSET(shader_environment_specular_properties, brightness)
LAYOUT_OFFSET(shader_environment_specular_properties, view_perpendicular_color)
LAYOUT_OFFSET(shader_environment_specular_properties, view_parallel_color)
LAYOUT_OFFSET(shader_environment_reflection_properties, flags)
LAYOUT_OFFSET(shader_environment_reflection_properties, type)
LAYOUT_OFFSET(shader_environment_reflection_properties, view_perpendicular_brightness)
LAYOUT_OFFSET(shader_environment_reflection_properties, view_parallel_brightness)
LAYOUT_OFFSET(shader_environment_reflection_properties, cube_map)
