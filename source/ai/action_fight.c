/*
ACTION_FIGHT.C

symbols in this file:
00002C20 0040:
	_action_fight_setup (0000)
00002C60 0010:
	_action_fight_begin (0000)
00002C70 0010:
	_action_fight_end (0000)
00002C80 0070:
	_action_fight_update (0000)
00002CF0 0080:
	_action_fight_control (0000)
00002D70 03d0:
	_action_fight_perform (0000)
00243080 0021:
	??_C@_0CB@KJFGPOLC@c?3?2halo?2SOURCE?2ai?2action_fight?4c@ (0000)
*/

/* ---------- headers */

#include "cseries.h"
#include "actions.h"

#include "actor_definitions.h"
#include "actors.h"
#include "ai_scenario_definitions.h"
#include "props.h"
#include "scenario/scenario.h"
#include "scenario/scenario_definitions.h"
#include "objects/objects.h"
#include "units/vehicle_definitions.h"
#include "units/vehicles.h"
#include "halo_sim.h" /* port: port/linux/game/halo_sim.c */

/* ---------- constants */

/* ---------- macros */

/* ---------- structures */

struct fight_vehicle_definition
{
	byte unresolved[0x3A8];
	real minimum_firing_position_time;
};

/* ---------- prototypes */

/* ---------- globals */

/* ---------- public code */

/* port: what the action asks of the rest of the game, shared by the game's
and the Rust port's (crates/halo_map's actions); in check mode each is
recorded */
void action_fight_call(
	struct halo_sim_ai_call *call)
{
	int opened = halo_sim_ai_event_begin(call);
	long actor_index = call->actor;
	struct actor_datum *actor = actor_get(actor_index);

	switch (call->kind)
	{
		case _action_fight_call_discard_firing_position:
			actor_discard_firing_position(actor_index, (short)call->a, FALSE);
			break;

		case _action_fight_call_firing_variant:
			call->result = (long)actor_combat_get_firing_variant_definition(actor_index);
			break;

		case _action_fight_call_find_pathfinding_location:
			actor_find_pathfinding_location(actor_index);
			break;

		case _action_fight_call_nearby_firing_positions:
			call->result = actor_nearby_firing_positions(
				actor_index,
				&actor->input.pathfinding_point,
				actor->input.pathfinding_surface_index,
				_firing_position_group_normal) ? 1 : 0;
			break;

		case _action_fight_call_near_current_firing_position:
		{
			struct encounter_definition *encounter = TAG_BLOCK_GET_ELEMENT(
				&global_scenario_get()->ai_encounters,
				DATUM_INDEX_TO_ABSOLUTE_INDEX(actor->meta.encounter_index),
				struct encounter_definition);
			struct firing_position_definition *firing_position = TAG_BLOCK_GET_ELEMENT(
				&encounter->firing_positions,
				actor->firing_positions.current_position_index,
				struct firing_position_definition);
			real tolerance = actor_destination_tolerance(actor_index);

			call->result = distance_squared3d(
				&actor->input.position.body_position,
				&firing_position->position) < tolerance * tolerance;
			break;
		}

		case _action_fight_call_change_firing_position:
		{
			short firing_position_index;
			boolean position_flags;
			long previous_owner_actor_index;
			struct firing_position candidate;
			struct firing_position_evaluation_context search;
			struct path_state workspace;

			csmemset(&search, 0, sizeof(search));
			search.evaluation_mode = _firing_point_evaluation_mode_fight;
			firing_position_index = actor_active_select_firing_position(
				actor_index,
				&search,
				&candidate,
				&previous_owner_actor_index,
				&workspace,
				&position_flags);
			call->result = actor_change_firing_position(
				actor_index,
				firing_position_index,
				&candidate,
				previous_owner_actor_index,
				&workspace,
				position_flags);
			break;
		}

		case _action_fight_call_random_range:
			call->output[0] = real_seed_random_range(get_global_random_seed_address(), call->input[0], call->input[1]);
			break;

		case _action_fight_call_vehicle_minimum_firing_position_time:
		{
			struct unit_datum *vehicle = vehicle_get(actor->input.vehicle_index);
			struct fight_vehicle_definition *vehicle_definition =
				(struct fight_vehicle_definition *)vehicle_definition_get(vehicle->definition_index);

			call->output[0] = vehicle_definition->minimum_firing_position_time;
			break;
		}

		case _action_fight_call_has_ranged_weapon:
			call->result = actor_has_ranged_weapon(actor_index) ? 1 : 0;
			break;

		case _action_fight_call_firing_position_reaches:
		{
			struct prop_datum *prop = prop_get(call->a);
			struct encounter_definition *encounter = TAG_BLOCK_GET_ELEMENT(
				&global_scenario_get()->ai_encounters,
				DATUM_INDEX_TO_ABSOLUTE_INDEX(actor->meta.encounter_index),
				struct encounter_definition);
			struct firing_position_definition *firing_position = TAG_BLOCK_GET_ELEMENT(
				&encounter->firing_positions,
				actor->firing_positions.current_position_index,
				struct firing_position_definition);
			real tolerance = actor_destination_tolerance(actor_index);

			call->result = distance_squared3d(
				&actor->input.position.body_position,
				&firing_position->position) > tolerance * tolerance &&
				distance_squared3d(
					&firing_position->position,
					&prop->body_position) < actor->control.weapon_maximum_range *
						actor->control.weapon_maximum_range;
			break;
		}

		case _action_fight_call_perception_unreachable:
			actor_perception_unreachable(actor_index, call->a, (boolean)call->b);
			break;
	}
	halo_sim_ai_event_end(call, opened);

