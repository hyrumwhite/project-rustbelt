/*
BEVY_BRIDGE.C

Shares each rendered frame's world with a Bevy renderer (bevy-rt-test's
halo_rt, which draws it with ray tracing): the first player's camera and
every object's node matrices, interpolated as this frame draws them
(render_interpolation.c). The layout is bevy_bridge_protocol.h's.

Off unless the game.bevy_bridge setting is on (HALO_BEVY_BRIDGE=1); then
render_frame calls bevy_bridge_export_frame once a frame. The shared memory
comes from the platform layer (posix_bevy_bridge.c, win32_bevy_bridge.c).
*/

/* ---------- headers */

#include "cseries.h"
#include "camera/director.h"
#include "game/game.h"
#include "game/players.h"
#include "objects/objects.h"
#include "render/render_cameras.h"
#include "render/render_cameras_internal.h"
#include "scenario/scenario.h"
#include "tag_files/tag_groups.h"
#include "bevy_bridge.h"
#include "bevy_bridge_protocol.h"

#include <stdlib.h>
#include <string.h>

/* port/linux/src/port_config.c */
int config_boolean(const char *name);
/* render_interpolation.c: this frame's blended node matrices, or NULL */
real_matrix4x3 *render_interpolation_object_node_matrices(long object_index);

/* ---------- globals */

static struct bevy_bridge_shared *bevy_bridge_shared;
static boolean bevy_bridge_failed;
static unsigned long bevy_bridge_frame_index;

/* models drawn outside the object list since the last frame (the
first-person weapon and hands), shared with the next */
#define MAXIMUM_RECORDED_MODELS 4
#define MAXIMUM_RECORDED_MODEL_NODES 64
static struct
{
	long model_index;
	short node_count;
	real_matrix4x3 nodes[MAXIMUM_RECORDED_MODEL_NODES];
} bevy_bridge_recorded[MAXIMUM_RECORDED_MODELS];
static short bevy_bridge_recorded_count;

/* the overlay buffer being filled, or NONE */
static long bevy_bridge_overlay_buffer = NONE;

/* ---------- private code */

static struct bevy_bridge_shared *bevy_bridge_get(void)
{
	if (bevy_bridge_shared || bevy_bridge_failed)
		return bevy_bridge_shared;
	if (!config_boolean("game.bevy_bridge"))
	{
		bevy_bridge_failed = TRUE;
		return NULL;
	}
	bevy_bridge_shared = (struct bevy_bridge_shared *)bevy_bridge_platform_map(sizeof(struct bevy_bridge_shared));
	if (!bevy_bridge_shared)
	{
		bevy_bridge_failed = TRUE;
		return NULL;
	}
	bevy_bridge_shared->size = sizeof(struct bevy_bridge_shared);
	bevy_bridge_shared->version = BEVY_BRIDGE_VERSION;
	bevy_bridge_shared->writer_process_id = bevy_bridge_platform_process_id();
	bevy_bridge_shared->latest_frame = 0;
	bevy_bridge_shared->overlay_enabled = config_boolean("game.bevy_bridge_overlay") != 0;
	/* last, so a reader that sees the magic sees the rest */
	__atomic_store_n(&bevy_bridge_shared->magic, BEVY_BRIDGE_MAGIC, __ATOMIC_RELEASE);
	return bevy_bridge_shared;
}

/* the local player's unit, when the camera is its first-person view
(render_objects.c, object_is_first_person_camera) */
static long bevy_bridge_first_person_unit(short local_player_index)
{
	long player_index = local_player_get_player_index(local_player_index);

	if (player_index == NONE ||
		director_get_perspective(local_player_index) != _director_perspective_first_person)
	{
		return NONE;
	}
	return player_get(player_index)->unit_index;
}

/* an object the game does not draw: invisible, or the first-person unit or
something attached to it */
static unsigned long bevy_bridge_object_flags(long object_index, long first_person_unit)
{
	struct object_datum *object = object_get(object_index);
	unsigned long flags = 0;
	long ancestor;

	if (TEST_FLAG(object->object.flags, _object_invisible_bit))
		SET_FLAG(flags, _bevy_bridge_object_hidden_bit, TRUE);
	if (object_index == first_person_unit)
		SET_FLAG(flags, _bevy_bridge_object_first_person_bit, TRUE);
	for (ancestor = object_index; ancestor != NONE; ancestor = object_get(ancestor)->object.parent_object_index)
	{
		if (ancestor == first_person_unit)
		{
			SET_FLAG(flags, _bevy_bridge_object_hidden_bit, TRUE);
			break;
		}
	}
	return flags;
}

