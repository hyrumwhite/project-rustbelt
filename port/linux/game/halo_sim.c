/*
HALO_SIM.C

Runs bevy-rt-test's Rust ports of the game's simulation (crates/halo_sim,
a DLL: halo_sim.dll beside the game, or HALO_SIM_DLL) in place of the
game's own C, one system at a time: the object simulation handing off from
the game to Rust (ROADMAP phase 7).

Off unless game.rust_simulation names systems (HALO_RUST_SIMULATION, e.g.
"point_physics" or "point_physics:check"). The DLL reads the level from
the game's own map file at its first call in the level. In check mode both
run: the game keeps its own result, and the Rust result is compared with it
bit for bit; differences are counted and the first few logged.
*/

/* ---------- headers */

#include "cseries.h"
#include "cache/cache_files.h"
#include "cache/physical_memory_map.h"
#include "physics/breakable_surfaces.h"
#include "game/game.h"
#include "models/model_definitions.h"
#include "objects/objects.h"
#include "physics/point_physics.h"
#include "scenario/scenario.h"
#include "tag_files/tag_groups.h"
#include "halo_sim.h"

#include <stdio.h>
#include <string.h>


/* ---------- constants */

/* the interface version this side speaks (crates/halo_sim's VERSION) */
#define HALO_SIM_VERSION 2

/* ---------- structures */

/* crates/halo_sim's PointPhysicsCall and PointPhysicsResult */
struct halo_sim_point_physics_call
{
	long structure_bsp_index;
	unsigned long flags;
	struct point_physics_definition const *definition;
	unsigned char const *breakable_surfaces;
	real_point3d position;
	real_vector3d velocity;
	long location_leaf;
	short location_cluster;
	short underwater;
	unsigned long has_force;
	real_vector3d force;
	real_vector3d wind;
	real radius;
	real dt;
};

struct halo_sim_point_physics_result
{
	unsigned long flags;
	real_point3d position;
	real_vector3d velocity;
	long location_leaf;
	short location_cluster;
	short collided;
	real_vector3d collision_normal;
	long collision_material_type;
};

/* crates/halo_sim's PoseOrientationsCall and PoseMatricesCall */
struct halo_sim_pose_orientations_call
{
	unsigned long object_index;
	unsigned long definition_index;
	long animation_graph_index;
	short animation_index;
	short frame_index;
	unsigned long object_flags;
	long game_time;
	real scale;
	real function_values[4];
	short node_count;
	short pad;
	real_orientation *orientations;
};

struct halo_sim_pose_matrices_call
{
	unsigned long definition_index;
	short node_count;
	short interpolation_frame_index;
	short interpolation_frame_count;
	short world_relative;
	unsigned long object_flags;
	real_point3d position;
	real_vector3d forward;
	real_vector3d up;
	unsigned long has_parent;
	unsigned long parent_mirrored;
	real_matrix4x3 parent_node_matrix;
	real_orientation const *original_orientations;
	real_orientation *orientations;
	real_matrix4x3 *matrices;
};

typedef unsigned long (*halo_sim_version_function)(void);
typedef long (*halo_sim_set_tag_cache_function)(void const *memory, unsigned long size, unsigned long base);
typedef long (*halo_sim_pose_orientations_function)(struct halo_sim_pose_orientations_call *call);
typedef long (*halo_sim_pose_matrices_function)(struct halo_sim_pose_matrices_call const *call);
typedef long (*halo_sim_load_map_function)(char const *path);
typedef long (*halo_sim_point_physics_function)(struct halo_sim_point_physics_call const *call,
	struct halo_sim_point_physics_result *result);

/* ---------- globals */

static struct
{
	boolean initialized;
	boolean loaded;
	short modes[NUMBER_OF_HALO_SIM_SYSTEMS];
	halo_sim_load_map_function load_map;
	halo_sim_point_physics_function point_physics;
	halo_sim_set_tag_cache_function set_tag_cache;
	halo_sim_pose_orientations_function pose_orientations;
	halo_sim_pose_matrices_function pose_matrices;
	long scenario_index;
	boolean level_loaded;
} halo_sim;

/* the check mode's comparisons */
static struct
{
	boolean pending;
	struct halo_sim_point_physics_call call;
	struct halo_sim_point_physics_result result;
	unsigned long compared;
	unsigned long differed;
} halo_sim_point_physics_checks;

