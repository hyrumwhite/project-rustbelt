/*
NETWORK_VOTEKICK.C

The players' vote to kick a player (port/linux/NETCODE.md). A player starts
one by picking another's name on the scoreboard (game_engine.c) or with the
console's "votekick <name>"; the others vote the same way. The host alone
counts, and kicks the machine of the player once enough have voted. A
vote's power is against the game's other players, so the host decides
every part of it and trusts nothing a client says but which player it
votes against:

- A vote counts only from its machine's own stream: a datagram's source
  address is all that names its machine, and anyone can send one as from
  another's.
- One vote a person. The votes and the players who may vote are counted by
  their machines' real addresses (an internet play peer's endpoint, not its
  stand-in, which the peer can make anew), and the votes again by hardware
  id: two machines of one address or one hardware id are one vote. The
  hardware id a machine tells the host joining (anyone's it likes), so it
  only ever takes votes away: the players who may vote are counted by their
  addresses alone, or a machine could copy another's id and so lower the
  votes needed.
- Time played: on this host, counted in its own ticks of game, not told
  by the client: votekick_minutes (config.toml) to start a vote, and a
  part of that to vote, so that a player who has only just joined (or a
  cheater's second machine) neither starts nor counts. Joining again starts
  over.
- More than half of the players who may vote, the player voted against
  counted among them (who never votes for it), and at least two: one team
  alone kicks nobody from the other in an even game, and two people never
  kick each other.
- One vote at a time, for VOTE_SECONDS, then a pause before the next. A
  player whose vote failed starts none for STARTER_COOLDOWN_SECONDS, and a
  player a vote failed against is not voted against again for
  TARGET_PROTECTION_SECONDS, both by address and hardware id.
- The host's own players are never voted against; the host kicks and bans
  as it likes (network_server_manager.c), and its machine votes as any
  other.
- Only the host speaks: what a client did wrong is told to it alone, at
  most once a second; the votes to everyone, only as they change.
- A machine kicked by a vote is kept out of the host's games, by address
  and hardware id, for votekick_ban_minutes. One that leaves during a vote
  against it is too, as if the vote passed, and while the vote runs it is
  refused if it tries to join again: no one dodges a vote by leaving.

A client hears the host's count every second (and as it changes), for the
scoreboard. A build without votes drops both messages as kinds it does not
know: its players cannot vote, and count as players who did not.

port: the vote's decisions run in Rust in Rust's mode (crates/halo_map's
network_votekick), checked against the game's otherwise: the host taking a
request (a client's, or its own machine's), its tick, a client hearing the
count, and the host's server's questions (a machine joined, one kept out, a
new game). Their records are in network_votekick_records.h; what they read
of the game is gathered before them (struct votekick_inputs), and what they
end in (a machine's players' names, notices, the kick, the count sent, the
log) goes through network_votekick_call. What the scoreboard and the
console ask (the count as shown, a player's name, the host's kick and ban,
sending a request) stays the game's.
*/

#include "cseries.h"
#include "cseries/errors.h"
#include "memory/data.h"
#include "game/game.h"
#include "game/players.h"
#include "networking/network_game_globals.h"
#include "network_distributed.h"
#include "network_votekick.h"
#include "network_votekick_records.h"
#include "halo_sim.h" /* port: port/linux/game/halo_sim.c */

#include <stdio.h>

/* port_config.c's, network_server_manager.c's, cseries_windows.c's */
int config_boolean(const char *name);
long config_integer(const char *name);
void network_game_server_kick_machine(long machine_index, boolean kept_out);
boolean network_game_server_kick_machine_of_player(long machine_index, boolean ban);
char const *network_game_server_machine_hardware_id(long machine_index);
void p2p_hardware_id_sanitize(char *destination, int size, const char *source);
unsigned long system_milliseconds(void);
void console_warning(const char *format, ...);

/* ---------- constants */

enum
{
	VOTE_SECONDS = 45,
	/* after a vote, before the next */
	VOTE_GAP_SECONDS = 30,
	STARTER_COOLDOWN_SECONDS = 5 * 60,
	TARGET_PROTECTION_SECONDS = 10 * 60,
	/* the time played to vote, at most (and no more than to start one) */
	VOTER_MINIMUM_SECONDS = 2 * 60,
	/* a machine's requests the host takes, at most one this often */
	REQUEST_INTERVAL_MILLISECONDS = 1000,
	MINIMUM_VOTES = 2,
	/* a client forgets the host's count it has not heard again this long */
	STATUS_TIMEOUT_MILLISECONDS = 3000,
};

/* the status's flags */
enum
{
	/* this machine (the one told) has voted */
	_votekick_status_voted_bit,
	/* ... may vote */
	_votekick_status_may_vote_bit,
};

/* ---------- structures */

typedef char distributed_votekick_request_size_assert[sizeof(struct distributed_votekick_request) == 4 ? 1 : -1];
typedef char distributed_votekick_status_size_assert[sizeof(struct distributed_votekick_status) == 8 ? 1 : -1];

/* a machine as the count sees it (votekick_count) */
struct votekick_voter
{
	/* its real address (host byte order; 0: not known) */
	unsigned long address;
	char hardware_id[VOTEKICK_HARDWARE_ID_SIZE];
	/* the host's own machine */
	boolean host;
	/* the machine of the player voted against */
	boolean target;
	/* has played long enough to vote (the host's, always) */
	boolean eligible;
	boolean voted;
};

