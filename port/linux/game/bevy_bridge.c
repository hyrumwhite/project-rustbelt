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
#include "game/game_globals.h"
#include "game/players.h"
#include "objects/objects.h"
#include "units/units.h"
#include "items/projectiles.h"
#include "items/weapons.h"
#include "physics/breakable_surfaces.h"
#include "units/bipeds.h"
#include "units/vehicle_datum.h"
#include "physics/powered_mass_point_datum.h"
#include "physics/friction_datum.h"
#include "physics/mass_point_datum.h"
#include "devices/device_definitions.h"
#include "devices/devices.h"
#include "models/model_definitions.h"
#include "objects/object_lights.h"
#include "effects/contrails.h"
#include "physics/collision_bsp.h"
#include "physics/collisions.h"
#include "physics/point_physics.h"
#include "cache/cache_files.h"
#include "render/render_cameras.h"
#include "render/render_cameras_internal.h"
#include "scenario/scenario.h"
#include "tag_files/tag_groups.h"
#include "bitmaps/bitmaps.h"
#include "bitmaps/bitmap_group.h"
#include "bevy_bridge.h"
#include "bevy_bridge_protocol.h"

boolean unit_animation_enhanced(long unit_index); /* units.c */

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

/* models drawn outside the object list this frame (the first-person weapon
and hands), shared with it */
#define MAXIMUM_RECORDED_MODELS 4
#define MAXIMUM_RECORDED_MODEL_NODES 64
static struct
{
	long model_index;
	short node_count;
	real_matrix4x3 nodes[MAXIMUM_RECORDED_MODEL_NODES];
	real_rgb_color change_colors[4];
	real function_values[4];
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

static void bevy_bridge_vector(float *out, real_vector3d const *in);
static void bevy_bridge_point(float *out, real_point3d const *in);
/* the fog last noted (bevy_bridge_note_fog), for the next frame's export */
static struct bevy_bridge_fog bevy_bridge_fog;
/* ... and the fog screen, noted afresh each frame */
static struct bevy_bridge_fog_screen bevy_bridge_fog_screen;
/* ... and the sprites */
static unsigned long bevy_bridge_sprite_batch_count;
static unsigned long bevy_bridge_sprite_vertex_count;
static struct bevy_bridge_sprite_batch bevy_bridge_sprite_batches[BEVY_BRIDGE_SPRITE_BATCHES];
struct bevy_bridge_shared;
static void bevy_bridge_share_text_atlas(struct bevy_bridge_shared *shared);
/* the screen flash staged for the frame's export */
static unsigned long bevy_bridge_screen_flash_type;
static float bevy_bridge_screen_flash_intensity;
static float bevy_bridge_screen_flash_color[4];
static struct bevy_bridge_screen_effect bevy_bridge_screen_effect;
/* the screen quads staged for the frame's export */
static unsigned long bevy_bridge_screen_draw_count;
static unsigned long bevy_bridge_screen_vertex_count;
static struct bevy_bridge_screen_draw bevy_bridge_screen_draws[BEVY_BRIDGE_SCREEN_DRAWS];
static struct bevy_bridge_screen_vertex bevy_bridge_screen_vertices[BEVY_BRIDGE_SCREEN_VERTICES];
static struct bevy_bridge_sprite_vertex bevy_bridge_sprite_vertices[BEVY_BRIDGE_SPRITE_VERTICES];
/* ... and the decals drawn */
static unsigned long bevy_bridge_decal_count;
static unsigned long bevy_bridge_decal_vertex_count;
static struct bevy_bridge_decal bevy_bridge_decals[BEVY_BRIDGE_DECALS];
static struct bevy_bridge_decal_vertex bevy_bridge_decal_vertices[BEVY_BRIDGE_DECAL_VERTICES];

/* game.bevy_bridge_ray_tests: the vehicles' physics placement, and rays from
the camera at them tested with their physics shapes
(_collision_test_use_vehicle_physics_bit, which the game itself sets only
for debugging) */
static void bevy_bridge_vehicle_ray_tests(
	struct bevy_bridge_frame *frame,
	real_point3d const *origin)
{
	static int enabled = -1;
	struct object_iterator iterator;
	struct object_datum *object;

	frame->physics_vehicle_count = 0;
	frame->vehicle_ray_test_count = 0;
	if (enabled < 0)
		enabled = config_boolean("game.bevy_bridge_ray_tests") != 0;
	if (!enabled || !global_collision_bsp_get())
		return;
	object_iterator_new(&iterator, _object_mask_vehicle, 0);
	while ((object = (struct object_datum *)object_iterator_next(&iterator)) != NULL &&
		frame->physics_vehicle_count < BEVY_BRIDGE_PHYSICS_VEHICLES)
	{
		struct bevy_bridge_physics_vehicle *vehicle = &frame->physics_vehicles[frame->physics_vehicle_count++];
		real_point3d position;
		real_vector3d forward;
		real_vector3d up;
		short ray;

		vehicle->object_index = (unsigned long)iterator.index;
		object_get_origin(iterator.index, &position);
		object_get_orientation(iterator.index, &forward, &up);
		bevy_bridge_point(vehicle->origin, &position);
		bevy_bridge_vector(vehicle->forward, &forward);
		bevy_bridge_vector(vehicle->up, &up);
		for (ray = 0; ray < 2 && frame->vehicle_ray_test_count < BEVY_BRIDGE_VEHICLE_RAY_TESTS; ray++)
		{
			struct bevy_bridge_vehicle_ray_test *test = &frame->vehicle_ray_tests[frame->vehicle_ray_test_count++];
			struct collision_result collision;
			real_point3d target = object->object.bounding_sphere_center;
			real_vector3d vector;

			/* (through the centre, and half the radius above it; on past
			it as far again) */
			if (ray == 1)
				target.z += object->object.bounding_sphere_radius * 0.5f;
			vector.i = (target.x - origin->x) * 2.0f;
			vector.j = (target.y - origin->y) * 2.0f;
			vector.k = (target.z - origin->z) * 2.0f;
			bevy_bridge_point(test->point, origin);
			bevy_bridge_vector(test->vector, &vector);
			test->type = NONE;
			test->node_index = NONE;
			test->object_index = (unsigned long)NONE;
			test->t = 1.0f;
			memset(test->plane, 0, sizeof(test->plane));
			test->surface_index = NONE;
			if (collision_test_vector(FLAG(_collision_test_front_facing_surfaces_bit) |
					FLAG(_collision_test_structure_bit) | FLAG(_collision_test_objects_bit) |
					_collision_test_objects_all_types_flags | FLAG(_collision_test_use_vehicle_physics_bit),
					origin, &vector, NONE, &collision))
			{
				test->type = collision.type;
				test->node_index = collision.node_index;
				test->object_index = collision.type == _collision_result_object ?
					(unsigned long)collision.object_index : (unsigned long)NONE;
				test->t = collision.t;
				test->plane[0] = collision.plane.n.i;
				test->plane[1] = collision.plane.n.j;
				test->plane[2] = collision.plane.n.k;
				test->plane[3] = collision.plane.d;
				test->surface_index = collision.surface_index;
			}
		}
	}
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

/* the contrails, after the lights, as lights of no colour flagged
BEVY_BRIDGE_LIGHT_CONTRAIL: each instance's line from its oldest point to
its newest (the light's forward), its middle the position and half its
length the radius, by its contrail tag (the viewer lights the ones it knows,
the Sentinels' beams: their projectiles cross 40 world units a tick and
never outlive the tick that fired them, so only the contrail is seen) */
static short bevy_bridge_contrails(struct bevy_bridge_light *lights, short maximum)
{
	long contrail_index;
	short count = 0;

	if (!contrail_data || !contrail_data->valid || !contrail_point_data || !contrail_point_data->valid)
		return 0;
	for (contrail_index = data_next_index(contrail_data, NONE);
		contrail_index != NONE && count < maximum;
		contrail_index = data_next_index(contrail_data, contrail_index))
	{
		struct contrail_datum *contrail = (struct contrail_datum *)datum_get(contrail_data, contrail_index);
		short instance_index;

		for (instance_index = 0; instance_index < 4 && count < maximum; instance_index++)
		{
			long point_index = contrail->first_contrail_point_indices[instance_index];
			real_point3d newest, oldest, middle;
			real_vector3d along;
			real length;
			struct bevy_bridge_light *out;

			if (point_index == NONE || contrail->contrail_point_counts[instance_index] < 2)
				continue;
			newest = ((struct contrail_point_datum *)datum_get(contrail_point_data, point_index))->position;
			oldest = newest;
			while (point_index != NONE)
			{
				struct contrail_point_datum *point = (struct contrail_point_datum *)datum_get(contrail_point_data, point_index);

				oldest = point->position;
				point_index = point->next_contrail_point_index;
			}
			vector_from_points3d(&oldest, &newest, &along);
			length = normalize3d(&along);
			if (length <= 0.0f)
				continue;
			middle.x = 0.5f * (oldest.x + newest.x);
			middle.y = 0.5f * (oldest.y + newest.y);
			middle.z = 0.5f * (oldest.z + newest.z);
			out = &lights[count++];
			memset(out, 0, sizeof(*out));
			bevy_bridge_point(out->position, &middle);
			bevy_bridge_vector(out->forward, &along);
			out->up[2] = 1.0f;
			out->radius = 0.5f * length;
			out->cutoff_angle = _pi;
			out->falloff_angle = _pi;
			out->definition_index = (unsigned long)contrail->definition_index;
			out->definition_flags = BEVY_BRIDGE_LIGHT_CONTRAIL;
			out->object_index = (unsigned long)contrail->object_index;
		}
	}
	return count;
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
			SET_FLAG(flags, _bevy_bridge_object_first_person_body_bit, !TEST_FLAG(flags, _bevy_bridge_object_hidden_bit));
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
	real const *function_values,
	real active_camouflage)
{
	short value_index;

	if (!bevy_bridge_shared || model_index == NONE || bevy_bridge_recorded_count >= MAXIMUM_RECORDED_MODELS)
		return;
	for (value_index = 0; value_index < 4; value_index++)
	{
		bevy_bridge_recorded[bevy_bridge_recorded_count].function_values[value_index] =
			function_values ? function_values[value_index] : 0.0f;
	}
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
	memset(&frame->fog, 0, sizeof(frame->fog));
	memset(&frame->fog_screen, 0, sizeof(frame->fog_screen));
	frame->decal_count = 0;
	frame->decal_vertex_count = 0;
	frame->sprite_batch_count = 0;
	frame->sprite_vertex_count = 0;
	frame->ray_test_count = 0;
	frame->physics_vehicle_count = 0;
	frame->vehicle_ray_test_count = 0;
	frame->point_physics_definition = (unsigned long)bevy_bridge_point_physics();
	/* the windows' screen quads (the menus' too: drawn whether or not a
	player's window is) */
	frame->screen_draw_count = bevy_bridge_screen_draw_count;
	frame->screen_vertex_count = bevy_bridge_screen_vertex_count;
	memcpy(frame->screen_draws, bevy_bridge_screen_draws,
		bevy_bridge_screen_draw_count * sizeof(struct bevy_bridge_screen_draw));
	memcpy(frame->screen_vertices, bevy_bridge_screen_vertices,
		bevy_bridge_screen_vertex_count * sizeof(struct bevy_bridge_screen_vertex));
	bevy_bridge_screen_draw_count = 0;
	bevy_bridge_screen_vertex_count = 0;
	frame->screen_flash_type = bevy_bridge_screen_flash_type;
	frame->screen_flash_intensity = bevy_bridge_screen_flash_intensity;
	memcpy(frame->screen_flash_color, bevy_bridge_screen_flash_color, sizeof(frame->screen_flash_color));
	bevy_bridge_screen_flash_type = 0;
	frame->screen_effect = bevy_bridge_screen_effect;
	memset(&bevy_bridge_screen_effect, 0, sizeof(bevy_bridge_screen_effect));
	if (bevy_bridge_viewer_draws_hud())
		bevy_bridge_share_text_atlas(shared);
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
		frame->camera.near_clip_distance = window->render_camera.z_near;
		frame->camera_valid = TRUE;
		frame->fog = bevy_bridge_fog;
		frame->fog_screen = bevy_bridge_fog_screen;
		frame->sprite_batch_count = bevy_bridge_sprite_batch_count;
		frame->sprite_vertex_count = bevy_bridge_sprite_vertex_count;
		memcpy(frame->sprite_batches, bevy_bridge_sprite_batches,
			bevy_bridge_sprite_batch_count * sizeof(struct bevy_bridge_sprite_batch));
		memcpy(frame->sprite_vertices, bevy_bridge_sprite_vertices,
			bevy_bridge_sprite_vertex_count * sizeof(struct bevy_bridge_sprite_vertex));
		frame->decal_count = bevy_bridge_decal_count;
		frame->decal_vertex_count = bevy_bridge_decal_vertex_count;
		memcpy(frame->decals, bevy_bridge_decals, bevy_bridge_decal_count * sizeof(struct bevy_bridge_decal));
		memcpy(frame->decal_vertices, bevy_bridge_decal_vertices,
			bevy_bridge_decal_vertex_count * sizeof(struct bevy_bridge_decal_vertex));
		memset(&bevy_bridge_fog_screen, 0, sizeof(bevy_bridge_fog_screen));
		first_person_unit = bevy_bridge_first_person_unit(window->local_player_index);
		frame->ray_test_count = bevy_bridge_ray_tests(frame->ray_tests, &window->render_camera.position,
			&window->render_camera.forward, &window->render_camera.up);
		bevy_bridge_vehicle_ray_tests(frame, &window->render_camera.position);
		break;
	}

	/* (the decals staged for this frame, shared or not) */
	bevy_bridge_decal_count = 0;
	bevy_bridge_decal_vertex_count = 0;
	bevy_bridge_sprite_batch_count = 0;
	bevy_bridge_sprite_vertex_count = 0;

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
			bevy_bridge_vector(out->up, &lights[index].up);
			out->color[0] = lights[index].color.red;
			out->color[1] = lights[index].color.green;
			out->color[2] = lights[index].color.blue;
			out->radius = lights[index].radius;
			out->cutoff_angle = lights[index].cutoff_angle;
			out->falloff_angle = lights[index].falloff_angle;
			out->definition_index = (unsigned long)lights[index].definition_index;
			out->definition_flags = (unsigned long)lights[index].definition_flags;
			out->object_index = (unsigned long)lights[index].object_index;
			out->first_person = 0;
			if (first_person_unit != NONE)
			{
				long ancestor;
				struct object_datum *object;

				/* (a light can outlive its object, a grenade's: object_get
				halts on one deleted) */
				for (ancestor = lights[index].object_index;
					ancestor != NONE && (object = object_try_and_get(ancestor)) != NULL;
					ancestor = object->object.parent_object_index)
				{
					if (ancestor == first_person_unit)
					{
						out->first_person = 1;
						break;
					}
				}
			}
		}
		if (global_scenario_index != NONE)
			count += bevy_bridge_contrails(&frame->lights[count], (short)(BEVY_BRIDGE_MAXIMUM_LIGHTS - count));
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
			memcpy(out->function_values, object->object.outgoing_function_values, sizeof(out->function_values));
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

