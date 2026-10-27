/*
NETWORK_ACTORS.C

AI sync (port/linux/NETCODE.md).

Only the host runs the AI (game.c skips ai_update on clients). An actor
drives its unit the way a player does, through unit_control. The clients
have the units (network_objects.c); this file makes them move like the
host's.

Each tick the host records the control every actor gave its unit (a hook
in unit_control, and the bursts it holds the trigger for) and sends it to
each client with the unit's state: position, seat, shields and health,
camouflage, and whether it flees in a panic. Units near a client's players
are sent every tick, others less often, but a unit starting a one-tick
action (a jump, a grenade) goes to everyone that tick. Each tick a client
feeds the latest control into unit_control, the same path that drives a
remote player's unit, so the unit walks, aims, fires and throws as it does
on the host. The state corrects any drift.

What the AI starts once is numbered, so a client plays each once however
many times it receives it: animation impulses (dives, vaults), melee
attacks and leaps (which the AI starts directly, not through the control),
speech, and custom animations (outside co-op, whose events carry them).
Pain and death sounds aren't sent: a client plays those itself as it
replays the host's damage.

A client creates no actors of its own (actors.c): the host's are the only
ones. Deaths need nothing here: network_damage.c kills a client's copy of
an AI unit the same way it kills a player's.

port: each of the file's steps runs in Rust in Rust's mode (crates/halo_map's
network_actors), checked against the game's otherwise: noting what an actor
gave its unit, the host's tick, and a client's handling of the damage and
the states and its driving of the units. Their records are in
network_actors_records.h; what they end in of the game goes through
network_actors_call.
*/

/* ---------- headers */

#include "cseries.h"
#include "game/game.h"
#include "game/players.h"
#include "memory/memory_pool.h"
#include "networking/network_game_globals.h"
#include "objects/objects.h"
#include "objects/damage.h"
#include "sound/game_sound.h"
#include "sound/sound_definitions.h"
#include "units/units.h"
#include "units/unit_control_data.h"
#include "network_coop.h"
#include "network_distributed.h"
#include "network_actors_records.h"
#include "halo_sim.h" /* port: port/linux/game/halo_sim.c */

#include <math.h>

/* ---------- constants */

enum
{
	/* A client keeps applying a control for this long after the host last
	sent one. After that the unit is left alone: the actor let go of it, or
	the host went quiet. */
	CONTROL_HELD_TICKS = 2 * TICKS_PER_SECOND,
	/* an impulse is repeated in this many entries, in case one is lost */
	IMPULSE_REPEAT_TICKS = 3,
	/* the number of unit_start_animation_impulse impulses (private to units.c) */
	NUMBER_OF_UNIT_ANIMATION_IMPULSES = 14,
	NO_IMPULSE = 0xFF,
	NO_TEAM = 0xFF,

	/* An entry's impulse is one of the animation impulses or, past them, an
	action the AI starts directly: a melee attack (unit_melee_attack_begin)
	or a leap (unit_leap_begin). */
	_actor_action_melee = NUMBER_OF_UNIT_ANIMATION_IMPULSES,
	_actor_action_leap,
	NUMBER_OF_ACTOR_ACTIONS,

	/* control flags a unit acts on in one tick: a unit that sets one goes to
	every client that tick, however far away, so the action isn't missed */
	ONE_SHOT_CONTROL_FLAGS = FLAG(_unit_control_jump_bit) | FLAG(_unit_control_action_bit) |
		FLAG(_unit_control_weapon_reload_bit) | FLAG(_unit_control_throw_grenade_bit) |
		FLAG(_unit_control_swap_weapons_bit),
};

/* port: a client's handling of states: its regions (as many as the world
checks take) */
#define MAXIMUM_ACTOR_STATE_REGIONS 16

/* struct distributed_actor_state flags */
enum
{
	_distributed_actor_rides_bit = 0,
	_distributed_actor_shield_depleted_bit,
	_distributed_actor_shield_charging_bit,
	_distributed_actor_shield_over_charging_bit,
	_distributed_actor_camouflaged_bit,
	_distributed_actor_super_camouflaged_bit,
	/* the impulse turns the unit to impulse_alignment */
	_distributed_actor_impulse_aligned_bit,
	/* fleeing in a panic, toward run_blindly_angle (unit_start_running_blindly) */
	_distributed_actor_running_blindly_bit,
};

/* ---------- globals */

static struct network_actors_host actors_host;
static struct network_actors_client actors_client;
static struct network_actors_host_sends actors_host_sends;
static struct network_actors_host_inputs actors_host_inputs;

/* port: the steps' arguments (halo_sim.h) */
static struct halo_sim_network_actors_note actors_note_arguments;
static struct halo_sim_network_actors_host_tick actors_host_tick_arguments;
static struct halo_sim_network_actors_client actors_client_arguments;

/* ---------- prototypes */

/* cache_files.c */
void *cache_files_tag_instances(long *count);
/* objects.c */
struct memory_pool *objects_memory_pool(void);
/* network_objects.c */
long const *network_objects_client_has_table(void);

/* ---------- private code */

static void angles_pack(
	real_vector3d const *vector,
	short *angles)
{
	angles[_actor_angle_yaw] = distributed_angle_pack((real)atan2(vector->j, vector->i));
	angles[_actor_angle_pitch] = distributed_angle_pack((real)asin(PIN(vector->k, -1.0f, 1.0f)));
}

static void angles_unpack(
	short const *angles,
	real_vector3d *vector)
{
	real yaw = distributed_angle_unpack(angles[_actor_angle_yaw], FALSE);
	real pitch = distributed_angle_unpack(angles[_actor_angle_pitch], TRUE);

	vector->i = (real)(cos(pitch) * cos(yaw));
	vector->j = (real)(cos(pitch) * sin(yaw));
	vector->k = (real)sin(pitch);
}

/* host: the next event's number (never 0, which a client's new entry has
as played) */
static byte next_event_number(
	void)
{
	if (++actors_host.event_number == 0)
		actors_host.event_number = 1;
	return actors_host.event_number;
}

