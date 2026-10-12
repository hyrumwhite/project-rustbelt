/*
ACTION_GUARD.C

symbols in this file:
00003E80 0080:
	_action_guard_setup_current_position (0000)
00003F00 00d0:
	_action_guard_setup_find_position (0000)
00003FD0 0160:
	_action_guard_setup_postcombat (0000)
00004130 0040:
	_action_guard_begin (0000)
00004170 0040:
	_action_guard_end (0000)
000041B0 0140:
	_action_guard_speak_post_combat (0000)
000042F0 01d0:
	_action_guard_update (0000)
000044C0 0080:
	_action_guard_flush_position_indices (0000)
00004540 0030:
	_action_guard_flush_structure_indices (0000)
00004570 00a0:
	_action_guard_modify_color (0000)
00004610 0050:
	_action_guard_replace_prop (0000)
00004660 01c0:
	_action_guard_setup_from_combat_transition (0000)
00004820 01c0:
	_action_guard_setup_from_fleeing (0000)
000049E0 01c0:
	_action_guard_perform (0000)
00004BA0 03d0:
	_action_guard_control (0000)
002431D8 0021:
	??_C@_0CB@GJJCDDDH@c?3?2halo?2SOURCE?2ai?2action_guard?4c@ (0000)
002431FC 0018:
	??_C@_0BI@NGADMAPA@state_data?9?$DOpost_combat?$AA@ (0000)
00243214 0034:
	??_C@_0DE@PILBLNDH@actor?9?$DOstimuli?4combat_transition@ (0000)
00243248 0021:
	??_C@_0CB@CNNLKLII@actor?9?$DOstimuli?4combat_transition@ (0000)
0024326C 0004:
	__real@41100000 (0000)
*/

/* ---------- headers */

#include "cseries.h"
#include "actions.h"

#include "actors.h"
#include "actor_definitions.h"
#include "ai_communication.h"
#include "props.h"
#include "game/game_allegiance.h"
#include "halo_sim.h" /* port: port/linux/game/halo_sim.c */

/* ---------- constants */

enum
{
	_actor_guard_crouch_when_inactive_bit = 6,
	_actor_guard_crouch_when_uncovered_bit = 7,
	_actor_guard_crouch_when_cowering_bit = 23,
};

/* ---------- macros */

/*
 * January cachebeta layout evidence.  The inherited HCEX prop and actor
 * control layouts differ at these fields; keep the accessors local until the
 * shared structures are corrected independently.
 */
#define action_guard_prop_is_dead(prop) (*(boolean *)((byte *)(prop) + 0x60))
#define action_guard_actor_is_moving(actor) (*(boolean *)((byte *)(actor) + 0x484))
#define action_guard_cower_retreat_timer(actor) (*(short *)((byte *)(actor) + 0x3A8))

/* ---------- structures */

/* ---------- prototypes */

/* ---------- globals */

/* ---------- public code */

boolean
action_guard_setup_current_position(
	long actor_index,
	struct guard_state_data *state_data)
{
	struct actor_datum *actor = actor_get(actor_index);

	match_assert("c:\\halo\\SOURCE\\ai\\action_guard.c", 114, state_data);
	csmemset(state_data, 0, sizeof(*state_data));
	state_data->guard_location_type = 1;
	state_data->has_guard_direction = TRUE;
	state_data->guard_direction = actor->input.facing_vector;
	state_data->guard_look_prop_index = NONE;

	return TRUE;
}

boolean
action_guard_setup_find_position(
	long actor_index,
	long wait_ticks,
	struct guard_state_data *state_data)
{
	struct actor_datum *actor = actor_get(actor_index);

	match_assert("c:\\halo\\SOURCE\\ai\\action_guard.c", 134, state_data);
	csmemset(state_data, 0, sizeof(*state_data));

