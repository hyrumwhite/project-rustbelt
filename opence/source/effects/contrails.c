/*
CONTRAILS.C

symbols in this file:
00085EE0 0020:
	_contrail_scale_value (0000)
00085F00 0020:
	_contrails_initialize_for_new_map (0000)
00085F20 0020:
	_contrails_dispose_from_old_map (0000)
00085F40 0020:
	_contrails_dispose (0000)
00085F60 0080:
	_contrail_delete (0000)
00085FE0 0010:
	_contrails_disconnect_from_structure_bsp (0000)
00085FF0 00d0:
	_contrails_reconnect_to_structure_bsp (0000)
000860C0 0090:
	_contrail_compute_new_point_count (0000)
00086150 01a0:
	_contrail_verify (0000)
000862F0 0020:
	_local_random_range (0000)
00086310 0030:
	_local_random_vector_in_cone3d (0000)
00086340 0070:
	_contrail_scale_random_value (0000)
000863B0 0070:
	_contrails_initialize (0000)
00086420 0090:
	_contrail_next_frame (0000)
000864B0 03c0:
	_contrail_add_points (0000)
00086870 0380:
	_contrail_update_points (0000)
00086BF0 0150:
	_contrail_new (0000)
00086D40 0080:
	_contrail_owner_collision (0000)
00086DC0 0220:
	_contrails_update (0000)
0025976C 0039:
	??_C@_0DJ@LAEOKLOH@contrail?5?$CFs?5thinks?5it?5has?5?$CFd?5poi@ (0000)
002597A8 0041:
	??_C@_0EB@OBNMHKML@contrail?5?$CFs?5?$CI?$CFld?$CJ?5has?5an?5object?5@ (0000)
002597EC 003c:
	??_C@_0DM@BEOPFMNL@contrail?5?$CFs?5attachment?5index?5?$CFd?5@ (0000)
00259828 0023:
	??_C@_0CD@HFOFGNFP@c?3?2halo?2SOURCE?2effects?2contrails@ (0000)
0025984C 0023:
	??_C@_0CD@MIFFODIN@couldn?8t?5allocate?5contrail?5globa@ (0000)
00259870 000f:
	??_C@_0P@NFJLENP@contrail?5point?$AA@ (0000)
00259880 0009:
	??_C@_08LACPGLIF@contrail?$AA@ (0000)
0025988C 0002:
	??_C@_01PFHFFBPC@t?$AA@ (0000)
00259890 0017:
	??_C@_0BH@KJOJEDNC@definition_index?$CB?$DNNONE?$AA@ (0000)
002598A8 0013:
	??_C@_0BD@GHNECCAG@object_index?$CB?$DNNONE?$AA@ (0000)
002598C0 0070:
	??_C@_0HA@NOKFPIOB@contrail?9?$DOcontrail_point_counts?$FL@ (0000)
*/

/* ---------- headers */

#include "effects/contrails.h"
#include "game/game.h"
#include "halo_sim.h" /* port: port/linux/game/halo_sim.c */

#include "bitmaps/bitmap_group.h"
#include "cseries/errors.h"
#include "effects/contrail_definitions.h"
#include "memory/data.h"
#include "objects/object_definitions.h"
#include "physics/point_physics.h"
#include "saved games/game_state.h"
#include "scenario/scenario.h"

/* ---------- constants */

enum
{
	MAXIMUM_CONTRAIL_INSTANCES = 4,
	/* the native builds' larger contrail point pool (halo_port_capacity.h);
	contrail_update_points lists a contrail's points in an array of this size
	without a bound check, so it must stay the size of the pool */
	MAXIMUM_CONTRAIL_POINTS = HALO_PORT_MAXIMUM_CONTRAIL_POINTS
};

enum
{
	_contrail_active_bit = 0
};

enum
{
	_contrail_point_new_bit = 0,
	_contrail_point_transitioning_bit,
	_contrail_point_expired_bit
};

enum
{
	_contrail_state_duration_bit = 0,
	_contrail_state_duration_delta_bit,
	_contrail_state_transition_duration_bit,
	_contrail_state_transition_duration_delta_bit
};

enum
{
	_contrail_scales_point_generation_rate_bit = 0,
	_contrail_scales_point_velocity_bit,
	_contrail_scales_point_velocity_delta_bit,
	_contrail_scales_point_velocity_cone_angle_bit,
	_contrail_scales_inherited_velocity_fraction_bit,
	_contrail_scales_sequence_animation_rate_bit,
	_contrail_scales_texture_scale_u_bit,
	_contrail_scales_texture_scale_v_bit,
	_contrail_scales_texture_animation_u_bit,
	_contrail_scales_texture_animation_v_bit
};

/* ---------- macros */

#define contrail_get(index) \
	((struct contrail_datum *)datum_get(contrail_data, (index)))
#define contrail_point_get(index) \
	((struct contrail_point_datum *)datum_get(contrail_point_data, (index)))

/* ---------- structures */

/* ---------- prototypes */

static void contrail_next_frame(
	struct contrail_datum *contrail);
static short contrail_compute_new_point_count(
	long contrail_index,
	real dt);
static void contrail_add_points(
	long contrail_index,
	short contrail_point_count,
	boolean force);
static void contrail_update_points(
	long contrail_index,
	real dt);
static void contrail_verify(
	long contrail_index);
static void contrail_update_game(
	long contrail_index,
	real dt,
	real point_dt);

/* port: a contrail's calls of the game (contrail_call) */
static void contrail_call_new(
	struct halo_sim_contrail_call *call,
	long kind)
{
	memset(call, 0, sizeof(*call));
	call->kind = kind;
}