/* the unit's entry, adding one if needed; NULL if the table is full */
static struct host_actor *host_actor_for(
	long unit_index)
{
	short index;

	for (index = 0; index < actors_host.count; index++)
	{
		if (actors_host.actors[index].unit_index == unit_index)
			return &actors_host.actors[index];
	}
	if (actors_host.count >= MAXIMUM_NETWORK_ACTORS)
		return NULL;
	csmemset(&actors_host.actors[actors_host.count], 0, sizeof(actors_host.actors[0]));
	actors_host.actors[actors_host.count].unit_index = unit_index;
	actors_host.actors[actors_host.count].impulse = NONE;
	actors_host.actors[actors_host.count].speech_sound = NONE;
	actors_host.actors[actors_host.count].user_animation_graph = NONE;
	return &actors_host.actors[actors_host.count++];
}

static struct client_actor *client_actor_for(
	long unit_index)
{
	short index;

	for (index = 0; index < actors_client.count; index++)
	{
		if (actors_client.actors[index].unit_index == unit_index)
			return &actors_client.actors[index];
	}
	if (actors_client.count >= MAXIMUM_NETWORK_ACTORS)
		return NULL;
	csmemset(&actors_client.actors[actors_client.count], 0, sizeof(actors_client.actors[0]));
	actors_client.actors[actors_client.count].unit_index = unit_index;
	actors_client.actors[actors_client.count].impulse = NONE;
	actors_client.actors[actors_client.count].speech_sound = NONE;
	actors_client.actors[actors_client.count].user_animation_graph = NONE;
	return &actors_client.actors[actors_client.count++];
}

/* whether the unit exists, is alive, and isn't a player's */
static boolean actor_unit_valid(
	long unit_index)
{
	struct unit_datum *unit;

	if (!object_try_and_get_and_verify_type(unit_index, _object_mask_unit))
		return FALSE;
	unit = unit_get(unit_index);
	return !TEST_FLAG(unit->object.damage_flags, _object_dead_bit) && unit->unit.player_index == NONE;
}

/* port: the steps' calls, through the world's (the Rust port's) */
static long actors_call(
	long kind,
	long actor,
	long a,
	long b,
	real const *reals,
	short real_count)
{
	struct halo_sim_ai_call call;
	short index;

	memset(&call, 0, sizeof(call));
	call.kind = kind;
	call.actor = actor;
	call.a = a;
	call.b = b;
	for (index = 0; index < real_count; index++)
		call.input[index] = reals[index];
	network_actors_call(&call);
	return call.result;
}

/* port: damage_set_network_state through the world's calls */
static void actors_set_damage(
	long unit_index,
	struct damage_network_state const *damage)
{
	real reals[6];

	reals[0] = damage->body_vitality;
	reals[1] = damage->shield_vitality;
	reals[2] = damage->current_body_damage;
	reals[3] = damage->recent_body_damage;
	reals[4] = damage->current_shield_damage;
	reals[5] = damage->recent_shield_damage;
	actors_call(_network_actors_call_set_damage, NONE, unit_index,
		(damage->shield_depleted ? FLAG(0) : 0) | (damage->shield_charging ? FLAG(1) : 0) |
		(damage->shield_over_charging ? FLAG(2) : 0), reals, 6);
}

/* host: builds the entry to send for an AI unit */
static void actor_state_from_unit(
	struct host_actor const *actor,
	struct distributed_actor_state *state)
{
	struct unit_datum *unit = unit_get(actor->unit_index);
	struct damage_network_state damage;
	real camouflage = PIN(unit->unit.active_camouflage, 0.0f, 1.0f);

	csmemset(state, 0, sizeof(*state));
	state->unit_index = actor->unit_index;
	state->vehicle_index = NONE;
	state->seat_index = NONE;
	if (unit->object.parent_object_index != NONE && unit->unit.parent_seat_index != NONE)
	{
		SET_FLAG(state->flags, _distributed_actor_rides_bit, TRUE);
		state->vehicle_index = unit->object.parent_object_index;
		state->seat_index = unit->unit.parent_seat_index;
	}
	state->animation_state = (byte)actor->control.animation_state;
	state->aiming_speed = (byte)actor->control.aiming_speed;
	state->primary_trigger = (byte)(long)floor(PIN(actor->control.primary_trigger, 0.0f, 1.0f) * 255.0f + 0.5f);
	state->control_flags = actor->control.control_flags;
	/* for a burst the AI holds the trigger for (unit_persistent_control),
	send the flags and trigger the unit's update produced this tick */
	if (unit->unit.persistent_control_timer > 0)
	{
		state->control_flags = (word)(unit->unit.control_flags & (FLAG(NUMBER_OF_UNIT_CONTROL_FLAGS) - 1));
		state->primary_trigger = (byte)(long)floor(PIN(unit->unit.primary_trigger, 0.0f, 1.0f) * 255.0f + 0.5f);
	}
	state->throttle[0] = (signed char)(long)floor(PIN(actor->control.throttle.i, -1.0f, 1.0f) * 127.0f + 0.5f);
	state->throttle[1] = (signed char)(long)floor(PIN(actor->control.throttle.j, -1.0f, 1.0f) * 127.0f + 0.5f);
	state->throttle[2] = (signed char)(long)floor(PIN(actor->control.throttle.k, -1.0f, 1.0f) * 127.0f + 0.5f);
	angles_pack(&actor->control.facing_vector, state->facing);
	angles_pack(&actor->control.aiming_vector, state->aiming);
	angles_pack(&actor->control.looking_vector, state->looking);
	state->impulse = NO_IMPULSE;
	if (actor->impulse != NONE && actor->impulse_sends > 0)
	{
		state->impulse = (byte)actor->impulse;
		state->impulse_number = actor->impulse_number;
		SET_FLAG(state->flags, _distributed_actor_impulse_aligned_bit, actor->impulse_aligned);
		if (actor->impulse_aligned)
		{
			state->impulse_alignment = distributed_angle_pack(
				(real)atan2(actor->impulse_alignment.j, actor->impulse_alignment.i));
		}
	}
	state->speech_sound = NONE;
	if (actor->speech_sound != NONE && actor->speech_sends > 0)
	{
		state->speech_sound = actor->speech_sound;
		state->speech_number = actor->speech_number;
	}
	state->user_animation_graph = NONE;
	if (actor->user_animation_graph != NONE && actor->user_animation_sends > 0)
	{
		state->user_animation_graph = actor->user_animation_graph;
		state->user_animation = actor->user_animation;
		state->user_animation_interpolate = (byte)actor->user_animation_interpolate;
		state->user_animation_number = actor->user_animation_number;
	}
	if (TEST_FLAG(unit->unit.flags, _unit_running_blindly_bit))
	{
		SET_FLAG(state->flags, _distributed_actor_running_blindly_bit, TRUE);
		state->run_blindly_angle = distributed_angle_pack(unit->unit.run_blindly_angle);
	}
	damage_get_network_state(actor->unit_index, &damage);
	SET_FLAG(state->flags, _distributed_actor_shield_depleted_bit, damage.shield_depleted);
	SET_FLAG(state->flags, _distributed_actor_shield_charging_bit, damage.shield_charging);
	SET_FLAG(state->flags, _distributed_actor_shield_over_charging_bit, damage.shield_over_charging);
	state->body_vitality = distributed_vitality_pack(damage.body_vitality);
	state->shield_vitality = distributed_vitality_pack(damage.shield_vitality);
	SET_FLAG(state->flags, _distributed_actor_camouflaged_bit,
		TEST_FLAG(unit->unit.flags, _unit_active_camouflaged_bit));
	SET_FLAG(state->flags, _distributed_actor_super_camouflaged_bit,
		TEST_FLAG(unit->unit.flags, _unit_super_camouflaged_bit));
	state->active_camouflage = (byte)(long)floor(camouflage * 255.0f + 0.5f);
	state->team = unit->object.owner_team_index >= 0 && unit->object.owner_team_index < NO_TEAM ?
		(byte)unit->object.owner_team_index : NO_TEAM;
	state->position = unit->object.position;
	distributed_vector_pack(&unit->object.translational_velocity, DISTRIBUTED_VELOCITY_SCALE, &state->velocity);
	distributed_vector_pack(&unit->object.forward, DISTRIBUTED_UNIT_SCALE, &state->forward);
	distributed_vector_pack(&unit->object.up, DISTRIBUTED_UNIT_SCALE, &state->up);
}

