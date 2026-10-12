/*
ACTION_FLEE.C

symbols in this file:
00003140 0030:
	_action_flee_end (0000)
00003170 0030:
	_action_flee_flush_position_indices (0000)
000031A0 0070:
	_action_flee_modify_color (0000)
00003210 0280:
	_code_00003210 (0000)
00003490 0160:
	_code_00003490 (0000)
000035F0 0030:
	_action_flee_replace_prop (0000)
00003620 0020:
	_action_flee_blind_panic (0000)
00003640 0110:
	_action_flee_setup (0000)
00003750 0060:
	_action_flee_begin (0000)
000037B0 00a0:
	_action_flee_update (0000)
00003850 0190:
	_action_flee_control (0000)
000039E0 0140:
	_code_000039e0 (0000)
00003B20 0360:
	_action_flee_perform (0000)
002430A4 0020:
	??_C@_0CA@MCHAFBCO@c?3?2halo?2SOURCE?2ai?2action_flee?4c?$AA@ (0000)
002430C4 0004:
	__real@3ecccccd (0000)
002430C8 00d8:
	??_C@_0NI@KCCMPDJP@?$CI?$CBactor?9?$DOmeta?4timeslice?5?$CG?$CG?5state@ (0000)
002431A0 0037:
	??_C@_0DH@GFKJLMEI@actor?9?$DOfiring_positions?4current_@ (0000)
*/

/* ---------- headers */

#include "cseries.h"
#include "actions.h"
#include "actor_definitions.h"
#define PATH_EXTERNAL_FLEE_ROUTINES
#include "actors.h"
#undef PATH_EXTERNAL_FLEE_ROUTINES
#include "ai.h"
#include "ai_communication.h"
#include "ai_scenario_definitions.h"
#include "game/game.h"
#include "math/real_math.h"
#include "path.h"
#include "props.h"
#include "scenario/scenario.h"
#include "scenario/scenario_definitions.h"
#include "units/units.h"
#include "halo_sim.h" /* port: port/linux/game/halo_sim.c */

/* ---------- constants */

enum
{
	_actor_panic_none = 0,
	_actor_panic_friend_leader_type_killed = 8,
	_actor_panic_grenade_attached_to_us = 9,
	_actor_panic_delayed_projectile_attached_to_us = 10,
	_actor_panic_melee_attached_to_us = 11,
	_actor_panic_burning_to_death = 12,
};

enum
{
	_ai_information_flee = 4,
};

enum
{
	/* Later-build symbols authenticate these meanings; January uses this TU's 3/6/7 layout. */
	_action_flee_primary_priority_facing = 3,
	_action_flee_primary_priority_locked_facing = 6,
	_action_flee_primary_priority_locked_aiming = 7,
	_action_flee_idle_look_type = 4,
};

/* ---------- macros */

/* ---------- structures */

/* ---------- prototypes */

static void action_flee_find_flee_position(
	long actor_index,
	boolean respect_direction,
	struct flee_state_data *state_data);
static boolean action_flee_current_position_exposed(
	long actor_index,
	struct flee_state_data *state_data);
static boolean action_flee_at_flee_position(
	long actor_index);

/* ---------- globals */

/* ---------- public code */

void action_flee_end(
	long actor_index)
{
	struct actor_datum *actor = actor_get(actor_index);

	if (actor->meta.unit_index != NONE)
	{
		{
			/* port: through actions.c's actions_port_handler_call, as the Rust
			port's asks for it */
			struct halo_sim_ai_call call;

			memset(&call, 0, sizeof(call));
			call.kind = _actions_port_handler_call_unit_stop_running_blindly;
			call.actor = actor_index;
			call.a = 0;
			actions_port_handler_call(&call);
		}
	}

	return;
}

void action_flee_flush_position_indices(
	long actor_index)
{
	struct flee_state_data *state_data = &actor_get(actor_index)->state.action_data.flee;

	state_data->flee_firing_position_index = NONE;
	state_data->find_new_flee_position = TRUE;

	return;
}

