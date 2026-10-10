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
#include "units/units.h"
#include "items/projectiles.h"
#include "physics/breakable_surfaces.h"
#include "units/bipeds.h"
#include "devices/device_definitions.h"
#include "devices/devices.h"
#include "objects/object_lights.h"
#include "physics/collision_bsp.h"
#include "physics/collisions.h"
#include "physics/point_physics.h"
#include "cache/cache_files.h"
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
const char *config_string(const char *name);
/* port/linux/src/platform.h */
void platform_log(const char *format, ...);
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
	real_rgb_color change_colors[4];
	real active_camouflage;
} bevy_bridge_recorded[MAXIMUM_RECORDED_MODELS];
static short bevy_bridge_recorded_count;

/* whether the renderer's camera is the listener (for the log) */
static boolean bevy_bridge_listening;

/* the overlay buffer being filled, or NONE */
static long bevy_bridge_overlay_buffer = NONE;

/* ---------- private code */

/* FNV-1a over 32-bit indices */
static unsigned long bevy_bridge_checksum(unsigned long hash, long const *indices, long count)
{
	long index;
	short byte_index;

	for (index = 0; index < count; index++)
	{
		unsigned long value = (unsigned long)indices[index];

		for (byte_index = 0; byte_index < 4; byte_index++)
		{
			hash ^= (value >> (8 * byte_index)) & 0xFF;
			hash *= 16777619UL;
		}
	}
	return hash;
}

/* the map's first point physics with structure collisions (preferring one
that collides with water too), NONE for none */
static long bevy_bridge_point_physics(void)
{
	static long scenario = NONE - 1;
	static long chosen = NONE;
	struct tag_iterator iterator;
	long tag_index;

	if (scenario == global_scenario_index)
		return chosen;
	scenario = global_scenario_index;
	chosen = NONE;
	tag_iterator_new(&iterator, POINT_PHYSICS_DEFINITION_TAG);
	for (tag_index = tag_iterator_next(&iterator); tag_index != NONE; tag_index = tag_iterator_next(&iterator))
	{
		struct point_physics_definition *definition = point_physics_definition_get(tag_index);

		if (TEST_FLAG(definition->flags, _point_physics_structure_collisions_bit))
		{
			if (chosen == NONE)
				chosen = tag_index;
			if (TEST_FLAG(definition->flags, _point_physics_water_collisions_bit))
			{
				chosen = tag_index;
				break;
			}
		}
	}
	return chosen;
}