static char const *const halo_sim_system_names[NUMBER_OF_HALO_SIM_SYSTEMS] =
{
	"point_physics",
	"poses",
};

/* the pose checks: Rust's result, and the comparisons */
static struct
{
	boolean orientations_pending;
	long orientations_object;
	int world_relative;
	real_orientation orientations[MAXIMUM_NODES_PER_MODEL];
	boolean matrices_pending;
	long matrices_object;
	real_orientation interpolated[MAXIMUM_NODES_PER_MODEL];
	real_matrix4x3 matrices[MAXIMUM_NODES_PER_MODEL];
	unsigned long compared[2];
	unsigned long differed[2];
} halo_sim_pose_checks;

/* the platform layer's (port/linux/src) */
void platform_log(char const *format, ...);
void platform_translate_path(char const *xbox_path, char *host_path, unsigned long host_path_size);
char const *config_string(char const *name);

/* ---------- private code */

/* the platform layer's (win32_library.c, posix_library.c) */
void *platform_library_open(char const *path);
void *platform_library_symbol(void *library, char const *name);

static void *halo_sim_symbol(void *library, char const *name)
{
	return platform_library_symbol(library, name);
}

static void halo_sim_initialize(void)
{
	char const *systems;
	char const *path;
	void *library;
	short system;

	halo_sim.initialized = TRUE;
	halo_sim.scenario_index = NONE;
	systems = config_string("game.rust_simulation");
	if (!systems || !systems[0])
		return;

	path = getenv("HALO_SIM_DLL");
	if (!path || !path[0])
		path = "halo_sim.dll";
	library = platform_library_open(path);
	if (!library)
	{
		platform_log("halo_sim: %s isn't loaded; the game's own simulation runs", path);
		return;
	}
	{
		halo_sim_version_function version = (halo_sim_version_function)halo_sim_symbol(library, "halo_sim_version");

		if (!version || version() != HALO_SIM_VERSION)
		{
			platform_log("halo_sim: %s speaks another version; the game's own simulation runs", path);
			return;
		}
	}
	halo_sim.load_map = (halo_sim_load_map_function)halo_sim_symbol(library, "halo_sim_load_map");
	halo_sim.point_physics = (halo_sim_point_physics_function)halo_sim_symbol(library, "halo_sim_point_physics_update");
	halo_sim.set_tag_cache = (halo_sim_set_tag_cache_function)halo_sim_symbol(library, "halo_sim_set_tag_cache");
	halo_sim.pose_orientations = (halo_sim_pose_orientations_function)halo_sim_symbol(library, "halo_sim_pose_orientations");
	halo_sim.pose_matrices = (halo_sim_pose_matrices_function)halo_sim_symbol(library, "halo_sim_pose_matrices");
	if (!halo_sim.load_map || !halo_sim.point_physics || !halo_sim.set_tag_cache || !halo_sim.pose_orientations ||
		!halo_sim.pose_matrices)
	{
		platform_log("halo_sim: %s lacks an entry point; the game's own simulation runs", path);
		return;
	}
	halo_sim.loaded = TRUE;

	for (system = 0; system < NUMBER_OF_HALO_SIM_SYSTEMS; system++)
	{
		char const *name = halo_sim_system_names[system];
		char const *found = strstr(systems, name);
		size_t length = strlen(name);

		if (found && (found == systems || found[-1] == ',') &&
			(found[length] == 0 || found[length] == ',' || found[length] == ':'))
		{
			halo_sim.modes[system] = strncmp(found + length, ":check", 6) == 0 ? _halo_sim_mode_check : _halo_sim_mode_rust;
			platform_log("halo_sim: %s runs in Rust%s", name,
				halo_sim.modes[system] == _halo_sim_mode_check ? ", checked against the game's" : "");
		}
	}
}