static void contrail_call_location(
	struct halo_sim_contrail_call *call,
	struct location const *location)
{
	call->a = location->leaf_index;
	call->b = location->cluster_index;
	call->c = location->bonus;
}

static void contrail_call_out_location(
	struct halo_sim_contrail_call const *call,
	struct location *location)
{
	location->leaf_index = call->out_location_leaf;
	location->cluster_index = call->out_location_cluster;
	location->bonus = call->out_location_bonus;
}

/* ---------- globals */

/* ---------- public code */

real contrail_scale_value(
	real value,
	real scale,
	unsigned long flags,
	short flag_bit)
{
	real result = scale;

	if (TEST_FLAG(flags, flag_bit))
		result *= value;

	return result;
}

real contrail_scale_random_value(
	real value,
	real lower_bound,
	real upper_bound,
	unsigned long flags,
	short flag_bit)
{
	real minimum = contrail_scale_value(
		value,
		lower_bound,
		flags,
		flag_bit);
	real range = contrail_scale_value(
		value,
		upper_bound - lower_bound,
		flags,
		flag_bit + 1);

	return real_local_random_range(0.0f, range) + minimum;
}

void contrails_initialize(
	void)
{
	/* the native builds' larger contrail pools (halo_port_capacity.h) */
	contrail_data = game_state_data_new("contrail", HALO_PORT_MAXIMUM_CONTRAILS, 0x44);
	contrail_point_data = game_state_data_new("contrail point", MAXIMUM_CONTRAIL_POINTS, 0x38);
	if (contrail_data && contrail_point_data)
		return;

	if (contrail_point_data)
	{
		contrail_point_data = NULL;
		error(_error_immediate, "couldn't allocate contrail globals");
		return;
	}

	if (contrail_data)
		contrail_data = NULL;

	error(_error_immediate, "couldn't allocate contrail globals");

	return;
}

void contrails_initialize_for_new_map(
	void)
{
	data_make_valid(contrail_data);
	data_make_valid(contrail_point_data);

	return;
}

void contrails_dispose_from_old_map(
	void)
{
	data_make_invalid(contrail_point_data);
	data_make_invalid(contrail_data);

	return;
}

void contrails_dispose(
	void)
{
	if (contrail_point_data)
		contrail_point_data = NULL;
	if (contrail_data)
		contrail_data = NULL;

	return;
}

void contrail_delete(
	long contrail_index)
{
	struct contrail_datum *contrail = contrail_get(contrail_index);
	short instance_index;

	for (instance_index = 0; instance_index < MAXIMUM_CONTRAIL_INSTANCES; instance_index++)
	{
		long contrail_point_index = contrail->first_contrail_point_indices[instance_index];

		while (contrail_point_index != NONE)
		{
			long next_contrail_point_index = contrail_point_get(contrail_point_index)->next_contrail_point_index;

			datum_delete(contrail_point_data, contrail_point_index);
			contrail_point_index = next_contrail_point_index;
		}
	}

	datum_delete(contrail_data, contrail_index);

	return;
}

void contrails_disconnect_from_structure_bsp(
	void)
{
	return;
}

void contrails_reconnect_to_structure_bsp(
	void)
{
	long contrail_index;

	for (contrail_index = data_next_index(contrail_data, NONE);
		contrail_index != NONE;
		contrail_index = data_next_index(contrail_data, contrail_index))
	{
		struct contrail_datum *contrail = contrail_get(contrail_index);
		struct contrail_definition *definition = contrail_definition_get(contrail->definition_index);
		short instance_index;

		for (instance_index = 0; instance_index < MAXIMUM_CONTRAIL_INSTANCES; instance_index++)
		{
			long contrail_point_index = contrail->first_contrail_point_indices[instance_index];

			while (contrail_point_index != NONE)
			{
				struct contrail_point_datum *contrail_point = contrail_point_get(contrail_point_index);

				if (contrail_point->location.cluster_index != NONE)
					scenario_location_from_point(&contrail_point->location, &contrail_point->position);

				contrail_point_index = contrail_point->next_contrail_point_index;
			}
		}
	}

	return;
}

static long contrail_new_game(
	long definition_index,
	long object_index,
	short attachment_index);

/* port: the Rust port's in place of the game's when asked (halo_sim.c); in
check mode the game's runs and is compared */
long contrail_new(
	long definition_index,
	long object_index,
	short attachment_index)
{
	int checking = FALSE;
	long contrail_index;

	if (halo_sim_contrail_new(definition_index, object_index, attachment_index, &contrail_index, &checking))
		return contrail_index;
	contrail_index = contrail_new_game(definition_index, object_index, attachment_index);
	if (checking)
		halo_sim_contrail_new_check(contrail_index);

	return contrail_index;
}