	if (!actor->input.vehicle_passenger && !actor->meta.swarm)
	{
		state_data->wait_ticks = wait_ticks;
		if ((short)wait_ticks == 0)
		{
			state_data->find_new_guard_position = TRUE;
			state_data->guard_location_type = 0;
		}
		else
		{
			state_data->guard_location_type = 1;
			state_data->has_guard_direction = TRUE;
			state_data->guard_direction = actor->input.facing_vector;
		}
	}
	else
	{
		state_data->guard_location_type = 1;
	}

	state_data->guard_look_prop_index = NONE;

	return TRUE;
}

boolean
action_guard_setup_postcombat(
	long actor_index,
	struct guard_state_data *state_data)
{
	struct actor_datum *actor = actor_get(actor_index);
	boolean result = TRUE;

	match_assert("c:\\halo\\SOURCE\\ai\\action_guard.c", 172, state_data);
	csmemset(state_data, 0, sizeof(*state_data));
	state_data->wait_ticks = 120;
	state_data->guard_location_type = 1;
	state_data->post_combat = TRUE;
	state_data->guard_look_prop_index = NONE;

	if (actor->input.vehicle_driver_type == 4)
	{
		result = FALSE;
	}
	else if (!actor->input.vehicle_passenger &&
		!actor->meta.swarm &&
		actor->external_orders.postcombat_prop_index != NONE)
	{
		struct prop_datum *prop = prop_get(actor->external_orders.postcombat_prop_index);

		state_data->guard_look_prop_index = actor->external_orders.postcombat_prop_index;
		state_data->look_ticks = 120;
		state_data->guard_look_until_reached_point = TRUE;

		switch (actor->external_orders.postcombat_type)
		{
		case _actor_postcombat_run_to:
			state_data->guard_point.radius = 2.f;
			break;
		case _actor_postcombat_check_enemy:
		case _actor_postcombat_check_friend:
			state_data->guard_point.radius = 1.f;
			break;
		case _actor_postcombat_shoot_corpse:
			state_data->guard_point.radius = 1.5f;
			break;
		default:
			goto done;
		}

		{
			/* port: through actions.c's actions_port_handler_call, as the
			Rust port's asks for it */
			struct halo_sim_ai_call find;

			memset(&find, 0, sizeof(find));
			find.kind = _actions_port_handler_call_find_prop_pathfinding_location;
			find.actor = actor_index;
			find.a = actor->external_orders.postcombat_prop_index;
			actions_port_handler_call(&find);
		}
		state_data->guard_location_type = 2;
		state_data->guard_point.position = prop->pathfinding_point;
		state_data->guard_point.surface_index = prop->pathfinding_surface_index;
	}

done:
	return result;
}

boolean
action_guard_setup_from_combat_transition(
	long actor_index,
	struct guard_state_data *state_data)
{
	struct actor_datum *actor = actor_get(actor_index);

	match_assert("c:\\halo\\SOURCE\\ai\\action_guard.c", 32, state_data);
	csmemset(state_data, 0, sizeof(*state_data));
	state_data->wait_ticks = (short)actor->stimuli.combat_transition_guard_timer;

	if (!actor->input.vehicle_passenger && !actor->meta.swarm)
	{
		match_assert(
			"c:\\halo\\SOURCE\\ai\\action_guard.c",
			45,
			actor->stimuli.combat_transition);

		/* port: the positions through actions.c's actions_port_handler_call,
		as the Rust port's asks for them */
		struct halo_sim_ai_call nearby;

		memset(&nearby, 0, sizeof(nearby));
		nearby.kind = _actions_port_handler_call_nearby_firing_positions_at;
		nearby.actor = actor_index;
		nearby.a = actor->stimuli.combat_transition_guard_point_surface_index;
		memcpy(nearby.input, &actor->stimuli.combat_transition_guard_point, sizeof(real_point3d));
		if (actor->stimuli.combat_transition_guard_at_point &&
			(actions_port_handler_call(&nearby), nearby.result))
		{
			state_data->guard_location_type = 2;
			state_data->guard_point.position = actor->stimuli.combat_transition_guard_point;
			state_data->guard_point.surface_index = actor->stimuli.combat_transition_guard_point_surface_index;
			state_data->guard_point.radius = actor->stimuli.combat_transition_guard_point_distance;
		}
		else if (state_data->wait_ticks > 0)
		{
			state_data->guard_location_type = 1;
			state_data->has_guard_direction = actor->stimuli.combat_transition_has_vector;
			state_data->aim_in_guard_direction = FALSE;
			if (state_data->has_guard_direction)
			{
				state_data->guard_direction = actor->stimuli.combat_transition_vector;
				if (normalize3d(&state_data->guard_direction) == 0.f)
				{
					state_data->has_guard_direction = FALSE;
				}
			}
		}
		else
		{
			state_data->guard_location_type = 0;
			state_data->find_new_guard_position = TRUE;
		}
	}
	else
	{
		state_data->guard_location_type = 1;
	}