	return;
}

static long action_fight_call_simple(
	long actor_index,
	long kind,
	long a,
	long b,
	real input0,
	real input1,
	real *output)
{
	struct halo_sim_ai_call call;

	memset(&call, 0, sizeof(call));
	call.kind = kind;
	call.actor = actor_index;
	call.a = a;
	call.b = b;
	call.input[0] = input0;
	call.input[1] = input1;
	action_fight_call(&call);
	if (output)
		*output = call.output[0];

	return call.result;
}


boolean action_fight_setup(
	long actor_index,
	struct fight_state_data *state_data)
{
	match_assert("c:\\halo\\SOURCE\\ai\\action_fight.c", 30, state_data);
	memset(state_data, 0, sizeof(*state_data));
	return TRUE;
}

void action_fight_begin(
	long actor_index)
{
	return;
}

void action_fight_end(
	long actor_index)
{
	return;
}

/* ---------- private code */

void
action_fight_update(
	long actor_index)
{
	struct actor_datum *actor = actor_get(actor_index);
	struct fight_state_data *state_data = &actor->state.action_data.fight;

	if (state_data->firing_position_timer > 0 && actor->control.path.at_destination)
	{
		state_data->firing_position_timer--;
		if (state_data->firing_position_timer == 0 &&
			actor->firing_positions.current_position_index != NONE &&
			!actor->firing_positions.current_position_found_outside_range)
		{
			/* port: the discard's writes are Rust's own too, not a call */
			actor_discard_firing_position(actor_index, actor->firing_positions.current_position_index, FALSE);
		}
	}

	return;
}

void
action_fight_control(
	long actor_index)
{
	struct actor_datum *actor = actor_get(actor_index);

	actor->orders.look.primary_priority = 5;
	actor->orders.look.primary_direction.type = 2;
	actor->orders.look.idle_look_type = 4;
	actor->orders.move.stationary_crouch = actor->emotions.defensive_crouch;
	actor->orders.move.moving_crouch = FALSE;
	actor->orders.move.panicked = FALSE;
	actor->orders.move.dive_into_cover = FALSE;
	actor->orders.move.emerge_from_cover = FALSE;

	if (actor->input.vehicle_driver_type != 4 && actor->state.combat_status >= 5)
	{
		actor->orders.combat.shoot_at_target = TRUE;
		actor->orders.look.primary_priority = 7;
	}

	return;
}

