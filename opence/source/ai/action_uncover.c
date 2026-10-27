/*
ACTION_UNCOVER.C

symbols in this file:
00008690 0080: _action_uncover_setup_target
00008710 0100: _action_uncover_setup_pursuit
00008810 0220: _action_uncover_perform
00008A30 0170: _action_uncover_control
00008BA0 0040: _action_uncover_flush_position_indices
00008BE0 0030: _action_uncover_flush_structure_indices
00008C10 0070: _action_uncover_modify_color
00008C80 0170: _action_uncover_begin
00008DF0 0300: _action_uncover_update
*/

/* ---------- headers */

#include "cseries.h"
#include "cseries/errors.h"
#include "actions.h"

#include "actor_definitions.h"
#include "actors.h"
#include "ai.h"
#include "ai_communication.h"
#include "ai_debug.h"
#include "ai_scenario_definitions.h"
#include "props.h"
#include "scenario/scenario.h"
#include "scenario/scenario_definitions.h"
#include "math/real_math.h"
#include "halo_sim.h" /* port: port/linux/game/halo_sim.c */

/* ---------- macros */

#define ACTION_UNCOVER_DEBUG_PRINTING_ENABLED() (ai_debug.print_uncovering)

/* ---------- constants */

/* ---------- structures */

typedef char action_uncover_ai_debug_printing_offset_must_be_0xA4[
	offsetof(struct ai_debug_state, print_uncovering) == 0xA4 ? 1 : -1];

/* ---------- prototypes */

/* ---------- public code */

/* port: what the uncover action asks of the rest of the game, shared by the
game's and the Rust port's (crates/halo_map's actions); in check mode each is
recorded. The firing position the search selected is kept for its change. */
static struct
{
	boolean position_flags;
	long previous_owner_actor_index;
	short selected_firing_position_index;
	struct firing_position candidate;
	struct path_state workspace;
} action_uncover_selection;

void action_uncover_call(
	struct halo_sim_ai_call *call)
{
	int opened = halo_sim_ai_event_begin(call);
	long actor_index = call->actor;
	struct actor_datum *actor = actor_get(actor_index);
	struct uncover_state_data *state_data = &actor->state.action_data.uncover;
	char temporary[256];

	switch (call->kind)
	{
		case _action_uncover_call_select_firing_position:
		{
			struct firing_position_evaluation_context search;

			csmemset(&search, 0, sizeof(search));
			search.evaluation_mode = _firing_point_evaluation_mode_uncover;
			if (state_data->pursuit_location.type == _pursuit_location_position)
			{
				search.specific_target_enable = TRUE;
				search.specific_target_point = state_data->pursuit_location.position;
				search.specific_target_surface_index = state_data->pursuit_location.surface_index;
				search.specific_target_cluster_index = state_data->pursuit_location.cluster_index;
			}
			else
			{
				search.use_last_visible_target_position = state_data->no_target_sight_available;
			}
			action_uncover_selection.selected_firing_position_index = actor_active_select_firing_position(
				actor_index,
				&search,
				&action_uncover_selection.candidate,
				&action_uncover_selection.previous_owner_actor_index,
				&action_uncover_selection.workspace,
				&action_uncover_selection.position_flags);
			call->result = action_uncover_selection.selected_firing_position_index;
			call->result2 = action_uncover_selection.candidate.line_of_sight;
			call->output[0] = action_uncover_selection.candidate.path_distance_from_actor;
			break;
		}

		case _action_uncover_call_tolerance:
			call->output[0] = actor_destination_tolerance(actor_index);
			break;

		case _action_uncover_call_debug:
			if (ACTION_UNCOVER_DEBUG_PRINTING_ENABLED())
			{
				ai_debug_describe_actor(actor_index, NONE, TRUE, temporary, NUMBEROF(temporary));
				error(2, call->a ? "%s: inspected pursuit location" : "%s: unable to see target's current location",
					temporary);
			}
			break;

		case _action_uncover_call_change_firing_position:
			call->result = actor_change_firing_position(
				actor_index,
				action_uncover_selection.selected_firing_position_index,
				&action_uncover_selection.candidate,
				action_uncover_selection.previous_owner_actor_index,
				&action_uncover_selection.workspace,
				action_uncover_selection.position_flags);
			break;

		case _action_uncover_call_discard_firing_position:
			actor_discard_firing_position(actor_index, (short)call->a, FALSE);
			break;

		case _action_uncover_call_done:
			if (ACTION_UNCOVER_DEBUG_PRINTING_ENABLED())
			{
				char reason[256];

				ai_debug_describe_actor(actor_index, NONE, TRUE, temporary, NUMBEROF(temporary));
				if (state_data->uncover_remaining_time == 0)
				{
					sprintf(reason, "timer %d finished", state_data->uncover_desired_time);
				}
				else if (state_data->uncover_exit_persistent_timer >= 12*TICKS_PER_SECOND)
				{
					sprintf(reason, "persistent timer %d", state_data->uncover_exit_persistent_timer);
				}
				else if (state_data->pursuit_location.type == _pursuit_location_position && state_data->pursuit_location_inspected)
				{
					csstrcpy(reason, "location inspected");
				}
				else
				{
					csstrcpy(reason, "<unknown reason>");
				}
				error(
					2,
					"%s: %s uncover done: %s",
					temporary,
					state_data->pursuit_location.type == _pursuit_location_target ? "target" : "pursuit",
					reason);
			}
			break;
	}
	halo_sim_ai_event_end(call, opened);