static long contrail_new_game(
	long definition_index,
	long object_index,
	short attachment_index)
{
	long contrail_index = NONE;
	struct halo_sim_contrail_call call;

	match_assert(
		"c:\\halo\\SOURCE\\effects\\contrails.c",
		97,
		object_index!=NONE);
	match_assert(
		"c:\\halo\\SOURCE\\effects\\contrails.c",
		98,
		definition_index!=NONE);

	if (definition_index != NONE)
	{
		struct contrail_definition *definition = contrail_definition_get(definition_index);

		/* port: the datum and the attachment's function, through
		contrail_call */
		contrail_call_new(&call, _contrail_call_new);
		call.a = object_index;
		call.b = attachment_index;
		contrail_call(NONE, &call);
		contrail_index = call.result;
		if (contrail_index != NONE)
		{
			struct contrail_datum *contrail = contrail_get(contrail_index);
			short instance_index;

			contrail->flags = 0;
			contrail->definition_index = definition_index;
			contrail->object_index = object_index;
			contrail->attachment_index = attachment_index;
			contrail->density_function_index = (short)call.out_location_leaf;
			contrail->sequence_index = NONE;
			contrail_next_frame(contrail);
			contrail->texture_offset_u = 0.0f;
			contrail->texture_offset_v = 0.0f;

			for (instance_index = 0;
				instance_index < MAXIMUM_CONTRAIL_INSTANCES;
				instance_index++)
			{
				contrail->contrail_point_counts[instance_index] = 0;
				contrail->first_contrail_point_indices[instance_index] = NONE;
			}

			contrail_call_new(&call, _contrail_call_function_value);
			call.a = contrail->object_index;
			call.b = contrail->density_function_index;
			call.f[0] = contrail->density;
			contrail_call(contrail_index, &call);
			contrail->density = call.out[0];
			if (call.result)
			{
				SET_FLAG(contrail->flags, _contrail_active_bit, TRUE);
				contrail_add_points(contrail_index, 1, TRUE);
			}
		}
	}

	return contrail_index;
}

void contrails_update(
	real dt)
{
	long contrail_index;
	/* The native builds update contrails every frame, several frames per
	tick (port/linux/game/render_interpolation.c), but objects and their
	markers move only on ticks: points due on a frame that ran no tick would
	land on the last point, be skipped and be lost. Emit on the first frame
	after each tick, for all the time since the last emission. */
	static long last_point_game_time = NONE;
	static real pending_point_dt = 0.0f;
	real point_dt;

	pending_point_dt += dt;
	if (last_point_game_time != game_time_get())
	{
		last_point_game_time = game_time_get();
		point_dt = pending_point_dt;
		pending_point_dt = 0.0f;
	}
	else
	{
		point_dt = 0.0f;
	}

	for (contrail_index = data_next_index(contrail_data, NONE);
		contrail_index != NONE;
		contrail_index = data_next_index(contrail_data, contrail_index))
	{
		/* port: the Rust port's update in place of the game's when asked
		(halo_sim.c); in check mode the game's runs and is compared */
		int checking = FALSE;

		if (halo_sim_contrail_update(contrail_index, dt, point_dt, &checking))
			continue;
		contrail_update_game(contrail_index, dt, point_dt);
		if (checking)
			halo_sim_contrail_update_check(contrail_index);
	}

	return;
}

static void contrail_update_game(
	long contrail_index,
	real dt,
	real point_dt)
{
	{
		struct contrail_datum *contrail = contrail_get(contrail_index);
		struct contrail_definition *definition = contrail_definition_get(contrail->definition_index);
		real frame_dt = dt - contrail->expired_dt;
		real frames_per_second;
		real frame_period;
		real remaining_dt;
		real texture_animation_u;
		real texture_animation_v;
		short instance_index;

		contrail_verify(contrail_index);
		contrail->expired_dt = 0.0f;

		if (contrail->object_index != NONE)
		{
			struct halo_sim_contrail_call call;
			boolean active;

			/* port: object_get_function_value, through contrail_call */
			contrail_call_new(&call, _contrail_call_function_value);
			call.a = contrail->object_index;
			call.b = contrail->density_function_index;
			call.f[0] = contrail->density;
			contrail_call(contrail_index, &call);
			contrail->density = call.out[0];
			active = call.result != 0;

			if (active != TEST_FLAG(contrail->flags, _contrail_active_bit))
			{
				real density = contrail->density;

				contrail->density = 0.0f;
				contrail_add_points(contrail_index, 1, TRUE);
				contrail->density = density;
			}

			SET_FLAG(contrail->flags, _contrail_active_bit, active);

			if (active)
			{
				contrail_add_points(
					contrail_index,
					contrail_compute_new_point_count(contrail_index, point_dt),
					TRUE);
			}
		}

		remaining_dt = frame_dt;
		frames_per_second = definition->frames_per_second;
		if (TEST_FLAG(definition->scale_flags, _contrail_scales_sequence_animation_rate_bit))
			frames_per_second = frames_per_second * contrail->density;
		frame_period = 1.0f / frames_per_second;

		while (remaining_dt > 0.0f)
		{
			real time_until_next_frame = frame_period - contrail->frame_time;

			if (time_until_next_frame <= remaining_dt)
			{
				contrail_next_frame(contrail);
				remaining_dt = remaining_dt - time_until_next_frame;
			}
			else
			{
				contrail->frame_time = remaining_dt + contrail->frame_time;
				break;
			}
		}

		texture_animation_u = definition->texture_animation_u;
		if (TEST_FLAG(definition->scale_flags, _contrail_scales_texture_animation_u_bit))
			texture_animation_u = texture_animation_u * contrail->density;
		contrail->texture_offset_u = contrail->texture_offset_u - texture_animation_u * frame_dt;

		texture_animation_v = definition->texture_animation_v;
		if (TEST_FLAG(definition->scale_flags, _contrail_scales_texture_animation_v_bit))
			texture_animation_v = texture_animation_v * contrail->density;
		contrail->texture_offset_v = texture_animation_v * frame_dt + contrail->texture_offset_v;

		contrail_update_points(contrail_index, dt);

		for (instance_index = 0;
			instance_index < MAXIMUM_CONTRAIL_INSTANCES;
			instance_index++)
		{
			match_assert(
				"c:\\halo\\SOURCE\\effects\\contrails.c",
				266,
				contrail->contrail_point_counts[instance_index]>0==contrail->first_contrail_point_indices[instance_index]!=NONE);

			if (contrail->first_contrail_point_indices[instance_index] != NONE)
				break;
		}

		contrail_verify(contrail_index);

		if (instance_index == MAXIMUM_CONTRAIL_INSTANCES &&
			contrail->object_index == NONE)
		{
			struct halo_sim_contrail_call call;

			contrail_call_new(&call, _contrail_call_delete);
			contrail_call(contrail_index, &call);
		}
	}

	return;
}