boolean
action_fight_perform(
	long actor_index)
{
	struct actor_datum *actor = actor_get(actor_index);
	struct actor_definition *definition;

	match_assert("c:\\halo\\SOURCE\\ai\\action_fight.c", 55, !actor->meta.swarm);
	if (actor->meta.timeslice)
	{
		struct actor_variant_definition *firing_variant;

		definition = actor_definition_get(actor->meta.definition_index);
		firing_variant = (struct actor_variant_definition *)action_fight_call_simple(actor_index,
			_action_fight_call_firing_variant, 0, 0, 0.0f, 0.0f, NULL);

		if (!actor->input.vehicle_passenger)
		{
			if (actor->emotions.defensive_crouch &&
				TEST_FLAG(definition->flags, 5))
			{
				action_fight_call_simple(actor_index, _action_fight_call_find_pathfinding_location, 0, 0, 0.0f, 0.0f, NULL);
				if (action_fight_call_simple(actor_index, _action_fight_call_nearby_firing_positions, 0, 0, 0.0f, 0.0f, NULL))
				{
					boolean near_current_firing_position;

					near_current_firing_position = FALSE;
					if (actor->meta.encounter_index != NONE &&
						actor->firing_positions.current_position_index != NONE)
					{
						near_current_firing_position = (boolean)action_fight_call_simple(actor_index,
							_action_fight_call_near_current_firing_position, 0, 0, 0.0f, 0.0f, NULL);
					}

					if (actor->control.moving &&
						actor->situation.cumulative_threats[5] == 0)
					{
						if (actor->target.target_prop_index == NONE)
							goto update_unreachable;

						if (!(prop_get(actor->target.target_prop_index)->distance <
							firing_variant->ranged_combat.combat_range_upper_bound))
							goto update_unreachable;
					}
					else if (near_current_firing_position)
					{
						goto update_unreachable;
					}

					actor->firing_positions.current_position_index = NONE;
					actor_move_halt(actor_index);
					goto update_unreachable;
				}
			}

			{
				short old_firing_position_index;
				short new_firing_position_index;

				old_firing_position_index = actor->firing_positions.current_position_index;
				/* port: the selection's calls are its own */
				new_firing_position_index = actor_firing_position_port_change_to_active_selection(actor_index,
					_firing_point_evaluation_mode_fight);
				if (new_firing_position_index == NONE)
				{
					actor->state.action_data.fight.firing_position_timer = 0;
				}
				else if (new_firing_position_index != old_firing_position_index)
				{
					real combat_position_time;

					action_fight_call_simple(actor_index, _action_fight_call_random_range, 0, 0,
						definition->firing_position.combat_position_time_lower_bound,
						definition->firing_position.combat_position_time_upper_bound,
						&combat_position_time);
					if (actor->input.vehicle_driver_type > 0)
					{
						real minimum_firing_position_time;

						action_fight_call_simple(actor_index, _action_fight_call_vehicle_minimum_firing_position_time,
							0, 0, 0.0f, 0.0f, &minimum_firing_position_time);
						if (minimum_firing_position_time > 0.f &&
							combat_position_time > minimum_firing_position_time)
						{
							combat_position_time = minimum_firing_position_time;
						}
					}

					actor->state.action_data.fight.firing_position_timer =
						(short)(combat_position_time * 30.f);
				}
			}
		}

update_unreachable:
		if (actor->target.target_type >= 7)
		{
			struct prop_datum *prop = prop_get(actor->target.target_prop_index);
			boolean unreachable = TRUE;

			if (action_fight_call_simple(actor_index, _action_fight_call_has_ranged_weapon, 0, 0, 0.0f, 0.0f, NULL))
			{
				if (prop->distance < actor->control.weapon_maximum_range)
				{
					unreachable = FALSE;
				}
				else if (actor->meta.encounter_index != NONE &&
					actor->firing_positions.current_position_index != NONE &&
					action_fight_call_simple(actor_index, _action_fight_call_firing_position_reaches,
						actor->target.target_prop_index, 0, 0.0f, 0.0f, NULL))
				{
					unreachable = FALSE;
				}
			}

			action_fight_call_simple(actor_index, _action_fight_call_perception_unreachable,
				actor->target.target_prop_index, unreachable, 0.0f, 0.0f, NULL);
		}
	}

	return FALSE;
}