void action_flee_modify_color(
	long actor_index,
	real_argb_color *color)
{
	struct flee_state_data *state_data = &actor_get(actor_index)->state.action_data.flee;

	if (state_data->panic_type > _actor_panic_none)
	{
		*color = *global_real_argb_yellow;
	}
	else
	{
		*color = *global_real_argb_green;
	}

	return;
}

/* respect_direction is not consulted by this build; setup passes FALSE and perform passes TRUE. */
static void action_flee_find_flee_position(
	long actor_index,
	boolean respect_direction,
	struct flee_state_data *state_data)
{
	struct firing_position_evaluation_context search;
	struct firing_position candidate;
	long current_owner_actor_index;
	struct path_state area_path_state;
	boolean area_path_state_valid;
	struct actor_datum *actor = actor_get(actor_index);
	struct actor_definition *definition = actor_definition_get(actor->meta.definition_index);

	match_assert(
		"c:\\halo\\SOURCE\\ai\\action_flee.c",
		522,
		!actor->meta.swarm);
	csmemset(&search, 0, sizeof(search));
	search.use_last_visible_target_position = state_data->flee_from_last_visible_location;
	if (state_data->panic_type > _actor_panic_none)
	{
		search.evaluation_mode = _firing_point_evaluation_mode_panic;
		if (state_data->forced_flee_ticks > 0)
		{
			search.allow_rejected_positions = TRUE;
			search.allow_outside_range = TRUE;
		}
		search.attractor_enable = TRUE;
		search.attractor_weight = 10.0f;
		search.attractor_radius = 6.0f;
	}
	else
	{
		search.evaluation_data.cover.allow_occluded_points = state_data->allow_occluded_points;
		search.evaluation_mode = _firing_point_evaluation_mode_cover;
		search.maximum_search_range = definition->defensive.cover_max_distance > 0.0f ?
			definition->defensive.cover_max_distance : 6.0f;
	}

	search.allowed_position_mask = actor_get_firing_position_group(
		actor_index,
		search.evaluation_mode,
		_firing_position_group_normal);
	state_data->flee_firing_position_index = actor_select_firing_position(
		actor_index,
		&search,
		&candidate,
		&current_owner_actor_index,
		&area_path_state,
		&area_path_state_valid);
	state_data->flee_firing_position_index = actor_change_firing_position(
		actor_index,
		state_data->flee_firing_position_index,
		&candidate,
		current_owner_actor_index,
		&area_path_state,
		area_path_state_valid);
	state_data->flee_firing_position_found_randomly =
		state_data->flee_firing_position_index != NONE && !area_path_state_valid;
	state_data->has_approach_point = FALSE;

	if (state_data->flee_firing_position_index != NONE &&
		state_data->flee_prop_index != NONE)
	{
		struct prop_datum *prop = prop_get(state_data->flee_prop_index);

		if (prop->state >= _prop_state_becoming_unacknowledged &&
			prop->state <= _prop_state_acknowledged)
		{
			actor_perception_find_prop_pathfinding_location(
				actor_index,
				state_data->flee_prop_index);
		}

		{
			long target_object_index = prop->vehicle_index != NONE ?
				prop->vehicle_index : prop->unit_index;
			struct path_input input;
			struct path_state state;
			boolean straight_line;

			path_input_new(
				&input,
				definition->moving.pathfinding_radius,
				FALSE,
				target_object_index);
			path_input_set_start(
				&input,
				&prop->pathfinding_point,
				prop->pathfinding_surface_index);
			path_input_set_target_object(&input, actor->meta.unit_index);
			path_state_new(&input, &state, NULL);
			path_state_destination(
				&state,
				&candidate.definition->position,
				candidate.definition->surface_index,
				0.0f);
			if (path_state_find(&state))
			{
				state_data->has_approach_point = path_state_approach_point(
					&state,
					(real_point2d const *)&candidate.definition->position,
					candidate.definition->surface_index,
					&straight_line,
					&state_data->approach_point);
			}
		}
	}

	state_data->find_new_flee_position = FALSE;
	return;
}

static boolean action_flee_current_position_exposed(
	long actor_index,
	struct flee_state_data *state_data)
{
	struct actor_datum *actor = actor_get(actor_index);
	boolean exposed = FALSE;