	if (actor->stimuli.combat_transition == 2)
	{
		match_assert(
			"c:\\halo\\SOURCE\\ai\\action_guard.c",
			90,
			actor->stimuli.combat_transition_prop_index != NONE);
		state_data->shout_about_dead_friend = TRUE;
		state_data->shout_dead_friend_prop_index = actor->stimuli.combat_transition_prop_index;
	}

	state_data->guard_look_prop_index = actor->stimuli.combat_transition_prop_index;
	if (state_data->guard_look_prop_index != NONE)
	{
		state_data->look_ticks = (short)actor->stimuli.combat_transition_prop_look_timer;
		state_data->guard_look_until_reached_point = actor->stimuli.combat_transition_prop_look_while_moving;
	}

	return TRUE;
}

boolean
action_guard_setup_from_fleeing(
	long actor_index,
	struct flee_state_data *flee_state,
	struct guard_state_data *state_data)
{
	struct actor_datum *actor = actor_get(actor_index);
	struct actor_definition *definition = actor_definition_get(actor->meta.definition_index);

	match_assert("c:\\halo\\SOURCE\\ai\\action_guard.c", 237, state_data);
	csmemset(state_data, 0, sizeof(*state_data));

	if (!actor->input.vehicle_passenger && !actor->meta.swarm)
	{
		state_data->wait_ticks = 0;
		state_data->cower = TRUE;
		state_data->cower_ticks = 0;
		state_data->cower_panicked = flee_state->panic_type > 0;
		state_data->cower_from_retreat =
			action_guard_cower_retreat_timer(actor) > 0 &&
			!state_data->cower_panicked;

		if (!state_data->cower_from_retreat)
		{
			if (state_data->cower_panicked)
			{
				real upper_bound = definition->panic.cower_time_upper_bound;
				real lower_bound = definition->panic.cower_time_lower_bound;
				struct halo_sim_ai_call draw;

				/* port: the draw through actions.c's actions_port_handler_call,
				as the Rust port's asks for it */
				memset(&draw, 0, sizeof(draw));
				draw.kind = _actions_port_handler_call_real_seed_random_range;
				draw.actor = actor_index;
				draw.input[0] = lower_bound;
				draw.input[1] = upper_bound;
				actions_port_handler_call(&draw);
				state_data->cower_ticks = (short)(draw.output[0] * 30.f);
			}
			else
			{
				real upper_bound = definition->defensive.hide_time_upper_bound;
				real lower_bound = definition->defensive.hide_time_lower_bound;
				struct halo_sim_ai_call draw;

				/* port: the draw through actions.c's actions_port_handler_call,
				as the Rust port's asks for it */
				memset(&draw, 0, sizeof(draw));
				draw.kind = _actions_port_handler_call_real_seed_random_range;
				draw.actor = actor_index;
				draw.input[0] = lower_bound;
				draw.input[1] = upper_bound;
				actions_port_handler_call(&draw);
				state_data->cower_ticks = (short)(draw.output[0] * 30.f);
			}
		}

		if (flee_state->flee_firing_position_index == NONE)
		{
			state_data->find_new_guard_position = TRUE;
			state_data->guard_location_type = 0;
		}
		else
		{
			state_data->find_new_guard_position = FALSE;
			state_data->guard_location_type = 3;
			state_data->guard_firing_position_index = flee_state->flee_firing_position_index;
			if (flee_state->has_approach_point)
			{
				state_data->has_guard_direction = TRUE;
				state_data->aim_in_guard_direction = TRUE;
				vector_from_points3d(
					&actor->input.position.body_position,
					&flee_state->approach_point,
					&state_data->guard_direction);
				if (normalize3d(&state_data->guard_direction) == 0.f)
				{
					state_data->has_guard_direction = FALSE;
				}
			}
		}
	}
	else
	{
		state_data->guard_location_type = 1;
	}