void contrail_owner_collision(
	long contrail_index,
	unsigned char object_dying,
	real dt)
{
	struct contrail_datum *contrail = contrail_get(contrail_index);
	struct contrail_definition *definition = contrail_definition_get(contrail->definition_index);

	if (TEST_FLAG(contrail->flags, _contrail_active_bit))
	{
		short contrail_point_count = contrail_compute_new_point_count(contrail_index, dt);

		contrail_add_points(contrail_index, MAX(1, contrail_point_count), FALSE);
	}

	if (object_dying)
		contrail->object_index = NONE;

	contrail->expired_dt += dt;

	return;
}

/* ---------- private code */

static void contrail_verify(
	long contrail_index)
{
	struct contrail_datum *contrail = contrail_get(contrail_index);
	struct contrail_definition *definition = contrail_definition_get(contrail->definition_index);
	short instance_index;

	if (contrail->object_index != NONE)
	{
		struct object_datum *object = object_get(contrail->object_index);

		match_vassert(
			"c:\\halo\\SOURCE\\effects\\contrails.c",
			654,
			contrail->attachment_index >= 0 &&
				contrail->attachment_index < object_definition_get(
					object->definition_index)->object.attachments.count,
			csprintf(
				temporary,
				"contrail %s attachment index %d is outside the valid range.",
				tag_get_name(contrail->definition_index),
				contrail->attachment_index));
		/* port: and only an index the object holds (8; the first assert
		only logs in a release build, and the count is the map's) */
		if (VALID_INDEX(contrail->attachment_index, MAXIMUM_NUMBER_OF_ATTACHMENTS_PER_OBJECT))
		{
			match_vassert(
				"c:\\halo\\SOURCE\\effects\\contrails.c",
				655,
				object->object.attachment_indices[contrail->attachment_index] == contrail_index,
				csprintf(
					temporary,
					"contrail %s (%ld) has an object that thinks it's attached to %ld",
					tag_get_name(contrail->definition_index),
					contrail_index,
					object->object.attachment_indices[contrail->attachment_index]));
		}
	}

	for (instance_index = 0;
		instance_index < MAXIMUM_CONTRAIL_INSTANCES;
		instance_index++)
	{
		short contrail_point_count = 0;
		long contrail_point_index = contrail->first_contrail_point_indices[instance_index];

		while (contrail_point_index != NONE)
		{
			contrail_point_index = contrail_point_get(contrail_point_index)->next_contrail_point_index;
			contrail_point_count++;
		}

		match_vassert(
			"c:\\halo\\SOURCE\\effects\\contrails.c",
			677,
			contrail_point_count == contrail->contrail_point_counts[instance_index],
			csprintf(
				temporary,
				"contrail %s thinks it has %d points but really it has %d",
				tag_get_name(contrail->definition_index),
				contrail->contrail_point_counts[instance_index],
				contrail_point_count));
	}

	return;
}

/* port: datum_delete of a point, through contrail_call */
static void contrail_call_point_delete(
	long contrail_index,
	long contrail_point_index)
{
	struct halo_sim_contrail_call call;

	contrail_call_new(&call, _contrail_call_point_delete);
	call.a = contrail_point_index;
	contrail_call(contrail_index, &call);
}

