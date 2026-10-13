/*
NETWORK_OBJECTS_RECORDS.H

The distributed netcode's records the host keeps of its objects
(network_objects.c), as types, for the Rust port's layouts (crates/halo_map's
network_host, tools/tag_layout/probe_net.c).
*/

#ifndef __NETWORK_OBJECTS_RECORDS_H
#define __NETWORK_OBJECTS_RECORDS_H

/* an object the host made (or deleted), as it tells its clients */
struct distributed_object_change
{
	byte change;
	byte flags;
	byte owner_player_index;
	/* which bitmap of its shaders it draws with, when not its tag's (0). An
	AI unit's actor variant sets it after the unit is made
	(actor_customize_unit): the Elite major's and commander's armor are 1
	and 2. Builds before it sent 0 here. */
	byte forced_shader_permutation_index;
	long object_index;
	long definition_index;
	short owner_team_index;
	short variant_number;
	real_point3d position;
	real_vector3d forward;
	real_vector3d up;
	real_vector3d translational_velocity;
	real_vector3d angular_velocity;
	real_rgb_color change_colors[NUMBER_OF_OBJECT_CHANGE_COLORS];
	byte region_permutations[MAXIMUM_REGIONS_PER_OBJECT];
};

/* an object's state the host sends: where it is and how it moves */
struct distributed_object_state
{
	long object_index;
	byte flags;
	byte pad;
	/* the client's tick, its low 16 bits: a vehicle prediction's, and
	(_distributed_object_predicted_bit) the one the host has it at */
	short time;
	real_point3d position;
	struct distributed_vector forward;
	struct distributed_vector up;
	struct distributed_vector translational_velocity;
	struct distributed_vector angular_velocity;
};

/* what a unit's inventory was last sent as (in all, and its weapons and
grenades), when to every client, and when its ammunition alone last changed
(NONE: not since); when it last carried anything (NONE: never) */
struct objects_host_inventory
{
	unsigned long checksum;
	unsigned long weapons_checksum;
	long time;
	long ammunition_time;
	long carried_time;
};

/* a client machine, found once a tick: where its players' living units
are, those units, and the vehicles they drive (with the player's absolute
index) */
struct objects_host_viewer_machine
{
	short unit_count;
	long unit_indices[MAXIMUM_LOCAL_PLAYERS];
	real_point3d origins[MAXIMUM_LOCAL_PLAYERS];
	short vehicle_count;
	long vehicle_indices[MAXIMUM_LOCAL_PLAYERS];
	short vehicle_player_indices[MAXIMUM_LOCAL_PLAYERS];
};

/* the client machines, and where every player's living unit is, for a
machine with none (dead or joining, it watches one of them) */
struct objects_host_viewers
{
	short count;
	long indices[HALO_PORT_MAXIMUM_NETWORK_MACHINES];
	short player_unit_count;
	real_point3d player_unit_origins[HALO_PORT_MAXIMUM_NETWORK_PLAYERS];
	struct objects_host_viewer_machine machines[HALO_PORT_MAXIMUM_NETWORK_MACHINES];
};

/* a client's player's latest vehicle prediction, taken at the next tick;
the vehicle it last took (NONE: none), at which of the client's ticks and its
own, and where that left the host's copy; and the anchor predictions are
measured from: one it took, at which of the client's ticks and its own, and
where the client had it; the velocity it took (what its next tick started
from); and how fast its own ticks sent the vehicle beyond the client's word,
the most of this span of VEHICLE_PREDICTION_SPEED_TICKS and of the one
before */
struct objects_host_vehicle_prediction
{
	boolean valid;
	long machine_index;
	struct distributed_object_state state;
	long accepted_vehicle_index;
	word accepted_time;
	long accepted_host_time;
	real_point3d accepted_host_position;
	word anchor_time;
	long anchor_host_time;
	real_point3d anchor_position;
	real_vector3d accepted_velocity;
	long speed_time;
	real speeds[2];
	/* the vehicle it last had on the ground (NONE: none), how high, and how
	fast up it went as it left it, and when (network_objects_note_vehicle_ground) */
	long ground_vehicle_index;
	long ground_time;
	/* the vehicle whose copy was at rest when it last took its word (NONE:
	none), and where it came to rest */
	long rest_vehicle_index;
	real_point3d rest_position;
	real ground_height;
	real ground_rise_speed;
};

/* a unit's flinch or death the host picked. type: units.h's
_unit_damage_animation_soft_ping and the rest; number: the pick's, so a
client tells a pick sent again from a new one */
struct distributed_damage_animation
{
	long unit_index;
	short animation_index;
	byte type;
	byte number;
};

/* ... still to be sent, and how many times more */
struct objects_host_damage_animation
{
	struct distributed_damage_animation animation;
	short sends;
};

/* a client: whether the host has told it all its objects, when it last
asked for them and how long until it asks again, whether it failed to make
one since it last asked (it says so when it asks again), how many of its own
objects it made since the last tick (of any kind: those of the kinds the
host has go), and whether to look through all the objects instead (the
host's all told, too many made, or made where it cannot say) */
struct objects_client_state
{
	boolean synchronized;
	boolean ask_again;
	boolean check_all;
	short new_object_count;
	long ready_time;
	long ready_interval;
};

/* a client's own players' vehicles where they were at its last ticks */
struct distributed_own_vehicle
{
	long time;
	long vehicle_index;
	real_point3d position;
};

/* the weapon a client's own player last picked up (the host said so), its
unit and when, until it is readied (definition NONE: none) */
struct distributed_picked_up_weapon
{
	long unit_index;
	long definition_index;
	long time;
};

/* for the automated tests' reports (network_test.c) */
struct objects_statistics
{
	long creates;
	long deletes;
	long create_failures;
	long own_objects_removed;
};

#endif