/* ---------- globals */

static struct votekick_state votekick_state;
static struct votekick_texts votekick_texts;
static struct votekick_inputs votekick_inputs;
/* port: the steps' arguments (halo_sim.h) */
static struct halo_sim_network_votekick votekick_arguments;

#define votekick (votekick_state.vote)

/* ---------- the count (pure: tools/harness/tests/votekick_count.c) */

/* whether two machines are one person's by address: the host's own is
only itself; an address not known, no other's */
static boolean votekick_same_address(
	struct votekick_voter const *a,
	struct votekick_voter const *b)
{
	if (a->host || b->host)
		return a->host && b->host;
	return a->address != 0 && a->address == b->address;
}

/* ... or by hardware id too (none told: no other's) */
static boolean votekick_same_person(
	struct votekick_voter const *a,
	struct votekick_voter const *b)
{
	if (votekick_same_address(a, b))
		return TRUE;
	return !a->host && !b->host && a->hardware_id[0] && !csstrcmp(a->hardware_id, b->hardware_id);
}

/* whether a machine is one of those who may vote: played long enough, and
its address known (else it could not be told from another) */
static boolean votekick_may_vote(
	struct votekick_voter const *voter)
{
	return voter->eligible && !voter->target && (voter->host || voter->address != 0);
}

/* the players who may vote (by address: the target's among them, once) and
the votes for (by address and hardware id, none of the target's person) */
static void votekick_count(
	struct votekick_voter const *voters,
	short count,
	short *votes,
	short *electorate)
{
	short index;
	short other;

	*votes = 0;
	*electorate = 0;
	for (index = 0; index < count; index++)
	{
		boolean counted = votekick_may_vote(&voters[index]) || voters[index].target;

		/* (an address counted once: the first of its machines) */
		for (other = 0; counted && other < index; other++)
		{
			if ((votekick_may_vote(&voters[other]) || voters[other].target) &&
				votekick_same_address(&voters[index], &voters[other]))
			{
				counted = FALSE;
			}
		}
		if (counted)
			(*electorate)++;
	}
	for (index = 0; index < count; index++)
	{
		boolean counted = voters[index].voted && votekick_may_vote(&voters[index]);

		for (other = 0; counted && other < count; other++)
		{
			/* (not the target's person, whatever it says; a person once) */
			if (voters[other].target && votekick_same_person(&voters[index], &voters[other]))
				counted = FALSE;
			if (other < index && voters[other].voted && votekick_may_vote(&voters[other]) &&
				votekick_same_person(&voters[index], &voters[other]))
			{
				counted = FALSE;
			}
		}
		if (counted)
			(*votes)++;
	}
}

/* the votes that kick: more than half of those who may vote, and at least
MINIMUM_VOTES */
static short votekick_votes_needed(
	short electorate)
{
	short needed = (short)(electorate / 2 + 1);

	return needed < MINIMUM_VOTES ? MINIMUM_VOTES : needed;
}

/* whether a time (system_milliseconds) has come; times wrap, so by their
difference */
static boolean votekick_time_reached(
	unsigned long now,
	unsigned long time)
{
	return (long)(now - time) >= 0;
}

/* ---------- the steps' calls */

/* port: through the world's calls (the Rust port's) */
static void votekick_call(
	long kind,
	long a,
	long b)
{
	struct halo_sim_ai_call call;

	memset(&call, 0, sizeof(call));
	call.kind = kind;
	call.actor = NONE;
	call.a = a;
	call.b = b;
	network_votekick_call(&call);
}

/* the name of a machine's players for a notice (NONE: the host's own), into
the texts' names (or the vote's target's) */
static void votekick_names(
	long machine_index,
	boolean target)
{
	votekick_call(_network_votekick_call_names, machine_index, target);
}

/* the notice in the texts, to everyone */
static void votekick_notice(
	void)
{
	votekick_call(_network_votekick_call_notice, 0, 0);
}

/* a refusal, to the asking machine alone */
static void votekick_refuse(
	long machine_index,
	char const *text)
{
	snprintf(votekick_texts.notice, sizeof(votekick_texts.notice), "%s", text);
	votekick_call(_network_votekick_call_refuse, machine_index, 0);
}

/* ---------- remembered identities */

static void votekick_remember(
	struct votekick_identity *identities,
	unsigned long address,
	char const *hardware_id,
	unsigned long milliseconds)
{
	unsigned long now = votekick_inputs.now;
	short index;
	short oldest = 0;

	/* (a free entry, one gone by, else the one that ends soonest) */
	for (index = 0; index < MAXIMUM_VOTEKICK_REMEMBERED; index++)
	{
		if (!identities[index].valid || votekick_time_reached(now, identities[index].until))
		{
			oldest = index;
			break;
		}
		if ((long)(identities[index].until - identities[oldest].until) < 0)
			oldest = index;
	}
	identities[oldest].valid = TRUE;
	identities[oldest].address = address;
	p2p_hardware_id_sanitize(identities[oldest].hardware_id, VOTEKICK_HARDWARE_ID_SIZE, hardware_id);
	identities[oldest].until = now + milliseconds;
}

