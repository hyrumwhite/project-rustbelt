/*
Netcode layouts (see probe.c): what the distributed netcode's samplers read
of a player, its unit, an object and a weapon (port/linux/game/
network_distributed.c's distributed_state_from_player, network_objects.c's
distributed_state_from_object and distributed_inventory_from_unit).
*/

#include "cseries.h"
#include "objects/objects.h"
#include "objects/object_types.h"
#include "units/units.h"
#include "items/weapons.h"
#include "game/players.h"

#include <stddef.h>

#define LAYOUT_MEMBER(s, name, path) const int layout__##s##__##name = offsetof(struct s, path);
#define LAYOUT_SIZE(s) const int layout__##s##__SIZEOF = sizeof(struct s);
#define LAYOUT_VALUE(name) const int layout__net_values__##name = name;

/* a unit (its object's and its own) */
LAYOUT_MEMBER(unit_datum, object_position, object.position)
LAYOUT_MEMBER(unit_datum, object_flags, object.flags)
LAYOUT_MEMBER(unit_datum, object_current_body_damage, object.current_body_damage)
LAYOUT_MEMBER(unit_datum, object_current_shield_damage, object.current_shield_damage)
LAYOUT_MEMBER(unit_datum, unit_parent_seat_index, unit.parent_seat_index)
LAYOUT_MEMBER(unit_datum, unit_active_camouflage, unit.active_camouflage)
LAYOUT_MEMBER(unit_datum, unit_flags, unit.flags)
LAYOUT_MEMBER(unit_datum, unit_grenade_counts, unit.grenade_counts)
LAYOUT_MEMBER(unit_datum, unit_current_weapon_index, unit.current_weapon_index)
LAYOUT_MEMBER(unit_datum, unit_weapon_object_indices, unit.weapon_object_indices)

/* a weapon */
LAYOUT_MEMBER(weapon_datum, weapon_magazines, weapon.magazines)
LAYOUT_MEMBER(weapon_datum, weapon_age, weapon.age)
LAYOUT_SIZE(weapon_magazine)
LAYOUT_MEMBER(weapon_magazine, rounds_total, rounds_total)
LAYOUT_MEMBER(weapon_magazine, rounds_loaded, rounds_loaded)

/* a player */
LAYOUT_MEMBER(player_datum, identifier, identifier)
LAYOUT_MEMBER(player_datum, unit_index, unit_index)
LAYOUT_MEMBER(player_datum, powerup_durations, powerup_durations)

/* flags and counts */
LAYOUT_VALUE(_object_at_rest_bit)
LAYOUT_VALUE(_object_dead_bit)
LAYOUT_VALUE(_object_shield_depleted_bit)
LAYOUT_VALUE(_object_shield_charging_bit)
LAYOUT_VALUE(_object_shield_over_charging_bit)
LAYOUT_VALUE(_object_mask_unit)
LAYOUT_VALUE(_object_type_weapon)
LAYOUT_VALUE(_unit_active_camouflaged_bit)
LAYOUT_VALUE(_unit_super_camouflaged_bit)
LAYOUT_VALUE(NUMBER_OF_UNIT_GRENADE_TYPES)
LAYOUT_VALUE(MAXIMUM_WEAPONS_PER_UNIT)
