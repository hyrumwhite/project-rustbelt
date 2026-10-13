/*
NETWORK_DAMAGE_RECORDS.H

The distributed netcode's damage records the host keeps (network_damage.c),
as types, for the Rust port's layouts (crates/halo_map's network_damage,
tools/tag_layout/probe_net.c).
*/

#ifndef __NETWORK_DAMAGE_RECORDS_H
#define __NETWORK_DAMAGE_RECORDS_H

enum
{
	/* the weapons a player has carried lately (network_damage.c) */
	MAXIMUM_RECENT_WEAPONS = 8,
	/* the grenades a player has thrown lately */
	MAXIMUM_GRENADE_THROWS = 32,
	/* the explosions' hits of a tick told apart, a player's */
	MAXIMUM_EXPLOSIONS_PER_TICK = 64,
	/* the ticks of where players' units and vehicles were */
	TARGET_HISTORY_TICKS = 32,
	/* where each player's unit was every so many ticks, and how many of
	those are kept */
	PLAYER_TRAIL_INTERVAL_TICKS = 3,
	PLAYER_TRAIL_POINTS = 128,
};

/* how far from its firer what a source deals goes off (world units), in
how many ticks from when it is fired, and for how many of them what it
fires is carried at its firer's own speed (a grenade thrown on the run); a
distance below 0 where the tags do not bound it */
struct damage_reach
{
	real distance;
	real ticks;
	real carried_ticks;
};

/* each player's weapons of late, and hits left */
struct damage_player_record
{
	long definition_indices[MAXIMUM_RECENT_WEAPONS];
	long times[MAXIMUM_RECENT_WEAPONS];
	/* (the grenades thrown, not yet gone off: NONE for none) */
	long throw_times[MAXIMUM_GRENADE_THROWS];
	short throw_types[MAXIMUM_GRENADE_THROWS];
	long driven_time;
	/* (the blow of the player's unit with no weapon: units.c's
	unit_unarmed_melee_damage) */
	long unarmed_melee_damage_index;
	long unarmed_time;
	real hit_seconds;
	long hit_seconds_time;
};

/* the explosions whose hits came this tick, each paid for once, and the
objects each hit (each once), each player's */
struct damage_explosion
{
	long definition_index;
	real_point3d epicenter;
	long object_index;
};

struct damage_explosion_record
{
	long time;
	short count;
	struct damage_explosion explosions[MAXIMUM_EXPLOSIONS_PER_TICK];
};

/* where each player's unit and vehicle were at each of the last ticks (and
whether they drove it) */
struct damage_history_object
{
	long object_index;
	real_point3d position;
	real speed;
	boolean driving;
};

/* (MAXIMUM_TRACKED_PLAYERS is network_distributed.h's) */
struct damage_history_tick
{
	long time;
	struct damage_history_object objects[HALO_PORT_MAXIMUM_NETWORK_PLAYERS][2];
};

/* where each player's living unit was every few ticks, longer (NONE for no
time: none), how fast it or its vehicle went, and the size of the vehicle
it rode */
struct damage_trail_point
{
	long time;
	real_point3d position;
	real speed;
	real radius;
};

/* the vehicle damage in the globals' falling damage block, as vehicles.c
has it */
struct distributed_falling_damage
{
	byte unused0[0x2c];
	struct tag_reference maximum_distance_damage;
	struct tag_reference vehicle_hit_environment_damage_effect;
	struct tag_reference vehicle_killed_unit_damage_effect;
	struct tag_reference vehicle_collision_damage;
	struct tag_reference flaming_death_damage;
};

#endif // __NETWORK_DAMAGE_RECORDS_H
