/*
Weapon and first-person animation layouts (see probe.c): what a player's
melee reads (bipeds.c's biped_update, weapons.c's
weapon_get_first_person_animation_time, units.c's
unit_animation_start_action).
*/

#include "cseries.h"
#include "items/weapon_definitions.h"
#include "models/model_animation_definitions.h"

#include <stddef.h>

#define LAYOUT_OFFSET(s, f) const int layout__##s##__##f = offsetof(struct s, f);
#define LAYOUT_SIZE(s) const int layout__##s##__SIZEOF = sizeof(struct s);

/* (private to interface/first_person_weapons.c; copied as it is) */
struct animation_graph_first_person_weapon_animations
{
	long unused[4];
	struct tag_block animations;
};

LAYOUT_OFFSET(weapon_definition, weapon)
LAYOUT_OFFSET(_weapon_definition, flags)
LAYOUT_OFFSET(_weapon_definition, interface_definition)
LAYOUT_OFFSET(_weapon_definition, weapon_type)
LAYOUT_OFFSET(weapon_interface_definition, first_person_animations)
LAYOUT_OFFSET(animation_graph, first_person_weapon_animations)
LAYOUT_SIZE(animation_graph_first_person_weapon_animations)
LAYOUT_OFFSET(animation_graph_first_person_weapon_animations, animations)
LAYOUT_OFFSET(animation_graph_weapon_type, animations)