/* whether an address or hardware id is remembered (not gone by); the
milliseconds left */
static boolean votekick_remembered(
	struct votekick_identity const *identities,
	unsigned long address,
	char const *hardware_id,
	unsigned long *milliseconds_left)
{
	unsigned long now = votekick_inputs.now;
	short index;

	for (index = 0; index < MAXIMUM_VOTEKICK_REMEMBERED; index++)
	{
		struct votekick_identity const *identity = &identities[index];

		if (!identity->valid || votekick_time_reached(now, identity->until))
			continue;
		if ((address && identity->address == address) ||
			(hardware_id && hardware_id[0] && identity->hardware_id[0] &&
				!csstrcmp(hardware_id, identity->hardware_id)))
		{
			if (milliseconds_left)
				*milliseconds_left = identity->until - now;
			return TRUE;
		}
	}
	return FALSE;
}

/* ---------- the host */

static boolean votekick_host(
	void)
{
	return game_connection() == _game_connection_network_server;
}

/* the time played to start a vote, and to vote (in ticks) */
static long votekick_starter_ticks(
	void)
{
	long minutes = votekick_inputs.minutes;

	return PIN(minutes, 0, 60) * 60 * TICKS_PER_SECOND;
}

static long votekick_voter_ticks(
	void)
{
	return MIN(votekick_starter_ticks(), VOTER_MINIMUM_SECONDS * TICKS_PER_SECOND);
}

static boolean votekick_machine_valid(
	long machine_index)
{
	return machine_index >= 0 && machine_index < MAXIMUM_VOTEKICK_MACHINES;
}

/* a machine (NONE: the host's own) as the count sees it */
static void votekick_voter_get(
	long machine_index,
	struct votekick_voter *voter)
{
	csmemset(voter, 0, sizeof(*voter));
	if (machine_index == NONE)
	{
		voter->host = TRUE;
		voter->eligible = TRUE;
		voter->voted = votekick.active && votekick.host_voted;
		return;
	}
	voter->address = votekick_inputs.addresses[machine_index];
	csstrcpy(voter->hardware_id, votekick_inputs.hardware_ids[machine_index]);
	voter->eligible = votekick_state.played_ticks[machine_index] >= votekick_voter_ticks();
	voter->target = votekick.active && machine_index == votekick.target_machine;
	voter->voted = votekick.active && votekick.voted[machine_index] &&
		votekick.voted_generations[machine_index] == votekick_state.generations[machine_index];
}

/* the target's machine still the one voted against, in the game */
static boolean votekick_target_present(
	void)
{
	short index;

	if (votekick_state.generations[votekick.target_machine] != votekick.target_generation)
		return FALSE;
	for (index = 0; index < votekick_inputs.machine_count; index++)
	{
		if (votekick_inputs.machine_indices[index] == votekick.target_machine)
			return TRUE;
	}
	return FALSE;
}

/* the count now, of the machines in the game (the target's among them:
votekick_target_present) */
static void votekick_recount(
	void)
{
	struct votekick_voter voters[MAXIMUM_VOTEKICK_MACHINES + 1];
	short count = votekick_inputs.machine_count;
	short electorate;
	short index;

	for (index = 0; index < count; index++)
		votekick_voter_get(votekick_inputs.machine_indices[index], &voters[index]);
	votekick_count(voters, count, &votekick.votes, &electorate);
	votekick.needed = votekick_votes_needed(electorate);
}

/* the count to every client: what it may do, and has */
static void votekick_send_status(
	void)
{
	unsigned long now = votekick_inputs.now;
	short index;

	for (index = 0; index < votekick_inputs.client_count; index++)
	{
		long machine_index = votekick_inputs.client_indices[index];
		struct distributed_votekick_status *status = &votekick_texts.message.status;
		struct votekick_voter voter;

		csmemset(&votekick_texts.message, 0, sizeof(votekick_texts.message));
		status->player_index = NO_PLAYER;
		if (votekick.active)
		{
			unsigned long elapsed = now - votekick.started_at;
			long left = VOTE_SECONDS - (long)(elapsed / 1000);

			votekick_voter_get(machine_index, &voter);
			status->player_index = distributed_player_to_byte(votekick.target_player);
			status->votes = (byte)PIN(votekick.votes, 0, 255);
			status->needed = (byte)PIN(votekick.needed, 0, 255);
			status->seconds_left = (byte)PIN(left, 0, 255);
			if (voter.voted)
				status->flags |= FLAG(_votekick_status_voted_bit);
			if (votekick_may_vote(&voter))
				status->flags |= FLAG(_votekick_status_may_vote_bit);
		}
		votekick_call(_network_votekick_call_send_status, machine_index, 0);
	}
}

/* the vote over (passed or not: its notice said), and none for a while */
static void votekick_end(
	void)
{
	votekick.active = FALSE;
	votekick_state.next_allowed = votekick_inputs.now + VOTE_GAP_SECONDS * 1000;
	votekick_state.gap = TRUE;
	votekick_send_status();
}