static void contrail_update_points(
	long contrail_index,
	real dt)
{
	struct contrail_datum *contrail = contrail_get(contrail_index);
	struct contrail_definition *definition = contrail_definition_get(contrail->definition_index);
	long contrail_point_indices[MAXIMUM_CONTRAIL_POINTS];
	short instance_index;

	for (instance_index = 0;
		instance_index < MAXIMUM_CONTRAIL_INSTANCES;
		instance_index++)
	{
		short contrail_point_count = 0;
		long contrail_point_index = contrail->first_contrail_point_indices[instance_index];

		while (contrail_point_index != NONE)
		{
			struct contrail_point_datum *contrail_point = contrail_point_get(contrail_point_index);

			if (!TEST_FLAG(contrail_point->flags, _contrail_point_expired_bit))
			{
				contrail_point->time = dt * contrail_point->delta + contrail_point->time;

				while (contrail_point->delta == 0.0f || contrail_point->time > 1.0f)
				{
					if (TEST_FLAG(contrail_point->flags, _contrail_point_transitioning_bit))
					{
						struct contrail_point_state *state = TAG_BLOCK_GET_ELEMENT(
							&definition->states,
							contrail_point->state_index + 1,
							struct contrail_point_state);

						contrail_point->state_index++;
						contrail_point->time = 0.0f;
						contrail_point->delta = contrail_scale_random_value(
							contrail_point->density,
							state->duration_lower_bound,
							state->duration_upper_bound,
							state->scale_flags,
							_contrail_state_duration_bit);
						if (contrail_point->delta != 0.0f)
							contrail_point->delta = 1.0f / contrail_point->delta;

						SET_FLAG(contrail_point->flags, _contrail_point_transitioning_bit, FALSE);
					}
					else if (contrail_point->state_index + 1 < definition->states.count)
					{
						struct contrail_point_state *state = TAG_BLOCK_GET_ELEMENT(
							&definition->states,
							contrail_point->state_index,
							struct contrail_point_state);

						contrail_point->time = 0.0f;
						contrail_point->delta = contrail_scale_random_value(
							contrail_point->density,
							state->transition_duration_lower_bound,
							state->transition_duration_upper_bound,
							state->scale_flags,
							_contrail_state_transition_duration_bit);
						if (contrail_point->delta != 0.0f)
							contrail_point->delta = 1.0f / contrail_point->delta;

						SET_FLAG(contrail_point->flags, _contrail_point_transitioning_bit, TRUE);
					}
					else
					{
						SET_FLAG(contrail_point->flags, _contrail_point_expired_bit, TRUE);
						break;
					}
				}
			}

			if (TEST_FLAG(contrail_point->flags, _contrail_point_new_bit))
			{
				SET_FLAG(contrail_point->flags, _contrail_point_new_bit, FALSE);
			}
			else if (!TEST_FLAG(contrail_point->flags, _contrail_point_expired_bit))
			{
				struct contrail_point_state *state = TAG_BLOCK_GET_ELEMENT(
					&definition->states,
					contrail_point->state_index,
					struct contrail_point_state);

				if (state->physics.index != NONE)
				{
					/* port: point_physics_update, through contrail_call */
					struct halo_sim_contrail_call call;

					contrail_call_new(&call, _contrail_call_physics);
					contrail_call_location(&call, &contrail_point->location);
					call.d = state->physics.index;
					call.f[0] = contrail_point->position.x;
					call.f[1] = contrail_point->position.y;
					call.f[2] = contrail_point->position.z;
					call.f[3] = contrail_point->velocity.i;
					call.f[4] = contrail_point->velocity.j;
					call.f[5] = contrail_point->velocity.k;
					call.f[6] = state->width * 0.5f;
					call.f[7] = dt;
					contrail_call(contrail_index, &call);
					contrail_point->position.x = call.out[0];
					contrail_point->position.y = call.out[1];
					contrail_point->position.z = call.out[2];
					contrail_point->velocity.i = call.out[3];
					contrail_point->velocity.j = call.out[4];
					contrail_point->velocity.k = call.out[5];
					contrail_call_out_location(&call, &contrail_point->location);
				}
			}

			contrail_point_indices[contrail_point_count++] = contrail_point_index;
			contrail_point_index = contrail_point->next_contrail_point_index;
		}

		while (contrail_point_count > 1)
		{
			struct contrail_point_datum *contrail_point;
			struct contrail_point_datum *previous_contrail_point;

			contrail_point_count--;
			contrail_point = contrail_point_get(contrail_point_indices[contrail_point_count]);
			previous_contrail_point = contrail_point_get(
				contrail_point_indices[contrail_point_count - 1]);

			if (!TEST_FLAG(contrail_point->flags, _contrail_point_expired_bit) ||
				!TEST_FLAG(previous_contrail_point->flags, _contrail_point_expired_bit) ||
				contrail_point->next_contrail_point_index != NONE)
			{
				break;
			}

			previous_contrail_point->next_contrail_point_index = NONE;
			contrail->contrail_point_counts[instance_index]--;
			contrail_call_point_delete(contrail_index, contrail_point_indices[contrail_point_count]);
		}

		if (contrail->contrail_point_counts[instance_index] == 1)
		{
			if (TEST_FLAG(
				contrail_point_get(
					contrail->first_contrail_point_indices[instance_index])->flags,
				_contrail_point_expired_bit))
			{
				contrail_call_point_delete(
					contrail_index,
					contrail->first_contrail_point_indices[instance_index]);
				contrail->first_contrail_point_indices[instance_index] = NONE;
				contrail->contrail_point_counts[instance_index] = 0;
			}
		}
	}

	return;
}

static void contrail_next_frame(
	struct contrail_datum *contrail)
{
	struct contrail_definition *definition = contrail_definition_get(contrail->definition_index);
	struct bitmap_group *bitmap = bitmap_group_get(definition->bitmap.index);
	short sequence_index = contrail->sequence_index;

	contrail->frame_time = 0.0f;
	contrail->frame_index++;

	if (sequence_index < 0 ||
		sequence_index >= bitmap->sequences.count ||
		contrail->frame_index < 0 ||
		contrail->frame_index >= TAG_BLOCK_GET_ELEMENT(
			&bitmap->sequences,
			sequence_index,
			struct bitmap_group_sequence)->bitmap_count)
	{
		contrail->sequence_index = local_random_range(
			definition->first_sequence_index,
			definition->sequence_count + definition->first_sequence_index);
		contrail->frame_index = 0;
	}

	return;
}

static short contrail_compute_new_point_count(
	long contrail_index,
	real dt)
{
	struct contrail_datum *contrail = contrail_get(contrail_index);
	struct contrail_definition *definition = contrail_definition_get(contrail->definition_index);
	real point_generation_rate = definition->point_generation_rate;
	real time_between_points;
	short contrail_point_count = 0;

	if (TEST_FLAG(definition->scale_flags, _contrail_scales_point_generation_rate_bit))
		point_generation_rate = point_generation_rate * contrail->density;

	time_between_points = 1.0f / point_generation_rate;

	while (dt != 0.0f)
	{
		if (dt >= contrail->time_until_point)
		{
			dt = dt - contrail->time_until_point;
			contrail->time_until_point = time_between_points;
			contrail_point_count++;
		}
		else
		{
			contrail->time_until_point = contrail->time_until_point - dt;
			break;
		}
	}

	return contrail_point_count;
}