	/* the models this frame drew by themselves (the first-person weapon and
	hands: render.c exports the frame once its windows are drawn) */
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
			memcpy(out->function_values, bevy_bridge_recorded[recorded].function_values, sizeof(out->function_values));
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

boolean bevy_bridge_viewer_draws_transparent(void)
{
	static int viewer_transparent = -1;

	if (viewer_transparent < 0)
		viewer_transparent = config_boolean("game.bevy_bridge_viewer_transparent") != 0;
	return viewer_transparent && bevy_bridge_overlay_enabled();
}

boolean bevy_bridge_viewer_draws_glass(void)
{
	static int viewer_glass = -1;

	if (viewer_glass < 0)
		viewer_glass = config_boolean("game.bevy_bridge_viewer_glass") != 0;
	return viewer_glass && bevy_bridge_overlay_enabled();
}

boolean bevy_bridge_viewer_draws_fog(void)
{
	static int viewer_fog = -1;

	if (viewer_fog < 0)
		viewer_fog = config_boolean("game.bevy_bridge_viewer_fog") != 0;
	return viewer_fog && bevy_bridge_overlay_enabled();
}

void bevy_bridge_note_fog(void const *data)
{
	struct render_fog const *fog = (struct render_fog const *)data;

	bevy_bridge_fog.valid = TRUE;
	bevy_bridge_fog.flags = fog->fog_definition_flags;
	bevy_bridge_fog.planar_mode = fog->planar_mode;
	bevy_bridge_fog.atmospheric_color[0] = fog->atmospheric_color.red;
	bevy_bridge_fog.atmospheric_color[1] = fog->atmospheric_color.green;
	bevy_bridge_fog.atmospheric_color[2] = fog->atmospheric_color.blue;
	bevy_bridge_fog.atmospheric_maximum_density = fog->atmospheric_maximum_density;
	bevy_bridge_fog.atmospheric_minimum_distance = fog->atmospheric_minimum_distance;
	bevy_bridge_fog.atmospheric_maximum_distance = fog->atmospheric_maximum_distance;
	bevy_bridge_vector(bevy_bridge_fog.plane, &fog->plane.n);
	bevy_bridge_fog.plane[3] = fog->plane.d;
	bevy_bridge_fog.planar_color[0] = fog->planar_color.red;
	bevy_bridge_fog.planar_color[1] = fog->planar_color.green;
	bevy_bridge_fog.planar_color[2] = fog->planar_color.blue;
	bevy_bridge_fog.planar_maximum_density = fog->planar_maximum_density;
	bevy_bridge_fog.planar_maximum_distance = fog->planar_maximum_distance;
	bevy_bridge_fog.planar_maximum_depth = fog->planar_maximum_depth;
}

boolean bevy_bridge_viewer_draws_effects(void)
{
	static int viewer_effects = -1;

	if (viewer_effects < 0)
		viewer_effects = config_boolean("game.bevy_bridge_viewer_effects") != 0;
	return viewer_effects && bevy_bridge_overlay_enabled();
}

unsigned long bevy_bridge_screen_texture(unsigned long id, void const *png, unsigned long size, unsigned long flags)
{
	struct bevy_bridge_shared *shared = bevy_bridge_get();
	unsigned long index;
	unsigned long count;

	if (!shared || !png || !size)
		return 0xFFFFFFFFUL;
	count = shared->screen_texture_count;
	for (index = 0; index < count; index++)
	{
		if (shared->screen_textures[index].id == id)
			return index;
	}
	if (count >= BEVY_BRIDGE_SCREEN_TEXTURES || shared->screen_texture_bytes + size > BEVY_BRIDGE_SCREEN_TEXTURE_BYTES)
		return 0xFFFFFFFFUL;
	memcpy(&shared->screen_texture_png[shared->screen_texture_bytes], png, size);
	shared->screen_textures[count].id = id;
	shared->screen_textures[count].offset = shared->screen_texture_bytes;
	shared->screen_textures[count].size = size;
	shared->screen_textures[count].flags = flags;
	shared->screen_texture_bytes += size;
	/* (complete before it is counted) */
	__atomic_thread_fence(__ATOMIC_RELEASE);
	__atomic_store_n(&shared->screen_texture_count, count + 1, __ATOMIC_RELAXED);
	return count;
}

/* a screen map for a bitmap: the port's art in its place (a screen
texture), or its tag's bitmap; FALSE if it is neither (rasterizer code that
shares its draws, rasterizer_xbox_dynavobgeom.c's screen_map_texture) */
unsigned long rasterizer_screen_map_texture(void const *bitmap);

boolean bevy_bridge_screen_map_from_bitmap(void const *bitmap, struct bevy_bridge_screen_map *map)
{
	map->bitmap = 0xFFFFFFFFUL;
	map->bitmap_index = 0;
	map->texture = rasterizer_screen_map_texture(bitmap);
	return map->texture != 0xFFFFFFFFUL || bevy_bridge_bitmap_tag(bitmap, &map->bitmap, &map->bitmap_index);
}

/* the high-res text's atlas, when it has changed (text_hires.c) */
unsigned long text_hires_atlas_generation(void);
unsigned char const *text_hires_atlas_coverage(void);

static void bevy_bridge_share_text_atlas(struct bevy_bridge_shared *shared)
{
	unsigned long generation = text_hires_atlas_generation();
	unsigned char const *coverage = text_hires_atlas_coverage();

	if (!coverage || generation == shared->text_atlas_generation)
		return;
	__atomic_store_n(&shared->text_atlas_sequence, shared->text_atlas_sequence | 1, __ATOMIC_RELAXED);
	__atomic_thread_fence(__ATOMIC_RELEASE);
	memcpy(shared->text_atlas, coverage, sizeof(shared->text_atlas));
	shared->text_atlas_generation = generation;
	__atomic_thread_fence(__ATOMIC_RELEASE);
	__atomic_store_n(&shared->text_atlas_sequence, (shared->text_atlas_sequence | 1) + 1, __ATOMIC_RELAXED);
}

void bevy_bridge_note_screen_flash(short type, float intensity, float const color[4])
{
	/* (the first window's: the first player's) */
	if (bevy_bridge_screen_flash_type)
		return;
	bevy_bridge_screen_flash_type = (unsigned long)type;
	bevy_bridge_screen_flash_intensity = intensity;
	memcpy(bevy_bridge_screen_flash_color, color, sizeof(bevy_bridge_screen_flash_color));
}

void bevy_bridge_note_screen_effect(struct bevy_bridge_screen_effect const *effect)
{
	/* (the first window's: the first player's) */
	if (bevy_bridge_screen_effect.flags)
		return;
	bevy_bridge_screen_effect = *effect;
}

boolean bevy_bridge_viewer_draws_hud(void)
{
	static int viewer_hud = -1;

	if (viewer_hud < 0)
		viewer_hud = config_boolean("game.bevy_bridge_viewer_hud") != 0;
	return viewer_hud && bevy_bridge_overlay_enabled();
}

boolean bevy_bridge_note_screen_draw(struct bevy_bridge_screen_draw const *draw,
	struct bevy_bridge_screen_vertex const *vertices)
{
	struct bevy_bridge_screen_draw *out;