/* Client: stores the entry's control and applies its state to the unit.
Returns FALSE for an entry with invalid values. */
static boolean actor_state_apply(
	struct distributed_actor_state const *state,
	long now)
{
	struct client_actor *actor;
	struct unit_datum *unit;
	struct damage_network_state damage;
	real_vector3d velocity;
	real_vector3d forward;
	real_vector3d up;

	if (!distributed_object_index_valid(state->unit_index) || !network_objects_client_has(state->unit_index))
		return FALSE;
	/* (a body the host's unit got back up from: a Flood combat form that
	feigned death, which it does only once its body is at rest. Left dead,
	the ragdoll was dragged about by the host's positions. The host sends
	only living units.) */
	if (object_try_and_get_and_verify_type(state->unit_index, _object_mask_unit) &&
		TEST_FLAG(unit_get(state->unit_index)->object.damage_flags, _object_dead_bit) &&
		TEST_FLAG(unit_get(state->unit_index)->object.flags, _object_at_rest_bit) &&
		unit_get(state->unit_index)->unit.player_index == NONE && state->body_vitality > 0)
	{
		actors_call(_network_actors_call_resurrect, NONE, state->unit_index, 0, NULL, 0);
	}
	if (!actor_unit_valid(state->unit_index) ||
		state->animation_state >= NUMBER_OF_UNIT_ANIMATION_STATES ||
		state->aiming_speed >= NUMBER_OF_UNIT_AIMING_SPEEDS ||
		!VALID_FLAGS(state->control_flags, NUMBER_OF_UNIT_CONTROL_FLAGS) ||
		(state->impulse != NO_IMPULSE && state->impulse >= NUMBER_OF_ACTOR_ACTIONS) ||
		(state->speech_sound != NONE && !distributed_tag_of_group(state->speech_sound, SOUND_DEFINITION_TAG)) ||
		(state->user_animation_graph != NONE &&
			!distributed_graph_animation(state->user_animation_graph, state->user_animation)) ||
		!distributed_point_valid(&state->position, UNIT_WORLD_BOUND))
	{
		return FALSE;
	}
	distributed_vector_unpack(&state->velocity, DISTRIBUTED_VELOCITY_SCALE, &velocity);
	distributed_vector_unpack(&state->forward, DISTRIBUTED_UNIT_SCALE, &forward);
	distributed_vector_unpack(&state->up, DISTRIBUTED_UNIT_SCALE, &up);
	if (!distributed_axes_make_valid(&forward, &up))
		return FALSE;
	actor = client_actor_for(state->unit_index);
	if (!actor)
		return TRUE;

	/* the control, applied every tick until the next entry */
	actor->received_time = now;
	actor->control.animation_state = (char)state->animation_state;
	actor->control.aiming_speed = (char)state->aiming_speed;
	actor->control.control_flags = state->control_flags;
	actor->control.weapon_index = NONE;
	actor->control.grenade_index = NONE;
	actor->control.zoom_level = NONE;
	actor->control.throttle.i = (real)state->throttle[0] / 127.0f;
	actor->control.throttle.j = (real)state->throttle[1] / 127.0f;
	actor->control.throttle.k = (real)state->throttle[2] / 127.0f;
	actor->control.primary_trigger = (real)state->primary_trigger / 255.0f;
	angles_unpack(state->facing, &actor->control.facing_vector);
	angles_unpack(state->aiming, &actor->control.aiming_vector);
	angles_unpack(state->looking, &actor->control.looking_vector);
	if (state->impulse != NO_IMPULSE && state->impulse_number != actor->played_impulse_number)
	{
		real yaw = distributed_angle_unpack(state->impulse_alignment, FALSE);

		actor->impulse = state->impulse;
		actor->impulse_number = state->impulse_number;
		actor->impulse_aligned = TEST_FLAG(state->flags, _distributed_actor_impulse_aligned_bit);
		actor->impulse_alignment.i = (real)cos(yaw);
		actor->impulse_alignment.j = (real)sin(yaw);
	}

	if (state->speech_sound != NONE && state->speech_number != actor->played_speech_number)
	{
		actor->speech_sound = state->speech_sound;
		actor->speech_number = state->speech_number;
	}
	if (state->user_animation_graph != NONE && state->user_animation_number != actor->played_user_animation_number)
	{
		actor->user_animation_graph = state->user_animation_graph;
		actor->user_animation = state->user_animation;
		actor->user_animation_interpolate = state->user_animation_interpolate != 0;
		actor->user_animation_number = state->user_animation_number;
	}

	/* the state: seat, shields and health, camouflage, running blindly, and
	position (a unit in a vehicle goes where the vehicle goes) */
	unit = unit_get(state->unit_index);
	if (TEST_FLAG(state->flags, _distributed_actor_rides_bit))
	{
		if (unit->object.parent_object_index != state->vehicle_index ||
			unit->unit.parent_seat_index != state->seat_index)
		{
			actors_call(_network_actors_call_set_seat, state->seat_index, state->unit_index, state->vehicle_index,
				NULL, 0);
		}
	}
	else if (unit->object.parent_object_index != NONE)
	{
		actors_call(_network_actors_call_set_seat, NONE, state->unit_index, NONE, NULL, 0);
	}
	damage_get_network_state(state->unit_index, &damage);
	damage.shield_depleted = TEST_FLAG(state->flags, _distributed_actor_shield_depleted_bit);
	damage.shield_charging = TEST_FLAG(state->flags, _distributed_actor_shield_charging_bit);
	damage.shield_over_charging = TEST_FLAG(state->flags, _distributed_actor_shield_over_charging_bit);
	damage.body_vitality = distributed_vitality_unpack(state->body_vitality);
	damage.shield_vitality = distributed_vitality_unpack(state->shield_vitality);
	actors_set_damage(state->unit_index, &damage);
	SET_FLAG(unit->unit.flags, _unit_active_camouflaged_bit,
		TEST_FLAG(state->flags, _distributed_actor_camouflaged_bit));
	SET_FLAG(unit->unit.flags, _unit_super_camouflaged_bit,
		TEST_FLAG(state->flags, _distributed_actor_super_camouflaged_bit));
	unit->unit.active_camouflage = (real)state->active_camouflage / 255.0f;
	/* (the game's 10 teams, game_allegiance.c) */
	if (state->team != NO_TEAM && state->team < 10)
		unit->object.owner_team_index = state->team;
	SET_FLAG(unit->unit.flags, _unit_running_blindly_bit,
		TEST_FLAG(state->flags, _distributed_actor_running_blindly_bit));
	if (TEST_FLAG(state->flags, _distributed_actor_running_blindly_bit))
		unit->unit.run_blindly_angle = distributed_angle_unpack(state->run_blindly_angle, FALSE);
	/* vehicles are placed only by the host's object states (network_objects.c) */
	if (unit->object.parent_object_index == NONE && unit->object.type != _object_type_vehicle)
	{
		real dx = state->position.x - unit->object.position.x;
		real dy = state->position.y - unit->object.position.y;
		real dz = state->position.z - unit->object.position.z;

		if (dx * dx + dy * dy + dz * dz > REMOTE_CORRECTION_TOLERANCE * REMOTE_CORRECTION_TOLERANCE)
		{
			/* port: (where it goes, for the call) */
			actors_client.reconcile_position = state->position;
			actors_client.reconcile_forward = forward;
			actors_client.reconcile_up = up;
			actors_client.reconcile_velocity = velocity;
			if (actors_call(_network_actors_call_reconcile, NONE, state->unit_index, 0, NULL, 0))
				actors_call(_network_actors_call_count_correction, NONE, NONE, 0, NULL, 0);
		}
	}
	return TRUE;
}

