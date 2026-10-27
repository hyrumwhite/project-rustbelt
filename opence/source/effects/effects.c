/*
EFFECTS.C

symbols in this file:
0008ACC0 0050:
	_effects_initialize (0000)
0008AD10 0020:
	_effects_initialize_for_new_map (0000)
0008AD30 0020:
	_effects_dispose_from_old_map (0000)
0008AD50 0020:
	_effects_dispose (0000)
0008AD70 0050:
	_effects_object_is_corpse (0000)
0008ADC0 00c0:
	_effect_delete (0000)
0008AE80 0100:
	_effects_stop_on_first_person_weapon (0000)
0008AF80 0080:
	_effects_information_get (0000)
0008B000 0010:
	_effects_disconnect_from_structure_bsp (0000)
0008B010 0030:
	_effect_scale (0000)
0008B040 0020:
	_effect_get_random_seed (0000)
0008B060 0080:
	_effect_real_random_range (0000)
0008B0E0 0080:
	_effect_random_angular_velocity (0000)
0008B160 0080:
	_effect_allowed_by_environment (0000)
0008B1E0 0020:
	_effect_part_allowed_by_disposition (0000)
0008B200 0090:
	_effect_set_event (0000)
0008B290 0080:
	_effect_build_location (0000)
0008B310 00e0:
	_effect_location_get_next_instance (0000)
0008B3F0 0050:
	_effect_get_node_matrix (0000)
0008B440 00b0:
	_effect_evaluate_function_integral (0000)
0008B4F0 0010:
	_real_local_random (0000)
0008B500 0020:
	_local_random_direction3d (0000)
0008B520 0090:
	_effect_stop (0000)
0008B5B0 00c0:
	_effects_reconnect_to_structure_bsp (0000)
0008B670 01f0:
	_dangerous_effects_near_player (0000)
0008B860 00e0:
	_effect_random_translational_velocity (0000)
0008B940 0160:
	_effect_allocate (0000)
0008BAA0 00b0:
	_impulse_effect_initialize (0000)
0008BB50 00b0:
	_effect_build_locations (0000)
0008BC00 0760:
	_effect_generate_particles (0000)
0008C360 03e0:
	_effect_generate_part (0000)
0008C740 00b0:
	_effects_start_on_first_person_weapon (0000)
0008C7F0 0190:
	_effect_marker_list_get_marker (0000)
0008C980 0250:
	_effect_generate_parts (0000)
0008CBD0 00b0:
	_effect_marker_list_get_markers_by_name (0000)
0008CC80 0530:
	_effect_update (0000)
0008D1B0 00f0:
	_effect_new_looping (0000)
0008D2A0 0210:
	_effect_new_from_object (0000)
0008D4B0 02a0:
	_effect_new_attached_from_markers (0000)
0008D750 0240:
	_effect_new_unattached_from_markers (0000)
0008D990 0090:
	_effects_update (0000)
0025A424 000f:
	??_C@_0P@FBPJPKBN@effects_update?$AA@ (0000)
0025A434 0021:
	??_C@_0CB@HMDGHGFM@couldn?8t?5allocate?5effect?5globals@ (0000)
0025A458 0010:
	??_C@_0BA@DBNCDKDD@effect?5location?$AA@ (0000)
0025A468 0007:
	??_C@_06HOPIODND@effect?$AA@ (0000)
0025A470 0021:
	??_C@_0CB@KCIIEPGL@c?3?2halo?2SOURCE?2effects?2effects?4c@ (0000)
0025A494 0015:
	??_C@_0BF@IACBPFOC@location_datum_index?$AA@ (0000)
0025A4AC 0013:
	??_C@_0BD@IEHBIGCF@effect_index?$CB?$DNNONE?$AA@ (0000)
0025A4C0 004e:
	??_C@_0EO@BKJIDONM@cannot?5create?5objects?0?5lights?0?5o@ (0000)
0025A510 000f:
	??_C@_0P@HFFHMHLJ@?$CGeffect?9?$DOcolor?$AA@ (0000)
0025A520 000f:
	??_C@_0P@MDAIOCAG@count_delta?$DO?$DN0?$AA@ (0000)
0025A530 001c:
	??_C@_0BM@HEALEJFA@effect?5?$CFs?5has?5a?5bad?5part?5?$CFs?$AA@ (0000)
0025A54C 0021:
	??_C@_0CB@LPDGPLO@effect?9?$DOlocal_player_index?$DN?$DNNONE@ (0000)
0025A570 0009:
	??_C@_08FLABJHKG@?$CGforward?$AA@ (0000)
0025A57C 002c:
	??_C@_0CM@MOGGIEEI@?$CGmarker_list?9?$DOforwards?$FLeffect_ma@ (0000)
0025A5A8 0041:
	??_C@_0EB@KCJAOKOI@effect_marker_index?$DO?$DN0?5?$CG?$CG?5effect@ (0000)
0025A5EC 001b:
	??_C@_0BL@MLPMKFCB@created?5?$CFsviolent?5?$CFs?5on?5?$CFs?$AA@ (0000)
0025A608 0004:
	??_C@_03LNLHGKOH@non?$AA@ (0000)
0025A60C 0018:
	??_C@_0BI@JECIIDID@scale_b?5?$CFf?5not?5in?5?$FL0?01?$FN?$AA@ (0000)
0025A624 0018:
	??_C@_0BI@HOKOFOOB@scale_a?5?$CFf?5not?5in?5?$FL0?01?$FN?$AA@ (0000)
0025A63C 0010:
	??_C@_0BA@BNLBDOE@marker_forwards?$AA@ (0000)
0025A64C 000e:
	??_C@_0O@BFOEKDJK@marker_points?$AA@ (0000)
0025A65C 000f:
	??_C@_0P@DBOLECLB@marker_count?$DO0?$AA@ (0000)
0025A670 0047:
	??_C@_0EH@MLPNNEFC@?$CBtranslational_velocity?5?$HM?$HM?5valid@ (0000)
002DD1A0 0600:
	_effects_corpse_nonviolent (0000)
0043D584 0005:
	_global_effect_marker_list (0000)
	_debug_effects_nonviolent (0004)
*/

/* ---------- headers */

#define real_local_random real_local_random_inline
#include "cseries/cseries.h"
#include "cseries/errors.h"
#include "bitmaps/bitmaps.h"
#include "cseries/profile.h"
#include "editor/editor_stubs.h"
#include "effects/decal_definitions.h"
#include "effects/decals.h"
#include "effects/effect_definitions.h"
#include "effects/effects.h"
#include "effects/particle_system_definitions.h"
#include "effects/particle_systems.h"
#include "effects/particles.h"
#include "game/game.h"
#include "game/players.h"
#include "interface/first_person_weapons.h"
#include "memory/data.h"
#include "math/real_math.h"
#include "game/game_globals.h"
#include "objects/damage.h"
#include "objects/damage_effect_definitions.h"
#include "objects/light_definitions.h"
#include "objects/object_definitions.h"
#include "objects/object_lights.h"
#include "objects/objects.h"
#include "saved games/game_state.h"
#include "scenario/scenario.h"
#include "sound/game_sound.h"
#include "sound/sound_definitions.h"
#include "tag_files/tag_files.h"
#include "tag_files/tag_groups.h"
#include "units/bipeds.h"
#include "units/units.h"
#include "halo_sim.h" /* port: port/linux/game/halo_sim.c */
#undef real_local_random

/* ---------- constants */

enum effect_flags
{
	_effect_in_duration_bit = 0,
	_effect_loop_bit,
	_effect_stopping_bit,
	_effect_stopped_bit,
	_effect_invisible_bit,
	_effect_delete_on_stop_bit,
	_effect_nonviolent_bit,
	NUMBER_OF_EFFECT_FLAGS
};

enum
{
	MAXIMUM_EFFECT_INSTANCES = 16,
	MAXIMUM_EFFECT_EVENTS_PER_UPDATE = 8,
	CORPSE_SETTLING_TIME = 30,
	MAXIMUM_SPLIT_SCREEN_PARTICLE_COUNT = 6
};

/* an effect location's node designator packs the object's node index in its low bits
and sets this bit when the location lives on the first person weapon */
enum
{
	_effect_location_first_person_bit = 15
};

enum effect_camera_mode
{
	_effect_camera_mode_independent_of_camera_mode = 0,
	_effect_camera_mode_first_person_only,
	_effect_camera_mode_third_person_only,
	_effect_camera_mode_both,
	NUMBER_OF_EFFECT_CAMERA_MODES
};

enum effect_part_definition_flags
{
	_effect_part_definition_world_down_bit = 0,
	NUMBER_OF_EFFECT_PART_DEFINITION_FLAGS
};

enum effect_part_type_scale_flags
{
	_effect_part_type_scale_bit = 5,
	NUMBER_OF_EFFECT_PART_SCALEABLE_VALUES
};

enum effect_particle_definition_flags
{
	_effect_particle_attached_bit = 0,
	_effect_particle_random_orientation_bit,
	_effect_particle_tint_from_change_color_bit,
	_effect_particle_tint_interpolate_hsv_bit,
	_effect_particle_tint_do_it_the_hard_way_bit,
	NUMBER_OF_EFFECT_PARTICLE_DEFINITION_FLAGS
};

enum effect_particle_scale_flags
{
	_effect_particle_count_bit = 5,
	_effect_particle_count_delta_bit,
	_effect_particle_distribution_radius_bit,
	_effect_particle_distribution_radius_delta_bit,
	_effect_particle_radius_bit,
	_effect_particle_radius_delta_bit,
	_effect_particle_tint_bit,
	NUMBER_OF_EFFECT_PARTICLES_SCALEABLE_VALUES
};

enum effect_environment
{
	_effect_environment_anywhere = 0,
	_effect_environment_air,
	_effect_environment_water,
	_effect_environment_vacuum,
	NUMBER_OF_EFFECT_ENVIRONMENTS
};

enum effect_disposition
{
	_effect_disposition_agnostic = 0,
	_effect_disposition_violent,
	_effect_disposition_nonviolent,
	NUMBER_OF_EFFECT_DISPOSITIONS
};

enum effect_particle_distribution_function
{
	_effect_particle_distribution_function_start = 0,
	_effect_particle_distribution_function_end,
	_effect_particle_distribution_function_constant,
	_effect_particle_distribution_function_buildup,
	_effect_particle_distribution_function_falloff,
	_effect_particle_distribution_function_quadratic,
	NUMBER_OF_EFFECT_PARTICLE_DISTRIBUTION_FUNCTIONS
};

enum effect_velocity_scaleable_values_flags
{
	_effect_velocity_bit = 0,
	_effect_velocity_delta_bit,
	_effect_velocity_cone_bit,
	_effect_angular_velocity_bit,
	_effect_angular_velocity_delta_bit,
	NUMBER_OF_EFFECT_VELOCITY_SCALEABLE_VALUES
};

enum effect_definition_flags
{
	_effect_definition_deleted_when_inactive_bit = 0,
	_effect_definition_must_be_deterministic_bit,
	NUMBER_OF_EFFECT_DEFINITION_FLAGS
};

/* ---------- macros */

#define match_assert_valid_real_rgb_color(file, line, rgb) \
	match_vassert( \
		file, \
		line, \
		valid_real_rgb_color(rgb), \
		csprintf( \
			temporary, \
			"%s: assert_valid_real_rgb_color(%f, %f, %f)", \
			#rgb, \
			(*rgb).red, \
			(*rgb).green, \
			(*rgb).blue))

#define effect_get(index) ((struct effect_datum *)datum_get(effect_data, (index)))
#define effect_try_and_get(index) ((struct effect_datum *)datum_try_and_get(effect_data, (index)))
#define effect_location_get(index) ((struct effect_location_datum *)datum_get(effect_location_data, (index)))
#define effect_definition_get(index) ((struct effect_definition *)tag_get(EFFECT_DEFINITION_TAG, (index)))

/* ---------- structures */

struct effect_datum_header
{
	short identifier;
	word flags;
};

struct effect_vector_field
{
	void *user_data;
	void (*translational_function)(
		real_vector3d *velocity,
		real_point3d const *position,
		void *user_data);
	void (*angular_function)(
		real_vector3d *velocity,
		real_point3d const *position,
		void *user_data);
};

struct effect_datum
{
	struct effect_datum_header header;
	long definition_index;
	short scale_a_function_index;
	short scale_b_function_index;
	short change_color_index;
	short unused0e;
	struct location location;
	real_rgb_color color;
	real_vector3d velocity;
	struct effect_vector_field impulse_field;
	long object_index;
	long owner_object_index;
	real scale_a;
	real scale_b;
	short local_player_index;
	short event_index;
	real event_time;
	real event_duration;
	real last_event_fraction;
	long location_datum_indices[MAXIMUM_EFFECT_LOCATIONS];
	byte particle_counts[MAXIMUM_EFFECT_EVENTS];
};

struct effect_marker_list
{
	short node_index;
	real_matrix4x3 const *node_matrix;
	short count;
	char const **names;
	real_point3d const *points;
	real_vector3d const *forwards;
};

typedef char effect_marker_list_size_assert[
	sizeof(struct effect_marker_list) == 0x18 ? 1 : -1];
typedef char effect_marker_list_names_offset_assert[
	offsetof(struct effect_marker_list, names) == 0x0C ? 1 : -1];


