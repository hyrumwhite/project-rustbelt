/*
NETWORK_VOICE.C

Voice chat (port/linux/NETCODE.md): each machine's microphone, encoded with
Opus (port/linux/src/voice_audio.c), goes to the host, which sends it on to
the machines that may hear it. The host decides everything, from its own
config.toml:

- In the lobby, before and after a game (and in a game's postgame), every
  player hears every other (network.voice_lobby).
- In a game, network.voice_mode: off; teammates near; anyone near; all
  teammates; or all teammates and enemies near ("near": within
  network.voice_proximity world units of each other, both alive). A game
  without teams has only enemies, and co-op only teammates. A voice heard
  for being near is quieter further away, and comes from the speaker's side
  (each listener's own reckoning of where everyone is).
- The quality: network.voice_kbps (8 to 64), in the lobby and in a game,
  which the host tells each machine to encode at, and holds it to.

What the host takes is held to that too: a machine's frames only with the
key the host gave it over its stream (a datagram's source address is all
that names its machine, and anyone can send one as from another's), no
larger than the quality allows, no more than VOICE_FRAMES_PER_SECOND (with
a little burst), only while voice is on, and no more than
VOICE_MAXIMUM_TALKERS machines at once. A machine is sent voices once it
has said it hears them (a frame, or an empty one every two seconds), so a
build without voice chat is never sent any. The host relays only: it never
decodes, and a machine's own voice never comes back to it.

Each machine decides only for itself: whether it talks (audio.voice_chat:
push to talk, open mic, or not), how loud the others are
(audio.voice_volume), and whom it mutes (the scoreboard's menu,
game_engine.c), which it does not tell anyone.

port: voice chat's decisions run in Rust in Rust's mode (crates/halo_map's
network_voice), checked against the game's otherwise: each frame's update
(the host's settings told, a client's hello, the microphone, this machine's
frames sent or, the host's, relayed), the messages (a client's frame to the
host, relayed; a frame relayed to a client, played; the host's settings
heard) and a machine joining the host (its key). Their records are in
network_voice_records.h; what they read of the game is gathered before them
(struct voice_inputs); sending, the microphone, encoding, playing and a new
key go through network_voice_call. Opus and the sound (voice_audio.c) are
the platform's; what the menus and the scoreboard ask (who is speaking, the
mutes, the icon) stays the game's.
*/

#include "cseries.h"
#include "cseries/errors.h"
#include "memory/data.h"
#include "math/integer_math.h"
#include "bitmaps/bitmap_group.h"
#include "bitmaps/bitmap_group_lookup.h"
#include "interface/ui_widget.h"
#include "tag_files/tag_groups.h"
#include "camera/observer.h"
#include "game/game.h"
#include "game/game_engine.h"
#include "game/players.h"
#include "main/console.h"
#include "networking/network_game_globals.h"
#include "networking/network_game_manager.h"
#include "networking/network_client_manager.h"
#include "objects/objects.h"
#include "network_distributed.h"
#include "network_voice.h"
#include "network_voice_records.h"
#include "halo_sim.h" /* port: port/linux/game/halo_sim.c */

#include <math.h>
#include <stdio.h>

/* port_config.c's, network_game_globals.c's, network_server_message_handler.c's,
xinput_sdl.c's, posix_net.c's, cseries_windows.c's */
int config_boolean(const char *name);
long config_integer(const char *name);
double config_real(const char *name);
const char *config_string(const char *name);
unsigned long config_changes(void);
boolean network_distributed_client_send(void *message, word size);
boolean network_distributed_server_send_to_machine(long machine_index, void *message, word size);
boolean network_distributed_server_send_to_machine_reliably(long machine_index, void *message, word size);
short network_distributed_server_machines(long *machine_indices, short maximum);
int halo_push_to_talk_held(void);
void posix_random_bytes(void *buffer, unsigned int size);
unsigned long system_milliseconds(void);

/* ---------- constants */

enum
{
	/* a frame every 20 ms, and a burst of a few (a client's frames sent
	in a clump after a stall) */
	VOICE_FRAMES_PER_SECOND = 50,
	VOICE_FRAME_BURST = 10,
	/* machines heard at once, at most */
	VOICE_MAXIMUM_TALKERS = 8,
	/* a machine talking: one whose frame came this lately (ms) */
	VOICE_TALKING_MS = 500,
	/* a machine that hears voices: one heard from this lately (ms) */
	VOICE_LISTENING_MS = 6000,
	/* how often a client says it hears voices, and the host repeats its
	settings (ms) */
	VOICE_HELLO_MS = 2000,
	VOICE_CONFIG_MS = 10000,
	/* a client forgets the host's settings unheard this long (ms) */
	VOICE_CONFIG_TIMEOUT_MS = 25000,
	/* a speaker's icon shows this long after their last frame (ms) */
	VOICE_SPEAKING_MS = 300,
	/* open mic: talking while the microphone's level is over this (about
	-40 dB), and for this long after (ms) */
	VOICE_OPEN_MIC_HOLD_MS = 400,
	VOICE_MINIMUM_KBPS = 8,
	VOICE_MAXIMUM_KBPS = 64,
};