static void contrail_add_points(
	long contrail_index,
	short contrail_point_count,
	boolean force)
{
	struct contrail_datum *contrail = contrail_get(contrail_index);
	struct contrail_definition *definition = contrail_definition_get(contrail->definition_index);

	if (contrail_point_count != 0)
	{
		real_point3d marker_positions[MAXIMUM_CONTRAIL_INSTANCES];
		real_vector3d marker_forwards[MAXIMUM_CONTRAIL_INSTANCES];
		struct halo_sim_contrail_call call;
		short marker_count = 1;
		short marker_index;

		/* port: the attachment's markers, one by one through contrail_call */
		for (marker_index = 0;
			marker_index < marker_count && marker_index < MAXIMUM_CONTRAIL_INSTANCES;
			marker_index++)
		{
			contrail_call_new(&call, _contrail_call_markers);
			call.a = contrail->object_index;
			call.b = contrail->attachment_index;
			call.c = marker_index;
			contrail_call(contrail_index, &call);
			marker_count = (short)call.result;
			if (marker_index < marker_count)
			{
				marker_positions[marker_index].x = call.out[0];
				marker_positions[marker_index].y = call.out[1];
				marker_positions[marker_index].z = call.out[2];
				marker_forwards[marker_index].i = call.out[3];
				marker_forwards[marker_index].j = call.out[4];
				marker_forwards[marker_index].k = call.out[5];
			}
		}
		if (marker_count > MAXIMUM_CONTRAIL_INSTANCES)
			marker_count = MAXIMUM_CONTRAIL_INSTANCES;

		if (marker_count > 0)
		{
			real point_velocity = contrail_scale_random_value(
				contrail->density,
				definition->point_velocity_lower_bound,
				definition->point_velocity_upper_bound,
				definition->scale_flags,
				_contrail_scales_point_velocity_bit);
			real point_velocity_cone_angle = contrail_scale_value(
				contrail->density,
				definition->point_velocity_cone_angle,
				definition->scale_flags,
				_contrail_scales_point_velocity_cone_angle_bit);
			real point_inherited_velocity_fraction = contrail_scale_value(
				contrail->density,
				definition->point_inherited_velocity_fraction,
				definition->scale_flags,
				_contrail_scales_inherited_velocity_fraction_bit);

			for (marker_index = 0; marker_index < marker_count; marker_index++)
			{
				struct contrail_point_datum *previous_contrail_point;
				short new_contrail_point_count;
				short contrail_point_index;

				if (contrail->first_contrail_point_indices[marker_index] != NONE)
				{
					previous_contrail_point = contrail_point_get(
						contrail->first_contrail_point_indices[marker_index]);
					new_contrail_point_count = contrail_point_count;
				}
				else
				{
					previous_contrail_point = NULL;
					new_contrail_point_count = 1;
				}

				if (previous_contrail_point &&
					!csmemcmp(
						&marker_positions[marker_index],
						&previous_contrail_point->position,
						sizeof(real_point3d)) &&
					!force)
				{
					continue;
				}

				for (contrail_point_index = 1;
					contrail_point_index <= new_contrail_point_count;
					contrail_point_index++)
				{
					long new_contrail_point_index;

					contrail_call_new(&call, _contrail_call_point_new);
					contrail_call(contrail_index, &call);
					new_contrail_point_index = call.result;

					if (new_contrail_point_index != NONE)
					{
						struct contrail_point_datum *contrail_point =
							contrail_point_get(new_contrail_point_index);
						real_vector3d direction;
						real_vector3d object_velocity;

						contrail_point->time = 0.0f;
						contrail_point->delta = 0.0f;
						contrail_point->flags = FLAG(_contrail_point_new_bit) |
							FLAG(_contrail_point_transitioning_bit);
						contrail_point->state_index = NONE;
						contrail_point->density = contrail->density;
						local_random_vector_in_cone3d(
							&marker_forwards[marker_index],
							0.0f,
							point_velocity_cone_angle,
							&direction);
						contrail_point->position = marker_positions[marker_index];
						contrail_call_new(&call, _contrail_call_location_from_point);
						contrail_call_location(&call, &contrail_point->location);
						call.f[0] = contrail_point->position.x;
						call.f[1] = contrail_point->position.y;
						call.f[2] = contrail_point->position.z;
						contrail_call(contrail_index, &call);
						contrail_call_out_location(&call, &contrail_point->location);
						contrail_call_new(&call, _contrail_call_velocity);
						call.a = contrail->object_index;
						contrail_call(contrail_index, &call);
						object_velocity.i = call.out[0];
						object_velocity.j = call.out[1];
						object_velocity.k = call.out[2];
						contrail_point->velocity.i = point_inherited_velocity_fraction *
							object_velocity.i + direction.i * point_velocity;
						contrail_point->velocity.j = point_inherited_velocity_fraction *
							object_velocity.j + direction.j * point_velocity;
						contrail_point->velocity.k = point_inherited_velocity_fraction *
							object_velocity.k + direction.k * point_velocity;

						if (contrail_point_index < new_contrail_point_count)
						{
							real t = (real)contrail_point_index / new_contrail_point_count;
							real one_minus_t = 1.0f - t;
							real_point3d interpolated_position;

							match_assert_valid_real(
								"c:\\halo\\SOURCE\\effects\\contrails.c",
								471,
								t);

							contrail_point->density = one_minus_t * previous_contrail_point->density +
								t * contrail_point->density;

							interpolated_position.x = one_minus_t * previous_contrail_point->position.x +
								t * contrail_point->position.x;
							interpolated_position.y = one_minus_t * previous_contrail_point->position.y +
								t * contrail_point->position.y;
							interpolated_position.z = one_minus_t * previous_contrail_point->position.z +
								t * contrail_point->position.z;
							contrail_call_new(&call, _contrail_call_location_from_line);
							contrail_call_location(&call, &contrail_point->location);
							call.f[0] = contrail_point->position.x;
							call.f[1] = contrail_point->position.y;
							call.f[2] = contrail_point->position.z;
							call.f[3] = interpolated_position.x;
							call.f[4] = interpolated_position.y;
							call.f[5] = interpolated_position.z;
							contrail_call(contrail_index, &call);
							contrail_call_out_location(&call, &contrail_point->location);
							contrail_point->position = interpolated_position;

							contrail_point->velocity.i = one_minus_t * previous_contrail_point->velocity.i +
								t * contrail_point->velocity.i;
							contrail_point->velocity.j = one_minus_t * previous_contrail_point->velocity.j +
								t * contrail_point->velocity.j;
							contrail_point->velocity.k = one_minus_t * previous_contrail_point->velocity.k +
								t * contrail_point->velocity.k;
						}

						contrail_point->next_contrail_point_index =
							contrail->first_contrail_point_indices[marker_index];
						contrail->contrail_point_counts[marker_index]++;
						contrail->first_contrail_point_indices[marker_index] = new_contrail_point_index;
					}
				}
			}
		}
	}

	return;
}