struct effect_particles_definition
{
	short environment;
	short disposition;
	short camera_mode;
	short unused006;
	short location_index;
	short unused00a;
	real_euler_angles2d direction;
	real_vector3d offset;
	real_vector3d runtime_direction;
	long unused02c[10];
	struct tag_reference particle;
	unsigned long flags;
	short distribution_function;
	short unused06a;
	short count_lower_bound;
	short count_upper_bound;
	real distribution_radius_lower_bound;
	real distribution_radius_upper_bound;
	long unused078[3];
	real velocity_lower_bound;
	real velocity_upper_bound;
	real velocity_cone_angle;
	real angular_velocity_lower_bound;
	real angular_velocity_upper_bound;
	long unused098[2];
	real radius_lower_bound;
	real radius_upper_bound;
	long unused0a8[2];
	real_argb_color tint_lower_bound;
	real_argb_color tint_upper_bound;
	long unused0d0[4];
	unsigned long scale_a_flags;
	unsigned long scale_b_flags;
};

typedef char effect_particles_definition_size_assert[
	sizeof(struct effect_particles_definition) == 0xE8 ? 1 : -1];
typedef char effect_particles_definition_count_offset_assert[
	offsetof(struct effect_particles_definition, count_lower_bound) == 0x6C ? 1 : -1];
typedef char effect_particles_definition_scales_offset_assert[
	offsetof(struct effect_particles_definition, scale_a_flags) == 0xE0 ? 1 : -1];

struct effect_location_definition
{
	char marker_name[32];
};

struct effect_location_datum
{
	short identifier;
	short node_designator;
	long next_instance_location_index;
	real_matrix4x3 matrix;
};

typedef char effect_datum_header_size_assert[
	sizeof(struct effect_datum_header) == 0x4 ? 1 : -1];
typedef char effects_information_size_assert[
	sizeof(struct effects_information) == 0x6 ? 1 : -1];
typedef char effect_datum_location_offset_assert[
	offsetof(struct effect_datum, location) == 0x10 ? 1 : -1];
typedef char effect_datum_object_index_offset_assert[
	offsetof(struct effect_datum, object_index) == 0x3C ? 1 : -1];
typedef char effect_datum_location_indices_offset_assert[
	offsetof(struct effect_datum, location_datum_indices) == 0x5C ? 1 : -1];
typedef char effect_datum_size_assert[
	sizeof(struct effect_datum) == 0xFC ? 1 : -1];
typedef char effect_location_datum_matrix_offset_assert[
	offsetof(struct effect_location_datum, matrix) == 0x08 ? 1 : -1];
typedef char effect_location_datum_size_assert[
	sizeof(struct effect_location_datum) == 0x3C ? 1 : -1];
typedef char effect_definition_size_assert[
	sizeof(struct effect_definition) == 0x40 ? 1 : -1];
typedef char effect_definition_locations_offset_assert[
	offsetof(struct effect_definition, locations) == 0x28 ? 1 : -1];
typedef char effect_definition_events_offset_assert[
	offsetof(struct effect_definition, events) == 0x34 ? 1 : -1];
typedef char effect_event_definition_size_assert[
	sizeof(struct effect_event_definition) == 0x44 ? 1 : -1];

/* ---------- prototypes */

static struct effect_location_datum *effect_location_get_next_instance(
	struct effect_datum const *effect,
	long *location_datum_index,
	short camera_mode);
// effect_marker_list_get_marker: the marker list is passed in ebx and the index in ax
static void effect_marker_list_get_marker(
	struct object_marker *marker,
	struct effect_marker_list const *marker_list,
	short effect_marker_index);

static short effect_marker_list_get_markers_by_name(
	long object_index,
	char const *name,
	struct object_marker *markers,
	short maximum_marker_count);

// effects_object_is_corpse: object_index is passed in eax
static boolean effects_object_is_corpse(
	long object_index);

// effect_allocate: definition_index is passed in ebx
static long effect_allocate(
	long definition_index,
	long owner_object_index,
	boolean can_be_deterministic);

// impulse_effect_initialize: effect is passed in ecx, color in eax, impulse_field in ebx
static void impulse_effect_initialize(
	long effect_index,
	struct effect_datum *effect,
	real scale_a,
	real scale_b,
	real_rgb_color const *color,
	struct effect_vector_field const *impulse_field);

// effect_generate_part: the part definition and the location instance are passed in registers
static void effect_generate_part(
	long effect_index,
	struct effect_datum const *effect,
	struct effect_part_definition const *part_definition,
	struct effect_location_datum const *instance,
	real_point3d const *world_point,
	real_vector3d const *world_forward,
	real_vector3d const *world_up,
	real scale);

// effect_generate_parts: effect is passed in ebx
static void effect_generate_parts(
	long effect_index,
	struct effect_datum *effect);

// effect_generate_particles: effect is passed in edi
static void effect_generate_particles(
	long effect_index,
	struct effect_datum *effect);

static void effect_update(
	long effect_index,
	real dt);
static void effect_update_game(
	long effect_index,
	real dt);

// effect_scale: effect is passed in edx
static real effect_scale(
	struct effect_datum const *effect,
	real value,
	unsigned long scale_a_flags,
	unsigned long scale_b_flags,
	short bit_index);

// effect_real_random_range: effect, both scale masks and the bit index are passed in registers
static real effect_real_random_range(
	unsigned long *seed,
	struct effect_datum const *effect,
	real lower_bound,
	real upper_bound,
	unsigned long scale_a_flags,
	unsigned long scale_b_flags,
	short first_bit_index);

// effect_random_translational_velocity: effect is passed in eax
static void effect_random_translational_velocity(
	unsigned long *seed,
	struct effect_datum const *effect,
	real_vector3d const *forward,
	real_vector3d *direction,
	real_vector3d *velocity,
	real lower_bound,
	real upper_bound,
	real cone_angle,
	unsigned long scale_a_flags,
	unsigned long scale_b_flags);

// effect_random_angular_velocity: effect and the b scale mask are passed in registers
static void effect_random_angular_velocity(
	unsigned long *seed,
	struct effect_datum const *effect,
	real_vector3d *velocity,
	real lower_bound,
	real upper_bound,
	unsigned long scale_a_flags,
	unsigned long scale_b_flags);

// effect_allowed_by_environment: all three arguments are passed in registers
static boolean effect_allowed_by_environment(
	long effect_index,
	short environment,
	real_point3d const *world_point);

// effect_part_allowed_by_disposition: both arguments are passed in registers
static boolean effect_part_allowed_by_disposition(
	boolean nonviolent,
	short disposition);

// effect_get_node_matrix: effect is passed in ecx and the designator in ax
static real_matrix4x3 *effect_get_node_matrix(
	struct effect_datum const *effect,
	short node_designator);

static real effect_evaluate_function_integral(
	short function_index,
	real fraction);

/* port: where effect_build_locations takes its markers (January passes the
getter itself) */
enum
{
	_effect_markers_object = 0,
	_effect_markers_first_person_weapon,
	_effect_markers_list,
};

// effect_build_locations: effect is passed in ebx
static void effect_build_locations(
	long effect_index,
	struct effect_datum *effect,
	short source);

static unsigned long *effect_get_random_seed(
	long definition_index);

/* port: an effect's calls of the game (effect_call) */
static void effect_call_new(
	struct halo_sim_effect_call *call,
	long kind);
static void effect_call_vector(
	float *f,
	real const *v);
/* random_math.c's (as halo_sim.c declares it) */
short random_direction_table_get(real_vector3d const **table);
static void effect_set_event(
	long effect_index,
	short event_index);

/* ---------- globals */

extern struct data_array *effect_data;
struct data_array *effect_location_data;

boolean effects_corpse_nonviolent = TRUE;

boolean debug_effects_nonviolent = FALSE;

/* the effect marker list the effect_new_*_from_markers entry points hand to
effect_marker_list_get_markers_by_name (January's own name for this static is unrecovered) */
static struct effect_marker_list const *global_effect_marker_list;

static struct profile_section effects_update_section = {"effects_update", NONE, TRUE};

/* ---------- public code */

void effects_initialize(
	void)
{
	/* the native builds' larger effect pools (halo_port_capacity.h); a full
	pool drops deterministic effects, damage included */
	effect_data = game_state_data_new("effect", HALO_PORT_MAXIMUM_EFFECTS, 0xFC);
	effect_location_data = game_state_data_new("effect location", HALO_PORT_MAXIMUM_EFFECT_LOCATIONS, 0x3C);
	if (!effect_data || !effect_location_data)
		error(_error_immediate, "couldn't allocate effect globals");

	return;
}

void effects_initialize_for_new_map(
	void)
{
	data_make_valid(effect_data);
	data_make_valid(effect_location_data);

	return;
}

void effects_dispose_from_old_map(
	void)
{
	data_make_invalid(effect_data);
	data_make_invalid(effect_location_data);

	return;
}

void effects_dispose(
	void)
{
	if (effect_data)
		effect_data = NULL;
	if (effect_location_data)
		effect_location_data = NULL;

	return;
}

void effect_delete(
	long effect_index)
{
	struct effect_datum *effect = effect_try_and_get(effect_index);

	if (effect)
	{
		struct effect_definition *definition =
			effect_definition_get(effect->definition_index);
		short location_index;
		/* port: no more locations than the effect holds (the map's count) */
		short location_count = (short)MIN(
			definition->locations.count,
			(long)NUMBEROF(effect->location_datum_indices));

		for (location_index = 0;
			location_index < location_count;
			location_index++)
		{
			struct effect_location_datum *location;
			long location_datum_index;

			/* datum_delete only clears the header identifier in the fixed data array;
			January therefore reads the still-backed link while advancing the loop. */
			for (location_datum_index = effect->location_datum_indices[location_index];
				location_datum_index != NONE;
				location_datum_index = location->next_instance_location_index)
			{
				location = effect_location_get(location_datum_index);
				datum_delete(effect_location_data, location_datum_index);
			}
		}

		datum_delete(effect_data, effect_index);
	}

	return;
}

/* port: an effect_new_*'s arguments for the Rust port (halo_sim.c), which
runs it in place of the game's when asked; in check mode the game's runs
and is compared */
static void effect_new_call_new(
	struct halo_sim_effect_new_call *call,
	long kind,
	long definition_index,
	long owner_object_index,
	long object_index,
	real scale_a,
	real scale_b,
	real_rgb_color const *color,
	struct effect_vector_field const *impulse_field)
{
	real_vector3d const *directions = NULL;

	memset(call, 0, sizeof(*call));
	call->kind = kind;
	call->definition_index = definition_index;
	call->owner_object_index = owner_object_index;
	call->object_index = object_index;
	call->node_index = NONE;
	call->force_local_player_index = NONE;
	call->scale_a_function_index = NONE;
	call->scale_b_function_index = NONE;
	call->change_color_index = NONE;
	call->scale_a = scale_a;
	call->scale_b = scale_b;
	if (color)
	{
		call->has_color = TRUE;
		call->color[0] = color->red;
		call->color[1] = color->green;
		call->color[2] = color->blue;
	}
	call->impulse_field = (unsigned long)impulse_field;
	call->can_be_deterministic = TRUE;
	call->game_in_editor = game_in_editor() ? 1 : 0;
	call->corpse_nonviolent = effects_corpse_nonviolent ? 1 : 0;
	call->local_player_count = local_player_count();
	call->direction_count = random_direction_table_get(&directions);
	call->directions = directions;
	call->state.effect_index = (unsigned long)NONE;
	effect_halo_sim_state_load(NONE, &call->state);
	call->result = NONE;
}

static void effect_new_call_markers(
	struct halo_sim_effect_new_call *call,
	short marker_count,
	char const **marker_names,
	real_point3d const *marker_points,
	real_vector3d const *marker_forwards)
{
	call->marker_count = marker_count;
	call->marker_names = marker_names;
	call->marker_points = (float const *)marker_points;
	call->marker_forwards = (float const *)marker_forwards;
}

static long effect_new_attached_from_markers_game(
	long definition_index,
	long owner_object_index,
	long object_index,
	short node_index,
	short marker_count,
	char const **marker_names,
	real_point3d const *marker_points,
	real_vector3d const *marker_forwards,
	real scale_a,
	real scale_b,
	real_rgb_color const *color,
	struct effect_vector_field const *impulse_field);
static long effect_new_unattached_from_markers_game(
	long definition_index,
	long owner_object_index,
	real_vector3d const *translational_velocity,
	short marker_count,
	char const **marker_names,
	real_point3d const *marker_points,
	real_vector3d const *marker_forwards,
	real scale_a,
	real scale_b,
	real_rgb_color const *color,
	struct effect_vector_field const *impulse_field,
	boolean can_be_deterministic);
static long effect_new_from_object_game(
	long definition_index,
	long owner_object_index,
	long object_index,
	short force_local_player_index,
	real scale_a,
	real scale_b,
	real_rgb_color const *color,
	struct effect_vector_field const *impulse_field);
static long effect_new_looping_game(
	long definition_index,
	long object_index,
	short scale_a_function_index,
	short scale_b_function_index,
	short change_color_index);

long effect_new_attached_from_markers(
	long definition_index,
	long owner_object_index,
	long object_index,
	short node_index,
	short marker_count,
	char const **marker_names,
	real_point3d const *marker_points,
	real_vector3d const *marker_forwards,
	real scale_a,
	real scale_b,
	real_rgb_color const *color,
	struct effect_vector_field const *impulse_field)
{
	struct halo_sim_effect_new_call call;
	int checking = FALSE;
	long effect_index;

	effect_new_call_new(&call, _effect_new_attached_from_markers, definition_index, owner_object_index,
		object_index, scale_a, scale_b, color, impulse_field);
	call.node_index = node_index;
	effect_new_call_markers(&call, marker_count, marker_names, marker_points, marker_forwards);
	if (!debug_effects_nonviolent && halo_sim_effect_new(&call, &checking))
		return call.result;
	effect_index = effect_new_attached_from_markers_game(definition_index, owner_object_index, object_index,
		node_index, marker_count, marker_names, marker_points, marker_forwards, scale_a, scale_b, color,
		impulse_field);
	if (checking)
		halo_sim_effect_new_check(effect_index);

