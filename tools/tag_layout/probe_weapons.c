/*
Weapon layouts (see probe.c): what a weapon's update reads of its definition
(weapons.c's weapon_update and weapon_trigger_can_fire_again).
*/

#include "cseries.h"
#include "tag_files/tag_files.h"
#include "items/weapon_definitions.h"

#include <stddef.h>

#define LAYOUT_OFFSET(s, f) const int layout__##s##__##f = offsetof(struct s, f);
#define LAYOUT_SIZE(s) const int layout__##s##__SIZEOF = sizeof(struct s);

/* weapons: heat, age and their triggers and magazines */
LAYOUT_OFFSET(_weapon_definition, secondary_trigger_mode)
LAYOUT_OFFSET(_weapon_definition, maximum_alternate_shots_loaded)
LAYOUT_OFFSET(_weapon_definition, heat_recovery_threshold)
LAYOUT_OFFSET(_weapon_definition, heat_overheated_threshold)
LAYOUT_OFFSET(_weapon_definition, heat_loss_per_second)
LAYOUT_OFFSET(_weapon_definition, age_heat_recovery_penalty)
LAYOUT_OFFSET(_weapon_definition, age_rate_of_fire_penalty)
LAYOUT_OFFSET(_weapon_definition, magazines)
LAYOUT_OFFSET(_weapon_definition, triggers)
LAYOUT_SIZE(weapon_magazine_definition)
LAYOUT_OFFSET(weapon_magazine_definition, rounds_recharged_per_second)
LAYOUT_OFFSET(weapon_magazine_definition, rounds_loaded_maximum)
LAYOUT_SIZE(weapon_trigger_definition)
LAYOUT_OFFSET(weapon_trigger_definition, flags)
LAYOUT_OFFSET(weapon_trigger_definition, initial_rate_of_fire)
LAYOUT_OFFSET(weapon_trigger_definition, final_rate_of_fire)
LAYOUT_OFFSET(weapon_trigger_definition, blurred_rate_of_fire)
LAYOUT_OFFSET(weapon_trigger_definition, magazine_index)
LAYOUT_OFFSET(weapon_trigger_definition, rounds_per_shot)
LAYOUT_OFFSET(weapon_trigger_definition, minimum_rounds_loaded_per_shot)
LAYOUT_OFFSET(weapon_trigger_definition, charged_time)
LAYOUT_OFFSET(weapon_trigger_definition, runtime_illumination_recovery_time)
LAYOUT_OFFSET(weapon_trigger_definition, runtime_ejection_port_recovery_time)
LAYOUT_OFFSET(weapon_trigger_definition, runtime_rate_of_fire_acceleration_time)
LAYOUT_OFFSET(weapon_trigger_definition, runtime_rate_of_fire_deceleration_time)
LAYOUT_OFFSET(weapon_trigger_definition, runtime_error_acceleration_time)
LAYOUT_OFFSET(weapon_trigger_definition, runtime_error_deceleration_time)