	match_assert(
		"c:\\halo\\SOURCE\\ai\\action_flee.c",
		613,
		!actor->meta.swarm);
	if (state_data->flee_prop_index != NONE &&
		actor->meta.encounter_index != NONE &&
		state_data->flee_firing_position_index != NONE)
	{
		struct encounter_definition *encounter = TAG_BLOCK_GET_ELEMENT(
			&global_scenario_get()->ai_encounters,
			DATUM_INDEX_TO_ABSOLUTE_INDEX(actor->meta.encounter_index),
			struct encounter_definition);
		struct prop_datum *prop = prop_get(state_data->flee_prop_index);
		struct firing_position_definition *firing_position = TAG_BLOCK_GET_ELEMENT(
			&encounter->firing_positions,
			state_data->flee_firing_position_index,
			struct firing_position_definition);
		real_point3d estimated_position;
		short line_of_sight;

		unit_estimate_position(
			actor->meta.unit_index,
			_unit_estimate_head_crouching,
			&firing_position->position,
			NULL,
			NULL,
			&estimated_position);
		line_of_sight = ai_test_line_of_sight(
			&estimated_position,
			firing_position->cluster_index,
			&prop->head_position,
			prop->body_location.cluster_index,
			_ai_line_of_sight_expand_source,
			FALSE,
			prop->vehicle_index,
			actor->input.vehicle_index != NONE);
		if (prop->state >= _prop_state_becoming_unacknowledged &&
			prop->state <= _prop_state_acknowledged)
		{
			exposed = line_of_sight == _ai_line_of_sight_clear;
		}
		if (line_of_sight == _ai_line_of_sight_clear ||
			line_of_sight == _ai_line_of_sight_to_cover)
		{
			state_data->has_approach_point = TRUE;
			state_data->approach_point = prop->body_position;
		}
	}

	return exposed;
}

void action_flee_replace_prop(
	long actor_index,
	long invalid_prop_index,
	long replacement_prop_index)
{
	struct flee_state_data *state_data = &actor_get(actor_index)->state.action_data.flee;

	if (state_data->flee_prop_index == invalid_prop_index)
	{
		state_data->flee_prop_index = replacement_prop_index;
	}

	return;
}

/* port: what the flee action asks of the rest of the game, shared by the
game's and the Rust port's (crates/halo_map's actions); in check mode each is
recorded */
void action_flee_call(
	struct halo_sim_ai_call *call)
{
	int opened = halo_sim_ai_event_begin(call);
	long actor_index = call->actor;
	struct actor_datum *actor = actor_get(actor_index);
	struct flee_state_data *state_data = &actor->state.action_data.flee;

	switch (call->kind)
	{
		case _action_flee_call_run_blindly:
			unit_start_running_blindly(actor->meta.unit_index);
			break;

		case _action_flee_call_move_halt:
			actor_move_halt(actor_index);
			break;

		case _action_flee_call_move_to_firing_position:
			call->result = actor_move_to_firing_position(actor_index, (short)call->a, NULL) ? 1 : 0;
			break;

		case _action_flee_call_discard_firing_position:
			actor_discard_firing_position(actor_index, (short)call->a, FALSE);
			break;

		case _action_flee_call_at_destination:
			call->result = actor_path_at_destination(actor_index) ? 1 : 0;
			break;

		case _action_flee_call_tolerance:
			call->output[0] = actor_destination_tolerance(actor_index);
			break;

		case _action_flee_call_situation_status:
			actor_situation_update_target_status(actor_index);
			actor_situation_combat_status_update(actor_index);
			break;

		case _action_flee_call_position_exposed:
			call->result = action_flee_current_position_exposed(actor_index, state_data) ? 1 : 0;
			break;

		case _action_flee_call_find_flee_position:
			action_flee_find_flee_position(actor_index, TRUE, state_data);
			break;

		case _action_flee_call_unit_is_speaking:
			call->result = unit_is_speaking(actor->meta.unit_index) ? 1 : 0;
			break;

		case _action_flee_call_scream:
			unit_scream(actor->meta.unit_index,
				call->a ? _unit_scream_grenade_attached_to_us : _unit_scream_burning_to_death);
			break;

		case _action_flee_call_shout:
			if (!call->a)
			{
				ai_communication_event(
					state_data->panic_type == _actor_panic_friend_leader_type_killed ?
						_ai_communication_flee_leader_died : _ai_communication_flee,
					actor->meta.unit_index,
					call->b,
					NONE,
					NONE,
					_ai_information_flee,
					NULL);
			}
			else
			{
				ai_communication_event(
					_ai_communication_flee_idle,
					actor->meta.unit_index,
					call->b,
					NONE,
					NONE,
					NONE,
					NULL);
			}
			break;
	}
	halo_sim_ai_event_end(call, opened);