static void bevy_bridge_vector(float *out, real_vector3d const *in)
{
	out[0] = in->i;
	out[1] = in->j;
	out[2] = in->k;
}

static void bevy_bridge_point(float *out, real_point3d const *in)
{
	out[0] = in->x;
	out[1] = in->y;
	out[2] = in->z;
}

static void bevy_bridge_write_node(struct bevy_bridge_node *node, real_matrix4x3 const *matrix)
{
	node->scale = matrix->scale;
	bevy_bridge_vector(node->forward, &matrix->forward);
	bevy_bridge_vector(node->left, &matrix->left);
	bevy_bridge_vector(node->up, &matrix->up);
	bevy_bridge_point(node->position, &matrix->position);
}

/* ---------- public code */

void bevy_bridge_record_model(
	long model_index,
	struct real_matrix4x3 const *node_matrices,
	short node_count)
{
	if (!bevy_bridge_shared || model_index == NONE || bevy_bridge_recorded_count >= MAXIMUM_RECORDED_MODELS)
		return;
	node_count = (short)MIN(node_count, MAXIMUM_RECORDED_MODEL_NODES);
	bevy_bridge_recorded[bevy_bridge_recorded_count].model_index = model_index;
	bevy_bridge_recorded[bevy_bridge_recorded_count].node_count = node_count;
	memcpy(bevy_bridge_recorded[bevy_bridge_recorded_count].nodes, node_matrices, node_count * sizeof(real_matrix4x3));
	bevy_bridge_recorded_count++;
}

void bevy_bridge_export_frame(
	struct render_window const *windows,
	short window_count)
{
	struct bevy_bridge_shared *shared = bevy_bridge_get();
	struct bevy_bridge_frame *frame;
	unsigned long buffer;
	struct object_iterator iterator;
	struct object_datum *object;
	short window_index;
	long first_person_unit = NONE;

	if (!shared)
		return;
	buffer = (shared->latest_frame + 1) % BEVY_BRIDGE_FRAMES;
	frame = &shared->frames[buffer];
	__atomic_store_n(&frame->sequence, frame->sequence | 1, __ATOMIC_RELAXED);
	__atomic_thread_fence(__ATOMIC_RELEASE);

	frame->frame_index = ++bevy_bridge_frame_index;
	frame->game_tick = (unsigned long)game_time_get();
	frame->structure_bsp_index = global_structure_bsp_index;
	memset(frame->scenario_name, 0, sizeof(frame->scenario_name));
	if (global_scenario_index != NONE)
		strncpy(frame->scenario_name, tag_get_name(global_scenario_index), sizeof(frame->scenario_name) - 1);

	/* the first player's view */
	frame->camera_valid = FALSE;
	for (window_index = 0; window_index < window_count; window_index++)
	{
		struct render_window const *window = &windows[window_index];

		if (window->console_window || window->local_player_index == NONE)
			continue;
		bevy_bridge_point(frame->camera.position, &window->render_camera.position);
		bevy_bridge_vector(frame->camera.forward, &window->render_camera.forward);
		bevy_bridge_vector(frame->camera.up, &window->render_camera.up);
		/* the field of view the picture covers: the frustum's, which (with
		the window's bounds inside the viewport's, as in cinematics) can be
		wider than the camera's */
		{
			real_rectangle2d bounds;

			render_camera_build_frustum_bounds(&window->render_camera, &bounds);
			frame->camera.vertical_field_of_view = 2.0f * arctangent(
				tangent(window->render_camera.vertical_field_of_view * 0.5f) * (bounds.y1 - bounds.y0) * 0.5f, 1.0f);
		}
		frame->camera_valid = TRUE;
		first_person_unit = bevy_bridge_first_person_unit(window->local_player_index);
		break;
	}

	frame->object_count = 0;
	frame->node_count = 0;
	if (global_scenario_index != NONE)
	{
		object_iterator_new(&iterator, _object_mask_all, 0);
		while ((object = (struct object_datum *)object_iterator_next(&iterator)) != NULL)
		{
			struct bevy_bridge_object *out;
			real_matrix4x3 const *nodes;
			short node_count = (short)(object->object.node_matrices.size / (short)sizeof(real_matrix4x3));
			short node_index;

			if (node_count <= 0 ||
				frame->object_count >= BEVY_BRIDGE_MAXIMUM_OBJECTS ||
				frame->node_count + node_count > BEVY_BRIDGE_MAXIMUM_NODES)
			{
				continue;
			}
			nodes = render_interpolation_object_node_matrices(iterator.index);
			if (!nodes)
				nodes = object_get_node_matrices(iterator.index);

			out = &frame->objects[frame->object_count++];
			out->object_index = (unsigned long)iterator.index;
			out->definition_index = (unsigned long)object->definition_index;
			out->first_node = frame->node_count;
			out->node_count = (unsigned short)node_count;
			out->type = object->object.type;
			out->flags = bevy_bridge_object_flags(iterator.index, first_person_unit);
			memcpy(out->region_permutations, object->object.region_permutations, sizeof(out->region_permutations));
			for (node_index = 0; node_index < node_count; node_index++)
				bevy_bridge_write_node(&frame->nodes[frame->node_count++], &nodes[node_index]);
		}
	}

	/* the models the last frame drew by themselves */
	{
		short recorded;

		for (recorded = 0; recorded < bevy_bridge_recorded_count; recorded++)
		{
			struct bevy_bridge_object *out;
			short node_index;
			short node_count = bevy_bridge_recorded[recorded].node_count;

			if (frame->object_count >= BEVY_BRIDGE_MAXIMUM_OBJECTS ||
				frame->node_count + node_count > BEVY_BRIDGE_MAXIMUM_NODES)
			{
				break;
			}
			out = &frame->objects[frame->object_count++];
			memset(out, 0, sizeof(*out));
			/* (a stable identity per slot, outside the datum indices' range) */
			out->object_index = 0xFFFF0000UL | (unsigned long)recorded;
			out->definition_index = (unsigned long)bevy_bridge_recorded[recorded].model_index;
			out->first_node = frame->node_count;
			out->node_count = (unsigned short)node_count;
			out->type = NONE;
			SET_FLAG(out->flags, _bevy_bridge_object_model_bit, TRUE);
			for (node_index = 0; node_index < node_count; node_index++)
				bevy_bridge_write_node(&frame->nodes[frame->node_count++], &bevy_bridge_recorded[recorded].nodes[node_index]);
		}
		bevy_bridge_recorded_count = 0;
	}

	__atomic_thread_fence(__ATOMIC_RELEASE);
	__atomic_store_n(&frame->sequence, (frame->sequence | 1) + 1, __ATOMIC_RELAXED);
	__atomic_store_n(&shared->latest_frame, buffer, __ATOMIC_RELEASE);
}