/* the vote passed: the player kicked and kept out; or (left) the player
left during it, which passes it too (no one dodges a vote by leaving), and
is kept out as well */
static void votekick_pass(
	boolean left)
{
	char *notice = votekick_texts.notice;
	long ban_minutes = PIN(votekick_inputs.ban_minutes, 1, 24 * 60);

	if (left)
	{
		snprintf(notice, VOTEKICK_NOTICE_SIZE, "%s left during the vote to kick them, kept out for %ld minute%s",
			votekick.target_names, ban_minutes, ban_minutes == 1 ? "" : "s");
	}
	else
	{
		snprintf(notice, VOTEKICK_NOTICE_SIZE, "%s kicked by vote (%d of %d), kept out for %ld minute%s",
			votekick.target_names, votekick.votes, votekick.needed, ban_minutes, ban_minutes == 1 ? "" : "s");
	}
	votekick_notice();
	votekick_remember(votekick_state.kept_out, votekick.target_address, votekick.target_hardware_id,
		(unsigned long)ban_minutes * 60 * 1000);
	/* (one that left is gone: its slot may be another's now) */
	if (!left)
		votekick_call(_network_votekick_call_kick, votekick.target_machine, 0);
	votekick_end();
}

static void votekick_fail(
	void)
{
	snprintf(votekick_texts.notice, VOTEKICK_NOTICE_SIZE, "The vote to kick %s failed (%d of %d)",
		votekick.target_names, votekick.votes, votekick.needed);
	votekick_notice();
	votekick_remember(votekick_state.cooling_starters, votekick.starter_address, votekick.starter_hardware_id,
		STARTER_COOLDOWN_SECONDS * 1000UL);
	votekick_remember(votekick_state.protected_targets, votekick.target_address, votekick.target_hardware_id,
		TARGET_PROTECTION_SECONDS * 1000UL);
	votekick_end();
}

/* a machine's vote against a player (NONE: the host's own machine): a new
vote, or one for the vote running */
static void votekick_host_request(
	long machine_index,
	short player_index)
{
	long target_machine = votekick_inputs.player_valid ? votekick_inputs.player_machine : NONE;
	struct votekick_voter voter;
	struct votekick_voter target;
	char *notice = votekick_texts.notice;
	unsigned long now = votekick_inputs.now;
	unsigned long left;

	if (!votekick_inputs.enabled)
	{
		votekick_refuse(machine_index, "votekick: the host has turned votes off");
		return;
	}
	if (machine_index == NONE && !votekick_inputs.host_has_player)
		return;
	if (!votekick_inputs.player_valid || votekick_inputs.player_quit)
	{
		votekick_refuse(machine_index, "votekick: no such player");
		return;
	}
	if (target_machine == NONE)
	{
		votekick_refuse(machine_index, "votekick: the host's players cannot be voted out");
		return;
	}
	if (!votekick_machine_valid(target_machine) || target_machine == machine_index)
	{
		votekick_refuse(machine_index, "votekick: not against your own players");
		return;
	}
	votekick_voter_get(machine_index, &voter);
	votekick_voter_get(target_machine, &target);
	target.target = TRUE;
	if (votekick_same_person(&voter, &target))
	{
		votekick_refuse(machine_index, "votekick: not against your own machine");
		return;
	}
	if (!voter.host && !voter.address)
	{
		votekick_refuse(machine_index, "votekick: the host does not know your address");
		return;
	}
	/* a vote for the vote running */
	if (votekick.active)
	{
		if (target_machine != votekick.target_machine)
		{
			snprintf(notice, VOTEKICK_NOTICE_SIZE, "votekick: wait for the vote to kick %s to end",
				votekick.target_names);
			votekick_call(_network_votekick_call_refuse, machine_index, 0);
			return;
		}
		if (!votekick_may_vote(&voter))
		{
			votekick_refuse(machine_index, "votekick: you have not played long enough on this server to vote");
			return;
		}
		if (voter.voted)
			return;
		if (machine_index == NONE)
			votekick.host_voted = TRUE;
		else
		{
			votekick.voted[machine_index] = TRUE;
			votekick.voted_generations[machine_index] = votekick_state.generations[machine_index];
		}
		{
			short votes = votekick.votes;

			votekick_recount();
			/* (told only as the count changes: a second machine of one
			person's adds nothing) */
			if (votekick.votes != votes && votekick.votes < votekick.needed)
			{
				votekick_names(machine_index, FALSE);
				snprintf(notice, VOTEKICK_NOTICE_SIZE, "%s voted to kick %s (%d of %d)", votekick_texts.names,
					votekick.target_names, votekick.votes, votekick.needed);
				votekick_notice();
			}
		}
		if (votekick.votes >= votekick.needed)
			votekick_pass(FALSE);
		else
			votekick_send_status();
		return;
	}
	/* a new vote */
	if (votekick_state.gap && !votekick_time_reached(now, votekick_state.next_allowed))
	{
		snprintf(notice, VOTEKICK_NOTICE_SIZE, "votekick: the next vote can start in %lu seconds",
			(votekick_state.next_allowed - now + 999) / 1000);
		votekick_call(_network_votekick_call_refuse, machine_index, 0);
		return;
	}
	if (!voter.host && votekick_state.played_ticks[machine_index] < votekick_starter_ticks())
	{
		long minutes = votekick_starter_ticks() / (60 * TICKS_PER_SECOND);

		snprintf(notice, VOTEKICK_NOTICE_SIZE,
			"votekick: play %ld minute%s on this server to start a vote (%ld played)", minutes,
			minutes == 1 ? "" : "s", votekick_state.played_ticks[machine_index] / (60 * TICKS_PER_SECOND));
		votekick_call(_network_votekick_call_refuse, machine_index, 0);
		return;
	}
	if (!voter.host && votekick_remembered(votekick_state.cooling_starters, voter.address, voter.hardware_id, &left))
	{
		snprintf(notice, VOTEKICK_NOTICE_SIZE, "votekick: your last vote failed: wait %lu seconds", (left + 999) / 1000);
		votekick_call(_network_votekick_call_refuse, machine_index, 0);
		return;
	}
	if (votekick_remembered(votekick_state.protected_targets, target.address, target.hardware_id, &left))
	{
		snprintf(notice, VOTEKICK_NOTICE_SIZE,
			"votekick: a vote against that player failed lately: wait %lu seconds", (left + 999) / 1000);
		votekick_call(_network_votekick_call_refuse, machine_index, 0);
		return;
	}
	csmemset(&votekick, 0, sizeof(votekick));
	votekick.active = TRUE;
	votekick.target_machine = target_machine;
	votekick.target_generation = votekick_state.generations[target_machine];
	votekick.target_player = player_index;
	votekick.target_address = target.address;
	csstrcpy(votekick.target_hardware_id, target.hardware_id);
	votekick_names(target_machine, TRUE);
	votekick.starter_address = voter.address;
	csstrcpy(votekick.starter_hardware_id, voter.hardware_id);
	votekick.started_at = now;
	if (machine_index == NONE)
		votekick.host_voted = TRUE;
	else
	{
		votekick.voted[machine_index] = TRUE;
		votekick.voted_generations[machine_index] = votekick_state.generations[machine_index];
	}
	votekick_recount();
	votekick_names(machine_index, FALSE);
	snprintf(notice, VOTEKICK_NOTICE_SIZE,
		"%s started a vote to kick %s (%d of %d): open the scores and pick the name to vote",
		votekick_texts.names, votekick.target_names, votekick.votes, votekick.needed);
	votekick_notice();
	votekick_call(_network_votekick_call_log_started, machine_index, target_machine);
	if (votekick.votes >= votekick.needed)
		votekick_pass(FALSE);
	else
		votekick_send_status();
}