	return;
}

static long action_flee_call_simple(
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
	action_flee_call(&call);
	if (output)
		*output = call.output[0];

	return call.result;
}

boolean action_flee_blind_panic(
	short panic_type)
{
	return panic_type >= _actor_panic_grenade_attached_to_us &&
		panic_type <= _actor_panic_burning_to_death;
}

/* port: the draw through actions.c's actions_port_handler_call */
static real action_flee_port_random(
	long actor_index)
{
	struct halo_sim_ai_call draw;

	memset(&draw, 0, sizeof(draw));
	draw.kind = _actions_port_handler_call_random;
	draw.actor = actor_index;
	actions_port_handler_call(&draw);

	return draw.output[0];
}

/* port: the flee position's search for a flee being set up, for actions.c's
actions_port_handler_call */
void action_flee_port_find_flee_position(
	long actor_index,
	struct flee_state_data *state_data)
{
	action_flee_find_flee_position(actor_index, FALSE, state_data);

	return;
}

boolean action_flee_setup(
	long actor_index,
	short panic_type,
	long panic_prop_index,
	boolean force_state_entry,
	boolean flee_from_last_visible_location,
	boolean allow_occluded_points,
	struct flee_state_data *state_data)
{
	struct actor_datum *actor = actor_get(actor_index);
	boolean success = FALSE;

	if (!actor->input.vehicle_passenger)
	{
		match_assert(
			"c:\\halo\\SOURCE\\ai\\action_flee.c",
			44,
			state_data);
		csmemset(state_data, 0, sizeof(*state_data));
		state_data->forced_flee_ticks = force_state_entry ? 6*TICKS_PER_SECOND : 0;
		state_data->flee_firing_position_index = NONE;
		state_data->panic_type = panic_type;
		state_data->flee_from_last_visible_location = flee_from_last_visible_location;
		state_data->allow_occluded_points = allow_occluded_points;
		state_data->flee_prop_index = panic_prop_index;

		/* port: what it asks of the rest of the game through actions.c's
		actions_port_handler_call, as the Rust port's does (crates/halo_map's
		actor_transitions) */
		if (panic_prop_index != NONE)
		{
			struct halo_sim_ai_call target;

			memset(&target, 0, sizeof(target));
			target.kind = _actions_port_handler_call_try_new_target;
			target.actor = actor_index;
			target.a = panic_prop_index;
			actions_port_handler_call(&target);
		}

		if (action_flee_blind_panic(panic_type) &&
			action_flee_port_random(actor_index) < 0.4f)
		{
			state_data->flee_stationary_ticks = 45;
			success = TRUE;
		}
		else if (!actor->meta.swarm)
		{
			struct halo_sim_ai_call find;

			memset(&find, 0, sizeof(find));
			find.kind = _actions_port_handler_call_find_flee_position;
			find.actor = actor_index;
			find.a = panic_type;
			find.b = panic_prop_index;
			find.input[0] = (real)state_data->forced_flee_ticks;
			find.input[1] = (real)(flee_from_last_visible_location != FALSE);
			find.input[2] = (real)(allow_occluded_points != FALSE);
			actions_port_handler_call(&find);
			state_data->flee_firing_position_index = (short)find.result;
			state_data->flee_firing_position_found_randomly = (find.result2 & 1) != 0;
			state_data->has_approach_point = (find.result2 & 2) != 0;
			memcpy(&state_data->approach_point, find.output, sizeof(real_point3d));
			state_data->find_new_flee_position = FALSE;
			if (state_data->flee_firing_position_index != NONE)
			{
				success = TRUE;
			}
			else
			{
				state_data->unable_to_flee = FALSE;
			}
		}
	}