	state_data->guard_look_prop_index = NONE;

	return TRUE;
}

void
action_guard_begin(
	long actor_index)
{
	struct actor_datum *actor = actor_get(actor_index);
	struct guard_state_data *state_data = &actor->state.action_data.guard;

	{
		/* port: through actions.c's actions_port_handler_call, as the Rust
		port's asks for it */
		struct halo_sim_ai_call call;

		memset(&call, 0, sizeof(call));
		call.kind = _actions_port_handler_call_forget_recent_damage;
		call.actor = actor_index;
		call.a = 0;
		actions_port_handler_call(&call);
	}
	actor->state.searching = FALSE;
	if (state_data->cower_from_retreat)
	{
		/* port: through actions.c's actions_port_handler_call, as the Rust
		port's asks for it */
		struct halo_sim_ai_call call;

		memset(&call, 0, sizeof(call));
		call.kind = _actions_port_handler_call_retreat_successful;
		call.actor = actor_index;
		call.a = 0;
		actions_port_handler_call(&call);
	}

	return;
}

void
action_guard_end(
	long actor_index)
{
	struct actor_datum *actor = actor_get(actor_index);

	if (actor->state.action_data.guard.post_combat)
	{
		actor->external_orders.postcombat_type = _actor_postcombat_none;
		actor->external_orders.postcombat_prop_index = NONE;
	}

	return;
}

void
action_guard_flush_position_indices(
	long actor_index)
{
	struct actor_datum *actor = actor_get(actor_index);
	struct guard_state_data *state_data = &actor->state.action_data.guard;

	if (state_data->cower && state_data->guard_location_type == 3)
	{
		state_data->cower = FALSE;
		state_data->cower_ticks = 0;
		state_data->cower_from_retreat = FALSE;
	}

	if (state_data->guard_location_type == 3 ||
		(state_data->guard_location_type == 1 && !actor->input.vehicle_passenger))
	{
		state_data->guard_location_type = 0;
		state_data->guard_firing_position_index = NONE;
		state_data->find_new_guard_position = TRUE;
	}

	return;
}

void
action_guard_flush_structure_indices(
	long actor_index)
{
	struct actor_datum *actor = actor_get(actor_index);
	struct guard_state_data *state_data = &actor->state.action_data.guard;

	if (state_data->guard_location_type == 2)
	{
		state_data->guard_point.surface_index = NONE;
	}

	return;
}

void
action_guard_modify_color(
	long actor_index,
	real_argb_color *color)
{
	struct guard_state_data *state_data = &actor_get(actor_index)->state.action_data.guard;

	if (!state_data->cower)
	{
		*color = *global_real_argb_salmon;
	}
	else if (state_data->cower_from_retreat)
	{
		*color = *global_real_argb_violet;
	}
	else if (state_data->cower_panicked)
	{
		*color = *global_real_argb_pink;
	}
	else
	{
		*color = *global_real_argb_purple;
	}

	return;
}

void
action_guard_replace_prop(
	long actor_index,
	long old_prop_index,
	long new_prop_index)
{
	struct guard_state_data *state_data = &actor_get(actor_index)->state.action_data.guard;

	if (state_data->guard_look_prop_index == old_prop_index)
	{
		state_data->guard_look_prop_index = new_prop_index;
	}

	if (state_data->shout_dead_friend_prop_index == old_prop_index)
	{
		state_data->shout_dead_friend_prop_index = new_prop_index;
		if (new_prop_index == NONE)
		{
			state_data->shout_about_dead_friend = FALSE;
		}
	}

	return;
}