/* a request: a client's message, or the host's own machine's */
static void votekick_request_game(
	struct halo_sim_network_votekick const *arguments)
{
	long machine_index = arguments->machine_index;
	struct distributed_votekick_request request;
	unsigned long now = votekick_inputs.now;

	if (arguments->kind == _network_votekick_request_own)
	{
		votekick_host_request(NONE, (short)arguments->player_index);
		return;
	}
	if (!votekick_machine_valid(machine_index) || !arguments->from_stream)
		return;
	/* (at most one a second: more are dropped, untold) */
	if (votekick_state.requested[machine_index] &&
		!votekick_time_reached(now, votekick_state.request_times[machine_index] + REQUEST_INTERVAL_MILLISECONDS))
	{
		return;
	}
	votekick_state.requested[machine_index] = TRUE;
	votekick_state.request_times[machine_index] = now;
	csmemcpy(&request, (void const *)arguments->entries, sizeof(request));
	if (request.player_index >= MAXIMUM_TRACKED_PLAYERS)
		return;
	votekick_host_request(machine_index, request.player_index);
}

static void votekick_host_tick_game(
	void)
{
	short index;

	for (index = 0; index < votekick_inputs.client_count; index++)
	{
		if (votekick_machine_valid(votekick_inputs.client_indices[index]))
			votekick_state.played_ticks[votekick_inputs.client_indices[index]]++;
	}
	if (!votekick.active)
		return;
	/* (the player left: the vote passes) */
	if (!votekick_target_present())
	{
		votekick_pass(TRUE);
		return;
	}
	/* (those gone counted no more, and those who have since played long
	enough to vote among those who may) */
	if (votekick_inputs.game_time % TICKS_PER_SECOND == 0)
	{
		votekick_recount();
		if (votekick.votes >= votekick.needed)
		{
			votekick_pass(FALSE);
			return;
		}
		if (votekick_time_reached(votekick_inputs.now, votekick.started_at + VOTE_SECONDS * 1000))
		{
			votekick_fail();
			return;
		}
		votekick_send_status();
	}
}

static void votekick_status_game(
	struct halo_sim_network_votekick const *arguments)
{
	csmemcpy(&votekick_state.status, (void const *)arguments->entries, sizeof(votekick_state.status));
	votekick_state.status_heard = votekick_inputs.now;
	if (votekick_state.status.player_index != NO_PLAYER && !votekick_inputs.player_valid)
		votekick_state.status.player_index = NO_PLAYER;
	/* (0: none heard) */
	if (!votekick_state.status_heard)
		votekick_state.status_heard = 1;
}

static void votekick_machine_game(
	struct halo_sim_network_votekick *arguments)
{
	long machine_index = arguments->machine_index;
	unsigned long real_address = votekick_inputs.joining_address;
	char const *hardware_id = votekick_inputs.joining_hardware_id;