	return success;
}

void action_flee_begin(
	long actor_index)
{
	struct actor_datum *actor = actor_get(actor_index);
	struct flee_state_data *state_data = &actor->state.action_data.flee;

	state_data->flee_timer = 0;
	if (state_data->panic_type > _actor_panic_none)
	{
		actor->state.searching = FALSE;
	}

	if (state_data->flee_stationary_ticks == 0 &&
		actor->meta.unit_index != NONE &&
		action_flee_blind_panic(state_data->panic_type))
	{
		{
			/* port: through actions.c's actions_port_handler_call, as the Rust
			port's asks for it */
			struct halo_sim_ai_call call;

			memset(&call, 0, sizeof(call));
			call.kind = _actions_port_handler_call_unit_start_running_blindly;
			call.actor = actor_index;
			call.a = 0;
			actions_port_handler_call(&call);
		}
	}

	return;
}

void action_flee_update(
	long actor_index)
{
	struct actor_datum *actor = actor_get(actor_index);
	struct flee_state_data *state_data = &actor->state.action_data.flee;

	state_data->flee_timer += 1;
	if (state_data->forced_flee_ticks > 0)
	{
		state_data->forced_flee_ticks -= 1;
	}

	if (state_data->flee_stationary_ticks > 0)
	{
		state_data->flee_stationary_ticks -= 1;
		if (state_data->flee_stationary_ticks == 0 &&
			actor->meta.unit_index != NONE &&
			action_flee_blind_panic(state_data->panic_type))
		{
			action_flee_call_simple(actor_index, _action_flee_call_run_blindly, 0, 0, NULL);
		}
	}

	if (state_data->panic_type > _actor_panic_none)
	{
		actor->emotions.flee_with_friends_disable_time = game_time_get() + 25*TICKS_PER_SECOND;
	}

	return;
}

void action_flee_control(
	long actor_index)
{
	struct actor_datum *actor = actor_get(actor_index);
	struct flee_state_data *state_data = &actor->state.action_data.flee;

	if (state_data->panic_type > _actor_panic_none)
	{
		actor->orders.look.primary_priority = _action_flee_primary_priority_locked_facing;
		actor->orders.look.primary_direction.type = _direction_specification_movement;
		actor->orders.combat.abort_burst = TRUE;
	}
	else if (actor->target.target_prop_index != NONE &&
		prop_get(actor->target.target_prop_index)->visibility > 0)
	{
		actor->orders.look.primary_priority = _action_flee_primary_priority_locked_aiming;
		actor->orders.look.primary_direction.type = _direction_specification_target;
		actor->orders.combat.shoot_at_target = TRUE;
	}
	else if (state_data->flee_prop_index != NONE)
	{
		actor->orders.look.primary_priority = _action_flee_primary_priority_facing;
		actor->orders.look.primary_direction.type = _direction_specification_prop;
		actor->orders.look.primary_direction.prop_index = state_data->flee_prop_index;
	}
	else
	{
		actor->orders.look.primary_priority = _primary_priority_none;
	}
	actor->orders.look.idle_look_type = _action_flee_idle_look_type;

	actor->orders.move.panicked = state_data->panic_type > _actor_panic_none;
	actor->orders.move.flaming = action_flee_blind_panic(state_data->panic_type);
	actor->orders.move.stationary_crouch = TRUE;
	actor->orders.move.moving_crouch = FALSE;
	actor->orders.move.dive_into_cover = TRUE;
	actor->orders.move.emerge_from_cover = FALSE;

	if (state_data->flee_firing_position_index == NONE)
	{
		action_flee_call_simple(actor_index, _action_flee_call_move_halt, 0, 0, NULL);
	}
	else if (actor->meta.timeslice)
	{
		if (action_flee_call_simple(actor_index, _action_flee_call_move_to_firing_position,
			state_data->flee_firing_position_index, 0, NULL))
		{
			actor->firing_positions.current_position_index = state_data->flee_firing_position_index;
			actor->firing_positions.current_position_found_outside_range = state_data->flee_firing_position_found_randomly;
		}
		else
		{
			if (actor->firing_positions.current_position_index != NONE)
			{
				action_flee_call_simple(actor_index, _action_flee_call_discard_firing_position,
					actor->firing_positions.current_position_index, 0, NULL);
				action_flee_call_simple(actor_index, _action_flee_call_move_halt, 0, 0, NULL);
				actor->firing_positions.current_position_index = NONE;
			}
			state_data->flee_firing_position_index = NONE;
			state_data->find_new_flee_position = TRUE;
		}
	}

	return;
}