/* ---------- private code */

/* action_guard_speak_post_combat */
/* port: what the guard action asks of the rest of the game, shared by the
game's and the Rust port's (crates/halo_map's actions); in check mode each is
recorded */
void action_guard_call(
	struct halo_sim_ai_call *call)
{
	int opened = halo_sim_ai_event_begin(call);
	long actor_index = call->actor;
	struct actor_datum *actor = actor_get(actor_index);
	struct guard_state_data *state_data = &actor->state.action_data.guard;

	switch (call->kind)
	{
		case _action_guard_call_team_is_ally:
			call->result = game_team_is_ally((short)call->a, (short)call->b) ? 1 : 0;
			break;

		case _action_guard_call_communication:
			ai_communication_event(
				(short)call->a,
				actor->meta.unit_index,
				call->b,
				(long)call->input[0],
				NONE,
				NONE,
				NULL);
			break;

		case _action_guard_call_wake:
			actor_set_dormant(actor_index, FALSE);
			break;

		case _action_guard_call_communication_target:
			ai_communication_event(
				(short)call->a,
				actor->meta.unit_index,
				actor_target_unit_index(actor_index),
				NONE,
				NONE,
				NONE,
				NULL);
			break;

		case _action_guard_call_discard_firing_position:
			actor_discard_firing_position(actor_index, (short)call->a, FALSE);
			break;

		case _action_guard_call_change_firing_position:
		{
			boolean position_flags;
			long previous_owner_actor_index;
			short selected_firing_position_index;
			struct firing_position candidate;
			struct firing_position_evaluation_context search;
			struct path_state workspace;

			csmemset(&search, 0, sizeof(search));
			search.evaluation_mode = _firing_point_evaluation_mode_guard;
			search.allowed_position_mask = actor_get_firing_position_group(
				actor_index,
				_firing_point_evaluation_mode_guard,
				_firing_position_group_normal);
			search.allow_outside_range = TRUE;
			selected_firing_position_index = actor_select_firing_position(
				actor_index,
				&search,
				&candidate,
				&previous_owner_actor_index,
				&workspace,
				&position_flags);
			call->result = actor_change_firing_position(
				actor_index,
				selected_firing_position_index,
				&candidate,
				previous_owner_actor_index,
				&workspace,
				position_flags);
			break;
		}

		case _action_guard_call_random_range:
			call->output[0] = real_seed_random_range(get_global_random_seed_address(), call->input[0], call->input[1]);
			break;

		case _action_guard_call_shout_dead_friend:
		{
			struct prop_datum *prop = prop_get(call->a);

			actor_stimulus_suspicion(actor_index, 2, 600);
			ai_communication_event(
				7,
				actor->meta.unit_index,
				prop->unit_index,
				NONE,
				NONE,
				2,
				NULL);
			break;
		}
	}
	halo_sim_ai_event_end(call, opened);

	return;
}

static long action_guard_call_simple(
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
	action_guard_call(&call);
	if (output)
		*output = call.output[0];

	return call.result;
}