/* the level's map file, read by the DLL once a level */
static boolean halo_sim_level_loaded(void)
{
	if (global_scenario_index != halo_sim.scenario_index)
	{
		char xbox_path[256] = "";
		char path[1024] = "";
		long count = -1;

		halo_sim.scenario_index = global_scenario_index;
		halo_sim.level_loaded = FALSE;
		if (global_scenario_index != NONE &&
			snprintf(xbox_path, sizeof(xbox_path), "%s%s.map", cache_files_map_directory(),
				tag_name_strip_path(tag_get_name(global_scenario_index))) < (int)sizeof(xbox_path))
		{
			/* (the game's d:\maps\, on the host) */
			platform_translate_path(xbox_path, path, sizeof(path));
			count = halo_sim.load_map(path);
		}
		if (count > 0)
		{
			/* the level's tags, where the game holds them (its pointers
			address them from where they are) */
			void *tag_cache = physical_memory_get_tag_cache_base_address();
			long tags = halo_sim.set_tag_cache(tag_cache, TAG_CACHE_SIZE, (unsigned long)tag_cache);

			platform_log("halo_sim: reads the game's %ld tags at %p", tags, tag_cache);
			if (tags <= 0)
				count = -1;
		}
		halo_sim.level_loaded = count > 0;
		platform_log("halo_sim: %s the level %s (%ld structure bsps)", halo_sim.level_loaded ? "loaded" : "couldn't load",
			global_scenario_index != NONE ? path : "(none)", count);
	}

	return halo_sim.level_loaded;
}

/* ---------- public code */

short halo_sim_mode(short system)
{
	if (!halo_sim.initialized)
		halo_sim_initialize();
	if (!halo_sim.loaded || system < 0 || system >= NUMBER_OF_HALO_SIM_SYSTEMS ||
		halo_sim.modes[system] == _halo_sim_mode_c || !halo_sim_level_loaded())
	{
		return _halo_sim_mode_c;
	}

	return halo_sim.modes[system];
}

int halo_sim_point_physics_update(
	unsigned long flags,
	void const *definition,
	void *location,
	void *position,
	void *velocity,
	void const *force,
	void const *wind,
	int underwater,
	float radius,
	float dt,
	void *collision_normal,
	short *collision_material_type,
	unsigned long *result)
{
	short mode = halo_sim_mode(_halo_sim_point_physics);
	struct halo_sim_point_physics_call call;
	struct halo_sim_point_physics_result rust;
	struct location *point_location = (struct location *)location;

	halo_sim_point_physics_checks.pending = FALSE;
	if (mode == _halo_sim_mode_c)
		return FALSE;

	call.structure_bsp_index = global_structure_bsp_index;
	call.flags = flags;
	call.definition = (struct point_physics_definition const *)definition;
	call.breakable_surfaces = (unsigned char const *)breakable_surface_flags_get();
	call.position = *(real_point3d *)position;
	call.velocity = *(real_vector3d *)velocity;
	call.location_leaf = point_location->leaf_index;
	call.location_cluster = point_location->cluster_index;
	call.underwater = underwater ? 1 : 0;
	call.has_force = force != NULL;
	if (force)
		call.force = *(real_vector3d const *)force;
	else
		memset(&call.force, 0, sizeof(call.force));
	call.wind = *(real_vector3d const *)wind;
	call.radius = radius;
	call.dt = dt;
	if (halo_sim.point_physics(&call, &rust) != 0)
		return FALSE;

	if (mode == _halo_sim_mode_check)
	{
		halo_sim_point_physics_checks.pending = TRUE;
		halo_sim_point_physics_checks.call = call;
		halo_sim_point_physics_checks.result = rust;
		return FALSE;
	}

	*result = rust.flags;
	*(real_point3d *)position = rust.position;
	*(real_vector3d *)velocity = rust.velocity;
	point_location->leaf_index = rust.location_leaf;
	point_location->cluster_index = rust.location_cluster;
	if (rust.collided)
	{
		if (collision_normal)
			*(real_vector3d *)collision_normal = rust.collision_normal;
		if (collision_material_type)
			*collision_material_type = (short)rust.collision_material_type;
	}

	return TRUE;
}