	return effect_index;
}

long effect_new_unattached_from_markers(
	long definition_index,
	long owner_object_index,
	real_vector3d const *translational_velocity,
	short marker_count,
	char const **marker_names,
	real_point3d const *marker_points,
	real_vector3d const *marker_forwards,
	real scale_a,
	real scale_b,
	real_rgb_color const *color,
	struct effect_vector_field const *impulse_field,
	boolean can_be_deterministic)
{
	struct halo_sim_effect_new_call call;
	int checking = FALSE;
	long effect_index;

	effect_new_call_new(&call, _effect_new_unattached_from_markers, definition_index, owner_object_index, NONE,
		scale_a, scale_b, color, impulse_field);
	if (translational_velocity)
	{
		call.has_velocity = TRUE;
		call.velocity[0] = translational_velocity->i;
		call.velocity[1] = translational_velocity->j;
		call.velocity[2] = translational_velocity->k;
	}
	call.can_be_deterministic = can_be_deterministic ? 1 : 0;
	effect_new_call_markers(&call, marker_count, marker_names, marker_points, marker_forwards);
	if (!debug_effects_nonviolent && halo_sim_effect_new(&call, &checking))
		return call.result;
	effect_index = effect_new_unattached_from_markers_game(definition_index, owner_object_index,
		translational_velocity, marker_count, marker_names, marker_points, marker_forwards, scale_a, scale_b,
		color, impulse_field, can_be_deterministic);
	if (checking)
		halo_sim_effect_new_check(effect_index);

	return effect_index;
}

long effect_new_from_object(
	long definition_index,
	long owner_object_index,
	long object_index,
	short force_local_player_index,
	real scale_a,
	real scale_b,
	real_rgb_color const *color,
	struct effect_vector_field const *impulse_field)
{
	struct halo_sim_effect_new_call call;
	int checking = FALSE;
	long effect_index;

	effect_new_call_new(&call, _effect_new_from_object, definition_index, owner_object_index, object_index,
		scale_a, scale_b, color, impulse_field);
	call.force_local_player_index = force_local_player_index;
	if (!debug_effects_nonviolent && halo_sim_effect_new(&call, &checking))
		return call.result;
	effect_index = effect_new_from_object_game(definition_index, owner_object_index, object_index,
		force_local_player_index, scale_a, scale_b, color, impulse_field);
	if (checking)
		halo_sim_effect_new_check(effect_index);

	return effect_index;
}

long effect_new_looping(
	long definition_index,
	long object_index,
	short scale_a_function_index,
	short scale_b_function_index,
	short change_color_index)
{
	struct halo_sim_effect_new_call call;
	int checking = FALSE;
	long effect_index;

	effect_new_call_new(&call, _effect_new_looping, definition_index, object_index, object_index, 0.0f, 0.0f,
		NULL, NULL);
	call.scale_a_function_index = scale_a_function_index;
	call.scale_b_function_index = scale_b_function_index;
	call.change_color_index = change_color_index;
	if (!debug_effects_nonviolent && halo_sim_effect_new(&call, &checking))
		return call.result;
	effect_index = effect_new_looping_game(definition_index, object_index, scale_a_function_index,
		scale_b_function_index, change_color_index);
	if (checking)
		halo_sim_effect_new_check(effect_index);

	return effect_index;
}

/* port: effects_object_is_corpse, through effect_call */
static boolean effect_call_corpse(
	long effect_index,
	long object_index)
{
	struct halo_sim_effect_call call;

	effect_call_new(&call, _effect_call_corpse);
	call.a = object_index;
	effect_call(effect_index, &call);
	return call.result != 0;
}

/* port: first_person_weapon_get_local_index, through effect_call */
static short effect_call_local_index(
	long effect_index,
	long object_index)
{
	struct halo_sim_effect_call call;

	effect_call_new(&call, _effect_call_local_index);
	call.a = object_index;
	effect_call(effect_index, &call);
	return (short)call.result;
}

static void effect_call_clear_locations(
	long effect_index)
{
	struct halo_sim_effect_call call;

	effect_call_new(&call, _effect_call_clear_locations);
	effect_call(effect_index, &call);
}

static long effect_new_attached_from_markers_game(
	long definition_index,
	long owner_object_index,
	long object_index,
	short node_index,
	short marker_count,
	char const **marker_names,
	real_point3d const *marker_points,
	real_vector3d const *marker_forwards,
	real scale_a,
	real scale_b,
	real_rgb_color const *color,
	struct effect_vector_field const *impulse_field)
{
	long effect_index;

	match_assert(
		"c:\\halo\\SOURCE\\effects\\effects.c",
		375,
		object_index!=NONE);
	match_assert(
		"c:\\halo\\SOURCE\\effects\\effects.c",
		377,
		marker_count>0);
	match_assert(
		"c:\\halo\\SOURCE\\effects\\effects.c",
		379,
		marker_points);
	match_assert(
		"c:\\halo\\SOURCE\\effects\\effects.c",
		380,
		marker_forwards);
	match_vassert(
		"c:\\halo\\SOURCE\\effects\\effects.c",
		381,
		scale_a>=0.f && scale_a<=1.f,
		csprintf(temporary, "scale_a %f not in [0,1]", scale_a));
	match_vassert(
		"c:\\halo\\SOURCE\\effects\\effects.c",
		382,
		scale_b>=0.f && scale_b<=1.f,
		csprintf(temporary, "scale_b %f not in [0,1]", scale_b));

	effect_index = effect_allocate(definition_index, owner_object_index, TRUE);
	if (effect_index != NONE)
	{
		struct effect_datum *effect = effect_get(effect_index);
		struct effect_marker_list marker_list;
		struct halo_sim_effect_call call;
		real_matrix4x3 node_matrix;

		impulse_effect_initialize(effect_index, effect, scale_a, scale_b, color, impulse_field);

		effect->object_index = object_index;

		if (effects_corpse_nonviolent &&
			effect_call_corpse(effect_index, effect->object_index))
		{
			SET_FLAG(effect->header.flags, _effect_nonviolent_bit, TRUE);
		}

		if (debug_effects_nonviolent)
		{
			struct object_datum *object = object_try_and_get(object_index);

			error(
				_error_silent,
				"created %sviolent %s on %s",
				TEST_FLAG(effect->header.flags, _effect_nonviolent_bit) ? "non" : "",
				tag_name_strip_path(tag_get_name(definition_index)),
				object == NULL
					? "<none>"
					: tag_name_strip_path(
						tag_get_name(object->definition_index)));
		}

		marker_list.count = marker_count;
		marker_list.names = marker_names;
		marker_list.points = marker_points;
		marker_list.forwards = marker_forwards;
		marker_list.node_index = node_index != NONE ? node_index : 0;
		effect_call_new(&call, _effect_call_object_node_matrix);
		call.a = object_index;
		call.b = marker_list.node_index;
		effect_call(effect_index, &call);
		memcpy(&node_matrix, call.out, sizeof(node_matrix));
		marker_list.node_matrix = &node_matrix;

		global_effect_marker_list = &marker_list;
		effect_call_clear_locations(effect_index);
		effect_build_locations(effect_index, effect, _effect_markers_list);

		effect_update(effect_index, 0.0f);
	}

	return effect_index;
}

static long effect_new_unattached_from_markers_game(
	long definition_index,
	long owner_object_index,
	real_vector3d const *translational_velocity,
	short marker_count,
	char const **marker_names,
	real_point3d const *marker_points,
	real_vector3d const *marker_forwards,
	real scale_a,
	real scale_b,
	real_rgb_color const *color,
	struct effect_vector_field const *impulse_field,
	boolean can_be_deterministic)
{
	long effect_index;

	match_assert(
		"c:\\halo\\SOURCE\\effects\\effects.c",
		448,
		!translational_velocity || valid_real_vector3d(translational_velocity));
	match_assert(
		"c:\\halo\\SOURCE\\effects\\effects.c",
		449,
		marker_count>0);
	match_assert(
		"c:\\halo\\SOURCE\\effects\\effects.c",
		451,
		marker_points);
	match_assert(
		"c:\\halo\\SOURCE\\effects\\effects.c",
		452,
		marker_forwards);
	match_vassert(
		"c:\\halo\\SOURCE\\effects\\effects.c",
		453,
		scale_a>=0.f && scale_a<=1.f,
		csprintf(temporary, "scale_a %f not in [0,1]", scale_a));
	match_vassert(
		"c:\\halo\\SOURCE\\effects\\effects.c",
		454,
		scale_b>=0.f && scale_b<=1.f,
		csprintf(temporary, "scale_b %f not in [0,1]", scale_b));

	effect_index = effect_allocate(
		definition_index,
		owner_object_index,
		can_be_deterministic);
	if (effect_index != NONE)
	{
		struct effect_datum *effect = effect_get(effect_index);
		struct effect_marker_list marker_list;
		struct halo_sim_effect_call call;

		impulse_effect_initialize(effect_index, effect, scale_a, scale_b, color, impulse_field);

		effect->object_index = NONE;

		marker_list.count = marker_count;
		marker_list.names = marker_names;
		marker_list.points = marker_points;
		marker_list.forwards = marker_forwards;
		marker_list.node_index = NONE;
		marker_list.node_matrix = NULL;

		effect_call_new(&call, _effect_call_location_from_point);
		effect_call_vector(&call.f[0], &marker_points->x);
		effect_call(effect_index, &call);
		effect->location.leaf_index = call.out_location_leaf;
		effect->location.cluster_index = call.out_location_cluster;
		effect->location.bonus = call.out_location_bonus;

		effect->velocity = translational_velocity
			? *translational_velocity
			: *global_zero_vector3d;

		global_effect_marker_list = &marker_list;
		effect_call_clear_locations(effect_index);
		effect_build_locations(effect_index, effect, _effect_markers_list);

		effect_update(effect_index, 0.0f);
	}

	return effect_index;
}

static long effect_new_from_object_game(
	long definition_index,
	long owner_object_index,
	long object_index,
	short force_local_player_index,
	real scale_a,
	real scale_b,
	real_rgb_color const *color,
	struct effect_vector_field const *impulse_field)
{
	long effect_index;

	match_assert(
		"c:\\halo\\SOURCE\\effects\\effects.c",
		305,
		object_index!=NONE);
	match_vassert(
		"c:\\halo\\SOURCE\\effects\\effects.c",
		306,
		scale_a>=0.f && scale_a<=1.f,
		csprintf(temporary, "scale_a %f not in [0,1]", scale_a));
	match_vassert(
		"c:\\halo\\SOURCE\\effects\\effects.c",
		307,
		scale_b>=0.f && scale_b<=1.f,
		csprintf(temporary, "scale_b %f not in [0,1]", scale_b));

	effect_index = effect_allocate(definition_index, owner_object_index, TRUE);
	if (effect_index != NONE)
	{
		struct effect_datum *effect = effect_get(effect_index);

		impulse_effect_initialize(effect_index, effect, scale_a, scale_b, color, impulse_field);

		effect->object_index = object_index;
		effect->local_player_index = effect_call_local_index(effect_index, object_index);

		if (effects_corpse_nonviolent &&
			effect_call_corpse(effect_index, effect->object_index))
		{
			SET_FLAG(effect->header.flags, _effect_nonviolent_bit, TRUE);
		}

		if (debug_effects_nonviolent)
		{
			struct object_datum *object = object_try_and_get(object_index);

			error(
				_error_silent,
				"created %sviolent %s on %s",
				TEST_FLAG(effect->header.flags, _effect_nonviolent_bit) ? "non" : "",
				tag_name_strip_path(tag_get_name(definition_index)),
				object == NULL
					? "<none>"
					: tag_name_strip_path(
						tag_get_name(object->definition_index)));
		}

		effect_call_clear_locations(effect_index);

		effect_build_locations(effect_index, effect, _effect_markers_object);
		if (effect->local_player_index != NONE)
			effect_build_locations(effect_index, effect, _effect_markers_first_person_weapon);

		if (force_local_player_index != NONE)
			effect->local_player_index = force_local_player_index;

		effect_update(effect_index, 0.0f);
	}

	return effect_index;
}

static long effect_new_looping_game(
	long definition_index,
	long object_index,
	short scale_a_function_index,
	short scale_b_function_index,
	short change_color_index)
{
	long effect_index;

	match_assert(
		"c:\\halo\\SOURCE\\effects\\effects.c",
		249,
		object_index!=NONE);

	effect_index = effect_allocate(definition_index, object_index, TRUE);
	if (effect_index != NONE)
	{
		struct effect_datum *effect = effect_get(effect_index);
		struct halo_sim_effect_call call;

		effect->object_index = object_index;
		effect->local_player_index = effect_call_local_index(effect_index, object_index);
		effect->scale_a_function_index = scale_a_function_index;
		effect->scale_b_function_index = scale_b_function_index;
		/* port: the index is the map's attachment's (its change color less
		one): one the object has no change color for is none (white), and
		said once */
		if (change_color_index != NONE &&
			!VALID_INDEX(change_color_index, NUMBER_OF_OBJECT_CHANGE_COLORS))
		{
			static boolean bad_change_color_reported = FALSE;

			if (!bad_change_color_reported)
			{
				bad_change_color_reported = TRUE;
				error(
					_error_silent,
					"effect %s is attached with change color %d (of %d)",
					tag_get_name(definition_index),
					change_color_index,
					NUMBER_OF_OBJECT_CHANGE_COLORS);
			}
			change_color_index = NONE;
		}
		effect->change_color_index = change_color_index;
		effect_call_new(&call, _effect_call_impulse_field);
		call.a = 0;
		effect_call(effect_index, &call);

		if (change_color_index == NONE)
			effect->color = *global_real_rgb_white;

		SET_FLAG(effect->header.flags, _effect_loop_bit, TRUE);
		effect_call_clear_locations(effect_index);

		effect_build_locations(effect_index, effect, _effect_markers_object);
		if (effect->local_player_index != NONE)
			effect_build_locations(effect_index, effect, _effect_markers_first_person_weapon);

		effect_update(effect_index, 0.0f);
	}

	return effect_index;
}

