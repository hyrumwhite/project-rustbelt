/*
ACTION_CONVERSE.C

symbols in this file:
000028A0 00b0:
	_action_converse_setup (0000)
00002950 0010:
	_action_converse_begin (0000)
00002960 0100:
	_action_converse_perform (0000)
00002A60 0020:
	_action_converse_update (0000)
00002A80 0090:
	_action_converse_control (0000)
00002B10 0030:
	_action_converse_replace_prop (0000)
00002B40 0080:
	_actor_conversation_control (0000)
00002BC0 0030:
	_actor_conversation_end (0000)
00002BF0 0030:
	_action_converse_end (0000)
0024305C 0024:
	??_C@_0CE@NNMIMHKJ@c?3?2halo?2SOURCE?2ai?2action_convers@ (0000)
*/

/* ---------- headers */

#include "cseries.h"
#include "actions.h"

#include "actors.h"
#include "ai/ai_scenario_definitions.h"
#include "ai_communication.h"
#include "props.h"
#include "scenario/scenario.h"
#include "scenario/scenario_definitions.h"
#include "units/units.h"
#include "halo_sim.h" /* port: port/linux/game/halo_sim.c */

/* ---------- constants */

/* ---------- macros */

/* ---------- structures */

typedef char scenario_conversation_definition_size_assert[
	sizeof(struct ai_conversation) == 0x74 ? 1 : -1];

/* ---------- prototypes */

/* ---------- globals */

/* ---------- public code */

boolean action_converse_setup(
	long actor_index,
	long conversation_index,
	struct converse_state_data *state_data)
{
	struct conversation_datum *conversation;
	struct ai_conversation *definition;

	actor_get(actor_index);
	conversation = ai_conversation_header_get(conversation_index);
	definition = TAG_BLOCK_GET_ELEMENT(
		&global_scenario_get()->ai_conversations,
		conversation->scenario_conversation_index,
		struct ai_conversation);

	match_assert("c:\\halo\\SOURCE\\ai\\action_converse.c", 33, state_data);
	csmemset(state_data, 0, sizeof(*state_data));
	state_data->conversation_index = conversation_index;
	state_data->run_to_distance = definition->run_to_player_dist;
	state_data->run_to_unit_index = state_data->run_to_distance == 0.0f
		? NONE
		: conversation->triggering_player_unit_index;
	state_data->run_to_prop_index = NONE;
	state_data->in_range = FALSE;

	return TRUE;
}

void action_converse_begin(
	long actor_index)
{
	return;
}

/* port: what the converse action asks of the rest of the game, shared by
the game's and the Rust port's (crates/halo_map's actions); in check mode
each is recorded */
void action_converse_call(
	struct halo_sim_ai_call *call)
{
	int opened = halo_sim_ai_event_begin(call);
	long actor_index = call->actor;
	struct actor_datum *actor = actor_get(actor_index);
	struct converse_state_data *state_data = &actor->state.action_data.converse;

	switch (call->kind)
	{
		case _action_converse_call_base_prop:
			call->result = prop_get_base_by_unit_index(actor_index, call->a, TRUE, TRUE);
			break;

		case _action_converse_call_move_halt:
			actor_move_halt(actor_index);
			break;

		case _action_converse_call_move_to_prop:
			call->result = actor_move_to_prop(actor_index, call->a, call->input[0]) ? 1 : 0;
			break;

		case _action_converse_call_player_prop:
		{
			struct conversation_datum *conversation = ai_conversation_header_get(state_data->conversation_index);

			call->result = conversation && conversation->triggering_player_unit_index != NONE
				? prop_get_active_by_unit_index(actor_index, conversation->triggering_player_unit_index)
				: NONE;
			break;
		}
	}
	halo_sim_ai_event_end(call, opened);

	return;
}

static long action_converse_call_simple(
	long actor_index,
	long kind,
	long a,
	real input)
{
	struct halo_sim_ai_call call;

	memset(&call, 0, sizeof(call));
	call.kind = kind;
	call.actor = actor_index;
	call.a = a;
	call.input[0] = input;
	action_converse_call(&call);

	return call.result;
}

boolean action_converse_perform(
	long actor_index)
{
	struct actor_datum *actor = actor_get(actor_index);
	struct converse_state_data *state_data = &actor->state.action_data.converse;

	if (actor->meta.timeslice)
	{
		if (state_data->run_to_prop_index == NONE &&
			state_data->run_to_unit_index != NONE)
		{
			state_data->run_to_prop_index = action_converse_call_simple(actor_index,
				_action_converse_call_base_prop, state_data->run_to_unit_index, 0.0f);
		}

		if (state_data->run_to_prop_index == NONE)
		{
			state_data->failed = TRUE;
		}
		else
		{
			if (!state_data->in_range)
			{
				struct prop_datum *prop = prop_get(state_data->run_to_prop_index);

				if (prop->visibility >= _actor_perception_full &&
					prop->distance < state_data->run_to_distance)
				{
					state_data->in_range = TRUE;
				}
				else if (prop->distance < 0.7f)
				{
					state_data->in_range = TRUE;
				}
			}

			if (state_data->in_range)
			{
				action_converse_call_simple(actor_index, _action_converse_call_move_halt, 0, 0.0f);
			}
			else if (!action_converse_call_simple(actor_index, _action_converse_call_move_to_prop,
				state_data->run_to_prop_index, state_data->run_to_distance))
			{
				state_data->failed = TRUE;
			}
		}
	}

	return state_data->failed;
}