	if (bevy_bridge_screen_draw_count >= BEVY_BRIDGE_SCREEN_DRAWS ||
		bevy_bridge_screen_vertex_count + draw->vertex_count > BEVY_BRIDGE_SCREEN_VERTICES)
	{
		return FALSE;
	}
	out = &bevy_bridge_screen_draws[bevy_bridge_screen_draw_count++];
	*out = *draw;
	out->first_vertex = bevy_bridge_screen_vertex_count;
	memcpy(&bevy_bridge_screen_vertices[bevy_bridge_screen_vertex_count], vertices,
		draw->vertex_count * sizeof(struct bevy_bridge_screen_vertex));
	bevy_bridge_screen_vertex_count += draw->vertex_count;
	return TRUE;
}

/* whether `bitmap` is one of the bitmaps of bitmap tag `tag_index`, and
which */
static boolean bevy_bridge_bitmap_in_tag(void const *bitmap, long tag_index, unsigned short *bitmap_index)
{
	struct bitmap_group *group;
	char const *first;
	unsigned long offset;

	/* (a recent tag may be one of the last map's, another group's here) */
	if (!tag_index_is_group(tag_index, BITMAP_GROUP_TAG))
		return FALSE;
	group = (struct bitmap_group *)tag_get(BITMAP_GROUP_TAG, tag_index);
	if (!group ||!group->bitmaps.address || group->bitmaps.count <= 0)
		return FALSE;
	first = (char const *)group->bitmaps.address;
	if ((char const *)bitmap < first)
		return FALSE;
	offset = (unsigned long)((char const *)bitmap - first);
	if (offset % sizeof(struct bitmap_data) || offset / sizeof(struct bitmap_data) >= (unsigned long)group->bitmaps.count)
		return FALSE;
	*bitmap_index = (unsigned short)(offset / sizeof(struct bitmap_data));
	return TRUE;
}

boolean bevy_bridge_bitmap_tag(void const *bitmap, unsigned long *tag_index, unsigned short *bitmap_index)
{
	/* the tags last found (checked again on use: the tag data's address is
	every map's) */
	static long recent[16] = { NONE, NONE, NONE, NONE, NONE, NONE, NONE, NONE,
		NONE, NONE, NONE, NONE, NONE, NONE, NONE, NONE };
	static unsigned long next;
	struct tag_iterator iterator;
	long index;
	short slot;

	if (!bitmap || global_scenario_index == NONE)
		return FALSE;
	for (slot = 0; slot < 16; slot++)
	{
		if (recent[slot] != NONE && bevy_bridge_bitmap_in_tag(bitmap, recent[slot], bitmap_index))
		{
			*tag_index = (unsigned long)recent[slot];
			return TRUE;
		}
	}
	tag_iterator_new(&iterator, BITMAP_GROUP_TAG);
	for (index = tag_iterator_next(&iterator); index != NONE; index = tag_iterator_next(&iterator))
	{
		if (bevy_bridge_bitmap_in_tag(bitmap, index, bitmap_index))
		{
			recent[next++ % 16] = index;
			*tag_index = (unsigned long)index;
			return TRUE;
		}
	}
	return FALSE;
}

boolean bevy_bridge_note_sprite_batch(struct bevy_bridge_sprite_batch const *batch,
	struct bevy_bridge_sprite_vertex const *vertices)
{
	struct bevy_bridge_sprite_batch *out;
	unsigned long vertex_count = batch->quad_count * 4;