void effects_update(
	real dt)
{
	long effect_index;

	profile_enter(effects_update_section);

	for (effect_index = data_next_index(effect_data, NONE);
		effect_index != NONE;
		effect_index = data_next_index(effect_data, effect_index))
	{
		effect_update(effect_index, dt);
	}

	profile_exit(effects_update_section);

	return;
}

void effects_start_on_first_person_weapon(
	short local_player_index,
	long object_index)
{
	long effect_index;

	for (effect_index = data_next_index(effect_data, NONE);
		effect_index != NONE;
		effect_index = data_next_index(effect_data, effect_index))
	{
		struct effect_datum *effect = effect_get(effect_index);
		struct effect_definition *definition =
			effect_definition_get(effect->definition_index);

		if (effect->object_index == object_index)
		{
			match_assert(
				"c:\\halo\\SOURCE\\effects\\effects.c",
				585,
				effect->local_player_index==NONE);

			effect->local_player_index = local_player_index;
			effect_build_locations(
				effect_index,
				effect,
				_effect_markers_first_person_weapon);
		}
	}

	return;
}

void effects_stop_on_first_person_weapon(
	short local_player_index)
{
	long effect_index;

	for (effect_index = data_next_index(effect_data, NONE);
		effect_index != NONE;
		effect_index = data_next_index(effect_data, effect_index))
	{
		struct effect_datum *effect = effect_get(effect_index);
		struct effect_definition *definition =
			effect_definition_get(effect->definition_index);

		if (effect->local_player_index == local_player_index)
		{
			short location_index;
			/* port: no more locations than the effect holds (the map's count) */
			short location_count = (short)MIN(
				definition->locations.count,
				(long)NUMBEROF(effect->location_datum_indices));

			for (location_index = 0;
				location_index < location_count;
				location_index++)
			{
				long *location_datum_index =
					&effect->location_datum_indices[location_index];

				while (*location_datum_index != NONE)
				{
					struct effect_location_datum *location =
						effect_location_get(*location_datum_index);

					if (location->node_designator == NONE ||
						!TEST_FLAG(location->node_designator, _effect_location_first_person_bit))
					{
						location_datum_index =
							&location->next_instance_location_index;
					}
					else
					{
						/* See effect_delete: deletion leaves this fixed-array slot backed. */
						datum_delete(
							effect_location_data,
							*location_datum_index);
						*location_datum_index =
							location->next_instance_location_index;
					}
				}
			}

			effect->local_player_index = NONE;
		}
	}

	return;
}

void effects_information_get(
	struct effects_information *information)
{
	long effect_index;

	information->effect_count = effect_data->actual_count;
	information->location_count = effect_location_data->actual_count;
	information->active_effect_count = 0;

	for (effect_index = data_next_index(effect_data, NONE);
		effect_index != NONE;
		effect_index = data_next_index(effect_data, effect_index))
	{
		short flags = ((struct effect_datum_header *)datum_get(
			effect_data,
			effect_index))->flags;
		if (!TEST_FLAG(flags, _effect_stopped_bit) &&
			!TEST_FLAG(flags, _effect_invisible_bit))
		{
			information->active_effect_count++;
		}
	}

	return;
}

void effects_disconnect_from_structure_bsp(
	void)
{
	return;
}

void effects_reconnect_to_structure_bsp(
	void)
{
	long effect_index;

	for (effect_index = data_next_index(effect_data, NONE);
		effect_index != NONE;
		effect_index = data_next_index(effect_data, effect_index))
	{
		struct effect_datum *effect = effect_get(effect_index);

		if (effect->object_index == NONE)
		{
			long location_datum_index = effect->location_datum_indices[0];
			struct effect_location_datum *instance = NULL;

			if (location_datum_index != NONE)
			{
				instance = effect_location_get(location_datum_index);
				location_datum_index = instance->next_instance_location_index;
				if (instance->node_designator != NONE &&
					TEST_FLAG(instance->node_designator, _effect_location_first_person_bit))
				{
					instance = effect_location_get_next_instance(
						effect,
						&location_datum_index,
						0);
				}
			}

			if (instance)
			{
				scenario_location_from_point(
					&effect->location,
					&instance->matrix.position);
			}
			else
			{
				effect_delete(effect_index);
			}
		}
	}

	return;
}

static struct effect_location_datum *effect_location_get_next_instance(
	struct effect_datum const *effect,
	long *location_datum_index,
	short camera_mode)
{
	struct effect_location_datum *location = NULL;

	match_assert(
		"c:\\halo\\SOURCE\\effects\\effects.c",
		1766,
		effect);
	match_assert(
		"c:\\halo\\SOURCE\\effects\\effects.c",
		1767,
		location_datum_index);

	if (*location_datum_index != NONE)
	{
		location = effect_location_get(*location_datum_index);
		*location_datum_index = location->next_instance_location_index;

		if (camera_mode == _effect_camera_mode_first_person_only ||
			(camera_mode == _effect_camera_mode_both &&
			effect->local_player_index != NONE &&
			local_player_count() == 1))
		{
			if (location->node_designator == NONE ||
				!TEST_FLAG(location->node_designator, _effect_location_first_person_bit))
			{
				location = effect_location_get_next_instance(
					effect,
					location_datum_index,
					camera_mode);
			}
		}
		else
		{
			if (location->node_designator != NONE &&
				TEST_FLAG(location->node_designator, _effect_location_first_person_bit))
			{
				location = effect_location_get_next_instance(
					effect,
					location_datum_index,
					camera_mode);
			}
		}
	}

	return location;
}

void effect_stop(
	long effect_index,
	boolean and_delete)
{
	struct effect_datum *effect = effect_try_and_get(effect_index);

	if (effect)
	{
		struct effect_definition *definition =
			effect_definition_get(effect->definition_index);

		if (TEST_FLAG(effect->header.flags, _effect_loop_bit))
		{
			short stop_event_index;

			SET_FLAG(effect->header.flags, _effect_delete_on_stop_bit, and_delete);

			stop_event_index = definition->loop_stop_index;
			if (stop_event_index < 0 ||
				stop_event_index + 1 >= definition->events.count)
			{
				SET_FLAG(effect->header.flags, _effect_stopped_bit, TRUE);
			}
			else
			{
				effect_set_event(effect_index, (short)(stop_event_index + 1));
				SET_FLAG(effect->header.flags, _effect_stopping_bit, TRUE);
			}
		}
		else
		{
			effect_delete(effect_index);
		}
	}

	return;
}

boolean dangerous_effects_near_player(
	void)
{
	long effect_index;

	for (effect_index = data_next_index(effect_data, NONE);
		effect_index != NONE;
		effect_index = data_next_index(effect_data, effect_index))
	{
		struct effect_datum *effect = effect_get(effect_index);
		struct effect_definition *definition =
			effect_definition_get(effect->definition_index);

		if (!TEST_FLAG(effect->header.flags, _effect_stopped_bit) &&
			definition->runtime_danger_radius != 0.0f)
		{
			struct data_iterator iterator;
			struct player_datum *player;

			data_iterator_new(&iterator, player_data);
			while ((player = (struct player_datum *)data_iterator_next(&iterator)) != NULL)
			{
				if (player->unit_index != NONE)
				{
					struct object_datum *unit = object_get(player->unit_index);
					long location_datum_index = effect->location_datum_indices[0];
					struct effect_location_datum *instance;

					while ((instance = effect_location_get_next_instance(
						effect,
						&location_datum_index,
						_effect_camera_mode_independent_of_camera_mode)) != NULL)
					{
						real_point3d position;

						if (instance->node_designator != NONE)
						{
							real_matrix4x3 *node_matrix;

							if (TEST_FLAG(
								instance->node_designator,
								_effect_location_first_person_bit))
							{
								node_matrix = first_person_weapon_get_node_matrix(
									effect->local_player_index,
									(short)(instance->node_designator &
										(FLAG(_effect_location_first_person_bit) - 1)));
							}
							else
							{
								node_matrix = object_get_node_matrix(
									effect->object_index,
									(short)(instance->node_designator &
										(FLAG(_effect_location_first_person_bit) - 1)));
							}

							matrix4x3_transform_point(
								node_matrix,
								&instance->matrix.position,
								&position);
						}
						else
						{
							position = instance->matrix.position;
						}

						if (point_in_sphere(
							&unit->object.bounding_sphere_center,
							&position,
							unit->object.bounding_sphere_radius +
								definition->runtime_danger_radius))
						{
							return TRUE;
						}
					}
				}
			}
		}
	}

	return FALSE;
}

real real_local_random(
	void)
{
	real result = real_seed_random(get_global_local_random_seed_address());

	return result;
}

/* ---------- private code */


static void effect_marker_list_get_marker(
	struct object_marker *marker,
	struct effect_marker_list const *marker_list,
	short effect_marker_index)
{
	real_point3d point;
	real_vector3d forward;
	real_vector3d up;

	match_assert(
		"c:\\halo\\SOURCE\\effects\\effects.c",
		1120,
		effect_marker_index>=0 && effect_marker_index<marker_list->count);
	match_assert_valid_real_normal3d(
		"c:\\halo\\SOURCE\\effects\\effects.c",
		1121,
		&marker_list->forwards[effect_marker_index]);

	marker->node_index = marker_list->node_index;

	if (marker_list->node_matrix)
	{
		matrix4x3_inverse_transform_point(
			marker_list->node_matrix,
			&marker_list->points[effect_marker_index],
			&point);
		matrix4x3_inverse_transform_normal(
			marker_list->node_matrix,
			&marker_list->forwards[effect_marker_index],
			&forward);
	}
	else
	{
		point = marker_list->points[effect_marker_index];
		forward = marker_list->forwards[effect_marker_index];
	}

	match_assert_valid_real_normal3d(
		"c:\\halo\\SOURCE\\effects\\effects.c",
		1136,
		&forward);

	perpendicular3d(&forward, &up);
	normalize3d(&up);
	matrix4x3_from_point_and_vectors(&marker->node_matrix, &point, &forward, &up);

	return;
}

static short effect_marker_list_get_markers_by_name(
	long object_index,
	char const *name,
	struct object_marker *markers,
	short maximum_marker_count)
{
	short marker_count = 0;

	if (global_effect_marker_list->names && csstrlen(name))
	{
		short marker_index;

		for (marker_index = 0;
			marker_count < maximum_marker_count;
			marker_index++)
		{
			if (marker_index >= global_effect_marker_list->count)
				break;

			if (csstrcmp(name, global_effect_marker_list->names[marker_index]) == 0)
			{
				struct object_marker *marker = &markers[marker_count++];

				effect_marker_list_get_marker(
					marker,
					global_effect_marker_list,
					marker_index);
			}
		}
	}

	if (marker_count == 0)
	{
		effect_marker_list_get_marker(markers, global_effect_marker_list, 0);
		marker_count = 1;
	}

	return marker_count;
}

static boolean effects_object_is_corpse(
	long object_index)
{
	boolean is_corpse = FALSE;
	struct biped_datum *unit = biped_try_and_get(object_index);

	if (unit && TEST_FLAG(unit->object.damage_flags, _object_dead_bit))
	{
		is_corpse = unit->unit.time_of_death != NONE &&
			unit->unit.time_of_death + CORPSE_SETTLING_TIME < game_time_get();
	}

	return is_corpse;
}

static long effect_allocate(
	long definition_index,
	long owner_object_index,
	boolean can_be_deterministic)
{
	long effect_index = NONE;

	if (!game_in_editor() && definition_index != NONE)
	{
		struct halo_sim_effect_call call;

		/* port: the definition's test and the datum, through effect_call */
		effect_call_new(&call, _effect_call_allocate);
		call.a = definition_index;
		call.b = owner_object_index;
		call.c = can_be_deterministic ? 1 : 0;
		effect_call(NONE, &call);
		effect_index = call.result;

		if (effect_index != NONE)
		{
			struct effect_datum *effect = effect_get(effect_index);

			effect->definition_index = definition_index;
			effect->owner_object_index = owner_object_index;
			effect->local_player_index = NONE;
			effect->header.flags = 0;

			effect_set_event(effect_index, 0);
		}
	}

	return effect_index;
}

/* the definition's test and the datum of effect_allocate */
static long effect_allocate_datum(
	long definition_index,
	boolean can_be_deterministic)
{
	struct effect_definition *definition =
		effect_definition_get(definition_index);
	long effect_index = NONE;

	if (!can_be_deterministic && TEST_FLAG(
		definition->flags,
		_effect_definition_must_be_deterministic_bit))
	{
		error(
			_error_silent,
			"cannot create objects, lights, or damage from an effect created by particles.");
	}
	else if (definition->events.count > 0)
	{
		effect_index = datum_new(effect_data);

		if (effect_index == NONE && TEST_FLAG(
			definition->flags,
			_effect_definition_must_be_deterministic_bit))
		{
			for (effect_index = data_next_index(effect_data, NONE);
				effect_index != NONE;
				effect_index = data_next_index(effect_data, effect_index))
			{
				if (!TEST_FLAG(
					effect_definition_get(
						effect_get(effect_index)->definition_index)->flags,
					_effect_definition_must_be_deterministic_bit))
				{
					datum_delete(effect_data, effect_index);
					effect_index = datum_new(effect_data);
					match_assert(
						"c:\\halo\\SOURCE\\effects\\effects.c",
						941,
						effect_index!=NONE);
					break;
				}
			}
		}
	}

	return effect_index;
}