	return;
}

static long action_uncover_call_simple(
	long actor_index,
	long kind,
	long a,
	long b,
	real *output)
{
	struct halo_sim_ai_call call;

	memset(&call, 0, sizeof(call));
	call.kind = kind;
	call.actor = actor_index;
	call.a = a;
	call.b = b;
	action_uncover_call(&call);
	if (output)
		*output = call.output[0];

	return call.result;
}

boolean action_uncover_setup_target(
	long actor_index,
	boolean able_to_search,
	struct uncover_state_data *state_data)
{
	struct actor_datum *actor = actor_get(actor_index);
	boolean success = FALSE;

	match_assert("c:\\halo\\SOURCE\\ai\\action_uncover.c", 34, state_data);
	csmemset(state_data, 0, sizeof(*state_data));
	if (!actor->input.vehicle_passenger && !actor->meta.swarm)
	{
		state_data->pursuit_location.type = _pursuit_location_target;
		state_data->able_to_search = able_to_search;
		success = TRUE;
	}
	return success;
}

boolean action_uncover_setup_pursuit(
	long actor_index,
	short firing_position_index,
	struct uncover_state_data *state_data)
{
	struct actor_datum *actor = actor_get(actor_index);
	boolean success = FALSE;

	match_assert("c:\\halo\\SOURCE\\ai\\action_uncover.c", 56, state_data);
	csmemset(state_data, 0, sizeof(*state_data));
	if (!actor->input.vehicle_passenger &&
		!actor->meta.swarm &&
		actor->meta.encounter_index != NONE &&
		firing_position_index != NONE)
	{
		struct encounter_definition *encounter = TAG_BLOCK_GET_ELEMENT(
			&global_scenario_get()->ai_encounters,
			DATUM_INDEX_TO_ABSOLUTE_INDEX(actor->meta.encounter_index),
			struct encounter_definition);
		struct firing_position_definition *firing_position = TAG_BLOCK_GET_ELEMENT(
			&encounter->firing_positions,
			firing_position_index,
			struct firing_position_definition);

		state_data->pursuit_location.firing_position_index = firing_position_index;
		state_data->pursuit_location.type = _pursuit_location_position;
		state_data->pursuit_location.position = firing_position->position;
		state_data->pursuit_location.surface_index = firing_position->surface_index;
		state_data->pursuit_location.cluster_index = firing_position->cluster_index;
		state_data->pursuit_location_inspected = FALSE;
		state_data->able_to_search = TRUE;
		actor->state.searching = TRUE;
		success = TRUE;
	}
	return success;
}