void halo_sim_point_physics_check(
	unsigned long result,
	void const *location,
	void const *position,
	void const *velocity,
	void const *collision_normal,
	short const *collision_material_type)
{
	struct halo_sim_point_physics_result const *rust = &halo_sim_point_physics_checks.result;
	struct location const *point_location = (struct location const *)location;
	boolean same;

	if (!halo_sim_point_physics_checks.pending)
		return;
	halo_sim_point_physics_checks.pending = FALSE;
	same = rust->flags == result &&
		memcmp(&rust->position, position, sizeof(real_point3d)) == 0 &&
		memcmp(&rust->velocity, velocity, sizeof(real_vector3d)) == 0 &&
		rust->location_leaf == point_location->leaf_index &&
		rust->location_cluster == point_location->cluster_index &&
		(!rust->collided ||
			((!collision_normal || memcmp(&rust->collision_normal, collision_normal, sizeof(real_vector3d)) == 0) &&
			(!collision_material_type || (short)rust->collision_material_type == *collision_material_type)));
	halo_sim_point_physics_checks.compared++;
	if (!same)
	{
		halo_sim_point_physics_checks.differed++;
		if (halo_sim_point_physics_checks.differed <= 8)
		{
			real_point3d const *p = (real_point3d const *)position;

			platform_log("halo_sim: point physics differs: game %lx (%f %f %f) leaf %ld, rust %lx (%f %f %f) leaf %ld",
				result, p->x, p->y, p->z, point_location->leaf_index,
				rust->flags, rust->position.x, rust->position.y, rust->position.z, rust->location_leaf);
			{
				struct halo_sim_point_physics_call const *c = &halo_sim_point_physics_checks.call;

				platform_log("halo_sim:   from bsp %ld flags %lx position %08lx %08lx %08lx velocity %08lx %08lx %08lx leaf %ld wind %08lx %08lx %08lx underwater %d radius %08lx dt %08lx force %lu",
					c->structure_bsp_index, c->flags,
					*(unsigned long const *)&c->position.x, *(unsigned long const *)&c->position.y, *(unsigned long const *)&c->position.z,
					*(unsigned long const *)&c->velocity.i, *(unsigned long const *)&c->velocity.j, *(unsigned long const *)&c->velocity.k,
					c->location_leaf,
					*(unsigned long const *)&c->wind.i, *(unsigned long const *)&c->wind.j, *(unsigned long const *)&c->wind.k,
					c->underwater, *(unsigned long const *)&c->radius, *(unsigned long const *)&c->dt, c->has_force);
			}
		}
	}
	if ((halo_sim_point_physics_checks.compared & 0xffff) == 0)
	{
		platform_log("halo_sim: point physics: %lu of %lu agree with the game",
			halo_sim_point_physics_checks.compared - halo_sim_point_physics_checks.differed,
			halo_sim_point_physics_checks.compared);
	}
}

/* ---------- poses */

int halo_sim_pose_orientations(long object_index, short node_count, void *orientations,
	int *world_relative, short *frame_index)
{
	short mode = halo_sim_mode(_halo_sim_poses);
	struct object_datum *object;
	struct halo_sim_pose_orientations_call call;
	long result;

	halo_sim_pose_checks.orientations_pending = FALSE;
	if (mode == _halo_sim_mode_c || node_count <= 0 || node_count > MAXIMUM_NODES_PER_MODEL)
		return FALSE;
	object = object_get(object_index);
	call.object_index = (unsigned long)object_index;
	call.definition_index = (unsigned long)object->definition_index;
	call.animation_graph_index = object->object.animation.animation_graph_index;
	call.animation_index = object->object.animation.state.index;
	call.frame_index = object->object.animation.state.frame_index;
	call.object_flags = object->object.flags;
	call.game_time = game_time_get();
	call.scale = object->object.scale;
	memcpy(call.function_values, object->object.outgoing_function_values, sizeof(call.function_values));
	call.node_count = node_count;
	call.pad = 0;
	call.orientations = mode == _halo_sim_mode_check ? halo_sim_pose_checks.orientations : (real_orientation *)orientations;
	result = halo_sim.pose_orientations(&call);
	if (result < 0)
		return FALSE;
	if (mode == _halo_sim_mode_check)
	{
		halo_sim_pose_checks.orientations_pending = TRUE;
		halo_sim_pose_checks.orientations_object = object_index;
		halo_sim_pose_checks.world_relative = (int)result;
		return FALSE;
	}
	*world_relative = (int)result;
	*frame_index = call.frame_index;

	return TRUE;
}

static void halo_sim_pose_report(short stage, long object_index, short node_count, short node_index)
{
	halo_sim_pose_checks.differed[stage]++;
	if (halo_sim_pose_checks.differed[stage] <= 8)
	{
		platform_log("halo_sim: pose %s of %s (object %08lx) differs at node %d of %d",
			stage ? "matrices" : "orientations", tag_get_name(object_get(object_index)->definition_index),
			(unsigned long)object_index, node_index, node_count);
	}
}

