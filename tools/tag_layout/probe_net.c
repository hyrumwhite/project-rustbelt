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
#include "items/items.h"
#include "items/weapon_definitions.h"
#include "game/players.h"
#include "objects/object_definitions.h"
#include "models/model_definitions.h"

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
LAYOUT_MEMBER(item_datum, item_flags, item.flags)
LAYOUT_SIZE(weapon_magazine)
LAYOUT_MEMBER(weapon_magazine, rounds_total, rounds_total)
LAYOUT_MEMBER(weapon_magazine, rounds_loaded, rounds_loaded)
LAYOUT_MEMBER(weapon_magazine, state, state)
LAYOUT_MEMBER(weapon_datum, weapon_game_time_last_fired, weapon.game_time_last_fired)
LAYOUT_MEMBER(unit_datum, unit_desired_weapon_index, unit.desired_weapon_index)
LAYOUT_MEMBER(unit_datum, object_next_object_index, object.next_object_index)
LAYOUT_MEMBER(unit_datum, object_first_child_object_index, object.first_child_object_index)
LAYOUT_MEMBER(unit_datum, object_forced_shader_permutation_index, object.forced_shader_permutation_index)
LAYOUT_MEMBER(unit_datum, object_region_permutations, object.region_permutations)
LAYOUT_MEMBER(unit_datum, object_base_change_colors, object.base_change_colors)
LAYOUT_MEMBER(unit_datum, object_outgoing_change_colors, object.outgoing_change_colors)
LAYOUT_MEMBER(player_datum, action_object_index, action_object_index)
LAYOUT_MEMBER(player_datum, action_result, action_result)
LAYOUT_MEMBER(object_definition, object_type, object.type)
LAYOUT_MEMBER(object_definition, object_model, object.model)
LAYOUT_SIZE(model_region)
LAYOUT_MEMBER(model_region, permutations, permutations)
LAYOUT_MEMBER(model, regions, regions)
LAYOUT_MEMBER(_weapon_definition, magazines, magazines)
LAYOUT_MEMBER(weapon_magazine_definition, rounds_total_initial, rounds_total_initial)
LAYOUT_MEMBER(weapon_magazine_definition, rounds_total_maximum, rounds_total_maximum)
LAYOUT_MEMBER(weapon_magazine_definition, rounds_loaded_maximum, rounds_loaded_maximum)

/* a player */
LAYOUT_MEMBER(player_datum, identifier, identifier)
LAYOUT_MEMBER(player_datum, unit_index, unit_index)
LAYOUT_MEMBER(player_datum, powerup_durations, powerup_durations)
LAYOUT_MEMBER(player_datum, local_player_index, local_player_index)
LAYOUT_MEMBER(player_datum, teleporter_index, teleporter_index)

/* flags and counts */
LAYOUT_VALUE(_object_at_rest_bit)
LAYOUT_VALUE(_object_dead_bit)
LAYOUT_VALUE(_object_shield_depleted_bit)
LAYOUT_VALUE(_object_shield_charging_bit)
LAYOUT_VALUE(_object_shield_over_charging_bit)
LAYOUT_VALUE(_object_mask_unit)
LAYOUT_VALUE(_object_mask_item)
LAYOUT_VALUE(_object_mask_biped)
LAYOUT_VALUE(_object_mask_vehicle)
LAYOUT_VALUE(_object_connected_to_map_bit)
LAYOUT_VALUE(_item_attached_to_unit_bit)
LAYOUT_VALUE(_magazine_reloading)
#define MAXIMUM_TRACKED_OBJECTS HALO_PORT_MAXIMUM_OBJECTS_PER_MAP
#define MAXIMUM_TRACKED_PLAYERS HALO_PORT_MAXIMUM_NETWORK_PLAYERS
LAYOUT_VALUE(MAXIMUM_TRACKED_OBJECTS)
LAYOUT_VALUE(MAXIMUM_TRACKED_PLAYERS)
LAYOUT_VALUE(_object_mask_equipment)
LAYOUT_VALUE(_object_type_biped)
LAYOUT_VALUE(_object_type_equipment)
LAYOUT_VALUE(MAXIMUM_REGIONS_PER_OBJECT)
LAYOUT_VALUE(NUMBER_OF_OBJECT_CHANGE_COLORS)
LAYOUT_VALUE(_object_mask_weapon)
LAYOUT_VALUE(_object_type_weapon)
LAYOUT_VALUE(_unit_active_camouflaged_bit)
LAYOUT_VALUE(_unit_super_camouflaged_bit)
LAYOUT_VALUE(NUMBER_OF_UNIT_GRENADE_TYPES)
LAYOUT_VALUE(MAXIMUM_WEAPONS_PER_UNIT)
