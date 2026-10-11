/*
Particle system layouts (see probe.c): what a particle system's update reads
of its definition, its types, their states and their particles' states
(particle_systems.c).
*/

#include "cseries.h"
#include "math/real_math.h"
#include "tag_files/tag_files.h"
#include "effects/particle_system_definitions.h"

#include <stddef.h>

#define LAYOUT_OFFSET(s, f) const int layout__##s##__##f = offsetof(struct s, f);
#define LAYOUT_SIZE(s) const int layout__##s##__SIZEOF = sizeof(struct s);

LAYOUT_OFFSET(particle_system_definition, system_update_point_physics)
LAYOUT_OFFSET(particle_system_definition, system_update_physics)
LAYOUT_OFFSET(particle_system_definition, types)

LAYOUT_SIZE(particle_system_type)
LAYOUT_OFFSET(particle_system_type, flags)
LAYOUT_OFFSET(particle_system_type, initial_particle_count)
LAYOUT_OFFSET(particle_system_type, variables)
LAYOUT_OFFSET(particle_system_type, initial_particle_creation_physics)
LAYOUT_OFFSET(particle_system_type, physics_constants)
LAYOUT_OFFSET(particle_system_type, type_states)
LAYOUT_OFFSET(particle_system_type, particle_states)

LAYOUT_SIZE(particle_system_physics_constant)

LAYOUT_SIZE(particle_system_type_state)
LAYOUT_OFFSET(particle_system_type_state, duration_lower_bound)
LAYOUT_OFFSET(particle_system_type_state, duration_upper_bound)
LAYOUT_OFFSET(particle_system_type_state, transition_time_lower_bound)
LAYOUT_OFFSET(particle_system_type_state, transition_time_upper_bound)
LAYOUT_OFFSET(particle_system_type_state, variables)
LAYOUT_OFFSET(particle_system_type_state, particle_creation_physics)
LAYOUT_OFFSET(particle_system_type_state, particle_update_physics)

LAYOUT_SIZE(particle_system_type_particle_state)
LAYOUT_OFFSET(particle_system_type_particle_state, duration_lower_bound)
LAYOUT_OFFSET(particle_system_type_particle_state, duration_upper_bound)
LAYOUT_OFFSET(particle_system_type_particle_state, transition_time_lower_bound)
LAYOUT_OFFSET(particle_system_type_particle_state, transition_time_upper_bound)
LAYOUT_OFFSET(particle_system_type_particle_state, scale_lower_bound)
LAYOUT_OFFSET(particle_system_type_particle_state, scale_upper_bound)
LAYOUT_OFFSET(particle_system_type_particle_state, animation_rate_lower_bound)
LAYOUT_OFFSET(particle_system_type_particle_state, animation_rate_upper_bound)
LAYOUT_OFFSET(particle_system_type_particle_state, rotation_rate_lower_bound)
LAYOUT_OFFSET(particle_system_type_particle_state, rotation_rate_upper_bound)
LAYOUT_OFFSET(particle_system_type_particle_state, color_lower_bound)
LAYOUT_OFFSET(particle_system_type_particle_state, color_upper_bound)
LAYOUT_OFFSET(particle_system_type_particle_state, variables)
LAYOUT_OFFSET(particle_system_type_particle_state, point_physics)
