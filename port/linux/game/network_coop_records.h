/*
NETWORK_COOP_RECORDS.H

Network co-op's messages' entries and the records the host keeps
(network_coop.c), as types, for the Rust port's layouts (crates/halo_map's
network_coop, tools/tag_layout/probe_net.c).
*/

#ifndef __NETWORK_COOP_RECORDS_H
#define __NETWORK_COOP_RECORDS_H

/* where one of the host's scenery or machines is, when it moved */
struct distributed_coop_object_transform
{
	short name_index;
	short pad;
	long object_index;
	long definition_index;
	real_point3d position;
	struct distributed_vector forward;
	struct distributed_vector up;
};

/* how one of the host's objects looks (its permutations, scale), when that
changed */
struct distributed_coop_object_look
{
	short name_index;
	short pad;
	long object_index;
	long definition_index;
	byte region_permutations[MAXIMUM_REGIONS_PER_OBJECT];
	real scale;
};

/* where one of the host's devices is: its position (open, closed, how far
up an elevator has gone) and power, which the device groups' values only aim
it at */
struct distributed_coop_device_state
{
	short name_index;
	byte moving;
	byte pad;
	long object_index;
	long definition_index;
	real position;
	real power;
};

/* the host: where each scenery or machine was last sent, by its absolute
index, and whether it has moved since the map placed it */
struct coop_sent_transform
{
	long object_index;
	boolean moved;
	real_point3d position;
	real_vector3d forward;
};

/* the host: each device's position and power as last sent, by its absolute
index, and whether they have changed since the map placed it */
struct coop_sent_device
{
	long object_index;
	boolean moved;
	real position;
	real power;
};

/* the host: how each object looked when last sent, by its absolute index,
and whether that has changed since it was made */
struct coop_sent_look
{
	long object_index;
	boolean changed;
	byte region_permutations[MAXIMUM_REGIONS_PER_OBJECT];
	real scale;
};

#endif