boolean action_uncover_perform(
	long actor_index)
{
	struct actor_datum *actor = actor_get(actor_index);
	struct uncover_state_data *state_data = &actor->state.action_data.uncover;

	match_assert("c:\\halo\\SOURCE\\ai\\action_uncover.c", 132, !actor->meta.swarm);
	if (actor->meta.timeslice &&
		!actor->input.vehicle_passenger &&
		!state_data->uncover_done)
	{
		struct firing_position_evaluation_context search;
		struct firing_position candidate;
		long previous_owner_actor_index = NONE;
		boolean position_valid = FALSE;
		short selected_firing_position_index;

		/* port: the search's calls are its own (actor_firing_position_call),
		the change too */
		csmemset(&search, 0, sizeof(search));
		search.evaluation_mode = _firing_point_evaluation_mode_uncover;
		if (state_data->pursuit_location.type == _pursuit_location_position)
		{
			search.specific_target_enable = TRUE;
			search.specific_target_point = state_data->pursuit_location.position;
			search.specific_target_surface_index = state_data->pursuit_location.surface_index;
			search.specific_target_cluster_index = state_data->pursuit_location.cluster_index;
		}
		else
		{
			search.use_last_visible_target_position = state_data->no_target_sight_available;
		}
		selected_firing_position_index = actor_active_select_firing_position(
			actor_index,
			&search,
			&candidate,
			&previous_owner_actor_index,
			NULL,
			&position_valid);
		if (selected_firing_position_index != NONE)
		{
			short line_of_sight = candidate.line_of_sight;

			if (state_data->pursuit_location.type == _pursuit_location_target)
			{
				if (line_of_sight != _ai_line_of_sight_clear &&
					line_of_sight != _ai_line_of_sight_occluded)
				{
					if (!state_data->no_target_sight_available)
						action_uncover_call_simple(actor_index, _action_uncover_call_debug, 0, 0, NULL);
					state_data->no_target_sight_available = TRUE;
				}
			}
			else if (line_of_sight == _ai_line_of_sight_clear)
			{
				real tolerance;

				action_uncover_call_simple(actor_index, _action_uncover_call_tolerance, 0, 0, &tolerance);
				if (tolerance > candidate.path_distance_from_actor)
				{
					if (!state_data->pursuit_location_inspected)
						action_uncover_call_simple(actor_index, _action_uncover_call_debug, 1, 0, NULL);
					state_data->pursuit_location_inspected = TRUE;
				}
			}
		}
		if (actor_firing_position_port_change(actor_index, selected_firing_position_index, previous_owner_actor_index,
			position_valid) == NONE)
		{
			state_data->uncover_exit_failure = TRUE;
		}
	}
	return FALSE;
}

void action_uncover_control(
	long actor_index)
{
	struct actor_datum *actor = actor_get(actor_index);
	struct actor_definition *definition = actor_definition_get(actor->meta.definition_index);
	struct uncover_state_data *state_data = &actor->state.action_data.uncover;

	if (actor->target.target_prop_index != NONE)
	{
		struct prop_datum *prop = prop_get(actor->target.target_prop_index);
		boolean force_shoot = FALSE;

		if (state_data->pursuit_location.type == _pursuit_location_target)
		{
			if (actor->input.vehicle_gunner_bombardment)
			{
				actor->orders.combat.shoot_at_target = TRUE;
				actor->orders.combat.bombard_target = TRUE;
				force_shoot = TRUE;
			}
			else if (TEST_FLAG(definition->flags, _actor_definition_shoot_at_targets_last_location_bit))
			{
				actor->orders.combat.shoot_at_target = actor->target.target_type >= 5;
			}
			else
			{
				actor->orders.combat.shoot_at_target = actor->target.target_type >= 6;
			}
		}

		if ((actor->orders.combat.shoot_at_target &&
			(prop->line_of_sight == _ai_line_of_sight_clear ||
				prop->line_of_sight == _ai_line_of_sight_occluded)) || force_shoot)
		{
			actor->orders.look.primary_priority = 7;
		}
		else if (actor->target.target_type < 5)
		{
			actor->orders.look.primary_priority = 3;
		}
		else if (prop->line_of_sight != _ai_line_of_sight_from_cover &&
			prop->line_of_sight != _ai_line_of_sight_obstructed)
		{
			actor->orders.look.primary_priority = 5;
		}
		else
		{
			actor->orders.look.primary_priority = 2;
		}

		if (state_data->pursuit_location.type == _pursuit_location_target)
		{
			actor->orders.look.primary_direction.type = 2;
		}
		else if (state_data->pursuit_location.type == _pursuit_location_position)
		{
			actor->orders.look.primary_direction.type = 3;
			actor->orders.look.primary_direction.point = state_data->pursuit_location.position;
		}
	}

	actor->orders.look.idle_look_type = 3;
	actor->orders.move.stationary_crouch = state_data->sneaking;
	actor->orders.move.moving_crouch = state_data->sneaking;
	actor->orders.move.panicked = FALSE;
	actor->orders.move.dive_into_cover = FALSE;
	actor->orders.move.emerge_from_cover = TRUE;
	return;
}

