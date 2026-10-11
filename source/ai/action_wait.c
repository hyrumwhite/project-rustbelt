/*
ACTION_WAIT.C

symbols in this file:
0000A4B0 0010:
	_action_wait_begin (0000)
0000A4C0 0180:
	_action_wait_perform (0000)
0000A640 00b0:
	_action_wait_control (0000)
0000A6F0 00b0:
	_action_wait_setup (0000)
0000A7A0 00e0:
	_action_wait_update (0000)
00243B18 0004:
	__real@41000000 (0000)
00243B1C 0020:
	??_C@_0CA@JCBCAIHB@c?3?2halo?2SOURCE?2ai?2action_wait?4c?$AA@ (0000)
*/

/* ---------- headers */

#include "cseries.h"
#include "actions.h"

#include "actors.h"
#include "ai_communication.h"
#include "props.h"
#include "game/game.h"
#include "halo_sim.h" /* port: port/linux/game/halo_sim.c */

/* ---------- constants */

/* ---------- macros */

/*
 * actors.h currently overstates the path-control block by four bytes.  The
 * target reads the `moving` flag at actor+0x504; keep this evidence-bounded
 * accessor local until the shared path layout is corrected as its own change.
 */
#define actor_wait_is_moving(actor) (*(boolean *)((byte *)(actor) + 0x504))

/* ---------- structures */

/* ---------- prototypes */

/* ---------- globals */

/* ---------- public code */

/* port: what the action asks of the rest of the game, shared by the game's
and the Rust port's (crates/halo_map's actions); in check mode each is
recorded */
void action_wait_call(
	struct halo_sim_ai_call *call)
{
	int opened = halo_sim_ai_event_begin(call);
	long actor_index = call->actor;
	struct actor_datum *actor = actor_get(actor_index);

	switch (call->kind)
	{
		case _action_wait_call_find_nearby_actors:
			actor_pursuit_find_nearby_actors(actor_index, (boolean)call->a);
			break;

		case _action_wait_call_move_to_prop:
			call->result = actor_move_to_prop(actor_index, call->a, 8.0f) ? 1 : 0;
			break;

		case _action_wait_call_move_halt:
			actor_move_halt(actor_index);
			break;

		case _action_wait_call_communication:
			ai_communication_event((short)call->a, actor->meta.unit_index, NONE, NONE, NONE, NONE, NULL);
			break;

		case _action_wait_call_query_time:
			call->result = seed_random_range(get_global_random_seed_address(), 300, 600);
			break;
	}
	halo_sim_ai_event_end(call, opened);

	return;
}

static long action_wait_call_simple(
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
	action_wait_call(&call);
	if (output)
		*output = call.output[0];

	return call.result;
}


void
action_wait_begin(
	long actor_index)
{
	return;
}

boolean
action_wait_perform(
	long actor_index)
{
	struct actor_datum *actor = actor_get(actor_index);
	struct wait_state_data *state_data = &actor->state.action_data.wait;

	if (actor->meta.timeslice)
	{
		state_data->desire_move = FALSE;
		action_wait_call_simple(actor_index, _action_wait_call_find_nearby_actors,
			actor->external_orders.pursuit_is_coordinator, 0, 0.0f, 0.0f, NULL);
		if (state_data->waiting_as_coordinator)
		{
			if (actor->external_orders.pursuit_group_prop_index == NONE)
			{
				if (state_data->exit_timer == 0)
					state_data->exit_timer = 150;
			}
			else if (game_time_get() >= state_data->entry_time + 2700)
			{
				state_data->wait_done = TRUE;
			}
		}
		else
		{
			state_data->wait_done = TRUE;
			if (actor->external_orders.pursuit_group_prop_index != NONE)
			{
				struct prop_datum *prop = prop_get(actor->external_orders.pursuit_group_prop_index);

				if ((!state_data->was_actively_searching || state_data->move_failed) &&
					(prop->visibility < 2 || !(prop->distance < 8.0f)))
				{
					state_data->wait_done = TRUE;
				}
				else
				{
					state_data->desire_move = !state_data->move_failed && prop->distance > 3.5f;
					state_data->wait_done = FALSE;
				}
			}
		}

		if (!actor->meta.swarm)
		{
			if (state_data->desire_move)
			{
				if (!action_wait_call_simple(actor_index, _action_wait_call_move_to_prop,
					actor->external_orders.pursuit_group_prop_index, 0, 0.0f, 0.0f, NULL))
					state_data->move_failed = TRUE;
			}
			else
			{
				action_wait_call_simple(actor_index, _action_wait_call_move_halt, 0, 0, 0.0f, 0.0f, NULL);
			}
		}
	}

	return state_data->wait_done;
}

