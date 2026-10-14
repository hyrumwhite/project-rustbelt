/*
NETWORK_VOICE_RECORDS.H

Voice chat's messages' entries and the records each machine keeps
(network_voice.c), as types, for the Rust port's layouts (crates/halo_map's
network_voice, tools/tag_layout/probe_net.c).
*/

#ifndef __NETWORK_VOICE_RECORDS_H
#define __NETWORK_VOICE_RECORDS_H

#include "../src/voice_audio.h"

#define VOICE_MACHINES HALO_PORT_MAXIMUM_NETWORK_MACHINES
#define VOICE_PLAYERS HALO_PORT_MAXIMUM_NETWORK_PLAYERS
/* a player's name (struct network_player's), and a setting's text as read */
#define VOICE_NAME_LENGTH 12
#define VOICE_SETTING_TEXT_SIZE 32

/* a client's frame to the host (its Opus packet follows; none: it hears
voices, and says so) */
struct distributed_voice_up
{
	unsigned long key;
	word sequence;
	byte length;
	byte pad;
};

/* a frame relayed to a client (the packet follows) */
struct distributed_voice_down
{
	unsigned long key;
	word sequence;
	/* the machine that spoke, and its first player (absolute; NO_PLAYER
	in the lobby) */
	byte machine_index;
	byte player_index;
	/* how it is heard (_voice_route_*) */
	byte route;
	byte length;
	byte pad[2];
};

/* the host's settings, to each client over its stream, with its key */
struct distributed_voice_config
{
	unsigned long key;
	byte flags;
	byte mode;
	byte kbps;
	byte pad0;
	word proximity_tenths;
	byte pad[2];
};

/* the host's settings as they are used */
struct voice_settings
{
	boolean lobby;
	short mode;
	short kbps;
	real proximity;
};

/* the host: a machine slot's key, whether it was told it, and when it was
told the settings last; when it was heard from; its frames' allowance and
when it was topped up; when its last frame was relayed */
struct voice_machine
{
	unsigned long key;
	boolean told;
	unsigned long told_at;
	unsigned long heard_at;
	boolean heard;
	real allowance;
	unsigned long allowance_at;
	unsigned long talked_at;
	boolean talked;
};

/* everything voice chat keeps */
struct voice_state
{
	struct voice_machine machines[VOICE_MACHINES];
	struct voice_settings host_settings;
	unsigned long host_settings_read_at;
	/* (the settings told, to tell again when they change) */
	struct voice_settings host_settings_told;
	/* this machine: the host's settings and its key (heard: a client), when
	they came; the sequence of its frames; whether it talked lately, and the
	microphone was asked for */
	struct voice_settings settings;
	unsigned long key;
	boolean settings_heard;
	unsigned long settings_heard_at;
	word sequence;
	unsigned long hello_at;
	unsigned long talked_at;
	boolean talked;
	unsigned long open_mic_until;
	boolean microphone_wanted;
	boolean session;
	/* every machine slot's last frame heard (for its icon), and whether this
	machine mutes it, with the names it had then (a new machine at the slot is
	not muted) */
	unsigned long heard_at[VOICE_MACHINES];
	boolean heard[VOICE_MACHINES];
	boolean muted[VOICE_MACHINES];
	wchar_t muted_names[VOICE_MACHINES][VOICE_NAME_LENGTH];
};

/* the steps' messages and the packet this machine encoded */
struct voice_texts
{
	struct
	{
		struct distributed_message_header header;
		struct distributed_voice_config config;
	} config_message;
	struct
	{
		struct distributed_message_header header;
		struct distributed_voice_up up;
		byte packet[VOICE_MAXIMUM_PACKET];
	} up_message;
	struct
	{
		struct distributed_message_header header;
		struct distributed_voice_down down;
		byte packet[VOICE_MAXIMUM_PACKET];
	} down_message;
	byte packet[VOICE_MAXIMUM_PACKET];
};

/* a player as voice chat sees it */
struct voice_player
{
	boolean valid;
	boolean quit;
	/* its living unit's origin (alive: it has one) */
	boolean alive;
	byte pad;
	long machine_index;
	long team_index;
	real_point3d position;
};

/* what the steps read of the game, gathered before them */
struct voice_inputs
{
	/* system_milliseconds; config_changes; config.toml's network.voice_lobby,
	voice_mode (its text), voice_kbps and voice_proximity, audio.voice_chat
	(its text) and voice_volume */
	unsigned long now;
	unsigned long config_changes;
	boolean lobby_setting;
	char mode_text[VOICE_SETTING_TEXT_SIZE];
	long kbps_setting;
	double proximity_setting;
	char chat_text[VOICE_SETTING_TEXT_SIZE];
	double volume_setting;
	/* this machine: in a network game, hosting it, its machine index; in the
	lobby as voice has it (voice_in_lobby); a game in progress, a game engine
	(with teams); the console open; a local player, and its camera */
	boolean network_game;
	boolean host;
	boolean in_lobby;
	boolean game_in_progress;
	boolean game_engine;
	boolean game_engine_has_teams;
	boolean console_active;
	boolean local_player;
	boolean camera;
	long local_machine;
	real_point3d camera_position;
	real_vector3d camera_forward;
	real_vector3d camera_up;
	/* the host's machines (network_distributed_server_machines) */
	short machine_count;
	short player_count;
	long machine_indices[VOICE_MACHINES];
	/* the game's players (by absolute index, player_count of them), and each
	machine slot's first player's name as the network game has it */
	struct voice_player players[VOICE_PLAYERS];
	wchar_t machine_names[VOICE_MACHINES][VOICE_NAME_LENGTH];
};

#endif