/* game.bevy_bridge_ray_tests: rays fanned out from the camera, tested
against the structure (a renderer checks its own collision code with them) */
static unsigned long bevy_bridge_ray_tests(
	struct bevy_bridge_ray_test *tests,
	real_point3d const *origin,
	real_vector3d const *forward,
	real_vector3d const *up)
{
	static int enabled = -1;
	struct collision_bsp *bsp;
	real_vector3d right;
	unsigned long count = 0;
	short row, column;

	if (enabled < 0)
		enabled = config_boolean("game.bevy_bridge_ray_tests") != 0;
	bsp = enabled ? global_collision_bsp_get() : NULL;
	if (!bsp)
		return 0;
	cross_product3d(forward, up, &right);
	for (row = 0; row < 4; row++)
	{
		for (column = 0; column < 4 && count < BEVY_BRIDGE_RAY_TESTS; column++)
		{
			struct bevy_bridge_ray_test *test = &tests[count++];
			struct collision_bsp_test_vector_result result;
			/* (a 4 by 4 fan, 200 world units long) */
			real a = -0.6f + 0.4f * column, b = -0.6f + 0.4f * row;
			real_vector3d vector;

			vector.i = (forward->i + right.i * a + up->i * b) * 200.0f;
			vector.j = (forward->j + right.j * a + up->j * b) * 200.0f;
			vector.k = (forward->k + right.k * a + up->k * b) * 200.0f;
			test->point[0] = origin->x;
			test->point[1] = origin->y;
			test->point[2] = origin->z;
			test->vector[0] = vector.i;
			test->vector[1] = vector.j;
			test->vector[2] = vector.k;
			test->hit = collision_bsp_test_vector(FLAG(_collision_test_front_facing_surfaces_bit), bsp, 0, NULL,
				origin, &vector, 1.0f, &result);
			test->t = test->hit ? result.t : 1.0f;
			test->surface_index = test->hit ? result.surface_index : NONE;
			{
				static struct collision_bsp_test_sphere_result sphere;
				real_point3d center;
				/* (half its radius short of where the ray hit: touching
				the geometry there) */
				real radius = (count & 1) ? 1.0f : 0.25f;
				real fraction = test->t - 0.5f * radius / magnitude3d(&vector);
				unsigned long checksum = 2166136261UL;

				center.x = origin->x + vector.i * fraction;
				center.y = origin->y + vector.j * fraction;
				center.z = origin->z + vector.k * fraction;
				test->sphere_center[0] = center.x;
				test->sphere_center[1] = center.y;
				test->sphere_center[2] = center.z;
				test->sphere_radius = radius;
				collision_bsp_test_sphere(bsp, 0, NULL, &center, test->sphere_radius, &sphere);
				test->sphere_counts[0] = (unsigned short)sphere.surface_count;
				test->sphere_counts[1] = (unsigned short)sphere.edge_count;
				test->sphere_counts[2] = (unsigned short)sphere.vertex_count;
				test->sphere_counts[3] = (unsigned short)sphere.leaf_count;
				checksum = bevy_bridge_checksum(checksum, sphere.surface_indices, sphere.surface_count);
				checksum = bevy_bridge_checksum(checksum, sphere.edge_indices, sphere.edge_count);
				checksum = bevy_bridge_checksum(checksum, sphere.vertex_indices, sphere.vertex_count);
				checksum = bevy_bridge_checksum(checksum, sphere.leaf_indices, sphere.leaf_count);
				test->sphere_checksum = checksum;
			}
			{
				static struct collision_bsp_test_pill_result pill;
				/* (half the full ray, half just past where it hit) */
				real length = (count & 4) ? 1.0f : test->t * 1.02f;

				test->pill_radius = (count & 2) ? 0.6f : 0.2f;
				test->pill_vector[0] = vector.i * length;
				test->pill_vector[1] = vector.j * length;
				test->pill_vector[2] = vector.k * length;
				{
					real_vector3d pill_vector;

					pill_vector.i = test->pill_vector[0];
					pill_vector.j = test->pill_vector[1];
					pill_vector.k = test->pill_vector[2];
					test->pill_hit = collision_bsp_test_pill(bsp, origin, &pill_vector, test->pill_radius, REAL_MAX, &pill);
				}
				test->pill_t = test->pill_hit ? pill.t : 0.0f;
				test->pill_normal[0] = test->pill_hit ? pill.plane.n.i : 0.0f;
				test->pill_normal[1] = test->pill_hit ? pill.plane.n.j : 0.0f;
				test->pill_normal[2] = test->pill_hit ? pill.plane.n.k : 0.0f;
				test->pill_surface_index = test->pill_hit ? pill.surface_index : NONE;
			}
			{
				struct collision_result collision;

				test->object_index = (unsigned long)NONE;
				test->object_node_index = NONE;
				test->object_bsp_index = NONE;
				test->object_t = 1.0f;
				test->object_surface_index = NONE;
				if (collision_test_vector(FLAG(_collision_test_front_facing_surfaces_bit) |
						FLAG(_collision_test_structure_bit) | FLAG(_collision_test_objects_bit) |
						_collision_test_objects_all_types_flags,
						origin, &vector, NONE, &collision))
				{
					test->object_index = collision.type == _collision_result_object ?
						(unsigned long)collision.object_index : (unsigned long)NONE;
					test->object_node_index = collision.type == _collision_result_object ? collision.node_index : NONE;
					test->object_bsp_index = collision.type == _collision_result_object ? collision.bsp_index : NONE;
					test->object_t = collision.t;
					test->object_surface_index = collision.surface_index;
				}
			}
			{
				long definition_index = bevy_bridge_point_physics();

				test->point_radius = (count & 2) ? 0.2f : 0.05f;
				test->point_underwater = (count & 3) == 3;
				test->point_result_flags = 0;
				test->point_collisions = 0;
				if (definition_index != NONE)
				{
					struct point_physics_definition const *definition = point_physics_definition_get(definition_index);
					struct location location = { NONE, NONE };
					real_point3d position = *origin;
					real_vector3d velocity = vector;
					unsigned long update_flags = FLAG(_point_physics_ignore_position_bit) |
						(test->point_underwater ? FLAG(_point_physics_ignore_position_under_water_bit) : 0);
					short step;

					normalize3d(&velocity);
					scale_vector3d(&velocity, 15.0f, &velocity);
					for (step = 0; step < BEVY_BRIDGE_POINT_PHYSICS_STEPS; step++)
					{
						unsigned long result = point_physics_update(update_flags, definition, &location, NONE,
							&position, &velocity, NULL, NULL, NULL, test->point_radius, 1.0f / 30.0f);

						test->point_result_flags |= result;
						if (result & (FLAG(_point_physics_collided_with_structure_bit) | FLAG(_point_physics_collided_with_water_bit)))
							test->point_collisions++;
					}
					test->point_position[0] = position.x;
					test->point_position[1] = position.y;
					test->point_position[2] = position.z;
					test->point_velocity[0] = velocity.i;
					test->point_velocity[1] = velocity.j;
					test->point_velocity[2] = velocity.k;
				}
			}
		}
	}
	return count;
}