void action_converse_update(
	long actor_index)
{
	actor_get(actor_index);

	return;
}

void action_converse_control(
	long actor_index)
{
	struct actor_datum *actor = actor_get(actor_index);
	struct converse_state_data *state_data = &actor->state.action_data.converse;
	long prop_index = NONE;

	/* port: the conversation's player's prop through action_converse_call
	(its header read only where it is used) */
	if (state_data->run_to_prop_index != NONE)
	{
		prop_index = state_data->run_to_prop_index;
	}
	else if (state_data->conversation_index != NONE)
	{
		prop_index = action_converse_call_simple(actor_index, _action_converse_call_player_prop, 0, 0.0f);
	}

	actor->orders.look.idle_look_type = 1;

	if (prop_index != NONE)
	{
		actor->orders.look.primary_priority = _primary_priority_facing;
		actor->orders.look.primary_direction.type = _direction_specification_prop;
		actor->orders.look.primary_direction.prop_index = prop_index;
	}

	return;
}

void action_converse_replace_prop(
	long actor_index,
	long old_prop_index,
	long new_prop_index)
{
	struct actor_datum *actor = actor_get(actor_index);
	struct converse_state_data *state_data = &actor->state.action_data.converse;

	if (state_data->run_to_prop_index == old_prop_index)
		state_data->run_to_prop_index = new_prop_index;

	return;
}

/* port: what an actor's conversation asks of the rest of the game, shared
by the game's and the Rust port's (halo_sim.c); in check mode each is
recorded */
void actor_conversation_call(
	struct halo_sim_ai_call *call)
{
	int opened = halo_sim_ai_event_begin(call);

	switch (call->kind)
	{
		case _actor_conversation_call_active_prop:
			call->result = prop_get_active_by_unit_index(call->actor, call->a);
			break;

		case _actor_conversation_call_head_position:
			unit_get_head_position(call->a, (real_point3d *)call->output);
			break;

		case _actor_conversation_call_look:
		{
			/* port: the direction starts from zeros (the game's left the
			union's rest of a prop direction as the stack's leftovers, and
			actor_look_secondary copies all of it into the actor) */
			struct direction_specification direction;

			memset(&direction, 0, sizeof(direction));
			direction.type = (short)call->a;
			if (direction.type == _direction_specification_prop)
				direction.prop_index = call->b;
			else
				memcpy(&direction.point, call->input, sizeof(direction.point));
			actor_look_secondary(
				call->actor,
				_secondary_look_communicating_prop,
				_secondary_look_priority_turn_and_aim,
				&direction);
			break;
		}
	}
	halo_sim_ai_event_end(call, opened);

	return;
}

static void actor_conversation_control_game(
	long actor_index);

/* port: the Rust port's in place of the game's when asked (halo_sim.c); in
check mode the game's runs and is compared */
void actor_conversation_control(
	long actor_index)
{
	int checking = FALSE;

	if (halo_sim_ai_step_begin(_halo_sim_ai_step_conversation, actor_index, &checking))
		return;
	actor_conversation_control_game(actor_index);
	if (checking)
		halo_sim_ai_step_check(_halo_sim_ai_step_conversation, 0);

	return;
}

/* the game's, what it asks through actor_conversation_call */
static void actor_conversation_control_game(
	long actor_index)
{
	struct actor_datum *actor = actor_get(actor_index);

	if (actor->external_orders.conversation_index != NONE &&
		actor->external_orders.conversation_attention_unit_index != NONE)
	{
		struct halo_sim_ai_call call;

		memset(&call, 0, sizeof(call));
		call.kind = _actor_conversation_call_active_prop;
		call.actor = actor_index;
		call.a = actor->external_orders.conversation_attention_unit_index;
		actor_conversation_call(&call);
		if (call.result != NONE)
		{
			long prop_index = call.result;

			memset(&call, 0, sizeof(call));
			call.kind = _actor_conversation_call_look;
			call.actor = actor_index;
			call.a = _direction_specification_prop;
			call.b = prop_index;
		}
		else
		{
			real_point3d head_position;

			memset(&call, 0, sizeof(call));
			call.kind = _actor_conversation_call_head_position;
			call.actor = actor_index;
			call.a = actor->external_orders.conversation_attention_unit_index;
			actor_conversation_call(&call);
			memcpy(&head_position, call.output, sizeof(head_position));

			memset(&call, 0, sizeof(call));
			call.kind = _actor_conversation_call_look;
			call.actor = actor_index;
			call.a = _direction_specification_point;
			memcpy(call.input, &head_position, sizeof(head_position));
		}
		actor_conversation_call(&call);
	}

	return;
}

void actor_conversation_end(
	long actor_index)
{
	struct actor_datum *actor = actor_get(actor_index);

	if (actor->external_orders.conversation_index != NONE)
	{
		ai_conversation_finish(
			actor->external_orders.conversation_index,
			FALSE,
			FALSE);
	}

	return;
}

void action_converse_end(
	long actor_index)
{
	struct actor_datum *actor = actor_get(actor_index);

	if (actor->external_orders.conversation_index != NONE)
	{
		{
			/* port: through actions.c's actions_port_handler_call, as the Rust
			port's asks for it */
			struct halo_sim_ai_call call;

			memset(&call, 0, sizeof(call));
			call.kind = _actions_port_handler_call_conversation_finish;
			call.actor = actor_index;
			call.a = actor->external_orders.conversation_index;
			actions_port_handler_call(&call);
		}
	}

	return;
}

/* ---------- private code */