/* the speaker's bitmap (network_voice_draw_icon): the menus' voice/speaker,
as their bitmaps are named (menu_tags.c) */
#define VOICE_ICON_BITMAP "pc\\voice\\speaker"
#define VOICE_OPEN_MIC_LEVEL 0.01f
/* proximity: full volume this near, quieter further (world units) */
#define VOICE_NEAR_DISTANCE 3.0f
#define VOICE_FARTHEST_GAIN 0.2f

/* how the host's settings travel */
enum
{
	_voice_config_lobby_bit,
};

/* network_client_manager.c's client state in game (its enum is its own) */
enum
{
	_voice_client_state_ingame = 3,
};

typedef char distributed_voice_up_size_assert[sizeof(struct distributed_voice_up) == 8 ? 1 : -1];
typedef char distributed_voice_down_size_assert[sizeof(struct distributed_voice_down) == 12 ? 1 : -1];
typedef char distributed_voice_config_size_assert[sizeof(struct distributed_voice_config) == 12 ? 1 : -1];

/* ---------- globals */

static struct voice_state voice_state = { .host_settings_read_at = (unsigned long)-1 };
static struct voice_texts voice_texts;
static struct voice_inputs voice_inputs;
/* port: the steps' arguments (halo_sim.h) */
static struct halo_sim_network_voice voice_arguments;
/* the microphone's frame (the calls' alone: the steps never read it) */
static float voice_frame[VOICE_FRAME_SAMPLES];

#define voice_machines (voice_state.machines)
#define voice_settings (voice_state.settings)
#define voice_host_settings (voice_state.host_settings)
#define voice_muted (voice_state.muted)

/* ---------- the route (pure: tools/harness/tests/voice_route.c) */

/* how a listener hears a speaker: in the lobby (and the postgame), anyone
if the lobby has voice; in a game, by the mode, teammates anywhere or near,
enemies near ("near": within the range, both alive) */
static short voice_route(
	short mode,
	boolean lobby,
	boolean lobby_voice,
	boolean teammates,
	boolean both_alive,
	real distance,
	real range)
{
	boolean near = both_alive && distance <= range;

	if (lobby)
		return lobby_voice ? _voice_route_global : _voice_route_none;
	switch (mode)
	{
	case _voice_mode_team_proximity:
		return teammates && near ? _voice_route_proximity : _voice_route_none;
	case _voice_mode_team_enemy_proximity:
		return near ? _voice_route_proximity : _voice_route_none;
	case _voice_mode_team_global:
		return teammates ? _voice_route_global : _voice_route_none;
	case _voice_mode_team_global_enemy_proximity:
		if (teammates)
			return _voice_route_global;
		return near ? _voice_route_proximity : _voice_route_none;
	default:
		return _voice_route_none;
	}
}

/* the largest packet a frame may be at the quality (kilobits a second):
twice its share, for Opus's variable rate, and a little */
static long voice_packet_limit(
	short kbps)
{
	long limit = (long)kbps * 1000 / 8 / VOICE_FRAMES_PER_SECOND * 2 + 16;

	return MIN(limit, VOICE_MAXIMUM_PACKET);
}

/* ---------- the steps' calls */

/* port: through the world's calls (the Rust port's) */
static long voice_call(
	long kind,
	long a,
	long b,
	long actor,
	real const *reals,
	short real_count,
	real *output)
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
	network_voice_call(&call);
	if (output)
		*output = call.output[0];
	return call.result;
}

/* ---------- settings */

static short voice_mode_from_text(
	char const *text)
{
	static char const *const names[NUMBER_OF_VOICE_MODES] =
	{
		"off", "team_proximity", "team_enemy_proximity", "team_global", "team_global_enemy_proximity",
	};
	short mode;

	for (mode = 0; text && mode < NUMBER_OF_VOICE_MODES; mode++)
	{
		if (!csstrcmp(text, names[mode]))
			return mode;
	}
	return _voice_mode_team_global_enemy_proximity;
}

/* the host's, from its config.toml (read again as it changes) */
static void voice_host_settings_read(
	void)
{
	if (voice_state.host_settings_read_at == voice_inputs.config_changes)
		return;
	voice_state.host_settings_read_at = voice_inputs.config_changes;
	voice_host_settings.lobby = voice_inputs.lobby_setting;
	voice_host_settings.mode = voice_mode_from_text(voice_inputs.mode_text);
	voice_host_settings.kbps = (short)PIN(voice_inputs.kbps_setting, VOICE_MINIMUM_KBPS, VOICE_MAXIMUM_KBPS);
	voice_host_settings.proximity = (real)PIN(voice_inputs.proximity_setting, 5.0, 100.0);
}

/* ---------- players */

/* a machine's first player in the game (absolute index), or NONE */
static short voice_machine_player(
	long machine_index)
{
	short index;

	if (!voice_inputs.game_in_progress)
		return NONE;
	for (index = 0; index < voice_inputs.player_count; index++)
	{
		struct voice_player const *player = &voice_inputs.players[index];

		if (player->valid && !player->quit && player->machine_index == machine_index)
			return index;
	}
	return NONE;
}

