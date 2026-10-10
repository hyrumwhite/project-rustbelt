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
#include "units/units.h"
#include "units/bipeds.h"
#include "units/vehicle_datum.h"
#include "devices/devices.h"
#include "physics/point_physics.h"
#include "physics/collisions.h"
#include "items/projectiles.h"
#include "items/projectiles_callbacks.h"
#include "scenario/scenario.h"
#include "tag_files/tag_groups.h"
#include "bevy_bridge.h"
#include "halo_sim.h"

#include <stdio.h>
#include <string.h>


/* ---------- constants */

/* the interface version this side speaks (crates/halo_sim's VERSION) */
#define HALO_SIM_VERSION 5

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

/* crates/halo_sim's PosePreprocessCall */
struct halo_sim_pose_preprocess_call
{
	unsigned long object_index;
	unsigned long definition_index;
	short object_type;
	short node_count;
	real_orientation *orientations;
	unsigned long enhanced;
	unsigned long airborne;
	unsigned long parented;
	unsigned long has_weapon;
	unsigned long player;
	long game_time;
	unsigned short unit_animation_flags;
	short aiming_screen_index;
	short looking_screen_index;
	short override_emotion_animation_index;
	char seat_index;
	char weapon_index;
	char state;
	char action;
	char base_seat_index;
	char emotion_index;
	short pad;
	short action_animation[2];
	short overlay_action_animation[2];
	short soft_ping_animation[2];
	real mouth_aperture;
	real_vector3d seat_acceleration;
	real_vector3d aiming_vector;
	real_vector3d looking_vector;
	real_vector3d orientation_forward;
	real_vector3d orientation_up;
	real_vector3d forward;
	real_vector3d up;
	real_vector3d translational_velocity;
	real leg_weight;
	long leg_time;
	real air_weight;
	long air_time;
	unsigned long aiming_with_euler_screen;
	unsigned long looking_with_euler_screen;
	unsigned long aiming_bounds_set;
	unsigned long looking_bounds_set;
	real_rectangle2d aiming_screen_bounds;
	real_rectangle2d looking_screen_bounds;
	real vehicle_turn;
	real vehicle_speed;
	real vehicle_wheel;
	unsigned char vehicle_suspension[8];
	unsigned long device_flags;
	real device_position;
	real device_power;
};

/* crates/halo_sim's HostObject and Host: the running game's objects, as
Rust's ray test reads them */
struct halo_sim_host_object
{
	unsigned long object_index;
	unsigned long definition_index;
	short object_type;
	short location_cluster;
	unsigned long flags;
	real_point3d bounding_center;
	real bounding_radius;
	unsigned long parent;
	unsigned char region_permutations[8];
	unsigned long node_count;
	real_matrix4x3 const *matrices;
	real_point3d origin;
	real_vector3d forward;
	real_vector3d up;
	unsigned long projectile_flags;
};

/* crates/halo_map's ProjectileState: what a projectile update reads and
changes of a projectile */
struct halo_sim_projectile_state
{
	real_point3d position;
	real_vector3d velocity;
	unsigned long flags;
	unsigned long object_flags;
	short action;
	short hit_material_type;
	long ignore_object_index;
	long target_object_index;
	long parent_object_index;
	real detonation_timer;
	real detonation_timer_delta;
	real arming_time;
	real arming_time_delta;
	real odometer;
	real deceleration_timer;
	real deceleration_timer_delta;
	real deceleration;
	real maximum_damage_distance;
	long location_leaf;
	short location_cluster;
	short pad;
};

/* crates/halo_sim's GuidanceC, ProjectileEventC and ProjectileUpdateCall */
struct halo_sim_guidance
{
	real_point3d projectile_center;
	real_point3d target_center;
	real_point3d target_center_of_mass;
	real guidance_scale;
};

struct halo_sim_projectile_event
{
	short kind;
	short material_type;
	unsigned long seed_before;
	unsigned long seed_after;
	long object_index;
	long potential;
	real scale;
	real material_effect_scale;
	struct halo_sim_guidance guidance;
	long game_time;
	struct halo_sim_projectile_state state;
};

struct halo_sim_projectile_call
{
	unsigned long projectile_index;
	unsigned long definition_index;
	long structure_bsp_index;
	unsigned char const *breakable_surfaces;
	struct halo_sim_projectile_state state;
	unsigned long *random_seed;
	real_vector3d const *directions;
	long direction_count;
	unsigned char const *wander_table;
	long game_time;
	struct halo_sim_projectile_event const *events;
	unsigned long event_count;
	long collision_count;
	real time_remaining;
	long difference_event;
	long difference_reason;
};

/* crates/halo_sim's ProjectileCallbacks */
struct halo_sim_projectile_callbacks
{
	void (*timers_updated)(unsigned long projectile_index, struct halo_sim_projectile_state *state);
	void (*guidance)(unsigned long projectile_index, struct halo_sim_projectile_state *state, struct halo_sim_guidance *out);
	void (*collision_begin)(unsigned long projectile_index, struct halo_sim_projectile_state *state);
	void (*impact_damage)(unsigned long projectile_index, struct halo_sim_projectile_state *state,
		struct collision_result const *collision, real damage_scale, real_vector3d const *velocity,
		short *material_type, real *material_effect_scale);
	void (*breakable_surface_damage)(unsigned long projectile_index, struct halo_sim_projectile_state *state,
		struct collision_result const *collision, real_vector3d const *velocity);
	void (*effect)(unsigned long projectile_index, struct halo_sim_projectile_state *state, long effect,
		long material_type, long potential, struct collision_result const *collision, real_vector3d const *direction,
		real scale, real material_effect_scale);
	void (*combining_reset)(unsigned long child_index);
	void (*attach)(unsigned long projectile_index, struct halo_sim_projectile_state *state,
		real_point3d const *new_position, struct collision_result const *collision);
	void (*impact_noise)(unsigned long projectile_index, struct halo_sim_projectile_state *state, real_point3d const *point);
	void (*moved)(unsigned long projectile_index, struct halo_sim_projectile_state *state, real_point3d const *new_position,
		real_vector3d const *new_velocity, long location_leaf, long location_cluster, real time_remaining,
		long collision_count, long ignore_object_index);
};

