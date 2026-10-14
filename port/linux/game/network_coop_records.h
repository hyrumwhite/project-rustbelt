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

/* the device groups a level has, told apart (network_coop.c's
MAXIMUM_DEVICE_GROUPS) */
#define COOP_MAXIMUM_DEVICE_GROUPS 1024

/* one of the host's device groups, a scenario's (group_index) or a
device's own (its object and role) */
struct distributed_coop_device_group
{
	short group_index;
	short name_index;
	long object_index;
	long definition_index;
	byte role;
	byte flags;
	/* counts the host's immediate sets (wraps at 256) */
	byte snaps;
	/* sent because it changed, rather than in the rotation */
	byte changed;
	real value;
};

#define MAXIMUM_DEVICE_GROUPS_PER_MESSAGE 64

struct distributed_coop_device_groups_message
{
	struct distributed_message_header header;
	struct distributed_coop_device_group groups[COOP_MAXIMUM_DEVICE_GROUPS];
};

/* host: each device group as last sent, indexed by group */
struct coop_host_devices
{
	/* false until the first tick, which records the map's starting state */
	boolean started;
	real values[COOP_MAXIMUM_DEVICE_GROUPS];
	byte flags[COOP_MAXIMUM_DEVICE_GROUPS];
	byte snaps[COOP_MAXIMUM_DEVICE_GROUPS];
	/* ticks this group is still to be sent as a change */
	byte sends[COOP_MAXIMUM_DEVICE_GROUPS];
	/* immediate sets so far (devices.c calls network_coop_note_device_snap) */
	byte snap_counts[COOP_MAXIMUM_DEVICE_GROUPS];
	/* whether this group has changed since the map loaded */
	boolean moved[COOP_MAXIMUM_DEVICE_GROUPS];
	short refresh_next, moved_refresh_next;
	struct distributed_coop_device_groups_message message;
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

/* the host's events queued, each sent in this many ticks' messages
(network_coop.c's MAXIMUM_QUEUED_EVENTS, EVENT_SENDS) */
#define COOP_MAXIMUM_QUEUED_EVENTS 128
#define COOP_EVENT_SENDS 3

/* distributed_coop_event.kind */
enum
{
	_coop_event_sound,
	_coop_event_title,
	_coop_event_hud,
	_coop_event_player_effect,
	_coop_event_nav_point,
	_coop_event_unit_animation,
	_coop_event_scenery_animation,
	_coop_event_effect,
	_coop_event_attach,
	/* the host skipped the cutscene: a client stops its dialogue */
	/* the host reverted to its last saved state */
	_coop_event_reverted,
	/* a unit opened (value TRUE) or closed: a dropship's doors */
	_coop_event_unit_open,
	/* a breakable surface broke: value its index, frame its BSP, reals the
	damage's epicenter */
	_coop_event_surface_broken,
};

/* distributed_coop_event.type of an attach event */
enum
{
	_coop_attach,
	_coop_detach,
};

/* Something that happens once. Which fields mean what depends on the kind:

	sound             type: _coop_sound_ kind; tag_index; object_index (the source, or NONE); reals[0]: scale
	title             value: title index; reals[0]: delay in seconds
	hud               type: _coop_hud_ kind; value
	player_effect     type: _coop_player_effect_ kind; reals: the function's arguments
	nav_point         type: _coop_nav_point_ kind; value: nav index (NONE to deactivate);
	                  target: team or unit; object_index: cutscene flag or object; reals[0]: offset
	unit_animation    object_index: unit; tag_index: graph (NONE: stop); value: animation;
	                  frame; interpolate
	scenery_animation object_index, name_index, definition_index: the scenery; tag_index: graph;
	                  value: animation; frame */
struct distributed_coop_event
{
	byte kind;
	byte type;
	word number;
	short value;
	short frame;
	short name_index;
	byte interpolate;
	byte pad;
	long object_index;
	long target;
	long tag_index;
	long definition_index;
	real reals[3];
};

/* host: events still to be sent; client: the last one applied */
struct coop_events
{
	struct distributed_coop_event events[COOP_MAXIMUM_QUEUED_EVENTS];
	short sends[COOP_MAXIMUM_QUEUED_EVENTS];
	short count;
	word next_number;
	word applied_number;
	boolean applied_any;
};

#define COOP_MAXIMUM_ATTACHMENTS 32
#define COOP_MAXIMUM_NAV_POINTS 16
#define COOP_MAXIMUM_LOOPING_SOUNDS 32

/* host: what its scripts have attached, sent again every
OBJECT_REFRESH_TICKS for a client that joined since */
struct coop_host_attachment
{
	long parent_index;
	long child_index;
	short parent_marker_index;
	short child_marker_index;
};

/* host: a nav point active, as the note was given */
struct coop_host_nav_point
{
	short kind;
	short nav_index;
	long target;
	long marker;
	real vertical_offset;
};

/* host: the HUD state the scripts set that a late joiner would lack: the
objective and the nav points active, resent by host_send_hud_state */
struct coop_host_hud_state
{
	short objective;
	short nav_point_count;
	struct coop_host_nav_point nav_points[COOP_MAXIMUM_NAV_POINTS];
};

/* host: a looping sound the scripts have started (music, ambience), resent
every OBJECT_REFRESH_TICKS so a late joiner hears it */
struct coop_host_looping_sound
{
	long definition_index;
	long object_index;
	real scale;
};

/* host: the scripts' state a late joiner would lack, and where the broken
surfaces' resend goes on from */
struct coop_host_queue
{
	struct coop_host_attachment attachments[COOP_MAXIMUM_ATTACHMENTS];
	short attachment_count;
	short looping_sound_count;
	struct coop_host_looping_sound looping_sounds[COOP_MAXIMUM_LOOPING_SOUNDS];
	struct coop_host_hud_state hud_state;
	short broken_surface_cursor;
	short pad;
};

/* the scenario object names' bits (network_coop.c's OBJECT_NAME_BYTES) */
#define COOP_OBJECT_NAME_BYTES (MAXIMUM_OBJECT_NAMES_PER_SCENARIO / 8)

/* one bit per scenario object name: whether the host has that object */
struct distributed_coop_object_names
{
	short structure_bsp_index;
	short name_count;
	byte present[COOP_OBJECT_NAME_BYTES];
};

struct distributed_coop_object_names_message
{
	struct distributed_message_header header;
	struct distributed_coop_object_names names;
};

/* the scripts' full-screen effects, as the host's rasterizer has them */
struct distributed_coop_screen_effect_message
{
	struct distributed_message_header header;
	struct rasterizer_screen_effect_port_state state;
};

/* distributed_coop_presentation.flags */
enum
{
	_presentation_cinematic_bit = 0,
	_presentation_letterbox_bit,
	_presentation_fading_out_bit,
	_presentation_skippable_bit,
	_presentation_show_hud_bit,
	_presentation_show_help_text_bit,
	_presentation_timer_paused_bit,
	_presentation_timer_enabled_bit,
};

/* what the host shows: the cinematic, fades and letterbox, its camera, the
skip vote, the scripts' HUD settings and timer, allegiances */
struct distributed_coop_presentation
{
	byte flags;
	byte fade_color[3];
	short fade_ticks;
	/* ticks since the fade began, capped at SHORT_MAX */
	short fade_elapsed;
	/* the host's game time when the fade began; a new value means a new fade */
	long fade_start_time;
	real_point3d camera_position;
	struct distributed_vector camera_forward;
	struct distributed_vector camera_up;
	word camera_field_of_view;
	/* the skip vote: machines that voted, out of how many */
	byte skip_votes;
	byte skip_voters;
	/* the scripts' HUD settings (hud_unit.c, hud_weapon.c) */
	long unit_hud_flags;
	long weapon_hud_flags;
	/* the script timer (hud_messaging.c); the clocks agree, so its
	reference time means the same on every machine */
	long timer_reference_time;
	short timer_ticks;
	short timer_flash_cutoff;
	short timer_x;
	short timer_y;
	short timer_corner;
	/* whether the scripts have the host's camera (camera_control): else a
	cutscene leaves it with the player, and a client spectates */
	byte camera_scripted;
	/* whether the scripts hold the players' controls (player_enable_input) */
	byte input_disabled;
	/* the object the camera films (camera_object_get) and the camera's
	offset from it: a client puts its camera by its own copy, which its
	timing may have a tick from the host's */
	long camera_object_index;
	real_vector3d camera_object_offset;
	/* whether the scripts' screen shake is running on the host. A client
	whose shake outlives it (the stop was skipped with a cutscene, or lost)
	ends its own. */
	byte scripted_shake;
	/* whether the scripts have set the players' maximum vitality, and to
	what (network_coop_set_players_vitality) */
	byte players_vitality_set;
	/* the cluster the scripts keep active (object_pvs_activate) plus one,
	0 for none: a cutscene filmed far from every player (b30's Elite at the
	shaft door) is active on the clients too. Plus one so the zeroed padding
	of a host without it reads as none. */
	short activating_cluster;
	real players_maximum_body_vitality;
	real players_maximum_shield_vitality;
	/* which teams are allies and friends (game_allegiance.c): the scripts
	make them, so a client's would leave the marines its enemies */
	unsigned long ally_teams[GAME_ALLEGIANCE_BITVECTOR_LONGS];
	unsigned long friendly_teams[GAME_ALLEGIANCE_BITVECTOR_LONGS];
};

struct distributed_coop_presentation_message
{
	struct distributed_message_header header;
	struct distributed_coop_presentation presentation;
};

/* The cutscene skip vote. The host records when it last heard from each
client that is showing the cutscene, and when that client last voted. A
client keeps the host's latest tally for display. */
struct coop_skip_vote
{
	boolean voted;
	boolean offered;
	short votes;
	short voters;
	long client_heard_times[HALO_PORT_MAXIMUM_NETWORK_MACHINES];
	long client_vote_times[HALO_PORT_MAXIMUM_NETWORK_MACHINES];
	/* host: no voting before this game time (set after a skip) */
	long cooldown_until;
	/* host: the vote passed and main_skip_cinematic was called once */
	boolean requested;
	/* host: the save the script makes as the cutscene becomes skippable is
	written (a skip reverts to it) */
	boolean skip_save_written;
};

/* what the skip vote and the presentation read of the host's game
(network_coop.c's host_presentation_inputs) */
struct coop_presentation_inputs
{
	/* the client machines, by index */
	long machine_indices[HALO_PORT_MAXIMUM_NETWORK_MACHINES];
	short machine_count;
	/* a cinematic in progress that can be skipped; the map saving; a local
	player */
	boolean skippable;
	boolean saving_map;
	boolean local_player;
	boolean cinematic;
	boolean letterbox;
	boolean fading_out;
	/* the camera of local player 0 (observer_get_camera), if any */
	boolean camera_valid;
	boolean camera_scripted;
	boolean input_disabled;
	boolean scripted_shake;
	boolean players_vitality_set;
	boolean show_hud;
	boolean show_help_text;
	short fade_ticks;
	long fade_start_time;
	real_rgb_color fade_color;
	real_point3d camera_position;
	real_vector3d camera_forward;
	real_vector3d camera_up;
	real camera_field_of_view;
	long camera_object_index;
	/* objects_get_activating_cluster_index (NONE for none) */
	short activating_cluster;
	short pad;
	real players_maximum_body_vitality;
	real players_maximum_shield_vitality;
	unsigned long ally_teams[GAME_ALLEGIANCE_BITVECTOR_LONGS];
	unsigned long friendly_teams[GAME_ALLEGIANCE_BITVECTOR_LONGS];
	long unit_hud_flags;
	long weapon_hud_flags;
	struct hud_timer_state timer;
};

/* host: the client machines it had last tick. On the tick a new one
appears, and every OBJECT_REFRESH_TICKS, the state a client that joined
since would lack is sent again (refresh), and on the join every device that
has moved since the map loaded too (joined). */
struct coop_host_resend
{
	long machines[HALO_PORT_MAXIMUM_NETWORK_MACHINES];
	short machine_count;
	boolean joined;
	boolean refresh;
};

/* a player's unit (NONE for none) and its maximum vitality */
struct coop_player_unit
{
	long unit_index;
	real maximum_body;
	real maximum_shield;
};

/* host: the client machines it has this tick (distributed_client_machines),
and each player's unit, by the player's absolute index, as the tick left it
(what players_vitality_keep reads: gathered before the step, since its calls
change the units) */
struct coop_resend_inputs
{
	long machines[HALO_PORT_MAXIMUM_NETWORK_MACHINES];
	short machine_count;
	short player_count;
	struct coop_player_unit player_units[HALO_PORT_MAXIMUM_NETWORK_PLAYERS];
};

/* The players' maximum vitality, once the scripts have set a player's: on
the host as they set it, on a client as the host's presentation has it.
Every player's unit is kept at it, a new one included. */
struct coop_players_vitality
{
	boolean set;
	real maximum_body;
	real maximum_shield;
};

/* client: the host's snap count last seen for each of its groups */
struct coop_client_devices
{
	byte snaps[COOP_MAXIMUM_DEVICE_GROUPS];
	boolean seen[COOP_MAXIMUM_DEVICE_GROUPS];
};

/* client: the script sounds (impulse ones: dialogue) it played lately,
which a skipped cutscene stops */
#define COOP_CLIENT_SCRIPT_SOUND_COUNT 16

struct coop_client_script_sounds
{
	long definition_indices[COOP_CLIENT_SCRIPT_SOUND_COUNT];
	short next;
};

#endif