/* (the host) how a listener's machine hears a speaker's, now, by their
first players (NONE: none) */
static short voice_route_between(
	short speaker_index,
	short listener_index)
{
	struct voice_player const *speaker;
	struct voice_player const *listener;
	boolean both_alive;
	boolean teammates;
	real distance = 0.0f;

	if (voice_inputs.in_lobby)
		return voice_route(voice_host_settings.mode, TRUE, voice_host_settings.lobby, FALSE, FALSE, 0.0f, 0.0f);
	if (speaker_index == NONE || listener_index == NONE)
		return _voice_route_none;
	speaker = &voice_inputs.players[speaker_index];
	listener = &voice_inputs.players[listener_index];
	/* (co-op: one team; a game without teams: none) */
	teammates = !voice_inputs.game_engine ||
		(voice_inputs.game_engine_has_teams && speaker->team_index == listener->team_index);
	both_alive = speaker->alive && listener->alive;
	if (both_alive)
		distance = distance3d(&speaker->position, &listener->position);
	return voice_route(voice_host_settings.mode, FALSE, voice_host_settings.lobby, teammates, both_alive, distance,
		voice_host_settings.proximity);
}

/* ---------- the host */

static boolean voice_machine_valid(
	long machine_index)
{
	return machine_index >= 0 && machine_index < VOICE_MACHINES;
}

/* a machine joined at the slot: a new key, told to it before anything is
taken from it */
static void voice_machine_joined_game(
	long machine_index)
{
	if (!voice_machine_valid(machine_index))
		return;
	csmemset(&voice_machines[machine_index], 0, sizeof(voice_machines[machine_index]));
	voice_call(_network_voice_call_new_key, machine_index, 0, NONE, NULL, 0, NULL);
}

/* the settings and its key, to one client, over its stream */
static void voice_host_tell(
	long machine_index)
{
	struct distributed_voice_config *config = &voice_texts.config_message.config;

	csmemset(&voice_texts.config_message, 0, sizeof(voice_texts.config_message));
	config->key = voice_machines[machine_index].key;
	config->flags = voice_host_settings.lobby ? FLAG(_voice_config_lobby_bit) : 0;
	config->mode = (byte)voice_host_settings.mode;
	config->kbps = (byte)voice_host_settings.kbps;
	config->proximity_tenths = (word)(voice_host_settings.proximity * 10.0f);
	if (voice_call(_network_voice_call_send_config, machine_index, 0, NONE, NULL, 0, NULL))
	{
		voice_machines[machine_index].told = TRUE;
		voice_machines[machine_index].told_at = voice_inputs.now;
	}
}

/* whether a machine is one this machine plays a voice of: not muted */
static boolean voice_audible(
	long machine_index)
{
	return voice_machine_valid(machine_index) && !voice_muted[machine_index];
}

/* the gain and pan of a voice heard for being near (this machine's own
reckoning: its first player's view, and the speaker's unit) */
static void voice_proximity_gain(
	short player_index,
	real *gain,
	real *pan)
{
	struct voice_player const *player = player_index >= 0 && player_index < voice_inputs.player_count &&
		voice_inputs.players[player_index].valid ? &voice_inputs.players[player_index] : NULL;
	real_vector3d direction;
	real_vector3d right;
	real distance;
	real range = voice_settings.proximity > VOICE_NEAR_DISTANCE ? voice_settings.proximity : VOICE_NEAR_DISTANCE + 1.0f;

	*gain = 1.0f;
	*pan = 0.0f;
	if (!player || !player->alive || !voice_inputs.local_player)
		return;
	if (!voice_inputs.camera)
		return;
	vector_from_points3d(&voice_inputs.camera_position, &player->position, &direction);
	distance = magnitude3d(&direction);
	if (distance > VOICE_NEAR_DISTANCE)
	{
		real t = PIN((distance - VOICE_NEAR_DISTANCE) / (range - VOICE_NEAR_DISTANCE), 0.0f, 1.0f);

		*gain = 1.0f - (1.0f - VOICE_FARTHEST_GAIN) * t;
	}
	if (distance > 0.01f)
	{
		cross_product3d(&voice_inputs.camera_forward, &voice_inputs.camera_up, &right);
		normalize3d(&right);
		scale_vector3d(&direction, 1.0f / distance, &direction);
		*pan = 0.8f * dot_product3d(&direction, &right);
	}
}

/* a frame heard here (the host's own listening, or one relayed to a
client); the speaker's first player (absolute index, NONE for none) */
static void voice_play(
	long machine_index,
	short player_index,
	short route,
	word sequence,
	byte const *packet,
	long length)
{
	real reals[3];

	reals[0] = 1.0f;
	reals[1] = 0.0f;
	if (!voice_machine_valid(machine_index) || length <= 0)
		return;
	voice_state.heard_at[machine_index] = voice_inputs.now;
	voice_state.heard[machine_index] = TRUE;
	if (!voice_audible(machine_index))
		return;
	if (route == _voice_route_proximity)
		voice_proximity_gain(player_index, &reals[0], &reals[1]);
	reals[2] = (real)length;
	voice_call(_network_voice_call_play, machine_index, (long)packet, sequence, reals, 3, NULL);
}