struct halo_sim_host
{
	unsigned long (*cluster_objects)(short cluster, unsigned long *out, unsigned long maximum);
	long (*object)(unsigned long index, struct halo_sim_host_object *out);
	unsigned long (*children)(unsigned long index, unsigned long *out, unsigned long maximum);
	struct halo_sim_projectile_callbacks projectile;
};

/* crates/halo_sim's CollisionVectorCall and CollisionVectorResult */
struct halo_sim_collision_vector_call
{
	long structure_bsp_index;
	unsigned long flags;
	unsigned char const *breakable_surfaces;
	real_point3d point;
	real_vector3d vector;
	unsigned long ignore_object_index;
};

struct halo_sim_collision_vector_result
{
	long hit;
	short type;
	short node_index;
	real t;
	real_point3d point;
	real_plane3d plane;
	unsigned long object_index;
	short bsp_index;
	short material_type;
	long surface_index;
	long location_leaf;
	short location_cluster;
	unsigned short surface_flags;
	short region_index;
	short material_index;
	long plane_designator;
	unsigned long breakable_surface_index;
};

typedef unsigned long (*halo_sim_version_function)(void);
typedef long (*halo_sim_set_host_function)(struct halo_sim_host const *host);
typedef long (*halo_sim_projectile_function)(struct halo_sim_projectile_call *call);
typedef long (*halo_sim_collision_test_vector_function)(struct halo_sim_collision_vector_call const *call,
	struct halo_sim_collision_vector_result *result);
typedef long (*halo_sim_pose_preprocess_function)(struct halo_sim_pose_preprocess_call *call);
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
	halo_sim_pose_preprocess_function pose_preprocess;
	halo_sim_set_host_function set_host;
	halo_sim_collision_test_vector_function collision_test_vector;
	halo_sim_projectile_function projectile_update;
	halo_sim_projectile_function projectile_check;
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
	"collision",
	"projectiles",
};

/* the projectile checks: the state and random seed the game's update began
with, its calls, and the comparisons */
#define MAXIMUM_HALO_SIM_PROJECTILE_EVENTS 256
static struct
{
	boolean recording;
	boolean overflow;
	long projectile_index;
	unsigned long seed;
	struct halo_sim_projectile_state before;
	unsigned long event_count;
	struct halo_sim_projectile_event events[MAXIMUM_HALO_SIM_PROJECTILE_EVENTS];
	unsigned long compared;
	unsigned long differed;
	unsigned long skipped;
	unsigned long collisions;
	unsigned long events_compared;
} halo_sim_projectile_checks;

/* the collision checks' comparisons */
static struct
{
	unsigned long compared;
	unsigned long differed;
	/* what the game's hit: the structure, media, objects (vehicles' physics) */
	unsigned long structure_hits;
	unsigned long media_hits;
	unsigned long object_hits;
	unsigned long vehicle_physics_hits;
} halo_sim_collision_checks;

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
	boolean preprocess_pending;
	long preprocess_object;
	struct halo_sim_pose_preprocess_call preprocess;
	real_orientation preprocessed[MAXIMUM_NODES_PER_MODEL];
	unsigned long compared[3];
	unsigned long differed[3];
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

/* the host's callbacks: the running game's objects */
static unsigned long halo_sim_host_cluster_objects(short cluster, unsigned long *out, unsigned long maximum)
{
	unsigned long count = 0;
	long reference_index;
	long object_index;

	for (object_index = cluster_get_first_collideable_object(&reference_index, cluster);
		object_index != NONE;
		object_index = cluster_get_next_collideable_object(&reference_index))
	{
		if (count < maximum)
			out[count] = (unsigned long)object_index;
		count++;
	}

	return count;
}

/* whether a projectile update runs in Rust (the objects its ray tests
visit, recorded for the bridge as the game's own ray tests record them) */
static boolean halo_sim_projectile_updating;

static long halo_sim_host_object(unsigned long index, struct halo_sim_host_object *out)
{
	struct object_datum *object = object_get((long)index);
	short region;

	if (halo_sim_projectile_updating)
		bevy_bridge_projectile_object((long)index);

	out->object_index = index;
	out->definition_index = (unsigned long)object->definition_index;
	out->object_type = object->object.type;
	out->location_cluster = object->object.location.cluster_index;
	out->flags = object->object.flags;
	out->bounding_center = object->object.bounding_sphere_center;
	out->bounding_radius = object->object.bounding_sphere_radius;
	out->parent = (unsigned long)object->object.parent_object_index;
	for (region = 0; region < 8; region++)
	{
		out->region_permutations[region] = region < (short)sizeof(object->object.region_permutations) ?
			object->object.region_permutations[region] : 0;
	}
	out->node_count = (unsigned long)(object->object.node_matrices.size / (long)sizeof(real_matrix4x3));
	out->matrices = object_get_node_matrices((long)index);
	if (object->object.type == _object_type_vehicle)
	{
		object_get_origin((long)index, &out->origin);
		object_get_orientation((long)index, &out->forward, &out->up);
	}
	else
	{
		memset(&out->origin, 0, sizeof(out->origin));
		memset(&out->forward, 0, sizeof(out->forward));
		memset(&out->up, 0, sizeof(out->up));
	}
	out->projectile_flags = object->object.type == _object_type_projectile ?
		((struct projectile_datum *)object)->projectile.flags : 0;

	return TRUE;
}