boolean bevy_bridge_overlay_enabled(void)
{
	struct bevy_bridge_shared *shared = bevy_bridge_get();

	return shared && shared->overlay_enabled;
}

unsigned char *bevy_bridge_overlay_begin(
	unsigned long width,
	unsigned long height)
{
	struct bevy_bridge_shared *shared = bevy_bridge_get();
	struct bevy_bridge_overlay *overlay;

	if (!shared || !shared->overlay_enabled || !width || !height ||
		width * height > BEVY_BRIDGE_OVERLAY_MAXIMUM_PIXELS)
	{
		return NULL;
	}
	bevy_bridge_overlay_buffer = (long)((shared->latest_overlay + 1) % BEVY_BRIDGE_OVERLAYS);
	overlay = &shared->overlays[bevy_bridge_overlay_buffer];
	__atomic_store_n(&overlay->sequence, overlay->sequence | 1, __ATOMIC_RELAXED);
	__atomic_thread_fence(__ATOMIC_RELEASE);
	overlay->frame_index = bevy_bridge_frame_index;
	overlay->width = width;
	overlay->height = height;
	return shared->overlay_pixels[bevy_bridge_overlay_buffer];
}

void bevy_bridge_overlay_commit(void)
{
	struct bevy_bridge_shared *shared = bevy_bridge_shared;
	struct bevy_bridge_overlay *overlay;

	if (!shared || bevy_bridge_overlay_buffer == NONE)
		return;
	overlay = &shared->overlays[bevy_bridge_overlay_buffer];
	__atomic_thread_fence(__ATOMIC_RELEASE);
	__atomic_store_n(&overlay->sequence, (overlay->sequence | 1) + 1, __ATOMIC_RELAXED);
	__atomic_store_n(&shared->latest_overlay, (unsigned long)bevy_bridge_overlay_buffer, __ATOMIC_RELEASE);
	bevy_bridge_overlay_buffer = NONE;
}
