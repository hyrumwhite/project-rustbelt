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

#endif
