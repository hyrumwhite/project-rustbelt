/*
Damage layouts (see probe.c): what a damage effect does to what it hits
(damage.c's object_cause_damage, object_damage_shield, object_damage_body).
*/

#include "cseries.h"
#include "tag_files/tag_files.h"
#include "game/game_globals.h"
#include "objects/damage_effect_definitions.h"
#include "objects/damage_resistances.h"
#include "physics/collision_model_definitions.h"
#include "physics/collision_bsp_definitions.h"
#include "units/unit_definitions.h"

#include <stddef.h>

#define LAYOUT_OFFSET(s, f) const int layout__##s##__##f = offsetof(struct s, f);
#define LAYOUT_SIZE(s) const int layout__##s##__SIZEOF = sizeof(struct s);

/* the material types the damage modifiers have */
const int layout__material_types__COUNT = NUMBER_OF_MATERIAL_TYPES;

/* damage effects */
LAYOUT_OFFSET(damage_effect_definition, damage)
LAYOUT_OFFSET(damage_effect_definition, falloff_radius)
LAYOUT_OFFSET(damage_effect_definition, cutoff_radius)
LAYOUT_OFFSET(damage_effect_definition, flags)
LAYOUT_OFFSET(damage_definition, area_of_effect_core_radius)
LAYOUT_OFFSET(damage_definition, side_effect)
LAYOUT_OFFSET(damage_definition, category)
LAYOUT_OFFSET(damage_definition, flags)
LAYOUT_OFFSET(damage_definition, damage_minimum)
LAYOUT_OFFSET(damage_definition, damage_lower_bound)
LAYOUT_OFFSET(damage_definition, damage_upper_bound)
LAYOUT_OFFSET(damage_definition, vehicle_passthrough_penalty)
LAYOUT_OFFSET(damage_definition, material_modifiers)

/* collision models: their damage resistance and nodes */
LAYOUT_OFFSET(collision_model, resistance)
LAYOUT_OFFSET(collision_model, nodes)
LAYOUT_SIZE(collision_node)
LAYOUT_OFFSET(collision_node, runtime_damage_part)
LAYOUT_OFFSET(damage_resistance, flags)
LAYOUT_OFFSET(damage_resistance, indirect_damage_material_index)
LAYOUT_OFFSET(damage_resistance, friendly_damage_resistance)
LAYOUT_OFFSET(damage_resistance, localized_damage_effect)
LAYOUT_OFFSET(damage_resistance, area_damage_effect_threshold)
LAYOUT_OFFSET(damage_resistance, area_damage_effect)
LAYOUT_OFFSET(damage_resistance, body_damaged_effect_threshold)
LAYOUT_OFFSET(damage_resistance, body_destroyed_threshold)
LAYOUT_OFFSET(damage_resistance, shield_material_type)
LAYOUT_OFFSET(damage_resistance, shield_failure_function)
LAYOUT_OFFSET(damage_resistance, shield_failure_threshold)
LAYOUT_OFFSET(damage_resistance, maximum_shield_failure)
LAYOUT_OFFSET(damage_resistance, minimum_shield_stun_damage)
LAYOUT_OFFSET(damage_resistance, shield_stun_time)
LAYOUT_OFFSET(damage_resistance, shield_damaged_effect_threshold)
LAYOUT_OFFSET(damage_resistance, runtime_shield_recharge_velocity)
LAYOUT_OFFSET(damage_resistance, materials)
LAYOUT_OFFSET(damage_resistance, regions)
LAYOUT_SIZE(damage_resistance_material)
LAYOUT_OFFSET(damage_resistance_material, flags)
LAYOUT_OFFSET(damage_resistance_material, material_type)
LAYOUT_OFFSET(damage_resistance_material, shield_leak_fraction)
LAYOUT_OFFSET(damage_resistance_material, shield_damage_multiplier)
LAYOUT_OFFSET(damage_resistance_material, body_damage_multiplier)

/* units: what a vehicle passes through to its riders */
LAYOUT_OFFSET(_unit_definition, child_damage_fraction)