static void impulse_effect_initialize(
	long effect_index,
	struct effect_datum *effect,
	real scale_a,
	real scale_b,
	real_rgb_color const *color,
	struct effect_vector_field const *impulse_field)
{
	struct halo_sim_effect_call call;

	effect->scale_a = scale_a;
	effect->scale_b = scale_b;
	effect->color = color ? *color : *global_real_rgb_white;
	match_assert_valid_real_rgb_color(
		"c:\\halo\\SOURCE\\effects\\effects.c",
		980,
		&effect->color);

	/* port: the field, through effect_call */
	effect_call_new(&call, _effect_call_impulse_field);
	call.a = (long)impulse_field;
	effect_call(effect_index, &call);

	return;
}

/* port: an effect update's call of the game (effect_call) */
static void effect_call_new(
	struct halo_sim_effect_call *call,
	long kind)
{
	memset(call, 0, sizeof(*call));
	call->kind = kind;
}

static void effect_call_vector(
	float *f,
	real const *v)
{
	f[0] = v[0];
	f[1] = v[1];
	f[2] = v[2];
}

static void effect_generate_part(
	long effect_index,
	struct effect_datum const *effect,
	struct effect_part_definition const *part_definition,
	struct effect_location_datum const *instance,
	real_point3d const *world_point,
	real_vector3d const *world_forward,
	real_vector3d const *world_up,
	real scale)
{
	struct halo_sim_effect_call call;
	real_vector3d direction;
	real_vector3d velocity;

	switch (part_definition->runtime_base_class_tag)
	{
		case PARTICLE_SYSTEM_DEFINITION_TAG:
		{
			effect_random_translational_velocity(
				get_global_local_random_seed_address(),
				effect,
				world_forward,
				&direction,
				&velocity,
				part_definition->velocity_lower_bound,
				part_definition->velocity_upper_bound,
				part_definition->velocity_cone_angle,
				part_definition->scale_a_flags,
				part_definition->scale_b_flags);

			velocity.i += effect->velocity.i;
			velocity.j += effect->velocity.j;
			velocity.k += effect->velocity.k;

			effect_call_new(&call, _effect_call_particle_system);
			call.a = part_definition->reference.index;
			effect_call_vector(&call.f[0], &world_point->x);
			effect_call_vector(&call.f[3], &velocity.i);
			effect_call_vector(&call.f[6], &effect->color.red);
			call.f[9] = scale;
			effect_call(effect_index, &call);
			break;
		}

		case SOUND_DEFINITION_TAG:
			effect_call_new(&call, _effect_call_sound);
			call.b = part_definition->reference.index;
			if (effect->object_index != NONE)
			{
				short node_index = instance->node_designator;

				if (node_index != NONE)
					node_index &= (short)(FLAG(_effect_location_first_person_bit) - 1);

				call.a = effect->object_index;
				call.c = node_index;
				effect_call_vector(&call.f[0], &instance->matrix.position.x);
				effect_call_vector(&call.f[3], &instance->matrix.forward.i);
			}
			else
			{
				call.a = NONE;
				effect_call_vector(&call.f[0], &world_point->x);
				effect_call_vector(&call.f[3], &world_forward->i);
			}
			call.f[6] = scale;
			effect_call(effect_index, &call);
			break;

		case OBJECT_DEFINITION_TAG:
		{
			real_vector3d translational_velocity;
			real_vector3d angular_velocity;
			unsigned long *seed;

			effect_random_translational_velocity(
				get_global_random_seed_address(),
				effect,
				world_forward,
				&direction,
				&translational_velocity,
				part_definition->velocity_lower_bound,
				part_definition->velocity_upper_bound,
				part_definition->velocity_cone_angle,
				part_definition->scale_a_flags,
				part_definition->scale_b_flags);

			translational_velocity.i += effect->velocity.i;
			translational_velocity.j += effect->velocity.j;
			translational_velocity.k += effect->velocity.k;

			seed = get_global_random_seed_address();
			effect_random_angular_velocity(
				seed,
				effect,
				&angular_velocity,
				part_definition->angular_velocity_lower_bound,
				part_definition->angular_velocity_upper_bound,
				part_definition->scale_a_flags,
				part_definition->scale_b_flags);

			effect_call_new(&call, _effect_call_object_new);
			call.a = part_definition->reference.index;
			call.b = effect->owner_object_index;
			effect_call_vector(&call.f[0], &world_point->x);
			effect_call_vector(&call.f[3], &world_forward->i);
			effect_call_vector(&call.f[6], &world_up->i);
			effect_call_vector(&call.f[9], &translational_velocity.i);
			effect_call_vector(&call.f[12], &angular_velocity.i);
			effect_call(effect_index, &call);
			break;
		}

		case DECAL_GROUP_TAG:
		{
			real radius_modifier;

			effect_random_translational_velocity(
				get_global_local_random_seed_address(),
				effect,
				world_forward,
				&direction,
				&velocity,
				part_definition->velocity_lower_bound,
				part_definition->velocity_upper_bound,
				part_definition->velocity_cone_angle,
				part_definition->scale_a_flags,
				part_definition->scale_b_flags);

			radius_modifier = real_seed_random_range(
				get_global_local_random_seed_address(),
				part_definition->radius_modifier_lower_bound,
				part_definition->radius_modifier_upper_bound);

			effect_call_new(&call, _effect_call_decal);
			call.a = part_definition->reference.index;
			effect_call_vector(&call.f[0], &world_point->x);
			effect_call_vector(&call.f[3], &velocity.i);
			call.f[6] = radius_modifier;
			effect_call(effect_index, &call);
			break;
		}

		case DAMAGE_EFFECT_DEFINITION_TAG:
			effect_call_new(&call, _effect_call_damage);
			call.a = part_definition->reference.index;
			effect_call_vector(&call.f[0], &world_point->x);
			effect_call_vector(&call.f[3], &world_forward->i);
			call.f[6] = scale;
			effect_call(effect_index, &call);
			break;

		case LIGHT_DEFINITION_TAG:
		{
			short node_index = instance->node_designator;

			if (node_index != NONE)
				node_index &= (short)(FLAG(_effect_location_first_person_bit) - 1);

			effect_call_new(&call, _effect_call_light);
			call.a = part_definition->reference.index;
			call.b = effect->object_index;
			call.c = node_index;
			effect_call_vector(&call.f[0], &instance->matrix.position.x);
			effect_call_vector(&call.f[3], &instance->matrix.forward.i);
			call.f[6] = scale;
			effect_call(effect_index, &call);
			break;
		}

		default:
			match_vassert(
				"c:\\halo\\SOURCE\\effects\\effects.c",
				1753,
				FALSE,
				csprintf(
					temporary,
					"effect %s has a bad part %s",
					tag_get_name(effect->definition_index),
					part_definition->reference.name));
			break;
	}

	return;
}

/* port: effect_get_node_matrix, through effect_call */
static void effect_node_matrix(
	long effect_index,
	short node_designator,
	real_matrix4x3 *matrix)
{
	struct halo_sim_effect_call call;

	effect_call_new(&call, _effect_call_node_matrix);
	call.a = node_designator;
	effect_call(effect_index, &call);
	memcpy(matrix, call.out, sizeof(*matrix));
}

static void effect_generate_parts(
	long effect_index,
	struct effect_datum *effect)
{
	struct effect_definition *definition =
		effect_definition_get(effect->definition_index);
	struct effect_event_definition *event = TAG_BLOCK_GET_ELEMENT(
		&definition->events,
		effect->event_index,
		struct effect_event_definition);
	short part_index;

	for (part_index = 0; part_index < event->parts.count; part_index++)
	{
		struct effect_part_definition *part = TAG_BLOCK_GET_ELEMENT(
			&event->parts,
			part_index,
			struct effect_part_definition);
		long location_datum_index;
		struct effect_location_datum *instance;

		/* port: and a location the effect holds */
		if (part->location_index < 0 ||
			part->location_index >= definition->locations.count ||
			part->location_index >= (short)NUMBEROF(effect->location_datum_indices) ||
			part->reference.index == NONE)
		{
			continue;
		}

		if (!effect_part_allowed_by_disposition(
			TEST_FLAG(effect->header.flags, _effect_nonviolent_bit),
			part->disposition))
		{
			continue;
		}

		location_datum_index = effect->location_datum_indices[part->location_index];

		while ((instance = effect_location_get_next_instance(
			effect,
			&location_datum_index,
			_effect_camera_mode_independent_of_camera_mode)) != NULL)
		{
			real_point3d world_point;
			real_vector3d world_forward;
			real_vector3d world_up;

			if (instance->node_designator != NONE)
			{
				real_matrix4x3 node_matrix;

				effect_node_matrix(effect_index, instance->node_designator, &node_matrix);

				matrix4x3_transform_point(
					&node_matrix,
					&instance->matrix.position,
					&world_point);
				matrix4x3_transform_normal(
					&node_matrix,
					&instance->matrix.forward,
					&world_forward);
				matrix4x3_transform_normal(
					&node_matrix,
					&instance->matrix.up,
					&world_up);
			}
			else
			{
				world_point = instance->matrix.position;
				world_forward = instance->matrix.forward;
				world_up = instance->matrix.up;
			}

			if (TEST_FLAG(part->flags, _effect_part_definition_world_down_bit))
			{
				world_forward = *global_down3d;
				world_up = *global_forward3d;
			}

			if (effect_allowed_by_environment(
				effect_index,
				part->environment,
				&world_point))
			{
				effect_generate_part(
					effect_index,
					effect,
					part,
					instance,
					&world_point,
					&world_forward,
					&world_up,
					effect_scale(
						effect,
						1.0f,
						part->scale_a_flags,
						part->scale_b_flags,
						_effect_part_type_scale_bit));
			}
		}
	}

	return;
}

static void effect_generate_particles(
	long effect_index,
	struct effect_datum *effect)
{
	struct effect_definition *definition =
		effect_definition_get(effect->definition_index);
	struct effect_event_definition *event = TAG_BLOCK_GET_ELEMENT(
		&definition->events,
		effect->event_index,
		struct effect_event_definition);
	real last_event_fraction = effect->last_event_fraction;
	real event_fraction;
	short particle_index;

	if (effect->event_duration > 0.0f)
		event_fraction = effect->event_time / effect->event_duration;
	else
		event_fraction = 1.0f;

