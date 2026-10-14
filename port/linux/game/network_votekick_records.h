/*
NETWORK_VOTEKICK_RECORDS.H

The players' vote to kick a player: its messages' entries and the records
the host and a client keep (network_votekick.c), as types, for the Rust
port's layouts (crates/halo_map's network_votekick,
tools/tag_layout/probe_net.c).
*/

#ifndef __NETWORK_VOTEKICK_RECORDS_H
#define __NETWORK_VOTEKICK_RECORDS_H

/* p2p.h's P2P_HARDWARE_ID_SIZE */
#define VOTEKICK_HARDWARE_ID_SIZE 33
#define MAXIMUM_VOTEKICK_MACHINES HALO_PORT_MAXIMUM_NETWORK_MACHINES
/* the addresses (and hardware ids) remembered: kept out, cooling down,
protected; the oldest forgotten first */
#define MAXIMUM_VOTEKICK_REMEMBERED 32
/* a notice's text (network_distributed.c's MAXIMUM_NOTICE_LENGTH), and a
machine's players' names, with their ends */
#define VOTEKICK_NOTICE_SIZE 160
#define VOTEKICK_NAMES_SIZE 64
/* a hardware id a machine joining tells, as kept for the kept-out test
(longer, it is no id kept) */
#define VOTEKICK_JOINING_HARDWARE_ID_SIZE 64

struct distributed_votekick_request
{
	/* the player voted against (absolute index) */
	byte player_index;
	byte pad[3];
};

struct distributed_votekick_status
{
	/* the player voted against (absolute index), NO_PLAYER for no vote */
	byte player_index;
	byte votes;
	byte needed;
	byte seconds_left;
	byte flags;
	byte pad[3];
};

/* an address and hardware id remembered until a time (system_milliseconds) */
struct votekick_identity
{
	unsigned long address;
	char hardware_id[VOTEKICK_HARDWARE_ID_SIZE];
	unsigned long until;
	boolean valid;
};

/* the host: the vote */
struct votekick_vote
{
	boolean active;
	/* the machine voted against, and its players' first (for the clients) */
	long target_machine;
	long target_generation;
	short target_player;
	unsigned long target_address;
	char target_hardware_id[VOTEKICK_HARDWARE_ID_SIZE];
	char target_names[VOTEKICK_NAMES_SIZE];
	/* who started it, cooling down if it fails */
	unsigned long starter_address;
	char starter_hardware_id[VOTEKICK_HARDWARE_ID_SIZE];
	unsigned long started_at;
	/* the machines that voted (as they were at their slots), the host's */
	boolean voted[MAXIMUM_VOTEKICK_MACHINES];
	long voted_generations[MAXIMUM_VOTEKICK_MACHINES];
	boolean host_voted;
	/* as last counted, and told */
	short votes;
	short needed;
};

/* everything the vote keeps */
struct votekick_state
{
	/* the host: each machine slot's time in game, in the host's ticks, since
	it joined, and how many machines joined at it (a vote names the one it
	was); when it last asked something (system_milliseconds), and whether it
	has */
	long played_ticks[MAXIMUM_VOTEKICK_MACHINES];
	long generations[MAXIMUM_VOTEKICK_MACHINES];
	unsigned long request_times[MAXIMUM_VOTEKICK_MACHINES];
	boolean requested[MAXIMUM_VOTEKICK_MACHINES];
	struct votekick_vote vote;
	/* the host: no vote before this (a vote's end and the gap after) */
	unsigned long next_allowed;
	boolean gap;
	struct votekick_identity kept_out[MAXIMUM_VOTEKICK_REMEMBERED];
	struct votekick_identity cooling_starters[MAXIMUM_VOTEKICK_REMEMBERED];
	struct votekick_identity protected_targets[MAXIMUM_VOTEKICK_REMEMBERED];
	/* a client: the host's count, as last heard (the time: 0, none) */
	struct distributed_votekick_status status;
	unsigned long status_heard;
};

/* the steps' texts and message: a notice, a machine's players' names, the
count to a client */
struct votekick_texts
{
	char notice[VOTEKICK_NOTICE_SIZE];
	char names[VOTEKICK_NAMES_SIZE];
	struct
	{
		struct distributed_message_header header;
		struct distributed_votekick_status status;
	} message;
};

/* what the steps read of the game, gathered before them */
struct votekick_inputs
{
	/* system_milliseconds, and the game's tick */
	unsigned long now;
	long game_time;
	/* config.toml's network.votekick, votekick_minutes and
	votekick_ban_minutes */
	boolean enabled;
	long minutes;
	long ban_minutes;
	/* the host has a player of its own (local_player_get_next) */
	boolean host_has_player;
	/* the machines in the game with a player who has not quit, the host's own
	as NONE (votekick_machines), and the client machines
	(distributed_client_machines) */
	short machine_count;
	short client_count;
	long machine_indices[MAXIMUM_VOTEKICK_MACHINES + 1];
	long client_indices[MAXIMUM_VOTEKICK_MACHINES];
	/* each machine slot's real address (distributed_machine_real_address)
	and hardware id (as the host's server has it, sanitized) */
	unsigned long addresses[MAXIMUM_VOTEKICK_MACHINES];
	char hardware_ids[MAXIMUM_VOTEKICK_MACHINES][VOTEKICK_HARDWARE_ID_SIZE];
	/* the player a request or the count names: there, has quit, its machine
	(NONE: the host's) */
	boolean player_valid;
	boolean player_quit;
	long player_machine;
	/* a machine joining: its real address (distributed_real_address) and the
	hardware id it tells (none: empty) */
	unsigned long joining_address;
	char joining_hardware_id[VOTEKICK_JOINING_HARDWARE_ID_SIZE];
};

#endif