/* Client: plays a sound at the unit's head, as unit_dialogue_update starts
a unit's speech. */
static void speech_play(
	long unit_index,
	long sound_definition_index)
{
	struct object_marker marker;
	real_point3d position = *global_origin3d;
	real_vector3d forward = *global_forward3d;
	short node_index = 0;

	if (object_get_marker_by_name(unit_index, "head", &marker, 1))
	{
		node_index = marker.node_index;
		position = marker.node_matrix.position;
		forward = marker.node_matrix.forward;
	}
	object_impulse_sound_new(unit_index, sound_definition_index, node_index, &position, &forward, 1.0f);
}

/* host: what an actor gave its unit, or started it on (the note's kind) */
static void note_game(
	struct halo_sim_network_actors_note const *note)
{
	struct host_actor *actor = host_actor_for(note->unit_index);

	if (!actor)
		return;
	switch (note->kind)
	{
	case _network_actors_note_control:
		actor->noted = TRUE;
		csmemcpy(&actor->control, note->control, sizeof(actor->control));
		break;
	case _network_actors_note_action:
		actor->impulse = (short)note->action;
		actor->impulse_sends = IMPULSE_REPEAT_TICKS;
		actor->impulse_number = next_event_number();
		actor->impulse_aligned = note->aligned != 0;
		if (note->aligned)
		{
			actor->impulse_alignment.i = note->alignment_i;
			actor->impulse_alignment.j = note->alignment_j;
		}
		break;
	case _network_actors_note_speech:
		actor->speech_sound = note->sound;
		actor->speech_sends = IMPULSE_REPEAT_TICKS;
		actor->speech_number = next_event_number();
		break;
	case _network_actors_note_user_animation:
		actor->user_animation_graph = note->graph;
		actor->user_animation = (short)note->animation;
		actor->user_animation_interpolate = note->interpolate != 0;
		actor->user_animation_sends = IMPULSE_REPEAT_TICKS;
		actor->user_animation_number = next_event_number();
		break;
	}
}

