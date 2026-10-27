/*
COOP_ENEMIES_RECORDS.H

Network co-op's extra enemies' records (coop_enemies.c), as types, for the
Rust port's layouts (crates/halo_map's coop_enemies,
tools/tag_layout/probe_net.c).
*/

#ifndef __COOP_ENEMIES_RECORDS_H
#define __COOP_ENEMIES_RECORDS_H

/* the vehicles a script loaded riders into, the riders seated in each, and
the riders kept for want of a seat */
#define COOP_ENEMIES_MAXIMUM_RIDING_VEHICLES 32
#define COOP_ENEMIES_MAXIMUM_SEATED_RIDERS 16
#define COOP_ENEMIES_MAXIMUM_KEPT_RIDERS 512

/* a vehicle a script loaded riders into: its seated ones, and whether they
have begun to get out */
struct coop_riding_vehicle
{
	long vehicle_index;
	long seated_unit_indices[COOP_ENEMIES_MAXIMUM_SEATED_RIDERS];
	short seat_indices[COOP_ENEMIES_MAXIMUM_SEATED_RIDERS];
	short seated_count;
	boolean releasing;
	short released_count;
	long next_release_time;
};

/* a rider left without a seat, to place once its vehicle's get out */
struct coop_kept_rider
{
	long vehicle_index;
	long variant_definition_index;
	long encounter_index;
	short squad_index;
};

/* EXTRA ENEMIES, as the game began with it, and the riders kept */
struct coop_enemies_state
{
	/* EXTRA ENEMIES, as the game began with it: its choice, and the amount
	of PER PLAYER (a percentage) and of STATIC MULTIPLIER */
	short mode;
	short percent;
	short multiplier;
	struct coop_riding_vehicle vehicles[COOP_ENEMIES_MAXIMUM_RIDING_VEHICLES];
	short vehicle_count;
	struct coop_kept_rider riders[COOP_ENEMIES_MAXIMUM_KEPT_RIDERS];
	short rider_count;
};

#endif