	switch (arguments->kind)
	{
	case _network_votekick_machine_joined:
		if (!votekick_machine_valid(machine_index))
			return;
		votekick_state.played_ticks[machine_index] = 0;
		votekick_state.generations[machine_index]++;
		votekick_state.requested[machine_index] = FALSE;
		votekick.voted[machine_index] = FALSE;
		break;
	case _network_votekick_machine_kept_out:
		/* (nor the player a vote runs against, joining again during it: it
		would drop the machine voted against for a new one, out of the vote) */
		if (votekick.active && arguments->host &&
			((real_address && real_address == votekick.target_address) ||
				(hardware_id[0] && votekick.target_hardware_id[0] &&
					!csstrcmp(hardware_id, votekick.target_hardware_id))))
		{
			arguments->result = TRUE;
			return;
		}
		arguments->result = votekick_remembered(votekick_state.kept_out, real_address, hardware_id, NULL);
		break;
	case _network_votekick_new_game:
		/* (a vote ends with its game, failing nobody; the time played stays) */
		if (votekick.active && arguments->host)
		{
			snprintf(votekick_texts.notice, VOTEKICK_NOTICE_SIZE, "%s", "The vote ended with the game");
			votekick_notice();
		}
		votekick.active = FALSE;
		csmemset(&votekick_state.status, 0, sizeof(votekick_state.status));
		votekick_state.status_heard = 0;
		break;
	}
}

/* port: what the steps read of the game, gathered before them; the player
(absolute index; NONE for none) a request or the count names */
static void votekick_gather(
	short player_index)
{
	struct votekick_inputs *inputs = &votekick_inputs;
	struct data_iterator iterator;
	struct player_datum *player;
	short index;

	csmemset(inputs, 0, sizeof(*inputs));
	inputs->now = system_milliseconds();
	inputs->game_time = game_time_get();
	inputs->enabled = config_boolean("network.votekick") != FALSE;
	inputs->minutes = config_integer("network.votekick_minutes");
	inputs->ban_minutes = config_integer("network.votekick_ban_minutes");
	inputs->host_has_player = local_player_get_next(NONE) != NONE;
	/* (votekick_machines: the client machines in the game with a player who
	has not quit, and the host's own if it has one) */
	if (player_data)
	{
		data_iterator_new(&iterator, player_data);
		while ((player = (struct player_datum *)data_iterator_next(&iterator)) != NULL)
		{
			long machine_index = distributed_player_machine((short)DATUM_INDEX_TO_ABSOLUTE_INDEX(iterator.datum_index));

			if (player->quit_out_of_game || (machine_index != NONE && !votekick_machine_valid(machine_index)))
				continue;
			for (index = 0; index < inputs->machine_count && inputs->machine_indices[index] != machine_index; index++)
				;
			if (index == inputs->machine_count && inputs->machine_count < (short)NUMBEROF(inputs->machine_indices))
				inputs->machine_indices[inputs->machine_count++] = machine_index;
		}
	}
	inputs->client_count = distributed_client_machines(inputs->client_indices, MAXIMUM_VOTEKICK_MACHINES);
	for (index = 0; index < MAXIMUM_VOTEKICK_MACHINES; index++)
	{
		inputs->addresses[index] = distributed_machine_real_address(index);
		p2p_hardware_id_sanitize(inputs->hardware_ids[index], VOTEKICK_HARDWARE_ID_SIZE,
			network_game_server_machine_hardware_id(index));
	}
	player = player_index != NONE && player_data ? distributed_player(player_index) : NULL;
	inputs->player_valid = player != NULL;
	inputs->player_quit = player && player->quit_out_of_game;
	inputs->player_machine = player ? distributed_player_machine(player_index) : NONE;
}

/* port: a step of the vote: Rust's in Rust's mode (crates/halo_map's
network_votekick), checked against the game's otherwise. Its world: the
arguments, what it reads of the game, the vote's records and the texts. */
static void votekick_step(
	long step)
{
	struct halo_sim_network_votekick *arguments = &votekick_arguments;
	struct halo_sim_world_context context;
	struct halo_sim_world_region regions[4];
	int checking = FALSE;

	arguments->state = (unsigned long)&votekick_state;
	arguments->texts = (unsigned long)&votekick_texts;
	arguments->inputs = (unsigned long)&votekick_inputs;
	arguments->host = votekick_host();
	regions[0].address = (unsigned long)arguments;
	regions[0].size = sizeof(*arguments);
	regions[1].address = (unsigned long)&votekick_inputs;
	regions[1].size = sizeof(votekick_inputs);
	regions[2].address = (unsigned long)&votekick_state;
	regions[2].size = sizeof(votekick_state);
	regions[3].address = (unsigned long)&votekick_texts;
	regions[3].size = sizeof(votekick_texts);
	memset(&context, 0, sizeof(context));
	context.game_time = game_time_get();
	context.player_data = (unsigned long)player_data;
	context.arguments = (unsigned long)arguments;
	if (halo_sim_world_step_begin(step, &context, regions, 4, network_votekick_call, &checking))
		return;
	switch (step)
	{
	case _halo_sim_world_step_network_votekick_request: votekick_request_game(arguments); break;
	case _halo_sim_world_step_network_votekick_host_tick: votekick_host_tick_game(); break;
	case _halo_sim_world_step_network_votekick_status: votekick_status_game(arguments); break;
	case _halo_sim_world_step_network_votekick_machine: votekick_machine_game(arguments); break;
	}
	if (checking)
		halo_sim_world_step_check(step);
}

/* ---------- public code */

/* port: the steps' calls (halo_sim.h's _network_votekick_call_*) */
void network_votekick_call(
	struct halo_sim_ai_call *call)
{
	int opened = halo_sim_world_event_begin(call);