/* the host's table, its used entries and the extra ones (new ones) */
static unsigned long host_table_size(
	long extra)
{
	long count = MIN(actors_host.count + extra, MAXIMUM_NETWORK_ACTORS);

	return (unsigned long)((byte *)&actors_host.actors[count] - (byte *)&actors_host);
}

static unsigned long client_table_size(
	long extra)
{
	long count = MIN(actors_client.count + extra, MAXIMUM_NETWORK_ACTORS);

	return (unsigned long)((byte *)&actors_client.actors[count] - (byte *)&actors_client);
}

/* port: the objects (their headers, and the pool their datums are in), as a
step's regions */
static long objects_regions(
	struct halo_sim_world_region *regions)
{
	struct memory_pool *pool = objects_memory_pool();
	long region_count = 0;

	regions[region_count].address = (unsigned long)object_header_data;
	regions[region_count++].size = sizeof(struct data_array);
	regions[region_count].address = (unsigned long)object_header_data->data;
	regions[region_count++].size = object_header_data->maximum_count * object_header_data->size;
	if (pool)
	{
		byte *used_end = pool->last_block ? (byte *)pool->last_block + pool->last_block->size : (byte *)pool->base_address;
		byte *end = used_end + 0x10000 < (byte *)pool->base_address + pool->size ?
			used_end + 0x10000 : (byte *)pool->base_address + pool->size;

		regions[region_count].address = (unsigned long)pool;
		regions[region_count++].size = sizeof(*pool);
		regions[region_count].address = (unsigned long)pool->base_address;
		regions[region_count++].size = (unsigned long)(end - (byte *)pool->base_address);
	}
	return region_count;
}

static void world_context(
	struct halo_sim_world_context *context,
	void const *arguments)
{
	memset(context, 0, sizeof(*context));
	context->game_time = game_time_get();
	context->object_header_data = (unsigned long)object_header_data;
	context->player_data = (unsigned long)player_data;
	context->arguments = (unsigned long)arguments;
}

/* host: a note of the kind for an AI unit */
/* port: Rust's in Rust's mode (crates/halo_map's network_actors), checked
against the game's otherwise. Its world: the arguments and the host's
table (with room for a new entry). */
static void note(
	struct halo_sim_network_actors_note *arguments)
{
	struct halo_sim_world_context context;
	struct halo_sim_world_region regions[2];
	int checking = FALSE;

	arguments->host = (unsigned long)&actors_host;
	regions[0].address = (unsigned long)arguments;
	regions[0].size = sizeof(*arguments);
	regions[1].address = (unsigned long)&actors_host;
	regions[1].size = host_table_size(1);
	world_context(&context, arguments);
	if (halo_sim_world_step_begin(_halo_sim_world_step_network_actors_note, &context, regions, 2, network_actors_call,
		&checking))
	{
		return;
	}
	note_game(arguments);
	if (checking)
		halo_sim_world_step_check(_halo_sim_world_step_network_actors_note);
}

/* host: an impulse or action for an AI unit, sent in the next few entries */
static void note_action(
	long unit_index,
	short action,
	real_vector2d const *alignment_vector)
{
	struct halo_sim_network_actors_note *arguments = &actors_note_arguments;

	if (game_connection() != _game_connection_network_server)
		return;
	memset(arguments, 0, sizeof(*arguments));
	arguments->kind = _network_actors_note_action;
	arguments->unit_index = unit_index;
	arguments->action = action;
	arguments->aligned = alignment_vector != NULL;
	if (alignment_vector)
	{
		arguments->alignment_i = alignment_vector->i;
		arguments->alignment_j = alignment_vector->j;
	}
	note(arguments);
}

/* host: the damage of the AI units taking some, to every client (and once
more when it ends, so a flare goes) */
static void host_send_damage(
	void)
{
	struct distributed_actor_damage_message *message = &actors_host_sends.damage_message;
	short count = 0;
	short index;

	for (index = 0; index < actors_host.count; index++)
	{
		struct host_actor *actor = &actors_host.actors[index];
		struct damage_network_state damage;
		boolean damaged;

		if (!actor_unit_valid(actor->unit_index))
			continue;
		damage_get_network_state(actor->unit_index, &damage);
		damaged = damage.current_shield_damage > 0.0f || damage.recent_shield_damage > 0.0f ||
			damage.current_body_damage > 0.0f || damage.recent_body_damage > 0.0f;
		if (!damaged && !actor->damaged)
			continue;
		actor->damaged = damaged;
		message->units[count].unit_index = actor->unit_index;
		message->units[count].current_shield_damage = distributed_vitality_pack(damage.current_shield_damage);
		message->units[count].recent_shield_damage = distributed_vitality_pack(damage.recent_shield_damage);
		message->units[count].current_body_damage = distributed_vitality_pack(damage.current_body_damage);
		message->units[count].recent_body_damage = distributed_vitality_pack(damage.recent_body_damage);
		if (++count == MAXIMUM_ACTOR_ENTRIES_PER_MESSAGE)
		{
			actors_call(_network_actors_call_send_damage, NONE, count, 0, NULL, 0);
			count = 0;
		}
	}
	if (count)
		actors_call(_network_actors_call_send_damage, NONE, count, 0, NULL, 0);
}

