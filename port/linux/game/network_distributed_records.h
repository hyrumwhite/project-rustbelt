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

#endif