/* ---------- port: contrail_call */

/* a contrail update's call of the rest of the game, shared by the C's update
and the Rust port's (halo_sim.c); in check mode each is recorded */
void contrail_call(
	long contrail_index,
	struct halo_sim_contrail_call *call)
{
	int opened = halo_sim_contrail_event_begin(contrail_index, call);

	switch (call->kind)
	{
		case _contrail_call_function_value:
		{
			real value = call->f[0];

			call->result = object_get_function_value(call->a, (short)call->b, &value);
			call->out[0] = value;
			break;
		}

		case _contrail_call_markers:
		{
			struct object_datum *object = object_get(call->a);
			struct object_marker markers[MAXIMUM_CONTRAIL_INSTANCES];
			short marker_count = object_get_marker_by_name(
				call->a,
				TAG_BLOCK_GET_ELEMENT(
					&object_definition_get(object->definition_index)->object.attachments,
					call->b,
					struct object_attachment_definition)->marker_name,
				markers,
				NUMBEROF(markers));

			call->result = marker_count;
			if (call->c >= 0 && call->c < marker_count)
			{
				call->out[0] = markers[call->c].matrix.position.x;
				call->out[1] = markers[call->c].matrix.position.y;
				call->out[2] = markers[call->c].matrix.position.z;
				call->out[3] = markers[call->c].matrix.forward.i;
				call->out[4] = markers[call->c].matrix.forward.j;
				call->out[5] = markers[call->c].matrix.forward.k;
			}
			break;
		}

		case _contrail_call_velocity:
		{
			real_vector3d velocity;

			object_get_velocities(call->a, &velocity, NULL);
			call->out[0] = velocity.i;
			call->out[1] = velocity.j;
			call->out[2] = velocity.k;
			break;
		}

		case _contrail_call_point_new:
			call->result = datum_new(contrail_point_data);
			break;

		case _contrail_call_point_delete:
			datum_delete(contrail_point_data, call->a);
			break;

		case _contrail_call_location_from_point:
		case _contrail_call_location_from_line:
		{
			struct location location;
			real_point3d point0;

			location.leaf_index = call->a;
			location.cluster_index = (short)call->b;
			location.bonus = (word)call->c;
			point0.x = call->f[0];
			point0.y = call->f[1];
			point0.z = call->f[2];
			if (call->kind == _contrail_call_location_from_point)
			{
				scenario_location_from_point(&location, &point0);
			}
			else
			{
				real_point3d point1;

				point1.x = call->f[3];
				point1.y = call->f[4];
				point1.z = call->f[5];
				scenario_location_from_line(&location, &location, &point0, &point1);
			}
			call->out_location_leaf = location.leaf_index;
			call->out_location_cluster = location.cluster_index;
			call->out_location_bonus = location.bonus;
			break;
		}

		case _contrail_call_physics:
		{
			struct location location;
			real_point3d position;
			real_vector3d velocity;

			location.leaf_index = call->a;
			location.cluster_index = (short)call->b;
			location.bonus = (word)call->c;
			position.x = call->f[0];
			position.y = call->f[1];
			position.z = call->f[2];
			velocity.i = call->f[3];
			velocity.j = call->f[4];
			velocity.k = call->f[5];
			point_physics_update(
				0,
				point_physics_definition_get(call->d),
				&location,
				NONE,
				&position,
				&velocity,
				NULL,
				NULL,
				NULL,
				call->f[6],
				call->f[7]);
			call->out[0] = position.x;
			call->out[1] = position.y;
			call->out[2] = position.z;
			call->out[3] = velocity.i;
			call->out[4] = velocity.j;
			call->out[5] = velocity.k;
			call->out_location_leaf = location.leaf_index;
			call->out_location_cluster = location.cluster_index;
			call->out_location_bonus = location.bonus;
			break;
		}

		case _contrail_call_delete:
			contrail_delete(contrail_index);
			break;

		case _contrail_call_new:
		{
			struct object_datum *object = object_get(call->a);

			call->result = datum_new(contrail_data);
			call->out_location_leaf = TAG_BLOCK_GET_ELEMENT(
				&object_definition_get(object->definition_index)->object.attachments,
				call->b,
				struct object_attachment_definition)->primary_scale_function_reference - 1;
			break;
		}
	}
	halo_sim_contrail_event_end(contrail_index, call, opened);