static void host_tick_game(
	void)
{
	struct network_actors_host_sends *sends = &actors_host_sends;
	struct network_actors_host_inputs const *inputs = &actors_host_inputs;
	long now = inputs->now;
	short state_count = 0;
	short index;
	short machine_number;

	for (index = 0; index < actors_host.count; index++)
	{
		struct host_actor *actor = &actors_host.actors[index];

		if (!actor_unit_valid(actor->unit_index) ||
			(!actor->noted && !TEST_FLAG(unit_get(actor->unit_index)->unit.flags, _unit_running_blindly_bit)))
		{
			actors_host.actors[index--] = actors_host.actors[--actors_host.count];
			continue;
		}
		actor_state_from_unit(actor, &sends->states[state_count]);
		sends->urgent[state_count] = sends->states[state_count].impulse != NO_IMPULSE ||
			sends->states[state_count].speech_sound != NONE ||
			sends->states[state_count].user_animation_graph != NONE ||
			(sends->states[state_count].control_flags & ~actor->sent_control_flags & ONE_SHOT_CONTROL_FLAGS) != 0;
		actor->sent_control_flags = sends->states[state_count].control_flags;
		state_count++;
		actor->noted = FALSE;
		if (actor->impulse_sends > 0)
			actor->impulse_sends--;
		if (actor->speech_sends > 0)
			actor->speech_sends--;
		if (actor->user_animation_sends > 0)
			actor->user_animation_sends--;
	}
	host_send_damage();
	for (machine_number = 0; machine_number < inputs->machine_count; machine_number++)
	{
		long machine_index = inputs->machine_indices[machine_number];
		short count = 0;

		for (index = 0; index < state_count; index++)
		{
			struct distributed_actor_state const *state = &sends->states[index];
			short period = (short)actors_call(_network_actors_call_send_period, NONE, machine_index, 0,
				&state->position.x, 3);

			if (!sends->urgent[index] && (now + DATUM_INDEX_TO_ABSOLUTE_INDEX(state->unit_index)) % period != 0)
				continue;
			sends->state_message.states[count++] = *state;
			if (count == MAXIMUM_ACTOR_ENTRIES_PER_MESSAGE || count == DATAGRAM_ENTRIES(struct distributed_actor_state))
			{
				actors_call(_network_actors_call_send_states, NONE, machine_index, count, NULL, 0);
				count = 0;
			}
		}
		if (count)
			actors_call(_network_actors_call_send_states, NONE, machine_index, count, NULL, 0);
	}
}

static void handle_damage_game(
	struct distributed_actor_damage const *units,
	short count)
{
	short index;

	for (index = 0; index < count; index++)
	{
		struct damage_network_state damage;

		if (!distributed_object_index_valid(units[index].unit_index) ||
			!network_objects_client_has(units[index].unit_index) || !actor_unit_valid(units[index].unit_index))
		{
			continue;
		}
		damage_get_network_state(units[index].unit_index, &damage);
		damage.current_shield_damage = distributed_vitality_unpack(units[index].current_shield_damage);
		damage.recent_shield_damage = distributed_vitality_unpack(units[index].recent_shield_damage);
		damage.current_body_damage = distributed_vitality_unpack(units[index].current_body_damage);
		damage.recent_body_damage = distributed_vitality_unpack(units[index].recent_body_damage);
		actors_set_damage(units[index].unit_index, &damage);
	}
}

static void handle_states_game(
	struct distributed_actor_state const *states,
	short count,
	long now)
{
	short index;

	for (index = 0; index < count; index++)
		actor_state_apply(&states[index], now);
}

static void drive_game(
	long now)
{
	short index;

	for (index = 0; index < actors_client.count; index++)
	{
		struct client_actor *actor = &actors_client.actors[index];

		if (!actor_unit_valid(actor->unit_index) || now - actor->received_time > CONTROL_HELD_TICKS)
		{
			if (actor->driven && object_try_and_get_and_verify_type(actor->unit_index, _object_mask_unit))
				actors_call(_network_actors_call_set_actively_controlled, NONE, actor->unit_index, FALSE, NULL, 0);
			actors_client.actors[index--] = actors_client.actors[--actors_client.count];
			continue;
		}
		/* vehicles follow the host's transform instead (network_objects.c) */
		if (object_get(actor->unit_index)->object.type == _object_type_vehicle)
			continue;
		if (!actor->driven)
		{
			actors_call(_network_actors_call_set_actively_controlled, NONE, actor->unit_index, TRUE, NULL, 0);
			actor->driven = TRUE;
		}
		actors_call(_network_actors_call_unit_control, NONE, actor->unit_index, (long)&actor->control, NULL, 0);
		if (actor->impulse != NONE && actor->impulse_number != actor->played_impulse_number)
		{
			real alignment[2];

			alignment[0] = actor->impulse_alignment.i;
			alignment[1] = actor->impulse_alignment.j;
			actors_call(_network_actors_call_action, actor->impulse, actor->unit_index, actor->impulse_aligned,
				alignment, 2);
			actor->played_impulse_number = actor->impulse_number;
			actor->impulse = NONE;
		}
		if (actor->speech_sound != NONE)
		{
			actors_call(_network_actors_call_speech, NONE, actor->unit_index, actor->speech_sound, NULL, 0);
			actor->played_speech_number = actor->speech_number;
			actor->speech_sound = NONE;
		}
		if (actor->user_animation_graph != NONE)
		{
			real interpolate = actor->user_animation_interpolate ? 1.0f : 0.0f;

			actors_call(_network_actors_call_user_animation, actor->user_animation, actor->unit_index,
				actor->user_animation_graph, &interpolate, 1);
			actor->played_user_animation_number = actor->user_animation_number;
			actor->user_animation_graph = NONE;
		}
	}
}

/* port: a client's step's arguments, and its first regions: the arguments
and the client's table (with extra entries) */
static long client_step_regions(
	struct halo_sim_network_actors_client *arguments,
	struct halo_sim_world_region *regions,
	long extra)
{
	long region_count = 0;

	arguments->client = (unsigned long)&actors_client;
	arguments->client_has = (unsigned long)network_objects_client_has_table();
	arguments->tag_instances = (unsigned long)cache_files_tag_instances(&arguments->tag_count);
	arguments->now = game_time_get();
	regions[region_count].address = (unsigned long)arguments;
	regions[region_count++].size = sizeof(*arguments);
	regions[region_count].address = (unsigned long)&actors_client;
	regions[region_count++].size = client_table_size(extra);
	return region_count;
}

/* ---------- public code */

/* port: the steps' calls (halo_sim.h's _network_actors_call_*) */
void network_actors_call(
	struct halo_sim_ai_call *call)
{
	int opened = halo_sim_world_event_begin(call);

