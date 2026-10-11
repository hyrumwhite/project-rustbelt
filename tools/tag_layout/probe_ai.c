/*
AI layouts (see probe.c): what the AI's ports read and write of an actor, its
props and its definition (ai/actors.h, props.h, actor_definitions.h).
*/

#include "cseries.h"
#include "tag_files/tag_files.h"
#include "tag_files/tag_groups.h"
#include "ai/ai.h"
#include "ai/actors.h"
#include "ai/props.h"
#include "ai/actor_definitions.h"

#include <stddef.h>

#define LAYOUT_OFFSET(s, f) const int layout__##s##__##f = offsetof(struct s, f);
#define LAYOUT_MEMBER(s, name, path) const int layout__##s##__##name = offsetof(struct s, path);
#define LAYOUT_SIZE(s) const int layout__##s##__SIZEOF = sizeof(struct s);

LAYOUT_SIZE(actor_datum)
LAYOUT_MEMBER(actor_datum, meta_definition_index, meta.definition_index)
LAYOUT_MEMBER(actor_datum, meta_unit_index, meta.unit_index)
LAYOUT_MEMBER(actor_datum, meta_first_prop_index, meta.first_prop_index)
LAYOUT_MEMBER(actor_datum, state_mode, state.mode)
LAYOUT_MEMBER(actor_datum, state_combat_status, state.combat_status)
LAYOUT_MEMBER(actor_datum, input_shield_vitality, input.shield_vitality)
LAYOUT_MEMBER(actor_datum, input_body_vitality, input.body_vitality)
LAYOUT_MEMBER(actor_datum, input_vehicle_index, input.vehicle_index)
LAYOUT_MEMBER(actor_datum, input_body_position, input.position.body_position)
LAYOUT_MEMBER(actor_datum, external_orders_defending, external_orders.defending)
LAYOUT_MEMBER(actor_datum, external_orders_stand_down, external_orders.stand_down)
LAYOUT_MEMBER(actor_datum, situation_specific_threats, situation.specific_threats)
LAYOUT_MEMBER(actor_datum, situation_cumulative_threats, situation.cumulative_threats)
LAYOUT_MEMBER(actor_datum, target_target_type, target.target_type)
LAYOUT_MEMBER(actor_datum, target_target_prop_index, target.target_prop_index)
LAYOUT_MEMBER(actor_datum, emotions_instantaneous_danger, emotions.instantaneous_danger)
LAYOUT_MEMBER(actor_datum, emotions_perceived_danger, emotions.perceived_danger)
LAYOUT_MEMBER(actor_datum, emotions_defensive_crouch, emotions.defensive_crouch)
LAYOUT_MEMBER(actor_datum, emotions_defensive_crouch_timer, emotions.defensive_crouch_timer)
LAYOUT_MEMBER(actor_datum, emotions_crouch_blocking_player_line_of_fire, emotions.crouch_blocking_player_line_of_fire)
LAYOUT_MEMBER(actor_datum, emotions_crouch_blocking_line_of_fire, emotions.crouch_blocking_line_of_fire)
LAYOUT_MEMBER(actor_datum, emotions_crouch_friends_in_line_of_fire, emotions.crouch_friends_in_line_of_fire)
LAYOUT_MEMBER(actor_datum, emotions_moving_into_player_line_of_fire, emotions.moving_into_player_line_of_fire)
LAYOUT_MEMBER(actor_datum, emotions_moving_into_fire_timer, emotions.moving_into_fire_timer)
LAYOUT_MEMBER(actor_datum, emotions_evasion_delay_timer, emotions.evasion_delay_timer)
LAYOUT_MEMBER(actor_datum, emotions_currently_defending, emotions.currently_defending)
LAYOUT_MEMBER(actor_datum, emotions_forced_to_charge, emotions.forced_to_charge)
LAYOUT_MEMBER(actor_datum, emotions_berserk, emotions.berserk)
LAYOUT_MEMBER(actor_datum, emotions_original_body_vitality, emotions.original_body_vitality)
LAYOUT_MEMBER(actor_datum, firing_positions_current_position_index, firing_positions.current_position_index)
LAYOUT_MEMBER(actor_datum, control_moving, control.moving)
LAYOUT_MEMBER(actor_datum, control_moving_towards_vector, control.moving_towards_vector)

LAYOUT_SIZE(prop_datum)
LAYOUT_OFFSET(prop_datum, next_prop_index)
LAYOUT_OFFSET(prop_datum, swarm)
LAYOUT_OFFSET(prop_datum, state)
LAYOUT_OFFSET(prop_datum, enemy)
LAYOUT_OFFSET(prop_datum, dead)
LAYOUT_OFFSET(prop_datum, player)
LAYOUT_OFFSET(prop_datum, vehicle_index)
LAYOUT_OFFSET(prop_datum, body_position)
LAYOUT_OFFSET(prop_datum, shooting)
LAYOUT_OFFSET(prop_datum, actor_to_prop)

LAYOUT_OFFSET(actor_definition, flags)
LAYOUT_OFFSET(actor_definition, defensive)
LAYOUT_OFFSET(actor_defensive_properties, defensive_crouch_type)
LAYOUT_OFFSET(actor_defensive_properties, defensive_threshold_attacking)
LAYOUT_OFFSET(actor_defensive_properties, defensive_threshold_defending)
LAYOUT_OFFSET(actor_defensive_properties, defensive_crouch_min_stand_time)
LAYOUT_OFFSET(actor_defensive_properties, defensive_crouch_min_crouch_time)

#include "ai/ai_communication.h"

/* the constants (`ai_values`) */
#define LAYOUT_VALUE(name) const int layout__ai_values__##name = name;
LAYOUT_VALUE(_actor_definition_always_charge_bit)
LAYOUT_VALUE(_actor_definition_charge_in_attacking_mode_bit)
LAYOUT_VALUE(_actor_definition_crouch_in_line_of_fire_bit)
LAYOUT_VALUE(_actor_definition_avoid_friend_line_of_fire_bit)
LAYOUT_VALUE(_ai_communication_advance)
LAYOUT_VALUE(_actor_threat_visible_facing_me)
LAYOUT_VALUE(_actor_threat_visible_aiming_at_me)
LAYOUT_VALUE(_actor_threat_shooting_near_me)
LAYOUT_VALUE(_actor_threat_shooting_at_me)
LAYOUT_VALUE(_actor_threat_extremely_close_to_me)
LAYOUT_VALUE(_actor_threat_damaging_me)
LAYOUT_VALUE(NUMBER_OF_ACTOR_THREAT_TYPES)
LAYOUT_VALUE(_prop_state_becoming_unacknowledged)
LAYOUT_VALUE(_prop_state_acknowledged)
