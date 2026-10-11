/*
Particle layouts (see probe.c): what a particle's update reads of its
definition and its bitmap's sequences (particles.c).
*/

#include "cseries.h"
#include "math/real_math.h"
#include "tag_files/tag_files.h"
#include "effects/particles.h"
#include "bitmaps/bitmap_group.h"

#include <stddef.h>

#define LAYOUT_OFFSET(s, f) const int layout__##s##__##f = offsetof(struct s, f);
#define LAYOUT_SIZE(s) const int layout__##s##__SIZEOF = sizeof(struct s);

LAYOUT_OFFSET(particle_definition, flags)
LAYOUT_OFFSET(particle_definition, bitmap)
LAYOUT_OFFSET(particle_definition, physics)
LAYOUT_OFFSET(particle_definition, collision_material_effects)
LAYOUT_OFFSET(particle_definition, life_span_lower_bound)
LAYOUT_OFFSET(particle_definition, life_span_upper_bound)
LAYOUT_OFFSET(particle_definition, collision_effect)
LAYOUT_OFFSET(particle_definition, effect)
LAYOUT_OFFSET(particle_definition, radius_lower_bound)
LAYOUT_OFFSET(particle_definition, radius_upper_bound)
LAYOUT_OFFSET(particle_definition, frames_per_second_lower_bound)
LAYOUT_OFFSET(particle_definition, frames_per_second_upper_bound)
LAYOUT_OFFSET(particle_definition, frames_per_second_contact_deterioration)
LAYOUT_OFFSET(particle_definition, first_sequence_index)
LAYOUT_OFFSET(particle_definition, initial_sequence_count)
LAYOUT_OFFSET(particle_definition, looping_sequence_count)
LAYOUT_OFFSET(particle_definition, final_sequence_count)

LAYOUT_OFFSET(bitmap_group, sequences)
LAYOUT_SIZE(bitmap_group_sequence)
LAYOUT_OFFSET(bitmap_group_sequence, sprites)
LAYOUT_SIZE(bitmap_group_sprite)