	switch (call->kind)
	{
	case _network_actors_call_send_period:
	{
		real_point3d position;

		position.x = call->input[0];
		position.y = call->input[1];
		position.z = call->input[2];
		call->result = network_objects_send_period(call->a, &position);
		break;
	}
	case _network_actors_call_send_damage:
		distributed_send(&actors_host_sends.damage_message, _distributed_message_actor_damage, (short)call->a,
			(word)(sizeof(actors_host_sends.damage_message.header) +
				call->a * sizeof(actors_host_sends.damage_message.units[0])), _distributed_to_clients);
		break;
	case _network_actors_call_send_states:
		distributed_send_to_machine(call->a, &actors_host_sends.state_message, _distributed_message_actor_states,
			(short)call->b, (word)(sizeof(actors_host_sends.state_message.header) +
				call->b * sizeof(struct distributed_actor_state)));
		break;
	case _network_actors_call_resurrect:
		unit_port_resurrect(call->a);
		break;
	case _network_actors_call_set_seat:
		network_objects_set_seat(call->a, call->b, (short)call->actor);
		break;
	case _network_actors_call_set_damage:
	{
		struct damage_network_state damage;

		damage.shield_depleted = TEST_FLAG(call->b, 0);
		damage.shield_charging = TEST_FLAG(call->b, 1);
		damage.shield_over_charging = TEST_FLAG(call->b, 2);
		damage.body_vitality = call->input[0];
		damage.shield_vitality = call->input[1];
		damage.current_body_damage = call->input[2];
		damage.recent_body_damage = call->input[3];
		damage.current_shield_damage = call->input[4];
		damage.recent_shield_damage = call->input[5];
		damage_set_network_state(call->a, &damage);
		break;
	}
	case _network_actors_call_reconcile:
		call->result = network_objects_reconcile(call->a, &actors_client.reconcile_position,
			&actors_client.reconcile_forward, &actors_client.reconcile_up, &actors_client.reconcile_velocity, NULL,
			REMOTE_BLEND_DISTANCE);
		break;
	case _network_actors_call_count_correction:
		distributed_count_correction();
		break;
	case _network_actors_call_set_actively_controlled:
		unit_set_actively_controlled(call->a, (boolean)call->b);
		break;
	case _network_actors_call_unit_control:
		unit_control(call->a, (struct unit_control_data const *)call->b);
		break;
	case _network_actors_call_action:
	{
		real_vector2d alignment_vector;
		real_vector2d *alignment = call->b ? &alignment_vector : NULL;

		alignment_vector.i = call->input[0];
		alignment_vector.j = call->input[1];
		if (call->actor == _actor_action_melee)
			unit_melee_attack_begin(call->a, FALSE, alignment);
		else if (call->actor == _actor_action_leap)
			unit_leap_begin(call->a, alignment);
		else
			unit_start_animation_impulse(call->a, (short)call->actor, alignment);
		break;
	}
	case _network_actors_call_speech:
		speech_play(call->a, call->b);
		break;
	case _network_actors_call_user_animation:
		unit_port_play_user_animation(call->a, call->b, (short)call->actor, call->input[0] != 0.0f, 0);
		break;
	}
	halo_sim_world_event_end(call, opened);
}

void network_actors_new_game(
	void)
{
	actors_host.count = 0;
	actors_client.count = 0;
}

/* host: unit_control calls this when an actor drives its unit */
void network_actors_note_control(
	long unit_index,
	struct unit_control_data const *control_data)
{
	struct halo_sim_network_actors_note *arguments = &actors_note_arguments;

	if (game_connection() != _game_connection_network_server)
		return;
	memset(arguments, 0, sizeof(*arguments));
	arguments->kind = _network_actors_note_control;
	arguments->unit_index = unit_index;
	csmemcpy(arguments->control, control_data, sizeof(arguments->control));
	note(arguments);
}

/* host: unit_start_animation_impulse calls this for an AI unit */
void network_actors_note_impulse(
	long unit_index,
	short animation_impulse,
	real_vector2d const *alignment_vector)
{
	note_action(unit_index, animation_impulse, alignment_vector);
}

/* host: unit_melee_attack_begin calls this when an AI unit starts a melee attack */
void network_actors_note_melee(
	long unit_index,
	real_vector2d const *alignment_vector)
{
	note_action(unit_index, _actor_action_melee, alignment_vector);
}

/* host: unit_leap_begin calls this when an AI unit leaps */
void network_actors_note_leap(
	long unit_index,
	real_vector2d const *alignment_vector)
{
	note_action(unit_index, _actor_action_leap, alignment_vector);
}

/* host: unit_dialogue_update calls this when an AI unit starts saying
something (not a pain or death sound, which clients play themselves) */
void network_actors_note_speech(
	long unit_index,
	long sound_definition_index)
{
	struct halo_sim_network_actors_note *arguments = &actors_note_arguments;

	if (game_connection() != _game_connection_network_server)
		return;
	memset(arguments, 0, sizeof(*arguments));
	arguments->kind = _network_actors_note_speech;
	arguments->unit_index = unit_index;
	arguments->sound = sound_definition_index;
	note(arguments);
}

/* host: unit_start_user_animation calls this when an AI unit starts a
custom animation (co-op's events carry them in co-op) */
void network_actors_note_user_animation(
	long unit_index,
	long animation_graph_index,
	short animation_index,
	boolean interpolate)
{
	struct halo_sim_network_actors_note *arguments = &actors_note_arguments;

	if (game_connection() != _game_connection_network_server || network_coop_active())
		return;
	memset(arguments, 0, sizeof(*arguments));
	arguments->kind = _network_actors_note_user_animation;
	arguments->unit_index = unit_index;
	arguments->graph = animation_graph_index;
	arguments->animation = animation_index;
	arguments->interpolate = interpolate;
	note(arguments);
}