	/* port: no more particles than the effect counts (the map's count) */
	for (particle_index = 0;
		particle_index < event->particles.count &&
			particle_index < (short)NUMBEROF(effect->particle_counts);
		particle_index++)
	{
		struct effect_particles_definition *particles = TAG_BLOCK_GET_ELEMENT(
			&event->particles,
			particle_index,
			struct effect_particles_definition);
		short count_delta;
		long last_particle_count;
		long location_datum_index;
		struct effect_location_datum *instance;

		/* port: and a location the effect holds */
		if (particles->location_index < 0 ||
			particles->location_index >= definition->locations.count ||
			particles->location_index >= (short)NUMBEROF(effect->location_datum_indices))
		{
			continue;
		}

		if (!effect_part_allowed_by_disposition(
			TEST_FLAG(effect->header.flags, _effect_nonviolent_bit),
			particles->disposition))
		{
			continue;
		}

		last_particle_count = (long)(effect_evaluate_function_integral(
				particles->distribution_function,
				last_event_fraction) *
			effect->particle_counts[particle_index]);
		count_delta = (short)((long)(effect_evaluate_function_integral(
				particles->distribution_function,
				event_fraction) *
			effect->particle_counts[particle_index]) -
			last_particle_count);

		match_assert(
			"c:\\halo\\SOURCE\\effects\\effects.c",
			1411,
			count_delta>=0);

		if (count_delta <= 0)
			continue;

		location_datum_index =
			effect->location_datum_indices[particles->location_index];
		instance = effect_location_get_next_instance(
			effect,
			&location_datum_index,
			particles->camera_mode);

		while (instance)
		{
			short instance_particle_index;

			for (instance_particle_index = 0;
				instance_particle_index < count_delta;
				instance_particle_index++)
			{
				struct new_particle_data data;
				real_vector3d random_offset;
				real_vector3d relative_offset;
				real_point3d world_point;
				real_vector3d world_direction;
				real_vector3d world_velocity;
				struct halo_sim_effect_call call;
				real emission_radius = effect_real_random_range(
					get_global_local_random_seed_address(),
					effect,
					particles->distribution_radius_lower_bound,
					particles->distribution_radius_upper_bound,
					particles->scale_a_flags,
					particles->scale_b_flags,
					_effect_particle_distribution_radius_bit);

				local_random_direction3d(&random_offset);
				matrix4x3_transform_vector(
					&instance->matrix,
					&particles->offset,
					&relative_offset);

				data.position.x = instance->matrix.position.x +
					random_offset.i * emission_radius + relative_offset.i;
				data.position.y = instance->matrix.position.y +
					random_offset.j * emission_radius + relative_offset.j;
				data.position.z = instance->matrix.position.z +
					random_offset.k * emission_radius + relative_offset.k;

				effect_random_translational_velocity(
					get_global_local_random_seed_address(),
					effect,
					&particles->runtime_direction,
					&data.direction,
					&data.velocity,
					particles->velocity_lower_bound,
					particles->velocity_upper_bound,
					particles->velocity_cone_angle,
					particles->scale_a_flags,
					particles->scale_b_flags);

				matrix4x3_transform_normal(
					&instance->matrix,
					&data.direction,
					&data.direction);
				matrix4x3_transform_vector(
					&instance->matrix,
					&data.velocity,
					&data.velocity);

				if (instance->node_designator != NONE)
				{
					real_matrix4x3 node_matrix;

					effect_node_matrix(effect_index, instance->node_designator, &node_matrix);

					matrix4x3_transform_point(
						&node_matrix,
						&data.position,
						&world_point);
					matrix4x3_transform_normal(
						&node_matrix,
						&data.direction,
						&world_direction);
					matrix4x3_transform_vector(
						&node_matrix,
						&data.velocity,
						&world_velocity);
				}
				else
				{
					world_point = data.position;
					world_direction = data.direction;
					world_velocity = data.velocity;
				}

				if (effect_allowed_by_environment(
					effect_index,
					particles->environment,
					&world_point))
				{
					data.definition_index = particles->particle.index;

					if (TEST_FLAG(particles->flags, _effect_particle_attached_bit))
					{
						short node_index = instance->node_designator;

						if (node_index != NONE)
							node_index &= (short)(FLAG(_effect_location_first_person_bit) - 1);

						data.object_index = effect->object_index;
						data.node_index = node_index;
						data.initial_impulse = *global_zero_vector3d;
					}
					else
					{
						if (effect->impulse_field.translational_function)
						{
							effect_call_new(&call, _effect_call_impulse);
							effect_call_vector(&call.f[0], &world_point.x);
							effect_call(effect_index, &call);
							data.initial_impulse.i = call.out[0];
							data.initial_impulse.j = call.out[1];
							data.initial_impulse.k = call.out[2];
						}
						else
						{
							data.initial_impulse = *global_zero_vector3d;
						}

						data.object_index = NONE;
						/* (the node is the game's leftover; the particle has no object) */
						data.node_index = 0;
						data.position = world_point;
						data.direction = world_direction;
						data.velocity.i = world_velocity.i + effect->velocity.i * 30.0f;
						data.velocity.j = world_velocity.j + effect->velocity.j * 30.0f;
						data.velocity.k = world_velocity.k + effect->velocity.k * 30.0f;
					}

					data.radius = effect_real_random_range(
						get_global_local_random_seed_address(),
						effect,
						particles->radius_lower_bound,
						particles->radius_upper_bound,
						particles->scale_a_flags,
						particles->scale_b_flags,
						_effect_particle_radius_bit);
					data.angular_velocity = effect_real_random_range(
						get_global_local_random_seed_address(),
						effect,
						particles->angular_velocity_lower_bound,
						particles->angular_velocity_upper_bound,
						particles->scale_a_flags,
						particles->scale_b_flags,
						_effect_angular_velocity_bit);
					data.rotation = TEST_FLAG(
						particles->flags,
						_effect_particle_random_orientation_bit)
						? real_seed_random_range(
							get_global_local_random_seed_address(),
							0.0f,
							2.f*_pi)
						: 0.0f;

					if (TEST_FLAG(particles->scale_a_flags, _effect_particle_tint_bit) ||
						TEST_FLAG(particles->scale_b_flags, _effect_particle_tint_bit))
					{
						data.color.alpha = effect_scale(
							effect,
							1.0f,
							particles->scale_a_flags,
							particles->scale_b_flags,
							_effect_particle_tint_bit);
					}
					else
					{
						data.color.alpha = real_seed_random(
							get_global_local_random_seed_address());
					}

					rgb_colors_interpolate(
						&data.color.rgb,
						(particles->flags >> _effect_particle_tint_interpolate_hsv_bit) & 3,
						&particles->tint_lower_bound.rgb,
						&particles->tint_upper_bound.rgb,
						data.color.alpha);
					data.color.alpha =
						(1.0f - data.color.alpha) * particles->tint_lower_bound.alpha +
						particles->tint_upper_bound.alpha * data.color.alpha;

					if (TEST_FLAG(
						particles->flags,
						_effect_particle_tint_from_change_color_bit))
					{
						data.color.rgb.red *= effect->color.red;
						data.color.rgb.green *= effect->color.green;
						data.color.rgb.blue *= effect->color.blue;
					}

					data.local_player_index = effect->local_player_index;
					data.attached_to_local_player = instance->node_designator != NONE &&
						TEST_FLAG(
							instance->node_designator,
							_effect_location_first_person_bit);
					data.dont_draw_first_person =
						particles->camera_mode == _effect_camera_mode_third_person_only;
					data.dont_draw_third_person =
						particles->camera_mode == _effect_camera_mode_first_person_only;

					effect_call_new(&call, _effect_call_particle);
					call.a = data.definition_index;
					call.b = data.object_index;
					call.c = (unsigned short)data.node_index |
						(data.attached_to_local_player ? 1 << 16 : 0) |
						(data.dont_draw_first_person ? 1 << 17 : 0) |
						(data.dont_draw_third_person ? 1 << 18 : 0);
					call.d = data.local_player_index;
					effect_call_vector(&call.f[0], &data.position.x);
					effect_call_vector(&call.f[3], &data.direction.i);
					effect_call_vector(&call.f[6], &data.velocity.i);
					effect_call_vector(&call.f[9], &data.initial_impulse.i);
					call.f[12] = data.rotation;
					call.f[13] = data.angular_velocity;
					call.f[14] = data.radius;
					call.f[15] = data.color.alpha;
					effect_call_vector(&call.f[16], &data.color.red);
					effect_call(effect_index, &call);
				}
			}

			instance = effect_location_get_next_instance(
				effect,
				&location_datum_index,
				particles->camera_mode);
		}
	}

	effect->last_event_fraction = event_fraction;

	return;
}

/* port: the Rust port's update in place of the game's when asked
(halo_sim.c); in check mode the game's runs and is compared */
static void effect_update(
	long effect_index,
	real dt)
{
	int checking = FALSE;

	if (halo_sim_effect_update(effect_index, dt, &checking))
		return;
	effect_update_game(effect_index, dt);
	if (checking)
		halo_sim_effect_update_check(effect_index);

	return;
}

static void effect_update_game(
	long effect_index,
	real dt)
{
	struct effect_datum *effect = effect_get(effect_index);
	struct effect_definition *definition =
		effect_definition_get(effect->definition_index);
	struct halo_sim_effect_call call;
	boolean visible;
	short iteration_index;

	if (effect->object_index != NONE)
	{
		effect_call_new(&call, _effect_call_object);
		call.a = effect->object_index;
		effect_call(effect_index, &call);
		if (call.result == 0)
		{
			effect_call_new(&call, _effect_call_delete);
			effect_call(effect_index, &call);

			return;
		}

		if (call.result == 2)
		{
			effect->location.leaf_index = call.out_location_leaf;
			effect->location.cluster_index = call.out_location_cluster;
			effect->location.bonus = call.out_location_bonus;
			effect->velocity.i = call.out[0];
			effect->velocity.j = call.out[1];
			effect->velocity.k = call.out[2];
		}
		else
		{
			effect->location.cluster_index = NONE;
		}

		if (TEST_FLAG(effect->header.flags, _effect_loop_bit))
		{
			effect_call_new(&call, _effect_call_function_value);
			call.a = effect->object_index;
			call.b = effect->scale_a_function_index;
			call.f[0] = effect->scale_a;
			effect_call(effect_index, &call);
			effect->scale_a = call.out[0];
			if (!call.result)
			{
				if (TEST_FLAG(
					definition->flags,
					_effect_definition_deleted_when_inactive_bit))
				{
					effect_call_new(&call, _effect_call_detach_delete);
					call.a = effect->object_index;
					effect_call(effect_index, &call);

					return;
				}
				else if (!TEST_FLAG(effect->header.flags, _effect_stopping_bit) &&
					!TEST_FLAG(effect->header.flags, _effect_stopped_bit))
				{
					effect_stop(effect_index, FALSE);
				}
			}
			else
			{
				word flags = effect->header.flags;

				if (TEST_FLAG(flags, _effect_stopped_bit))
				{
					if (TEST_FLAG(flags, _effect_delete_on_stop_bit))
					{
						effect_call_new(&call, _effect_call_delete);
						effect_call(effect_index, &call);

						return;
					}
					else
					{
						effect->header.flags =
							(word)(flags & ~FLAG(_effect_stopped_bit));
						effect_set_event(effect_index, 0);
					}
				}
			}

			effect_call_new(&call, _effect_call_function_value);
			call.a = effect->object_index;
			call.b = effect->scale_b_function_index;
			call.f[0] = effect->scale_b;
			effect_call(effect_index, &call);
			effect->scale_b = call.out[0];

			if (effect->change_color_index != NONE)
			{
				effect_call_new(&call, _effect_call_change_color);
				call.a = effect->object_index;
				call.b = effect->change_color_index;
				effect_call(effect_index, &call);
				effect->color.red = call.out[0];
				effect->color.green = call.out[1];
				effect->color.blue = call.out[2];
				match_assert_valid_real_rgb_color(
					"c:\\halo\\SOURCE\\effects\\effects.c",
					1227,
					&effect->color);
			}
		}
	}

	visible = FALSE;
	if (effect->location.cluster_index != NONE)
	{
		effect_call_new(&call, _effect_call_visible);
		call.a = TEST_FLAG(
			definition->flags,
			_effect_definition_must_be_deterministic_bit) ? 1 : 0;
		effect_call(effect_index, &call);
		visible = call.result != 0;
	}

	if (!visible)
	{
		word flags = effect->header.flags;

		if (!TEST_FLAG(flags, _effect_invisible_bit))
		{
			if (TEST_FLAG(flags, _effect_loop_bit))
			{
				effect->header.flags = (word)(flags | FLAG(_effect_invisible_bit));
			}
			else
			{
				effect_call_new(&call, _effect_call_delete);
				effect_call(effect_index, &call);

				return;
			}
		}
	}
	else
	{
		word flags = effect->header.flags;

		if (TEST_FLAG(flags, _effect_invisible_bit))
			effect->header.flags = (word)(flags & ~FLAG(_effect_invisible_bit));
	}

	for (iteration_index = 0;
		dt >= 0.0f;
		iteration_index++)
	{
		word flags = effect->header.flags;
		boolean event_completed;

		if (TEST_FLAG(flags, _effect_stopped_bit) ||
			iteration_index >= MAXIMUM_EFFECT_EVENTS_PER_UPDATE)
		{
			break;
		}

		if (effect->event_duration - effect->event_time <= dt)
		{
			dt -= effect->event_duration - effect->event_time;
			event_completed = TRUE;
			effect->event_time = effect->event_duration;
		}
		else
		{
			effect->event_time += dt;
			event_completed = FALSE;
			dt = -1.0f;
		}

		if (TEST_FLAG(flags, _effect_in_duration_bit))
		{
			short next_event_index;

			if (!TEST_FLAG(flags, _effect_invisible_bit))
				effect_generate_particles(effect_index, effect);

			if (!event_completed)
				continue;

			if (TEST_FLAG(effect->header.flags, _effect_loop_bit) &&
				effect->event_index == definition->loop_stop_index &&
				definition->loop_start_index != NONE)
			{
				next_event_index = definition->loop_start_index;
			}
			else
			{
				next_event_index = (short)(effect->event_index + 1);
			}

			while (next_event_index < definition->events.count &&
				real_seed_random(
					effect_get_random_seed(effect->definition_index)) <
				TAG_BLOCK_GET_ELEMENT(
					&definition->events,
					next_event_index,
					struct effect_event_definition)->skip_fraction)
			{
				next_event_index++;
			}

			if (next_event_index >= definition->events.count)
			{
				word end_flags = effect->header.flags;

				if (TEST_FLAG(end_flags, _effect_loop_bit))
				{
					effect->header.flags = (word)(end_flags | FLAG(_effect_stopped_bit));
				}
				else
				{
					effect_call_new(&call, _effect_call_delete);
					effect_call(effect_index, &call);
				}

				return;
			}

			effect_set_event(effect_index, next_event_index);
		}
		else if (event_completed)
		{
			struct effect_event_definition *event = TAG_BLOCK_GET_ELEMENT(
				&definition->events,
				effect->event_index,
				struct effect_event_definition);
			short particle_index;

			SET_FLAG(effect->header.flags, _effect_in_duration_bit, TRUE);
			effect->event_time = 0.0f;
			effect->last_event_fraction = -1.0f;
			effect->event_duration = real_seed_random_range(
				effect_get_random_seed(effect->definition_index),
				event->duration_lower_bound,
				event->duration_upper_bound);

			/* port: no more particles than the effect counts. A tag with more
			is cut to that, and said once. */
			if (event->particles.count > (long)NUMBEROF(effect->particle_counts))
			{
				error(
					_error_silent,
					"effect %s has an event with %d particles (only %d are made)",
					tag_get_name(effect->definition_index),
					event->particles.count,
					(long)NUMBEROF(effect->particle_counts));
				event->particles.count = NUMBEROF(effect->particle_counts);
			}

			for (particle_index = 0;
				particle_index < event->particles.count;
				particle_index++)
			{
				struct effect_particles_definition *particles = TAG_BLOCK_GET_ELEMENT(
					&event->particles,
					particle_index,
					struct effect_particles_definition);
				byte count = (byte)(long)effect_real_random_range(
					get_global_local_random_seed_address(),
					effect,
					particles->count_lower_bound,
					particles->count_upper_bound,
					particles->scale_a_flags,
					particles->scale_b_flags,
					_effect_particle_count_bit);

				effect->particle_counts[particle_index] = count;

				if (count > MAXIMUM_SPLIT_SCREEN_PARTICLE_COUNT)
				{
					effect->particle_counts[particle_index] = (byte)(long)
						(((real)count - MAXIMUM_SPLIT_SCREEN_PARTICLE_COUNT) /
						local_player_count() +
						MAXIMUM_SPLIT_SCREEN_PARTICLE_COUNT);
				}
			}

			if (!TEST_FLAG(effect->header.flags, _effect_invisible_bit))
				effect_generate_parts(effect_index, effect);
		}
	}

	return;
}

