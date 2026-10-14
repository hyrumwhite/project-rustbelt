/*
NETWORK_ACTORS_RECORDS.H

AI sync's messages' entries and the tables the host and a client keep
(network_actors.c), as types, for the Rust port's layouts (crates/halo_map's
network_actors, tools/tag_layout/probe_net.c).
*/

#ifndef __NETWORK_ACTORS_RECORDS_H
#define __NETWORK_ACTORS_RECORDS_H

#include "units/unit_control_data.h"

/* AI units tracked at once: every actor (halo_port_capacity.h's
HALO_PORT_MAXIMUM_ACTORS) and the units the cutscenes' recorded animations
drive */
#define MAXIMUM_NETWORK_ACTORS (HALO_PORT_MAXIMUM_ACTORS + 32)
#define MAXIMUM_ACTOR_ENTRIES_PER_MESSAGE 64

/* facing, aiming and looking vectors are sent as yaw and pitch */
enum
{
	_actor_angle_yaw,
	_actor_angle_pitch,
	NUMBER_OF_ACTOR_ANGLES
};

/* an AI unit's control and state at the end of the host's tick (like
struct distributed_unit_state for players) */
struct distributed_actor_state
{
	long unit_index;
	long vehicle_index;
	short seat_index;
	byte flags;
	/* the impulse's sequence number; impulse is NO_IMPULSE when there is none */
	byte impulse_number;
	byte impulse;
	byte animation_state;
	byte aiming_speed;
	byte primary_trigger;
	word control_flags;
	signed char throttle[3];
	byte active_camouflage;
	/* the speech's sequence number; 0 when there is none */
	byte speech_number;
	/* the unit's team (NO_TEAM for none): the AI sets it after the unit is
	made, so a client's copy would keep the team it was made with */
	byte team;
	short facing[NUMBER_OF_ACTOR_ANGLES];
	short aiming[NUMBER_OF_ACTOR_ANGLES];
	short looking[NUMBER_OF_ACTOR_ANGLES];
	short impulse_alignment;
	short run_blindly_angle;
	word body_vitality;
	word shield_vitality;
	/* the sound the unit started saying (a sound tag) */
	long speech_sound;
	/* the custom animation it started (unit_start_user_animation), and its
	sequence number; the graph is NONE when there is none */
	long user_animation_graph;
	short user_animation;
	byte user_animation_number;
	byte user_animation_interpolate;
	real_point3d position;
	struct distributed_vector velocity;
	struct distributed_vector forward;
	struct distributed_vector up;
};

/* the damage an AI unit is taking: its shield's flare and its body's wounds
are drawn by it */
struct distributed_actor_damage
{
	long unit_index;
	word current_shield_damage;
	word recent_shield_damage;
	word current_body_damage;
	word recent_body_damage;
};

struct distributed_actor_damage_message
{
	struct distributed_message_header header;
	struct distributed_actor_damage units[MAXIMUM_ACTOR_ENTRIES_PER_MESSAGE];
};

struct distributed_actor_state_message
{
	struct distributed_message_header header;
	struct distributed_actor_state states[MAXIMUM_ACTOR_ENTRIES_PER_MESSAGE];
};

/* host: what an actor gave its unit this tick */
struct host_actor
{
	long unit_index;
	/* got a control since the last send (noted during game_tick, sent after it) */
	boolean noted;
	/* its damage was sent last tick (and once more when it ends) */
	boolean damaged;
	struct unit_control_data control;
	/* the control flags of the last tick's entry */
	word sent_control_flags;
	/* the latest impulse, how many more entries carry it, and its number */
	short impulse;
	short impulse_sends;
	byte impulse_number;
	boolean impulse_aligned;
	real_vector2d impulse_alignment;
	/* the latest speech, how many more entries carry it, and its number */
	long speech_sound;
	short speech_sends;
	byte speech_number;
	/* the latest custom animation, the same way */
	long user_animation_graph;
	short user_animation;
	boolean user_animation_interpolate;
	short user_animation_sends;
	byte user_animation_number;
};

/* client: the latest the host sent about an AI unit */
struct client_actor
{
	long unit_index;
	long received_time;
	struct unit_control_data control;
	/* the impulse to play, and the number of the last one played */
	short impulse;
	byte impulse_number;
	byte played_impulse_number;
	boolean impulse_aligned;
	real_vector2d impulse_alignment;
	/* the speech to play (NONE when there is none), and the number of the last played */
	long speech_sound;
	byte speech_number;
	byte played_speech_number;
	/* the custom animation to play (graph NONE when there is none), and the
	number of the last played */
	long user_animation_graph;
	short user_animation;
	boolean user_animation_interpolate;
	byte user_animation_number;
	byte played_user_animation_number;
	boolean driven;
};

/* host: its table of AI units (the used entries, then one free), and the
numbers of every unit's impulses, speech and animations, from one counter,
so a unit that drops out of the table and comes back can't repeat the
number a client last played */
struct network_actors_host
{
	short count;
	byte event_number;
	struct host_actor actors[MAXIMUM_NETWORK_ACTORS];
};

/* client: its table of AI units, and where a unit is moved to
(network_objects_reconcile's arguments, for the step's call) */
struct network_actors_client
{
	short count;
	short pad;
	real_point3d reconcile_position;
	real_vector3d reconcile_forward;
	real_vector3d reconcile_up;
	real_vector3d reconcile_velocity;
	struct client_actor actors[MAXIMUM_NETWORK_ACTORS];
};

/* host: the tick's sends: the entries built, which go to every client this
tick whatever its period, and the messages */
struct network_actors_host_sends
{
	struct distributed_actor_state_message state_message;
	struct distributed_actor_damage_message damage_message;
	boolean urgent[MAXIMUM_NETWORK_ACTORS];
	struct distributed_actor_state states[MAXIMUM_NETWORK_ACTORS];
};

/* host: what the tick reads of the game, gathered before it: the clients'
machines */
struct network_actors_host_inputs
{
	long now;
	short machine_count;
	short pad;
	long machine_indices[HALO_PORT_MAXIMUM_NETWORK_MACHINES];
};

#endif