static boolean action_guard_speak_post_combat(
	long actor_index)
{
	struct actor_datum *actor = actor_get(actor_index);
	struct guard_state_data *state_data = &actor->state.action_data.guard;
	boolean result = FALSE;

	match_assert("c:\\halo\\SOURCE\\ai\\action_guard.c", 344, state_data->post_combat);
	if (!state_data->post_combat_vocalized)
	{
		short communication_type = NONE;

		switch (actor->external_orders.postcombat_type)
		{
		case _actor_postcombat_speak_alone:
			communication_type = 48;
			break;
		case _actor_postcombat_speak_unscathed:
			communication_type = 49;
			break;
		case _actor_postcombat_speak_wounded:
			communication_type = 50;
			break;
		case _actor_postcombat_speak_massacre:
			communication_type = 51;
			break;
		case _actor_postcombat_speak_triumph:
			communication_type = 52;
			break;
		case _actor_postcombat_check_enemy:
			communication_type = 53;
			break;
		case _actor_postcombat_check_friend:
			communication_type = 54;
			break;
		case _actor_postcombat_shoot_corpse:
			communication_type = 55;
			break;
		case _actor_postcombat_celebrate:
			communication_type = 56;
			break;
		}

		if (communication_type != NONE)
		{
			long prop_unit_index = NONE;
			long prop_type = NONE;

			if (state_data->guard_look_prop_index != NONE)
			{
				struct prop_datum *prop = prop_get(state_data->guard_look_prop_index);

				prop_unit_index = prop->unit_index;
				if (action_guard_prop_is_dead(prop))
				{
					prop_type = action_guard_call_simple(actor_index, _action_guard_call_team_is_ally,
						actor->meta.team_index, prop->team_index, 0.0f, 0.0f, NULL) ? 4 : 3;
				}
				else
				{
					prop_type = 2;
				}
			}

			action_guard_call_simple(actor_index, _action_guard_call_communication,
				communication_type, prop_unit_index, (real)prop_type, 0.0f, NULL);
			result = TRUE;
		}

		state_data->post_combat_vocalized = TRUE;
	}

	return result;
}

void
action_guard_update(
	long actor_index)
{
	struct actor_datum *actor = actor_get(actor_index);
	struct guard_state_data *state_data = &actor->state.action_data.guard;

	if (!actor->meta.dormant &&
		action_guard_actor_is_moving(actor) &&
		state_data->wait_ticks > 0)
	{
		state_data->wait_ticks--;
		if (state_data->wait_ticks == 0 &&
			!actor->input.vehicle_passenger &&
			!actor->meta.swarm)
		{
			if (state_data->post_combat)
			{
				action_guard_speak_post_combat(actor_index);
				actor->external_orders.postcombat_type = _actor_postcombat_none;
				actor->external_orders.postcombat_prop_index = NONE;
				state_data->post_combat = FALSE;
				state_data->post_combat_shooting = FALSE;
				state_data->guard_look_prop_index = NONE;
			}
			state_data->find_new_guard_position = TRUE;
		}
	}

	if (state_data->look_ticks > 0 &&
		(!state_data->guard_look_until_reached_point || action_guard_actor_is_moving(actor)))
	{
		state_data->look_ticks--;
		if (state_data->look_ticks == 0)
		{
			state_data->guard_look_prop_index = NONE;
		}
	}

	if (state_data->path_begun && action_guard_actor_is_moving(actor))
	{
		boolean cower_finished;

		if (state_data->cower_from_retreat)
		{
			state_data->cower_from_retreat = action_guard_cower_retreat_timer(actor) > 0;
			cower_finished = !state_data->cower_from_retreat;
		}
		else if (state_data->cower_ticks > 0)
		{
			state_data->cower_ticks--;
			cower_finished = state_data->cower_ticks == 0;
		}
		else
		{
			cower_finished = FALSE;
		}

		if (cower_finished)
		{
			action_guard_call_simple(actor_index, _action_guard_call_wake, 0, 0, 0.0f, 0.0f, NULL);
			state_data->cower = FALSE;
			state_data->cower_panicked = FALSE;
			state_data->cower_from_retreat = FALSE;
			state_data->cower_ticks = 0;

			if (actor->state.combat_status >= 2 && actor->meta.unit_index != NONE)
			{
				action_guard_call_simple(actor_index, _action_guard_call_communication_target, 35, 0, 0.0f, 0.0f, NULL);
			}

			if (state_data->guard_location_type == 3)
			{
				/* port: the discard's writes are Rust's own too, not a call */
				actor_discard_firing_position(actor_index, state_data->guard_firing_position_index, FALSE);
				state_data->guard_firing_position_index = NONE;
			}

			actor->firing_positions.current_position_index = NONE;
			if (actor->input.vehicle_passenger)
			{
				state_data->guard_location_type = 1;
				return;
			}

			state_data->guard_location_type = 0;
			state_data->find_new_guard_position = TRUE;
		}
	}

	return;
}

