/*
Weapon helper layouts (see probe.c): what firing, reloading, chambering and a
weapon's state read of its definition and graph (weapons.c's helpers).
*/

#include "cseries.h"
#include "tag_files/tag_files.h"
#include "items/weapon_definitions.h"
#include "models/model_animation_definitions.h"

#include <stddef.h>

#define LAYOUT_OFFSET(s, f) const int layout__##s##__##f = offsetof(struct s, f);
#define LAYOUT_SIZE(s) const int layout__##s##__SIZEOF = sizeof(struct s);

/* weapons.c's own, mirrored */
struct trigger_firing_effect
{
	short shots_lower_bound;
	short shots_upper_bound;
	long unused[8];
	struct tag_reference effects[3];
	struct tag_reference damage_effects[3];
};

struct animation_graph_weapon_animations
{
	long unused1[4];
	struct tag_block animations;
};

LAYOUT_OFFSET(_weapon_definition, heat_detonation_threshold)
LAYOUT_OFFSET(_weapon_definition, overheated_explosion_fraction)
LAYOUT_OFFSET(_weapon_definition, overheated_effect)
LAYOUT_OFFSET(_weapon_definition, detonation_effect)
LAYOUT_OFFSET(_weapon_definition, age_misfire_start)
LAYOUT_OFFSET(_weapon_definition, age_misfire_chance)

LAYOUT_OFFSET(weapon_magazine_definition, flags)
LAYOUT_OFFSET(weapon_magazine_definition, rounds_reloaded)
LAYOUT_OFFSET(weapon_magazine_definition, chamber_time)
LAYOUT_OFFSET(weapon_magazine_definition, reloading_effect)
LAYOUT_OFFSET(weapon_magazine_definition, chambering_effect)

LAYOUT_OFFSET(weapon_trigger_definition, rounds_between_tracers)
LAYOUT_OFFSET(weapon_trigger_definition, firing_noise)
LAYOUT_OFFSET(weapon_trigger_definition, overcharged_action)
LAYOUT_OFFSET(weapon_trigger_definition, spew_time)
LAYOUT_OFFSET(weapon_trigger_definition, charging_effect)
LAYOUT_OFFSET(weapon_trigger_definition, projectile_distribution_function)
LAYOUT_OFFSET(weapon_trigger_definition, projectiles_per_shot)
LAYOUT_OFFSET(weapon_trigger_definition, projectile_distribution_angle)
LAYOUT_OFFSET(weapon_trigger_definition, projectile_error_inner_cone_angle)
LAYOUT_OFFSET(weapon_trigger_definition, projectile_error_angle_lower_bound)
LAYOUT_OFFSET(weapon_trigger_definition, projectile_error_angle_upper_bound)
LAYOUT_OFFSET(weapon_trigger_definition, first_person_weapon_offset)
LAYOUT_OFFSET(weapon_trigger_definition, projectile)
LAYOUT_OFFSET(weapon_trigger_definition, ejection_port_recovery_time)
LAYOUT_OFFSET(weapon_trigger_definition, illumination_recovery_time)
LAYOUT_OFFSET(weapon_trigger_definition, heat_generated_per_round)
LAYOUT_OFFSET(weapon_trigger_definition, age_generated_per_round)
LAYOUT_OFFSET(weapon_trigger_definition, overloading_time)
LAYOUT_OFFSET(weapon_trigger_definition, firing_effects)

LAYOUT_SIZE(trigger_firing_effect)
LAYOUT_OFFSET(trigger_firing_effect, shots_lower_bound)
LAYOUT_OFFSET(trigger_firing_effect, shots_upper_bound)
LAYOUT_OFFSET(trigger_firing_effect, effects)
LAYOUT_OFFSET(trigger_firing_effect, damage_effects)

LAYOUT_OFFSET(animation_graph, weapon_animations)
LAYOUT_SIZE(animation_graph_weapon_animations)
LAYOUT_OFFSET(animation_graph_weapon_animations, animations)