static boolean action_flee_at_flee_position(
	long actor_index)
{
	struct actor_datum *actor = actor_get(actor_index);
	struct encounter_definition *encounter;
	struct firing_position_definition *firing_position;
	real tolerance;
	boolean result;
	long prop_index;

	match_assert(
		"c:\\halo\\SOURCE\\ai\\action_flee.c",
		470,
		!actor->meta.swarm);
	if (actor->meta.encounter_index == NONE ||
		actor->firing_positions.current_position_index == NONE)
	{
		return FALSE;
	}

	encounter = TAG_BLOCK_GET_ELEMENT(
		&global_scenario_get()->ai_encounters,
		DATUM_INDEX_TO_ABSOLUTE_INDEX(actor->meta.encounter_index),
		struct encounter_definition);
	firing_position = TAG_BLOCK_GET_ELEMENT(
		&encounter->firing_positions,
		actor->firing_positions.current_position_index,
		struct firing_position_definition);
	if (action_flee_call_simple(actor_index, _action_flee_call_at_destination, 0, 0, NULL) &&
		actor->control.path.destination_orders.destination_type == _destination_firing_position &&
		actor->control.path.destination_orders.firing_position_index ==
			actor->firing_positions.current_position_index)
	{
		return TRUE;
	}

	action_flee_call_simple(actor_index, _action_flee_call_tolerance, 0, 0, &tolerance);
	if (!(distance_squared3d(
			&actor->input.position.body_position,
			&firing_position->position) < tolerance*tolerance))
	{
		return FALSE;
	}

	result = FALSE;
	prop_index = actor->state.action_data.flee.flee_prop_index;
	if (prop_index != NONE)
	{
		struct prop_datum *prop = prop_get(prop_index);

		result =
			prop->line_of_sight == _ai_line_of_sight_clear ||
			prop->line_of_sight == _ai_line_of_sight_occluded;
	}

	result = !result;
	return result;
}