/* how many machines talk now, but the one */
static short voice_host_talkers(
	long except_machine,
	unsigned long now)
{
	short count = 0;
	long index;

	for (index = 0; index < VOICE_MACHINES; index++)
	{
		if (index != except_machine && voice_machines[index].talked &&
			now - voice_machines[index].talked_at < VOICE_TALKING_MS)
		{
			count++;
		}
	}
	return count;
}

/* (the host) a frame of a machine (its own, or a client's), checked, and
sent to those that hear it */
static void voice_host_relay(
	long speaker_machine,
	word sequence,
	byte const *packet,
	long length)
{
	short local_machine = (short)voice_inputs.local_machine;
	short speaker_player = NONE;
	unsigned long now = voice_inputs.now;
	short index;

	if (!voice_machine_valid(speaker_machine) || length <= 0 ||
		length > voice_packet_limit(voice_host_settings.kbps))
	{
		return;
	}
	/* (no more than so many at once: those talking keep talking) */
	if (!(voice_machines[speaker_machine].talked && now - voice_machines[speaker_machine].talked_at < VOICE_TALKING_MS) &&
		voice_host_talkers(speaker_machine, now) >= VOICE_MAXIMUM_TALKERS)
	{
		return;
	}
	voice_machines[speaker_machine].talked = TRUE;
	voice_machines[speaker_machine].talked_at = now;
	/* (the lobby has no players' positions: the menus' game has players of
	its own) */
	if (!voice_inputs.in_lobby)
		speaker_player = voice_machine_player(speaker_machine);
	for (index = 0; index < voice_inputs.machine_count; index++)
	{
		long listener = voice_inputs.machine_indices[index];
		short route;

		if (!voice_machine_valid(listener) || listener == speaker_machine)
			continue;
		route = voice_route_between(speaker_player, voice_inputs.in_lobby ? NONE : voice_machine_player(listener));
		if (route == _voice_route_none)
			continue;
		if (listener == local_machine)
		{
			voice_play(speaker_machine, speaker_player, route, sequence, packet, length);
			continue;
		}
		/* (only to a machine that has said it hears voices) */
		if (!voice_machines[listener].heard || now - voice_machines[listener].heard_at >= VOICE_LISTENING_MS ||
			!voice_machines[listener].told)
		{
			continue;
		}
		csmemset(&voice_texts.down_message, 0, sizeof(voice_texts.down_message.header) +
			sizeof(voice_texts.down_message.down));
		voice_texts.down_message.down.key = voice_machines[listener].key;
		voice_texts.down_message.down.sequence = sequence;
		voice_texts.down_message.down.machine_index = (byte)speaker_machine;
		voice_texts.down_message.down.player_index = speaker_player != NONE ? (byte)speaker_player : NO_PLAYER;
		voice_texts.down_message.down.route = (byte)route;
		voice_texts.down_message.down.length = (byte)length;
		csmemcpy(voice_texts.down_message.packet, packet, length);
		voice_call(_network_voice_call_send_down, listener,
			(long)(sizeof(voice_texts.down_message.header) + sizeof(voice_texts.down_message.down) + length), NONE,
			NULL, 0, NULL);
	}
}

/* (the host) a client's frame: its key, its allowance, the settings' */
static void voice_host_handle_up(
	long machine_index,
	struct distributed_voice_up const *up,
	byte const *packet,
	long length)
{
	unsigned long now = voice_inputs.now;

	if (!voice_machine_valid(machine_index) || !voice_machines[machine_index].key ||
		up->key != voice_machines[machine_index].key || up->length != length)
	{
		return;
	}
	voice_machines[machine_index].heard = TRUE;
	voice_machines[machine_index].heard_at = now;
	/* (an empty frame: it hears voices) */
	if (!length)
		return;
	/* (its allowance: so many frames a second, a few at once) */
	if (!voice_machines[machine_index].allowance_at)
		voice_machines[machine_index].allowance = VOICE_FRAME_BURST;
	else
	{
		voice_machines[machine_index].allowance += (real)(now - voice_machines[machine_index].allowance_at) *
			VOICE_FRAMES_PER_SECOND / 1000.0f;
	}
	voice_machines[machine_index].allowance = MIN(voice_machines[machine_index].allowance, (real)VOICE_FRAME_BURST);
	voice_machines[machine_index].allowance_at = now;
	if (voice_machines[machine_index].allowance < 1.0f)
		return;
	voice_machines[machine_index].allowance -= 1.0f;
	voice_host_relay(machine_index, up->sequence, packet, length);
}

static void voice_handle_message_game(
	struct halo_sim_network_voice const *arguments)
{
	long machine_index = arguments->machine_index;
	word const *message = (word const *)arguments->message;
	struct distributed_message_header header;
	byte const *entries = (byte const *)message + sizeof(header);
	long length = (long)arguments->size - (long)sizeof(header);