static real effect_scale(
	struct effect_datum const *effect,
	real value,
	unsigned long scale_a_flags,
	unsigned long scale_b_flags,
	short bit_index)
{
	real scaled_value = value;

	if (TEST_FLAG(scale_a_flags, bit_index))
		scaled_value *= effect->scale_a;
	if (TEST_FLAG(scale_b_flags, bit_index))
		scaled_value *= effect->scale_b;

	return scaled_value;
}

static real effect_real_random_range(
	unsigned long *seed,
	struct effect_datum const *effect,
	real lower_bound,
	real upper_bound,
	unsigned long scale_a_flags,
	unsigned long scale_b_flags,
	short first_bit_index)
{
	real base = effect_scale(
		effect,
		lower_bound,
		scale_a_flags,
		scale_b_flags,
		first_bit_index);
	real range = effect_scale(
		effect,
		upper_bound - lower_bound,
		scale_a_flags,
		scale_b_flags,
		(short)(first_bit_index + 1));

	return real_seed_random_range(seed, 0.0f, range) + base;
}

static void effect_random_translational_velocity(
	unsigned long *seed,
	struct effect_datum const *effect,
	real_vector3d const *forward,
	real_vector3d *direction,
	real_vector3d *velocity,
	real lower_bound,
	real upper_bound,
	real cone_angle,
	unsigned long scale_a_flags,
	unsigned long scale_b_flags)
{
	real magnitude = effect_real_random_range(
		seed,
		effect,
		lower_bound,
		upper_bound,
		scale_a_flags,
		scale_b_flags,
		_effect_velocity_bit);
	real angle;

	cone_angle = effect_scale(
		effect,
		cone_angle,
		scale_a_flags,
		scale_b_flags,
		_effect_velocity_cone_bit);
	angle = real_seed_random(seed) * cone_angle;

	*direction = *forward;

	if (angle != 0.0f)
	{
		real_vector3d axis;

		rotate_vector_about_axis(
			direction,
			seed_random_direction3d(seed, &axis),
			(real)sin(angle),
			(real)cos(angle));
	}

	velocity->i = magnitude * direction->i;
	velocity->j = magnitude * direction->j;
	velocity->k = magnitude * direction->k;

	return;
}

static void effect_random_angular_velocity(
	unsigned long *seed,
	struct effect_datum const *effect,
	real_vector3d *velocity,
	real lower_bound,
	real upper_bound,
	unsigned long scale_a_flags,
	unsigned long scale_b_flags)
{
	real magnitude = effect_real_random_range(
		seed,
		effect,
		lower_bound,
		upper_bound,
		scale_a_flags,
		scale_b_flags,
		_effect_angular_velocity_bit);

	if (magnitude != 0.0f)
	{
		seed_random_direction3d(seed, velocity);
		velocity->i *= magnitude;
		velocity->j *= magnitude;
		velocity->k *= magnitude;
	}
	else
	{
		*velocity = *global_zero_vector3d;
	}

	return;
}

static boolean effect_allowed_by_environment(
	long effect_index,
	short environment,
	real_point3d const *world_point)
{
	struct halo_sim_effect_call call;
	boolean allowed;

	switch (environment)
	{
		case _effect_environment_anywhere:
			allowed = TRUE;
			break;

		case _effect_environment_air:
			effect_call_new(&call, _effect_call_underwater);
			effect_call_vector(&call.f[0], &world_point->x);
			effect_call(effect_index, &call);
			allowed = !call.result;
			break;

		case _effect_environment_water:
			effect_call_new(&call, _effect_call_underwater);
			effect_call_vector(&call.f[0], &world_point->x);
			effect_call(effect_index, &call);
			allowed = call.result != 0;
			break;

		case _effect_environment_vacuum:
			allowed = FALSE;
			break;

		default:
			match_vassert(
				"c:\\halo\\SOURCE\\effects\\effects.c",
				888,
				FALSE,
				NULL);
			/* BUG (original, preserved for exact matching): this arm leaves allowed
			 * unassigned, and January returns it after the fatal assertion: 0x48b160 +0x5f mov al,[ebp-1]
			 * reads the never-written byte slot. The later /Od+/RTC build attests the uninitialised
			 * declaration: its single exit calls _RTC_UninitUse("allowed").
			 * The read is not executed in January: display_assert returns into an unconditional
			 * system_exit, which never returns (it jumps to halt_and_catch_fire, which loops, or calls
			 * _exit on re-entry), and no other path reaches the read. The arm itself is entered only for
			 * an environment value outside the four cases above, each of which assigns allowed; the
			 * value comes from effect tag data, so this arm is not proven unreachable. A corrected build
			 * assigns allowed in this arm. */
			break;
	}

	return allowed;
}

static boolean effect_part_allowed_by_disposition(
	boolean nonviolent,
	short disposition)
{
	if (nonviolent)
		return disposition != _effect_disposition_violent;

	return disposition != _effect_disposition_nonviolent;
}

static real_matrix4x3 *effect_get_node_matrix(
	struct effect_datum const *effect,
	short node_designator)
{
	short node_index;

	if (node_designator != NONE &&
		TEST_FLAG(node_designator, _effect_location_first_person_bit))
	{
		return first_person_weapon_get_node_matrix(
			effect->local_player_index,
			(short)(node_designator &
				(FLAG(_effect_location_first_person_bit) - 1)));
	}

	if (node_designator == NONE)
		node_index = NONE;
	else
		node_index = (short)(node_designator & (FLAG(_effect_location_first_person_bit) - 1));

	return object_get_node_matrix(effect->object_index, node_index);
}

static real effect_evaluate_function_integral(
	short function_index,
	real fraction)
{
	real integral;

	if (fraction == -1.0f)
		return 0.0f;

	switch (function_index)
	{
		case _effect_particle_distribution_function_start:
			integral = 1.0f;
			break;

		case _effect_particle_distribution_function_end:
			if (fraction < 1.0f)
				integral = 0.0f;
			else
				integral = 1.0f;
			break;

		case _effect_particle_distribution_function_constant:
			integral = fraction;
			break;

		case _effect_particle_distribution_function_buildup:
			integral = fraction * fraction;
			break;

		case _effect_particle_distribution_function_falloff:
			integral = (2.0f - fraction) * fraction;
			break;

		case _effect_particle_distribution_function_quadratic:
			integral = (3.0f - (fraction + fraction)) * fraction * fraction;
			break;

		default:
			match_vassert(
				"c:\\halo\\SOURCE\\effects\\effects.c",
				1886,
				FALSE,
				NULL);
			integral = fraction;
			break;
	}

	return integral;
}

static void effect_build_locations(
	long effect_index,
	struct effect_datum *effect,
	short source)
{
	struct effect_definition *definition =
		effect_definition_get(effect->definition_index);
	struct object_marker markers[MAXIMUM_EFFECT_INSTANCES];
	short location_index;

	/* port: no more locations than the effect holds. A tag with more is
	cut to that, and said once. */
	if (definition->locations.count > (long)NUMBEROF(effect->location_datum_indices))
	{
		error(
			_error_silent,
			"effect %s has %d locations (only %d are used)",
			tag_get_name(effect->definition_index),
			definition->locations.count,
			(long)NUMBEROF(effect->location_datum_indices));
		definition->locations.count = NUMBEROF(effect->location_datum_indices);
	}

	for (location_index = 0;
		location_index < definition->locations.count;
		location_index++)
	{
		struct effect_location_definition *location = TAG_BLOCK_GET_ELEMENT(
			&definition->locations,
			location_index,
			struct effect_location_definition);
		short marker_count;
		short marker_index;

		if (source == _effect_markers_list)
		{
			marker_count = effect_marker_list_get_markers_by_name(
				effect->object_index,
				location->marker_name,
				markers,
				NUMBEROF(markers));
		}
		else
		{
			/* port: the object's or first person weapon's markers, one by one
			through effect_call */
			struct halo_sim_effect_call call;

			marker_count = 1;
			for (marker_index = 0; marker_index < marker_count; marker_index++)
			{
				effect_call_new(&call, _effect_call_markers);
				call.a = effect->object_index;
				call.b = location_index;
				call.c = source == _effect_markers_first_person_weapon ? 1 : 0;
				call.d = marker_index;
				effect_call(effect_index, &call);
				marker_count = (short)call.result;
				if (marker_index < marker_count)
				{
					markers[marker_index].node_index = (short)call.out_location_leaf;
					memcpy(&markers[marker_index].node_matrix, call.out, sizeof(real_matrix4x3));
				}
			}
		}

		for (marker_index = 0; marker_index < marker_count; marker_index++)
		{
			struct object_marker const *marker = &markers[marker_index];
			struct halo_sim_effect_call call;
			long node_designator;

			if (marker->node_index == NONE)
				node_designator = NONE;
			else if (source == _effect_markers_first_person_weapon)
				node_designator = marker->node_index | FLAG(_effect_location_first_person_bit);
			else
				node_designator = marker->node_index & (FLAG(_effect_location_first_person_bit) - 1);

			/* port: effect_build_location, through effect_call */
			effect_call_new(&call, _effect_call_build_location);
			call.a = location_index;
			call.b = (short)node_designator;
			memcpy(call.f, &marker->node_matrix, sizeof(real_matrix4x3));
			effect_call(effect_index, &call);
			if (call.result == NONE)
				break;
		}
	}

	return;
}

static unsigned long *effect_get_random_seed(
	long definition_index)
{
	struct effect_definition *definition = effect_definition_get(definition_index);

	if (TEST_FLAG(definition->flags, _effect_definition_must_be_deterministic_bit))
		return get_global_random_seed_address();

	return get_global_local_random_seed_address();
}

static void effect_set_event(
	long effect_index,
	short event_index)
{
	struct effect_datum *effect = effect_try_and_get(effect_index);

	if (effect)
	{
		struct effect_definition *definition =
			effect_definition_get(effect->definition_index);

		if (event_index >= 0 && event_index < definition->events.count)
		{
			struct effect_event_definition *event = TAG_BLOCK_GET_ELEMENT(
				&definition->events,
				event_index,
				struct effect_event_definition);

			effect->header.flags &= ~FLAG(_effect_in_duration_bit);
			effect->event_index = event_index;
			effect->event_time = 0.0f;
			effect->event_duration = real_seed_random_range(
				effect_get_random_seed(effect->definition_index),
				event->delay_lower_bound,
				event->delay_upper_bound);
		}
	}

	return;
}

/* ---------- port: effect_call */

/* an effect update's call of the rest of the game, shared by the C's update
and the Rust port's (halo_sim.c); in check mode each is recorded. TRUE if
the effect is still there after. */
long effect_call(
	long effect_index,
	struct halo_sim_effect_call *call)
{
	/* (the datum even if the effect was deleted during its update: the
	update reads it on, as January's does) */
	struct effect_datum *effect = (struct effect_datum *)((char *)effect_data->data +
		(effect_index & 0xffff) * effect_data->size);
	long still_there;
	int opened;