boolean action_flee_perform(
	long actor_index)
{
	struct actor_datum *actor = actor_get(actor_index);
	struct flee_state_data *state_data = &actor->state.action_data.flee;

	if (!actor->meta.swarm)
	{
		if (action_flee_blind_panic(state_data->panic_type))
		{
			state_data->forced_flee_ticks = 6*TICKS_PER_SECOND;
		}

		if (state_data->flee_stationary_ticks > 0)
		{
			state_data->flee_firing_position_index = NONE;
		}
		else
		{
			if (state_data->flee_firing_position_index == NONE)
			{
				state_data->find_new_flee_position = TRUE;
			}
			else if (actor->firing_positions.current_position_index == NONE)
			{
				state_data->flee_firing_position_index = NONE;
				state_data->find_new_flee_position = TRUE;
			}
			else if (action_flee_at_flee_position(actor_index))
			{
				match_assert(
					"c:\\halo\\SOURCE\\ai\\action_flee.c",
					152,
					actor->firing_positions.current_position_index != NONE);
				if (state_data->forced_flee_ticks != 0)
				{
					state_data->find_new_flee_position = TRUE;
				}
				else
				{
					long flee_prop_index = state_data->flee_prop_index;

					state_data->flee_firing_position_index =
						actor->firing_positions.current_position_index;
					state_data->flee_firing_position_found_randomly =
						actor->firing_positions.current_position_found_outside_range;
					state_data->done_fleeing = TRUE;
					state_data->find_new_flee_position = FALSE;
					if (flee_prop_index != NONE)
					{
						struct prop_datum *prop = prop_get(flee_prop_index);

						prop->visibility = 0;
						prop->perception = MAX(prop->audibility, prop->ineffability);
						prop->currently_damaging_me = FALSE;
						prop->line_of_sight = _ai_line_of_sight_from_cover;
						action_flee_call_simple(actor_index, _action_flee_call_situation_status, 0, 0, NULL);
					}
				}
			}
		}

		switch (state_data->panic_type)
		{
		case _actor_panic_grenade_attached_to_us:
		case _actor_panic_delayed_projectile_attached_to_us:
			if (actor->input.delayed_attached_projectile_index == NONE)
				state_data->done_fleeing = TRUE;
			break;

		case _actor_panic_melee_attached_to_us:
			if (!actor->input.melee_attacker_attached)
				state_data->done_fleeing = TRUE;
			break;

		case _actor_panic_burning_to_death:
			if (!actor->input.burning_to_death)
				state_data->done_fleeing = TRUE;
			break;
		}

		if (actor->meta.timeslice && !state_data->done_fleeing)
		{
			if (state_data->flee_firing_position_index != NONE &&
				state_data->forced_flee_ticks == 0 &&
				action_flee_call_simple(actor_index, _action_flee_call_position_exposed, 0, 0, NULL))
			{
				state_data->flee_firing_position_index = NONE;
				state_data->find_new_flee_position = TRUE;
			}

			if (actor->input.vehicle_passenger)
			{
				state_data->find_new_flee_position = FALSE;
				state_data->unable_to_flee = TRUE;
				actor->emotions.last_flee_failed_time = game_time_get();
			}
			else if (state_data->find_new_flee_position)
			{
				action_flee_call_simple(actor_index, _action_flee_call_find_flee_position, 0, 0, NULL);
				if (state_data->flee_firing_position_index == NONE)
				{
					state_data->unable_to_flee = TRUE;
					actor->emotions.last_flee_failed_time = game_time_get();
				}
			}
		}
	}

	if (action_flee_blind_panic(state_data->panic_type))
	{
		long unit_index = actor->meta.unit_index;

		if (unit_index != NONE && !action_flee_call_simple(actor_index, _action_flee_call_unit_is_speaking, 0, 0, NULL))
		{
			state_data->communicated_flee = FALSE;
		}
	}

	if (state_data->panic_type > _actor_panic_none &&
		state_data->flee_firing_position_index != NONE &&
		!state_data->unable_to_flee &&
		actor->meta.unit_index != NONE)
	{
		long now = game_time_get();

		if (!state_data->communicated_flee ||
			state_data->last_communication_time + 2*TICKS_PER_SECOND >= now)
		{
			if (state_data->panic_type == _actor_panic_burning_to_death ||
				state_data->panic_type == _actor_panic_melee_attached_to_us)
			{
				action_flee_call_simple(actor_index, _action_flee_call_scream, 0, 0, NULL);
			}
			else if (state_data->panic_type == _actor_panic_grenade_attached_to_us ||
				state_data->panic_type == _actor_panic_delayed_projectile_attached_to_us)
			{
				action_flee_call_simple(actor_index, _action_flee_call_scream, 1, 0, NULL);
			}
			else
			{
				long cause_unit_index = NONE;

				if (state_data->flee_prop_index != NONE)
				{
					cause_unit_index = prop_get(state_data->flee_prop_index)->unit_index;
				}

				if (!state_data->communicated_flee)
				{
					action_flee_call_simple(actor_index, _action_flee_call_shout, 0, cause_unit_index, NULL);
					state_data->communicated_flee = TRUE;
				}
				else
				{
					action_flee_call_simple(actor_index, _action_flee_call_shout, 1, cause_unit_index, NULL);
				}
			}
			state_data->last_communication_time = now;
		}
	}

	if (!actor->meta.swarm)
	{
		match_assert(
			"c:\\halo\\SOURCE\\ai\\action_flee.c",
			302,
			(!actor->meta.timeslice && state_data->find_new_flee_position) ||
				(state_data->flee_stationary_ticks > 0) ||
				(state_data->flee_firing_position_index != NONE) ||
				state_data->unable_to_flee ||
				state_data->done_fleeing);
	}
	return state_data->unable_to_flee || state_data->done_fleeing;
}

/* ---------- private code */