	csmemcpy(&header, message, sizeof(header));
	switch (header.type)
	{
	case _distributed_message_voice_up:
	{
		struct distributed_voice_up up;

		if (machine_index == NONE || !voice_inputs.host || length < (long)sizeof(up))
			return;
		csmemcpy(&up, entries, sizeof(up));
		voice_host_handle_up(machine_index, &up, entries + sizeof(up), length - (long)sizeof(up));
		break;
	}
	case _distributed_message_voice_down:
	{
		struct distributed_voice_down down;

		if (machine_index != NONE || voice_inputs.host || !voice_state.settings_heard || length < (long)sizeof(down))
			return;
		csmemcpy(&down, entries, sizeof(down));
		if (down.key != voice_state.key || down.length != length - (long)sizeof(down) ||
			down.machine_index == voice_inputs.local_machine)
		{
			return;
		}
		voice_play(down.machine_index, down.player_index != NO_PLAYER && !voice_inputs.in_lobby ? down.player_index :
			NONE, down.route, down.sequence, entries + sizeof(down), down.length);
		break;
	}
	case _distributed_message_voice_config:
	{
		struct distributed_voice_config config;

		if (machine_index != NONE || voice_inputs.host || length < (long)sizeof(config))
			return;
		csmemcpy(&config, entries, sizeof(config));
		voice_state.key = config.key;
		voice_settings.lobby = TEST_FLAG(config.flags, _voice_config_lobby_bit);
		voice_settings.mode = config.mode < NUMBER_OF_VOICE_MODES ? config.mode : _voice_mode_off;
		voice_settings.kbps = (short)PIN(config.kbps, VOICE_MINIMUM_KBPS, VOICE_MAXIMUM_KBPS);
		voice_settings.proximity = config.proximity_tenths / 10.0f;
		voice_state.settings_heard = TRUE;
		voice_state.settings_heard_at = voice_inputs.now;
		break;
	}
	}
}

/* whether this machine may talk now, as the host's settings have it */
static boolean voice_may_talk(
	void)
{
	if (voice_inputs.in_lobby)
		return voice_settings.lobby;
	return voice_settings.mode != _voice_mode_off;
}

/* the host: every client told its key and the settings (again as they
change, and now and then) */
static void voice_host_update(
	void)
{
	short local_machine = (short)voice_inputs.local_machine;
	boolean changed;
	unsigned long now = voice_inputs.now;
	short index;

	voice_host_settings_read();
	changed = csmemcmp(&voice_host_settings, &voice_state.host_settings_told, sizeof(voice_host_settings)) != 0;
	voice_state.host_settings_told = voice_host_settings;
	/* (the host's own, as a client hears them) */
	voice_settings = voice_host_settings;
	voice_state.settings_heard = TRUE;
	voice_state.settings_heard_at = now;
	for (index = 0; index < voice_inputs.machine_count; index++)
	{
		long machine_index = voice_inputs.machine_indices[index];

		if (!voice_machine_valid(machine_index) || machine_index == local_machine)
			continue;
		if (!voice_machines[machine_index].key)
			voice_machine_joined_game(machine_index);
		if (!voice_machines[machine_index].told || changed ||
			now - voice_machines[machine_index].told_at >= VOICE_CONFIG_MS)
		{
			voice_host_tell(machine_index);
		}
	}
}

/* this machine's frame: the host's own relayed, a client's sent up (none:
it hears voices) */
static void voice_send(
	byte const *packet,
	long length)
{
	if (voice_inputs.host)
	{
		if (length > 0)
			voice_host_relay(voice_inputs.local_machine, voice_state.sequence, packet, length);
	}
	else
	{
		csmemset(&voice_texts.up_message, 0, sizeof(voice_texts.up_message.header) + sizeof(voice_texts.up_message.up));
		voice_texts.up_message.up.key = voice_state.key;
		voice_texts.up_message.up.sequence = voice_state.sequence;
		voice_texts.up_message.up.length = (byte)length;
		if (length > 0)
			csmemcpy(voice_texts.up_message.packet, packet, length);
		voice_call(_network_voice_call_send_up, 0,
			(long)(sizeof(voice_texts.up_message.header) + sizeof(voice_texts.up_message.up) + length), NONE, NULL, 0,
			NULL);
	}
	if (length > 0)
		voice_state.sequence++;
}

/* the session over (or none): the microphone closed, voices forgotten */
static void voice_end_session(
	void)
{
	if (!voice_state.session)
		return;
	voice_state.session = FALSE;
	voice_state.microphone_wanted = FALSE;
	voice_call(_network_voice_call_microphone, FALSE, 0, NONE, NULL, 0, NULL);
	voice_call(_network_voice_call_forget_all, 0, 0, NONE, NULL, 0, NULL);
	voice_state.settings_heard = FALSE;
	voice_state.talked = FALSE;
	csmemset(voice_state.heard, 0, sizeof(voice_state.heard));
}

