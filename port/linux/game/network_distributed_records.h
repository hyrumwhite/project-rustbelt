/*
NETWORK_DISTRIBUTED_RECORDS.H

The distributed netcode's records the host keeps of its players
(network_distributed.c), as types, for the Rust port's layouts (crates/
halo_map's network_host, tools/tag_layout/probe_net.c).
*/

#ifndef __NETWORK_DISTRIBUTED_RECORDS_H
#define __NETWORK_DISTRIBUTED_RECORDS_H

/* what a player's unit was last sent as (a change goes to every client at
once) */
struct distributed_sent_unit
{
	byte flags;
	long unit_index;
	long vehicle_index;
	short seat_index;
};

/* where a client's player was when last alive (a client whose players are
dead watches from there) */
struct distributed_viewer
{
	boolean valid;
	short cluster_index;
	real_point3d position;
	real_vector3d aim;
};

/* a player's unit, as this machine has it (its bytes on the wire are fewer:
distributed_unit_state_write) */
struct distributed_unit_state
{
	byte player_index;
	byte flags;
	/* (dead) the player who killed it, NO_PLAYER for none */
	byte killing_player_index;
	byte unit_flags;
	/* the player's unit (the host's), NONE for none */
	long unit_index;
	/* the vehicle it rides and its seat, NONE for none */
	long vehicle_index;
	short seat_index;
	/* how camouflaged the unit is, of 255 */
	byte active_camouflage;
	byte pad;
	real_point3d position;
	struct distributed_vector velocity;
	struct distributed_vector forward;
	struct distributed_vector up;
	/* (_distributed_unit_predicted_bit) the client's tick, its low 16 bits */
	short predicted_time;
	/* (VITALITY_SCALE) */
	word body_vitality;
	word shield_vitality;
	/* what the shields' and the HUD's effects show */
	word current_body_damage;
	word recent_body_damage;
	word current_shield_damage;
	word recent_shield_damage;
	/* the player's powerups: how long each has left */
	short powerup_durations[NUMBER_OF_PLAYER_POWERUPS];
};

/* the host: a client's player's latest prediction, taken at the next tick,
and the client's tick of the one taken last and when */
struct distributed_prediction
{
	boolean valid;
	long time;
	struct distributed_unit_state state;
	long taken_time;
	long taken_host_time;
};

/* the host: the anchor, where it took each client's player to be (one
taken, a newer once a second: the next may be no further from it than the
player moves in the client's ticks since), at which of the client's ticks
and its own; and when it last took one, and where that left its unit */
struct distributed_accepted_prediction
{
	boolean valid;
	long unit_index;
	long time;
	long host_time;
	real_point3d position;
	long taken_host_time;
	real_point3d taken_host_position;
};

/* the host: what its own ticks did to its copy of each client's player's
unit on foot, beyond the client's word it took (network_distributed.c) */
struct distributed_host_speed
{
	long time;
	real speeds[2];
	real rises[2];
	real flight_speed;
	real flight_rise;
	long unit_index;
	real_vector3d velocity;
	boolean grounded;
	real ground_height;
	real top_height;
	/* the last tick it had the unit on the ground (or where it started) */
	long ground_time;
};

/* the host: each client's round trip, in ticks, and its jitter */
struct distributed_round_trip
{
	boolean valid;
	real average;
	real deviation;
};

/* the host, in co-op: where each client player's input last faced (how far
it turns between inputs is its looking around) */
struct distributed_input_facing
{
	boolean valid;
	real_euler_angles2d facing;
};

/* the host: each client machine's clock, measured each window against its
own, whether its game runs fast, for how many windows, whether its stream
said so too, and whether its datagrams' word was logged, unverified */
struct distributed_client_clock
{
	long latest_tick;
	long window_tick;
	unsigned long window_time;
	short fast_windows;
	boolean fast;
	boolean ahead_on_stream;
	boolean logged_unverified;
};

#endif