boolean
action_guard_perform(
	long actor_index)
{
	boolean position_flags;
	long previous_owner_actor_index;
	short selected_firing_position_index;
	real guard_position_time_lower_bound;
	real guard_position_time_upper_bound;
	short firing_position_index;
	struct firing_position candidate;
	struct firing_position_evaluation_context search;
	struct path_state workspace;
	struct actor_datum *actor = actor_get(actor_index);
	struct actor_definition *definition = actor_definition_get(actor->meta.definition_index);
	struct guard_state_data *state_data = &actor->state.action_data.guard;

	if (actor->meta.swarm)
	{
		state_data->guard_location_type = 1;
		return FALSE;
	}

	if (actor->input.vehicle_passenger)
	{
		state_data->guard_location_type = 1;
		state_data->find_new_guard_position = TRUE;
		return FALSE;
	}

	if (state_data->guard_location_type == 3 &&
		actor->firing_positions.current_position_index == NONE)
	{
		state_data->guard_location_type = 0;
		state_data->find_new_guard_position = TRUE;
	}

	if (actor->meta.timeslice && state_data->find_new_guard_position)
	{
		if (state_data->guard_location_type == 3 &&
			actor->firing_positions.current_position_index != NONE)
		{
			/* port: the discard's writes are Rust's own too, not a call */
			actor_discard_firing_position(actor_index, actor->firing_positions.current_position_index, FALSE);
		}

		/* port: the selection's calls are its own */
		firing_position_index = actor_firing_position_port_change_to_guard_selection(actor_index);
		state_data->find_new_guard_position = FALSE;
		state_data->has_guard_direction = FALSE;
		if (firing_position_index == NONE)
		{
			state_data->guard_location_type = 1;
		}
		else
		{
			state_data->guard_location_type = 3;
			state_data->guard_firing_position_index = firing_position_index;
		}

		guard_position_time_upper_bound = definition->firing_position.guard_position_time_upper_bound;
		guard_position_time_lower_bound = definition->firing_position.guard_position_time_lower_bound;
		{
			real guard_position_time;

			action_guard_call_simple(actor_index, _action_guard_call_random_range, 0, 0,
				guard_position_time_lower_bound, guard_position_time_upper_bound, &guard_position_time);
			state_data->wait_ticks = (short)(guard_position_time * 30.f);
		}
	}

	return FALSE;
}