static void voice_update_game(
	void)
{
	char const *chat = voice_inputs.chat_text;
	boolean open_mic = !csstrcmp(chat, "open_mic");
	boolean push_to_talk = !csstrcmp(chat, "push_to_talk");
	boolean held;
	unsigned long now = voice_inputs.now;
	real volume = (real)PIN(voice_inputs.volume_setting, 0.0, 2.0);
	long index;

	voice_call(_network_voice_call_set_volume, 0, 0, NONE, &volume, 1, NULL);
	/* (a network game: the host's, or one joined) */
	if (!voice_inputs.network_game || voice_inputs.local_machine == NONE)
	{
		voice_end_session();
		return;
	}
	voice_state.session = TRUE;
	if (voice_inputs.host)
		voice_host_update();
	else if (voice_state.settings_heard && now - voice_state.settings_heard_at >= VOICE_CONFIG_TIMEOUT_MS)
	{
		/* (the host repeats them: one that stopped has no voice chat) */
		voice_state.settings_heard = FALSE;
	}
	/* (mutes of a machine whose players changed are forgotten) */
	for (index = 0; index < VOICE_MACHINES; index++)
	{
		if (!voice_muted[index])
			continue;
		if (csmemcmp(voice_inputs.machine_names[index], voice_state.muted_names[index],
			sizeof(voice_state.muted_names[index])))
		{
			voice_muted[index] = FALSE;
		}
	}
	if (!voice_state.settings_heard)
	{
		voice_call(_network_voice_call_microphone, FALSE, 0, NONE, NULL, 0, NULL);
		voice_state.microphone_wanted = FALSE;
		return;
	}
	/* (a client says it hears voices) */
	if (!voice_inputs.host && now - voice_state.hello_at >= VOICE_HELLO_MS)
	{
		voice_state.hello_at = now;
		voice_send(NULL, 0);
	}
	/* the microphone: open mic, open while voice is on; push to talk, from
	the first press (none asked for until then) */
	held = push_to_talk && voice_call(_network_voice_call_push_to_talk_held, 0, 0, NONE, NULL, 0, NULL) &&
		!voice_inputs.console_active;
	if (!voice_may_talk() || (!open_mic && !push_to_talk))
		voice_state.microphone_wanted = FALSE;
	else if (open_mic || held)
		voice_state.microphone_wanted = TRUE;
	voice_call(_network_voice_call_microphone, voice_state.microphone_wanted, 0, NONE, NULL, 0, NULL);
	while (voice_state.microphone_wanted && voice_call(_network_voice_call_read_frame, 0, 0, NONE, NULL, 0, NULL))
	{
		boolean talk = held;
		long length;

		if (open_mic)
		{
			real level;

			voice_call(_network_voice_call_level, 0, 0, NONE, NULL, 0, &level);
			if (level >= VOICE_OPEN_MIC_LEVEL)
				voice_state.open_mic_until = now + VOICE_OPEN_MIC_HOLD_MS;
			talk = (long)(voice_state.open_mic_until - now) > 0;
		}
		if (!talk || !voice_may_talk())
			continue;
		length = voice_call(_network_voice_call_encode, voice_settings.kbps * 1000,
			MIN(voice_packet_limit(voice_settings.kbps), VOICE_MAXIMUM_PACKET), NONE, NULL, 0, NULL);
		if (length <= 0)
			continue;
		voice_send(voice_texts.packet, length);
		voice_state.talked = TRUE;
		voice_state.talked_at = now;
	}
}

/* port: what the steps read of the game, gathered before them */
static void voice_gather(
	void)
{
	struct voice_inputs *inputs = &voice_inputs;
	struct network_game_client *client = global_network_game_client_get();
	struct network_game *game = client ? network_game_client_get_game(client) : NULL;
	char const *text;
	short index;

	csmemset(inputs, 0, sizeof(*inputs));
	inputs->now = system_milliseconds();
	inputs->config_changes = config_changes();
	inputs->lobby_setting = config_boolean("network.voice_lobby") != 0;
	text = config_string("network.voice_mode");
	if (text)
		snprintf(inputs->mode_text, sizeof(inputs->mode_text), "%s", text);
	else
		inputs->mode_text[0] = 0;
	inputs->kbps_setting = config_integer("network.voice_kbps");
	inputs->proximity_setting = config_real("network.voice_proximity");
	text = config_string("audio.voice_chat");
	if (text)
		snprintf(inputs->chat_text, sizeof(inputs->chat_text), "%s", text);
	inputs->volume_setting = config_real("audio.voice_volume");
	inputs->network_game = game != NULL;
	inputs->host = global_network_game_server_get() != NULL;
	inputs->local_machine = network_game_client_get_local_machine_index();
	/* (voice_in_lobby: this machine's network game not in game, or in a
	multiplayer game's postgame) */
	{
		short state_data;

		inputs->in_lobby = !client || network_game_client_get_state(client, &state_data) != _voice_client_state_ingame ||
			!game_in_progress() || (game_engine && game_engine_showing_postgame());
	}
	inputs->game_in_progress = game_in_progress();
	inputs->game_engine = game_engine != NULL;
	inputs->game_engine_has_teams = game_engine && game_engine_has_teams();
	inputs->console_active = console_is_active();
	inputs->local_player = local_player_get_next(NONE) != NONE;
	if (inputs->local_player)
	{
		struct observer_result const *camera = observer_get_camera((short)local_player_get_next(NONE));

		if (camera)
		{
			inputs->camera = TRUE;
			inputs->camera_position = camera->position;
			inputs->camera_forward = camera->forward;
			inputs->camera_up = camera->up;
		}
	}
	if (inputs->host)
		inputs->machine_count = network_distributed_server_machines(inputs->machine_indices, VOICE_MACHINES);
	if (game_in_progress() && player_data)
	{
		inputs->player_count = (short)MIN(player_data->maximum_count, VOICE_PLAYERS);
		for (index = 0; index < inputs->player_count; index++)
		{
			struct player_datum *player = distributed_player(index);
			struct voice_player *voice_player = &inputs->players[index];
			long unit_index;

			if (!player)
				continue;
			voice_player->valid = TRUE;
			voice_player->quit = player->quit_out_of_game;
			voice_player->machine_index = player->network_player_data.machine_index;
			voice_player->team_index = player->team_index;
			unit_index = distributed_living_unit(player);
			if (unit_index != NONE)
			{
				voice_player->alive = TRUE;
				object_get_origin(unit_index, &voice_player->position);
			}
		}
	}
	/* (voice_machine_name: each slot's first player's name, as the network
	game has it) */
	for (index = 0; game && index < HALO_PORT_MAXIMUM_NETWORK_PLAYERS; index++)
	{
		struct network_player *player = &game->players[index];

		if (network_player_is_valid(player) && voice_machine_valid(player->machine_index) &&
			!inputs->machine_names[player->machine_index][0])
		{
			csmemcpy(inputs->machine_names[player->machine_index], player->name, sizeof(player->name));
		}
	}
}