/* an object's children: its first child, then that child's siblings */
static unsigned long halo_sim_host_children(unsigned long index, unsigned long *out, unsigned long maximum)
{
	unsigned long count = 0;
	long child_index;

	for (child_index = object_get((long)index)->object.first_child_object_index;
		child_index != NONE;
		child_index = object_get(child_index)->object.next_object_index)
	{
		if (count < maximum)
			out[count] = (unsigned long)child_index;
		count++;
	}

	return count;
}

/* a projectile's state, from the projectile */
static void halo_sim_projectile_state_load(long projectile_index, struct halo_sim_projectile_state *state)
{
	struct projectile_datum *projectile = projectile_get(projectile_index);

	state->position = projectile->object.position;
	state->velocity = projectile->object.translational_velocity;
	state->flags = projectile->projectile.flags;
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

/* and back to it: all but its place (position, location, parent), which
only the game's moves change */
static void halo_sim_projectile_state_store(long projectile_index, struct halo_sim_projectile_state const *state)
{
	struct projectile_datum *projectile = projectile_get(projectile_index);

	projectile->object.translational_velocity = state->velocity;
	projectile->projectile.flags = state->flags;
	projectile->object.flags = state->object_flags;
	projectile->projectile.action = state->action;
	projectile->projectile.hit_material_type = state->hit_material_type;
	projectile->projectile.ignore_object_index = state->ignore_object_index;
	projectile->projectile.target_object_index = state->target_object_index;
	projectile->projectile.detonation_timer = state->detonation_timer;
	projectile->projectile.detonation_timer_delta = state->detonation_timer_delta;
	projectile->projectile.arming_time = state->arming_time;
	projectile->projectile.arming_time_delta = state->arming_time_delta;
	projectile->projectile.odometer = state->odometer;
	projectile->projectile.deceleration_timer = state->deceleration_timer;
	projectile->projectile.deceleration_timer_delta = state->deceleration_timer_delta;
	projectile->projectile.deceleration = state->deceleration;
	projectile->projectile.maximum_damage_distance = state->maximum_damage_distance;
}

/* projectiles.c's, the game's side of a projectile update */
void projectile_rust_guidance(long projectile_index, real_point3d *projectile_center, real_point3d *target_center,
	real_point3d *target_center_of_mass, real *guidance_scale);
void projectile_rust_impact_damage(long projectile_index, struct collision_result const *collision, real damage_scale,
	real_vector3d const *new_velocity, short *material_type, real *material_effect_scale);
void projectile_rust_breakable_surface_damage(long projectile_index, struct collision_result const *collision,
	real_vector3d const *new_velocity);
void projectile_rust_effect(long projectile_index, short effect, short material_type, boolean potential,
	struct collision_result const *collision, real_vector3d const *direction, real effect_scale, real material_effect_scale);
void projectile_rust_combining_reset(long child_index);
void projectile_rust_attach(long projectile_index, real_point3d const *new_position, struct collision_result const *collision);
void projectile_rust_impact_noise(long projectile_index, real_point3d const *point);
void projectile_rust_moved(long projectile_index, real_point3d const *new_position, real_vector3d const *new_velocity,
	struct location const *location, real time_remaining, short collision_count, long ignore_object_index);

/* the Rust port's callbacks: the update's state written to the projectile,
the game's call, and the state read back */
static void halo_sim_projectile_timers_updated(unsigned long projectile_index, struct halo_sim_projectile_state *state)
{
	halo_sim_projectile_state_store((long)projectile_index, state);
	projectile_export_function_values((long)projectile_index);
	halo_sim_projectile_state_load((long)projectile_index, state);
}

static void halo_sim_projectile_guidance(unsigned long projectile_index, struct halo_sim_projectile_state *state,
	struct halo_sim_guidance *out)
{
	halo_sim_projectile_state_store((long)projectile_index, state);
	projectile_rust_guidance((long)projectile_index, &out->projectile_center, &out->target_center,
		&out->target_center_of_mass, &out->guidance_scale);
	halo_sim_projectile_state_load((long)projectile_index, state);
}

/* (for the bridge's recording: the seed a response begins with) */
static void halo_sim_projectile_collision_begin(unsigned long projectile_index, struct halo_sim_projectile_state *state)
{
	bevy_bridge_projectile_collision();
}

static void halo_sim_projectile_impact_damage(unsigned long projectile_index, struct halo_sim_projectile_state *state,
	struct collision_result const *collision, real damage_scale, real_vector3d const *velocity,
	short *material_type, real *material_effect_scale)
{
	halo_sim_projectile_state_store((long)projectile_index, state);
	projectile_rust_impact_damage((long)projectile_index, collision, damage_scale, velocity, material_type, material_effect_scale);
	halo_sim_projectile_state_load((long)projectile_index, state);
}

static void halo_sim_projectile_breakable_surface_damage(unsigned long projectile_index, struct halo_sim_projectile_state *state,
	struct collision_result const *collision, real_vector3d const *velocity)
{
	halo_sim_projectile_state_store((long)projectile_index, state);
	projectile_rust_breakable_surface_damage((long)projectile_index, collision, velocity);
	halo_sim_projectile_state_load((long)projectile_index, state);
}

static void halo_sim_projectile_effect(unsigned long projectile_index, struct halo_sim_projectile_state *state, long effect,
	long material_type, long potential, struct collision_result const *collision, real_vector3d const *direction,
	real scale, real material_effect_scale)
{
	halo_sim_projectile_state_store((long)projectile_index, state);
	projectile_rust_effect((long)projectile_index, (short)effect, (short)material_type, potential != 0, collision, direction,
		scale, material_effect_scale);
	halo_sim_projectile_state_load((long)projectile_index, state);
}

static void halo_sim_projectile_combining_reset(unsigned long child_index)
{
	projectile_rust_combining_reset((long)child_index);
}

static void halo_sim_projectile_attach(unsigned long projectile_index, struct halo_sim_projectile_state *state,
	real_point3d const *new_position, struct collision_result const *collision)
{
	halo_sim_projectile_state_store((long)projectile_index, state);
	projectile_rust_attach((long)projectile_index, new_position, collision);
	halo_sim_projectile_state_load((long)projectile_index, state);
}

static void halo_sim_projectile_impact_noise(unsigned long projectile_index, struct halo_sim_projectile_state *state,
	real_point3d const *point)
{
	halo_sim_projectile_state_store((long)projectile_index, state);
	projectile_rust_impact_noise((long)projectile_index, point);
	halo_sim_projectile_state_load((long)projectile_index, state);
}

static void halo_sim_projectile_moved(unsigned long projectile_index, struct halo_sim_projectile_state *state,
	real_point3d const *new_position, real_vector3d const *new_velocity, long location_leaf, long location_cluster,
	real time_remaining, long collision_count, long ignore_object_index)
{
	struct location location;

	location.leaf_index = location_leaf;
	location.cluster_index = (short)location_cluster;
	location.bonus = 0;
	halo_sim_projectile_state_store((long)projectile_index, state);
	projectile_rust_moved((long)projectile_index, new_position, new_velocity, &location, time_remaining,
		(short)collision_count, ignore_object_index);
	halo_sim_projectile_state_load((long)projectile_index, state);
}

static struct halo_sim_host const halo_sim_host_callbacks =
{
	halo_sim_host_cluster_objects,
	halo_sim_host_object,
	halo_sim_host_children,
	{
		halo_sim_projectile_timers_updated,
		halo_sim_projectile_guidance,
		halo_sim_projectile_collision_begin,
		halo_sim_projectile_impact_damage,
		halo_sim_projectile_breakable_surface_damage,
		halo_sim_projectile_effect,
		halo_sim_projectile_combining_reset,
		halo_sim_projectile_attach,
		halo_sim_projectile_impact_noise,
		halo_sim_projectile_moved,
	},
};

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
	halo_sim.pose_preprocess = (halo_sim_pose_preprocess_function)halo_sim_symbol(library, "halo_sim_pose_preprocess");
	halo_sim.set_host = (halo_sim_set_host_function)halo_sim_symbol(library, "halo_sim_set_host");
	halo_sim.collision_test_vector = (halo_sim_collision_test_vector_function)halo_sim_symbol(library, "halo_sim_collision_test_vector");
	halo_sim.projectile_update = (halo_sim_projectile_function)halo_sim_symbol(library, "halo_sim_projectile_update");
	halo_sim.projectile_check = (halo_sim_projectile_function)halo_sim_symbol(library, "halo_sim_projectile_check");
	if (!halo_sim.load_map || !halo_sim.point_physics || !halo_sim.set_tag_cache || !halo_sim.pose_orientations ||
		!halo_sim.pose_matrices || !halo_sim.pose_preprocess || !halo_sim.set_host || !halo_sim.collision_test_vector ||
		!halo_sim.projectile_update || !halo_sim.projectile_check)
	{
		platform_log("halo_sim: %s lacks an entry point; the game's own simulation runs", path);
		return;
	}
	halo_sim.loaded = TRUE;
	halo_sim.set_host(&halo_sim_host_callbacks);

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

static char const *const halo_sim_pose_stage_names[3] = { "orientations", "matrices", "preprocess" };

static void halo_sim_pose_report(short stage, long object_index, short node_count, short node_index)
{
	halo_sim_pose_checks.differed[stage]++;
	if (halo_sim_pose_checks.differed[stage] <= 8)
	{
		platform_log("halo_sim: pose %s of %s (object %08lx) differs at node %d of %d",
			halo_sim_pose_stage_names[stage], tag_get_name(object_get(object_index)->definition_index),
			(unsigned long)object_index, node_index, node_count);
	}
}

static void halo_sim_pose_tally(short stage)
{
	halo_sim_pose_checks.compared[stage]++;
	if ((halo_sim_pose_checks.compared[stage] & 0x3fff) == 0 || halo_sim_pose_checks.compared[stage] == 1)
	{
		platform_log("halo_sim: pose %s: %lu of %lu agree with the game", halo_sim_pose_stage_names[stage],
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

/* ---------- the object types' preprocess */

boolean unit_animation_enhanced(long unit_index); /* units.c */

static void halo_sim_pose_preprocess_gather(long object_index, short node_count, struct halo_sim_pose_preprocess_call *call)
{
	struct object_datum *object = object_get(object_index);

	memset(call, 0, sizeof(*call));
	call->object_index = (unsigned long)object_index;
	call->definition_index = (unsigned long)object->definition_index;
	call->object_type = object->object.type;
	call->node_count = node_count;
	call->forward = object->object.forward;
	call->up = object->object.up;
	call->translational_velocity = object->object.translational_velocity;
	call->game_time = game_time_get();
	if (TEST_FLAG(_object_mask_unit, object->object.type))
	{
		struct unit_datum *unit = unit_get(object_index);

		call->enhanced = unit_animation_enhanced(object_index) ? 1 : 0;
		call->airborne = unit_flying_through_air(object_index) ? 1 : 0;
		call->parented = object->object.parent_object_index != NONE;
		call->has_weapon = unit->unit.current_weapon_index != NONE;
		call->player = unit->unit.player_index != NONE;
		call->unit_animation_flags = unit->unit.animation.flags;
		call->aiming_screen_index = unit->unit.animation.aiming_screen_index;
		call->looking_screen_index = unit->unit.animation.looking_screen_index;
		call->override_emotion_animation_index = unit->unit.override_emotion_animation_index;
		call->seat_index = unit->unit.animation.seat_index;
		call->weapon_index = unit->unit.animation.weapon_index;
		call->state = unit->unit.animation.state;
		call->action = unit->unit.animation.action;
		call->base_seat_index = unit->unit.animation.base_seat_index;
		call->emotion_index = unit->unit.animation.emotion_index;
		call->action_animation[0] = unit->unit.animation.action_animation.index;
		call->action_animation[1] = unit->unit.animation.action_animation.frame_index;
		call->overlay_action_animation[0] = unit->unit.animation.overlay_action_animation.index;
		call->overlay_action_animation[1] = unit->unit.animation.overlay_action_animation.frame_index;
		call->soft_ping_animation[0] = unit->unit.animation.soft_ping_animation.index;
		call->soft_ping_animation[1] = unit->unit.animation.soft_ping_animation.frame_index;
		call->mouth_aperture = unit->unit.mouth_aperture;
		call->seat_acceleration = unit->unit.seat_acceleration;
		call->aiming_vector = unit->unit.aiming_vector;
		call->looking_vector = unit->unit.looking_vector;
		object_get_orientation(object_index, &call->orientation_forward, &call->orientation_up);
		unit_grenade_throw_leg_weights(object_index, &call->leg_weight, &call->leg_time, &call->air_weight, &call->air_time);
	}
	if (object->object.type == _object_type_vehicle)
	{
		struct vehicle_datum *vehicle = vehicle_datum_get(object_index);

		call->vehicle_turn = vehicle->vehicle.turn;
		call->vehicle_speed = vehicle->vehicle.speed;
		call->vehicle_wheel = vehicle->vehicle.wheel;
		memcpy(call->vehicle_suspension, vehicle->vehicle.suspension, sizeof(call->vehicle_suspension));
	}
	if (TEST_FLAG(_object_mask_device, object->object.type))
	{
		struct device_datum *device = device_get(object_index);

		call->device_flags = device->device.flags;
		call->device_position = device->device.position;
		call->device_power = device->device.power;
	}
}

int halo_sim_pose_preprocess(long object_index, short node_count, void *orientations)
{
	short mode = halo_sim_mode(_halo_sim_poses);
	struct halo_sim_pose_preprocess_call *call = &halo_sim_pose_checks.preprocess;

	halo_sim_pose_checks.preprocess_pending = FALSE;
	if (mode == _halo_sim_mode_c || node_count <= 0 || node_count > MAXIMUM_NODES_PER_MODEL)
		return FALSE;
	halo_sim_pose_preprocess_gather(object_index, node_count, call);
	if (mode == _halo_sim_mode_check)
	{
		memcpy(halo_sim_pose_checks.preprocessed, orientations, sizeof(real_orientation) * node_count);
		call->orientations = halo_sim_pose_checks.preprocessed;
	}
	else
	{
		call->orientations = (real_orientation *)orientations;
	}
	if (halo_sim.pose_preprocess(call) != 0)
	{
		static long failures = 0;

		if (failures++ < 5)
		{
			platform_log("halo_sim: the preprocess of %s (type %d) couldn't run in Rust",
				tag_get_name(object_get(object_index)->definition_index), call->object_type);
		}
		return FALSE;
	}
	if (mode == _halo_sim_mode_check)
	{
		halo_sim_pose_checks.preprocess_pending = TRUE;
		halo_sim_pose_checks.preprocess_object = object_index;
		return FALSE;
	}
	if (TEST_FLAG(_object_mask_unit, call->object_type))
	{
		struct unit_datum *unit = unit_get(object_index);

		unit->unit.animation.aiming_with_euler_screen = call->aiming_with_euler_screen ? TRUE : FALSE;
		unit->unit.animation.looking_with_euler_screen = call->looking_with_euler_screen ? TRUE : FALSE;
		if (call->aiming_bounds_set)
			unit->unit.animation.aiming_screen_bounds = call->aiming_screen_bounds;
		if (call->looking_bounds_set)
			unit->unit.animation.looking_screen_bounds = call->looking_screen_bounds;
		unit_grenade_throw_set_leg_weights(object_index, call->leg_weight, call->leg_time, call->air_weight, call->air_time);
	}

	return TRUE;
}

void halo_sim_pose_preprocess_check(long object_index, short node_count, void const *orientations)
{
	struct halo_sim_pose_preprocess_call const *call = &halo_sim_pose_checks.preprocess;
	real_orientation const *game = (real_orientation const *)orientations;
	short node_index;
	boolean same;

	if (!halo_sim_pose_checks.preprocess_pending || halo_sim_pose_checks.preprocess_object != object_index)
		return;
	halo_sim_pose_checks.preprocess_pending = FALSE;
	for (node_index = 0; node_index < node_count; node_index++)
	{
		if (memcmp(&game[node_index], &halo_sim_pose_checks.preprocessed[node_index], sizeof(real_orientation)) != 0)
			break;
	}
	same = node_index == node_count;
	if (same && TEST_FLAG(_object_mask_unit, call->object_type))
	{
		struct unit_datum *unit = unit_get(object_index);
		real leg_weight, air_weight;
		long leg_time, air_time;

		unit_grenade_throw_leg_weights(object_index, &leg_weight, &leg_time, &air_weight, &air_time);
		same = (unit->unit.animation.aiming_with_euler_screen != 0) == (call->aiming_with_euler_screen != 0) &&
			(unit->unit.animation.looking_with_euler_screen != 0) == (call->looking_with_euler_screen != 0) &&
			(!call->aiming_bounds_set ||
				memcmp(&unit->unit.animation.aiming_screen_bounds, &call->aiming_screen_bounds, sizeof(real_rectangle2d)) == 0) &&
			(!call->looking_bounds_set ||
				memcmp(&unit->unit.animation.looking_screen_bounds, &call->looking_screen_bounds, sizeof(real_rectangle2d)) == 0) &&
			memcmp(&leg_weight, &call->leg_weight, sizeof(real)) == 0 && leg_time == call->leg_time &&
			memcmp(&air_weight, &call->air_weight, sizeof(real)) == 0 && air_time == call->air_time;
	}
	if (!same)
		halo_sim_pose_report(2, object_index, node_count, node_index);
	halo_sim_pose_tally(2);
}

/* ---------- collision */

void halo_sim_collision_test_vector_check(unsigned long flags, void const *point, void const *vector,
	long ignore_object_index, int hit, void const *collision)
{
	struct collision_result const *game = (struct collision_result const *)collision;
	struct halo_sim_collision_vector_call call;
	struct halo_sim_collision_vector_result rust;
	boolean same;

	if (halo_sim_mode(_halo_sim_collision) == _halo_sim_mode_c)
		return;
	call.structure_bsp_index = global_structure_bsp_index;
	call.flags = flags;
	call.breakable_surfaces = (unsigned char const *)breakable_surface_flags_get();
	call.point = *(real_point3d const *)point;
	call.vector = *(real_vector3d const *)vector;
	call.ignore_object_index = (unsigned long)ignore_object_index;
	if (halo_sim.collision_test_vector(&call, &rust) != 0)
		return;

	same = (rust.hit != 0) == (hit != 0) &&
		rust.type == game->type &&
		memcmp(&rust.t, &game->t, sizeof(real)) == 0 &&
		memcmp(&rust.point, &game->point, sizeof(real_point3d)) == 0 &&
		rust.location_leaf == game->location.leaf_index &&
		rust.location_cluster == game->location.cluster_index;
	if (same && hit)
	{
		same = memcmp(&rust.plane, &game->plane, sizeof(real_plane3d)) == 0 &&
			rust.material_type == game->material_type;
		if (game->type == _collision_result_structure || game->type == _collision_result_object)
		{
			same = same && rust.surface_index == game->surface_index &&
				rust.surface_flags == game->flags &&
				rust.material_index == game->material_index &&
				rust.plane_designator == game->plane_designator &&
				rust.breakable_surface_index == game->breakable_surface_index;
		}
		if (game->type == _collision_result_object)
		{
			same = same && rust.object_index == (unsigned long)game->object_index &&
				rust.node_index == game->node_index &&
				rust.bsp_index == game->bsp_index &&
				rust.region_index == game->region_index;
		}
	}
	halo_sim_collision_checks.compared++;
	if (hit)
	{
		if (game->type == _collision_result_structure)
			halo_sim_collision_checks.structure_hits++;
		else if (game->type == _collision_result_media)
			halo_sim_collision_checks.media_hits++;
		else if (game->type == _collision_result_object)
		{
			halo_sim_collision_checks.object_hits++;
			if (game->node_index == NONE)
				halo_sim_collision_checks.vehicle_physics_hits++;
		}
	}
	if (!same)
	{
		halo_sim_collision_checks.differed++;
		if (halo_sim_collision_checks.differed <= 8)
		{
			platform_log("halo_sim: collision differs: flags %lx point %08lx %08lx %08lx vector %08lx %08lx %08lx ignore %lx",
				flags,
				*(unsigned long const *)&call.point.x, *(unsigned long const *)&call.point.y, *(unsigned long const *)&call.point.z,
				*(unsigned long const *)&call.vector.i, *(unsigned long const *)&call.vector.j, *(unsigned long const *)&call.vector.k,
				(unsigned long)ignore_object_index);
			platform_log("halo_sim:   game %d type %d t %08lx object %lx node %d bsp %d surface %ld material %d flags %x leaf %ld",
				hit, game->type, *(unsigned long const *)&game->t,
				hit && game->type == _collision_result_object ? (unsigned long)game->object_index : 0xfffffffful,
				game->node_index, game->bsp_index, game->surface_index, game->material_type, game->flags,
				game->location.leaf_index);
			platform_log("halo_sim:   rust %ld type %d t %08lx object %lx node %d bsp %d surface %ld material %d flags %x leaf %ld",
				rust.hit, rust.type, *(unsigned long const *)&rust.t, rust.object_index,
				rust.node_index, rust.bsp_index, rust.surface_index, rust.material_type, rust.surface_flags,
				rust.location_leaf);
		}
	}
	if ((halo_sim_collision_checks.compared & 0xffff) == 0)
	{
		platform_log("halo_sim: collision: %lu of %lu agree with the game (hits: %lu structure, %lu media, %lu objects, %lu of them vehicles' physics)",
			halo_sim_collision_checks.compared - halo_sim_collision_checks.differed,
			halo_sim_collision_checks.compared,
			halo_sim_collision_checks.structure_hits, halo_sim_collision_checks.media_hits,
			halo_sim_collision_checks.object_hits, halo_sim_collision_checks.vehicle_physics_hits);
	}
}

/* ---------- projectiles */

/* random_math.c's and periodic_functions.c's */
unsigned long get_random_seed(void);
unsigned long *get_global_random_seed_address(void);
short random_direction_table_get(real_vector3d const **table);
unsigned char const *periodic_function_table_get(short function_type);

/* projectiles.c's _periodic_function_wander */
#define HALO_SIM_PERIODIC_FUNCTION_WANDER 10

static void halo_sim_projectile_call_new(long projectile_index, struct halo_sim_projectile_call *call, unsigned long *random_seed)
{
	real_vector3d const *directions = NULL;

	memset(call, 0, sizeof(*call));
	call->projectile_index = (unsigned long)projectile_index;
	call->definition_index = (unsigned long)object_get(projectile_index)->definition_index;
	call->structure_bsp_index = global_structure_bsp_index;
	call->breakable_surfaces = (unsigned char const *)breakable_surface_flags_get();
	halo_sim_projectile_state_load(projectile_index, &call->state);
	call->random_seed = random_seed;
	call->direction_count = random_direction_table_get(&directions);
	call->directions = directions;
	call->wander_table = periodic_function_table_get(HALO_SIM_PERIODIC_FUNCTION_WANDER);
	call->game_time = game_time_get();
	call->difference_event = NONE;
}

int halo_sim_projectile_update(long projectile_index, short *collision_count, float *time_remaining)
{
	short mode = halo_sim_mode(_halo_sim_projectiles);
	struct halo_sim_projectile_call call;
	long result;

	halo_sim_projectile_checks.recording = FALSE;
	if (mode == _halo_sim_mode_c)
		return FALSE;
	if (mode == _halo_sim_mode_check)
	{
		halo_sim_projectile_state_load(projectile_index, &halo_sim_projectile_checks.before);
		halo_sim_projectile_checks.seed = get_random_seed();
		halo_sim_projectile_checks.projectile_index = projectile_index;
		halo_sim_projectile_checks.event_count = 0;
		halo_sim_projectile_checks.overflow = FALSE;
		halo_sim_projectile_checks.recording = TRUE;
		return FALSE;
	}

	halo_sim_projectile_call_new(projectile_index, &call, get_global_random_seed_address());
	halo_sim_projectile_updating = TRUE;
	result = halo_sim.projectile_update(&call);
	halo_sim_projectile_updating = FALSE;
	if (result == -1)
		return FALSE;
	halo_sim_projectile_state_store(projectile_index, &call.state);
	if (result != 0)
	{
		static long stops = 0;

		if (stops++ < 8)
			platform_log("halo_sim: projectile %lx's update stopped for an input it couldn't have", (unsigned long)projectile_index);
	}
	*collision_count = (short)call.collision_count;
	*time_remaining = call.time_remaining;

	return TRUE;
}

unsigned long halo_sim_projectile_seed(void)
{
	return halo_sim_projectile_checks.recording ? get_random_seed() : 0;
}

static struct halo_sim_projectile_event *halo_sim_projectile_event_new(short kind, unsigned long seed_before)
{
	struct halo_sim_projectile_event *event;

	if (!halo_sim_projectile_checks.recording)
		return NULL;
	if (halo_sim_projectile_checks.event_count >= MAXIMUM_HALO_SIM_PROJECTILE_EVENTS)
	{
		halo_sim_projectile_checks.overflow = TRUE;
		return NULL;
	}
	event = &halo_sim_projectile_checks.events[halo_sim_projectile_checks.event_count++];
	memset(event, 0, sizeof(*event));
	event->kind = kind;
	event->seed_before = seed_before;
	event->seed_after = get_random_seed();
	event->object_index = NONE;
	event->material_type = NONE;

	return event;
}

void halo_sim_projectile_event(short kind, unsigned long seed_before, long object_index, short material_type,
	int potential, float scale, float material_effect_scale)
{
	struct halo_sim_projectile_event *event = halo_sim_projectile_event_new(kind, seed_before);

	if (!event)
		return;
	event->object_index = object_index;
	event->material_type = material_type;
	event->potential = potential ? 1 : 0;
	event->scale = scale;
	event->material_effect_scale = material_effect_scale;
}

void halo_sim_projectile_event_guidance(void const *projectile_center, void const *target_center,
	void const *target_center_of_mass, float guidance_scale, long game_time)
{
	struct halo_sim_projectile_event *event = halo_sim_projectile_event_new(_halo_sim_projectile_event_guidance,
		halo_sim_projectile_seed());

	if (!event)
		return;
	event->guidance.projectile_center = *(real_point3d const *)projectile_center;
	event->guidance.target_center = *(real_point3d const *)target_center;
	event->guidance.target_center_of_mass = *(real_point3d const *)target_center_of_mass;
	event->guidance.guidance_scale = guidance_scale;
	event->game_time = game_time;
}

void halo_sim_projectile_event_state(short kind, unsigned long seed_before, long projectile_index)
{
	struct halo_sim_projectile_event *event = halo_sim_projectile_event_new(kind, seed_before);

	if (event)
		halo_sim_projectile_state_load(projectile_index, &event->state);
}

static boolean halo_sim_projectile_states_agree(struct halo_sim_projectile_state const *a, struct halo_sim_projectile_state const *b)
{
	return memcmp(&a->position, &b->position, sizeof(a->position)) == 0 &&
		memcmp(&a->velocity, &b->velocity, sizeof(a->velocity)) == 0 &&
		a->flags == b->flags &&
		a->object_flags == b->object_flags &&
		a->action == b->action &&
		a->hit_material_type == b->hit_material_type &&
		a->ignore_object_index == b->ignore_object_index &&
		a->target_object_index == b->target_object_index &&
		a->parent_object_index == b->parent_object_index &&
		memcmp(&a->detonation_timer, &b->detonation_timer, 9 * sizeof(real)) == 0 &&
		a->location_leaf == b->location_leaf &&
		a->location_cluster == b->location_cluster;
}

void halo_sim_projectile_check(long projectile_index, short collision_count, float time_remaining)
{
	struct halo_sim_projectile_call call;
	struct halo_sim_projectile_state game;
	unsigned long seed;
	boolean same;

	if (!halo_sim_projectile_checks.recording)
		return;
	halo_sim_projectile_checks.recording = FALSE;
	if (halo_sim_projectile_checks.projectile_index != projectile_index || halo_sim_projectile_checks.overflow)
	{
		halo_sim_projectile_checks.skipped++;
		return;
	}

	seed = halo_sim_projectile_checks.seed;
	halo_sim_projectile_call_new(projectile_index, &call, &seed);
	call.state = halo_sim_projectile_checks.before;
	call.events = halo_sim_projectile_checks.events;
	call.event_count = halo_sim_projectile_checks.event_count;
	if (halo_sim.projectile_check(&call) != 0)
	{
		halo_sim_projectile_checks.skipped++;
		return;
	}

	halo_sim_projectile_state_load(projectile_index, &game);
	same = halo_sim_projectile_states_agree(&call.state, &game) &&
		call.collision_count == collision_count &&
		memcmp(&call.time_remaining, &time_remaining, sizeof(real)) == 0 &&
		call.difference_event == NONE &&
		seed == get_random_seed();
	halo_sim_projectile_checks.compared++;
	halo_sim_projectile_checks.collisions += (unsigned long)collision_count;
	halo_sim_projectile_checks.events_compared += halo_sim_projectile_checks.event_count;
	if (!same)
	{
		halo_sim_projectile_checks.differed++;
		if (halo_sim_projectile_checks.differed <= 8)
		{
			struct halo_sim_projectile_state const *b = &halo_sim_projectile_checks.before;

			platform_log("halo_sim: projectile %lx (definition %lx) differs: call %ld of %lu (kind %d) reason %ld; collisions game %d rust %ld; time %08lx %08lx; seed game %08lx rust %08lx",
				(unsigned long)projectile_index, call.definition_index, call.difference_event,
				halo_sim_projectile_checks.event_count,
				call.difference_event >= 0 && call.difference_event < (long)halo_sim_projectile_checks.event_count ?
					halo_sim_projectile_checks.events[call.difference_event].kind : -1,
				call.difference_reason, collision_count, call.collision_count,
				*(unsigned long const *)&time_remaining, *(unsigned long const *)&call.time_remaining,
				get_random_seed(), seed);
			platform_log("halo_sim:   from (%f %f %f) velocity (%f %f %f) flags %lx action %d target %lx",
				b->position.x, b->position.y, b->position.z, b->velocity.i, b->velocity.j, b->velocity.k,
				b->flags, b->action, (unsigned long)b->target_object_index);
			platform_log("halo_sim:   game (%f %f %f) velocity (%f %f %f) flags %lx/%lx action %d leaf %ld parent %lx",
				game.position.x, game.position.y, game.position.z, game.velocity.i, game.velocity.j, game.velocity.k,
				game.flags, game.object_flags, game.action, game.location_leaf, (unsigned long)game.parent_object_index);
			platform_log("halo_sim:   rust (%f %f %f) velocity (%f %f %f) flags %lx/%lx action %d leaf %ld parent %lx",
				call.state.position.x, call.state.position.y, call.state.position.z,
				call.state.velocity.i, call.state.velocity.j, call.state.velocity.k,
				call.state.flags, call.state.object_flags, call.state.action, call.state.location_leaf,
				(unsigned long)call.state.parent_object_index);
		}
	}
	if ((halo_sim_projectile_checks.compared & 0x3ff) == 0 ||
		(halo_sim_projectile_checks.compared & (halo_sim_projectile_checks.compared - 1)) == 0)
	{
		platform_log("halo_sim: projectiles: %lu of %lu updates agree with the game (%lu collisions, %lu calls of the game; %lu left out)",
			halo_sim_projectile_checks.compared - halo_sim_projectile_checks.differed,
			halo_sim_projectile_checks.compared, halo_sim_projectile_checks.collisions,
			halo_sim_projectile_checks.events_compared, halo_sim_projectile_checks.skipped);
	}
}