/* an object's active camouflage: its own as a unit, or that of the unit it
is attached to (a held weapon) */
static float bevy_bridge_camouflage(long object_index)
{
	short depth;

	for (depth = 0; depth < 4 && object_index != NONE; depth++)
	{
		struct object_datum *object = (struct object_datum *)object_get(object_index);

		if (TEST_FLAG(_object_mask_unit, object->object.type))
			return unit_get(object_index)->unit.active_camouflage;
		object_index = object->object.parent_object_index;
	}
	return 0.0f;
}

/* game.bevy_bridge_name, or the default where it is empty or holds more
than letters, digits, '-' and '_' */
static char const *bevy_bridge_name(void)
{
	char const *name = config_string("game.bevy_bridge_name");
	size_t length = name ? strlen(name) : 0;
	size_t index;

	if (!length || length > BEVY_BRIDGE_MAXIMUM_NAME_LENGTH)
		return BEVY_BRIDGE_DEFAULT_NAME;
	for (index = 0; index < length; index++)
	{
		char c = name[index];

		if (!((c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') || (c >= '0' && c <= '9') || c == '-' || c == '_'))
			return BEVY_BRIDGE_DEFAULT_NAME;
	}
	return name;
}

static struct bevy_bridge_shared *bevy_bridge_get(void)
{
	if (bevy_bridge_shared || bevy_bridge_failed)
		return bevy_bridge_shared;
	if (!config_boolean("game.bevy_bridge"))
	{
		bevy_bridge_failed = TRUE;
		return NULL;
	}
	bevy_bridge_shared = (struct bevy_bridge_shared *)bevy_bridge_platform_map(bevy_bridge_name(), sizeof(struct bevy_bridge_shared));
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
	real_matrix4x3 const *node_matrices,
	short node_count,
	real_rgb_color const *change_colors,
	real active_camouflage)
{
	if (!bevy_bridge_shared || model_index == NONE || bevy_bridge_recorded_count >= MAXIMUM_RECORDED_MODELS)
		return;
	node_count = (short)MIN(node_count, MAXIMUM_RECORDED_MODEL_NODES);
	bevy_bridge_recorded[bevy_bridge_recorded_count].model_index = model_index;
	bevy_bridge_recorded[bevy_bridge_recorded_count].node_count = node_count;
	memcpy(bevy_bridge_recorded[bevy_bridge_recorded_count].nodes, node_matrices, node_count * sizeof(real_matrix4x3));
	{
		short color_index;

		/* (white without colours: the model as it is) */
		for (color_index = 0; color_index < 4; color_index++)
		{
			bevy_bridge_recorded[bevy_bridge_recorded_count].change_colors[color_index] =
				change_colors ? change_colors[color_index] : *global_real_rgb_white;
		}
	}
	bevy_bridge_recorded[bevy_bridge_recorded_count].active_camouflage = active_camouflage;
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
	frame->ray_test_count = 0;
	frame->point_physics_definition = (unsigned long)bevy_bridge_point_physics();
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
		frame->ray_test_count = bevy_bridge_ray_tests(frame->ray_tests, &window->render_camera.position,
			&window->render_camera.forward, &window->render_camera.up);
		break;
	}

	/* the dynamic lights */
	{
		struct lights_port_light lights[BEVY_BRIDGE_MAXIMUM_LIGHTS];
		short count = global_scenario_index != NONE ? lights_port_list(lights, BEVY_BRIDGE_MAXIMUM_LIGHTS) : 0;
		short index;

		for (index = 0; index < count; index++)
		{
			struct bevy_bridge_light *out = &frame->lights[index];

			bevy_bridge_point(out->position, &lights[index].position);
			bevy_bridge_vector(out->forward, &lights[index].forward);
			out->color[0] = lights[index].color.red;
			out->color[1] = lights[index].color.green;
			out->color[2] = lights[index].color.blue;
			out->radius = lights[index].radius;
			out->cutoff_angle = lights[index].cutoff_angle;
		}
		frame->light_count = (unsigned long)count;
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
			out->active_camouflage = bevy_bridge_camouflage(iterator.index);
			bevy_bridge_point(out->bounding_center, &object->object.bounding_sphere_center);
			out->bounding_radius = object->object.bounding_sphere_radius;
			out->object_flags = object->object.flags;
			out->parent_index = (unsigned long)object->object.parent_object_index;
			out->location_cluster = object->object.location.cluster_index;
			out->pad = 0;
			memcpy(out->region_permutations, object->object.region_permutations, sizeof(out->region_permutations));
			{
				short color_index;

				for (color_index = 0; color_index < 4; color_index++)
				{
					out->change_colors[color_index][0] = object->object.outgoing_change_colors[color_index].red;
					out->change_colors[color_index][1] = object->object.outgoing_change_colors[color_index].green;
					out->change_colors[color_index][2] = object->object.outgoing_change_colors[color_index].blue;
				}
			}
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
			out->active_camouflage = bevy_bridge_recorded[recorded].active_camouflage;
			{
				short color_index;

				for (color_index = 0; color_index < 4; color_index++)
				{
					out->change_colors[color_index][0] = bevy_bridge_recorded[recorded].change_colors[color_index].red;
					out->change_colors[color_index][1] = bevy_bridge_recorded[recorded].change_colors[color_index].green;
					out->change_colors[color_index][2] = bevy_bridge_recorded[recorded].change_colors[color_index].blue;
				}
			}
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

boolean bevy_bridge_listener(
	real_matrix4x3 *matrix)
{
	struct bevy_bridge_shared *shared = bevy_bridge_shared;
	struct bevy_bridge_input input;
	real_point3d position;
	real_vector3d forward, up;

	if (!shared)
		return FALSE;
	/* (the renderer writes the block whole; a torn read is a frame's
	listener between two cameras, which the next frame corrects) */
	__atomic_thread_fence(__ATOMIC_ACQUIRE);
	input = *(struct bevy_bridge_input volatile const *)&shared->input;
	if ((input.listener_valid != 0) != bevy_bridge_listening)
	{
		bevy_bridge_listening = input.listener_valid != 0;
		platform_log("bevy bridge: the first player hears from %s", bevy_bridge_listening ?
			"the renderer's camera" : "the game's camera");
	}
	if (!input.listener_valid)
		return FALSE;
	position.x = input.listener_position[0];
	position.y = input.listener_position[1];
	position.z = input.listener_position[2];
	forward.i = input.listener_forward[0];
	forward.j = input.listener_forward[1];
	forward.k = input.listener_forward[2];
	up.i = input.listener_up[0];
	up.j = input.listener_up[1];
	up.k = input.listener_up[2];
	if (normalize3d(&forward) == 0.0f || normalize3d(&up) == 0.0f)
		return FALSE;
	matrix4x3_from_point_and_vectors(matrix, &position, &forward, &up);
	return TRUE;
}

/* ---------- projectile updates */

/* random_math.c: its direction table */
short random_direction_table_get(real_vector3d const **table);
unsigned long get_random_seed(void);

static struct
{
	boolean active;
	struct bevy_bridge_projectile_record record;
} bevy_bridge_projectile;

static boolean bevy_bridge_projectile_recording(void)
{
	static int enabled = -1;

	if (enabled < 0)
		enabled = config_boolean("game.bevy_bridge_ray_tests") != 0;
	return enabled && bevy_bridge_shared;
}

static void bevy_bridge_projectile_state(long projectile_index, struct bevy_bridge_projectile_state *state)
{
	struct projectile_datum *projectile = (struct projectile_datum *)object_get(projectile_index);

	bevy_bridge_point(state->position, &projectile->object.position);
	bevy_bridge_vector(state->velocity, &projectile->object.translational_velocity);
	state->projectile_flags = projectile->projectile.flags;
	state->object_flags = projectile->object.flags;
	state->action = projectile->projectile.action;
	state->hit_material_type = projectile->projectile.hit_material_type;
	state->ignore_object_index = projectile->projectile.ignore_object_index;
	state->target_object_index = projectile->projectile.target_object_index;
	state->parent_object_index = projectile->object.parent_object_index;
	state->detonation_timer = projectile->projectile.detonation_timer;
	state->detonation_timer_delta = projectile->projectile.detonation_timer_delta;
	state->arming_time = projectile->projectile.arming_time;
	state->arming_time_delta = projectile->projectile.arming_time_delta;
	state->odometer = projectile->projectile.odometer;
	state->deceleration_timer = projectile->projectile.deceleration_timer;
	state->deceleration_timer_delta = projectile->projectile.deceleration_timer_delta;
	state->deceleration = projectile->projectile.deceleration;
	state->maximum_damage_distance = projectile->projectile.maximum_damage_distance;
	state->location_leaf = projectile->object.location.leaf_index;
	state->location_cluster = projectile->object.location.cluster_index;
	state->pad = 0;
}

void bevy_bridge_projectile_begin(long projectile_index)
{
	struct bevy_bridge_projectile_record *record = &bevy_bridge_projectile.record;

	if (!bevy_bridge_projectile_recording())
		return;
	/* (the random directions, once) */
	if (!bevy_bridge_shared->random_direction_count)
	{
		real_vector3d const *table;
		short count = random_direction_table_get(&table);
		short index;

		for (index = 0; index < count && index < BEVY_BRIDGE_RANDOM_DIRECTIONS; index++)
			bevy_bridge_vector(bevy_bridge_shared->random_directions[index], &table[index]);
		__atomic_store_n(&bevy_bridge_shared->random_direction_count, (unsigned long)index, __ATOMIC_RELEASE);
	}
	memset(record, 0, sizeof(*record));
	record->projectile_index = (unsigned long)projectile_index;
	record->definition_index = (unsigned long)object_get(projectile_index)->definition_index;
	record->structure_bsp_index = global_structure_bsp_index;
	record->random_seed = get_random_seed();
	bevy_bridge_projectile_state(projectile_index, &record->before);
	bevy_bridge_projectile.active = TRUE;
}

void bevy_bridge_projectile_collision(void)
{
	struct bevy_bridge_projectile_record *record = &bevy_bridge_projectile.record;

	if (!bevy_bridge_projectile.active)
		return;
	if (record->collision_count < 4)
		record->collision_seeds[record->collision_count] = get_random_seed();
	record->collision_count++;
}

void bevy_bridge_projectile_touched_object(void)
{
	if (bevy_bridge_projectile.active)
		bevy_bridge_projectile.record.touched_object = TRUE;
}

void bevy_bridge_projectile_end(long projectile_index)
{
	struct bevy_bridge_projectile_record *record = &bevy_bridge_projectile.record;
	unsigned long count;

	if (!bevy_bridge_projectile.active)
		return;
	bevy_bridge_projectile.active = FALSE;
	bevy_bridge_projectile_state(projectile_index, &record->after);
	count = bevy_bridge_shared->projectile_record_count;
	record->sequence = count;
	bevy_bridge_shared->projectile_records[count % BEVY_BRIDGE_PROJECTILE_RECORDS] = *record;
	__atomic_store_n(&bevy_bridge_shared->projectile_record_count, count + 1, __ATOMIC_RELEASE);
}

/* ---------- biped physics updates */

static struct
{
	boolean active;
	struct bevy_bridge_biped_record record;
} bevy_bridge_biped;

void bevy_bridge_biped_physics_begin(long biped_index, void const *physics, unsigned long size,
	int stand_test, float stand_height, float stand_radius)
{
	struct bevy_bridge_biped_record *record = &bevy_bridge_biped.record;

	if (!bevy_bridge_projectile_recording())
		return;
	memset(record, 0, sizeof(*record));
	record->biped_index = (unsigned long)biped_index;
	record->structure_bsp_index = global_structure_bsp_index;
	record->stand_test = stand_test ? 1 : 0;
	record->stand_height = stand_height;
	record->stand_radius = stand_radius;
	memcpy(record->breakable_surfaces, breakable_surface_flags_get(), sizeof(record->breakable_surfaces));
	memcpy(&record->before, physics, size < sizeof(record->before) ? size : sizeof(record->before));
	record->first_feature_object = bevy_bridge_shared->feature_object_count;
	record->feature_object_count = 0;
	record->feature_calls = 0;
	bevy_bridge_biped.active = TRUE;
}

void bevy_bridge_biped_physics_end(void const *physics, unsigned long size)
{
	struct bevy_bridge_biped_record *record = &bevy_bridge_biped.record;
	unsigned long count;

	if (!bevy_bridge_biped.active)
		return;
	bevy_bridge_biped.active = FALSE;
	memcpy(&record->after, physics, size < sizeof(record->after) ? size : sizeof(record->after));
	count = bevy_bridge_shared->biped_record_count;
	record->sequence = count;
	bevy_bridge_shared->biped_records[count % BEVY_BRIDGE_BIPED_RECORDS] = *record;
	__atomic_store_n(&bevy_bridge_shared->biped_record_count, count + 1, __ATOMIC_RELEASE);
}

void bevy_bridge_features_call(void)
{
	if (bevy_bridge_biped.active)
		bevy_bridge_biped.record.feature_calls++;
}

void bevy_bridge_feature_object(long object_index)
{
	struct bevy_bridge_biped_record *record = &bevy_bridge_biped.record;
	struct bevy_bridge_feature_object *entry;
	struct object_datum *object;
	unsigned long count;
	long node_count;
	long node_index;
	real_matrix4x3 const *matrices;

	if (!bevy_bridge_biped.active)
		return;
	object = object_get(object_index);
	count = bevy_bridge_shared->feature_object_count;
	entry = &bevy_bridge_shared->feature_objects[count % BEVY_BRIDGE_FEATURE_OBJECTS];
	memset(entry, 0, sizeof(*entry));
	entry->object_index = (unsigned long)object_index;
	entry->definition_index = (unsigned long)object->definition_index;
	entry->call = record->feature_calls ? record->feature_calls - 1 : 0;
	entry->type = object->object.type;
	entry->dead = TEST_FLAG(object->object.damage_flags, _object_dead_bit) ? 1 : 0;
	entry->flags = object->object.flags;
	bevy_bridge_point(entry->bounding_center, &object->object.bounding_sphere_center);
	entry->bounding_radius = object->object.bounding_sphere_radius;
	entry->parent_object_index = object->object.parent_object_index;
	bevy_bridge_vector(entry->translational_velocity, &object->object.translational_velocity);
	entry->player_index = NONE;
	entry->parent_seat_index = NONE;
	if (object->object.type == _object_type_machine)
	{
		struct machine_definition *machine_definition = machine_definition_get(object->definition_index);

		entry->elevator = TEST_FLAG(machine_definition->machine.flags, _machine_never_appears_locked_bit) &&
			machine_definition->machine.elevator_node_index != NONE;
	}
	if (object->object.type == _object_type_biped)
	{
		struct biped_datum *biped = biped_get(object_index);
		real_point3d base;

		entry->biped_passes_through = TEST_FLAG(biped->biped.flags, _biped_movement_passes_through_bipeds_bit) ? 1 : 0;
		entry->player_index = biped->unit.player_index;
		entry->parent_seat_index = biped->unit.parent_seat_index;
		biped_get_physics_pill(object_index, &base, &entry->pill_height, &entry->pill_width);
		bevy_bridge_point(entry->pill_base, &base);
	}
	memcpy(entry->region_permutations, object->object.region_permutations, sizeof(entry->region_permutations));
	node_count = object->object.node_matrices.size / (long)sizeof(real_matrix4x3);
	if (node_count > BEVY_BRIDGE_FEATURE_OBJECT_MAXIMUM_NODES)
		node_count = BEVY_BRIDGE_FEATURE_OBJECT_MAXIMUM_NODES;
	if (node_count < 0)
		node_count = 0;
	matrices = node_count ? object_get_node_matrices(object_index) : NULL;
	entry->first_matrix = bevy_bridge_shared->feature_matrix_count;
	entry->node_count = (unsigned long)node_count;
	for (node_index = 0; node_index < node_count; node_index++)
	{
		float *out = bevy_bridge_shared->feature_matrices[(entry->first_matrix + node_index) % BEVY_BRIDGE_FEATURE_MATRICES];

		memcpy(out, &matrices[node_index], sizeof(float) * 13);
	}
	__atomic_store_n(&bevy_bridge_shared->feature_matrix_count, entry->first_matrix + node_count, __ATOMIC_RELEASE);
	entry->sequence = count;
	__atomic_store_n(&bevy_bridge_shared->feature_object_count, count + 1, __ATOMIC_RELEASE);
	record->feature_object_count++;
}

/* ---------- biped movement updates */

static struct
{
	boolean active;
	unsigned long physics_records;
	struct bevy_bridge_biped_moving_record record;
} bevy_bridge_biped_moving;

int bevy_bridge_biped_moving_recording(void)
{
	return bevy_bridge_projectile_recording();
}

static void bevy_bridge_biped_moving_state(long biped_index, char state_desired, int crouching,
	struct bevy_bridge_biped_moving_state *state)
{
	struct biped_datum *biped = biped_get(biped_index);

	bevy_bridge_point(state->position, &biped->object.position);
	bevy_bridge_vector(state->forward, &biped->object.forward);
	bevy_bridge_vector(state->up, &biped->object.up);
	bevy_bridge_vector(state->translational_velocity, &biped->object.translational_velocity);
	bevy_bridge_vector(state->angular_velocity, &biped->object.angular_velocity);
	state->object_flags = biped->object.flags;
	state->location_leaf = biped->object.location.leaf_index;
	state->location_cluster = biped->object.location.cluster_index;
	state->landing = biped->biped.landing;
	state->biped_flags = biped->biped.flags;
	state->landing_recovery_counter = biped->biped.landing_recovery_counter;
	state->landing_recovery_time = biped->biped.landing_recovery_time;
	state->elevator_ticks = biped->biped.elevator_ticks;
	state->bump_ticks = biped->biped.bump_ticks;
	state->jump_recovery_timer = biped->biped.jump_recovery_timer;
	state->airborne_ticks = biped->biped.airborne_ticks;
	state->animation_state_desired = state_desired;
	state->animation_crouching = crouching ? 1 : 0;
	state->elevator_object_index = biped->biped.elevator_object_index;
	state->support_surface_index = biped->biped.support_surface_index;
	state->pathfinding_surface_index = biped->biped.pathfinding_surface_index;
	bevy_bridge_point(state->pathfinding_point, &biped->biped.pathfinding_point);
	state->bump_object_index = biped->biped.bump_object_index;
	state->crouch = biped->biped.crouch;
	state->bank = biped->biped.bank;
	state->ground_plane[0] = biped->biped.ground_plane.n.i;
	state->ground_plane[1] = biped->biped.ground_plane.n.j;
	state->ground_plane[2] = biped->biped.ground_plane.n.k;
	state->ground_plane[3] = biped->biped.ground_plane.d;
}

void bevy_bridge_biped_moving_begin(long biped_index, char state_desired, int crouching,
	float const *aiming_vector, float difficulty_infection_forms, float speed_multiplier,
	int cinematic, int double_speed, int coop_player_collisions, int actor_leaping, int recorded_animation)
{
	struct bevy_bridge_biped_moving_record *record = &bevy_bridge_biped_moving.record;
	struct biped_datum *biped;

	if (!bevy_bridge_projectile_recording())
		return;
	biped = biped_get(biped_index);
	memset(record, 0, sizeof(*record));
	record->biped_index = (unsigned long)biped_index;
	record->definition_index = (unsigned long)biped->definition_index;
	record->structure_bsp_index = global_structure_bsp_index;
	record->physics_record = NONE;
	record->dead = TEST_FLAG(biped->object.damage_flags, _object_dead_bit) ? 1 : 0;
	record->ignore_translation = TEST_FLAG(biped->unit.animation.flags, _unit_animation_ignore_translation_bit) ? 1 : 0;
	record->suspended = TEST_FLAG(biped->unit.flags, _unit_suspended_bit) ? 1 : 0;
	record->control_flags = biped->unit.control_flags;
	record->unit_animation_state = biped->unit.animation.state;
	record->base_seat_index = biped->unit.animation.base_seat_index;
	record->aiming_speed = biped->unit.aiming_speed;
	record->actor = (biped->unit.actor_index != NONE || biped->unit.swarm_actor_index != NONE) ? 1 : 0;
	record->player_index = biped->unit.player_index;
	bevy_bridge_vector(record->throttle, &biped->unit.throttle);
	record->body_stun = biped->unit.body_stun;
	record->recent_body_damage = biped->object.recent_body_damage;
	bevy_bridge_vector(record->desired_facing_vector, &biped->unit.desired_facing_vector);
	memcpy(record->aiming_vector, aiming_vector, sizeof(record->aiming_vector));
	bevy_bridge_point(record->bounding_center, &biped->object.bounding_sphere_center);
	record->animation_graph_index = biped->object.animation.animation_graph_index;
	record->animation_index = biped->object.animation.state.index;
	record->animation_frame_index = biped->object.animation.state.frame_index;
	record->difficulty_infection_forms = difficulty_infection_forms;
	record->speed_multiplier = speed_multiplier;
	record->cinematic = cinematic ? 1 : 0;
	record->double_speed = double_speed ? 1 : 0;
	record->coop_player_collisions = coop_player_collisions ? 1 : 0;
	record->actor_leaping = actor_leaping ? 1 : 0;
	record->recorded_animation = recorded_animation ? 1 : 0;
	bevy_bridge_biped_moving_state(biped_index, state_desired, crouching, &record->before);
	bevy_bridge_biped_moving.physics_records = bevy_bridge_shared->biped_record_count;
	bevy_bridge_biped_moving.active = TRUE;
}

void bevy_bridge_biped_moving_end(long biped_index, char state_desired, int crouching)
{
	struct bevy_bridge_biped_moving_record *record = &bevy_bridge_biped_moving.record;
	unsigned long count;

	if (!bevy_bridge_biped_moving.active)
		return;
	bevy_bridge_biped_moving.active = FALSE;
	if (bevy_bridge_shared->biped_record_count == bevy_bridge_biped_moving.physics_records + 1)
		record->physics_record = (long)bevy_bridge_biped_moving.physics_records;
	bevy_bridge_biped_moving_state(biped_index, state_desired, crouching, &record->after);
	count = bevy_bridge_shared->biped_moving_record_count;
	record->sequence = count;
	bevy_bridge_shared->biped_moving_records[count % BEVY_BRIDGE_BIPED_MOVING_RECORDS] = *record;
	__atomic_store_n(&bevy_bridge_shared->biped_moving_record_count, count + 1, __ATOMIC_RELEASE);
}

void bevy_bridge_biped_actor_jump(int jumped, float const *velocity)
{
	if (!bevy_bridge_biped_moving.active)
		return;
	bevy_bridge_biped_moving.record.actor_jump_asked = 1;
	bevy_bridge_biped_moving.record.actor_jumped = jumped ? 1 : 0;
	memcpy(bevy_bridge_biped_moving.record.actor_jump_velocity, velocity, sizeof(float) * 3);
}

void bevy_bridge_biped_aborted_animation(void)
{
	if (bevy_bridge_biped_moving.active)
		bevy_bridge_biped_moving.record.aborted_animation = 1;
}