/* port: a step of voice chat: Rust's in Rust's mode (crates/halo_map's
network_voice), checked against the game's otherwise. Its world: the
arguments, what it reads of the game, voice chat's records and the
messages. */
static void voice_step(
	long step)
{
	struct halo_sim_network_voice *arguments = &voice_arguments;
	struct halo_sim_world_context context;
	struct halo_sim_world_region regions[4];
	int checking = FALSE;

	arguments->state = (unsigned long)&voice_state;
	arguments->texts = (unsigned long)&voice_texts;
	arguments->inputs = (unsigned long)&voice_inputs;
	regions[0].address = (unsigned long)arguments;
	regions[0].size = sizeof(*arguments);
	regions[1].address = (unsigned long)&voice_inputs;
	regions[1].size = sizeof(voice_inputs);
	regions[2].address = (unsigned long)&voice_state;
	regions[2].size = sizeof(voice_state);
	regions[3].address = (unsigned long)&voice_texts;
	regions[3].size = sizeof(voice_texts);
	memset(&context, 0, sizeof(context));
	context.game_time = game_in_progress() ? game_time_get() : 0;
	context.arguments = (unsigned long)arguments;
	if (halo_sim_world_step_begin(step, &context, regions, 4, network_voice_call, &checking))
		return;
	switch (step)
	{
	case _halo_sim_world_step_network_voice_update: voice_update_game(); break;
	case _halo_sim_world_step_network_voice_message: voice_handle_message_game(arguments); break;
	case _halo_sim_world_step_network_voice_joined: voice_machine_joined_game(arguments->machine_index); break;
	}
	if (checking)
		halo_sim_world_step_check(step);
}

/* a message's headers: the transport's (its size: the connection checks it)
and the distributed netcode's (its time the game's, none in the lobby) */
static void voice_fill_header(
	struct distributed_message_header *header,
	byte type,
	word size)
{
	header->header = 0;
	build_message_header(&header->header, size, 2, 0);
	header->type = type;
	header->count = 1;
	header->game_time = game_in_progress() ? game_time_get() : 0;
}

/* ---------- public code */

/* port: the steps' calls (halo_sim.h's _network_voice_call_*) */
void network_voice_call(
	struct halo_sim_ai_call *call)
{
	int opened = halo_sim_world_event_begin(call);

	switch (call->kind)
	{
	case _network_voice_call_send_config:
		voice_fill_header(&voice_texts.config_message.header, _distributed_message_voice_config,
			(word)sizeof(voice_texts.config_message));
		call->result = network_distributed_server_send_to_machine_reliably(call->a, &voice_texts.config_message,
			(word)sizeof(voice_texts.config_message));
		break;
	case _network_voice_call_send_down:
		voice_fill_header(&voice_texts.down_message.header, _distributed_message_voice_down, (word)call->b);
		network_distributed_server_send_to_machine(call->a, &voice_texts.down_message, (word)call->b);
		break;
	case _network_voice_call_send_up:
		voice_fill_header(&voice_texts.up_message.header, _distributed_message_voice_up, (word)call->b);
		network_distributed_client_send(&voice_texts.up_message, (word)call->b);
		break;
	case _network_voice_call_play:
		voice_audio_play((int)call->a, (unsigned short)call->actor, (unsigned char const *)call->b,
			(int)call->input[2], call->input[0], call->input[1]);
		break;
	case _network_voice_call_microphone:
		voice_audio_microphone((int)call->a);
		break;
	case _network_voice_call_read_frame:
		call->result = voice_audio_read_frame(voice_frame);
		break;
	case _network_voice_call_level:
		call->output[0] = voice_audio_level(voice_frame);
		break;
	case _network_voice_call_encode:
		call->result = voice_audio_encode(voice_frame, (int)call->a, voice_texts.packet, (int)call->b);
		break;
	case _network_voice_call_forget_all:
		voice_audio_forget_all();
		break;
	case _network_voice_call_set_volume:
		voice_audio_set_volume(call->input[0]);
		break;
	case _network_voice_call_push_to_talk_held:
		call->result = halo_push_to_talk_held() != 0;
		break;
	case _network_voice_call_new_key:
		if (voice_machine_valid(call->a))
		{
			do
			{
				posix_random_bytes(&voice_machines[call->a].key, sizeof(voice_machines[call->a].key));
			} while (!voice_machines[call->a].key);
		}
		break;
	}
	halo_sim_world_event_end(call, opened);
}