static void halo_sim_pose_tally(short stage)
{
	halo_sim_pose_checks.compared[stage]++;
	if ((halo_sim_pose_checks.compared[stage] & 0x3fff) == 0)
	{
		platform_log("halo_sim: pose %s: %lu of %lu agree with the game", stage ? "matrices" : "orientations",
			halo_sim_pose_checks.compared[stage] - halo_sim_pose_checks.differed[stage], halo_sim_pose_checks.compared[stage]);
	}
}

void halo_sim_pose_orientations_check(long object_index, short node_count, void const *orientations,
	int world_relative)
{
	real_orientation const *game = (real_orientation const *)orientations;
	short node_index;

	if (!halo_sim_pose_checks.orientations_pending || halo_sim_pose_checks.orientations_object != object_index)
		return;
	halo_sim_pose_checks.orientations_pending = FALSE;
	for (node_index = 0; node_index < node_count; node_index++)
	{
		if (memcmp(&game[node_index], &halo_sim_pose_checks.orientations[node_index], sizeof(real_orientation)) != 0)
			break;
	}
	if (node_index < node_count || world_relative != halo_sim_pose_checks.world_relative)
		halo_sim_pose_report(0, object_index, node_count, node_index);
	halo_sim_pose_tally(0);
}

int halo_sim_pose_matrices(long object_index, short node_count, int world_relative,
	void const *parent_node_matrix, void *orientations, void *matrices)
{
	short mode = halo_sim_mode(_halo_sim_poses);
	struct object_datum *object;
	struct halo_sim_pose_matrices_call call;

	halo_sim_pose_checks.matrices_pending = FALSE;
	if (mode == _halo_sim_mode_c || node_count <= 0 || node_count > MAXIMUM_NODES_PER_MODEL)
		return FALSE;
	object = object_get(object_index);
	call.definition_index = (unsigned long)object->definition_index;
	call.node_count = node_count;
	call.interpolation_frame_index = object->object.animation.interpolation_frame_index;
	call.interpolation_frame_count = object->object.animation.interpolation_frame_count;
	call.world_relative = world_relative ? 1 : 0;
	call.object_flags = object->object.flags;
	call.position = object->object.position;
	call.forward = object->object.forward;
	call.up = object->object.up;
	call.has_parent = parent_node_matrix != NULL;
	call.parent_mirrored = parent_node_matrix &&
		TEST_FLAG(object_get(object->object.parent_object_index)->object.flags, _object_mirrored_bit);
	if (parent_node_matrix)
		call.parent_node_matrix = *(real_matrix4x3 const *)parent_node_matrix;
	else
		memset(&call.parent_node_matrix, 0, sizeof(call.parent_node_matrix));
	call.original_orientations = call.interpolation_frame_count > 0 ?
		(real_orientation const *)object_header_block_get(object_index, &object->object.original_node_orientations) :
		NULL;
	if (mode == _halo_sim_mode_check)
	{
		memcpy(halo_sim_pose_checks.interpolated, orientations, sizeof(real_orientation) * node_count);
		call.orientations = halo_sim_pose_checks.interpolated;
		call.matrices = halo_sim_pose_checks.matrices;
	}
	else
	{
		call.orientations = (real_orientation *)orientations;
		call.matrices = (real_matrix4x3 *)matrices;
	}
	if (halo_sim.pose_matrices(&call) != 0)
		return FALSE;
	if (mode == _halo_sim_mode_check)
	{
		halo_sim_pose_checks.matrices_pending = TRUE;
		halo_sim_pose_checks.matrices_object = object_index;
		return FALSE;
	}

	return TRUE;
}

void halo_sim_pose_matrices_check(long object_index, short node_count, void const *orientations,
	void const *matrices)
{
	real_orientation const *game_orientations = (real_orientation const *)orientations;
	real_matrix4x3 const *game_matrices = (real_matrix4x3 const *)matrices;
	short node_index;

	if (!halo_sim_pose_checks.matrices_pending || halo_sim_pose_checks.matrices_object != object_index)
		return;
	halo_sim_pose_checks.matrices_pending = FALSE;
	for (node_index = 0; node_index < node_count; node_index++)
	{
		if (memcmp(&game_orientations[node_index], &halo_sim_pose_checks.interpolated[node_index], sizeof(real_orientation)) != 0 ||
			memcmp(&game_matrices[node_index], &halo_sim_pose_checks.matrices[node_index], sizeof(real_matrix4x3)) != 0)
		{
			break;
		}
	}
	if (node_index < node_count)
		halo_sim_pose_report(1, object_index, node_count, node_index);
	halo_sim_pose_tally(1);
}