	switch (call->kind)
	{
	case _network_votekick_call_names:
		if (call->b)
			distributed_machine_player_names(call->a, votekick.target_names, sizeof(votekick.target_names));
		else
			distributed_machine_player_names(call->a, votekick_texts.names, sizeof(votekick_texts.names));
		break;
	case _network_votekick_call_notice:
		distributed_send_notice(votekick_texts.notice);
		break;
	case _network_votekick_call_refuse:
		distributed_send_notice_to_machine(call->a, votekick_texts.notice);
		break;
	case _network_votekick_call_kick:
		network_game_server_kick_machine(call->a, FALSE);
		break;
	case _network_votekick_call_send_status:
		distributed_send_to_machine(call->a, &votekick_texts.message, _distributed_message_votekick_status, 1,
			(word)sizeof(votekick_texts.message));
		break;
	case _network_votekick_call_log_started:
		error(_error_log, "votekick: started by machine %ld against machine %ld", call->a, call->b);
		break;
	}
	halo_sim_world_event_end(call, opened);
}

/* (network_server_manager.c) a machine joined the host at the slot: its
time played starts over, and no vote of the machine before is its */
void network_votekick_machine_joined(
	long machine_index)
{
	votekick_gather(NONE);
	memset(&votekick_arguments, 0, sizeof(votekick_arguments));
	votekick_arguments.kind = _network_votekick_machine_joined;
	votekick_arguments.machine_index = machine_index;
	votekick_step(_halo_sim_world_step_network_votekick_machine);
}

/* (network_server_manager.c) whether a machine of the address (as it
joins from: host byte order) and hardware id is kept out, kicked by a vote */
boolean network_votekick_kept_out(
	unsigned long address,
	char const *hardware_id)
{
	votekick_gather(NONE);
	votekick_inputs.joining_address = distributed_real_address(address);
	if (hardware_id)
		snprintf(votekick_inputs.joining_hardware_id, sizeof(votekick_inputs.joining_hardware_id), "%s", hardware_id);
	memset(&votekick_arguments, 0, sizeof(votekick_arguments));
	votekick_arguments.kind = _network_votekick_machine_kept_out;
	votekick_step(_halo_sim_world_step_network_votekick_machine);
	return (boolean)votekick_arguments.result;
}

void network_votekick_handle_request(
	long machine_index,
	void const *entries,
	boolean from_stream)
{
	struct distributed_votekick_request const *request = entries;

	if (!votekick_host())
		return;
	votekick_gather(request->player_index < MAXIMUM_TRACKED_PLAYERS ? request->player_index : NONE);
	memset(&votekick_arguments, 0, sizeof(votekick_arguments));
	votekick_arguments.kind = _network_votekick_request_message;
	votekick_arguments.machine_index = machine_index;
	votekick_arguments.from_stream = from_stream;
	votekick_arguments.entries = (unsigned long)entries;
	votekick_step(_halo_sim_world_step_network_votekick_request);
}

word network_votekick_request_entry_size(
	void)
{
	return sizeof(struct distributed_votekick_request);
}

void network_votekick_new_game(
	void)
{
	votekick_gather(NONE);
	memset(&votekick_arguments, 0, sizeof(votekick_arguments));
	votekick_arguments.kind = _network_votekick_new_game;
	votekick_step(_halo_sim_world_step_network_votekick_machine);
}

void network_votekick_host_tick(
	void)
{
	votekick_gather(NONE);
	memset(&votekick_arguments, 0, sizeof(votekick_arguments));
	votekick_step(_halo_sim_world_step_network_votekick_host_tick);
}

/* ---------- a client */

void network_votekick_handle_status(
	void const *entries)
{
	struct distributed_votekick_status const *status = entries;

	if (game_connection() != _game_connection_network_client)
		return;
	votekick_gather(status->player_index != NO_PLAYER ? status->player_index : NONE);
	memset(&votekick_arguments, 0, sizeof(votekick_arguments));
	votekick_arguments.entries = (unsigned long)entries;
	votekick_step(_halo_sim_world_step_network_votekick_status);
}

word network_votekick_status_entry_size(
	void)
{
	return sizeof(struct distributed_votekick_status);
}

/* ---------- every machine (network_votekick.h) */

boolean network_votekick_get_status(
	struct network_votekick_status *status)
{
	struct distributed_votekick_status const *heard = &votekick_state.status;

	csmemset(status, 0, sizeof(*status));
	status->player_index = NONE;
	if (votekick_host())
	{
		unsigned long elapsed;

		if (!votekick.active)
			return FALSE;
		elapsed = system_milliseconds() - votekick.started_at;
		status->player_index = votekick.target_player;
		status->votes = votekick.votes;
		status->needed = votekick.needed;
		status->seconds_left = (short)MAX(0, VOTE_SECONDS - (long)(elapsed / 1000));
		status->voted = votekick.host_voted;
		status->may_vote = TRUE;
		return TRUE;
	}
	if (game_connection() != _game_connection_network_client || !votekick_state.status_heard ||
		votekick_time_reached(system_milliseconds(), votekick_state.status_heard + STATUS_TIMEOUT_MILLISECONDS) ||
		heard->player_index == NO_PLAYER)
	{
		return FALSE;
	}
	status->player_index = distributed_player_from_byte(heard->player_index) == NONE ? NONE : heard->player_index;
	status->votes = heard->votes;
	status->needed = heard->needed;
	status->seconds_left = heard->seconds_left;
	status->voted = TEST_FLAG(heard->flags, _votekick_status_voted_bit);
	status->may_vote = TEST_FLAG(heard->flags, _votekick_status_may_vote_bit);
	return status->player_index != NONE;
}