void
action_guard_control(
	long actor_index)
{
	struct actor_datum *actor = actor_get(actor_index);
	struct actor_definition *definition = actor_definition_get(actor->meta.definition_index);
	struct guard_state_data *state_data = &actor->state.action_data.guard;

	if (TEST_FLAG(definition->flags, _actor_guard_crouch_when_inactive_bit) &&
		actor->state.combat_status == 0)
	{
		actor->orders.move.stationary_crouch = TRUE;
		actor->orders.move.moving_crouch = TRUE;
	}
	else
	{
		actor->orders.move.moving_crouch = FALSE;
		if (state_data->cower)
		{
			if (state_data->cower_from_retreat)
			{
				actor->orders.move.stationary_crouch =
					TEST_FLAG(definition->flags, _actor_guard_crouch_when_cowering_bit);
			}
			else
			{
				actor->orders.move.stationary_crouch = TRUE;
			}
		}
		else
		{
			actor->orders.move.stationary_crouch =
				TEST_FLAG(definition->flags, _actor_guard_crouch_when_uncovered_bit) &&
				actor->state.combat_status > 0;
		}
	}

	actor->orders.move.panicked = FALSE;
	actor->orders.move.dive_into_cover = FALSE;
	actor->orders.move.emerge_from_cover = FALSE;

	if (actor->meta.timeslice && !actor->meta.swarm)
	{
		boolean path_succeeded = FALSE;

		switch (state_data->guard_location_type)
		{
		case 0:
		case 1:
			actor_move_halt(actor_index);
			path_succeeded = TRUE;
			break;

		case 2:
		{
			real distance_squared = distance_squared3d(
				&actor->input.position.body_position,
				&state_data->guard_point.position);

			if (distance_squared < state_data->guard_point.radius * state_data->guard_point.radius)
			{
				actor_move_halt(actor_index);
			}
			else
			{
				actor_move_to_point(actor_index, &state_data->guard_point.position, state_data->guard_point.surface_index, NONE);
			}

			path_succeeded = distance_squared < 9.f;
			break;
		}

		case 3:
		{
			if (state_data->guard_firing_position_index != NONE)
			{
				actor->firing_positions.current_position_index = state_data->guard_firing_position_index;
				actor->firing_positions.current_position_found_outside_range = FALSE;
				if (!actor_move_to_firing_position(actor_index, actor->firing_positions.current_position_index, NULL))
				{
					/* port: the discard's writes are Rust's own too, not a call */
					actor_discard_firing_position(actor_index, state_data->guard_firing_position_index, FALSE);
					actor->firing_positions.current_position_index = NONE;
				}
			}

			path_succeeded = !actor->control.path.path.valid ||
				distance_squared3d(
					&actor->control.path.path.endpoint.point,
					&actor->input.position.body_position) < 9.f;
			break;
		}

		default:
			display_assert(NULL, "c:\\halo\\SOURCE\\ai\\action_guard.c", 685, TRUE);
			system_exit(-1);
			break;
		}

		state_data->path_begun = TRUE;
		if (path_succeeded)
		{
			if (state_data->shout_about_dead_friend)
			{
				long prop_index = state_data->shout_dead_friend_prop_index;

				state_data->shout_about_dead_friend = FALSE;
				state_data->shout_dead_friend_prop_index = NONE;
				action_guard_call_simple(actor_index, _action_guard_call_shout_dead_friend, prop_index, 0, 0.0f, 0.0f, NULL);
			}

			if (state_data->post_combat)
			{
				action_guard_speak_post_combat(actor_index);
				if (actor->external_orders.postcombat_type == _actor_postcombat_shoot_corpse &&
					state_data->guard_location_type == 2)
				{
					state_data->post_combat_shooting = TRUE;
				}
			}
		}
	}

	if (state_data->post_combat_shooting)
	{
		real_vector3d const *up;

		actor->orders.look.primary_priority = 7;
		actor->orders.look.primary_direction.type = _direction_specification_target;
		actor->orders.combat.shoot_at_target = TRUE;
		actor->orders.combat.use_manual_target_point = TRUE;
		up = global_up3d;
		actor->orders.combat.target_point.x =
			state_data->guard_point.position.x + up->i * 0.05f;
		actor->orders.combat.target_point.y =
			state_data->guard_point.position.y + up->j * 0.05f;
		actor->orders.combat.target_point.z =
			state_data->guard_point.position.z + up->k * 0.05f;
	}
	else if (state_data->guard_look_prop_index != NONE)
	{
		actor->orders.look.primary_priority = 5;
		actor->orders.look.primary_direction.type = _direction_specification_prop;
		actor->orders.look.primary_direction.prop_index = state_data->guard_look_prop_index;
	}
	else if (state_data->has_guard_direction)
	{
		actor->orders.look.primary_priority = state_data->aim_in_guard_direction ? 5 : 3;
		actor->orders.look.primary_direction.type = _direction_specification_vector;
		actor->orders.look.primary_direction.vector = state_data->guard_direction;
	}
	else if (actor->state.combat_status > 0 &&
		actor->target.target_prop_index != NONE)
	{
		actor->orders.look.primary_priority = 3;
		actor->orders.look.primary_direction.type = _direction_specification_prop;
		actor->orders.look.primary_direction.prop_index = actor->target.target_prop_index;
	}
	else
	{
		actor->orders.look.primary_priority = _primary_priority_none;
	}

	actor->orders.look.idle_look_type = actor->state.combat_status >= 4 ? 4 : 2;

	return;
}