	return;
}

/* ---------- port: the contrail and its points for halo_sim.c */

int contrail_halo_sim_load(
	long contrail_index,
	struct halo_sim_contrail_state *state,
	struct halo_sim_contrail_point *points,
	unsigned long maximum_point_count,
	unsigned long *point_count)
{
	struct contrail_datum *contrail = (struct contrail_datum *)datum_try_and_get(contrail_data, contrail_index);
	short instance_index;
	unsigned long count = 0;

	*point_count = 0;
	state->local_random_seed = *get_global_local_random_seed_address();
	if (!contrail)
		return FALSE;
	state->contrail_index = (unsigned long)contrail_index;
	state->definition_index = (unsigned long)contrail->definition_index;
	state->flags = contrail->flags;
	state->attachment_index = contrail->attachment_index;
	state->object_index = contrail->object_index;
	state->density_function_index = contrail->density_function_index;
	state->sequence_index = contrail->sequence_index;
	state->frame_index = contrail->frame_index;
	state->pad = 0;
	state->density = contrail->density;
	state->texture_offset_u = contrail->texture_offset_u;
	state->texture_offset_v = contrail->texture_offset_v;
	state->time_until_point = contrail->time_until_point;
	state->frame_time = contrail->frame_time;
	state->expired_dt = contrail->expired_dt;
	for (instance_index = 0; instance_index < MAXIMUM_CONTRAIL_INSTANCES; instance_index++)
	{
		long contrail_point_index = contrail->first_contrail_point_indices[instance_index];

		state->point_counts[instance_index] = contrail->contrail_point_counts[instance_index];
		state->first_points[instance_index] = contrail_point_index;
		while (contrail_point_index != NONE)
		{
			struct contrail_point_datum *contrail_point = contrail_point_get(contrail_point_index);
			struct halo_sim_contrail_point *point;

			if (count >= maximum_point_count)
				return FALSE;
			point = &points[count++];
			point->index = contrail_point_index;
			point->next = contrail_point->next_contrail_point_index;
			point->flags = contrail_point->flags;
			point->state_index = contrail_point->state_index;
			point->pad = 0;
			point->time = contrail_point->time;
			point->delta = contrail_point->delta;
			point->density = contrail_point->density;
			point->width = contrail_point->width;
			point->location_leaf = contrail_point->location.leaf_index;
			point->location_cluster = contrail_point->location.cluster_index;
			point->location_bonus = contrail_point->location.bonus;
			memcpy(point->position, &contrail_point->position, sizeof(point->position));
			memcpy(point->velocity, &contrail_point->velocity, sizeof(point->velocity));
			contrail_point_index = contrail_point->next_contrail_point_index;
		}
	}
	*point_count = count;

	return TRUE;
}

void contrail_halo_sim_store(
	struct halo_sim_contrail_state const *state,
	struct halo_sim_contrail_point const *points,
	unsigned long point_count)
{
	struct contrail_datum *contrail =
		(struct contrail_datum *)datum_try_and_get(contrail_data, (long)state->contrail_index);
	unsigned long point_number;
	short instance_index;

	*get_global_local_random_seed_address() = state->local_random_seed;
	if (!contrail)
		return;
	contrail->flags = state->flags;
	contrail->definition_index = (long)state->definition_index;
	contrail->object_index = state->object_index;
	contrail->attachment_index = state->attachment_index;
	contrail->density_function_index = state->density_function_index;
	contrail->sequence_index = state->sequence_index;
	contrail->frame_index = state->frame_index;
	contrail->density = state->density;
	contrail->texture_offset_u = state->texture_offset_u;
	contrail->texture_offset_v = state->texture_offset_v;
	contrail->time_until_point = state->time_until_point;
	contrail->frame_time = state->frame_time;
	contrail->expired_dt = state->expired_dt;
	for (instance_index = 0; instance_index < MAXIMUM_CONTRAIL_INSTANCES; instance_index++)
	{
		contrail->contrail_point_counts[instance_index] = state->point_counts[instance_index];
		contrail->first_contrail_point_indices[instance_index] = state->first_points[instance_index];
	}
	for (point_number = 0; point_number < point_count; point_number++)
	{
		struct halo_sim_contrail_point const *point = &points[point_number];
		struct contrail_point_datum *contrail_point =
			(struct contrail_point_datum *)datum_try_and_get(contrail_point_data, point->index);

		if (!contrail_point)
			continue;
		contrail_point->next_contrail_point_index = point->next;
		contrail_point->flags = point->flags;
		contrail_point->state_index = point->state_index;
		contrail_point->time = point->time;
		contrail_point->delta = point->delta;
		contrail_point->density = point->density;
		contrail_point->width = point->width;
		contrail_point->location.leaf_index = point->location_leaf;
		contrail_point->location.cluster_index = point->location_cluster;
		contrail_point->location.bonus = point->location_bonus;
		memcpy(&contrail_point->position, point->position, sizeof(point->position));
		memcpy(&contrail_point->velocity, point->velocity, sizeof(point->velocity));
	}
}