void action_uncover_flush_position_indices(
	long actor_index)
{
	struct uncover_state_data *state_data = &actor_get(actor_index)->state.action_data.uncover;

	if (state_data->pursuit_location.type == _pursuit_location_position)
	{
		state_data->pursuit_location.firing_position_index = NONE;
		state_data->uncover_done = TRUE;
	}
	return;
}

void action_uncover_flush_structure_indices(
	long actor_index)
{
	struct uncover_state_data *state_data = &actor_get(actor_index)->state.action_data.uncover;

	state_data->pursuit_location.cluster_index = NONE;
	state_data->pursuit_location.surface_index = NONE;
	return;
}

void action_uncover_modify_color(
	long actor_index,
	real_argb_color *color)
{
	struct uncover_state_data *state_data = &actor_get(actor_index)->state.action_data.uncover;

	if (state_data->sneaking)
	{
		*color = *global_real_argb_blue;
	}
	else
	{
		*color = *global_real_argb_lightblue;
	}
	return;
}

/* port: the begin's debug message, for actions.c's
actions_port_handler_call */
void action_uncover_port_debug(
	long actor_index,
	real lower_bound,
	real upper_bound,
	real uncover_time)
{
	char temporary[256];
	struct actor_datum *actor = actor_get(actor_index);
	struct uncover_state_data *state_data = &actor->state.action_data.uncover;

	if (ACTION_UNCOVER_DEBUG_PRINTING_ENABLED())
	{
		ai_debug_describe_actor(actor_index, NONE, TRUE, temporary, NUMBEROF(temporary));
		error(
			2,
			"%s: begin %s uncover for [%.1f-%.1f] = %.1f (%sable to search)",
			temporary,
			state_data->pursuit_location.type == _pursuit_location_target ? "target" : "pursuit",
			lower_bound,
			upper_bound,
			uncover_time,
			state_data->able_to_search ? "" : "un");
	}

	return;
}

void action_uncover_begin(
	long actor_index)
{
	char temporary[256];
	struct actor_datum *actor = actor_get(actor_index);
	struct actor_definition *definition = actor_definition_get(actor->meta.definition_index);
	struct uncover_state_data *state_data = &actor->state.action_data.uncover;
	real lower_bound = definition->pursuit.uncover_delay_time_lower_bound;
	real upper_bound = definition->pursuit.uncover_delay_time_upper_bound;
	real uncover_time;

	if (!state_data->able_to_search)
	{
		lower_bound = MAX(lower_bound, definition->pursuit.target_location_time_lower_bound);
		upper_bound = MAX(upper_bound, definition->pursuit.target_location_time_upper_bound);
	}
	{
		/* port: through actions.c's actions_port_handler_call, as the Rust
		port's asks for it */
		struct halo_sim_ai_call call;

		memset(&call, 0, sizeof(call));
		call.kind = _actions_port_handler_call_real_seed_random_range;
		call.actor = actor_index;
		call.a = 0;
		call.input[0] = lower_bound;
		call.input[1] = upper_bound;
		actions_port_handler_call(&call);
		uncover_time = call.output[0];
	}
	state_data->uncover_desired_time = (long)(uncover_time*TICKS_PER_SECOND);
	state_data->uncover_remaining_time = state_data->uncover_desired_time;

	{
		/* port: through actions.c's actions_port_handler_call, as the Rust
		port's asks for it */
		struct halo_sim_ai_call call;

		memset(&call, 0, sizeof(call));
		call.kind = _actions_port_handler_call_uncover_debug;
		call.actor = actor_index;
		call.a = 0;
		call.input[0] = lower_bound;
		call.input[1] = upper_bound;
		call.input[2] = uncover_time;
		actions_port_handler_call(&call);
	}

	if (state_data->pursuit_location.type == _pursuit_location_target &&
		actor->target.target_prop_index != NONE &&
		actor->state.combat_status < 3)
	{
		struct prop_datum *prop = prop_get(actor->target.target_prop_index);

		{
			/* port: through actions.c's actions_port_handler_call, as the Rust
			port's asks for it */
			struct halo_sim_ai_call call;

			memset(&call, 0, sizeof(call));
			call.kind = _actions_port_handler_call_communication;
			call.actor = actor_index;
			call.a = 21;
			call.b = prop->unit_index;
			call.input[0] = (real)NONE;
			actions_port_handler_call(&call);
		}
	}
	return;
}