	opened = halo_sim_effect_event_begin(effect_index, call);
	switch (call->kind)
	{
		case _effect_call_allocate:
			call->result = effect_allocate_datum(call->a, call->c != 0);
			break;

		case _effect_call_impulse_field:
		{
			struct effect_vector_field const *impulse_field =
				(struct effect_vector_field const *)call->a;

			if (impulse_field)
			{
				effect->impulse_field = *impulse_field;
			}
			else
			{
				effect->impulse_field.translational_function = NULL;
				effect->impulse_field.angular_function = NULL;
			}
			break;
		}

		case _effect_call_corpse:
			call->result = effects_object_is_corpse(call->a) ? 1 : 0;
			break;

		case _effect_call_local_index:
			call->result = first_person_weapon_get_local_index(call->a);
			break;

		case _effect_call_location_from_point:
		{
			struct location location = effect->location;
			real_point3d point;

			point.x = call->f[0];
			point.y = call->f[1];
			point.z = call->f[2];
			scenario_location_from_point(&location, &point);
			call->out_location_leaf = location.leaf_index;
			call->out_location_cluster = location.cluster_index;
			call->out_location_bonus = location.bonus;
			break;
		}

		case _effect_call_markers:
		{
			struct effect_definition *definition =
				effect_definition_get(effect->definition_index);
			struct effect_location_definition *location = TAG_BLOCK_GET_ELEMENT(
				&definition->locations,
				call->b,
				struct effect_location_definition);
			struct object_marker markers[MAXIMUM_EFFECT_INSTANCES];
			short marker_count = call->c
				? first_person_weapon_get_marker_by_name(call->a, location->marker_name, markers, NUMBEROF(markers))
				: object_get_marker_by_name(call->a, location->marker_name, markers, NUMBEROF(markers));

			call->result = marker_count;
			if (call->d >= 0 && call->d < marker_count)
			{
				call->out_location_leaf = markers[call->d].node_index;
				memcpy(call->out, &markers[call->d].node_matrix, sizeof(real_matrix4x3));
			}
			break;
		}

		case _effect_call_build_location:
		{
			long location_datum_index = datum_new(effect_location_data);

			if (location_datum_index != NONE)
			{
				struct effect_location_datum *location =
					effect_location_get(location_datum_index);

				location->node_designator = (short)call->b;
				memcpy(&location->matrix, call->f, sizeof(real_matrix4x3));
				location->next_instance_location_index =
					effect->location_datum_indices[call->a];
				effect->location_datum_indices[call->a] = location_datum_index;
			}
			call->result = location_datum_index;
			break;
		}

		case _effect_call_clear_locations:
			csmemset(
				effect->location_datum_indices,
				NONE,
				sizeof(effect->location_datum_indices));
			break;

		case _effect_call_object_node_matrix:
			memcpy(call->out, object_get_node_matrix(call->a, (short)call->b), sizeof(real_matrix4x3));
			break;

		case _effect_call_object:
		{
			struct object_datum *object = object_try_and_get(call->a);

			if (!object)
			{
				call->result = 0;
			}
			else
			{
				struct object_datum *parent = object_get(object_get_ultimate_parent(call->a));

				if (TEST_FLAG(parent->object.flags, _object_connected_to_map_bit))
				{
					call->result = 2;
					call->out_location_leaf = parent->object.location.leaf_index;
					call->out_location_cluster = parent->object.location.cluster_index;
					call->out_location_bonus = parent->object.location.bonus;
					call->out[0] = parent->object.translational_velocity.i;
					call->out[1] = parent->object.translational_velocity.j;
					call->out[2] = parent->object.translational_velocity.k;
				}
				else
				{
					call->result = 1;
				}
			}
			break;
		}

		case _effect_call_delete:
			effect_delete(effect_index);
			break;

		case _effect_call_detach_delete:
		{
			struct object_datum *object = object_get(call->a);
			struct object_definition *object_definition =
				object_definition_get(object->definition_index);
			short attachment_index;
			/* port: no more than the object holds (the map's count;
			objects.c makes no more than that) */
			short attachment_count = (short)MIN(
				object_definition->object.attachments.count,
				MAXIMUM_NUMBER_OF_ATTACHMENTS_PER_OBJECT);

			for (attachment_index = 0;
				attachment_index < attachment_count;
				attachment_index++)
			{
				if (object->object.attachment_indices[attachment_index] ==
					effect_index)
				{
					object->object.attachment_indices[attachment_index] =
						NONE;
					break;
				}
			}

			effect_delete(effect_index);
			break;
		}

		case _effect_call_function_value:
		{
			real value = call->f[0];

			call->result = object_get_function_value(call->a, (short)call->b, &value);
			call->out[0] = value;
			break;
		}

		case _effect_call_change_color:
		{
			struct object_datum *object = object_get(call->a);
			real_rgb_color const *color = &object->object.outgoing_change_colors[call->b];

			call->out[0] = color->red;
			call->out[1] = color->green;
			call->out[2] = color->blue;
			break;
		}

		case _effect_call_visible:
			call->result = (call->a
				? scenario_location_potentially_visible(&effect->location)
				: scenario_location_potentially_visible_local(&effect->location)) ? 1 : 0;
			break;

		case _effect_call_underwater:
		{
			real_point3d point;

			point.x = call->f[0];
			point.y = call->f[1];
			point.z = call->f[2];
			call->result = scenario_location_underwater(&effect->location, &point, NULL) ? 1 : 0;
			break;
		}

		case _effect_call_node_matrix:
		{
			real_matrix4x3 const *matrix = effect_get_node_matrix(effect, (short)call->a);

			memcpy(call->out, matrix, sizeof(*matrix));
			break;
		}

		case _effect_call_impulse:
		{
			real_point3d point;
			real_vector3d impulse;

			point.x = call->f[0];
			point.y = call->f[1];
			point.z = call->f[2];
			effect->impulse_field.translational_function(
				&impulse,
				&point,
				effect->impulse_field.user_data);
			call->out[0] = impulse.i;
			call->out[1] = impulse.j;
			call->out[2] = impulse.k;
			break;
		}

		case _effect_call_particle_system:
		{
			real_point3d point;
			real_vector3d velocity;
			real_argb_color tint;

			point.x = call->f[0];
			point.y = call->f[1];
			point.z = call->f[2];
			velocity.i = call->f[3];
			velocity.j = call->f[4];
			velocity.k = call->f[5];
			tint.alpha = 1.0f;
			tint.red = call->f[6];
			tint.green = call->f[7];
			tint.blue = call->f[8];
			particle_system_new_unattached(call->a, &point, &velocity, &tint, call->f[9]);
			break;
		}

		case _effect_call_sound:
		{
			real_point3d point;
			real_vector3d forward;

			point.x = call->f[0];
			point.y = call->f[1];
			point.z = call->f[2];
			forward.i = call->f[3];
			forward.j = call->f[4];
			forward.k = call->f[5];
			if (call->a != NONE)
			{
				object_impulse_sound_new(call->a, call->b, (short)call->c, &point, &forward, call->f[6]);
			}
			else
			{
				struct sound_location location;

				location.position = point;
				location.forward = forward;
				location.translational_velocity = *global_zero_vector3d;
				location.game_location = effect->location;

				unattached_impulse_sound_new(call->b, &location, call->f[6]);
			}
			break;
		}

		case _effect_call_object_new:
		{
			struct object_placement_data data;

			object_placement_data_new(&data, call->a, call->b);
			data.position.x = call->f[0];
			data.position.y = call->f[1];
			data.position.z = call->f[2];
			data.forward.i = call->f[3];
			data.forward.j = call->f[4];
			data.forward.k = call->f[5];
			data.up.i = call->f[6];
			data.up.j = call->f[7];
			data.up.k = call->f[8];
			data.translational_velocity.i = call->f[9];
			data.translational_velocity.j = call->f[10];
			data.translational_velocity.k = call->f[11];
			data.angular_velocity.i = call->f[12];
			data.angular_velocity.j = call->f[13];
			data.angular_velocity.k = call->f[14];
			object_new(&data);
			break;
		}

		case _effect_call_decal:
		{
			real_point3d point;
			real_vector3d velocity;

			point.x = call->f[0];
			point.y = call->f[1];
			point.z = call->f[2];
			velocity.i = call->f[3];
			velocity.j = call->f[4];
			velocity.k = call->f[5];
			decal_new(call->a, &point, &velocity, call->f[6], FALSE, NONE, NULL);
			break;
		}

		case _effect_call_damage:
		{
			struct damage_data damage;
			struct object_datum *owner =
				object_try_and_get(effect->owner_object_index);

			damage_data_new(&damage, call->a);

			if (owner)
			{
				damage.owner_player_index = owner->object.owner_player_index;
				damage.owner_object_index = effect->owner_object_index;
				damage.owner_team_index = owner->object.owner_team_index;
			}

			damage.scale = call->f[6];
			damage.location = effect->location;
			damage.origin.x = call->f[0];
			damage.origin.y = call->f[1];
			damage.origin.z = call->f[2];
			damage.epicenter = damage.origin;
			damage.direction.i = call->f[3];
			damage.direction.j = call->f[4];
			damage.direction.k = call->f[5];

			area_of_effect_cause_damage(&damage, NONE);
			break;
		}

		case _effect_call_light:
		{
			real_point3d point;
			real_vector3d forward;

			point.x = call->f[0];
			point.y = call->f[1];
			point.z = call->f[2];
			forward.i = call->f[3];
			forward.j = call->f[4];
			forward.k = call->f[5];
			light_new_unattached(call->a, call->b, (short)call->c, &point, &forward, call->f[6]);
			break;
		}

		case _effect_call_particle:
		{
			struct new_particle_data data;

			memset(&data, 0, sizeof(data));
			data.definition_index = call->a;
			data.object_index = call->b;
			data.node_index = (short)(call->c & 0xffff);
			data.attached_to_local_player = (call->c >> 16) & 1;
			data.dont_draw_first_person = (call->c >> 17) & 1;
			data.dont_draw_third_person = (call->c >> 18) & 1;
			data.local_player_index = (short)call->d;
			data.position.x = call->f[0];
			data.position.y = call->f[1];
			data.position.z = call->f[2];
			data.direction.i = call->f[3];
			data.direction.j = call->f[4];
			data.direction.k = call->f[5];
			data.velocity.i = call->f[6];
			data.velocity.j = call->f[7];
			data.velocity.k = call->f[8];
			data.initial_impulse.i = call->f[9];
			data.initial_impulse.j = call->f[10];
			data.initial_impulse.k = call->f[11];
			data.rotation = call->f[12];
			data.angular_velocity = call->f[13];
			data.radius = call->f[14];
			data.color.alpha = call->f[15];
			data.color.red = call->f[16];
			data.color.green = call->f[17];
			data.color.blue = call->f[18];
			particle_new(&data);
			break;
		}
	}
	/* (an allocation's effect is the new one) */
	if (call->kind == _effect_call_allocate)
		effect_index = call->result;
	still_there = effect_try_and_get(effect_index) != NULL;
	halo_sim_effect_event_end(effect_index, call, still_there, opened);

	return still_there;
}

/* ---------- port: the effect's state for halo_sim.c */

int effect_halo_sim_state_load(
	long effect_index,
	struct halo_sim_effect_state *state)
{
	struct effect_datum *effect = effect_try_and_get(effect_index);

	state->random_seed = *get_global_random_seed_address();
	state->local_random_seed = *get_global_local_random_seed_address();
	if (!effect)
		return FALSE;
	state->effect_index = (unsigned long)effect_index;
	state->definition_index = (unsigned long)effect->definition_index;
	state->flags = effect->header.flags;
	state->scale_a_function_index = effect->scale_a_function_index;
	state->scale_b_function_index = effect->scale_b_function_index;
	state->change_color_index = effect->change_color_index;
	state->location_leaf = effect->location.leaf_index;
	state->location_cluster = effect->location.cluster_index;
	state->location_bonus = effect->location.bonus;
	state->color[0] = effect->color.red;
	state->color[1] = effect->color.green;
	state->color[2] = effect->color.blue;
	state->velocity[0] = effect->velocity.i;
	state->velocity[1] = effect->velocity.j;
	state->velocity[2] = effect->velocity.k;
	state->object_index = effect->object_index;
	state->owner_object_index = effect->owner_object_index;
	state->scale_a = effect->scale_a;
	state->scale_b = effect->scale_b;
	state->local_player_index = effect->local_player_index;
	state->event_index = effect->event_index;
	state->event_time = effect->event_time;
	state->event_duration = effect->event_duration;
	state->last_event_fraction = effect->last_event_fraction;
	memcpy(state->particle_counts, effect->particle_counts, sizeof(state->particle_counts));
	state->has_translational_impulse = effect->impulse_field.translational_function ? 1 : 0;

	return TRUE;
}

void effect_halo_sim_state_store(
	struct halo_sim_effect_state const *state)
{
	struct effect_datum *effect = effect_try_and_get((long)state->effect_index);

	*get_global_random_seed_address() = state->random_seed;
	*get_global_local_random_seed_address() = state->local_random_seed;
	if (!effect)
		return;
	effect->definition_index = (long)state->definition_index;
	effect->owner_object_index = state->owner_object_index;
	effect->object_index = state->object_index;
	effect->local_player_index = state->local_player_index;
	effect->scale_a_function_index = state->scale_a_function_index;
	effect->scale_b_function_index = state->scale_b_function_index;
	effect->change_color_index = state->change_color_index;
	effect->header.flags = state->flags;
	effect->location.leaf_index = state->location_leaf;
	effect->location.cluster_index = state->location_cluster;
	effect->location.bonus = state->location_bonus;
	effect->color.red = state->color[0];
	effect->color.green = state->color[1];
	effect->color.blue = state->color[2];
	effect->velocity.i = state->velocity[0];
	effect->velocity.j = state->velocity[1];
	effect->velocity.k = state->velocity[2];
	effect->scale_a = state->scale_a;
	effect->scale_b = state->scale_b;
	effect->event_index = state->event_index;
	effect->event_time = state->event_time;
	effect->event_duration = state->event_duration;
	effect->last_event_fraction = state->last_event_fraction;
	memcpy(effect->particle_counts, state->particle_counts, sizeof(effect->particle_counts));
}

int effect_halo_sim_instances(
	long effect_index,
	long *first_instances,
	struct halo_sim_effect_instance *instances,
	unsigned long maximum_instance_count,
	unsigned long *instance_count)
{
	struct effect_datum *effect = effect_try_and_get(effect_index);
	struct effect_definition *definition;
	short location_count;
	short location_index;
	unsigned long count = 0;

	*instance_count = 0;
	for (location_index = 0; location_index < MAXIMUM_EFFECT_LOCATIONS; location_index++)
		first_instances[location_index] = NONE;
	if (!effect)
		return FALSE;
	definition = effect_definition_get(effect->definition_index);
	location_count = (short)MIN(definition->locations.count, (long)NUMBEROF(effect->location_datum_indices));
	for (location_index = 0; location_index < location_count; location_index++)
	{
		long location_datum_index = effect->location_datum_indices[location_index];
		long *link = &first_instances[location_index];

		while (location_datum_index != NONE)
		{
			struct effect_location_datum *location = effect_location_get(location_datum_index);
			struct halo_sim_effect_instance *instance;

			if (count >= maximum_instance_count)
				return FALSE;
			instance = &instances[count];
			instance->node_designator = location->node_designator;
			instance->pad = 0;
			instance->next = NONE;
			memcpy(instance->matrix, &location->matrix, sizeof(instance->matrix));
			*link = (long)count;
			link = &instance->next;
			count++;
			location_datum_index = location->next_instance_location_index;
		}
	}
	*instance_count = count;

	return TRUE;
}