/* a player's name in ASCII (player_name_character_ascii), at most size */
static void votekick_player_name(
	struct player_datum const *player,
	char *text,
	long size)
{
	long index;

	for (index = 0; index < (long)NUMBEROF(player->name) && player->name[index] && index < size - 1; index++)
		text[index] = player_name_character_ascii(player->name[index]);
	text[index] = 0;
}

/* whether a name begins with the text, in either case; and is it */
static boolean votekick_name_begins_with(
	char const *name,
	char const *text,
	boolean whole)
{
	for (; *text; name++, text++)
	{
		char a = *name >= 'A' && *name <= 'Z' ? *name - 'A' + 'a' : *name;
		char b = *text >= 'A' && *text <= 'Z' ? *text - 'A' + 'a' : *text;

		if (!*name || a != b)
			return FALSE;
	}
	return !whole || !*name;
}

short network_votekick_matching_player_names(
	char const *text,
	char (*names)[VOTEKICK_NAME_TEXT_SIZE],
	short maximum_count)
{
	struct data_iterator iterator;
	struct player_datum *player;
	short count = 0;

	if (!network_votekick_available())
		return 0;
	data_iterator_new(&iterator, player_data);
	while ((player = (struct player_datum *)data_iterator_next(&iterator)) != NULL && count < maximum_count)
	{
		if (player->quit_out_of_game || player->local_player_index != NONE)
			continue;
		votekick_player_name(player, names[count], VOTEKICK_NAME_TEXT_SIZE);
		if (votekick_name_begins_with(names[count], text, FALSE))
			count++;
	}
	return count;
}

boolean network_votekick_player_named(
	char const *text)
{
	struct data_iterator iterator;
	struct player_datum *player;
	long found = NONE;
	short matches = 0;
	short exact = 0;
	long exact_found = NONE;

	if (!network_votekick_available())
	{
		console_warning("votekick: only in a network game");
		return FALSE;
	}
	if (!text[0])
	{
		console_warning("votekick: give a player's name (Tab completes it)");
		return FALSE;
	}
	data_iterator_new(&iterator, player_data);
	while ((player = (struct player_datum *)data_iterator_next(&iterator)) != NULL)
	{
		char name[VOTEKICK_NAME_TEXT_SIZE];

		if (player->quit_out_of_game)
			continue;
		votekick_player_name(player, name, sizeof(name));
		if (votekick_name_begins_with(name, text, TRUE))
		{
			exact++;
			exact_found = iterator.datum_index;
		}
		if (votekick_name_begins_with(name, text, FALSE))
		{
			matches++;
			found = iterator.datum_index;
		}
	}
	if (exact == 1)
	{
		found = exact_found;
		matches = 1;
	}
	if (matches > 1)
	{
		console_warning("votekick: %d players' names begin with \"%s\": give more of it", matches, text);
		return FALSE;
	}
	if (matches == 0)
	{
		console_warning("votekick: no player's name begins with \"%s\"", text);
		return FALSE;
	}
	return network_votekick_request((short)DATUM_INDEX_TO_ABSOLUTE_INDEX(found));
}

/* (the host) its Kick and Ban on the scoreboard: the player's machine */
boolean network_votekick_host_kick(
	short player_index,
	boolean ban)
{
	long machine_index;

	if (!votekick_host() || !distributed_player(player_index))
		return FALSE;
	machine_index = distributed_player_machine(player_index);
	if (machine_index == NONE)
	{
		console_warning("%s: not a player of the host's own machine", ban ? "ban" : "kick");
		return FALSE;
	}
	return network_game_server_kick_machine_of_player(machine_index, ban);
}

boolean network_votekick_host(
	void)
{
	return votekick_host();
}

boolean network_votekick_available(
	void)
{
	return game_connection() == _game_connection_network_client ||
		game_connection() == _game_connection_network_server;
}

boolean network_votekick_request(
	short player_index)
{
	struct
	{
		struct distributed_message_header header;
		struct distributed_votekick_request request;
	} message;

	if (!distributed_player(player_index))
		return FALSE;
	if (distributed_player_is_local(player_index))
	{
		console_warning("votekick: not against your own players");
		return FALSE;
	}
	if (votekick_host())
	{
		votekick_gather(player_index);
		memset(&votekick_arguments, 0, sizeof(votekick_arguments));
		votekick_arguments.kind = _network_votekick_request_own;
		votekick_arguments.player_index = player_index;
		votekick_step(_halo_sim_world_step_network_votekick_request);
		return TRUE;
	}
	if (game_connection() != _game_connection_network_client)
	{
		console_warning("votekick: only in a network game");
		return FALSE;
	}
	csmemset(&message, 0, sizeof(message));
	message.request.player_index = distributed_player_to_byte(player_index);
	distributed_send(&message, _distributed_message_votekick, 1, (word)sizeof(message), _distributed_to_host_reliably);
	return TRUE;
}