	if (bevy_bridge_sprite_batch_count >= BEVY_BRIDGE_SPRITE_BATCHES ||
		bevy_bridge_sprite_vertex_count + vertex_count > BEVY_BRIDGE_SPRITE_VERTICES)
	{
		return FALSE;
	}
	out = &bevy_bridge_sprite_batches[bevy_bridge_sprite_batch_count++];
	*out = *batch;
	out->first_vertex = bevy_bridge_sprite_vertex_count;
	memcpy(&bevy_bridge_sprite_vertices[bevy_bridge_sprite_vertex_count], vertices,
		vertex_count * sizeof(struct bevy_bridge_sprite_vertex));
	bevy_bridge_sprite_vertex_count += vertex_count;
	return TRUE;
}

boolean bevy_bridge_viewer_draws_decals(void)
{
	static int viewer_decals = -1;

	if (viewer_decals < 0)
		viewer_decals = config_boolean("game.bevy_bridge_viewer_decals") != 0;
	return viewer_decals && bevy_bridge_overlay_enabled();
}

void bevy_bridge_note_decal(long decal_index, long map, short bitmap_index, short layer,
	short blend_function, unsigned long color, unsigned long intensity,
	void const *vertices, long quad_count)
{
	struct bevy_bridge_decal *decal;
	unsigned long vertex_count = (unsigned long)quad_count * 4;

	if (bevy_bridge_decal_count >= BEVY_BRIDGE_DECALS || quad_count <= 0 ||
		bevy_bridge_decal_vertex_count + vertex_count > BEVY_BRIDGE_DECAL_VERTICES)
	{
		return;
	}
	decal = &bevy_bridge_decals[bevy_bridge_decal_count++];
	decal->decal_index = (unsigned long)decal_index;
	decal->map = (unsigned long)map;
	decal->bitmap_index = (unsigned char)bitmap_index;
	decal->layer = (unsigned char)layer;
	decal->blend_function = (unsigned char)blend_function;
	decal->intensity = (unsigned char)(intensity < 255 ? intensity : 255);
	decal->color[0] = (unsigned char)(color >> 16);
	decal->color[1] = (unsigned char)(color >> 8);
	decal->color[2] = (unsigned char)color;
	decal->color[3] = 0;
	decal->first_vertex = bevy_bridge_decal_vertex_count;
	decal->quad_count = (unsigned long)quad_count;
	memcpy(&bevy_bridge_decal_vertices[bevy_bridge_decal_vertex_count], vertices,
		vertex_count * sizeof(struct bevy_bridge_decal_vertex));
	bevy_bridge_decal_vertex_count += vertex_count;
}

void bevy_bridge_note_fog_screen(void const *screen)
{
	memcpy(&bevy_bridge_fog_screen, screen, sizeof(bevy_bridge_fog_screen));
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
	overlay->empty = 0;
	return shared->overlay_pixels[bevy_bridge_overlay_buffer];
}

void bevy_bridge_overlay_mark_empty(void)
{
	struct bevy_bridge_shared *shared = bevy_bridge_shared;

	if (shared && bevy_bridge_overlay_buffer != NONE)
		shared->overlays[bevy_bridge_overlay_buffer].empty = 1;
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

boolean bevy_bridge_overlay_viewer(
	unsigned long *looked,
	unsigned long *interval)
{
	struct bevy_bridge_shared *shared = bevy_bridge_shared;

	if (!shared || !shared->overlay_enabled)
		return FALSE;
	*interval = __atomic_load_n(&shared->overlay_viewer_interval, __ATOMIC_RELAXED);
	*looked = __atomic_load_n(&shared->overlay_viewer_looked, __ATOMIC_RELAXED);
	return *interval != 0;
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
/* periodic_functions.c: a function's table */
unsigned char const *periodic_function_table_get(short function_type);
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
	/* (and the periodic function tables, once) */
	if (!bevy_bridge_shared->periodic_tables_valid)
	{
		short function_type;
		boolean valid = TRUE;

		for (function_type = 0; function_type < 12; function_type++)
		{
			unsigned char const *table = periodic_function_table_get(function_type);

			if (table)
				memcpy(bevy_bridge_shared->periodic_tables[function_type], table, 1024);
			else
				valid = FALSE;
		}
		__atomic_store_n(&bevy_bridge_shared->periodic_tables_valid, valid ? 1UL : 0UL, __ATOMIC_RELEASE);
	}
	memset(record, 0, sizeof(*record));
	record->damage_material_types[0] = record->damage_material_types[1] =
		record->damage_material_types[2] = record->damage_material_types[3] = -2;
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

static struct
{
	boolean active;
	struct bevy_bridge_vehicle_physics_record record;
} bevy_bridge_vehicle_physics;

static struct
{
	boolean active;
	struct bevy_bridge_vehicle_update_record record;
} bevy_bridge_vehicle_update;

void bevy_bridge_features_call(void)
{
	if (bevy_bridge_biped.active)
		bevy_bridge_biped.record.feature_calls++;
	else if (bevy_bridge_vehicle_physics.active)
		bevy_bridge_vehicle_physics.record.feature_calls++;
}

/* an object as collision features or ray tests see it, into the feature
object ring */
static void bevy_bridge_record_object(long object_index, unsigned long call)
{
	struct bevy_bridge_feature_object *entry;
	struct object_datum *object;
	unsigned long count;
	long node_count;
	long node_index;
	real_matrix4x3 const *matrices;

	object = object_get(object_index);
	count = bevy_bridge_shared->feature_object_count;
	entry = &bevy_bridge_shared->feature_objects[count % BEVY_BRIDGE_FEATURE_OBJECTS];
	memset(entry, 0, sizeof(*entry));
	entry->object_index = (unsigned long)object_index;
	entry->definition_index = (unsigned long)object->definition_index;
	entry->call = call;
	entry->type = object->object.type;
	entry->dead = TEST_FLAG(object->object.damage_flags, _object_dead_bit) ? 1 : 0;
	entry->flags = object->object.flags;
	bevy_bridge_point(entry->bounding_center, &object->object.bounding_sphere_center);
	entry->bounding_radius = object->object.bounding_sphere_radius;
	entry->parent_object_index = object->object.parent_object_index;
	bevy_bridge_vector(entry->translational_velocity, &object->object.translational_velocity);
	entry->player_index = NONE;
	entry->parent_seat_index = NONE;
	entry->location_leaf = object->object.location.leaf_index;
	entry->location_cluster = object->object.location.cluster_index;
	if (object->object.type == _object_type_projectile)
		entry->projectile_flags = ((struct projectile_datum *)object)->projectile.flags;
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
}

static void bevy_bridge_vehicle_physics_object(long object_index, unsigned long call)
{
	struct bevy_bridge_vehicle_physics_record *record = &bevy_bridge_vehicle_physics.record;

	if (!record->object_count)
		record->first_object = bevy_bridge_shared->feature_object_count;
	bevy_bridge_record_object(object_index, call);
	record->object_count++;
}

void bevy_bridge_feature_object(long object_index)
{
	struct bevy_bridge_biped_record *record = &bevy_bridge_biped.record;

	if (!bevy_bridge_biped.active && bevy_bridge_vehicle_physics.active)
	{
		unsigned long calls = bevy_bridge_vehicle_physics.record.feature_calls;

		bevy_bridge_vehicle_physics_object(object_index, calls ? calls - 1 : 0);
		return;
	}
	if (!bevy_bridge_biped.active)
		return;
	bevy_bridge_record_object(object_index, record->feature_calls ? record->feature_calls - 1 : 0);
	record->feature_object_count++;
}

void bevy_bridge_projectile_object(long object_index)
{
	struct bevy_bridge_projectile_record *record = &bevy_bridge_projectile.record;

	if (!bevy_bridge_projectile.active && bevy_bridge_vehicle_physics.active)
	{
		/* (a ray test's object: no features call) */
		bevy_bridge_vehicle_physics_object(object_index, (unsigned long)NONE);
		return;
	}
	if (!bevy_bridge_projectile.active && bevy_bridge_vehicle_update.active)
	{
		struct bevy_bridge_vehicle_update_record *update = &bevy_bridge_vehicle_update.record;

		if (!update->object_count)
			update->first_object = bevy_bridge_shared->feature_object_count;
		bevy_bridge_record_object(object_index, (unsigned long)NONE);
		update->object_count++;
		return;
	}
	if (!bevy_bridge_projectile.active)
		return;
	if (!record->object_count)
		record->first_object = bevy_bridge_shared->feature_object_count;
	bevy_bridge_record_object(object_index, 0);
	record->object_count++;
}

void bevy_bridge_projectile_damage(short material_type)
{
	struct bevy_bridge_projectile_record *record = &bevy_bridge_projectile.record;

	if (!bevy_bridge_projectile.active || record->collision_count == 0 || record->collision_count > 4)
		return;
	record->damage_material_types[record->collision_count - 1] = material_type;
	record->damage_seeds[record->collision_count - 1] = get_random_seed();
}

void bevy_bridge_projectile_guidance(long projectile_index, long target_index, void const *target_center_of_mass)
{
	struct bevy_bridge_projectile_record *record = &bevy_bridge_projectile.record;
	struct bevy_bridge_projectile_guidance *guidance;
	struct object_datum *target;

	if (!bevy_bridge_projectile.active || record->guidance_count >= 4)
		return;
	guidance = &record->guidance[record->guidance_count++];
	target = object_get(target_index);
	bevy_bridge_point(guidance->projectile_center, &object_get(projectile_index)->object.bounding_sphere_center);
	bevy_bridge_point(guidance->target_center, &target->object.bounding_sphere_center);
	bevy_bridge_point(guidance->target_center_of_mass, (real_point3d const *)target_center_of_mass);
	guidance->guidance_scale = 1.0f;
	if (TEST_FLAG(_object_mask_unit, target->object.type) && unit_get(target_index)->unit.player_index != NONE)
		guidance->guidance_scale = game_difficulty_get_value(_game_difficulty_value_guidance_vs_player);
	guidance->game_time = game_time_get();
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

/* ---------- biped updates */

static struct
{
	boolean active;
	unsigned long moving_records;
	struct bevy_bridge_biped_update_record record;
} bevy_bridge_biped_update;

static void bevy_bridge_biped_update_state(long biped_index, char state_desired, int crouching,
	struct bevy_bridge_biped_update_state *state)
{
	struct biped_datum *biped = biped_get(biped_index);

	bevy_bridge_biped_moving_state(biped_index, state_desired, crouching, &state->moving);
	bevy_bridge_vector(state->throttle, &biped->unit.throttle);
	bevy_bridge_vector(state->desired_facing_vector, &biped->unit.desired_facing_vector);
	state->biped_state = biped->biped.state;
	state->slipping_ticks = biped->biped.slipping_ticks;
	state->pad = 0;
}

void bevy_bridge_biped_update_begin(long biped_index, int enhanced_animation)
{
	struct bevy_bridge_biped_update_record *record = &bevy_bridge_biped_update.record;
	struct biped_datum *biped;

	if (!bevy_bridge_projectile_recording())
		return;
	biped = biped_get(biped_index);
	memset(record, 0, sizeof(*record));
	record->biped_index = (unsigned long)biped_index;
	record->definition_index = (unsigned long)biped->definition_index;
	record->structure_bsp_index = global_structure_bsp_index;
	record->moving_record = NONE;
	record->dead = TEST_FLAG(biped->object.damage_flags, _object_dead_bit) ? 1 : 0;
	record->control_flags = biped->unit.control_flags;
	record->aim_without_turning = TEST_FLAG(biped->unit.flags, _unit_aim_without_turning_bit) ? 1 : 0;
	record->unit_animation_state = biped->unit.animation.state;
	record->base_seat_index = biped->unit.animation.base_seat_index;
	record->enhanced_animation = enhanced_animation ? 1 : 0;
	record->limp_body = TEST_FLAG(biped->biped.flags, _biped_limp_body_physics_active_bit) &&
		biped->biped.limp_body_current_relaxation_iterations < biped->biped.limp_body_max_relaxation_iterations;
	record->player_index = biped->unit.player_index;
	/* (the animation update's: idle, not crouching, as biped_update sets it) */
	bevy_bridge_biped_update_state(biped_index, 0, 0, &record->before);
	bevy_bridge_biped_update.moving_records = bevy_bridge_shared->biped_moving_record_count;
	bevy_bridge_biped_update.active = TRUE;
}

void bevy_bridge_biped_update_end(long biped_index, char state_desired, int crouching)
{
	struct bevy_bridge_biped_update_record *record = &bevy_bridge_biped_update.record;
	unsigned long count;

	if (!bevy_bridge_biped_update.active)
		return;
	bevy_bridge_biped_update.active = FALSE;
	if (bevy_bridge_shared->biped_moving_record_count == bevy_bridge_biped_update.moving_records + 1)
		record->moving_record = (long)bevy_bridge_biped_update.moving_records;
	bevy_bridge_biped_update_state(biped_index, state_desired, crouching, &record->after);
	count = bevy_bridge_shared->biped_update_record_count;
	record->sequence = count;
	bevy_bridge_shared->biped_update_records[count % BEVY_BRIDGE_BIPED_UPDATE_RECORDS] = *record;
	__atomic_store_n(&bevy_bridge_shared->biped_update_record_count, count + 1, __ATOMIC_RELEASE);
}

void bevy_bridge_biped_update_airborne(void)
{
	if (!bevy_bridge_biped_update.active || bevy_bridge_biped_update.record.airborne_seed_taken)
		return;
	bevy_bridge_biped_update.record.airborne_seed_taken = 1;
	bevy_bridge_biped_update.record.airborne_seed = get_random_seed();
}

/* ---------- unit animation updates */

static struct
{
	boolean active;
	struct bevy_bridge_unit_animation_record record;
} bevy_bridge_unit_animation;

static void bevy_bridge_unit_animation_state(long unit_index, struct bevy_bridge_unit_animation_state *state)
{
	struct unit_datum *unit = unit_get(unit_index);

	memset(state, 0, sizeof(*state));
	state->flags = unit->unit.animation.flags;
	state->aiming_screen_index = unit->unit.animation.aiming_screen_index;
	state->looking_screen_index = unit->unit.animation.looking_screen_index;
	state->action_animation_index = unit->unit.animation.action_animation.index;
	state->action_animation_frame_index = unit->unit.animation.action_animation.frame_index;
	state->overlay_action_animation_index = unit->unit.animation.overlay_action_animation.index;
	state->overlay_action_animation_frame_index = unit->unit.animation.overlay_action_animation.frame_index;
	state->soft_ping_animation_index = unit->unit.animation.soft_ping_animation.index;
	state->soft_ping_animation_frame_index = unit->unit.animation.soft_ping_animation.frame_index;
	state->object_animation_index = unit->object.animation.state.index;
	state->object_animation_frame_index = unit->object.animation.state.frame_index;
	state->interpolation_frame_index = unit->object.animation.interpolation_frame_index;
	state->interpolation_frame_count = unit->object.animation.interpolation_frame_count;
	state->seat_index = unit->unit.animation.seat_index;
	state->weapon_index = unit->unit.animation.weapon_index;
	state->weapon_type_index = unit->unit.animation.weapon_type_index;
	state->state = unit->unit.animation.state;
	state->action = unit->unit.animation.action;
	state->overlay_action = unit->unit.animation.overlay_action;
	state->base_seat_index = unit->unit.animation.base_seat_index;
	state->object_animation_graph_index = unit->object.animation.animation_graph_index;
}

void bevy_bridge_unit_animation_begin(long unit_index, char state_desired, int crouching,
	short magic_base_seat_index, char const *weapon_label)
{
	struct bevy_bridge_unit_animation_record *record = &bevy_bridge_unit_animation.record;
	struct unit_datum *unit;

	if (!bevy_bridge_projectile_recording())
		return;
	unit = unit_get(unit_index);
	if (unit->object.type != _object_type_biped)
		return;
	memset(record, 0, sizeof(*record));
	record->unit_index = (unsigned long)unit_index;
	record->definition_index = (unsigned long)unit->definition_index;
	record->random_seed = get_random_seed();
	record->state_desired = state_desired;
	record->crouching = crouching ? 1 : 0;
	record->desired_state = unit->unit.animation.desired_state;
	record->dead = TEST_FLAG(unit->object.damage_flags, _object_dead_bit) ? 1 : 0;
	record->parented = unit->object.parent_object_index != NONE;
	record->player = unit->unit.player_index != NONE;
	record->force_alert = TEST_FLAG(unit->unit.control_flags, _unit_control_force_alert_bit) ? 1 : 0;
	record->flaming = unit->unit.flaming_death_delay > 0;
	record->magic_base_seat_index = magic_base_seat_index;
	record->magic_seat_index = unit->unit.magic_seat_index;
	record->at_rest = TEST_FLAG(unit->object.flags, _object_at_rest_bit) ? 1 : 0;
	record->airborne = TEST_FLAG(biped_get(unit_index)->biped.flags, _biped_airborne_bit) ? 1 : 0;
	if (weapon_label)
	{
		strncpy(record->weapon_label, weapon_label, sizeof(record->weapon_label) - 1);
	}
	bevy_bridge_unit_animation_state(unit_index, &record->before);
	bevy_bridge_unit_animation.active = TRUE;
}

void bevy_bridge_unit_animation_end(long unit_index, short result)
{
	struct bevy_bridge_unit_animation_record *record = &bevy_bridge_unit_animation.record;
	unsigned long count;

	if (!bevy_bridge_unit_animation.active)
		return;
	bevy_bridge_unit_animation.active = FALSE;
	record->result = result;
	/* (a destroyed unit may be gone) */
	if (record->side_effects & 4)
		record->after = record->before;
	else
		bevy_bridge_unit_animation_state(unit_index, &record->after);
	count = bevy_bridge_shared->unit_animation_record_count;
	record->sequence = count;
	bevy_bridge_shared->unit_animation_records[count % BEVY_BRIDGE_UNIT_ANIMATION_RECORDS] = *record;
	__atomic_store_n(&bevy_bridge_shared->unit_animation_record_count, count + 1, __ATOMIC_RELEASE);
}

void bevy_bridge_unit_animation_side_effect(unsigned long effect)
{
	if (bevy_bridge_unit_animation.active)
		bevy_bridge_unit_animation.record.side_effects |= effect;
}

/* ---------- poses */

static struct
{
	boolean active;
	struct bevy_bridge_pose_record record;
} bevy_bridge_pose;

static void bevy_bridge_pose_orientation(float *out, real_orientation const *orientation)
{
	out[0] = orientation->rotation.v.i;
	out[1] = orientation->rotation.v.j;
	out[2] = orientation->rotation.v.k;
	out[3] = orientation->rotation.w;
	out[4] = orientation->translation.x;
	out[5] = orientation->translation.y;
	out[6] = orientation->translation.z;
	out[7] = orientation->scale;
}

static struct bevy_bridge_pose_node *bevy_bridge_pose_node(unsigned long node_index)
{
	return &bevy_bridge_shared->pose_nodes[(bevy_bridge_pose.record.first_node + node_index) % BEVY_BRIDGE_POSE_NODES];
}

void bevy_bridge_pose_begin(long object_index, void const *node_orientations, short node_count,
	short frame_index, int world_relative, void const *parent_node_matrix)
{
	struct bevy_bridge_pose_record *record = &bevy_bridge_pose.record;
	struct object_datum *object;
	real_orientation const *orientations = (real_orientation const *)node_orientations;
	short node_index;

	bevy_bridge_pose.active = FALSE;
	if (!bevy_bridge_projectile_recording() || ((unsigned long)(object_index + game_time_get()) & 7) != 0 ||
		node_count <= 0 || node_count > MAXIMUM_NODES_PER_MODEL)
	{
		return;
	}
	object = object_get(object_index);
	memset(record, 0, sizeof(*record));
	record->object_index = (unsigned long)object_index;
	record->definition_index = (unsigned long)object->definition_index;
	record->type = object->object.type;
	record->node_count = node_count;
	record->first_node = bevy_bridge_shared->pose_node_count;
	record->object_flags = object->object.flags;
	record->animation_graph_index = object->object.animation.animation_graph_index;
	record->animation_index = object->object.animation.state.index;
	record->frame_index = frame_index;
	record->interpolation_frame_index = object->object.animation.interpolation_frame_index;
	record->interpolation_frame_count = object->object.animation.interpolation_frame_count;
	record->world_relative = world_relative ? 1 : 0;
	record->scale = object->object.scale;
	bevy_bridge_point(record->position, &object->object.position);
	bevy_bridge_vector(record->forward, &object->object.forward);
	bevy_bridge_vector(record->up, &object->object.up);
	memcpy(record->function_values, object->object.outgoing_function_values, sizeof(record->function_values));
	if (parent_node_matrix)
	{
		record->has_parent = 1;
		record->parent_mirrored = TEST_FLAG(object_get(object->object.parent_object_index)->object.flags, _object_mirrored_bit) ? 1 : 0;
		memcpy(record->parent_node_matrix, parent_node_matrix, sizeof(record->parent_node_matrix));
	}
	record->game_time = game_time_get();
	if (TEST_FLAG(_object_mask_unit, object->object.type))
	{
		struct unit_datum *unit = unit_get(object_index);
		real_vector3d forward;
		real_vector3d up;

		record->unit_recorded = 1;
		record->unit_enhanced = unit_animation_enhanced(object_index) ? 1 : 0;
		record->unit_airborne = unit_flying_through_air(object_index) ? 1 : 0;
		record->unit_parented = object->object.parent_object_index != NONE;
		record->unit_has_weapon = unit->unit.current_weapon_index != NONE;
		record->unit_player = unit->unit.player_index != NONE;
		record->unit_animation_flags = unit->unit.animation.flags;
		record->aiming_screen_index = unit->unit.animation.aiming_screen_index;
		record->looking_screen_index = unit->unit.animation.looking_screen_index;
		record->override_emotion_animation_index = unit->unit.override_emotion_animation_index;
		record->seat_index = unit->unit.animation.seat_index;
		record->weapon_index = unit->unit.animation.weapon_index;
		record->state = unit->unit.animation.state;
		record->action = unit->unit.animation.action;
		record->base_seat_index = unit->unit.animation.base_seat_index;
		record->emotion_index = unit->unit.animation.emotion_index;
		record->action_animation[0] = unit->unit.animation.action_animation.index;
		record->action_animation[1] = unit->unit.animation.action_animation.frame_index;
		record->overlay_action_animation[0] = unit->unit.animation.overlay_action_animation.index;
		record->overlay_action_animation[1] = unit->unit.animation.overlay_action_animation.frame_index;
		record->soft_ping_animation[0] = unit->unit.animation.soft_ping_animation.index;
		record->soft_ping_animation[1] = unit->unit.animation.soft_ping_animation.frame_index;
		record->mouth_aperture = unit->unit.mouth_aperture;
		bevy_bridge_vector(record->seat_acceleration, &unit->unit.seat_acceleration);
		bevy_bridge_vector(record->aiming_vector, &unit->unit.aiming_vector);
		bevy_bridge_vector(record->looking_vector, &unit->unit.looking_vector);
		object_get_orientation(object_index, &forward, &up);
		bevy_bridge_vector(record->orientation_forward, &forward);
		bevy_bridge_vector(record->orientation_up, &up);
		bevy_bridge_vector(record->translational_velocity, &object->object.translational_velocity);
		unit_grenade_throw_leg_weights(object_index, &record->grenade_leg_weight, &record->grenade_leg_time,
			&record->grenade_air_weight, &record->grenade_air_time);
	}
	if (object->object.type == _object_type_vehicle)
	{
		struct vehicle_datum *vehicle = vehicle_datum_get(object_index);

		record->vehicle_recorded = 1;
		record->vehicle_turn = vehicle->vehicle.turn;
		record->vehicle_speed = vehicle->vehicle.speed;
		record->vehicle_wheel = vehicle->vehicle.wheel;
		memcpy(record->vehicle_suspension, vehicle->vehicle.suspension, sizeof(record->vehicle_suspension));
	}
	if (TEST_FLAG(_object_mask_device, object->object.type))
	{
		struct device_datum *device = device_get(object_index);

		record->device_recorded = 1;
		record->device_flags = device->device.flags;
		record->device_position = device->device.position;
		record->device_power = device->device.power;
	}
	for (node_index = 0; node_index < node_count; node_index++)
	{
		struct bevy_bridge_pose_node *node = bevy_bridge_pose_node(node_index);

		memset(node, 0, sizeof(*node));
		bevy_bridge_pose_orientation(node->animated, &orientations[node_index]);
	}
	bevy_bridge_pose.active = TRUE;
}

void bevy_bridge_pose_preprocessed(long object_index, void const *node_orientations)
{
	struct bevy_bridge_pose_record *record = &bevy_bridge_pose.record;
	struct object_datum *object;
	real_orientation const *orientations = (real_orientation const *)node_orientations;
	real_orientation const *originals = NULL;
	short node_index;

	if (!bevy_bridge_pose.active || record->object_index != (unsigned long)object_index)
		return;
	object = object_get(object_index);
	if (object->object.animation.interpolation_frame_count > 0)
	{
		originals = (real_orientation const *)object_header_block_get(object_index, &object->object.original_node_orientations);
	}
	for (node_index = 0; node_index < record->node_count; node_index++)
	{
		struct bevy_bridge_pose_node *node = bevy_bridge_pose_node(node_index);

		bevy_bridge_pose_orientation(node->preprocessed, &orientations[node_index]);
		if (originals)
			bevy_bridge_pose_orientation(node->original, &originals[node_index]);
	}
	if (record->unit_recorded)
	{
		struct unit_datum *unit = unit_get(object_index);

		record->aiming_with_euler_screen = unit->unit.animation.aiming_with_euler_screen ? 1 : 0;
		record->looking_with_euler_screen = unit->unit.animation.looking_with_euler_screen ? 1 : 0;
		memcpy(record->aiming_screen_bounds, &unit->unit.animation.aiming_screen_bounds, sizeof(record->aiming_screen_bounds));
		memcpy(record->looking_screen_bounds, &unit->unit.animation.looking_screen_bounds, sizeof(record->looking_screen_bounds));
	}
}

void bevy_bridge_pose_end(long object_index, void const *node_matrices)
{
	struct bevy_bridge_pose_record *record = &bevy_bridge_pose.record;
	unsigned long count;
	short node_index;

	if (!bevy_bridge_pose.active || record->object_index != (unsigned long)object_index)
		return;
	bevy_bridge_pose.active = FALSE;
	for (node_index = 0; node_index < record->node_count; node_index++)
	{
		memcpy(bevy_bridge_pose_node(node_index)->matrix, (real_matrix4x3 const *)node_matrices + node_index, sizeof(float) * 13);
	}
	__atomic_store_n(&bevy_bridge_shared->pose_node_count, record->first_node + record->node_count, __ATOMIC_RELEASE);
	count = bevy_bridge_shared->pose_record_count;
	record->sequence = count;
	bevy_bridge_shared->pose_records[count % BEVY_BRIDGE_POSE_RECORDS] = *record;
	__atomic_store_n(&bevy_bridge_shared->pose_record_count, count + 1, __ATOMIC_RELEASE);
}

/* ---------- melee */

static struct
{
	boolean active;
	struct bevy_bridge_melee_record record;
} bevy_bridge_melee;

void bevy_bridge_melee_begin(long biped_index, long weapon_index)
{
	struct bevy_bridge_melee_record *record = &bevy_bridge_melee.record;
	struct biped_datum *biped;

	bevy_bridge_melee.active = FALSE;
	if (!bevy_bridge_projectile_recording())
		return;
	biped = biped_get(biped_index);
	if (biped->unit.player_index == NONE)
		return;
	memset(record, 0, sizeof(*record));
	record->biped_index = (unsigned long)biped_index;
	record->definition_index = (unsigned long)biped->definition_index;
	record->weapon_definition_index = weapon_index == NONE ? (unsigned long)NONE : (unsigned long)object_get(weapon_index)->definition_index;
	record->weapon_overcharged = weapon_index != NONE && weapon_overcharged(weapon_index);
	record->use_equipment = TEST_FLAG(biped->unit.control_flags, _unit_control_use_equipment_bit) ? 1 : 0;
	record->in_seat = biped->unit.parent_seat_index != NONE;
	record->zoomed = biped->unit.current_zoom_level != NONE;
	record->enhanced = unit_animation_enhanced(biped_index) ? 1 : 0;
	record->random_seed_before = get_random_seed();
	record->ticks_before = biped->biped.player_melee_ticks;
	record->attack_tick_before = biped->biped.player_melee_attack_tick;
	record->seat_index = biped->unit.animation.seat_index;
	record->weapon_index = biped->unit.animation.weapon_index;
	record->weapon_type_index = biped->unit.animation.weapon_type_index;
	record->action_before = biped->unit.animation.action;
	record->action_animation_before[0] = biped->unit.animation.action_animation.index;
	record->action_animation_before[1] = biped->unit.animation.action_animation.frame_index;
	bevy_bridge_melee.active = TRUE;
}

void bevy_bridge_melee_event(long biped_index, unsigned long event)
{
	struct bevy_bridge_melee_record *record = &bevy_bridge_melee.record;
	struct biped_datum *biped;

	if (!bevy_bridge_melee.active || record->biped_index != (unsigned long)biped_index)
		return;
	biped = biped_get(biped_index);
	if (event == 1)
	{
		record->started = 1;
		record->random_seed_after_start = get_random_seed();
	}
	else if (event == 2)
	{
		record->damaged = 1;
	}
}

void bevy_bridge_melee_end(long biped_index)
{
	struct bevy_bridge_melee_record *record = &bevy_bridge_melee.record;
	struct biped_datum *biped;
	unsigned long count;

	if (!bevy_bridge_melee.active || record->biped_index != (unsigned long)biped_index)
		return;
	bevy_bridge_melee.active = FALSE;
	biped = biped_get(biped_index);
	record->ticks_after = biped->biped.player_melee_ticks;
	record->attack_tick_after = biped->biped.player_melee_attack_tick;
	record->action_after = biped->unit.animation.action;
	record->action_animation_after[0] = biped->unit.animation.action_animation.index;
	record->action_animation_after[1] = biped->unit.animation.action_animation.frame_index;
	count = bevy_bridge_shared->melee_record_count;
	record->sequence = count;
	bevy_bridge_shared->melee_records[count % BEVY_BRIDGE_MELEE_RECORDS] = *record;
	__atomic_store_n(&bevy_bridge_shared->melee_record_count, count + 1, __ATOMIC_RELEASE);
}

/* ---------- vehicle physics */

typedef char bevy_bridge_mass_point_size_assert[sizeof(struct mass_point_datum) == 304 ? 1 : -1];

static void bevy_bridge_vehicle_physics_state(long vehicle_index, struct bevy_bridge_vehicle_physics_state *state)
{
	struct vehicle_datum *vehicle = vehicle_datum_get(vehicle_index);

	bevy_bridge_point(state->position, &vehicle->object.position);
	bevy_bridge_vector(state->forward, &vehicle->object.forward);
	bevy_bridge_vector(state->up, &vehicle->object.up);
	bevy_bridge_vector(state->translational_velocity, &vehicle->object.translational_velocity);
	bevy_bridge_vector(state->angular_velocity, &vehicle->object.angular_velocity);
	state->object_flags = vehicle->object.flags;
	state->location_leaf = vehicle->object.location.leaf_index;
	state->location_cluster = vehicle->object.location.cluster_index;
	state->pad = 0;
	bevy_bridge_vector(state->collision_force, &vehicle->vehicle.collision_force);
	bevy_bridge_vector(state->collision_torque, &vehicle->vehicle.collision_torque);
	state->stuck_mass_point_flags = vehicle->vehicle.stuck_mass_point_flags;
	bevy_bridge_point(state->bounding_center, &vehicle->object.bounding_sphere_center);
}

void bevy_bridge_vehicle_physics_begin(long vehicle_index, void const *powered_mass_points, short powered_count,
	void const *magic_force, void const *magic_torque, int old_physics)
{
	struct bevy_bridge_vehicle_physics_record *record = &bevy_bridge_vehicle_physics.record;
	struct powered_mass_point_datum const *powered = (struct powered_mass_point_datum const *)powered_mass_points;
	short index;

	bevy_bridge_vehicle_physics.active = FALSE;
	if (!bevy_bridge_projectile_recording())
		return;
	memset(record, 0, sizeof(*record));
	record->vehicle_index = (unsigned long)vehicle_index;
	record->definition_index = (unsigned long)object_get(vehicle_index)->definition_index;
	record->structure_bsp_index = global_structure_bsp_index;
	record->old_physics = old_physics ? 1 : 0;
	record->powered_count = powered ? (unsigned long)powered_count : 0;
	for (index = 0; powered && index < powered_count && index < 8; index++)
	{
		memcpy(record->powered[index].values, &powered[index], sizeof(record->powered[index].values));
		record->powered[index].rotation[0] = powered[index].rotation.v.i;
		record->powered[index].rotation[1] = powered[index].rotation.v.j;
		record->powered[index].rotation[2] = powered[index].rotation.v.k;
		record->powered[index].rotation[3] = powered[index].rotation.w;
	}
	if (magic_force && magic_torque)
	{
		record->has_magic = 1;
		bevy_bridge_vector(record->magic_force, (real_vector3d const *)magic_force);
		bevy_bridge_vector(record->magic_torque, (real_vector3d const *)magic_torque);
	}
	memcpy(record->breakable_surfaces, breakable_surface_flags_get(), sizeof(record->breakable_surfaces));
	bevy_bridge_vehicle_physics_state(vehicle_index, &record->before);
	bevy_bridge_vehicle_physics.active = TRUE;
}

void bevy_bridge_vehicle_physics_other(long object_index)
{
	struct bevy_bridge_vehicle_physics_record *record = &bevy_bridge_vehicle_physics.record;
	struct bevy_bridge_vehicle_physics_other *other;
	struct object_datum *object;
	real_point3d origin;
	real_vector3d forward;
	real_vector3d up;

	if (!bevy_bridge_vehicle_physics.active || record->other_count >= 8)
		return;
	other = &record->others[record->other_count++];
	object = object_get(object_index);
	other->object_index = (unsigned long)object_index;
	other->definition_index = (unsigned long)object->definition_index;
	other->at_rest = TEST_FLAG(object->object.flags, _object_at_rest_bit) ? 1 : 0;
	object_get_origin(object_index, &origin);
	object_get_orientation(object_index, &forward, &up);
	bevy_bridge_point(other->origin, &origin);
	bevy_bridge_vector(other->forward, &forward);
	bevy_bridge_vector(other->up, &up);
}

void bevy_bridge_vehicle_physics_end(long vehicle_index, void const *mass_points, short mass_point_count)
{
	struct bevy_bridge_vehicle_physics_record *record = &bevy_bridge_vehicle_physics.record;
	unsigned long count;
	short index;

	if (!bevy_bridge_vehicle_physics.active || record->vehicle_index != (unsigned long)vehicle_index)
		return;
	bevy_bridge_vehicle_physics.active = FALSE;
	bevy_bridge_vehicle_physics_state(vehicle_index, &record->after);
	record->first_mass_point = bevy_bridge_shared->mass_point_count;
	record->mass_point_count = mass_points ? (unsigned long)mass_point_count : 0;
	for (index = 0; index < (short)record->mass_point_count; index++)
	{
		memcpy(bevy_bridge_shared->mass_points[(record->first_mass_point + index) % BEVY_BRIDGE_MASS_POINTS].bytes,
			(unsigned char const *)mass_points + index * 304, 304);
	}
	__atomic_store_n(&bevy_bridge_shared->mass_point_count, record->first_mass_point + record->mass_point_count, __ATOMIC_RELEASE);
	count = bevy_bridge_shared->vehicle_physics_record_count;
	record->sequence = count;
	if (bevy_bridge_vehicle_update.active && bevy_bridge_vehicle_update.record.vehicle_index == (unsigned long)vehicle_index)
		bevy_bridge_vehicle_update.record.physics_record = count + 1;
	bevy_bridge_shared->vehicle_physics_records[count % BEVY_BRIDGE_VEHICLE_PHYSICS_RECORDS] = *record;
	__atomic_store_n(&bevy_bridge_shared->vehicle_physics_record_count, count + 1, __ATOMIC_RELEASE);
}

/* ---------- vehicle updates */

typedef char bevy_bridge_vehicle_datum_size_assert[sizeof(struct _vehicle_datum) == 88 ? 1 : -1];

boolean unit_driven_by_ai(long unit_index); /* units.c */

static void bevy_bridge_vehicle_update_state(long vehicle_index, struct bevy_bridge_vehicle_update_state *state)
{
	struct vehicle_datum *vehicle = vehicle_datum_get(vehicle_index);

	bevy_bridge_point(state->position, &vehicle->object.position);
	bevy_bridge_vector(state->forward, &vehicle->object.forward);
	bevy_bridge_vector(state->up, &vehicle->object.up);
	bevy_bridge_vector(state->translational_velocity, &vehicle->object.translational_velocity);
	bevy_bridge_vector(state->angular_velocity, &vehicle->object.angular_velocity);
	state->object_flags = vehicle->object.flags;
	state->location_leaf = vehicle->object.location.leaf_index;
	state->location_cluster = vehicle->object.location.cluster_index;
	state->pad = 0;
	bevy_bridge_point(state->bounding_center, &vehicle->object.bounding_sphere_center);
	memcpy(state->vehicle, &vehicle->vehicle, sizeof(state->vehicle));
}

void bevy_bridge_vehicle_update_begin(long vehicle_index)
{
	struct bevy_bridge_vehicle_update_record *record = &bevy_bridge_vehicle_update.record;
	struct vehicle_datum *vehicle;

	bevy_bridge_vehicle_update.active = FALSE;
	if (!bevy_bridge_projectile_recording())
		return;
	vehicle = vehicle_datum_get(vehicle_index);
	memset(record, 0, sizeof(*record));
	record->vehicle_index = (unsigned long)vehicle_index;
	record->definition_index = (unsigned long)vehicle->definition_index;
	record->structure_bsp_index = global_structure_bsp_index;
	bevy_bridge_vector(record->throttle, &vehicle->unit.throttle);
	record->control_flags = vehicle->unit.control_flags;
	bevy_bridge_vector(record->desired_facing, &vehicle->unit.desired_facing_vector);
	record->seat_power[0] = vehicle->unit.seat_power[0];
	record->seat_power[1] = vehicle->unit.seat_power[1];
	record->driven_by_ai = unit_driven_by_ai(vehicle_index) ? 1 : 0;
	record->parented = vehicle->object.parent_object_index != NONE;
	memcpy(record->breakable_surfaces, breakable_surface_flags_get(), sizeof(record->breakable_surfaces));
	bevy_bridge_vehicle_update_state(vehicle_index, &record->before);
	bevy_bridge_vehicle_update.active = TRUE;
}

void bevy_bridge_vehicle_update_end(long vehicle_index)
{
	struct bevy_bridge_vehicle_update_record *record = &bevy_bridge_vehicle_update.record;
	unsigned long count;

	if (!bevy_bridge_vehicle_update.active || record->vehicle_index != (unsigned long)vehicle_index)
		return;
	bevy_bridge_vehicle_update.active = FALSE;
	bevy_bridge_vehicle_update_state(vehicle_index, &record->after);
	count = bevy_bridge_shared->vehicle_update_record_count;
	record->sequence = count;
	bevy_bridge_shared->vehicle_update_records[count % BEVY_BRIDGE_VEHICLE_UPDATE_RECORDS] = *record;
	__atomic_store_n(&bevy_bridge_shared->vehicle_update_record_count, count + 1, __ATOMIC_RELEASE);
}