void action_uncover_update(
	long actor_index)
{
	char actor_name[256];
	char reason[256];
	struct actor_datum *actor = actor_get(actor_index);
	struct uncover_state_data *state_data = &actor->state.action_data.uncover;
	struct actor_definition *definition;
	boolean done;
	boolean target_visible;

	if (state_data->uncover_done)
	{
		return;
	}
	definition = actor_definition_get(actor->meta.definition_index);
	done = TRUE;
	target_visible = FALSE;
	state_data->sneaking = FALSE;
	if (state_data->pursuit_location.type == _pursuit_location_target)
	{
		if (definition->defensive.defensive_crouch_type == 4)
		{
			state_data->sneaking = actor->target.target_type != 6;
		}
		else if (TEST_FLAG(definition->flags, _actor_definition_sneak_uncovering_target_bit) &&
			actor->target.target_type == 5 &&
			prop_get(actor->target.target_prop_index)->quantized_distance <= 2)
		{
			state_data->sneaking = TRUE;
		}
	}
	else if (state_data->pursuit_location.type == _pursuit_location_position)
	{
		if (definition->defensive.defensive_crouch_type == 4 ||
			(TEST_FLAG(definition->flags, _actor_definition_sneak_uncovering_pursuit_position_bit) &&
				distance_squared3d(
					&actor->input.position.body_position,
					&state_data->pursuit_location.position) < 100.0f))
		{
			state_data->sneaking = TRUE;
		}
	}

	if (actor->control.moving)
	{
		state_data->current_position_hold_timer = 0;
	}
	else
	{
		state_data->current_position_hold_timer += 1;
		if (state_data->pursuit_location.type == _pursuit_location_target &&
			state_data->current_position_hold_timer >= TICKS_PER_SECOND)
		{
			/* port: the discard's writes are Rust's own too, not a call */
			actor_discard_firing_position(actor_index, actor->firing_positions.current_position_index, FALSE);
		}
	}

	if (state_data->pursuit_location.type == _pursuit_location_target)
	{
		if (actor->target.target_prop_index != NONE)
		{
			target_visible = prop_get(actor->target.target_prop_index)->visibility > 0;
			if (target_visible && actor->target.target_type < 5)
			{
				done = FALSE;
			}
			else
			{
				done = TRUE;
			}
		}
	}
	else
	{
		done = !state_data->pursuit_location_inspected;
	}

	if (actor->firing_positions.current_position_index == NONE)
	{
		state_data->uncover_exit_failure = TRUE;
	}
	else if (done &&
		(actor->input.vehicle_gunner_bombardment || target_visible || actor->control.moving))
	{
		state_data->uncover_remaining_time = state_data->uncover_desired_time;
		goto test_uncover_timers;
	}
	if (state_data->uncover_remaining_time > 0)
	{
		state_data->uncover_remaining_time -= 1;
	}
	state_data->uncover_exit_persistent_timer += 1;

test_uncover_timers:
	if (state_data->uncover_remaining_time != 0 &&
		state_data->uncover_exit_persistent_timer < 12*TICKS_PER_SECOND)
	{
		done = FALSE;
	}
	else
	{
		done = TRUE;
	}
	if (state_data->pursuit_location.type == _pursuit_location_position && state_data->pursuit_location_inspected)
	{
		done = TRUE;
	}

	if (done)
	{
		action_uncover_call_simple(actor_index, _action_uncover_call_done, 0, 0, NULL);
	}
	state_data->uncover_done = done;
	return;
}