/* Host, after each tick: sends each client the AI units driven this tick.
Units near the client's players go every tick, others less often. A unit
with a fresh impulse, action, speech or animation goes to everyone in each
of the next IMPULSE_REPEAT_TICKS ticks, and one starting a one-tick action
goes that tick. A unit no actor drove this tick is dropped from the table,
unless it is fleeing: a unit running blindly moves by itself, without its
actor's control, and dropped it stood running in place on the clients. */
/* port: Rust's in Rust's mode (crates/halo_map's network_actors), checked
against the game's otherwise. Its world: the arguments, what it reads of
the game (the clients' machines), the host's table and the sends. (Not the
objects, which it only reads and its calls don't change: copied for each of
its many calls, they slowed the game.) */
void network_actors_host_tick(
	void)
{
	struct halo_sim_network_actors_host_tick *arguments = &actors_host_tick_arguments;
	struct network_actors_host_inputs *inputs = &actors_host_inputs;
	struct halo_sim_world_context context;
	struct halo_sim_world_region regions[4];
	long region_count = 0;
	int checking = FALSE;

	csmemset(inputs, 0, sizeof(*inputs));
	inputs->now = game_time_get();
	inputs->machine_count = distributed_client_machines(inputs->machine_indices, HALO_PORT_MAXIMUM_NETWORK_MACHINES);
	memset(arguments, 0, sizeof(*arguments));
	arguments->host = (unsigned long)&actors_host;
	arguments->sends = (unsigned long)&actors_host_sends;
	arguments->inputs = (unsigned long)inputs;
	regions[region_count].address = (unsigned long)arguments;
	regions[region_count++].size = sizeof(*arguments);
	regions[region_count].address = (unsigned long)inputs;
	regions[region_count++].size = sizeof(*inputs);
	regions[region_count].address = (unsigned long)&actors_host;
	regions[region_count++].size = host_table_size(0);
	regions[region_count].address = (unsigned long)&actors_host_sends;
	regions[region_count++].size = sizeof(actors_host_sends);
	world_context(&context, arguments);
	if (halo_sim_world_step_begin(_halo_sim_world_step_network_actors_host_tick, &context, regions, region_count,
		network_actors_call, &checking))
	{
		return;
	}
	host_tick_game();
	if (checking)
		halo_sim_world_step_check(_halo_sim_world_step_network_actors_host_tick);
}

word network_actors_damage_entry_size(
	void)
{
	return sizeof(struct distributed_actor_damage);
}

/* client: the damage the host's AI units are taking */
/* port: Rust's in Rust's mode (crates/halo_map's network_actors), checked
against the game's otherwise. Its world: the arguments. (It writes nothing
of its own; its calls set what of the units it reads to what it was, but
for the damage, which it doesn't read.) */
void network_actors_handle_damage(
	void const *entries,
	short count)
{
	struct halo_sim_network_actors_client *arguments = &actors_client_arguments;
	struct halo_sim_world_context context;
	struct halo_sim_world_region regions[2];
	long region_count;
	int checking = FALSE;

	memset(arguments, 0, sizeof(*arguments));
	arguments->count = count;
	arguments->entries = (unsigned long)entries;
	region_count = client_step_regions(arguments, regions, 0) - 1;
	world_context(&context, arguments);
	if (halo_sim_world_step_begin(_halo_sim_world_step_network_actors_damage, &context, regions, region_count,
		network_actors_call, &checking))
	{
		return;
	}
	handle_damage_game(entries, count);
	if (checking)
		halo_sim_world_step_check(_halo_sim_world_step_network_actors_damage);
}

word network_actors_entry_size(
	void)
{
	return sizeof(struct distributed_actor_state);
}

/* client: entries from the host */
/* port: Rust's in Rust's mode (crates/halo_map's network_actors), checked
against the game's otherwise. Its world: the arguments, the client's table
(with room for a new entry each) and the units the entries name (their
datums), or for more entries than that fits, the objects. */
void network_actors_handle_states(
	void const *entries,
	short count)
{
	struct distributed_actor_state const *states = entries;
	struct halo_sim_network_actors_client *arguments = &actors_client_arguments;
	struct halo_sim_world_context context;
	struct halo_sim_world_region regions[MAXIMUM_ACTOR_STATE_REGIONS];
	long region_count;
	int checking = FALSE;
	short index;

	memset(arguments, 0, sizeof(*arguments));
	arguments->count = count;
	arguments->entries = (unsigned long)entries;
	region_count = client_step_regions(arguments, regions, count);
	if (region_count + count > MAXIMUM_ACTOR_STATE_REGIONS)
		region_count += objects_regions(regions + region_count);
	else
	{
		for (index = 0; index < count; index++)
		{
			struct object_header_datum *header = object_try_and_get_and_verify_type(states[index].unit_index,
				_object_mask_unit) ? object_header_get(states[index].unit_index) : NULL;

			if (header)
			{
				regions[region_count].address = (unsigned long)header->datum;
				regions[region_count++].size = (unsigned long)header->data_size;
			}
		}
	}
	world_context(&context, arguments);
	if (halo_sim_world_step_begin(_halo_sim_world_step_network_actors_states, &context, regions, region_count,
		network_actors_call, &checking))
	{
		return;
	}
	handle_states_game(entries, count, arguments->now);
	if (checking)
		halo_sim_world_step_check(_halo_sim_world_step_network_actors_states);
}

/* Client: called at the point in the tick where the host runs the AI.
Drives each AI unit with the last control the host sent. */
/* port: Rust's in Rust's mode (crates/halo_map's network_actors), checked
against the game's otherwise. Its world: the arguments and the client's
table. (Not the objects: what it reads of them its calls don't change, and
copied for each of its many calls, they slowed the game.) */
void network_actors_drive(
	void)
{
	struct halo_sim_network_actors_client *arguments = &actors_client_arguments;
	struct halo_sim_world_context context;
	struct halo_sim_world_region regions[2];
	long region_count;
	int checking = FALSE;

	memset(arguments, 0, sizeof(*arguments));
	region_count = client_step_regions(arguments, regions, 0);
	world_context(&context, arguments);
	if (halo_sim_world_step_begin(_halo_sim_world_step_network_actors_drive, &context, regions, region_count,
		network_actors_call, &checking))
	{
		return;
	}
	drive_game(arguments->now);
	if (checking)
		halo_sim_world_step_check(_halo_sim_world_step_network_actors_drive);
}