/* (network_server_manager.c) a machine joined at the slot: a new key, told
to it before anything is taken from it */
void network_voice_machine_joined(
	long machine_index)
{
	voice_gather();
	memset(&voice_arguments, 0, sizeof(voice_arguments));
	voice_arguments.machine_index = machine_index;
	voice_step(_halo_sim_world_step_network_voice_joined);
}

/* ---------- messages (network_distributed.c) */

boolean network_voice_handles_message(
	word const *message,
	word size)
{
	struct distributed_message_header header;

	if (size < sizeof(header))
		return FALSE;
	csmemcpy(&header, message, sizeof(header));
	return header.type == _distributed_message_voice_up || header.type == _distributed_message_voice_down ||
		header.type == _distributed_message_voice_config;
}

void network_voice_handle_message(
	long machine_index,
	word const *message,
	word size)
{
	if (!network_voice_handles_message(message, size))
		return;
	voice_gather();
	memset(&voice_arguments, 0, sizeof(voice_arguments));
	voice_arguments.machine_index = machine_index;
	voice_arguments.message = (unsigned long)message;
	voice_arguments.size = size;
	voice_step(_halo_sim_world_step_network_voice_message);
}

/* ---------- every frame (main.c) */

void network_voice_update(
	void)
{
	voice_gather();
	memset(&voice_arguments, 0, sizeof(voice_arguments));
	voice_step(_halo_sim_world_step_network_voice_update);
}

/* ---------- the menus and the scoreboard (network_voice.h) */

/* a machine's first player's name as the network game has it (empty if
none) */
static void voice_machine_name(
	long machine_index,
	wchar_t *name)
{
	struct network_game_client *client = global_network_game_client_get();
	struct network_game *game = client ? network_game_client_get_game(client) : NULL;
	short index;

	csmemset(name, 0, VOICE_NAME_LENGTH * sizeof(*name));
	for (index = 0; game && index < HALO_PORT_MAXIMUM_NETWORK_PLAYERS; index++)
	{
		struct network_player *player = &game->players[index];

		if (network_player_is_valid(player) && player->machine_index == machine_index)
		{
			csmemcpy(name, player->name, sizeof(player->name));
			return;
		}
	}
}

boolean network_voice_machine_speaking(
	long machine_index)
{
	unsigned long now = system_milliseconds();

	if (!voice_state.session || !voice_machine_valid(machine_index))
		return FALSE;
	if (machine_index == network_game_client_get_local_machine_index())
		return voice_state.talked && now - voice_state.talked_at < VOICE_SPEAKING_MS;
	return voice_state.heard[machine_index] && now - voice_state.heard_at[machine_index] < VOICE_SPEAKING_MS &&
		!voice_muted[machine_index];
}

boolean network_voice_machine_muted(
	long machine_index)
{
	return voice_machine_valid(machine_index) && voice_muted[machine_index];
}

void network_voice_mute_machine(
	long machine_index,
	boolean mute)
{
	if (!voice_machine_valid(machine_index) || machine_index == network_game_client_get_local_machine_index())
		return;
	voice_muted[machine_index] = mute;
	voice_machine_name(machine_index, voice_state.muted_names[machine_index]);
	if (mute)
		voice_audio_forget((int)machine_index);
}

boolean network_voice_available(
	void)
{
	return voice_state.session && voice_state.settings_heard;
}

/* a speaker drawn in the rectangle (a square of its height, at its left;
muted: struck through, in red), fading with alpha: Lucide's icons
(port/assets/icons/lucide), drawn white as the menus' bitmap voice/speaker,
a frame each (tools/ce_menus.py), and tinted; none without the menus'
bitmaps */
void network_voice_draw_icon(
	rectangle2d const *bounds,
	boolean muted,
	real alpha)
{
	long bitmap_index = tag_loaded(BITMAP_GROUP_TAG, VOICE_ICON_BITMAP);
	struct bitmap_data *bitmap = bitmap_index != NONE ? bitmap_group_try_and_get_bitmap(bitmap_index, muted ? 1 : 0) :
		NULL;
	pixel32 a = (pixel32)(PIN(alpha, 0.0f, 1.0f) * 255.0f + 0.5f) << 24;
	rectangle2d square = *bounds;

	if (!bitmap || bounds->y1 - bounds->y0 < 4)
		return;
	square.x1 = (short)(square.x0 + (square.y1 - square.y0));
	draw_bitmap_in_rect(bitmap, &square, NULL, NULL, a | (muted ? 0x00E05A5A : 0x0050E050), NULL, TRUE);
}