void
action_wait_control(
	long actor_index)
{
	struct actor_datum *actor = actor_get(actor_index);

	if (actor_wait_is_moving(actor))
	{
		actor->orders.look.primary_priority = 3;
		actor->orders.look.primary_direction.type = _direction_specification_movement;
	}
	else if (!actor->external_orders.pursuit_is_coordinator &&
		actor->external_orders.pursuit_group_prop_index != NONE &&
		actor->state.action_data.wait.look_timer > 0)
	{
		actor->orders.look.primary_priority = 5;
		actor->orders.look.primary_direction.type = _direction_specification_prop;
		actor->orders.look.primary_direction.prop_index = actor->external_orders.pursuit_group_prop_index;
	}
	else
	{
		actor->orders.look.primary_priority = 1;
	}

	actor->orders.look.idle_look_type = 3;
	actor->orders.combat.shoot_at_target = FALSE;
	actor->orders.move.stationary_crouch = FALSE;
	actor->orders.move.moving_crouch = FALSE;
	actor->orders.move.panicked = FALSE;
	actor->orders.move.dive_into_cover = FALSE;
	actor->orders.move.emerge_from_cover = FALSE;
	return;
}

boolean action_wait_setup(
	long actor_index,
	boolean was_actively_searching,
	struct wait_state_data *state_data)
{
	struct actor_datum *actor = actor_get(actor_index);
	boolean result = FALSE;

	match_assert("c:\\halo\\SOURCE\\ai\\action_wait.c", 33, state_data);
	memset(state_data, 0, sizeof(*state_data));
	if (!actor->input.vehicle_passenger)
	{
		state_data->waiting_as_coordinator = actor->external_orders.pursuit_is_coordinator;
		state_data->was_actively_searching = was_actively_searching;
		state_data->entry_time = game_time_get();
		state_data->exit_timer = 0;
		state_data->look_timer = 120;
		state_data->desire_move = TRUE;
		state_data->query_timer = seed_random_range(get_global_random_seed_address(), 300, 600);
		result = TRUE;
	}

	return result;
}

void
action_wait_update(
	long actor_index)
{
	struct actor_datum *actor = actor_get(actor_index);

	if (actor->state.action_data.wait.query_timer > 0)
	{
		actor->state.action_data.wait.query_timer--;
		if (actor->state.action_data.wait.query_timer == 0)
		{
			if (actor->meta.unit_index != NONE)
			{
				action_wait_call_simple(actor_index, _action_wait_call_communication, 17, 0, 0.0f, 0.0f, NULL);
			}
			actor->state.action_data.wait.query_timer = (short)action_wait_call_simple(actor_index,
				_action_wait_call_query_time, 0, 0, 0.0f, 0.0f, NULL);
		}
	}

	if (actor->state.action_data.wait.exit_timer > 0)
	{
		actor->state.action_data.wait.exit_timer--;
		if (actor->state.action_data.wait.exit_timer == 0)
		{
			if (actor->state.action_data.wait.waiting_as_coordinator && actor->meta.unit_index != NONE)
			{
				action_wait_call_simple(actor_index, _action_wait_call_communication, 20, 0, 0.0f, 0.0f, NULL);
			}
			actor->state.action_data.wait.wait_done = TRUE;
		}
	}

	if (!actor->state.action_data.wait.desire_move && actor->state.action_data.wait.look_timer > 0)
	{
		actor->state.action_data.wait.look_timer--;
	}
	return;
}

/* ---------- private code */
