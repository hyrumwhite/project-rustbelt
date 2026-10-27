/*
ACTION_AVOID.C

symbols in this file:
00000EC0 0040:
	_action_avoid_setup (0000)
00000F00 0010:
	_action_avoid_begin (0000)
00000F10 0010:
	_action_avoid_end (0000)
00000F20 00c0:
	_action_avoid_perform (0000)
00000FE0 0010:
	_action_avoid_update (0000)
00000FF0 0090:
	_action_avoid_control (0000)
00242F3C 0021:
	??_C@_0CB@LDCPOHMO@c?3?2halo?2SOURCE?2ai?2action_avoid?4c@ (0000)
*/

/* ---------- headers */

#include "cseries.h"
#include "actions.h"

#include "actors.h"
#include "halo_sim.h" /* port: port/linux/game/halo_sim.c */

/* ---------- constants */

enum
{
	_action_avoid_primary_target_priority = 7,
	_action_avoid_primary_danger_priority = 5,
	_action_avoid_idle_look_type = 4,
};

/* ---------- macros */

/* ---------- structures */

/* ---------- prototypes */

/* ---------- globals */

/* ---------- public code */

/* port: what the action asks of the rest of the game, shared by the game's
and the Rust port's (crates/halo_map's actions); in check mode each is
recorded */
void action_avoid_call(
	struct halo_sim_ai_call *call)
{
	int opened = halo_sim_ai_event_begin(call);
	long actor_index = call->actor;
	struct actor_datum *actor = actor_get(actor_index);

	switch (call->kind)
	{
		case _action_avoid_call_change_firing_position:
		{
			boolean position_flags;
			long previous_owner_actor_index;
			short firing_position_index;
			struct firing_position candidate;
			struct firing_position_evaluation_context search;
			struct path_state workspace;

			memset(&search, 0, sizeof(search));
			search.evaluation_mode = _firing_point_evaluation_mode_avoid;
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
	}
	halo_sim_ai_event_end(call, opened);

	return;
}

static long action_avoid_call_simple(
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
	action_avoid_call(&call);
	if (output)
		*output = call.output[0];

	return call.result;
}


boolean action_avoid_setup(
	long actor_index,
	struct avoid_state_data *state_data)
{
	match_assert("c:\\halo\\SOURCE\\ai\\action_avoid.c", 30, state_data);
	memset(state_data, 0, sizeof(*state_data));
	return TRUE;
}

void
action_avoid_begin(
	long actor_index)
{
	return;
}

void
action_avoid_end(
	long actor_index)
{
	return;
}

boolean action_avoid_perform(
	long actor_index)
{
	boolean position_flags;
	long previous_owner_actor_index;
	short firing_position_index;
	struct firing_position candidate;
	struct firing_position_evaluation_context search;
	struct path_state workspace;
	struct actor_datum *actor = actor_get(actor_index);

	match_assert("c:\\halo\\SOURCE\\ai\\action_avoid.c", 55, !actor->meta.swarm);
	if (actor->meta.timeslice)
	{
		/* port: the selection's calls are its own */
		actor_firing_position_port_change_to_active_selection(actor_index, _firing_point_evaluation_mode_avoid);
	}

	return actor->danger_zone.danger_type == 0;
}

void
action_avoid_update(
	long actor_index)
{
}

void
action_avoid_control(
	long actor_index)
{
	struct actor_datum *actor = actor_get(actor_index);

	if (actor->target.target_type >= _actor_target_uninspected_orphan)
	{
		actor->orders.combat.shoot_at_target = TRUE;
		actor->orders.look.primary_priority = _action_avoid_primary_target_priority;
	}
	else
	{
		actor->orders.look.primary_priority = _action_avoid_primary_danger_priority;
		if (actor->danger_zone.danger_type > 0)
		{
			actor->orders.look.primary_direction.type = _direction_specification_danger;
			goto finish_control;
		}
	}
	actor->orders.look.primary_direction.type = _direction_specification_target;

finish_control:
	actor->orders.look.idle_look_type = _action_avoid_idle_look_type;
	actor->orders.move.stationary_crouch = actor->emotions.defensive_crouch;
	actor->orders.move.moving_crouch = FALSE;
	actor->orders.move.panicked = FALSE;
	actor->orders.move.dive_into_cover = FALSE;
	actor->orders.move.emerge_from_cover = FALSE;
}

/* ---------- private code */
