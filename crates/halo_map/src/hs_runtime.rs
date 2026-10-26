//! The script runtime: its level start (`hs_runtime.c`'s
//! `hs_runtime_initialize_for_new_map`: the scenario's globals initialized
//! and its scripts' threads), a console command's evaluation
//! (`hs_runtime_evaluate`) and its update (`hs_runtime_update`, with
//! `hs_thread_main`, `hs_evaluate`, the thread stacks, `hs_return`, casting,
//! globals and the built-in functions: `begin`, `begin_random`, `if`, `set`,
//! `and`, `or`, arithmetic, comparisons, `sleep`, `sleep_until`, `wake`,
//! `inspect` and the object cast).
//!
//! Script threads, their stacks (frames linked by pointers), the globals and
//! the random seed are the game's state, saved with its games, so the runtime
//! works on the game's memory in place, through [`HsMemory`]. The library's
//! functions (`hs.c`'s) are the game's, called through
//! [`HsHost::call`] (hs_runtime.c's `hs_runtime_call`, shared with the C): a
//! library function's evaluation works on the same stack with the game's own
//! runtime calls. So do the object lists, the questions the runtime asks, and
//! its messages.

const NONE: i32 = -1;

/// `struct hs_thread_datum`.
const THREAD_TYPE: u32 = 2;
const THREAD_FLAGS: u32 = 3;
const THREAD_SCRIPT_INDEX: u32 = 4;
const THREAD_SLEEP_UNTIL: u32 = 8;
const THREAD_PREVIOUS_SLEEP_UNTIL: u32 = 12;
const THREAD_STACK: u32 = 16;
const THREAD_RESULT: u32 = 20;
const THREAD_STACK_DATA: u32 = 24;
/// `HS_THREAD_STACK_SIZE`.
const STACK_SIZE: u32 = 0x200;

/// `struct hs_stack_frame`.
const FRAME_PREVIOUS: u32 = 0;
const FRAME_EXPRESSION_INDEX: u32 = 4;
const FRAME_RESULT: u32 = 8;
const FRAME_SIZE: u32 = 12;
const FRAME_DATA: u32 = 14;
const FRAME_HEADER: u32 = 16;

/// `struct hs_syntax_node`.
const NODE_INDEX: u32 = 2;
const NODE_TYPE: u32 = 4;
const NODE_FLAGS: u32 = 6;
const NODE_NEXT: u32 = 8;
const NODE_DATA: u32 = 16;

/// `struct hs_function_definition`.
const FUNCTION_RETURN_TYPE: u32 = 0;

/// `struct hs_external_global_definition`.
const EXTERNAL_TYPE: u32 = 4;
const EXTERNAL_ADDRESS: u32 = 8;

/// `struct hs_script` and `struct hs_global` (92 bytes each).
const SCRIPT_SIZE: u32 = 92;
const SCRIPT_TYPE: u32 = 32;
const SCRIPT_RETURN_TYPE: u32 = 34;
const SCRIPT_ROOT_EXPRESSION_INDEX: u32 = 36;
const GLOBAL_SIZE: u32 = 92;
const GLOBAL_TYPE: u32 = 32;
const GLOBAL_INITIALIZATION_EXPRESSION_INDEX: u32 = 40;
/// `DATUM_INDEX_NEW`'s salt for the globals.
const GLOBAL_SALT: i32 = 0xaced;

/// `struct hs_runtime_globals`.
const RUNTIME_INITIALIZED: u32 = 0;
const RUNTIME_EXECUTING_THREAD_INDEX: u32 = 2;

/// Thread types.
const THREAD_TYPE_SCRIPT: u8 = 0;
const THREAD_TYPE_GLOBAL_INITIALIZE: u8 = 1;
const THREAD_TYPE_CONSOLE_COMMAND: u8 = 2;

/// Thread flags.
const THREAD_IN_FUNCTION_CALL: u8 = 1 << 0;
const THREAD_SLEEPING: u8 = 1 << 1;
const THREAD_STACK_OVERFLOW: u8 = 1 << 2;

/// Syntax node flags.
const NODE_PRIMITIVE: i16 = 1 << 0;
const NODE_SCRIPT: i16 = 1 << 1;
const NODE_GLOBAL: i16 = 1 << 2;

/// Script types.
const SCRIPT_STARTUP: i16 = 0;
const SCRIPT_DORMANT: i16 = 1;
const SCRIPT_STATIC: i16 = 3;
const SCRIPT_STUB: i16 = 4;

/// `enum hs_type`.
const HS_UNPARSED: i16 = 0;
const HS_PASSTHROUGH: i16 = 3;
const HS_TYPE_VOID: i16 = 4;
const HS_TYPE_BOOLEAN: i16 = 5;
const HS_TYPE_REAL: i16 = 6;
const HS_TYPE_SHORT_INTEGER: i16 = 7;
const HS_TYPE_LONG_INTEGER: i16 = 8;
const HS_TYPE_STRING: i16 = 9;
const HS_TYPE_SCRIPT: i16 = 10;
const HS_TYPE_AI: i16 = 17;
const HS_TYPE_OBJECT_LIST: i16 = 23;
const HS_TYPE_OBJECT: i16 = 37;
const HS_TYPE_SCENERY: i16 = 42;
const HS_TYPE_OBJECT_NAME: i16 = 43;
const HS_TYPE_SCENERY_NAME: i16 = 48;
const NUMBER_OF_HS_TYPES: i16 = 49;

/// `hs_type_sizes`.
const HS_TYPE_SIZES: [i16; 49] = [
    0, 0, 0, 0, 0, 1, 4, 2, 4, 4, 4, 2, 2, 2, 2, 2, 2, 4, 2, 2, 2, 2, 2, 4, 4, 4, 4, 4, 4, 4, 4, 4, 2, 2, 2, 2, 2, 4,
    4, 4, 4, 4, 4, 2, 2, 2, 2, 2, 2,
];
/// `hs_object_type_masks`.
const HS_OBJECT_TYPE_MASKS: [u16; 6] = [0xFFFF, 0x0003, 0x0002, 0x0004, 0x0380, 0x0040];

/// `enum hs_function_index`'s built-ins.
mod function {
    pub const BEGIN: i16 = 0;
    pub const BEGIN_RANDOM: i16 = 1;
    pub const IF: i16 = 2;
    pub const SET: i16 = 4;
    pub const AND: i16 = 5;
    pub const OR: i16 = 6;
    pub const PLUS: i16 = 7;
    pub const MINUS: i16 = 8;
    pub const TIMES: i16 = 9;
    pub const DIVIDE: i16 = 10;
    pub const MIN: i16 = 11;
    pub const MAX: i16 = 12;
    pub const EQUAL: i16 = 13;
    pub const NOT_EQUAL: i16 = 14;
    pub const GT: i16 = 15;
    pub const LT: i16 = 16;
    pub const GTE: i16 = 17;
    pub const LTE: i16 = 18;
    pub const SLEEP: i16 = 19;
    pub const SLEEP_UNTIL: i16 = 20;
    pub const WAKE: i16 = 21;
    pub const INSPECT: i16 = 22;
    pub const OBJECT_TO_UNIT: i16 = 23;
}

/// The runtime's calls of the rest of the game (halo_sim.h's
/// `_hs_runtime_call_*`).
pub mod call {
    /// A library function's evaluation: `a` the function, `b` the thread,
    /// `c` whether it is initializing.
    pub const EXTERNAL: i32 = 1;
    /// `hs_scenario_script_disabled(a)`.
    pub const SCRIPT_DISABLED: i32 = 2;
    /// A library function's body, its arguments evaluated by the runtime:
    /// `a` the function, `b` the thread, `c` the arguments' address.
    pub const LIBRARY: i32 = 17;
    /// `game_in_progress()`.
    pub const GAME_IN_PROGRESS: i32 = 3;
    /// `object_index_from_name_index(a)`.
    pub const OBJECT_FROM_NAME: i32 = 4;
    /// `object_list_from_ai_reference(a)`.
    pub const OBJECT_LIST_FROM_AI: i32 = 5;
    /// A new object list holding the object `a`.
    pub const OBJECT_LIST_OF_OBJECT: i32 = 6;
    /// `object_list_add_reference(a)`.
    pub const OBJECT_LIST_ADD_REFERENCE: i32 = 7;
    /// `object_list_remove_reference(a)`.
    pub const OBJECT_LIST_REMOVE_REFERENCE: i32 = 8;
    /// The object `a`'s type.
    pub const OBJECT_TYPE: i32 = 9;
    /// A message: `a` 1 a stack overflow, 2 a corrupt syntax tree, 3
    /// unexpected parameters, 4 an object (`c`) that can't be the type `b`;
    /// the thread in `b` for the first three.
    pub const ERROR: i32 = 10;
    /// `inspect`'s printing of the value `b` of the type `a`.
    pub const INSPECT: i32 = 11;
    /// `object_list_gc()`.
    pub const OBJECT_LIST_GC: i32 = 13;
    /// `hs_node_gc()`.
    pub const NODE_GC: i32 = 14;
    // (15, a console command's thread deleted, is the runtime's own now)
    /// `hs_scenario_global_initializer_disabled(a)`.
    pub const GLOBAL_INITIALIZER_DISABLED: i32 = 16;

    /// A library body's effects on the rest of the game, made as the
    /// game's body makes them through `hs_runtime_effect` (ai_script.c's
    /// `ai_script_effect_run`); in check mode they are held back and
    /// compared with the game's body's, so a body doesn't use what they
    /// give back.
    /// `action_obey_command_list_setup` of the actor `a` and command list
    /// `b`, and if it sets up, `actor_action_change` to obey.
    pub const EFFECT_OBEY_COMMAND_LIST: i32 = 0x40;
    /// The seats (bits, `b` and `c`) of the vehicle `a` the next entries take.
    pub const EFFECT_VEHICLE_SEATS: i32 = 0x41;
    /// `actor_action_try_to_enter_vehicle` of the actor `a` and vehicle `b`, in those seats.
    pub const EFFECT_ENTER_VEHICLE: i32 = 0x42;
    /// `action_obey_advance_command_list(a)`.
    pub const EFFECT_OBEY_ADVANCE: i32 = 0x43;
    /// `actor_kill(a, b, FALSE)`.
    pub const EFFECT_ACTOR_KILL: i32 = 0x44;
    /// `actor_braindead(a, b)`.
    pub const EFFECT_ACTOR_BRAINDEAD: i32 = 0x45;
    /// `actor_berserk(a, b)`.
    pub const EFFECT_ACTOR_BERSERK: i32 = 0x46;
    /// `actor_action_set_default_state(a, b)`.
    pub const EFFECT_SET_DEFAULT_STATE: i32 = 0x47;
    /// `unit_try_and_exit_seat(a)`.
    pub const EFFECT_UNIT_EXIT_SEAT: i32 = 0x48;
    /// `actor_look_secondary_stop(a)`.
    pub const EFFECT_LOOK_SECONDARY_STOP: i32 = 0x49;
    /// The actor `a`'s scripted look at the object `b`.
    pub const EFFECT_LOOK_AT_OBJECT: i32 = 0x4a;
    /// `actor_delete(a, FALSE)`.
    pub const EFFECT_ACTOR_DELETE: i32 = 0x4b;
    /// `encounter_force_activate(a)`.
    pub const EFFECT_ENCOUNTER_FORCE_ACTIVATE: i32 = 0x4c;
    /// The actor `a`'s magic sight of the unit `b` (its prop, then the effect).
    pub const EFFECT_MAGIC_SIGHT: i32 = 0x4d;
    /// `encounter_squad_timer_expire(a, b)`.
    pub const EFFECT_SQUAD_TIMER_EXPIRE: i32 = 0x4e;
    /// `ai_erase(a, b, c, FALSE)`.
    pub const EFFECT_AI_ERASE: i32 = 0x4f;
    /// `encounter_create(a, b, c)`.
    pub const EFFECT_ENCOUNTER_CREATE: i32 = 0x50;
    /// `encounter_spawn_actor(a, b)`.
    pub const EFFECT_ENCOUNTER_SPAWN_ACTOR: i32 = 0x51;
    /// `encounter_link_activation(a, b)`.
    pub const EFFECT_LINK_ACTIVATION: i32 = 0x52;
    /// `ai_conversation_stop(a)`.
    pub const EFFECT_CONVERSATION_STOP: i32 = 0x53;
    /// `ai_conversation_advance(a)`.
    pub const EFFECT_CONVERSATION_ADVANCE: i32 = 0x54;
    /// `game_allegiance_create`: `a` the teams (16 bits each), `b` the
    /// incident threshold and decay time, `c` each team's suspicion and the
    /// communication it requires (bits).
    pub const EFFECT_ALLEGIANCE_CREATE: i32 = 0x55;
    /// `game_allegiance_remove(a, b)`.
    pub const EFFECT_ALLEGIANCE_REMOVE: i32 = 0x56;
    /// `actor_set_team(a, b)`.
    pub const EFFECT_ACTOR_SET_TEAM: i32 = 0x57;
    /// `ai_update_team_status()`.
    pub const EFFECT_UPDATE_TEAM_STATUS: i32 = 0x58;
    /// The actor `a` out of its encounter (its positions flushed) and
    /// encounterless.
    pub const EFFECT_FREE_ACTOR: i32 = 0x59;
    /// `encounters_update_dirty_status()`.
    pub const EFFECT_ENCOUNTERS_UPDATE_DIRTY_STATUS: i32 = 0x5a;
    /// An actor for the unit `a` of the encounter `b`'s squad `c`'s palette
    /// variant (`ai_scripting_attach_squad`).
    pub const EFFECT_ATTACH: i32 = 0x5b;
    /// An encounterless actor of the variant `b` for the unit `a`
    /// (`ai_scripting_attach_free_actor`).
    pub const EFFECT_ATTACH_FREE: i32 = 0x5c;
    /// `actor_change_encounter(a, b, c)`.
    pub const EFFECT_CHANGE_ENCOUNTER: i32 = 0x5d;
    /// The actor `a`'s teleport to its starting location, `b` only if
    /// unsupported (`ai_scripting_teleport_actor`).
    pub const EFFECT_TELEPORT_STARTING_LOCATION: i32 = 0x5e;
    /// The actor `a`'s grenades renewed (`ai_scripting_renew_grenades`).
    pub const EFFECT_RENEW_GRENADES: i32 = 0x5f;
    /// `actor_stimulus_maneuvering(a, b, FALSE)`.
    pub const EFFECT_STIMULUS_MANEUVERING: i32 = 0x60;
    /// The encounterless actor `a` attached to the encounter `b`'s squad `c`.
    pub const EFFECT_ENCOUNTER_REATTACH: i32 = 0x61;
    /// `ai_update_team_status()` if the encounters `a` and `b`'s teams
    /// differ as it's made (after the effects before it).
    pub const EFFECT_TEAM_STATUS_IF_TEAMS_DIFFER: i32 = 0x62;
    /// `network_coop_note_player_effect` of the scripted effect's
    /// translation, rotation, start and stop (`a`, `b` and `c` reals).
    pub const EFFECT_COOP_PLAYER_EFFECT_TRANSLATION: i32 = 0x63;
    pub const EFFECT_COOP_PLAYER_EFFECT_ROTATION: i32 = 0x64;
    pub const EFFECT_COOP_PLAYER_EFFECT_START: i32 = 0x65;
    pub const EFFECT_COOP_PLAYER_EFFECT_STOP: i32 = 0x66;

    /// Whether the call is a body's effect.
    pub fn is_effect(kind: i32) -> bool {
        (EFFECT_OBEY_COMMAND_LIST..=EFFECT_COOP_PLAYER_EFFECT_STOP).contains(&kind)
    }
}

/// The messages' kinds.
const ERROR_STACK_OVERFLOW: i32 = 1;
const ERROR_SYNTAX: i32 = 2;
const ERROR_UNEXPECTED_PARAMETERS: i32 = 3;
const ERROR_OBJECT_CAST: i32 = 4;
/// "ran out of script threads."
const ERROR_OUT_OF_SCRIPT_THREADS: i32 = 5;
/// "there are not enough threads to execute that command."
const ERROR_OUT_OF_COMMAND_THREADS: i32 = 6;
/// A global's initialization slept (the game's assertion; `b` its thread).
const ERROR_GLOBAL_INITIALIZATION_SLEPT: i32 = 7;

/// `struct hs_function_definition`'s parse procedure, parameter count and
/// parameter types.
const FUNCTION_NAME: u32 = 4;
const FUNCTION_PARSE: u32 = 8;
const FUNCTION_PARAMETER_COUNT: u32 = 24;
const FUNCTION_PARAMETER_TYPES: u32 = 26;

/// `_object_mask_all` and `_object_mask_unit` (bipeds and vehicles).
const OBJECT_MASK_ALL: u32 = 0xFFFF;
const OBJECT_MASK_UNIT: u32 = 0b11;
/// `_object_dead_bit` of an object's damage flags.
const OBJECT_DEAD: u16 = 1 << 2;
/// `NUMBER_OF_UNIT_GRENADE_TYPES`.
const UNIT_GRENADE_TYPES: u32 = 2;
/// `ai_scripting_count_internal`'s counts.
#[derive(Clone, Copy)]
enum AiCount {
    Living,
    Swarm,
    Nonswarm,
}

/// `_object_mask_weapon` and `_object_mask_device` (machines, controls
/// and light fixtures).
const OBJECT_MASK_WEAPON: u32 = 1 << 2;
const OBJECT_MASK_DEVICE: u32 = 0b111 << 7;
/// `_object_cannot_take_damage_bit` of an object's damage flags, and
/// `_unit_ignored_by_actors_bit` of a unit's flags.
const OBJECT_CANNOT_TAKE_DAMAGE: u16 = 1 << 11;
const UNIT_IGNORED_BY_ACTORS: u32 = 1 << 10;
/// `_unit_preferred_target_bit` of a unit's flags.
const UNIT_PREFERRED_TARGET: u32 = 1 << 11;
/// `struct ai_globals`' grenades switch and enterable vehicles (their count
/// first), `struct actor_datum`'s encounterless flag and vehicle action's
/// vehicle, `struct encounter_datum`'s follow target AI (the unit's union),
/// and `struct conversation_datum`'s line (tag_layout's probe).
const AI_GLOBALS_GRENADES_ENABLED: u32 = 0x3b4;
const AI_GLOBALS_ENTERABLE_VEHICLE_COUNT: u32 = 0x3b6;
const AI_GLOBALS_ENTERABLE_VEHICLES: u32 = 0x3b8;
const ACTOR_META_ENCOUNTERLESS: u32 = 0x9;
const ACTOR_VEHICLE_INDEX: u32 = 0x9c;
const ENCOUNTER_FOLLOW_TARGET_AI_INDEX: u32 = 0x64;
/// `struct unit_datum`'s encounter and squad its riders join (tag_layout's
/// probe).
const UNIT_FAKE_ENCOUNTER_INDEX: u32 = 0x2e4;
const UNIT_FAKE_SQUAD_INDEX: u32 = 0x2e6;
/// `struct actor_datum`'s disconnected encounter and squad, its variant and
/// its prevehicle encounter and squad; `struct encounter_datum`'s
/// prevehicle flag; `struct actor_definition`'s type, `struct
/// actor_variant_definition`'s actor (its index), `struct
/// squad_definition`'s palette entry, and a tag reference's size and index
/// (tag_layout's probe).
const ACTOR_DISCONNECTED_ENCOUNTER_INDEX: u32 = 0x30;
const ACTOR_DISCONNECTED_SQUAD_INDEX: u32 = 0x38;
const ACTOR_VARIANT_DEFINITION_INDEX: u32 = 0x5c;
const ACTOR_PREVEHICLE_ENCOUNTER_INDEX: u32 = 0x44;
const ACTOR_PREVEHICLE_SQUAD_INDEX: u32 = 0x48;
const ENCOUNTER_IS_PREVEHICLE: u32 = 0x1e;
const ACTOR_DEFINITION_TYPE: u32 = 0x14;
const ACTOR_VARIANT_ACTOR_INDEX: u32 = 0x10;
const SQUAD_DEFINITION_ACTOR_PALETTE_INDEX: u32 = 0x20;
const TAG_REFERENCE_SIZE: u32 = 0x10;
const TAG_REFERENCE_INDEX: u32 = 0xc;
const CONVERSATION_LINE_INDEX: u32 = 0x48;
/// `MAXIMUM_AI_ENTERABLE_VEHICLES` and `MAXIMUM_AI_INDICES_PER_ENTERABLE_VEHICLE`.
const MAXIMUM_AI_ENTERABLE_VEHICLES: i16 = 32;
const MAXIMUM_AI_INDICES_PER_ENTERABLE_VEHICLE: i16 = 6;
/// `MAXIMUM_PLATOONS_PER_ENCOUNTER` and `_PER_MAP`, `MAXIMUM_SQUADS_PER_ENCOUNTER`
/// and `_PER_MAP` (encounters.h's `encounter_get_platoon` and `_squad`
/// stop the game past them).
const MAXIMUM_PLATOONS_PER_ENCOUNTER: i16 = 32;
const MAXIMUM_PLATOONS_PER_MAP: i16 = 256;
const MAXIMUM_SQUADS_PER_ENCOUNTER: i16 = 64;
const MAXIMUM_SQUADS_PER_MAP: i16 = 1024;
/// The encounter follow targets (actors.c's `_follow_target_none` on), the
/// desired targets of an actor's external orders (`_desired_target_none`
/// on), `_actor_mode_combat`, `_actor_target_definite_orphan` and
/// `_visible_enemy`, and `_actor_action_vehicle`.
const FOLLOW_TARGET_NONE: u16 = 0;
const FOLLOW_TARGET_PLAYERS: u16 = 1;
const FOLLOW_TARGET_UNIT: u16 = 2;
const FOLLOW_TARGET_AI: u16 = 3;
const DESIRED_TARGET_NONE: u16 = 0;
const DESIRED_TARGET_AI: u16 = 1;
const DESIRED_TARGET_PLAYER: u16 = 2;
const ACTOR_MODE_COMBAT: i16 = 3;
const ACTOR_TARGET_DEFINITE_ORPHAN: i16 = 6;
const ACTOR_TARGET_VISIBLE_ENEMY: i16 = 10;
const ACTOR_ACTION_VEHICLE: i16 = crate::layout::ai_values::_ACTOR_ACTION_VEHICLE as i16;
/// `number_of_actor_default_states`, and `_game_team_player`, `_human`
/// and `_sentinel`.
const NUMBER_OF_ACTOR_DEFAULT_STATES: i16 = 12;
const GAME_TEAM_PLAYER: i16 = 1;
const GAME_TEAM_HUMAN: i16 = 2;
const GAME_TEAM_SENTINEL: i16 = 5;
/// player_control.c's action bits (`_player_control_action_bit` on), the
/// look and move flags of all directions, and its camera control's bit.
const PLAYER_CONTROL_ACTION: u32 = 1 << 0;
const PLAYER_CONTROL_JUMP: u32 = 1 << 1;
const PLAYER_CONTROL_ACCEPT: u32 = 1 << 2;
const PLAYER_CONTROL_BACK: u32 = 1 << 3;
const PLAYER_CONTROL_PRIMARY_TRIGGER: u32 = 1 << 4;
const PLAYER_CONTROL_GRENADE_TRIGGER: u32 = 1 << 5;
const PLAYER_CONTROL_ZOOM: u32 = 1 << 6;
const PLAYER_CONTROL_LOOK_UP: u32 = 1 << 7;
const PLAYER_CONTROL_LOOK_DOWN: u32 = 1 << 8;
const PLAYER_CONTROL_LOOK_LEFT: u32 = 1 << 9;
const PLAYER_CONTROL_LOOK_RIGHT: u32 = 1 << 10;
const PLAYER_CONTROL_LOOK_ALL_DIRECTIONS: u32 = 0x780;
const PLAYER_CONTROL_MOVE_ALL_DIRECTIONS: u32 = 0x7800;
const PLAYER_CONTROL_CAMERA_CONTROL_DISABLED: u32 = 1 << 0;
/// The scripted player effect's flags (`_scripted_player_effect_active_bit`,
/// `_stopping_bit`).
const SCRIPTED_PLAYER_EFFECT_ACTIVE: u32 = 1 << 0;
const SCRIPTED_PLAYER_EFFECT_STOPPING: u32 = 1 << 1;
/// `NUMBER_OF_GAME_DIFFICULTY_LEVELS`.
const NUMBER_OF_GAME_DIFFICULTY_LEVELS: i16 = 4;
/// `NUMBER_OF_GAME_TEAMS`, and `struct game_allegiance_globals`' ally and
/// friendly bit vectors (game_allegiance.c).
const NUMBER_OF_GAME_TEAMS: i16 = 10;
const ALLEGIANCE_ALLY_BITVECTOR: u32 = 0x94;
const ALLEGIANCE_FRIENDLY_BITVECTOR: u32 = 0xa4;
/// `struct device_group_datum`'s flags and actual value, and its
/// `_device_group_can_change_only_once_bit` and `_changed_once_bit`.
const DEVICE_GROUP_FLAGS: u32 = 2;
const DEVICE_GROUP_ACTUAL_VALUE: u32 = 4;
const DEVICE_GROUP_CAN_CHANGE_ONLY_ONCE: u16 = 1 << 0;
const DEVICE_GROUP_CHANGED_ONCE: u16 = 1 << 1;
/// `_unit_suspended_bit` of a unit's flags, `_biped_airborne_bit` of a
/// biped's.
const UNIT_SUSPENDED: u32 = 1 << 24;
const BIPED_AIRBORNE: u32 = 1 << 0;
/// `struct screen_fade_definition`'s start time, ticks and direction (its
/// colour first).
const SCREEN_FADE_START_TIME: u32 = 12;
const SCREEN_FADE_TICKS: u32 = 16;
const SCREEN_FADE_FADING_OUT: u32 = 18;
/// `struct recent_conversation`: its size, whether it couldn't begin or
/// finished well, and when it finished (its conversation's index first).
const RECENT_CONVERSATION_SIZE: u32 = 0x10;
const RECENT_CONVERSATION_UNABLE_TO_BEGIN: u32 = 2;
const RECENT_CONVERSATION_FINISHED_SUCCESSFULLY: u32 = 3;
const RECENT_CONVERSATION_FINISH_TIME: u32 = 4;
/// `_actor_action_obey`, and `_obey_metadata_waiting_for_advance_notification_bit`
/// of a simple control's metadata.
const ACTOR_ACTION_OBEY: usize = crate::layout::ai_values::_ACTOR_ACTION_OBEY;
const OBEY_WAITING_FOR_ADVANCE: u8 = 1 << 4;
/// The unit tag group, where `struct unit_definition`'s unit part and its
/// seats block are, and `struct unit_seat`'s size and label (tag_layout's
/// probe, OpenCE-bevy d964b0b3).
const UNIT_TAG: u32 = u32::from_be_bytes(*b"unit");
const UNIT_DEFINITION_UNIT: u32 = 0x17c;
const UNIT_DEFINITION_SEATS: u32 = 0x168;
const UNIT_SEAT_SIZE: u32 = 0x11c;
const UNIT_SEAT_LABEL: u32 = 4;
/// `MAXIMUM_WEAPONS_PER_UNIT`.
const UNIT_WEAPONS: i16 = 4;
/// `_object_mask_machine`.
const OBJECT_MASK_MACHINE: u32 = 1 << 7;
/// `_object_movie_star_bit` of an object's flags; `_object_melee_attack_inhibited_bit`
/// and `_object_ranged_attack_inhibited_bit` of its damage flags.
const OBJECT_MOVIE_STAR: u32 = 1 << 22;
const OBJECT_MELEE_ATTACK_INHIBITED: u16 = 1 << 7;
const OBJECT_RANGED_ATTACK_INHIBITED: u16 = 1 << 8;
/// A unit's flags: `_unit_aim_without_turning_bit`, `_not_enterable_by_player_bit`,
/// `_integrated_light_on_bit`, `_doesnt_drop_items_bit`, `_cannot_blink_bit`,
/// `_impervious_bit`, `_desired_integrated_light_on_bit` and `_off_bit`.
const UNIT_AIM_WITHOUT_TURNING: u32 = 1 << 14;
const UNIT_NOT_ENTERABLE_BY_PLAYER: u32 = 1 << 16;
const UNIT_INTEGRATED_LIGHT_ON: u32 = 1 << 19;
const UNIT_DOESNT_DROP_ITEMS: u32 = 1 << 20;
const UNIT_CANNOT_BLINK: u32 = 1 << 22;
const UNIT_IMPERVIOUS: u32 = 1 << 23;
const UNIT_DESIRED_INTEGRATED_LIGHT_ON: u32 = 1 << 28;
const UNIT_DESIRED_INTEGRATED_LIGHT_OFF: u32 = 1 << 29;
/// `struct device_datum`'s flags and `struct machine_datum`'s (tag_layout's
/// probe, OpenCE-bevy ee325349); `_device_position_changed_bit`, and a
/// machine's `_machine_does_not_operate_automatically_bit`, `_one_sided_bit`
/// and `_never_appears_locked_bit`.
const DEVICE_FLAGS: u32 = 0x1a4;
const MACHINE_FLAGS: u32 = 0x1c4;
const DEVICE_POSITION_CHANGED: u32 = 1 << 2;
const MACHINE_DOES_NOT_OPERATE_AUTOMATICALLY: u32 = 1 << 0;
const MACHINE_ONE_SIDED: u32 = 1 << 1;
const MACHINE_NEVER_APPEARS_LOCKED: u32 = 1 << 2;
/// `struct device_datum`'s power velocity (tag_layout's probe).
const DEVICE_POWER_VELOCITY: u32 = 0x1b0;
/// `_object_no_collisions_bit` of an object's flags.
const OBJECT_NO_COLLISIONS: u32 = 1 << 24;
/// `_object_mask_scenery`, and `struct scenery_datum`'s flags (bit 0: its
/// animation playing; tag_layout's probe).
const OBJECT_MASK_SCENERY: u32 = 1 << 6;
const SCENERY_FLAGS: u32 = 0x1a4;
/// An object's animation graph, and its animation state's animation and
/// frame (`struct object_datum`'s `animation`; tag_layout's probe), and a
/// unit's animation state, `_unit_state_user_animation`.
const OBJECT_ANIMATION_GRAPH_INDEX: u32 = 0x7c;
const OBJECT_ANIMATION_INDEX: u32 = 0x80;
const OBJECT_ANIMATION_FRAME_INDEX: u32 = 0x82;
const UNIT_STATE_USER_ANIMATION: u8 = 28;
/// `struct object_list_header_datum`'s count and first reference, and
/// `struct data_reference`'s datum and next reference.
const OBJECT_LIST_HEADER_COUNT: u32 = 6;
const OBJECT_LIST_HEADER_FIRST_REFERENCE: u32 = 8;
const DATA_REFERENCE_DATUM: u32 = 4;
const DATA_REFERENCE_NEXT: u32 = 8;
/// `struct scenario_trigger_volume`: its type, forward and up, and its
/// bounds (or position and extents); `MAXIMUM_TRIGGER_VOLUMES_PER_SCENARIO`.
const TRIGGER_VOLUME_SIZE: u32 = 0x60;
const TRIGGER_VOLUME_FORWARD: u32 = 0x30;
const TRIGGER_VOLUME_UP: u32 = 0x3c;
const TRIGGER_VOLUME_BOUNDS: u32 = 0x48;
const TRIGGER_VOLUME_AXIS_ALIGNED: i16 = 0;
const TRIGGER_VOLUME_ORIENTED: i16 = 1;
const MAXIMUM_TRIGGER_VOLUMES: u32 = 256;
/// `_game_difficulty_level_normal`.
const DIFFICULTY_NORMAL: i32 = 1;

/// halo_sim.h's `struct halo_sim_hs_call`.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct HsCall {
    pub kind: i32,
    pub a: i32,
    pub b: i32,
    pub c: i32,
    pub result: i32,
}

/// The game's memory, where the runtime's state lives.
pub trait HsMemory {
    fn r8(&self, address: u32) -> u8;
    fn r16(&self, address: u32) -> u16;
    fn r32(&self, address: u32) -> u32;
    fn w8(&self, address: u32, value: u8);
    fn w16(&self, address: u32, value: u16);
    fn w32(&self, address: u32, value: u32);
}

/// The runtime's memory as the data arrays' ([`crate::data_array`]).
struct AsAiMemory<'a>(&'a dyn HsMemory);

impl crate::ai_actor::AiMemory for AsAiMemory<'_> {
    fn r8(&self, address: u32) -> u8 {
        self.0.r8(address)
    }
    fn r16(&self, address: u32) -> u16 {
        self.0.r16(address)
    }
    fn r32(&self, address: u32) -> u32 {
        self.0.r32(address)
    }
    fn w8(&self, address: u32, value: u8) {
        self.0.w8(address, value)
    }
    fn w16(&self, address: u32, value: u16) {
        self.0.w16(address, value)
    }
    fn w32(&self, address: u32, value: u32) {
        self.0.w32(address, value)
    }
}

/// The game's side of the runtime.
pub trait HsHost {
    fn memory(&self) -> &dyn HsMemory;
    fn call(&self, call: &mut HsCall);
    /// Whether the runtime gives up where it is (a check's replay that has
    /// gone apart from the game's calls, whose results no longer fit it).
    fn stopped(&self) -> bool {
        false
    }
    /// Whether the library bodies that run here do (a check's replay makes
    /// them calls, as the game recorded them: each was checked as it ran).
    fn run_bodies(&self) -> bool {
        true
    }
}

/// Where the runtime's state and tables are in the game's memory.
#[derive(Debug, Clone, Copy, Default)]
pub struct HsContext {
    /// `hs_thread_data`, `hs_syntax_data`, `hs_global_data` (their
    /// `struct data_array`s).
    pub thread_data: u32,
    pub syntax_data: u32,
    pub global_data: u32,
    /// `hs_function_table.functions` and its count.
    pub functions: u32,
    pub function_count: i32,
    /// `hs_external_globals` and its count.
    pub external_globals: u32,
    pub external_global_count: i32,
    /// The scenario's scripts and globals (their elements) and counts.
    pub scripts: u32,
    pub script_count: i32,
    pub globals: u32,
    pub global_count: i32,
    /// `hs_runtime_globals`.
    pub runtime_globals: u32,
    /// Whether a scenario is loaded (`global_scenario_index != NONE`).
    pub scenario_loaded: bool,
    /// `get_global_random_seed_address()`.
    pub random_seed: u32,
    /// `game_time_get()`.
    pub game_time: i32,
    /// `_hs_type_string_default` (the empty string's address).
    pub string_default: u32,
    /// `hs_macro_function_parse`: a library function it parses has its
    /// arguments evaluated by the runtime, then its body ([`call::LIBRARY`]).
    pub macro_parse: u32,
    /// The library functions whose bodies run here (a bit per function,
    /// hs_runtime.c's `hs_runtime_rust_bodies`), and what they read:
    /// `object_header_data`, `object_list_header_data` (their data arrays)
    /// and the game's difficulty.
    pub rust_bodies: u32,
    pub object_header_data: u32,
    pub object_list_header_data: u32,
    pub difficulty: i32,
    /// `object_list_data` (the lists' references), the scenario's trigger
    /// volumes (their first element) and count, and `hs_debug_data` (a bit
    /// per volume, its last test's).
    pub object_list_data: u32,
    pub trigger_volumes: u32,
    pub trigger_volume_count: i32,
    pub debug_data: u32,
    /// `encounter_data`, `squad_array` and `platoon_array`, and the
    /// scenario's encounter count.
    pub encounter_data: u32,
    pub squad_array: u32,
    pub platoon_array: u32,
    pub encounter_count: i32,
    /// `&global_structure_bsp_index` (a script can switch it).
    pub structure_bsp_index: u32,
    /// `game_is_cooperative`, and `coop_scripts_host` (this game the server
    /// of network co-op, whose tests of all of a list's objects differ).
    pub cooperative: bool,
    pub coop_host: bool,
    /// `device_groups_data`, and whether only network co-op's host moves
    /// devices (`network_coop_devices_remote`).
    pub device_groups_data: u32,
    pub devices_remote: bool,
    /// `player_data`; the screen fade (`player_effect_globals`'s);
    /// `conversation_data`, and `ai_globals`' recent conversations and the
    /// address of their count.
    pub player_data: u32,
    pub screen_fade: u32,
    pub conversation_data: u32,
    pub recent_conversations: u32,
    pub recent_conversation_count: u32,
    /// `actor_data`, and the scenario's command lists (their first
    /// element) and count.
    pub actor_data: u32,
    pub command_lists: u32,
    pub command_list_count: i32,
    /// The loaded tags' table and its count (`cache_files_tag_instances`).
    pub tag_instances: u32,
    pub tag_count: i32,
    /// `ai_globals`, the scenario's encounters (their first element) and
    /// the teams' allegiances (`game_allegiance_globals`).
    pub ai_globals: u32,
    pub encounters: u32,
    pub allegiance_globals: u32,
    /// The scenario's actor palette (its first element) and count.
    pub actor_palette: u32,
    pub actor_palette_count: i32,
    /// The game's fields the main loop's, players' and cameras' bodies read
    /// and write: an address each ([`game_field`]).
    pub game_fields: u32,
}

/// The game's fields in the context's `game_fields` (halo_sim.h's
/// `_hs_game_field_*`), each the address of what its comment names.
pub mod game_field {
    /// `main_globals`' saving (boolean), safely and with a timeout
    /// (booleans), its save checks' timers (longs) and safe intervals
    /// (short).
    pub const MAIN_SAVING_MAP: u32 = 0;
    pub const MAIN_SAVE_MAP_SAFELY: u32 = 1;
    pub const MAIN_SAVE_MAP_TIMEOUT: u32 = 2;
    pub const MAIN_TICKS_UNTIL_NEXT_SAVE_CHECK: u32 = 3;
    pub const MAIN_TICKS_UNABLE_TO_SAVE: u32 = 4;
    pub const MAIN_SAFE_INTERVALS: u32 = 5;
    /// Won and lost (booleans), the loss's timer (short) and its tick
    /// (main.c's `main_loss_last_tick`, long).
    pub const MAIN_WON_MAP: u32 = 6;
    pub const MAIN_LOST_MAP: u32 = 7;
    pub const MAIN_LOSS_TIMER: u32 = 8;
    pub const MAIN_LOSS_LAST_TICK: u32 = 9;
    /// Revert and reset (booleans), the structure to switch to (short),
    /// the ticks to skip (short) and whether to (boolean).
    pub const MAIN_REVERT_MAP: u32 = 10;
    pub const MAIN_RESET_MAP: u32 = 11;
    pub const MAIN_SWITCH_TO_STRUCTURE_BSP_INDEX: u32 = 12;
    pub const MAIN_SKIP_TICKS: u32 = 13;
    pub const MAIN_SKIP: u32 = 14;
    /// `global_difficulty_level` (short: the next map's).
    pub const DIFFICULTY_LEVEL: u32 = 15;
    /// `players_globals->input_disabled` (boolean).
    pub const PLAYERS_INPUT_DISABLED: u32 = 16;
    /// `player_control_globals`: the action, action test and suppressed
    /// action flags and its flags (longs), then each local player's zoom
    /// level (short).
    pub const PLAYER_CONTROL_GLOBALS: u32 = 17;
    pub const PLAYER_CONTROL_ZOOM_LEVEL0: u32 = 18;
    /// `camera_script_globals.timer`, `game_time_globals->speed` (reals),
    /// `game_state_globals.revert_time` (long).
    pub const CAMERA_SCRIPT_TIMER: u32 = 22;
    pub const GAME_TIME_SPEED: u32 = 23;
    pub const GAME_STATE_REVERT_TIME: u32 = 24;
    /// The scripted player effect (its translation and rotation, its
    /// intensity, its timer and total, shorts) and the player effects'
    /// flags.
    pub const PLAYER_EFFECT_SCRIPTED: u32 = 25;
    pub const PLAYER_EFFECT_FLAGS: u32 = 26;
}

/// `(long)value` as x86 truncates it.
fn c_long(value: f32) -> i32 {
    if (-2_147_483_648.0..2_147_483_648.0).contains(&value) { value as i32 } else { i32::MIN }
}

struct Runtime<'a, H: HsHost> {
    context: &'a HsContext,
    host: &'a H,
    /// Whether the library body running has made an effect (it can't then
    /// be left to the game's, which would make it again).
    effected: std::cell::Cell<bool>,
}

impl<'a, H: HsHost> Runtime<'a, H> {
    fn new(context: &'a HsContext, host: &'a H) -> Self {
        Self { context, host, effected: std::cell::Cell::new(false) }
    }
}

impl<H: HsHost> Runtime<'_, H> {
    fn m(&self) -> &dyn HsMemory {
        self.host.memory()
    }
    fn call(&self, kind: i32, a: i32, b: i32, c: i32) -> i32 {
        let mut call = HsCall { kind, a, b, c, result: 0 };
        self.host.call(&mut call);
        call.result
    }

    /// `datum_get`: the datum's address (None for one unused or changed).
    fn datum_get(&self, data: u32, index: i32) -> Option<u32> {
        crate::data_array::datum_get(&AsAiMemory(self.m()), data, index)
    }

    /// `data_next_index`.
    fn data_next_index(&self, data: u32, index: i32) -> i32 {
        crate::data_array::data_next_index(&AsAiMemory(self.m()), data, index)
    }

    /// `datum_new`.
    fn datum_new(&self, data: u32) -> i32 {
        crate::data_array::datum_new(&AsAiMemory(self.m()), data)
    }

    /// `datum_new_at_index`.
    fn datum_new_at_index(&self, data: u32, index: i32) -> i32 {
        crate::data_array::datum_new_at_index(&AsAiMemory(self.m()), data, index)
    }

    /// `datum_delete`.
    fn datum_delete(&self, data: u32, index: i32) {
        crate::data_array::datum_delete(&AsAiMemory(self.m()), data, index)
    }

    /// `data_make_valid` (with `data_delete_all`).
    fn data_make_valid(&self, data: u32) {
        crate::data_array::data_make_valid(&AsAiMemory(self.m()), data)
    }

    /// `hs_thread_new`.
    fn thread_new(&self, thread_type: u8, script_index: i32) -> i32 {
        let m = self.m();
        let thread_index = self.datum_new(self.context.thread_data);
        if thread_index != NONE {
            let thread = self.thread(thread_index);
            let base = thread + THREAD_STACK_DATA;
            m.w32(thread + THREAD_STACK, base);
            m.w32(base + FRAME_PREVIOUS, 0);
            m.w16(base + FRAME_SIZE, 0);
            m.w32(base + FRAME_EXPRESSION_INDEX, NONE as u32);
            m.w8(thread + THREAD_TYPE, thread_type);
            m.w32(thread + THREAD_SCRIPT_INDEX, script_index as u32);
            m.w8(thread + THREAD_FLAGS, 0);
            let dormant =
                script_index != NONE && m.r16(self.script(script_index) + SCRIPT_TYPE) as i16 == SCRIPT_DORMANT;
            m.w32(thread + THREAD_SLEEP_UNTIL, if dormant { (NONE - 1) as u32 } else { 0 });
        }
        thread_index
    }

    /// `hs_thread_delete`.
    fn thread_delete(&self, thread_index: i32) {
        self.datum_delete(self.context.thread_data, thread_index);
    }

    fn thread(&self, thread_index: i32) -> u32 {
        self.datum_get(self.context.thread_data, thread_index).unwrap_or(0)
    }
    fn syntax(&self, expression_index: i32) -> Option<u32> {
        self.datum_get(self.context.syntax_data, expression_index)
    }
    fn node_index(&self, expression_index: i32) -> i16 {
        self.syntax(expression_index).map_or(0, |n| self.m().r16(n + NODE_INDEX) as i16)
    }
    fn node_type(&self, expression_index: i32) -> i16 {
        self.syntax(expression_index).map_or(0, |n| self.m().r16(n + NODE_TYPE) as i16)
    }
    fn node_next(&self, expression_index: i32) -> i32 {
        self.syntax(expression_index).map_or(NONE, |n| self.m().r32(n + NODE_NEXT) as i32)
    }
    fn node_data(&self, expression_index: i32) -> i32 {
        self.syntax(expression_index).map_or(0, |n| self.m().r32(n + NODE_DATA) as i32)
    }

    fn stack(&self, thread: u32) -> u32 {
        self.m().r32(thread + THREAD_STACK)
    }
    fn frame_expression(&self, thread: u32) -> i32 {
        self.m().r32(self.stack(thread) + FRAME_EXPRESSION_INDEX) as i32
    }
    /// The first argument's node of the frame's expression (the function's
    /// name node's next).
    fn first_argument(&self, thread: u32) -> i32 {
        self.node_next(self.node_data(self.frame_expression(thread)))
    }
    fn flags(&self, thread: u32) -> u8 {
        self.m().r8(thread + THREAD_FLAGS)
    }
    fn set_flag(&self, thread: u32, flag: u8, value: bool) {
        let flags = self.flags(thread);
        self.m().w8(thread + THREAD_FLAGS, if value { flags | flag } else { flags & !flag });
    }

    fn script(&self, script_index: i32) -> u32 {
        self.context.scripts + SCRIPT_SIZE * script_index as u32
    }
    fn function_definition(&self, function_index: i16) -> u32 {
        self.m().r32(self.context.functions + 4 * function_index as u32)
    }

    /// `hs_stack_overflow`.
    fn stack_overflow(&self, thread_index: i32) {
        let thread = self.thread(thread_index);
        if self.flags(thread) & THREAD_STACK_OVERFLOW == 0 {
            self.call(call::ERROR, ERROR_STACK_OVERFLOW, thread_index, 0);
            self.set_flag(thread, THREAD_STACK_OVERFLOW, true);
        }
    }

    /// `hs_syntax_error`.
    fn syntax_error(&self, thread_index: i32) {
        let thread = self.thread(thread_index);
        if self.flags(thread) & THREAD_STACK_OVERFLOW == 0 {
            self.call(call::ERROR, ERROR_SYNTAX, thread_index, 0);
            self.set_flag(thread, THREAD_STACK_OVERFLOW, true);
        }
    }

    /// `hs_stack_push`.
    fn stack_push(&self, thread_index: i32) -> bool {
        let m = self.m();
        let thread = self.thread(thread_index);
        let stack = self.stack(thread);
        let size = m.r16(stack + FRAME_SIZE) as i16 as i32 as u32;
        let new_frame = stack.wrapping_add(size).wrapping_add(FRAME_HEADER);
        if new_frame.wrapping_add(FRAME_HEADER) >= thread + THREAD_STACK_DATA + STACK_SIZE
            || self.flags(thread) & THREAD_STACK_OVERFLOW != 0
        {
            self.stack_overflow(thread_index);
            return false;
        }
        m.w32(new_frame + FRAME_PREVIOUS, stack);
        m.w32(thread + THREAD_STACK, new_frame);
        m.w16(new_frame + FRAME_SIZE, 0);
        true
    }

    /// `hs_stack_allocate`: the space's address, or None (a stack overflow).
    fn stack_allocate(&self, thread_index: i32, size: i32) -> Option<u32> {
        let m = self.m();
        let thread = self.thread(thread_index);
        let frame = self.stack(thread);
        let frame_size = m.r16(frame + FRAME_SIZE) as i16 as i32;
        let end = (thread + THREAD_STACK_DATA + STACK_SIZE) as i64;
        let used = (frame + FRAME_DATA) as i64 + frame_size as i64;
        if size < 0 || size as i64 > end - used || self.flags(thread) & THREAD_STACK_OVERFLOW != 0 {
            self.stack_overflow(thread_index);
            return None;
        }
        let result = (frame + FRAME_DATA).wrapping_add(frame_size as u32);
        m.w16(frame + FRAME_SIZE, (frame_size + size) as i16 as u16);
        Some(result)
    }

    /// `hs_stack_pop`.
    fn stack_pop(&self, thread_index: i32) {
        let thread = self.thread(thread_index);
        let previous = self.m().r32(self.stack(thread) + FRAME_PREVIOUS);
        self.m().w32(thread + THREAD_STACK, previous);
    }

    /// `hs_global_get_type`.
    fn global_type(&self, designator: i16) -> i16 {
        let index = (designator as i32) & 0x7FFF;
        if designator as u16 & 0x8000 != 0 {
            if index >= self.context.external_global_count {
                return HS_UNPARSED;
            }
            let external = self.m().r32(self.context.external_globals + 4 * index as u32);
            return self.m().r16(external + EXTERNAL_TYPE) as i16;
        }
        if index >= self.context.global_count {
            return HS_UNPARSED;
        }
        self.m().r16(self.context.globals + GLOBAL_SIZE * index as u32 + GLOBAL_TYPE) as i16
    }

    /// The global's datum (its value at +4).
    fn global_datum(&self, designator: i16) -> u32 {
        let index = if designator as u16 & 0x8000 != 0 {
            (designator as i32) & 0x7FFF
        } else {
            ((designator as i32) & 0x7FFF) + self.context.external_global_count
        };
        self.datum_get(self.context.global_data, index).unwrap_or(0)
    }

    /// `hs_global_reconcile_read`.
    fn global_reconcile_read(&self, designator: i16) {
        if designator as u16 & 0x8000 == 0 {
            return;
        }
        let m = self.m();
        let global = self.global_datum(designator);
        let index = (designator as i32) & 0x7FFF;
        let external = m.r32(self.context.external_globals + 4 * index as u32);
        let address = m.r32(external + EXTERNAL_ADDRESS);
        let value = global + 4;
        let ty = self.global_type(designator);
        match external_width(ty) {
            1 => m.w8(value, if address != 0 { m.r8(address) } else { 0 }),
            2 => {
                let default = if ty == HS_TYPE_SHORT_INTEGER { 0 } else { NONE as u16 };
                m.w16(value, if address != 0 { m.r16(address) } else { default })
            }
            _ => {
                let default = match ty {
                    HS_TYPE_REAL | HS_TYPE_LONG_INTEGER => 0,
                    HS_TYPE_STRING => self.context.string_default,
                    _ => NONE as u32,
                };
                m.w32(value, if address != 0 { m.r32(address) } else { default })
            }
        }
    }

    /// `hs_global_reconcile_write`.
    fn global_reconcile_write(&self, designator: i16) {
        if designator as u16 & 0x8000 == 0 {
            return;
        }
        let m = self.m();
        let global = self.global_datum(designator);
        let index = (designator as i32) & 0x7FFF;
        let external = m.r32(self.context.external_globals + 4 * index as u32);
        let address = m.r32(external + EXTERNAL_ADDRESS);
        if address == 0 {
            return;
        }
        let value = m.r32(global + 4);
        match external_width(self.global_type(designator)) {
            1 => m.w8(address, value as u8),
            2 => m.w16(address, value as u16),
            _ => m.w32(address, value),
        }
    }

    /// `hs_global_evaluate`.
    fn global_evaluate(&self, designator: i16) -> i32 {
        self.global_reconcile_read(designator);
        self.m().r32(self.global_datum(designator) + 4) as i32
    }

    /// The `typecasting_procedures` table, as the game's lays it out (its
    /// rows past the integers sit two types early against `enum hs_type`:
    /// the real casts are from types 30 to 33, the object list's from types
    /// 16, 34 to 38 and 39 to 44), and the cast's result.
    fn typecast(&self, desired: i16, actual: i16, value: i32) -> i32 {
        let n = value as u32;
        match (desired, actual) {
            (HS_TYPE_VOID, 5..=48) => 0,
            // (the casts to a boolean or a short write only their low byte
            // or word: the rest is the argument's, as January leaves it)
            (HS_TYPE_BOOLEAN, HS_TYPE_REAL) | (HS_TYPE_BOOLEAN, HS_TYPE_LONG_INTEGER) => {
                ((n & 0xFFFF_FF00) | (n == 0) as u32) as i32
            }
            (HS_TYPE_BOOLEAN, HS_TYPE_SHORT_INTEGER) => ((n & 0xFFFF_FF00) | ((n as i16) == 0) as u32) as i32,
            // (inlined, January's upper bytes are zero)
            (HS_TYPE_BOOLEAN, HS_TYPE_STRING) => (self.string_length(n) == 0) as i32,
            (HS_TYPE_REAL, HS_TYPE_SHORT_INTEGER) => ((value as i16) as f32).to_bits() as i32,
            (HS_TYPE_REAL, HS_TYPE_LONG_INTEGER) => (value as f32).to_bits() as i32,
            (HS_TYPE_REAL, 30..=33) => (((value as i16) as i32 + 1) as f32).to_bits() as i32,
            (HS_TYPE_SHORT_INTEGER, HS_TYPE_REAL) => {
                ((n & 0xFFFF_0000) | (c_long(f32::from_bits(n)) as u16 as u32)) as i32
            }
            (HS_TYPE_SHORT_INTEGER, HS_TYPE_LONG_INTEGER) => value,
            (HS_TYPE_LONG_INTEGER, HS_TYPE_REAL) => c_long(f32::from_bits(n)),
            (HS_TYPE_OBJECT_LIST, 16) => self.call(call::OBJECT_LIST_FROM_AI, value, 0, 0),
            (HS_TYPE_OBJECT_LIST, 34..=38) => {
                if value != NONE {
                    self.call(call::OBJECT_LIST_OF_OBJECT, value, 0, 0)
                } else {
                    NONE
                }
            }
            (HS_TYPE_OBJECT_LIST, 39..=44) => {
                let object_index = self.call(call::OBJECT_FROM_NAME, value as i16 as i32, 0, 0);
                if object_index != NONE {
                    self.call(call::OBJECT_LIST_OF_OBJECT, object_index, 0, 0)
                } else {
                    NONE
                }
            }
            // (no cast: the game's table has none, and hs_can_cast keeps the
            // compiled scripts from asking)
            _ => value,
        }
    }

    fn string_length(&self, address: u32) -> u32 {
        let mut length = 0;
        while self.m().r8(address.wrapping_add(length)) != 0 {
            length += 1;
        }
        length
    }

    /// `hs_cast`.
    fn cast(&self, actual: i16, desired: i16, value: i32) -> i32 {
        let is_object = |t: i16| (HS_TYPE_OBJECT..=HS_TYPE_SCENERY).contains(&t);
        let is_object_name = |t: i16| (HS_TYPE_OBJECT_NAME..=HS_TYPE_SCENERY_NAME).contains(&t);
        if actual != desired && actual != HS_PASSTHROUGH && !is_object_name(desired) {
            if !is_object(desired) {
                return self.typecast(desired, actual, value);
            } else if is_object_name(actual) {
                return self.call(call::OBJECT_FROM_NAME, value as i16 as i32, 0, 0);
            }
        }
        value
    }

    /// `hs_return`.
    fn hs_return(&self, thread_index: i32, value: i32) {
        let m = self.m();
        let thread = self.thread(thread_index);
        let expression = self.frame_expression(thread);
        let node = self.syntax(expression).unwrap_or(0);
        let flags = m.r16(node + NODE_FLAGS) as i16;
        let index = m.r16(node + NODE_INDEX) as i16;
        let return_type = if flags & NODE_SCRIPT == 0 {
            m.r16(self.function_definition(index) + FUNCTION_RETURN_TYPE) as i16
        } else {
            m.r16(self.script(index as i32) + SCRIPT_RETURN_TYPE) as i16
        };
        let node_type = m.r16(node + NODE_TYPE) as i16;
        let previous = m.r32(self.stack(thread) + FRAME_PREVIOUS);
        let destination = m.r32(previous + FRAME_RESULT);
        let cast = self.cast(return_type, node_type, value);
        m.w32(destination, cast as u32);
        let thread = self.thread(thread_index);
        let previous = m.r32(self.stack(thread) + FRAME_PREVIOUS);
        m.w32(thread + THREAD_STACK, previous);
    }

    /// `hs_evaluate`.
    fn evaluate(&self, thread_index: i32, expression_index: i32, destination: u32) {
        let m = self.m();
        let thread = self.thread(thread_index);
        let Some(node) = self.syntax(expression_index) else {
            self.syntax_error(thread_index);
            return;
        };
        let node_type = m.r16(node + NODE_TYPE) as i16;
        if !(HS_TYPE_VOID..NUMBER_OF_HS_TYPES).contains(&node_type) {
            self.syntax_error(thread_index);
            return;
        }
        let flags = m.r16(node + NODE_FLAGS) as i16;
        if flags & NODE_PRIMITIVE != 0 {
            let data = m.r32(node + NODE_DATA) as i32;
            let value = if flags & NODE_GLOBAL != 0 {
                let designator = data as i16;
                let global_type = self.global_type(designator);
                let value = self.global_evaluate(designator);
                self.cast(global_type, node_type, value)
            } else {
                self.cast(m.r16(node + NODE_INDEX) as i16, node_type, data)
            };
            m.w32(destination, value as u32);
        } else {
            m.w32(self.stack(thread) + FRAME_RESULT, destination);
            if self.stack_push(thread_index) {
                self.set_flag(thread, THREAD_IN_FUNCTION_CALL, true);
                m.w32(self.stack(thread) + FRAME_EXPRESSION_INDEX, expression_index as u32);
            }
        }
    }

    /// `hs_arguments_evaluate`: the arguments' values' address once all are
    /// evaluated, or None.
    fn arguments_evaluate(&self, thread_index: i32, parameter_types: &[i16], initialize: bool) -> Option<u32> {
        let m = self.m();
        let thread = self.thread(thread_index);
        let values = self.stack_allocate(thread_index, 4 * parameter_types.len() as i32);
        let argument_index = self.stack_allocate(thread_index, 2);
        let expression_index = self.stack_allocate(thread_index, 4);
        let (Some(values), Some(argument_index), Some(expression_index)) = (values, argument_index, expression_index)
        else {
            return None;
        };
        if initialize {
            m.w16(argument_index, 0);
            m.w32(expression_index, self.first_argument(thread) as u32);
        }
        let argument = m.r16(argument_index) as i16;
        if (argument as usize) < parameter_types.len() {
            let expression = m.r32(expression_index) as i32;
            if self.node_type(expression) != parameter_types[argument as usize] {
                self.call(call::ERROR, ERROR_UNEXPECTED_PARAMETERS, thread_index, 0);
                return Some(values);
            }
            self.evaluate(thread_index, expression, values + 4 * argument as u32);
            m.w32(expression_index, self.node_next(expression) as u32);
            m.w16(argument_index, (argument + 1) as u16);
            return None;
        }
        Some(values)
    }

    /// `hs_find_thread_by_script`.
    fn find_thread_by_script(&self, script_index: i16) -> i32 {
        let mut thread_index = self.data_next_index(self.context.thread_data, NONE);
        while thread_index != NONE {
            let thread = self.thread(thread_index);
            if self.m().r32(thread + THREAD_SCRIPT_INDEX) as i32 == script_index as i32 {
                return thread_index;
            }
            thread_index = self.data_next_index(self.context.thread_data, thread_index);
        }
        NONE
    }

    /// `hs_wake`.
    fn wake(&self, thread_index: i32) {
        let m = self.m();
        let thread = self.thread(thread_index);
        if m.r32(thread + THREAD_SLEEP_UNTIL) as i32 != NONE {
            m.w32(thread + THREAD_SLEEP_UNTIL, 0);
            if self.flags(thread) & THREAD_SLEEPING != 0 {
                m.w32(thread + THREAD_SLEEP_UNTIL, m.r32(thread + THREAD_PREVIOUS_SLEEP_UNTIL));
                self.set_flag(thread, THREAD_SLEEPING, false);
            } else {
                let stack = self.stack(thread);
                let expression = m.r32(stack + FRAME_EXPRESSION_INDEX) as i32;
                if expression != NONE && self.node_index(expression) == function::SLEEP_UNTIL {
                    self.stack_pop(thread_index);
                    return;
                }
                let previous = m.r32(stack + FRAME_PREVIOUS);
                if previous != 0 {
                    let previous_expression = m.r32(previous + FRAME_EXPRESSION_INDEX) as i32;
                    if previous_expression != NONE && self.node_index(previous_expression) == function::SLEEP_UNTIL {
                        self.stack_pop(thread_index);
                        self.stack_pop(thread_index);
                        self.set_flag(thread, THREAD_IN_FUNCTION_CALL, false);
                    }
                }
            }
        }
    }

    // ---------- the built-in functions

    fn evaluate_begin(&self, thread_index: i32, initialize: bool) {
        let m = self.m();
        let thread = self.thread(thread_index);
        let expression_index = self.stack_allocate(thread_index, 4);
        let result = self.stack_allocate(thread_index, 4);
        let (Some(expression_index), Some(result)) = (expression_index, result) else { return };
        if initialize {
            m.w32(expression_index, self.first_argument(thread) as u32);
            m.w32(result, 0);
        }
        let expression = m.r32(expression_index) as i32;
        if expression != NONE {
            self.evaluate(thread_index, expression, result);
            m.w32(expression_index, self.node_next(expression) as u32);
        } else {
            self.hs_return(thread_index, m.r32(result) as i32);
        }
    }

    fn evaluate_begin_random(&self, thread_index: i32, initialize: bool) {
        let m = self.m();
        let thread = self.thread(thread_index);
        let argument_count = self.stack_allocate(thread_index, 2);
        let evaluated = self.stack_allocate(thread_index, 4);
        let result = self.stack_allocate(thread_index, 4);
        let (Some(argument_count), Some(evaluated), Some(result)) = (argument_count, evaluated, result) else { return };
        if initialize {
            let mut expression = self.first_argument(thread);
            let mut count: i16 = 0;
            while expression != NONE {
                expression = self.node_next(expression);
                count += 1;
            }
            m.w16(argument_count, count as u16);
            // BIT_VECTOR_SIZE_IN_BYTES
            let bytes = ((count as i32 + 31) >> 5) << 2;
            for i in 0..bytes.max(0) as u32 {
                m.w8(evaluated + i, 0);
            }
        }
        let count = m.r16(argument_count) as i16;
        // random_range(0, count): seed_random_range on the global seed
        let seed = m.r32(self.context.random_seed).wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        m.w32(self.context.random_seed, seed);
        let random = ((count as i32 as u32).wrapping_mul((seed >> 16) as u16 as u32) >> 16) as i16;
        let mut index: i16 = 0;
        while index < count {
            let choice = ((random as i32 + index as i32) % count as i32) as i16;
            let word = evaluated + 4 * (choice as u32 >> 5);
            let bit = 1u32 << (choice as u32 & 31);
            if m.r32(word) & bit == 0 {
                let mut expression = self.first_argument(thread);
                for _ in 0..choice {
                    expression = self.node_next(expression);
                }
                self.evaluate(thread_index, expression, result);
                m.w32(word, m.r32(word) | bit);
                break;
            }
            index += 1;
        }
        if index == count {
            self.hs_return(thread_index, m.r32(result) as i32);
        }
    }

    fn evaluate_if(&self, thread_index: i32, initialize: bool) {
        let m = self.m();
        let thread = self.thread(thread_index);
        let condition = self.stack_allocate(thread_index, 4);
        let expression_index = self.stack_allocate(thread_index, 4);
        let result = self.stack_allocate(thread_index, 4);
        let (Some(condition), Some(expression_index), Some(result)) = (condition, expression_index, result) else {
            return;
        };
        if initialize {
            m.w32(condition, 0);
            m.w32(expression_index, NONE as u32);
            self.evaluate(thread_index, self.first_argument(thread), condition);
        } else if m.r32(expression_index) as i32 == NONE {
            let then = self.node_next(self.first_argument(thread));
            if m.r8(condition) != 0 {
                m.w32(expression_index, then as u32);
            } else {
                let otherwise = self.node_next(then);
                m.w32(expression_index, otherwise as u32);
                if otherwise == NONE {
                    self.hs_return(thread_index, 0);
                    return;
                }
            }
            self.evaluate(thread_index, m.r32(expression_index) as i32, result);
        } else {
            self.hs_return(thread_index, m.r32(result) as i32);
        }
    }

    fn evaluate_set(&self, thread_index: i32, initialize: bool) {
        let m = self.m();
        let thread = self.thread(thread_index);
        let variable_expression = self.first_argument(thread);
        let variable = if variable_expression != NONE { self.syntax(variable_expression) } else { None };
        let Some(variable) = variable else {
            self.syntax_error(thread_index);
            return;
        };
        let flags = m.r16(variable + NODE_FLAGS) as i16;
        let designator = m.r32(variable + NODE_DATA) as i16;
        if flags & NODE_PRIMITIVE == 0 || flags & NODE_GLOBAL == 0 || self.global_type(designator) == HS_UNPARSED {
            self.syntax_error(thread_index);
            return;
        }
        if self.stack_allocate(thread_index, 4).is_none() {
            return;
        }
        let ty = self.global_type(designator);
        if initialize {
            if ty == HS_TYPE_OBJECT_LIST {
                let list = self.global_evaluate(designator);
                self.call(call::OBJECT_LIST_REMOVE_REFERENCE, list, 0, 0);
            }
            let value = self.global_datum(designator) + 4;
            self.evaluate(thread_index, self.node_next(variable_expression), value);
        } else {
            self.global_reconcile_write(designator);
            if ty == HS_TYPE_OBJECT_LIST {
                let list = self.global_evaluate(designator);
                self.call(call::OBJECT_LIST_ADD_REFERENCE, list, 0, 0);
            }
            let value = self.global_evaluate(designator);
            self.hs_return(thread_index, value);
        }
    }

    fn evaluate_logical(&self, function_index: i16, thread_index: i32, initialize: bool) {
        let m = self.m();
        let thread = self.thread(thread_index);
        let expression_index = self.stack_allocate(thread_index, 4);
        let value = self.stack_allocate(thread_index, 4);
        let result = self.stack_allocate(thread_index, 1);
        let and = function_index == function::AND;
        let (Some(expression_index), Some(value), Some(result)) = (expression_index, value, result) else { return };
        if initialize {
            m.w32(expression_index, self.first_argument(thread) as u32);
            m.w8(result, and as u8);
        } else {
            let argument = m.r8(value) != 0;
            let current = m.r8(result) != 0;
            m.w8(result, if and { current && argument } else { current || argument } as u8);
        }
        let expression = m.r32(expression_index) as i32;
        if expression != NONE && (m.r8(result) != 0) == and {
            self.evaluate(thread_index, expression, value);
            m.w32(expression_index, self.node_next(expression) as u32);
        } else {
            // (the upper bytes zero: January's were the initialize argument's
            // residue)
            self.hs_return(thread_index, m.r8(result) as i32);
        }
    }

    fn evaluate_arithmetic(&self, function_index: i16, thread_index: i32, initialize: bool) {
        let m = self.m();
        let thread = self.thread(thread_index);
        let argument_index = self.stack_allocate(thread_index, 2);
        let expression_index = self.stack_allocate(thread_index, 4);
        let value = self.stack_allocate(thread_index, 4);
        let result = self.stack_allocate(thread_index, 4);
        let (Some(argument_index), Some(expression_index), Some(value), Some(result)) =
            (argument_index, expression_index, value, result)
        else {
            return;
        };
        if initialize {
            m.w16(argument_index, 0);
            m.w32(expression_index, self.first_argument(thread) as u32);
        } else {
            let argument = f32::from_bits(m.r32(value));
            if m.r16(argument_index) == 0 {
                m.w32(result, argument.to_bits());
            } else {
                let current = f32::from_bits(m.r32(result));
                let combined = match function_index {
                    function::PLUS => current + argument,
                    function::MINUS => current - argument,
                    function::TIMES => current * argument,
                    function::DIVIDE => current / argument,
                    // MIN and MAX as the macros compare
                    function::MIN => {
                        if current < argument {
                            current
                        } else {
                            argument
                        }
                    }
                    function::MAX => {
                        if current > argument {
                            current
                        } else {
                            argument
                        }
                    }
                    _ => current,
                };
                m.w32(result, combined.to_bits());
            }
            m.w16(argument_index, m.r16(argument_index).wrapping_add(1));
        }
        let expression = m.r32(expression_index) as i32;
        if expression != NONE {
            self.evaluate(thread_index, expression, value);
            m.w32(expression_index, self.node_next(expression) as u32);
        } else {
            self.hs_return(thread_index, m.r32(result) as i32);
        }
    }

    fn evaluate_equality(&self, function_index: i16, thread_index: i32, initialize: bool) {
        let m = self.m();
        let thread = self.thread(thread_index);
        let ty = self.node_type(self.first_argument(thread));
        if let Some(arguments) = self.arguments_evaluate(thread_index, &[ty, ty], initialize) {
            let size = HS_TYPE_SIZES.get(ty as usize).copied().unwrap_or(0) as u32;
            let mut equal = (0..size).all(|i| m.r8(arguments + i) == m.r8(arguments + 4 + i));
            if function_index == function::NOT_EQUAL {
                equal = !equal;
            }
            // (the upper bytes as January leaves them: the type, twice in the
            // argument slot)
            self.hs_return(thread_index, ((ty as i32) << 16) | equal as i32);
        }
    }

    fn evaluate_inequality(&self, function_index: i16, thread_index: i32, initialize: bool) {
        let m = self.m();
        let thread = self.thread(thread_index);
        let ty = self.node_type(self.first_argument(thread));
        if let Some(arguments) = self.arguments_evaluate(thread_index, &[ty, ty], initialize) {
            let (a, b) = (m.r32(arguments), m.r32(arguments + 4));
            let (value0, value1) = match ty {
                HS_TYPE_REAL => (f32::from_bits(a), f32::from_bits(b)),
                HS_TYPE_LONG_INTEGER => (a as i32 as f32, b as i32 as f32),
                _ => ((a as i16) as f32, (b as i16) as f32),
            };
            let comparison = match function_index {
                function::GT => value0 > value1,
                function::LT => value0 < value1,
                function::GTE => value0 >= value1,
                function::LTE => value0 <= value1,
                _ => false,
            };
            // (the upper bytes as January leaves them: value1's)
            self.hs_return(thread_index, ((value1.to_bits() & 0xFFFF_FF00) | comparison as u32) as i32);
        }
    }

    fn evaluate_sleep(&self, thread_index: i32, initialize: bool) {
        let m = self.m();
        let thread = self.thread(thread_index);
        let ticks = self.stack_allocate(thread_index, 4);
        let script_index = self.stack_allocate(thread_index, 4);
        let argument_index = self.stack_allocate(thread_index, 2);
        let (Some(ticks), Some(script_index), Some(argument_index)) = (ticks, script_index, argument_index) else {
            return;
        };
        if initialize {
            self.evaluate(thread_index, self.first_argument(thread), ticks);
            m.w16(argument_index, 0);
            return;
        }
        if m.r16(argument_index) == 0 {
            let expression = self.node_next(self.first_argument(thread));
            m.w16(argument_index, 1);
            if expression != NONE {
                self.evaluate(thread_index, expression, script_index);
                return;
            }
            m.w32(script_index, NONE as u32);
        }
        if m.r16(argument_index) != 0 {
            let sleep_ticks = m.r32(ticks) as i16;
            if sleep_ticks != 0 {
                let mut sleep_thread_index = thread_index;
                if m.r32(script_index) as i16 != NONE as i16 {
                    sleep_thread_index = self.find_thread_by_script(m.r32(script_index) as i16);
                }
                if sleep_thread_index != NONE {
                    let sleep_thread = self.thread(sleep_thread_index);
                    let sleep_until =
                        if sleep_ticks < 0 { NONE - 1 } else { self.context.game_time.wrapping_add(sleep_ticks as i32) };
                    if m.r32(sleep_thread + THREAD_SLEEP_UNTIL) as i32 != NONE {
                        if sleep_thread_index != thread_index && self.flags(sleep_thread) & THREAD_SLEEPING == 0 {
                            self.set_flag(sleep_thread, THREAD_SLEEPING, true);
                            m.w32(sleep_thread + THREAD_PREVIOUS_SLEEP_UNTIL, m.r32(sleep_thread + THREAD_SLEEP_UNTIL));
                        }
                        m.w32(self.thread(sleep_thread_index) + THREAD_SLEEP_UNTIL, sleep_until as u32);
                    }
                }
            }
            self.hs_return(thread_index, 0);
        }
    }

    fn evaluate_sleep_until(&self, thread_index: i32, initialize: bool) {
        let m = self.m();
        let thread = self.thread(thread_index);
        let condition = self.stack_allocate(thread_index, 4);
        let ticks = self.stack_allocate(thread_index, 4);
        let timeout = self.stack_allocate(thread_index, 4);
        let start_time = self.stack_allocate(thread_index, 4);
        let argument_index = self.stack_allocate(thread_index, 2);
        let expression_index = self.node_next(self.first_argument(thread));
        let (Some(condition), Some(ticks), Some(timeout), Some(start_time), Some(argument_index)) =
            (condition, ticks, timeout, start_time, argument_index)
        else {
            return;
        };
        let game_time = self.context.game_time;
        if initialize {
            m.w8(condition, 0);
            m.w32(start_time, game_time as u32);
            m.w16(argument_index, 0);
            m.w16(ticks, 30);
            m.w32(timeout, NONE as u32);
            if expression_index != NONE {
                self.evaluate(thread_index, expression_index, ticks);
                return;
            }
        }
        if m.r16(argument_index) == 0 {
            m.w16(argument_index, 1);
            if expression_index != NONE {
                let timeout_expression = self.node_next(expression_index);
                if timeout_expression != NONE {
                    self.evaluate(thread_index, timeout_expression, timeout);
                    return;
                }
            }
        }
        if m.r16(argument_index) == 1 {
            let timeout_value = m.r32(timeout) as i32;
            let start = m.r32(start_time) as i32;
            if m.r8(condition) != 0 || (timeout_value != NONE && game_time >= timeout_value.wrapping_add(start)) {
                self.hs_return(thread_index, 0);
            } else {
                self.evaluate(thread_index, self.first_argument(thread), condition);
                let thread = self.thread(thread_index);
                let ticks = m.r16(ticks) as i16;
                let mut sleep_until = game_time.wrapping_add(if 1 > ticks { 1 } else { ticks as i32 });
                if timeout_value != NONE {
                    let limit = timeout_value.wrapping_add(start);
                    sleep_until = if limit < sleep_until { limit } else { sleep_until };
                }
                m.w32(thread + THREAD_SLEEP_UNTIL, sleep_until as u32);
            }
        }
    }

    fn evaluate_wake(&self, thread_index: i32) {
        let thread = self.thread(thread_index);
        let script_name_node = self.node_next(self.node_data(self.frame_expression(thread)));
        let wake_thread_index = self.find_thread_by_script(self.node_data(script_name_node) as i16);
        if wake_thread_index != NONE {
            self.wake(wake_thread_index);
        }
        self.hs_return(thread_index, 0);
    }

    fn evaluate_inspect(&self, thread_index: i32, initialize: bool) {
        let m = self.m();
        let thread = self.thread(thread_index);
        let Some(value) = self.stack_allocate(thread_index, 4) else { return };
        if initialize {
            self.evaluate(thread_index, self.first_argument(thread), value);
        } else {
            let ty = self.node_type(self.first_argument(thread));
            self.call(call::INSPECT, ty as i32, m.r32(value) as i32, 0);
            self.hs_return(thread_index, 0);
        }
    }

    fn evaluate_object_cast_up(&self, function_index: i16, thread_index: i32, initialize: bool) {
        let m = self.m();
        let thread = self.thread(thread_index);
        let Some(object_index) = self.stack_allocate(thread_index, 4) else { return };
        if initialize {
            self.evaluate(thread_index, self.first_argument(thread), object_index);
        } else if m.r32(object_index) as i32 != NONE {
            let object = m.r32(object_index) as i32;
            let object_type = function_index - function::INSPECT;
            let ty = self.call(call::OBJECT_TYPE, object, 0, 0);
            let mask = HS_OBJECT_TYPE_MASKS.get(object_type as usize).copied().unwrap_or(0) as i16;
            if mask as i32 & (1 << (ty & 31)) != 0 {
                self.hs_return(thread_index, object);
            } else {
                self.call(call::ERROR, ERROR_OBJECT_CAST, object_type as i32 + HS_TYPE_OBJECT as i32, object);
                self.hs_return(thread_index, NONE);
            }
        } else {
            self.hs_return(thread_index, NONE);
        }
    }

    /// A built-in function's evaluation, or the game's for a library
    /// function.
    fn evaluate_function(&self, function_index: i16, thread_index: i32, initialize: bool) {
        use function::*;
        match function_index {
            BEGIN => self.evaluate_begin(thread_index, initialize),
            BEGIN_RANDOM => self.evaluate_begin_random(thread_index, initialize),
            IF => self.evaluate_if(thread_index, initialize),
            SET => self.evaluate_set(thread_index, initialize),
            AND | OR => self.evaluate_logical(function_index, thread_index, initialize),
            PLUS..=MAX => self.evaluate_arithmetic(function_index, thread_index, initialize),
            EQUAL | NOT_EQUAL => self.evaluate_equality(function_index, thread_index, initialize),
            GT..=LTE => self.evaluate_inequality(function_index, thread_index, initialize),
            SLEEP => self.evaluate_sleep(thread_index, initialize),
            SLEEP_UNTIL => self.evaluate_sleep_until(thread_index, initialize),
            WAKE => self.evaluate_wake(thread_index),
            INSPECT => self.evaluate_inspect(thread_index, initialize),
            OBJECT_TO_UNIT => self.evaluate_object_cast_up(function_index, thread_index, initialize),
            // the library: a function its macro parses has its arguments
            // evaluated here, as hs_macro_function_evaluate would, then its
            // body (the game's); cond and the debug strings are the game's
            _ => {
                let definition = self.function_definition(function_index);
                let m = self.m();
                if self.context.macro_parse != 0 && m.r32(definition + FUNCTION_PARSE) == self.context.macro_parse {
                    let count = (m.r16(definition + FUNCTION_PARAMETER_COUNT) as i16).max(0) as u32;
                    let types: Vec<i16> = (0..count).map(|i| m.r16(definition + FUNCTION_PARAMETER_TYPES + 2 * i) as i16).collect();
                    // (none to evaluate for a function without parameters,
                    // whose evaluation doesn't ask for them)
                    let arguments = if types.is_empty() { Some(0) } else { self.arguments_evaluate(thread_index, &types, initialize) };
                    if let Some(arguments) = arguments {
                        // (a body that runs here, or the game's)
                        if !(self.host.run_bodies()
                            && self.rust_body(function_index)
                            && self.library_body(definition, thread_index, arguments))
                        {
                            self.call(call::LIBRARY, function_index as i32, thread_index, arguments as i32);
                        }
                    }
                } else {
                    self.call(call::EXTERNAL, function_index as i32, thread_index, initialize as i32);
                }
            }
        }
    }

    /// Whether the function's body runs here (hs_runtime.c's
    /// `hs_runtime_rust_bodies`).
    fn rust_body(&self, function_index: i16) -> bool {
        self.context.rust_bodies != 0
            && (0..1024).contains(&function_index)
            && self.m().r8(self.context.rust_bodies + (function_index as u32 >> 3)) & (1 << (function_index & 7)) != 0
    }

    /// The C string at the address (its bytes as they are, up to 256).
    fn c_string(&self, address: u32) -> Vec<u8> {
        let m = self.m();
        let mut bytes = Vec::new();
        if address == 0 {
            return bytes;
        }
        for i in 0..256 {
            let c = m.r8(address + i);
            if c == 0 {
                break;
            }
            bytes.push(c);
        }
        bytes
    }

    /// The function's name (its definition's).
    fn function_name(&self, definition: u32) -> String {
        let m = self.m();
        let name = m.r32(definition + FUNCTION_NAME);
        let mut text = String::new();
        if name == 0 {
            return text;
        }
        for i in 0..64 {
            let c = m.r8(name + i);
            if c == 0 {
                break;
            }
            text.push(c as char);
        }
        text
    }

    /// `object_try_and_get_and_verify_type`: the object's datum, if it is of
    /// one of the types (a bit per type).
    fn object_typed(&self, object_index: i32, types: u32) -> Option<u32> {
        let m = AsAiMemory(self.m());
        let header = crate::ai_actor::datum_get(&m, self.context.object_header_data, object_index)?;
        let ty = m.0.r8(header + crate::layout::object_header_datum::TYPE as u32) as u32;
        (types & (1 << ty) != 0).then(|| m.0.r32(header + crate::layout::object_header_datum::DATUM as u32))
    }

    /// `object_list_get_first` and `_get_next`: the list's objects, in order
    /// (None for a list that isn't there).
    fn object_list(&self, list: i32) -> Option<Vec<i32>> {
        let m = self.m();
        let mut objects = Vec::new();
        if list == NONE {
            return Some(objects);
        }
        let header = self.datum_get(self.context.object_list_header_data, list)?;
        let mut reference = m.r32(header + OBJECT_LIST_HEADER_FIRST_REFERENCE) as i32;
        while reference != NONE {
            let datum = self.datum_get(self.context.object_list_data, reference)?;
            objects.push(m.r32(datum + DATA_REFERENCE_DATUM) as i32);
            reference = m.r32(datum + DATA_REFERENCE_NEXT) as i32;
            if objects.len() > 0x10000 {
                return None;
            }
        }
        Some(objects)
    }

    /// `scenario_trigger_volume_test_point` (None for a volume that isn't
    /// the scenario's, which holds nothing).
    fn trigger_volume_test_point(&self, volume_index: i16, point: [f32; 3]) -> Option<bool> {
        if volume_index < 0 || volume_index as i32 >= self.context.trigger_volume_count {
            return None;
        }
        let m = self.m();
        let volume = self.context.trigger_volumes + volume_index as u32 * TRIGGER_VOLUME_SIZE;
        let f = |offset: u32| f32::from_bits(m.r32(volume + offset));
        let v = |offset: u32| [f(offset), f(offset + 4), f(offset + 8)];
        let [x, y, z] = point;
        match m.r16(volume) as i16 {
            TRIGGER_VOLUME_AXIS_ALIGNED => {
                // (bounds x0 x1 y0 y1 z0 z1)
                let b = |i: u32| f(TRIGGER_VOLUME_BOUNDS + 4 * i);
                Some(x > b(0) && y > b(2) && z > b(4) && x < b(1) && y < b(3) && z < b(5))
            }
            TRIGGER_VOLUME_ORIENTED => {
                // matrix4x3_from_point_and_vectors (left = up x forward),
                // then matrix4x3_inverse_transform_point at scale 1
                let forward = v(TRIGGER_VOLUME_FORWARD);
                let up = v(TRIGGER_VOLUME_UP);
                let position = v(TRIGGER_VOLUME_BOUNDS);
                let extents = v(TRIGGER_VOLUME_BOUNDS + 12);
                let left = [
                    up[1] * forward[2] - up[2] * forward[1],
                    up[2] * forward[0] - up[0] * forward[2],
                    up[0] * forward[1] - up[1] * forward[0],
                ];
                let (dx, dy, dz) = (x - position[0], y - position[1], z - position[2]);
                let along = |axis: [f32; 3]| dx * axis[0] + dy * axis[1] + dz * axis[2];
                let (tx, ty, tz) = (along(forward), along(left), along(up));
                Some(tx > 0.0 && ty > 0.0 && tz > 0.0 && tx < extents[0] && ty < extents[1] && tz < extents[2])
            }
            _ => None,
        }
    }

    /// `scenario_trigger_volume_test_object`: the object's bounding
    /// sphere's centre in the volume (false for no object).
    fn trigger_volume_test_object(&self, volume_index: i16, object_index: i32) -> Option<bool> {
        use crate::layout::unit_datum as u;
        if object_index == NONE {
            return Some(false);
        }
        let object = self.object_typed(object_index, OBJECT_MASK_ALL)?;
        let m = self.m();
        let center = u::OBJECT_BOUNDING_SPHERE_CENTER as u32;
        let point = [0, 4, 8].map(|i| f32::from_bits(m.r32(object + center + i)));
        Some(self.trigger_volume_test_point(volume_index, point).unwrap_or(false))
    }

    /// `ai_scripting_count_internal`: the living, swarm or nonswarm count of
    /// the encounter, platoon or squad the reference names (none for no
    /// reference, or one the scenario doesn't have), with its original
    /// count and strength; None for a reference of no kind.
    fn ai_count(&self, ai_reference: i32, count_type: AiCount) -> Option<(i32, i32, f32)> {
        use crate::layout::{encounter_datum as e, platoon_datum as pl, squad_datum as sq};
        let m = self.m();
        if ai_reference == NONE {
            return Some((0, 0, 0.0));
        }
        let kind = ai_reference as u32 >> 30;
        if kind > 2 {
            return None;
        }
        let encounter_index = ai_reference & 0xffff;
        if encounter_index >= self.context.encounter_count {
            return Some((0, 0, 0.0));
        }
        let encounter = self.datum_get(self.context.encounter_data, encounter_index)?;
        let index = ((ai_reference >> 16) & 0xff) as i16;
        // (the encounter, or its platoon or squad: where their current,
        // current swarm and original counts and strength are)
        let (datum, current, swarm, original, strength) = match kind {
            0 => (encounter, e::CURRENT_COUNT, e::CURRENT_SWARM_COUNT, e::ORIGINAL_COUNT, e::CURRENT_STRENGTH_FRACTION),
            1 => {
                if index >= m.r16(encounter + e::PLATOON_COUNT as u32) as i16 {
                    return Some((0, 0, 0.0));
                }
                let absolute = index.wrapping_add(m.r16(encounter + e::PLATOON_BASE as u32) as i16);
                let platoon = self.context.platoon_array + absolute as i32 as u32 * pl::SIZEOF as u32;
                (platoon, pl::CURRENT_COUNT, pl::CURRENT_SWARM_COUNT, pl::ORIGINAL_COUNT, pl::CURRENT_STRENGTH_FRACTION)
            }
            _ => {
                if index >= m.r16(encounter + e::SQUAD_COUNT as u32) as i16 {
                    return Some((0, 0, 0.0));
                }
                let absolute = index.wrapping_add(m.r16(encounter + e::SQUAD_BASE as u32) as i16);
                let squad = self.context.squad_array + absolute as i32 as u32 * sq::SIZEOF as u32;
                (squad, sq::CURRENT_COUNT, sq::CURRENT_SWARM_COUNT, sq::ORIGINAL_COUNT, sq::CURRENT_STRENGTH_FRACTION)
            }
        };
        let field = |offset: usize| m.r16(datum + offset as u32) as i16 as i32;
        let count = match count_type {
            AiCount::Living => field(current),
            AiCount::Swarm => field(swarm),
            AiCount::Nonswarm => (field(current) - field(swarm)).max(0),
        };
        Some((count, field(original), f32::from_bits(m.r32(datum + strength as u32))))
    }

    /// The objects of the types (a bit per type), as `object_iterator_next`
    /// gives them: each one's index and datum.
    fn objects_of_types(&self, types: u32) -> Vec<(i32, u32)> {
        use crate::layout::object_header_datum as oh;
        let m = self.m();
        let mut objects = Vec::new();
        let mut index = self.data_next_index(self.context.object_header_data, NONE);
        while index != NONE {
            if let Some(header) = self.datum_get(self.context.object_header_data, index) {
                if types & (1 << m.r8(header + oh::TYPE as u32)) != 0 {
                    objects.push((index, m.r32(header + oh::DATUM as u32)));
                }
            }
            index = self.data_next_index(self.context.object_header_data, index);
        }
        objects
    }

    /// `device_group_set_desired_value` (`device_group_change`): whether the
    /// group's value changed; None where the game's would make effects (the
    /// group powers a device), left to it.
    fn device_group_set_desired_value(&self, group_index: i16, desired_value: f32) -> Option<bool> {
        use crate::layout::device_datum as d;
        let m = self.m();
        if self.context.devices_remote {
            return Some(false);
        }
        let desired_value = if desired_value < 0.0 {
            0.0
        } else if desired_value > 1.0 {
            1.0
        } else {
            desired_value
        };
        if group_index == NONE as i16 {
            return Some(false);
        }
        let group = self.datum_get(self.context.device_groups_data, group_index as i32)?;
        let flags = m.r16(group + DEVICE_GROUP_FLAGS);
        let actual_value = f32::from_bits(m.r32(group + DEVICE_GROUP_ACTUAL_VALUE));
        if actual_value == desired_value
            || (flags & DEVICE_GROUP_CAN_CHANGE_ONLY_ONCE != 0 && flags & DEVICE_GROUP_CHANGED_ONCE != 0)
        {
            return Some(false);
        }
        let powers = self
            .objects_of_types(OBJECT_MASK_DEVICE)
            .into_iter()
            .any(|(_, device)| m.r16(device + d::DEVICE_POWER_GROUP_INDEX as u32) as i16 == group_index);
        if powers {
            return None;
        }
        m.w32(group + DEVICE_GROUP_ACTUAL_VALUE, desired_value.to_bits());
        m.w16(group + DEVICE_GROUP_FLAGS, flags | DEVICE_GROUP_CHANGED_ONCE);
        Some(true)
    }

    /// `ai_globals->ai_initialized_for_map`.
    fn ai_initialized(&self) -> bool {
        self.context.ai_globals != 0
            && self.m().r8(self.context.ai_globals + crate::layout::ai_globals::AI_INITIALIZED_FOR_MAP as u32) != 0
    }

    /// The encounter an AI reference names, as `ai_index_*_iterator_new`
    /// check it: its index and the scenario's definition, if the AI is set
    /// up and the scenario has it.
    fn ai_reference_encounter(&self, ai_reference: i32) -> Option<(i32, u32)> {
        let encounter_index = ai_reference & 0xffff;
        (self.context.scenario_loaded
            && self.context.encounters != 0
            && self.ai_initialized()
            && encounter_index < self.context.encounter_count)
            .then(|| {
                let definition = self.context.encounters
                    + encounter_index as u32 * crate::layout::encounter_definition::SIZEOF as u32;
                (encounter_index, definition)
            })
    }

    /// A tag block's count and first element.
    fn tag_block(&self, block: u32) -> (i32, u32) {
        let m = self.m();
        (m.r32(block) as i32, m.r32(block + 4))
    }

    /// `encounter_get_platoon`: None where its assertions would stop the game.
    fn encounter_platoon(&self, encounter: u32, platoon_index: i32) -> Option<u32> {
        use crate::layout::{encounter_datum as e, platoon_datum as pl};
        let m = self.m();
        let platoon_index = platoon_index as i16;
        if !(0..MAXIMUM_PLATOONS_PER_ENCOUNTER).contains(&platoon_index)
            || platoon_index >= m.r16(encounter + e::PLATOON_COUNT as u32) as i16
        {
            return None;
        }
        let absolute = platoon_index.wrapping_add(m.r16(encounter + e::PLATOON_BASE as u32) as i16);
        (0..MAXIMUM_PLATOONS_PER_MAP)
            .contains(&absolute)
            .then(|| self.context.platoon_array + absolute as u32 * pl::SIZEOF as u32)
    }

    /// `encounter_get_squad`: None where its assertions would stop the game.
    fn encounter_squad(&self, encounter: u32, squad_index: i32) -> Option<u32> {
        use crate::layout::{encounter_datum as e, squad_datum as sq};
        let m = self.m();
        let squad_index = squad_index as i16;
        if !(0..MAXIMUM_SQUADS_PER_ENCOUNTER).contains(&squad_index)
            || squad_index >= m.r16(encounter + e::SQUAD_COUNT as u32) as i16
        {
            return None;
        }
        let absolute = squad_index.wrapping_add(m.r16(encounter + e::SQUAD_BASE as u32) as i16);
        (0..MAXIMUM_SQUADS_PER_MAP)
            .contains(&absolute)
            .then(|| self.context.squad_array + absolute as u32 * sq::SIZEOF as u32)
    }

    /// `ai_index_platoon_iterator_new` and `_next`: the platoons the
    /// reference names (none for one the scenario doesn't have); None
    /// where the game's would stop.
    fn ai_platoons(&self, ai_reference: i32) -> Option<Vec<u32>> {
        use crate::layout::{encounter_definition as ed, squad_definition as sd};
        let m = self.m();
        let Some((encounter_index, definition)) = self.ai_reference_encounter(ai_reference) else { return Some(Vec::new()) };
        let (platoon_count, _) = self.tag_block(definition + ed::PLATOONS as u32);
        let (first, last) = match ai_reference as u32 >> 30 {
            0 => (0, platoon_count - 1),
            kind @ (1 | 2) => {
                let platoon_index = if kind == 2 {
                    let squad_index = (ai_reference >> 16) & 0xff;
                    let (squad_count, squads) = self.tag_block(definition + ed::SQUADS as u32);
                    if squad_index < squad_count {
                        m.r16(squads + squad_index as u32 * sd::SIZEOF as u32 + sd::PLATOON_INDEX as u32) as i16 as i32
                    } else {
                        NONE
                    }
                } else {
                    (ai_reference >> 16) & 0xff
                };
                if !(0..platoon_count).contains(&platoon_index) {
                    return Some(Vec::new());
                }
                (platoon_index, platoon_index)
            }
            _ => return Some(Vec::new()),
        };
        let mut platoons = Vec::new();
        if first <= last {
            let encounter = self.datum_get(self.context.encounter_data, encounter_index)?;
            for platoon_index in first..=last {
                platoons.push(self.encounter_platoon(encounter, platoon_index)?);
            }
        }
        Some(platoons)
    }

    /// `ai_index_squad_iterator_new` and `_next`: the squads the reference
    /// names (a platoon's, those of the platoon); None where the game's
    /// would stop.
    fn ai_squads(&self, ai_reference: i32) -> Option<Vec<(i32, u32)>> {
        use crate::layout::{encounter_definition as ed, squad_definition as sd};
        let m = self.m();
        let Some((encounter_index, definition)) = self.ai_reference_encounter(ai_reference) else { return Some(Vec::new()) };
        let (squad_count, squad_definitions) = self.tag_block(definition + ed::SQUADS as u32);
        let (first, last, required_platoon_index) = match ai_reference as u32 >> 30 {
            0 => (0, squad_count - 1, NONE),
            1 => (0, squad_count - 1, (ai_reference >> 16) & 0xff),
            2 => {
                let squad_index = (ai_reference >> 16) & 0xff;
                if squad_index >= squad_count {
                    return Some(Vec::new());
                }
                (squad_index, squad_index, NONE)
            }
            _ => return Some(Vec::new()),
        };
        let mut squads = Vec::new();
        if first <= last {
            let encounter = self.datum_get(self.context.encounter_data, encounter_index)?;
            for squad_index in first..=last {
                let squad_definition = squad_definitions + squad_index as u32 * sd::SIZEOF as u32;
                let platoon_index = m.r16(squad_definition + sd::PLATOON_INDEX as u32) as i16 as i32;
                if required_platoon_index == NONE || platoon_index == required_platoon_index {
                    squads.push((squad_index, self.encounter_squad(encounter, squad_index)?));
                }
            }
        }
        Some(squads)
    }

    /// `ai_index_actor_iterator_new` and `_next`: the actors (their index
    /// and datum) of the encounter the reference names, of its platoon or
    /// squad; None where the game's would stop.
    fn ai_actors(&self, ai_reference: i32) -> Option<Vec<(i32, u32)>> {
        use crate::layout::actor_datum as a;
        let m = self.m();
        let mut actors = Vec::new();
        let Some((encounter_index, _)) = self.ai_reference_encounter(ai_reference) else { return Some(actors) };
        let index = (ai_reference >> 16) & 0xff;
        let (platoon_index, squad_index) = match ai_reference as u32 >> 30 {
            0 => (NONE, NONE),
            1 => (index, NONE),
            2 => (NONE, index),
            _ => return Some(actors),
        };
        let encounter = self.datum_get(self.context.encounter_data, encounter_index)?;
        let mut actor_index = m.r32(encounter + crate::layout::encounter_datum::FIRST_ACTOR_INDEX as u32) as i32;
        while actor_index != NONE {
            let actor = self.datum_get(self.context.actor_data, actor_index)?;
            let field = |offset: usize| m.r16(actor + offset as u32) as i16 as i32;
            if (squad_index == NONE || squad_index == field(a::META_SQUAD_INDEX))
                && (platoon_index == NONE || platoon_index == field(a::META_PLATOON_INDEX))
            {
                actors.push((actor_index, actor));
            }
            actor_index = m.r32(actor + a::META_NEXT_ACTOR_INDEX as u32) as i32;
            if actors.len() > 0x10000 {
                return None;
            }
        }
        Some(actors)
    }

    /// `actor_iterator_new` (active only) and `actor_iterator_next`: every
    /// active encounter's actors, then the encounterless, each active. As
    /// the game's, it ends at an encounter whose last actor is inactive.
    fn active_actors(&self) -> Option<Vec<u32>> {
        use crate::layout::{actor_datum as a, encounter_datum as e};
        let m = self.m();
        let mut actors = Vec::new();
        if !self.ai_initialized() {
            return Some(actors);
        }
        let mut encounter_index = NONE;
        let mut encounters_done = false;
        let mut encounterless_done = false;
        let mut next_index = NONE;
        loop {
            if next_index == NONE {
                while !encounters_done && next_index == NONE {
                    encounter_index = self.data_next_index(self.context.encounter_data, encounter_index);
                    if encounter_index == NONE {
                        encounters_done = true;
                        break;
                    }
                    let encounter = self.datum_get(self.context.encounter_data, encounter_index)?;
                    if m.r8(encounter + e::ACTIVE as u32) != 0 {
                        next_index = m.r32(encounter + e::FIRST_ACTOR_INDEX as u32) as i32;
                    }
                }
                if encounters_done && !encounterless_done {
                    next_index = m.r32(self.context.ai_globals + crate::layout::ai_globals::FIRST_ENCOUNTERLESS_ACTOR_INDEX as u32) as i32;
                    encounterless_done = true;
                }
            }
            let mut found = None;
            while next_index != NONE {
                let actor = self.datum_get(self.context.actor_data, next_index)?;
                next_index = m.r32(actor + a::META_NEXT_ACTOR_INDEX as u32) as i32;
                if m.r8(actor + a::META_ACTIVE as u32) != 0 {
                    found = Some(actor);
                    break;
                }
            }
            match found {
                Some(actor) => actors.push(actor),
                None => return Some(actors),
            }
            if actors.len() > 0x10000 {
                return None;
            }
        }
    }

    /// `ai_scripting_find_vehicle_enterable`: the vehicle's entry, made
    /// (radius 8) if it has none; None where the game's would report too
    /// many, left to it.
    fn vehicle_enterable(&self, vehicle_index: i32) -> Option<u32> {
        use crate::layout::ai_vehicle_enterable as ve;
        let m = self.m();
        let count_address = self.context.ai_globals + AI_GLOBALS_ENTERABLE_VEHICLE_COUNT;
        let count = m.r16(count_address) as i16;
        let entry = |index: i16| self.context.ai_globals + AI_GLOBALS_ENTERABLE_VEHICLES + index as u32 * ve::SIZEOF as u32;
        let mut index = 0;
        while index < count {
            if m.r32(entry(index) + ve::VEHICLE_INDEX as u32) as i32 == vehicle_index {
                break;
            }
            index += 1;
        }
        if index >= MAXIMUM_AI_ENTERABLE_VEHICLES {
            return None;
        }
        let address = entry(index);
        if index >= count {
            for i in 0..ve::SIZEOF as u32 / 4 {
                m.w32(address + 4 * i, 0);
            }
            m.w32(address + ve::VEHICLE_INDEX as u32, vehicle_index as u32);
            m.w32(address + ve::RADIUS as u32, 8.0f32.to_bits());
            m.w16(count_address, count.wrapping_add(1) as u16);
        }
        Some(address)
    }

    /// `ai_scripting_assess_status`: how far into combat the actor is.
    fn ai_assess_status(&self, actor: u32) -> i32 {
        use crate::layout::actor_datum as a;
        let m = self.m();
        let field = |offset: usize| m.r16(actor + offset as u32) as i16;
        if m.r8(actor + a::META_ACTIVE as u32) == 0 {
            0
        } else if field(a::STATE_MODE) < ACTOR_MODE_COMBAT {
            1
        } else if field(a::STATE_COMBAT_STATUS) == 0 {
            2
        } else if field(a::TARGET_TARGET_TYPE) < ACTOR_TARGET_DEFINITE_ORPHAN {
            3
        } else if field(a::TARGET_TARGET_TYPE) < ACTOR_TARGET_VISIBLE_ENEMY {
            4
        } else if m.r8(actor + a::ORDERS_COMBAT_SHOOT_AT_TARGET as u32) != 0
            || m.r8(actor + a::ORDERS_COMBAT_THROW_GRENADE as u32) != 0
        {
            6
        } else {
            5
        }
    }

    /// The frames left of the object's animation (`unit_get_custom_animation_time`
    /// and `scenery_get_animation_time`: its frame count less its frame and
    /// 2, at least 0); None for a graph or animation not found.
    fn animation_frames_left(&self, object: u32) -> Option<i32> {
        let m = self.m();
        let tags = crate::ai_actor::AiStepContext {
            tag_instances: self.context.tag_instances,
            tag_count: self.context.tag_count,
            ..Default::default()
        };
        let graph_index = m.r32(object + OBJECT_ANIMATION_GRAPH_INDEX) as i32;
        let graph = crate::sensing::tag_get(&AsAiMemory(m), &tags, crate::fourcc(b"antr"), graph_index)?;
        let animations = graph + crate::layout::animation_graph::ANIMATIONS as u32;
        let (count, array) = (m.r32(animations) as i32, m.r32(animations + 4));
        let index = m.r16(object + OBJECT_ANIMATION_INDEX) as i16 as i32;
        if !(0..count).contains(&index) {
            return None;
        }
        let animation = array + index as u32 * crate::layout::animation::SIZEOF as u32;
        let frame_count = m.r16(animation + crate::layout::animation::FRAME_COUNT as u32) as i16 as i32;
        let frame_index = m.r16(object + OBJECT_ANIMATION_FRAME_INDEX) as i16 as i32;
        Some((frame_count - frame_index - 2).max(0))
    }

    /// `device_group_set_actual_value`: the group's value (clamped), and
    /// each device it powers or positions there at once; None for no such
    /// group, or as network co-op's host (whose snaps it counts), left to
    /// the game's.
    fn device_group_set_actual_value(&self, group_index: i16, actual_value: f32) -> Option<()> {
        use crate::layout::device_datum as d;
        let m = self.m();
        if self.context.coop_host {
            return None;
        }
        let actual_value = if actual_value < 0.0 {
            0.0
        } else if actual_value > 1.0 {
            1.0
        } else {
            actual_value
        };
        let group = self.datum_get(self.context.device_groups_data, group_index as i32)?;
        m.w32(group + DEVICE_GROUP_ACTUAL_VALUE, actual_value.to_bits());
        for (_, device) in self.objects_of_types(OBJECT_MASK_DEVICE) {
            if m.r16(device + d::DEVICE_POWER_GROUP_INDEX as u32) as i16 == group_index {
                self.set_flag32(device + DEVICE_FLAGS, DEVICE_POSITION_CHANGED, true);
                m.w32(device + d::DEVICE_POWER as u32, actual_value.to_bits());
                m.w32(device + DEVICE_POWER_VELOCITY, 0);
            }
            if m.r16(device + d::DEVICE_POSITION_GROUP_INDEX as u32) as i16 == group_index {
                self.set_flag32(device + DEVICE_FLAGS, DEVICE_POSITION_CHANGED, true);
                m.w32(device + d::DEVICE_POSITION as u32, actual_value.to_bits());
                m.w32(device + d::DEVICE_POSITION_VELOCITY as u32, 0);
            }
        }
        Some(())
    }

    /// The unit's seats' labels (its definition's seats block), None for a
    /// definition not found.
    fn unit_seats(&self, unit: u32) -> Option<Vec<Vec<u8>>> {
        let m = self.m();
        let tags = crate::ai_actor::AiStepContext {
            tag_instances: self.context.tag_instances,
            tag_count: self.context.tag_count,
            ..Default::default()
        };
        let definition_index = m.r32(unit + crate::layout::unit_datum::DEFINITION_INDEX as u32) as i32;
        let definition = crate::sensing::tag_get(&AsAiMemory(m), &tags, UNIT_TAG, definition_index)?;
        let seats = definition + UNIT_DEFINITION_UNIT + UNIT_DEFINITION_SEATS;
        let (seat_count, seat_array) = (m.r32(seats) as i32, m.r32(seats + 4));
        Some((0..seat_count.max(0)).map(|i| self.c_string(seat_array + i as u32 * UNIT_SEAT_SIZE + UNIT_SEAT_LABEL)).collect())
    }

    /// `object_list_new`: a new, empty list (NONE if none is left).
    fn object_list_new(&self) -> Option<i32> {
        let m = self.m();
        let list = self.datum_new(self.context.object_list_header_data);
        if list != NONE {
            let header = self.datum_get(self.context.object_list_header_data, list)?;
            m.w16(header + OBJECT_LIST_HEADER_COUNT, 0);
            m.w32(header + OBJECT_LIST_HEADER_FIRST_REFERENCE, NONE as u32);
        }
        Some(list)
    }

    /// `object_list_add`: the object first in the list (a full pool lists
    /// nothing, though the count goes up).
    fn object_list_add(&self, list: i32, object_index: i32) -> Option<()> {
        let m = self.m();
        let header = self.datum_get(self.context.object_list_header_data, list)?;
        let reference = self.datum_new(self.context.object_list_data);
        if reference != NONE {
            let datum = self.datum_get(self.context.object_list_data, reference)?;
            m.w32(datum + DATA_REFERENCE_DATUM, object_index as u32);
            m.w32(datum + DATA_REFERENCE_NEXT, m.r32(header + OBJECT_LIST_HEADER_FIRST_REFERENCE));
            m.w32(header + OBJECT_LIST_HEADER_FIRST_REFERENCE, reference as u32);
        }
        let count = m.r16(header + OBJECT_LIST_HEADER_COUNT);
        m.w16(header + OBJECT_LIST_HEADER_COUNT, count.wrapping_add(1));
        Some(())
    }

    /// The datums of a list's units (`unit_try_and_get`: the rest passed
    /// over); None for a list that isn't there.
    fn listed_units(&self, list: i32) -> Option<Vec<u32>> {
        Some(self.object_list(list)?.into_iter().filter_map(|o| self.object_typed(o, OBJECT_MASK_UNIT)).collect())
    }

    /// The object's datum, for `object_get`, `unit_get` and the like, which
    /// stop the game at an index of another type: Some(None) for NONE (they
    /// aren't asked), None to leave the body to the game's.
    fn typed_or_none(&self, object_index: i32, types: u32) -> Option<Option<u32>> {
        if object_index == NONE {
            return Some(None);
        }
        self.object_typed(object_index, types).map(Some)
    }

    /// A flag of 32 bits set or cleared.
    fn set_flag32(&self, address: u32, flag: u32, set: bool) {
        let m = self.m();
        let flags = m.r32(address);
        m.w32(address, if set { flags | flag } else { flags & !flag });
    }

    /// `unit_scripting_set_current_vitality_of`'s new body and shield
    /// vitalities for the object: Some(None) for a dead one, which keeps its
    /// own; None where the game's would deplete its shield or body (which
    /// makes effects), left to it.
    fn current_vitality(&self, object: u32, body_vitality: f32, shield_vitality: f32) -> Option<Option<(f32, f32)>> {
        use crate::layout::unit_datum as u;
        let m = self.m();
        if m.r16(object + u::OBJECT_DAMAGE_FLAGS as u32) & OBJECT_DEAD != 0 {
            return Some(None);
        }
        let f = |offset: usize| f32::from_bits(m.r32(object + offset as u32));
        let fraction = |value: f32, maximum: f32| {
            if maximum <= 0.0 {
                0.0
            } else if value >= maximum {
                1.0
            } else {
                value / maximum
            }
        };
        let shield = fraction(shield_vitality, f(u::OBJECT_MAXIMUM_SHIELD_VITALITY));
        let body = fraction(body_vitality, f(u::OBJECT_MAXIMUM_BODY_VITALITY));
        if (f(u::OBJECT_SHIELD_VITALITY) > 0.0 && shield <= 0.0) || (f(u::OBJECT_BODY_VITALITY) > 0.0 && body <= 0.0) {
            return None;
        }
        Some(Some((body, shield)))
    }

    /// A library function's body that runs here (the arguments evaluated
    /// at `arguments`), with its value returned; false if it has none here
    /// (or meets what the game's would stop at, which it then runs). One
    /// that has made an effect is done there, where the game's would have
    /// stopped.
    fn library_body(&self, definition: u32, thread_index: i32, arguments: u32) -> bool {
        self.effected.set(false);
        if self.library_body_run(definition, thread_index, arguments) {
            true
        } else if self.effected.get() {
            self.hs_return(thread_index, 0);
            true
        } else {
            false
        }
    }

    /// A body's effect on the rest of the game ([`call::EFFECT_OBEY_COMMAND_LIST`] on).
    fn effect(&self, kind: i32, a: i32, b: i32, c: i32) {
        self.effected.set(true);
        self.call(kind, a, b, c);
    }

    /// `ai_index_actor_iterator_new` and `_next`, each actor (its index and
    /// datum) handed on as it's reached, its next read first, as the game's
    /// effects can change the list; None where the game's would stop.
    fn for_each_ai_actor(&self, ai_reference: i32, mut each: impl FnMut(i32, u32) -> Option<()>) -> Option<()> {
        use crate::layout::actor_datum as a;
        let m = self.m();
        let Some((encounter_index, _)) = self.ai_reference_encounter(ai_reference) else { return Some(()) };
        let index = (ai_reference >> 16) & 0xff;
        let (platoon_index, squad_index) = match ai_reference as u32 >> 30 {
            0 => (NONE, NONE),
            1 => (index, NONE),
            2 => (NONE, index),
            _ => return Some(()),
        };
        let encounter = self.datum_get(self.context.encounter_data, encounter_index)?;
        let mut next_index = m.r32(encounter + crate::layout::encounter_datum::FIRST_ACTOR_INDEX as u32) as i32;
        let mut count = 0;
        while next_index != NONE {
            let actor_index = next_index;
            let actor = self.datum_get(self.context.actor_data, actor_index)?;
            next_index = m.r32(actor + a::META_NEXT_ACTOR_INDEX as u32) as i32;
            let field = |offset: usize| m.r16(actor + offset as u32) as i16 as i32;
            if (squad_index == NONE || squad_index == field(a::META_SQUAD_INDEX))
                && (platoon_index == NONE || platoon_index == field(a::META_PLATOON_INDEX))
            {
                each(actor_index, actor)?;
            }
            count += 1;
            if count > 0x10000 {
                return None;
            }
        }
        Some(())
    }

    /// `ai_scripting_magically_see_unit`: each actor's encounter forced
    /// active and its magic sight of the unit.
    fn magically_see_unit(&self, ai_reference: i32, unit_index: i32) -> Option<()> {
        if ai_reference == NONE || unit_index == NONE {
            return Some(());
        }
        self.for_each_ai_actor(ai_reference, |actor_index, actor| {
            let encounter_index = self.m().r32(actor + crate::layout::actor_datum::META_ENCOUNTER_INDEX as u32) as i32;
            if encounter_index != NONE {
                self.effect(call::EFFECT_ENCOUNTER_FORCE_ACTIVATE, encounter_index, 0, 0);
            }
            self.effect(call::EFFECT_MAGIC_SIGHT, actor_index, unit_index, 0);
            Some(())
        })
    }

    /// The unit's actor, else its swarm's (NONE for neither).
    fn unit_actor_or_swarm(&self, unit: u32) -> i32 {
        use crate::layout::unit_datum as u;
        let m = self.m();
        let actor_index = m.r32(unit + u::UNIT_ACTOR_INDEX as u32) as i32;
        if actor_index != NONE { actor_index } else { m.r32(unit + u::UNIT_SWARM_ACTOR_INDEX as u32) as i32 }
    }

    /// `vehicle_scripting_find_available_seats` (any seat type): the
    /// vehicle's empty seats whose lowered labels hold the name (all for
    /// none), up to 16; None for a definition not found.
    fn available_seats(&self, vehicle_index: i32, vehicle: u32, name: &[u8]) -> Option<Vec<i32>> {
        use crate::layout::unit_datum as u;
        let m = self.m();
        let labels = self.unit_seats(vehicle)?;
        let riders = self.objects_of_types(OBJECT_MASK_UNIT);
        let mut seats = Vec::new();
        for (seat_index, label) in labels.into_iter().enumerate() {
            if seats.len() >= 16 {
                break;
            }
            let lower = label.to_ascii_lowercase();
            if !name.is_empty() && !lower.windows(name.len()).any(|window| window == name) {
                continue;
            }
            let filled = riders.iter().any(|&(_, unit)| {
                m.r32(unit + u::OBJECT_PARENT_OBJECT_INDEX as u32) as i32 == vehicle_index
                    && m.r16(unit + u::UNIT_PARENT_SEAT_INDEX as u32) as i16 as i32 == seat_index as i32
            });
            if !filled {
                seats.push(seat_index as i32);
            }
        }
        Some(seats)
    }

    /// The loaded tag's data (`tag_get` of the group); None for one not
    /// loaded or of another group.
    fn tag_data(&self, group: &[u8; 4], tag_index: i32) -> Option<u32> {
        let tags = crate::ai_actor::AiStepContext {
            tag_instances: self.context.tag_instances,
            tag_count: self.context.tag_count,
            ..Default::default()
        };
        crate::sensing::tag_get(&AsAiMemory(self.m()), &tags, crate::fourcc(group), tag_index)
    }

    /// The actor variant and its actor definition (their data) of the actor
    /// palette's entry, either none for an entry not there or empty; None
    /// where the game's would stop.
    fn palette_definitions(&self, palette_index: i16) -> Option<(Option<u32>, Option<u32>)> {
        let m = self.m();
        if !(0..self.context.actor_palette_count).contains(&(palette_index as i32)) {
            return Some((None, None));
        }
        let entry = self.context.actor_palette + palette_index as u32 * TAG_REFERENCE_SIZE;
        let tag_index = m.r32(entry + TAG_REFERENCE_INDEX) as i32;
        if tag_index == NONE {
            return Some((None, None));
        }
        // (tag_get_group_tag: an entry of another group is passed over)
        let Some(variant) = self.tag_data(b"actv", tag_index) else { return Some((None, None)) };
        let actor_index = m.r32(variant + ACTOR_VARIANT_ACTOR_INDEX) as i32;
        let actor = if actor_index != NONE { Some(self.tag_data(b"actr", actor_index)?) } else { None };
        Some((Some(variant), actor))
    }

    /// `ai_scripting_migrate_find_target_squad`: the squad of the target an
    /// actor of the source squad goes to: its own (staying in its
    /// encounter), else the first of its variant, its actor definition, its
    /// type, the first of the target, or squad 0 (NONE for none); None where
    /// the game's would stop.
    fn migrate_target_squad(
        &self,
        source_squad_index: i32,
        source_actor: Option<u32>,
        source_variant: Option<u32>,
        same_encounter: bool,
        target_ai_reference: i32,
    ) -> Option<i32> {
        use crate::layout::{encounter_definition as ed, squad_definition as sd};
        let m = self.m();
        let target_encounter_index = target_ai_reference & 0xffff;
        if target_encounter_index >= self.context.encounter_count {
            return None;
        }
        let definition = self.context.encounters + target_encounter_index as u32 * ed::SIZEOF as u32;
        let (squad_count, squad_definitions) = self.tag_block(definition + ed::SQUADS as u32);
        let (mut first, mut same_type, mut same_actor, mut same_variant, mut same_squad) = (NONE, NONE, NONE, NONE, NONE);
        for (squad_index, _) in self.ai_squads(target_ai_reference)? {
            let squad_definition = squad_definitions + squad_index as u32 * sd::SIZEOF as u32;
            let palette_index = m.r16(squad_definition + SQUAD_DEFINITION_ACTOR_PALETTE_INDEX) as i16;
            let (variant, actor) = self.palette_definitions(palette_index)?;
            if same_squad == NONE && same_encounter && source_squad_index == squad_index {
                same_squad = squad_index;
            }
            if same_variant == NONE && source_variant.is_some() && variant.is_some() && source_variant == variant {
                same_variant = squad_index;
            }
            if same_actor == NONE && source_actor.is_some() && actor.is_some() && source_actor == actor {
                same_actor = squad_index;
            }
            if let (Some(source_actor), Some(actor)) = (source_actor, actor) {
                if same_type == NONE && m.r16(source_actor + ACTOR_DEFINITION_TYPE) == m.r16(actor + ACTOR_DEFINITION_TYPE) {
                    same_type = squad_index;
                }
            }
            if first == NONE {
                first = squad_index;
            }
        }
        let found = [same_squad, same_variant, same_actor, same_type, first]
            .into_iter()
            .find(|&index| index != NONE)
            .unwrap_or(if squad_count > 0 { 0 } else { NONE });
        if found != NONE && !(0..squad_count).contains(&found) {
            return None;
        }
        Some(found)
    }

    /// `ai_scripting_migrate_internal`: the source's squads matched with the
    /// target's, then its actors changed over (with their stimulus), the
    /// prevehicle and disconnected actors it had, the teams' status and the
    /// encounters'.
    fn migrate(&self, source: i32, target: i32, stimulus: Option<bool>) -> Option<()> {
        use crate::layout::{actor_datum as a, encounter_datum as e, encounter_definition as ed, squad_datum as sq};
        let m = self.m();
        if source == NONE || target == NONE {
            return Some(());
        }
        let (source_encounter_index, target_encounter_index) = (source & 0xffff, target & 0xffff);
        if source_encounter_index >= self.context.encounter_count || target_encounter_index >= self.context.encounter_count {
            return None;
        }
        let source_encounter = self.datum_get(self.context.encounter_data, source_encounter_index)?;
        let target_encounter = self.datum_get(self.context.encounter_data, target_encounter_index)?;
        let definition = |index: i32| self.context.encounters + index as u32 * ed::SIZEOF as u32;
        let (_, source_squads) = self.tag_block(definition(source_encounter_index) + ed::SQUADS as u32);
        let (target_squad_count, _) = self.tag_block(definition(target_encounter_index) + ed::SQUADS as u32);
        let same_encounter = source_encounter_index == target_encounter_index;
        let mut targets = [NONE; MAXIMUM_SQUADS_PER_ENCOUNTER as usize];
        for (squad_index, squad) in self.ai_squads(source)? {
            if !(0..MAXIMUM_SQUADS_PER_ENCOUNTER as i32).contains(&squad_index) {
                return None;
            }
            if m.r16(squad + sq::CURRENT_COUNT as u32) as i16 > 0 || m.r8(source_encounter + ENCOUNTER_IS_PREVEHICLE) != 0 {
                let squad_definition = source_squads + squad_index as u32 * crate::layout::squad_definition::SIZEOF as u32;
                let palette_index = m.r16(squad_definition + SQUAD_DEFINITION_ACTOR_PALETTE_INDEX) as i16;
                let (variant, actor) = self.palette_definitions(palette_index)?;
                targets[squad_index as usize] = self.migrate_target_squad(squad_index, actor, variant, same_encounter, target)?;
            }
        }
        // (the target squad of an actor in the source squad, if it moves)
        let moved = |squad_index: i16| -> Option<Option<i32>> {
            let target_squad = *targets.get(usize::try_from(squad_index).ok()?)?;
            if target_squad != NONE && (!same_encounter || target_squad != squad_index as i32) {
                (0..target_squad_count).contains(&target_squad).then_some(Some(target_squad))
            } else {
                Some(None)
            }
        };
        if self.ai_initialized() {
            let mut actor_index = m.r32(source_encounter + e::FIRST_ACTOR_INDEX as u32) as i32;
            while actor_index != NONE {
                let actor = self.datum_get(self.context.actor_data, actor_index)?;
                let next_index = m.r32(actor + a::META_NEXT_ACTOR_INDEX as u32) as i32;
                if let Some(target_squad) = moved(m.r16(actor + a::META_SQUAD_INDEX as u32) as i16)? {
                    self.effect(call::EFFECT_CHANGE_ENCOUNTER, actor_index, target_encounter_index, target_squad);
                    if let Some(advancing) = stimulus {
                        self.effect(call::EFFECT_STIMULUS_MANEUVERING, actor_index, advancing as i32, 0);
                    }
                }
                actor_index = next_index;
            }
        }
        if m.r8(source_encounter + ENCOUNTER_IS_PREVEHICLE) != 0 {
            for actor in self.all_actors(false)? {
                if m.r32(actor + ACTOR_PREVEHICLE_ENCOUNTER_INDEX) & 0xffff == source_encounter_index as u32 {
                    if let Some(target_squad) = moved(m.r16(actor + ACTOR_PREVEHICLE_SQUAD_INDEX) as i16)? {
                        m.w32(actor + ACTOR_PREVEHICLE_ENCOUNTER_INDEX, target_encounter_index as u32);
                        m.w16(actor + ACTOR_PREVEHICLE_SQUAD_INDEX, target_squad as u16);
                    }
                }
            }
            if !same_encounter {
                // (the January build tests the masked index's reference
                // type, so the source's flag clears for any reference)
                m.w8(source_encounter + ENCOUNTER_IS_PREVEHICLE, 0);
                m.w8(target_encounter + ENCOUNTER_IS_PREVEHICLE, 1);
            }
        }
        if self.ai_initialized() {
            let target_bsp = m.r16(definition(target_encounter_index) + ed::RUNTIME_STRUCTURE_BSP_REFERENCE_INDEX as u32) as i16;
            let mut actor_index =
                m.r32(self.context.ai_globals + crate::layout::ai_globals::FIRST_ENCOUNTERLESS_ACTOR_INDEX as u32) as i32;
            while actor_index != NONE {
                let actor = self.datum_get(self.context.actor_data, actor_index)?;
                let next_index = m.r32(actor + a::META_NEXT_ACTOR_INDEX as u32) as i32;
                if m.r32(actor + ACTOR_DISCONNECTED_ENCOUNTER_INDEX) & 0xffff == source_encounter_index as u32 {
                    if let Some(target_squad) = moved(m.r16(actor + ACTOR_DISCONNECTED_SQUAD_INDEX) as i16)? {
                        m.w32(actor + ACTOR_DISCONNECTED_ENCOUNTER_INDEX, target_encounter_index as u32);
                        m.w16(actor + ACTOR_DISCONNECTED_SQUAD_INDEX, target_squad as u16);
                        if !same_encounter && target_bsp == m.r16(self.context.structure_bsp_index) as i16 {
                            self.effect(call::EFFECT_ENCOUNTER_REATTACH, actor_index, target_encounter_index, target_squad);
                        }
                    }
                }
                actor_index = next_index;
            }
        }
        // (their teams compared as the effect is made: the actors' changes
        // can change them)
        self.effect(call::EFFECT_TEAM_STATUS_IF_TEAMS_DIFFER, source_encounter_index, target_encounter_index, 0);
        self.effect(call::EFFECT_ENCOUNTERS_UPDATE_DIRTY_STATUS, 0, 0, 0);
        Some(())
    }

    /// `ai_scripting_migrate_by_unit_internal`: the unit's actor (or its
    /// swarm's) to its squad of the target.
    fn migrate_unit(&self, unit: u32, target: i32) -> Option<()> {
        use crate::layout::actor_datum as a;
        let m = self.m();
        let actor_index = self.unit_actor_or_swarm(unit);
        if actor_index == NONE || target == NONE {
            return Some(());
        }
        let actor = self.datum_get(self.context.actor_data, actor_index)?;
        let actor_definition = self.tag_data(b"actr", m.r32(actor + a::META_DEFINITION_INDEX as u32) as i32)?;
        let variant = self.tag_data(b"actv", m.r32(actor + ACTOR_VARIANT_DEFINITION_INDEX) as i32)?;
        let target_encounter_index = target & 0xffff;
        let squad_index = m.r16(actor + a::META_SQUAD_INDEX as u32) as i16 as i32;
        let same_encounter = m.r32(actor + a::META_ENCOUNTER_INDEX as u32) as i32 & 0xffff == target_encounter_index;
        let target_squad = self.migrate_target_squad(squad_index, Some(actor_definition), Some(variant), same_encounter, target)?;
        if target_squad != NONE && (!same_encounter || target_squad != squad_index) {
            self.effect(call::EFFECT_CHANGE_ENCOUNTER, actor_index, target_encounter_index, target_squad);
        }
        Some(())
    }

    /// `actor_iterator_new` and `actor_iterator_next`, active only or not
    /// ([`Self::active_actors`]'s walk).
    fn all_actors(&self, active_only: bool) -> Option<Vec<u32>> {
        use crate::layout::{actor_datum as a, encounter_datum as e};
        if active_only {
            return self.active_actors();
        }
        let m = self.m();
        let mut actors = Vec::new();
        if !self.ai_initialized() {
            return Some(actors);
        }
        let mut walk = |first: i32| -> Option<()> {
            let mut actor_index = first;
            while actor_index != NONE {
                let actor = self.datum_get(self.context.actor_data, actor_index)?;
                actors.push(actor);
                actor_index = m.r32(actor + a::META_NEXT_ACTOR_INDEX as u32) as i32;
                if actors.len() > 0x10000 {
                    return None;
                }
            }
            Some(())
        };
        let mut encounter_index = self.data_next_index(self.context.encounter_data, NONE);
        while encounter_index != NONE {
            let encounter = self.datum_get(self.context.encounter_data, encounter_index)?;
            walk(m.r32(encounter + e::FIRST_ACTOR_INDEX as u32) as i32)?;
            encounter_index = self.data_next_index(self.context.encounter_data, encounter_index);
        }
        walk(m.r32(self.context.ai_globals + crate::layout::ai_globals::FIRST_ENCOUNTERLESS_ACTOR_INDEX as u32) as i32)?;
        Some(actors)
    }

    /// A body's value returned early (for a body's arm that returns from
    /// within its match).
    fn done_with(&self, thread_index: i32, value: i32) -> bool {
        self.hs_return(thread_index, value);
        true
    }

    /// The game field's address ([`game_field`]); None for one not there.
    fn game_field(&self, field: u32) -> Option<u32> {
        if self.context.game_fields == 0 {
            return None;
        }
        let address = self.m().r32(self.context.game_fields + 4 * field);
        (address != 0).then_some(address)
    }

    /// The body itself ([`Self::library_body`]).
    fn library_body_run(&self, definition: u32, thread_index: i32, arguments: u32) -> bool {
        use crate::layout::unit_datum as u;
        let m = self.m();
        let argument = |index: u32| m.r32(arguments + 4 * index) as i32;
        let short = |index: u32| m.r16(arguments + 4 * index) as i16;
        let boolean = |value: bool| value as i32;
        // (a short's value: its 16 bits, the rest clear)
        let short_value = |value: i32| value as u16 as i32;
        let value = match self.function_name(definition).as_str() {
            // global_structure_bsp_index_get, game_is_cooperative
            "structure_bsp_index" => short_value(m.r16(self.context.structure_bsp_index) as i16 as i32),
            "game_is_cooperative" => boolean(self.context.cooperative),
            // ai_scripting_living_count and the rest of
            // ai_scripting_count_internal's
            name @ ("ai_living_count" | "ai_swarm_count" | "ai_nonswarm_count") => {
                let count_type = match name {
                    "ai_living_count" => AiCount::Living,
                    "ai_swarm_count" => AiCount::Swarm,
                    _ => AiCount::Nonswarm,
                };
                let Some((count, _, _)) = self.ai_count(argument(0), count_type) else { return false };
                short_value(count)
            }
            "ai_living_fraction" => {
                let Some((count, original, _)) = self.ai_count(argument(0), AiCount::Living) else { return false };
                let fraction = if original > 0 { count as f32 / original as f32 } else { 0.0 };
                fraction.to_bits() as i32
            }
            "ai_strength" => {
                let Some((_, _, strength)) = self.ai_count(argument(0), AiCount::Living) else { return false };
                strength.to_bits() as i32
            }
            // hs_not
            "not" => boolean(m.r8(arguments) == 0),
            // hs_object_list_get_element
            "list_get" => {
                let Some(objects) = self.object_list(argument(0)) else { return false };
                objects.get(short(1).max(0) as usize).copied().unwrap_or(NONE)
            }
            // hs_trigger_volume_test_objects (any or all), which leaves its
            // value in hs_debug_data's bit for the volume (all of them not
            // as network co-op's host, where it waits for any player and
            // brings the rest)
            name @ ("volume_test_objects" | "volume_test_objects_all") => {
                let all = name == "volume_test_objects_all";
                if all && self.context.coop_host {
                    return false;
                }
                let volume_index = short(0);
                let Some(objects) = self.object_list(argument(1)) else { return false };
                let mut inside = all;
                for object in objects {
                    let Some(test) = self.trigger_volume_test_object(volume_index, object) else { return false };
                    if test != inside {
                        inside = test;
                        break;
                    }
                }
                if (0..MAXIMUM_TRIGGER_VOLUMES as i16).contains(&volume_index) {
                    let word = self.context.debug_data + 4 * (volume_index as u32 >> 5);
                    let bit = 1u32 << (volume_index & 31);
                    let bits = m.r32(word);
                    m.w32(word, if inside { bits | bit } else { bits & !bit });
                }
                boolean(inside)
            }
            // scenario_trigger_volume_test_object
            "volume_test_object" => match self.trigger_volume_test_object(short(0), argument(1)) {
                Some(inside) => boolean(inside),
                None => return false,
            },
            // unit_scripting_has_weapon_readied
            "unit_has_weapon_readied" => {
                let (unit_index, weapon_definition) = (argument(0), argument(1));
                if unit_index == NONE || weapon_definition == NONE {
                    0
                } else {
                    let Some(unit) = self.object_typed(unit_index, OBJECT_MASK_UNIT) else { return false };
                    let current = m.r16(unit + u::UNIT_CURRENT_WEAPON_INDEX as u32) as i16;
                    if current == NONE as i16 {
                        0
                    } else if !(0..UNIT_WEAPONS).contains(&current) {
                        return false;
                    } else {
                        let weapon_index = m.r32(unit + u::UNIT_WEAPON_OBJECT_INDICES as u32 + 4 * current as u32) as i32;
                        if weapon_index == NONE {
                            0
                        } else {
                            let Some(weapon) = self.object_typed(weapon_index, OBJECT_MASK_WEAPON) else { return false };
                            boolean(m.r32(weapon + u::DEFINITION_INDEX as u32) as i32 == weapon_definition)
                        }
                    }
                }
            }
            // object_cannot_take_damage and object_can_take_damage: each
            // listed object's flag
            name @ ("object_cannot_take_damage" | "object_can_take_damage") => {
                let Some(objects) = self.object_list(argument(0)) else { return false };
                let Some(data) = objects.iter().map(|&o| self.object_typed(o, OBJECT_MASK_ALL)).collect::<Option<Vec<_>>>()
                else {
                    return false;
                };
                for object in data {
                    let address = object + u::OBJECT_DAMAGE_FLAGS as u32;
                    let flags = m.r16(address);
                    let flags = if name == "object_cannot_take_damage" {
                        flags | OBJECT_CANNOT_TAKE_DAMAGE
                    } else {
                        flags & !OBJECT_CANNOT_TAKE_DAMAGE
                    };
                    m.w16(address, flags);
                }
                0
            }
            // ai_scripting_ignore: each listed unit ignored by actors, or not
            "ai_disregard" => {
                let Some(objects) = self.object_list(argument(0)) else { return false };
                let ignore = m.r8(arguments + 4) != 0;
                for object in objects {
                    if let Some(unit) = self.object_typed(object, OBJECT_MASK_UNIT) {
                        let address = unit + u::UNIT_FLAGS as u32;
                        let flags = m.r32(address);
                        m.w32(address, if ignore { flags | UNIT_IGNORED_BY_ACTORS } else { flags & !UNIT_IGNORED_BY_ACTORS });
                    }
                }
                0
            }
            // device_set_desired_position: its position group's desired value
            "device_set_position" => {
                let device_index = argument(0);
                let position = f32::from_bits(argument(1) as u32);
                if device_index == NONE {
                    0
                } else {
                    let Some(device) = self.object_typed(device_index, OBJECT_MASK_DEVICE) else { return false };
                    let group_index = m.r16(device + crate::layout::device_datum::DEVICE_POSITION_GROUP_INDEX as u32) as i16;
                    if group_index == NONE as i16 {
                        0
                    } else {
                        match self.device_group_set_desired_value(group_index, position) {
                            Some(changed) => boolean(changed),
                            None => return false,
                        }
                    }
                }
            }
            // hs_players: a new list of each player's unit (object_list_new,
            // object_list_add)
            "players" => {
                let Some(list) = self.object_list_new() else { return false };
                if list != NONE {
                    let mut player_index = self.data_next_index(self.context.player_data, NONE);
                    while player_index != NONE {
                        let Some(player) = self.datum_get(self.context.player_data, player_index) else { return false };
                        let unit_index = m.r32(player + crate::layout::player_datum::UNIT_INDEX as u32) as i32;
                        if unit_index != NONE && self.object_list_add(list, unit_index).is_none() {
                            return false;
                        }
                        player_index = self.data_next_index(self.context.player_data, player_index);
                    }
                }
                list
            }
            // unit_scripting_suspended (not as network co-op's host, where
            // the players the scripts can't name go with player0)
            "unit_suspended" => {
                if self.context.coop_host {
                    return false;
                }
                let unit_index = argument(0);
                if unit_index != NONE {
                    let Some(unit) = self.object_typed(unit_index, OBJECT_MASK_UNIT) else { return false };
                    let suspended = m.r8(arguments + 4) != 0;
                    let flags = m.r32(unit + u::UNIT_FLAGS as u32);
                    m.w32(unit + u::UNIT_FLAGS as u32, if suspended { flags | UNIT_SUSPENDED } else { flags & !UNIT_SUSPENDED });
                    for i in 0..3 {
                        m.w32(unit + u::OBJECT_TRANSLATIONAL_VELOCITY as u32 + 4 * i, 0);
                    }
                    if m.r16(unit + u::OBJECT_TYPE as u32) == 0 {
                        let address = unit + crate::layout::biped_datum::BIPED_FLAGS as u32;
                        m.w32(address, m.r32(address) & !BIPED_AIRBORNE);
                    }
                }
                0
            }
            // player_effect_screen_fade_in and _out
            name @ ("fade_in" | "fade_out") => {
                let fade = self.context.screen_fade;
                for i in 0..3 {
                    m.w32(fade + 4 * i, m.r32(arguments + 4 * i));
                }
                m.w16(fade + SCREEN_FADE_TICKS, m.r16(arguments + 12));
                m.w8(fade + SCREEN_FADE_FADING_OUT, (name == "fade_out") as u8);
                m.w32(fade + SCREEN_FADE_START_TIME, self.context.game_time as u32);
                0
            }
            // ai_conversation_status: the most advanced of the
            // conversation's running copies, else how its latest ended
            "ai_conversation_status" => {
                use crate::layout::conversation_datum as c;
                let conversation_index = short(0);
                let mut status = 0;
                let mut index = self.data_next_index(self.context.conversation_data, NONE);
                while index != NONE {
                    let Some(conversation) = self.datum_get(self.context.conversation_data, index) else { return false };
                    if m.r16(conversation + c::SCENARIO_CONVERSATION_INDEX as u32) as i16 == conversation_index {
                        let this = if m.r8(conversation + c::BEGUN as u32) == 0 {
                            1
                        } else if m.r8(conversation + c::ANY_LINE_SPOKEN as u32) == 0 {
                            2
                        } else if m.r8(conversation + c::WAITING_TO_ADVANCE as u32) != 0 {
                            4
                        } else {
                            3
                        };
                        status = status.max(this);
                    }
                    index = self.data_next_index(self.context.conversation_data, index);
                }
                if status == 0 && self.context.recent_conversations != 0 {
                    let recent_count = m.r16(self.context.recent_conversation_count) as i16;
                    let mut latest: Option<u32> = None;
                    let mut latest_finish_time = NONE;
                    for i in 0..recent_count.max(0) as u32 {
                        let recent = self.context.recent_conversations + i * RECENT_CONVERSATION_SIZE;
                        let finish_time = m.r32(recent + RECENT_CONVERSATION_FINISH_TIME) as i32;
                        if m.r16(recent) as i16 == conversation_index && finish_time > latest_finish_time {
                            latest = Some(recent);
                            latest_finish_time = finish_time;
                        }
                    }
                    if let Some(recent) = latest {
                        status = if m.r8(recent + RECENT_CONVERSATION_UNABLE_TO_BEGIN) != 0 {
                            5
                        } else if m.r8(recent + RECENT_CONVERSATION_FINISHED_SUCCESSFULLY) != 0 {
                            6
                        } else {
                            7
                        };
                    }
                }
                short_value(status)
            }
            // ai_scripting_command_list_status: the furthest along of the
            // listed units' command lists (a swarm's left to the game's)
            "ai_command_list_status" => {
                use crate::layout::{actor_datum as a, obey_individual_simple_control as sc};
                let Some(objects) = self.object_list(argument(0)) else { return false };
                let mut result = 0;
                for unit_index in objects {
                    let Some(unit) = self.object_typed(unit_index, OBJECT_MASK_UNIT) else { continue };
                    let actor_index = m.r32(unit + u::UNIT_ACTOR_INDEX as u32) as i32;
                    let swarm_actor_index = m.r32(unit + u::UNIT_SWARM_ACTOR_INDEX as u32) as i32;
                    let mut status = 0;
                    let actor = if actor_index != NONE {
                        let Some(actor) = self.datum_get(self.context.actor_data, actor_index) else { return false };
                        if m.r16(actor + a::STATE_ACTION as u32) as usize == ACTOR_ACTION_OBEY {
                            // ai_scripting_command_list_status_internal
                            let list_index = m.r16(actor + a::STATE_OBEY_COMMAND_LIST_INDEX as u32) as i16;
                            if list_index < 0 || list_index as i32 >= self.context.command_list_count {
                                return false;
                            }
                            let list = self.context.command_lists
                                + list_index as u32 * crate::layout::ai_command_list_definition::SIZEOF as u32;
                            let command_count = m.r32(list + crate::layout::ai_command_list_definition::COMMANDS as u32) as i32;
                            let control = actor + a::STATE_OBEY_SIMPLE_CONTROL as u32;
                            status = if (m.r8(control + sc::CURRENT_COMMAND_INDEX as u32) as i32) >= command_count {
                                1
                            } else if m.r8(control + sc::METADATA_FLAGS as u32) & OBEY_WAITING_FOR_ADVANCE != 0 {
                                2
                            } else {
                                3
                            };
                        }
                        Some(actor)
                    } else if swarm_actor_index != NONE {
                        let Some(actor) = self.datum_get(self.context.actor_data, swarm_actor_index) else { return false };
                        if m.r16(actor + a::STATE_ACTION as u32) as usize == ACTOR_ACTION_OBEY
                            && m.r16(actor + a::META_SWARM_CACHE_INDEX as u32) as i16 != NONE as i16
                        {
                            return false;
                        }
                        Some(actor)
                    } else {
                        None
                    };
                    if status == 0 {
                        if let Some(actor) = actor {
                            let last = m.r32(actor + a::STATE_LAST_COMMAND_LIST_TIME as u32) as i32;
                            if last != NONE && last.wrapping_add(150) >= self.context.game_time {
                                status = 1;
                            }
                        }
                    }
                    result = result.max(status);
                }
                short_value(result)
            }
            // unit_scripting_vehicle_test_seat_list: whether the unit in a
            // seat of that label (the first found by the object iterator)
            // is listed
            "vehicle_test_seat_list" => {
                let (vehicle_index, seat_name, list) = (argument(0), argument(1) as u32, argument(2));
                if vehicle_index == NONE {
                    0
                } else {
                    let Some(vehicle) = self.object_typed(vehicle_index, OBJECT_MASK_UNIT) else { return false };
                    let Some(seats) = self.unit_seats(vehicle) else { return false };
                    let name = self.c_string(seat_name);
                    let listed = match self.object_list(list) {
                        Some(objects) => objects,
                        None => return false,
                    };
                    let mut result = false;
                    for (seat_index, label) in seats.into_iter().enumerate() {
                        let seat_index = seat_index as i32;
                        if !name.eq_ignore_ascii_case(&label) {
                            continue;
                        }
                        let rider = self.objects_of_types(OBJECT_MASK_UNIT).into_iter().find(|&(_, unit)| {
                            m.r32(unit + u::OBJECT_PARENT_OBJECT_INDEX as u32) as i32 == vehicle_index
                                && m.r16(unit + u::UNIT_PARENT_SEAT_INDEX as u32) as i16 as i32 == seat_index
                        });
                        if let Some((rider_index, _)) = rider {
                            if listed.contains(&rider_index) {
                                result = true;
                            }
                        }
                    }
                    boolean(result)
                }
            }
            // device_get_position
            "device_get_position" => {
                let device_index = argument(0);
                let position = if device_index == NONE {
                    0.0f32
                } else {
                    let Some(device) = self.object_typed(device_index, OBJECT_MASK_DEVICE) else { return false };
                    f32::from_bits(m.r32(device + crate::layout::device_datum::DEVICE_POSITION as u32))
                };
                position.to_bits() as i32
            }
            // unit_scripting_get_health and _shield: -1 for no object, 0
            // for a dead one
            name @ ("unit_get_health" | "unit_get_shield") => {
                let vitality = match self.object_typed(argument(0), OBJECT_MASK_ALL) {
                    None => -1.0f32,
                    Some(object) if m.r16(object + u::OBJECT_DAMAGE_FLAGS as u32) & OBJECT_DEAD != 0 => 0.0,
                    Some(object) => f32::from_bits(m.r32(
                        object + if name == "unit_get_health" { u::OBJECT_BODY_VITALITY } else { u::OBJECT_SHIELD_VITALITY } as u32,
                    )),
                };
                vitality.to_bits() as i32
            }
            // unit_scripting_get_grenade_count: a unit's grenades of each type
            "unit_get_total_grenade_count" => match self.object_typed(argument(0), OBJECT_MASK_UNIT) {
                Some(unit) => (0..UNIT_GRENADE_TYPES).map(|i| m.r8(unit + u::UNIT_GRENADE_COUNTS as u32 + i) as i8 as i16).sum::<i16>() as i32,
                None => 0,
            },
            // object_list_count
            "list_count" => {
                let list = argument(0);
                if list == -1 {
                    0
                } else {
                    let header = crate::ai_actor::datum_get(&AsAiMemory(m), self.context.object_list_header_data, list);
                    header.map_or(0, |header| short_value(m.r16(header + OBJECT_LIST_HEADER_COUNT) as i32))
                }
            }
            // object_set_ranged_attack_inhibited and _melee_: the object's
            // damage flag (object_get)
            name @ ("object_set_ranged_attack_inhibited" | "object_set_melee_attack_inhibited") => {
                let Some(object) = self.typed_or_none(argument(0), OBJECT_MASK_ALL) else { return false };
                if let Some(object) = object {
                    let flag = if name == "object_set_ranged_attack_inhibited" {
                        OBJECT_RANGED_ATTACK_INHIBITED
                    } else {
                        OBJECT_MELEE_ATTACK_INHIBITED
                    };
                    let address = object + u::OBJECT_DAMAGE_FLAGS as u32;
                    let flags = m.r16(address);
                    m.w16(address, if m.r8(arguments + 4) != 0 { flags | flag } else { flags & !flag });
                }
                0
            }
            // object_beautify: the object a movie star (object_get)
            "object_beautify" => {
                let Some(object) = self.typed_or_none(argument(0), OBJECT_MASK_ALL) else { return false };
                if let Some(object) = object {
                    self.set_flag32(object + u::OBJECT_FLAGS as u32, OBJECT_MOVIE_STAR, m.r8(arguments + 4) != 0);
                }
                0
            }
            // hs_object_set_shield: the maximum times the fraction (clamped)
            "object_set_shield" => {
                let Some(object) = self.typed_or_none(argument(0), OBJECT_MASK_ALL) else { return false };
                if let Some(object) = object {
                    let mut fraction = f32::from_bits(argument(1) as u32);
                    if fraction < 0.0 {
                        fraction = 0.0;
                    } else if fraction > 1.0 {
                        fraction = 1.0;
                    }
                    let maximum = f32::from_bits(m.r32(object + u::OBJECT_MAXIMUM_SHIELD_VITALITY as u32));
                    m.w32(object + u::OBJECT_SHIELD_VITALITY as u32, (maximum * fraction).to_bits());
                }
                0
            }
            // a unit's flag (unit_get): unit_scripting_can_blink (cannot,
            // inverted), unit_aim_without_turning, unit_set_enterable_by_player
            // (not enterable, inverted)
            name @ ("unit_can_blink" | "unit_aim_without_turning" | "unit_set_enterable_by_player") => {
                let Some(unit) = self.typed_or_none(argument(0), OBJECT_MASK_UNIT) else { return false };
                if let Some(unit) = unit {
                    let value = m.r8(arguments + 4) != 0;
                    let (flag, set) = match name {
                        "unit_can_blink" => (UNIT_CANNOT_BLINK, !value),
                        "unit_aim_without_turning" => (UNIT_AIM_WITHOUT_TURNING, value),
                        _ => (UNIT_NOT_ENTERABLE_BY_PLAYER, !value),
                    };
                    self.set_flag32(unit + u::UNIT_FLAGS as u32, flag, set);
                }
                0
            }
            // unit_scripting_doesnt_drop_items and _impervious: each listed
            // unit's flag (unit_try_and_get)
            name @ ("unit_doesnt_drop_items" | "unit_impervious") => {
                let Some(units) = self.listed_units(argument(0)) else { return false };
                let (flag, set) = if name == "unit_impervious" {
                    (UNIT_IMPERVIOUS, m.r8(arguments + 4) != 0)
                } else {
                    (UNIT_DOESNT_DROP_ITEMS, true)
                };
                for unit in units {
                    self.set_flag32(unit + u::UNIT_FLAGS as u32, flag, set);
                }
                0
            }
            // unit_set_desired_flashlight_state (unit_get) and units_ (each
            // listed unit): the desired on or off flag set
            name @ ("unit_set_desired_flashlight_state" | "units_set_desired_flashlight_state") => {
                let units: Vec<u32> = if name == "unit_set_desired_flashlight_state" {
                    let Some(unit) = self.typed_or_none(argument(0), OBJECT_MASK_UNIT) else { return false };
                    unit.into_iter().collect()
                } else {
                    let Some(units) = self.listed_units(argument(0)) else { return false };
                    units
                };
                let flag = if m.r8(arguments + 4) != 0 { UNIT_DESIRED_INTEGRATED_LIGHT_ON } else { UNIT_DESIRED_INTEGRATED_LIGHT_OFF };
                for unit in units {
                    self.set_flag32(unit + u::UNIT_FLAGS as u32, flag, true);
                }
                0
            }
            // unit_get_current_flashlight_state (unit_get)
            "unit_get_current_flashlight_state" => {
                let Some(unit) = self.typed_or_none(argument(0), OBJECT_MASK_UNIT) else { return false };
                boolean(unit.is_some_and(|unit| m.r32(unit + u::UNIT_FLAGS as u32) & UNIT_INTEGRATED_LIGHT_ON != 0))
            }
            // unit_scripting_set_maximum_vitality (the object) and units_
            // (each listed): a living one's maximums, at full (not as network
            // co-op's host, where a player's is every player's)
            name @ ("unit_set_maximum_vitality" | "units_set_maximum_vitality") => {
                if self.context.coop_host {
                    return false;
                }
                let objects = if name == "unit_set_maximum_vitality" {
                    vec![argument(0)]
                } else {
                    let Some(objects) = self.object_list(argument(0)) else { return false };
                    objects
                };
                let Some(data) =
                    objects.iter().filter(|&&o| o != NONE).map(|&o| self.object_typed(o, OBJECT_MASK_ALL)).collect::<Option<Vec<_>>>()
                else {
                    return false;
                };
                let (body, shield) = (f32::from_bits(argument(1) as u32), f32::from_bits(argument(2) as u32));
                for object in data {
                    if m.r16(object + u::OBJECT_DAMAGE_FLAGS as u32) & OBJECT_DEAD != 0 {
                        continue;
                    }
                    // (object_initialize_vitality, both given)
                    m.w32(object + u::OBJECT_MAXIMUM_BODY_VITALITY as u32, body.to_bits());
                    m.w32(object + u::OBJECT_MAXIMUM_SHIELD_VITALITY as u32, shield.to_bits());
                    m.w32(object + u::OBJECT_BODY_VITALITY as u32, if body > 0.0 { 1.0f32 } else { 0.0 }.to_bits());
                    m.w32(object + u::OBJECT_SHIELD_VITALITY as u32, if shield > 0.0 { 1.0f32 } else { 0.0 }.to_bits());
                }
                0
            }
            // unit_scripting_set_current_vitality (the object) and units_
            // (each listed): a living one's vitalities as fractions of its
            // maximums (not as network co-op's host; nor where one would be
            // depleted, which makes effects: the game's then)
            name @ ("unit_set_current_vitality" | "units_set_current_vitality") => {
                if self.context.coop_host {
                    return false;
                }
                let objects = if name == "unit_set_current_vitality" {
                    vec![argument(0)]
                } else {
                    let Some(objects) = self.object_list(argument(0)) else { return false };
                    objects
                };
                let (body, shield) = (f32::from_bits(argument(1) as u32), f32::from_bits(argument(2) as u32));
                let mut changes = Vec::new();
                for object_index in objects.into_iter().filter(|&o| o != NONE) {
                    let Some(object) = self.object_typed(object_index, OBJECT_MASK_ALL) else { return false };
                    match self.current_vitality(object, body, shield) {
                        Some(Some(change)) => changes.push((object, change)),
                        Some(None) => {}
                        None => return false,
                    }
                }
                for (object, (body, shield)) in changes {
                    m.w32(object + u::OBJECT_SHIELD_VITALITY as u32, shield.to_bits());
                    m.w32(object + u::OBJECT_BODY_VITALITY as u32, body.to_bits());
                }
                0
            }
            // unit_scripting_has_weapon (unit_has_weapon_definition_index):
            // any of its weapons of the definition
            "unit_has_weapon" => {
                let (unit_index, weapon_definition) = (argument(0), argument(1));
                if unit_index == NONE || weapon_definition == NONE {
                    0
                } else {
                    let Some(unit) = self.object_typed(unit_index, OBJECT_MASK_UNIT) else { return false };
                    let mut has = false;
                    for i in 0..UNIT_WEAPONS as u32 {
                        let weapon_index = m.r32(unit + u::UNIT_WEAPON_OBJECT_INDICES as u32 + 4 * i) as i32;
                        if weapon_index != NONE {
                            let Some(weapon) = self.object_typed(weapon_index, OBJECT_MASK_WEAPON) else { return false };
                            if m.r32(weapon + u::DEFINITION_INDEX as u32) as i32 == weapon_definition {
                                has = true;
                                break;
                            }
                        }
                    }
                    boolean(has)
                }
            }
            // unit_scripting_vehicle_test_seat: the unit in a seat of the
            // label
            "vehicle_test_seat" => {
                let (vehicle_index, seat_name, unit_index) = (argument(0), argument(1) as u32, argument(2));
                if vehicle_index == NONE || unit_index == NONE {
                    0
                } else {
                    let Some(vehicle) = self.object_typed(vehicle_index, OBJECT_MASK_UNIT) else { return false };
                    let Some(seats) = self.unit_seats(vehicle) else { return false };
                    let name = self.c_string(seat_name);
                    let mut result = false;
                    for (seat_index, label) in seats.into_iter().enumerate() {
                        if !name.eq_ignore_ascii_case(&label) {
                            continue;
                        }
                        let Some(unit) = self.object_typed(unit_index, OBJECT_MASK_UNIT) else { return false };
                        if m.r32(unit + u::OBJECT_PARENT_OBJECT_INDEX as u32) as i32 == vehicle_index
                            && m.r16(unit + u::UNIT_PARENT_SEAT_INDEX as u32) as i16 as i32 == seat_index as i32
                        {
                            result = true;
                            break;
                        }
                    }
                    boolean(result)
                }
            }
            // unit_scripting_unit_riders: a new list of its children that
            // are units in a seat
            "vehicle_riders" => {
                let unit_index = argument(0);
                if unit_index == NONE {
                    NONE
                } else {
                    let Some(unit) = self.object_typed(unit_index, OBJECT_MASK_UNIT) else { return false };
                    let Some(list) = self.object_list_new() else { return false };
                    if list != NONE {
                        let mut child_index = m.r32(unit + u::OBJECT_FIRST_CHILD_OBJECT_INDEX as u32) as i32;
                        while child_index != NONE {
                            let Some(child) = self.object_typed(child_index, OBJECT_MASK_ALL) else { return false };
                            if 1u32.checked_shl(m.r16(child + u::OBJECT_TYPE as u32) as u32).is_some_and(|bit| OBJECT_MASK_UNIT & bit != 0)
                                && m.r16(child + u::UNIT_PARENT_SEAT_INDEX as u32) as i16 != NONE as i16
                                && self.object_list_add(list, child_index).is_none()
                            {
                                return false;
                            }
                            child_index = m.r32(child + u::OBJECT_NEXT_OBJECT_INDEX as u32) as i32;
                        }
                    }
                    list
                }
            }
            // unit_scripting_unit_driver and _gunner (unit_try_and_get)
            name @ ("vehicle_driver" | "vehicle_gunner") => match self.object_typed(argument(0), OBJECT_MASK_UNIT) {
                Some(unit) => {
                    let field = if name == "vehicle_driver" { u::UNIT_DRIVER_OBJECT_INDEX } else { u::UNIT_GUNNER_OBJECT_INDEX };
                    m.r32(unit + field as u32) as i32
                }
                None => NONE,
            },
            // a machine's flag (machine_try_and_get): device_set_never_appears_locked,
            // device_one_sided_set, device_operates_automatically_set (does
            // not, inverted)
            name @ ("device_set_never_appears_locked" | "device_one_sided_set" | "device_operates_automatically_set") => {
                if let Some(machine) = self.object_typed(argument(0), OBJECT_MASK_MACHINE) {
                    let value = m.r8(arguments + 4) != 0;
                    let (flag, set) = match name {
                        "device_set_never_appears_locked" => (MACHINE_NEVER_APPEARS_LOCKED, value),
                        "device_one_sided_set" => (MACHINE_ONE_SIDED, value),
                        _ => (MACHINE_DOES_NOT_OPERATE_AUTOMATICALLY, !value),
                    };
                    self.set_flag32(machine + MACHINE_FLAGS, flag, set);
                }
                0
            }
            // device_get_power (device_get)
            "device_get_power" => {
                let Some(device) = self.typed_or_none(argument(0), OBJECT_MASK_DEVICE) else { return false };
                device.map_or(0.0f32, |device| f32::from_bits(m.r32(device + crate::layout::device_datum::DEVICE_POWER as u32))).to_bits()
                    as i32
            }
            // device_set_power: its power, and its power group's desired
            // value (a group that powers a device, as its own does, the
            // game's, whose change makes effects)
            "device_set_power" => {
                use crate::layout::device_datum as d;
                let Some(device) = self.typed_or_none(argument(0), OBJECT_MASK_DEVICE) else { return false };
                if let Some(device) = device {
                    let power = f32::from_bits(argument(1) as u32);
                    let group_index = m.r16(device + d::DEVICE_POWER_GROUP_INDEX as u32) as i16;
                    if self.device_group_set_desired_value(group_index, power).is_none() {
                        return false;
                    }
                    self.set_flag32(device + DEVICE_FLAGS, DEVICE_POSITION_CHANGED, true);
                    m.w32(device + d::DEVICE_POWER as u32, power.to_bits());
                }
                0
            }
            // object_scripting_set_collideable (object_get)
            "object_set_collideable" => {
                let Some(object) = self.typed_or_none(argument(0), OBJECT_MASK_ALL) else { return false };
                if let Some(object) = object {
                    self.set_flag32(object + u::OBJECT_FLAGS as u32, OBJECT_NO_COLLISIONS, m.r8(arguments + 4) == 0);
                }
                0
            }
            // unit_is_playing_custom_animation (unit_get)
            "unit_is_playing_custom_animation" => {
                let Some(unit) = self.typed_or_none(argument(0), OBJECT_MASK_UNIT) else { return false };
                boolean(unit.is_some_and(|unit| m.r8(unit + u::UNIT_ANIMATION_STATE as u32) == UNIT_STATE_USER_ANIMATION))
            }
            // unit_get_custom_animation_time (unit_get): the frames left of
            // its custom animation, if it plays one
            "unit_get_custom_animation_time" => {
                let Some(unit) = self.typed_or_none(argument(0), OBJECT_MASK_UNIT) else { return false };
                match unit {
                    Some(unit) if m.r8(unit + u::UNIT_ANIMATION_STATE as u32) == UNIT_STATE_USER_ANIMATION => {
                        let Some(frames) = self.animation_frames_left(unit) else { return false };
                        short_value(frames)
                    }
                    _ => 0,
                }
            }
            // scenery_get_animation_time (scenery_get): the frames left of
            // its animation, if it plays one
            "scenery_get_animation_time" => {
                let Some(scenery) = self.object_typed(argument(0), OBJECT_MASK_SCENERY) else { return false };
                if m.r32(scenery + SCENERY_FLAGS) & 1 != 0 {
                    let Some(frames) = self.animation_frames_left(scenery) else { return false };
                    short_value(frames)
                } else {
                    0
                }
            }
            // device_set_actual_position: its position group's value at once
            "device_set_position_immediate" => {
                let Some(device) = self.typed_or_none(argument(0), OBJECT_MASK_DEVICE) else { return false };
                if let Some(device) = device {
                    let group_index = m.r16(device + crate::layout::device_datum::DEVICE_POSITION_GROUP_INDEX as u32) as i16;
                    if group_index != NONE as i16
                        && self.device_group_set_actual_value(group_index, f32::from_bits(argument(1) as u32)).is_none()
                    {
                        return false;
                    }
                }
                0
            }
            // device_group_set_actual_value
            "device_group_set_immediate" => {
                if self.device_group_set_actual_value(short(0), f32::from_bits(argument(1) as u32)).is_none() {
                    return false;
                }
                0
            }
            // device_group_get_value
            "device_group_get" => {
                let Some(group) = self.datum_get(self.context.device_groups_data, short(0) as i32) else { return false };
                m.r32(group + DEVICE_GROUP_ACTUAL_VALUE) as i32
            }
            // device_group_set_desired_value
            "device_group_set" => match self.device_group_set_desired_value(short(0), f32::from_bits(argument(1) as u32)) {
                Some(changed) => boolean(changed),
                None => return false,
            },
            // device_group_change_only_once_more_set
            "device_group_change_only_once_more_set" => {
                let group_index = short(0);
                if group_index != NONE as i16 {
                    let Some(group) = self.datum_get(self.context.device_groups_data, group_index as i32) else { return false };
                    let flags = m.r16(group + DEVICE_GROUP_FLAGS);
                    let flags = if m.r8(arguments + 4) != 0 {
                        flags | DEVICE_GROUP_CAN_CHANGE_ONLY_ONCE
                    } else {
                        flags & !DEVICE_GROUP_CAN_CHANGE_ONLY_ONCE
                    };
                    m.w16(group + DEVICE_GROUP_FLAGS, flags & !DEVICE_GROUP_CHANGED_ONCE);
                }
                0
            }
            // ai_scripting_attack, _defend, _retreat (ai_scripting_maneuver),
            // _maneuver and _maneuver_enable: each platoon's flag
            name @ ("ai_attack" | "ai_defend" | "ai_retreat" | "ai_maneuver" | "ai_maneuver_enable") => {
                use crate::layout::platoon_datum as pl;
                if argument(0) != NONE {
                    let Some(platoons) = self.ai_platoons(argument(0)) else { return false };
                    let (offset, value) = match name {
                        "ai_attack" => (pl::DEFENDING, 0),
                        "ai_defend" => (pl::DEFENDING, 1),
                        "ai_maneuver_enable" => (pl::MANEUVER_DISABLE, (m.r8(arguments + 4) == 0) as u8),
                        _ => (pl::MANEUVERING, 1),
                    };
                    for platoon in platoons {
                        m.w8(platoon + offset as u32, value);
                    }
                }
                0
            }
            // ai_scripting_is_attacking: any platoon not defending
            "ai_is_attacking" => {
                let mut attacking = false;
                if argument(0) != NONE {
                    let Some(platoons) = self.ai_platoons(argument(0)) else { return false };
                    attacking = platoons
                        .into_iter()
                        .any(|platoon| m.r8(platoon + crate::layout::platoon_datum::DEFENDING as u32) == 0);
                }
                boolean(attacking)
            }
            // ai_scripting_allow_dormant, _timer_start and
            // _automatic_migration_target: each squad's flag
            name @ ("ai_allow_dormant" | "ai_timer_start" | "ai_automatic_migration_target") => {
                use crate::layout::squad_datum as sq;
                if name == "ai_allow_dormant" || argument(0) != NONE {
                    let Some(squads) = self.ai_squads(argument(0)) else { return false };
                    let (offset, value) = match name {
                        "ai_allow_dormant" => (sq::DISABLE_DORMANT, (m.r8(arguments + 4) == 0) as u8),
                        "ai_timer_start" => (sq::DELAY_TIMER_STARTED, 1),
                        _ => (sq::AUTOMATIC_MIGRATION_TARGET, m.r8(arguments + 4)),
                    };
                    for (_, squad) in squads {
                        m.w8(squad + offset as u32, value);
                    }
                }
                0
            }
            // ai_scripting_allow_charge, _try_to_fight, _try_to_fight_nothing
            // and _try_to_fight_player: each actor's external orders
            name @ ("ai_allow_charge" | "ai_try_to_fight" | "ai_try_to_fight_nothing" | "ai_try_to_fight_player") => {
                use crate::layout::actor_datum as a;
                let Some(actors) = self.ai_actors(argument(0)) else { return false };
                for (_, actor) in actors {
                    match name {
                        "ai_allow_charge" => {
                            m.w8(actor + a::EXTERNAL_ORDERS_DISABLE_CHARGING as u32, (m.r8(arguments + 4) == 0) as u8)
                        }
                        "ai_try_to_fight" => {
                            m.w16(actor + a::EXTERNAL_ORDERS_DESIRED_TARGET_TYPE as u32, DESIRED_TARGET_AI);
                            m.w32(actor + a::EXTERNAL_ORDERS_DESIRED_TARGET_AI_INDEX as u32, argument(1) as u32);
                        }
                        "ai_try_to_fight_nothing" => m.w16(actor + a::EXTERNAL_ORDERS_DESIRED_TARGET_TYPE as u32, DESIRED_TARGET_NONE),
                        _ => m.w16(actor + a::EXTERNAL_ORDERS_DESIRED_TARGET_TYPE as u32, DESIRED_TARGET_PLAYER),
                    }
                }
                0
            }
            // ai_scripting_status: the furthest into combat of the actors
            "ai_status" => {
                let Some(actors) = self.ai_actors(argument(0)) else { return false };
                short_value(actors.into_iter().map(|(_, actor)| self.ai_assess_status(actor)).max().unwrap_or(0))
            }
            // object_list_from_ai_reference: a new list of the actors' units
            // and each one's swarm
            "ai_actors" => {
                use crate::layout::actor_datum as a;
                let ai_reference = argument(0);
                if ai_reference == NONE {
                    NONE
                } else {
                    let Some(list) = self.object_list_new() else { return false };
                    if list != NONE {
                        let Some(actors) = self.ai_actors(ai_reference) else { return false };
                        for (_, actor) in actors {
                            let unit_index = m.r32(actor + a::META_UNIT_INDEX as u32) as i32;
                            if unit_index != NONE && self.object_list_add(list, unit_index).is_none() {
                                return false;
                            }
                            let mut swarm_unit_index = m.r32(actor + a::META_SWARM_UNIT_INDEX as u32) as i32;
                            let mut count = 0;
                            while swarm_unit_index != NONE {
                                let Some(unit) = self.object_typed(swarm_unit_index, OBJECT_MASK_UNIT) else { return false };
                                if self.object_list_add(list, swarm_unit_index).is_none() || count > 0x10000 {
                                    return false;
                                }
                                swarm_unit_index = m.r32(unit + u::UNIT_SWARM_NEXT_UNIT_INDEX as u32) as i32;
                                count += 1;
                            }
                        }
                    }
                    list
                }
            }
            // ai_scripting_playfight, _follow_target_disable, _players, _unit
            // and _ai, and _follow_distance: the encounter's (encounter_get,
            // whatever the scenario has)
            name @ ("ai_playfight"
            | "ai_follow_target_disable"
            | "ai_follow_target_players"
            | "ai_follow_target_unit"
            | "ai_follow_target_ai"
            | "ai_follow_distance") => {
                use crate::layout::encounter_datum as e;
                let ai_reference = argument(0);
                if ai_reference != NONE {
                    let Some(encounter) = self.datum_get(self.context.encounter_data, ai_reference & 0xffff) else { return false };
                    let follow_type = encounter + e::FOLLOW_TARGET_TYPE as u32;
                    match name {
                        "ai_playfight" => m.w8(encounter + e::PLAYFIGHTING as u32, m.r8(arguments + 4)),
                        "ai_follow_target_disable" => m.w16(follow_type, FOLLOW_TARGET_NONE),
                        "ai_follow_target_players" => m.w16(follow_type, FOLLOW_TARGET_PLAYERS),
                        "ai_follow_distance" => m.w32(encounter + e::FOLLOW_TARGET_DISTANCE as u32, argument(1) as u32),
                        _ if argument(1) == NONE => m.w16(follow_type, FOLLOW_TARGET_NONE),
                        "ai_follow_target_unit" => {
                            m.w16(follow_type, FOLLOW_TARGET_UNIT);
                            m.w32(encounter + e::FOLLOW_TARGET_UNIT_INDEX as u32, argument(1) as u32);
                        }
                        _ => {
                            m.w16(follow_type, FOLLOW_TARGET_AI);
                            m.w32(encounter + ENCOUNTER_FOLLOW_TARGET_AI_INDEX, argument(1) as u32);
                        }
                    }
                }
                0
            }
            // ai_scripting_force_active: an encounter the scenario has
            "ai_force_active" => {
                let ai_reference = argument(0);
                if self.ai_initialized() && ai_reference != NONE && (ai_reference & 0xffff) < self.context.encounter_count {
                    let Some(encounter) = self.datum_get(self.context.encounter_data, ai_reference & 0xffff) else { return false };
                    m.w8(encounter + crate::layout::encounter_datum::FORCE_ACTIVE as u32, m.r8(arguments + 4));
                }
                0
            }
            // ai_scripting_set_deaf and _set_blind (encounter_set_deaf and
            // _blind)
            name @ ("ai_set_deaf" | "ai_set_blind") => {
                use crate::layout::encounter_datum as e;
                let ai_reference = argument(0);
                if ai_reference != NONE && self.ai_initialized() {
                    let Some(encounter) = self.datum_get(self.context.encounter_data, ai_reference & 0xffff) else { return false };
                    let offset = if name == "ai_set_deaf" { e::DEAF } else { e::BLIND };
                    m.w8(encounter + offset as u32, m.r8(arguments + 4));
                }
                0
            }
            // ai_scripting_prefer_target: each listed unit preferred by
            // actors, or not
            "ai_prefer_target" => {
                let Some(objects) = self.object_list(argument(0)) else { return false };
                let prefer = m.r8(arguments + 4) != 0;
                for object in objects {
                    if let Some(unit) = self.object_typed(object, OBJECT_MASK_UNIT) {
                        self.set_flag32(unit + u::UNIT_FLAGS as u32, UNIT_PREFERRED_TARGET, prefer);
                    }
                }
                0
            }
            // ai_scripting_force_active_by_unit: an encounterless actor's
            // (one of an encounter's is the game's, which reports it)
            "ai_force_active_by_unit" => {
                use crate::layout::actor_datum as a;
                let Some(unit) = self.typed_or_none(argument(0), OBJECT_MASK_UNIT) else { return false };
                if let Some(unit) = unit {
                    let actor_index = m.r32(unit + u::UNIT_ACTOR_INDEX as u32) as i32;
                    if actor_index != NONE {
                        let Some(actor) = self.datum_get(self.context.actor_data, actor_index) else { return false };
                        if m.r8(actor + ACTOR_META_ENCOUNTERLESS) == 0 {
                            return false;
                        }
                        m.w8(actor + a::META_FORCE_ACTIVE as u32, m.r8(arguments + 4));
                    }
                }
                0
            }
            // ai_globals_dialogue_triggers_enabled and _grenades_enabled
            name @ ("ai_dialogue_triggers" | "ai_grenades") => {
                if self.context.ai_globals == 0 {
                    return false;
                }
                let offset = if name == "ai_grenades" {
                    AI_GLOBALS_GRENADES_ENABLED
                } else {
                    crate::layout::ai_globals::DIALOGUE_TRIGGERS_ENABLED as u32
                };
                m.w8(self.context.ai_globals + offset, m.r8(arguments));
                0
            }
            // ai_scripting_vehicle_enterable_distance, _team, _actor_type and
            // _actors: the vehicle's entry
            name @ ("ai_vehicle_enterable_distance"
            | "ai_vehicle_enterable_team"
            | "ai_vehicle_enterable_actor_type"
            | "ai_vehicle_enterable_actors") => {
                use crate::layout::ai_vehicle_enterable as ve;
                if self.context.ai_globals == 0 {
                    return false;
                }
                let vehicle_index = argument(0);
                if vehicle_index != NONE && (name != "ai_vehicle_enterable_actors" || argument(1) != NONE) {
                    let Some(entry) = self.vehicle_enterable(vehicle_index) else { return false };
                    let bit = |offset: usize| {
                        let address = entry + offset as u32;
                        let bit = 1u32.wrapping_shl(short(1) as i32 as u32) as u16;
                        m.w16(address, m.r16(address) | bit);
                    };
                    match name {
                        "ai_vehicle_enterable_distance" => m.w32(entry + ve::RADIUS as u32, argument(1) as u32),
                        "ai_vehicle_enterable_team" => bit(ve::TEAM_BITMASK),
                        "ai_vehicle_enterable_actor_type" => bit(ve::ACTOR_TYPE_BITMASK),
                        _ => {
                            let count = m.r16(entry + ve::AI_INDICES_COUNT as u32) as i16;
                            if count >= MAXIMUM_AI_INDICES_PER_ENTERABLE_VEHICLE {
                                return false;
                            }
                            m.w32(entry + ve::AI_INDICES as u32 + 4 * count as u32, argument(1) as u32);
                            m.w16(entry + ve::AI_INDICES_COUNT as u32, (count + 1) as u16);
                        }
                    }
                }
                0
            }
            // ai_scripting_vehicle_enterable_disable: the vehicle's entry
            // removed, the last moved into its place
            "ai_vehicle_enterable_disable" => {
                use crate::layout::ai_vehicle_enterable as ve;
                if self.context.ai_globals == 0 {
                    return false;
                }
                let vehicle_index = argument(0);
                if vehicle_index != NONE {
                    let count_address = self.context.ai_globals + AI_GLOBALS_ENTERABLE_VEHICLE_COUNT;
                    let entry = |index: i16| {
                        self.context.ai_globals + AI_GLOBALS_ENTERABLE_VEHICLES + index as u32 * ve::SIZEOF as u32
                    };
                    let mut index = 0;
                    while index < m.r16(count_address) as i16 {
                        if m.r32(entry(index) + ve::VEHICLE_INDEX as u32) as i32 == vehicle_index {
                            let count = (m.r16(count_address) as i16).wrapping_sub(1);
                            m.w16(count_address, count as u16);
                            if index < count {
                                for i in 0..ve::SIZEOF as u32 / 4 {
                                    m.w32(entry(index) + 4 * i, m.r32(entry(count) + 4 * i));
                                }
                            }
                            break;
                        }
                        index += 1;
                    }
                }
                0
            }
            // ai_scripting_going_to_vehicle: the active actors on their way
            // to the vehicle
            "ai_going_to_vehicle" => {
                use crate::layout::actor_datum as a;
                let Some(actors) = self.active_actors() else { return false };
                let count = actors
                    .into_iter()
                    .filter(|&actor| {
                        m.r16(actor + a::STATE_ACTION as u32) as i16 == ACTOR_ACTION_VEHICLE
                            && m.r32(actor + ACTOR_VEHICLE_INDEX) as i32 == argument(0)
                    })
                    .count();
                short_value(count as i32)
            }
            // ai_scripting_allegiance_broken: allies whose allegiance has
            // made them enemies (game_team_is_ally, game_team_is_enemy)
            "ai_allegiance_broken" => {
                let (team0, team1) = (short(0), short(1));
                let mut broken = false;
                if team0 != NONE as i16 && team1 != NONE as i16 {
                    let teams = 0..NUMBER_OF_GAME_TEAMS;
                    if teams.contains(&team0) && teams.contains(&team1) {
                        if self.context.allegiance_globals == 0 {
                            return false;
                        }
                        let bit = (NUMBER_OF_GAME_TEAMS * team0 + team1) as u32;
                        let test = |vector: u32| {
                            m.r32(self.context.allegiance_globals + vector + 4 * (bit >> 5)) & (1 << (bit & 31)) != 0
                        };
                        broken = test(ALLEGIANCE_ALLY_BITVECTOR) && !test(ALLEGIANCE_FRIENDLY_BITVECTOR);
                    }
                }
                boolean(broken)
            }
            // ai_conversation_line: the first running copy's line, else 999
            "ai_conversation_line" => {
                use crate::layout::conversation_datum as c;
                let conversation_index = short(0);
                let mut line = 999;
                let mut index = self.data_next_index(self.context.conversation_data, NONE);
                while index != NONE {
                    let Some(conversation) = self.datum_get(self.context.conversation_data, index) else { return false };
                    if m.r16(conversation + c::SCENARIO_CONVERSATION_INDEX as u32) as i16 == conversation_index {
                        line = m.r16(conversation + CONVERSATION_LINE_INDEX) as i16 as i32;
                        break;
                    }
                    index = self.data_next_index(self.context.conversation_data, index);
                }
                short_value(line)
            }
            // ai_scripting_command_list: each actor told to obey the list
            "ai_command_list" => {
                let list_index = short(1) as i32;
                if self
                    .for_each_ai_actor(argument(0), |actor_index, _| {
                        self.effect(call::EFFECT_OBEY_COMMAND_LIST, actor_index, list_index, 0);
                        Some(())
                    })
                    .is_none()
                {
                    return false;
                }
                0
            }
            // ai_scripting_command_list_by_unit: the unit's actor told to obey
            "ai_command_list_by_unit" => {
                let unit_index = argument(0);
                if unit_index != NONE {
                    if let Some(unit) = self.object_typed(unit_index, OBJECT_MASK_UNIT) {
                        let actor_index = m.r32(unit + u::UNIT_ACTOR_INDEX as u32) as i32;
                        if actor_index != NONE {
                            if self.datum_get(self.context.actor_data, actor_index).is_none() {
                                return false;
                            }
                            self.effect(call::EFFECT_OBEY_COMMAND_LIST, actor_index, short(1) as i32, 0);
                        }
                    }
                }
                0
            }
            // ai_scripting_command_list_advance: each actor's list advanced
            "ai_command_list_advance" => {
                if self
                    .for_each_ai_actor(argument(0), |actor_index, _| {
                        self.effect(call::EFFECT_OBEY_ADVANCE, actor_index, 0, 0);
                        Some(())
                    })
                    .is_none()
                {
                    return false;
                }
                0
            }
            // ai_scripting_command_list_advance_by_unit: the unit's actor's
            // (or its swarm's)
            "ai_command_list_advance_by_unit" => {
                let unit_index = argument(0);
                if unit_index != NONE {
                    if let Some(unit) = self.object_typed(unit_index, OBJECT_MASK_UNIT) {
                        let actor_index = self.unit_actor_or_swarm(unit);
                        if actor_index != NONE {
                            self.effect(call::EFFECT_OBEY_ADVANCE, actor_index, 0, 0);
                        }
                    }
                }
                0
            }
            // ai_scripting_go_to_vehicle and _override
            // (ai_scripting_go_to_vehicle_internal): the actors nearest the
            // vehicle first, those already going to a vehicle last (the
            // UCRT's qsort), each into one of its empty seats, until one
            // already going (unless overriding)
            function @ ("ai_go_to_vehicle" | "ai_go_to_vehicle_override") => 'body: {
                use crate::layout::actor_datum as a;
                let (ai_reference, vehicle_index) = (argument(0), argument(1));
                let Some(vehicle) = self.object_typed(vehicle_index, OBJECT_MASK_UNIT) else { break 'body 0 };
                if ai_reference == NONE {
                    break 'body 0;
                }
                // (object_get_origin: a vehicle carried by another is left to the game's)
                if m.r32(vehicle + u::OBJECT_PARENT_OBJECT_INDEX as u32) as i32 != NONE {
                    return false;
                }
                let f = |address: u32| f32::from_bits(m.r32(address));
                let origin = [0, 4, 8].map(|i| f(vehicle + u::OBJECT_POSITION as u32 + i));
                let seat_name = self.c_string(argument(2) as u32);
                let Some(seats) = self.available_seats(vehicle_index, vehicle, &seat_name) else { return false };
                if seats.is_empty() {
                    break 'body 0;
                }
                let Some(actors) = self.ai_actors(ai_reference) else { return false };
                // (each candidate: the actor, its distance squared, whether it's going to a vehicle)
                let mut candidates: Vec<(i32, f32, bool)> = actors
                    .into_iter()
                    .take(64)
                    .map(|(actor_index, actor)| {
                        let body = |i: u32| f(actor + a::INPUT_BODY_POSITION as u32 + i);
                        let (dx, dy, dz) = (origin[0] - body(0), origin[1] - body(4), origin[2] - body(8));
                        let going = m.r16(actor + a::STATE_ACTION as u32) as i16 == ACTOR_ACTION_VEHICLE;
                        (actor_index, dy * dy + (dx * dx + dz * dz), going)
                    })
                    .collect();
                crate::msvc_qsort::qsort(&mut candidates, |c0, c1| {
                    use std::cmp::Ordering;
                    if c0.2 != c1.2 {
                        if c0.2 { Ordering::Greater } else { Ordering::Less }
                    } else if c0.1 < c1.1 {
                        Ordering::Less
                    } else if c0.1 > c1.1 {
                        Ordering::Greater
                    } else {
                        Ordering::Equal
                    }
                });
                let mut bits = [0u32; 2];
                for &seat in &seats {
                    if (0..64).contains(&seat) {
                        bits[(seat >> 5) as usize] |= 1 << (seat & 31);
                    }
                }
                self.effect(call::EFFECT_VEHICLE_SEATS, vehicle_index, bits[0] as i32, bits[1] as i32);
                let override_previous = function == "ai_go_to_vehicle_override";
                for (actor_index, _, going) in candidates {
                    if going && !override_previous {
                        break;
                    }
                    self.effect(call::EFFECT_ENTER_VEHICLE, actor_index, vehicle_index, 0);
                }
                0
            }
            // ai_scripting_kill and _kill_silent (ai_scripting_kill_internal)
            name @ ("ai_kill" | "ai_kill_silent") => {
                let silent = (name == "ai_kill_silent") as i32;
                if argument(0) != NONE
                    && self
                        .for_each_ai_actor(argument(0), |actor_index, _| {
                            self.effect(call::EFFECT_ACTOR_KILL, actor_index, silent, 0);
                            Some(())
                        })
                        .is_none()
                {
                    return false;
                }
                0
            }
            // ai_scripting_braindead and _berserk: each actor's
            name @ ("ai_braindead" | "ai_berserk") => {
                let kind = if name == "ai_braindead" { call::EFFECT_ACTOR_BRAINDEAD } else { call::EFFECT_ACTOR_BERSERK };
                let on = m.r8(arguments + 4) as i32;
                if argument(0) != NONE
                    && self
                        .for_each_ai_actor(argument(0), |actor_index, _| {
                            self.effect(kind, actor_index, on, 0);
                            Some(())
                        })
                        .is_none()
                {
                    return false;
                }
                0
            }
            // ai_scripting_braindead_by_unit: each listed unit's actor (or
            // its swarm's), and those of the units it carries
            "ai_braindead_by_unit" => {
                let Some(objects) = self.object_list(argument(0)) else { return false };
                let braindead = m.r8(arguments + 4) as i32;
                for object in objects {
                    let Some(unit) = self.object_typed(object, OBJECT_MASK_UNIT) else { continue };
                    let actor_index = self.unit_actor_or_swarm(unit);
                    if actor_index != NONE {
                        self.effect(call::EFFECT_ACTOR_BRAINDEAD, actor_index, braindead, 0);
                    }
                    let mut child_index = m.r32(unit + u::OBJECT_FIRST_CHILD_OBJECT_INDEX as u32) as i32;
                    let mut count = 0;
                    while child_index != NONE {
                        let Some(header) = self.datum_get(self.context.object_header_data, child_index) else { return false };
                        let child = m.r32(header + crate::layout::object_header_datum::DATUM as u32);
                        if OBJECT_MASK_UNIT & (1 << m.r16(child + u::OBJECT_TYPE as u32)) != 0 {
                            let actor_index = self.unit_actor_or_swarm(child);
                            if actor_index != NONE {
                                self.effect(call::EFFECT_ACTOR_BRAINDEAD, actor_index, braindead, 0);
                            }
                        }
                        child_index = m.r32(child + u::OBJECT_NEXT_OBJECT_INDEX as u32) as i32;
                        count += 1;
                        if count > 0x10000 {
                            return false;
                        }
                    }
                }
                0
            }
            // ai_scripting_set_current_state: each actor's default state set
            "ai_set_current_state" => {
                let state = short(1);
                if (0..NUMBER_OF_ACTOR_DEFAULT_STATES).contains(&state)
                    && self
                        .for_each_ai_actor(argument(0), |actor_index, _| {
                            self.effect(call::EFFECT_SET_DEFAULT_STATE, actor_index, state as i32, 0);
                            Some(())
                        })
                        .is_none()
                {
                    return false;
                }
                0
            }
            // ai_scripting_set_return_state: each actor's default state,
            // and one out of combat and not pursuing or active goes to it
            "ai_set_return_state" => {
                use crate::layout::actor_datum as a;
                let state = short(1);
                if (0..NUMBER_OF_ACTOR_DEFAULT_STATES).contains(&state)
                    && self
                        .for_each_ai_actor(argument(0), |actor_index, actor| {
                            // (actor_action_class)
                            let action = m.r16(actor + a::STATE_ACTION as u32) as i16;
                            let class = *crate::actor_transitions::ACTION_CLASSES.get(usize::try_from(action).ok()?)?;
                            m.w16(actor + a::STATE_DEFAULT_STATE as u32, state as u16);
                            if m.r16(actor + a::STATE_COMBAT_STATUS as u32) == 0 && class <= 2 {
                                self.effect(call::EFFECT_SET_DEFAULT_STATE, actor_index, NONE, 0);
                            }
                            Some(())
                        })
                        .is_none()
                {
                    return false;
                }
                0
            }
            // ai_scripting_exit_vehicle: each actor in a vehicle out of its seat
            "ai_exit_vehicle" => {
                use crate::layout::actor_datum as a;
                if self
                    .for_each_ai_actor(argument(0), |_, actor| {
                        let unit_index = m.r32(actor + a::META_UNIT_INDEX as u32) as i32;
                        if m.r32(actor + a::INPUT_VEHICLE_INDEX as u32) as i32 != NONE && unit_index != NONE {
                            self.effect(call::EFFECT_UNIT_EXIT_SEAT, unit_index, 0, 0);
                        }
                        Some(())
                    })
                    .is_none()
                {
                    return false;
                }
                0
            }
            // ai_scripting_stop_looking, _look_at_object and _detach_unit:
            // the unit's actor's (unit_get)
            name @ ("ai_stop_looking" | "ai_look_at_object" | "ai_detach") => {
                let unit_index = argument(0);
                if unit_index != NONE && (name != "ai_look_at_object" || argument(1) != NONE) {
                    let Some(unit) = self.object_typed(unit_index, OBJECT_MASK_UNIT) else { return false };
                    let actor_index = m.r32(unit + u::UNIT_ACTOR_INDEX as u32) as i32;
                    if actor_index != NONE {
                        match name {
                            "ai_stop_looking" => self.effect(call::EFFECT_LOOK_SECONDARY_STOP, actor_index, 0, 0),
                            "ai_look_at_object" => self.effect(call::EFFECT_LOOK_AT_OBJECT, actor_index, argument(1), 0),
                            _ => self.effect(call::EFFECT_ACTOR_DELETE, actor_index, 0, 0),
                        }
                    }
                }
                0
            }
            // ai_scripting_set_respawn (encounter_set_respawn): the
            // encounter's, and it forced active
            "ai_set_respawn" => {
                let ai_reference = argument(0);
                if ai_reference != NONE && self.ai_initialized() {
                    let encounter_index = ai_reference & 0xffff;
                    let Some(encounter) = self.datum_get(self.context.encounter_data, encounter_index) else { return false };
                    m.w8(encounter + crate::layout::encounter_datum::RESPAWN_ENABLED as u32, m.r8(arguments + 4));
                    self.effect(call::EFFECT_ENCOUNTER_FORCE_ACTIVATE, encounter_index, 0, 0);
                }
                0
            }
            // ai_scripting_magically_see_unit, _players and _encounter
            "ai_magically_see_unit" => {
                if self.magically_see_unit(argument(0), argument(1)).is_none() {
                    return false;
                }
                0
            }
            "ai_magically_see_players" => {
                let ai_reference = argument(0);
                if ai_reference != NONE {
                    let mut player_index = self.data_next_index(self.context.player_data, NONE);
                    while player_index != NONE {
                        let Some(player) = self.datum_get(self.context.player_data, player_index) else { return false };
                        let unit_index = m.r32(player + crate::layout::player_datum::UNIT_INDEX as u32) as i32;
                        if self.magically_see_unit(ai_reference, unit_index).is_none() {
                            return false;
                        }
                        player_index = self.data_next_index(self.context.player_data, player_index);
                    }
                }
                0
            }
            "ai_magically_see_encounter" => {
                use crate::layout::actor_datum as a;
                let (ai_reference, target) = (argument(0), argument(1));
                if ai_reference != NONE
                    && target != NONE
                    && self
                        .for_each_ai_actor(target, |_, actor| {
                            let unit_index = m.r32(actor + a::META_UNIT_INDEX as u32) as i32;
                            let swarm_unit_index = m.r32(actor + a::META_SWARM_UNIT_INDEX as u32) as i32;
                            if unit_index != NONE {
                                self.magically_see_unit(ai_reference, unit_index)
                            } else if swarm_unit_index != NONE {
                                self.magically_see_unit(ai_reference, swarm_unit_index)
                            } else {
                                Some(())
                            }
                        })
                        .is_none()
                {
                    return false;
                }
                0
            }
            // ai_scripting_timer_expire: each squad's delay timer
            "ai_timer_expire" => {
                let ai_reference = argument(0);
                if ai_reference != NONE {
                    let Some(squads) = self.ai_squads(ai_reference) else { return false };
                    for (squad_index, _) in squads {
                        self.effect(call::EFFECT_SQUAD_TIMER_EXPIRE, ai_reference & 0xffff, squad_index, 0);
                    }
                }
                0
            }
            // ai_scripting_erase and _place: the encounter's, platoon's or
            // squad's actors erased (ai_erase) or placed (encounter_create)
            name @ ("ai_erase" | "ai_place") => {
                let ai_reference = argument(0);
                if ai_reference != NONE {
                    let kind = ai_reference as u32 >> 30;
                    let index = (ai_reference >> 16) & 0xff;
                    let squad_index = if kind == 2 { index } else { NONE };
                    let platoon_index = if kind == 1 { index } else { NONE };
                    let effect = if name == "ai_erase" { call::EFFECT_AI_ERASE } else { call::EFFECT_ENCOUNTER_CREATE };
                    self.effect(effect, ai_reference & 0xffff, platoon_index, squad_index);
                }
                0
            }
            "ai_erase_all" => {
                self.effect(call::EFFECT_AI_ERASE, NONE, NONE, NONE);
                0
            }
            // ai_scripting_spawn_actor: an actor of the squad (a platoon's
            // first squad)
            "ai_spawn_actor" => {
                use crate::layout::{encounter_definition as ed, squad_definition as sd};
                let ai_reference = argument(0);
                if self.ai_initialized() && ai_reference != NONE {
                    let encounter_index = ai_reference & 0xffff;
                    let kind = ai_reference as u32 >> 30;
                    let index = (ai_reference >> 16) & 0xff;
                    let mut squad_index = if kind == 2 { index } else { NONE };
                    if squad_index == NONE && kind == 1 {
                        if encounter_index >= self.context.encounter_count {
                            return false;
                        }
                        let definition = self.context.encounters + encounter_index as u32 * ed::SIZEOF as u32;
                        let (squad_count, squads) = self.tag_block(definition + ed::SQUADS as u32);
                        for search_index in 0..squad_count.max(0).min(0x7fff) {
                            let squad = squads + search_index as u32 * sd::SIZEOF as u32;
                            if m.r16(squad + sd::PLATOON_INDEX as u32) as i16 as i32 == index {
                                squad_index = search_index;
                                break;
                            }
                        }
                    }
                    if squad_index != NONE {
                        self.effect(call::EFFECT_ENCOUNTER_SPAWN_ACTOR, encounter_index, squad_index, 0);
                    }
                }
                0
            }
            // ai_scripting_link_activation (its report of too many links left out)
            "ai_link_activation" => {
                if argument(0) != NONE && argument(1) != NONE {
                    self.effect(call::EFFECT_LINK_ACTIVATION, argument(0) & 0xffff, argument(1), 0);
                }
                0
            }
            // ai_scripting_conversation_stop and _advance
            name @ ("ai_conversation_stop" | "ai_conversation_advance") => {
                let kind = if name == "ai_conversation_stop" {
                    call::EFFECT_CONVERSATION_STOP
                } else {
                    call::EFFECT_CONVERSATION_ADVANCE
                };
                self.effect(kind, short(0) as i32, 0, 0);
                0
            }
            // ai_scripting_allegiance: the player's allegiance with the
            // humans or sentinels is suspicious, its incidents decaying by
            // the difficulty, the humans' needing communication
            "ai_allegiance" => {
                let (team0, team1) = (short(0), short(1));
                if team0 != NONE as i16 && team1 != NONE as i16 {
                    let special = if team0 == GAME_TEAM_PLAYER {
                        team1
                    } else if team1 == GAME_TEAM_PLAYER {
                        team0
                    } else {
                        NONE as i16
                    };
                    let (mut threshold, mut decay_time, mut communication, mut suspicious) = (NONE, NONE, false, false);
                    if special == GAME_TEAM_HUMAN || special == GAME_TEAM_SENTINEL {
                        let Some(&decay) = [300, 450, 1200, 2700].get(self.context.difficulty as usize) else { return false };
                        suspicious = true;
                        threshold = 5;
                        decay_time = decay;
                        communication = special == GAME_TEAM_HUMAN;
                    }
                    let teams = (team0 as i32 & 0xffff) | ((team1 as i32) << 16);
                    let timing = (threshold & 0xffff) | (decay_time << 16);
                    let flags = (suspicious && team0 == special) as i32
                        | ((suspicious && team1 == special) as i32) << 1
                        | (communication as i32) << 2;
                    self.effect(call::EFFECT_ALLEGIANCE_CREATE, teams, timing, flags);
                }
                0
            }
            "ai_allegiance_remove" => {
                let (team0, team1) = (short(0), short(1));
                if team0 != NONE as i16 && team1 != NONE as i16 {
                    self.effect(call::EFFECT_ALLEGIANCE_REMOVE, team0 as i32, team1 as i32, 0);
                }
                0
            }
            // ai_scripting_set_team: the encounter's, each of its actors',
            // then the teams' status
            "ai_set_team" => {
                use crate::layout::{actor_datum as a, encounter_datum as e};
                let encounter_index = argument(0) & 0xffff;
                let team = short(1);
                let Some(encounter) = self.datum_get(self.context.encounter_data, encounter_index) else { return false };
                m.w16(encounter + e::TEAM_INDEX as u32, team as u16);
                if self.ai_initialized() {
                    let mut actor_index = m.r32(encounter + e::FIRST_ACTOR_INDEX as u32) as i32;
                    let mut count = 0;
                    while actor_index != NONE {
                        let Some(actor) = self.datum_get(self.context.actor_data, actor_index) else { return false };
                        let next_index = m.r32(actor + a::META_NEXT_ACTOR_INDEX as u32) as i32;
                        self.effect(call::EFFECT_ACTOR_SET_TEAM, actor_index, team as i32, 0);
                        actor_index = next_index;
                        count += 1;
                        if count > 0x10000 {
                            return false;
                        }
                    }
                }
                self.effect(call::EFFECT_UPDATE_TEAM_STATUS, 0, 0, 0);
                0
            }
            // ai_scripting_attach_unit: an actor for the unit, of the
            // reference's squad (a platoon's first; its report of no squad
            // left out)
            "ai_attach" => {
                use crate::layout::{encounter_definition as ed, squad_definition as sd};
                let (unit_index, ai_reference) = (argument(0), argument(1));
                let encounter_index = ai_reference & 0xffff;
                if self.ai_initialized()
                    && unit_index != NONE
                    && ai_reference != NONE
                    && encounter_index < self.context.encounter_count
                {
                    let definition = self.context.encounters + encounter_index as u32 * ed::SIZEOF as u32;
                    let (squad_count, squads) = self.tag_block(definition + ed::SQUADS as u32);
                    let index = (ai_reference >> 16) & 0xff;
                    let mut squad_index = 0;
                    match ai_reference as u32 >> 30 {
                        2 => squad_index = index,
                        1 => {
                            for search_index in 0..squad_count.max(0) {
                                let squad = squads + search_index as u32 * sd::SIZEOF as u32;
                                if m.r16(squad + sd::PLATOON_INDEX as u32) as i16 as i32 == index {
                                    squad_index = search_index;
                                    break;
                                }
                            }
                        }
                        _ => {}
                    }
                    if (0..squad_count).contains(&squad_index) {
                        self.effect(call::EFFECT_ATTACH, unit_index, encounter_index, squad_index);
                    }
                }
                0
            }
            // ai_scripting_attach_free: an encounterless actor of the variant
            "ai_attach_free" => {
                let (unit_index, variant) = (argument(0), argument(1));
                if self.ai_initialized() && unit_index != NONE && variant != NONE {
                    self.effect(call::EFFECT_ATTACH_FREE, unit_index, variant, 0);
                }
                0
            }
            // ai_scripting_free: each actor out of its encounter, encounterless
            "ai_free" => {
                use crate::layout::actor_datum as a;
                let ai_reference = argument(0);
                if ai_reference != NONE {
                    if self
                        .for_each_ai_actor(ai_reference, |actor_index, actor| {
                            if m.r32(actor + a::META_ENCOUNTER_INDEX as u32) as i32 == NONE {
                                return None;
                            }
                            self.effect(call::EFFECT_FREE_ACTOR, actor_index, 0, 0);
                            Some(())
                        })
                        .is_none()
                    {
                        return false;
                    }
                    self.effect(call::EFFECT_ENCOUNTERS_UPDATE_DIRTY_STATUS, 0, 0, 0);
                }
                0
            }
            // ai_scripting_free_units: each listed unit's actor of an encounter
            "ai_free_units" => {
                use crate::layout::actor_datum as a;
                let Some(objects) = self.object_list(argument(0)) else { return false };
                let mut freed = false;
                for object in objects {
                    let Some(unit) = self.object_typed(object, OBJECT_MASK_UNIT) else { continue };
                    let actor_index = m.r32(unit + u::UNIT_ACTOR_INDEX as u32) as i32;
                    if actor_index == NONE {
                        continue;
                    }
                    let Some(actor) = self.datum_get(self.context.actor_data, actor_index) else { return false };
                    if m.r32(actor + a::META_ENCOUNTER_INDEX as u32) as i32 != NONE {
                        self.effect(call::EFFECT_FREE_ACTOR, actor_index, 0, 0);
                        freed = true;
                    }
                }
                if freed {
                    self.effect(call::EFFECT_ENCOUNTERS_UPDATE_DIRTY_STATUS, 0, 0, 0);
                }
                0
            }
            // ai_scripting_vehicle_encounter: the encounter and squad (a
            // platoon's first, else the first) the vehicle's riders join,
            // those riding it from its last one changed over
            "ai_vehicle_encounter" => {
                use crate::layout::{actor_datum as a, encounter_definition as ed, squad_definition as sd};
                let (unit_index, ai_reference) = (argument(0), argument(1));
                if unit_index != NONE {
                    let Some(vehicle) = self.object_typed(unit_index, OBJECT_MASK_UNIT) else { return false };
                    let (mut target_encounter, mut target_squad) = (NONE as i16, NONE as i16);
                    // (the reference as a short, as the game's takes it)
                    let encounter_index = ai_reference as i16;
                    if ai_reference != NONE && (0..self.context.encounter_count).contains(&(encounter_index as i32)) {
                        let definition = self.context.encounters + encounter_index as u32 * ed::SIZEOF as u32;
                        let (squad_count, squads) = self.tag_block(definition + ed::SQUADS as u32);
                        let squad_index = match ai_reference as u32 >> 30 {
                            0 => 0,
                            1 => {
                                let platoon_index = (ai_reference >> 16) & 0xff;
                                let found = (0..squad_count.max(0) as i16).find(|&search_index| {
                                    let squad = squads + search_index as u32 * sd::SIZEOF as u32;
                                    m.r16(squad + sd::PLATOON_INDEX as u32) as i16 as i32 == platoon_index
                                });
                                found.unwrap_or(0)
                            }
                            2 => ((ai_reference >> 16) & 0xff) as i16,
                            _ => return false,
                        };
                        if (0..squad_count).contains(&(squad_index as i32)) {
                            target_encounter = ai_reference as i16;
                            target_squad = squad_index;
                        }
                    }
                    if target_encounter != NONE as i16 && target_squad != NONE as i16 {
                        let previous = m.r16(vehicle + UNIT_FAKE_ENCOUNTER_INDEX) as i16;
                        if previous != NONE as i16 && self.ai_initialized() {
                            let Some(encounter) = self.datum_get(self.context.encounter_data, previous as i32) else { return false };
                            let mut actor_index = m.r32(encounter + crate::layout::encounter_datum::FIRST_ACTOR_INDEX as u32) as i32;
                            let mut count = 0;
                            while actor_index != NONE {
                                let Some(actor) = self.datum_get(self.context.actor_data, actor_index) else { return false };
                                let next_index = m.r32(actor + a::META_NEXT_ACTOR_INDEX as u32) as i32;
                                if m.r32(actor + a::INPUT_VEHICLE_INDEX as u32) as i32 == unit_index {
                                    self.effect(
                                        call::EFFECT_CHANGE_ENCOUNTER,
                                        actor_index,
                                        target_encounter as i32,
                                        target_squad as i32,
                                    );
                                }
                                actor_index = next_index;
                                count += 1;
                                if count > 0x10000 {
                                    return false;
                                }
                            }
                        }
                    }
                    m.w16(vehicle + UNIT_FAKE_ENCOUNTER_INDEX, target_encounter as u16);
                    m.w16(vehicle + UNIT_FAKE_SQUAD_INDEX, target_squad as u16);
                }
                0
            }
            // ai_scripting_teleport_starting_location and
            // _if_unsupported: each actor's
            name @ ("ai_teleport_to_starting_location" | "ai_teleport_to_starting_location_if_unsupported") => {
                let only_if_unsupported = (name == "ai_teleport_to_starting_location_if_unsupported") as i32;
                if self
                    .for_each_ai_actor(argument(0), |actor_index, _| {
                        self.effect(call::EFFECT_TELEPORT_STARTING_LOCATION, actor_index, only_if_unsupported, 0);
                        Some(())
                    })
                    .is_none()
                {
                    return false;
                }
                0
            }
            // ai_scripting_renew: each actor's unit's vitality full (none
            // where it has no maximum), and its grenades renewed
            "ai_renew" => {
                use crate::layout::actor_datum as a;
                if self
                    .for_each_ai_actor(argument(0), |actor_index, actor| {
                        let unit_index = m.r32(actor + a::META_UNIT_INDEX as u32) as i32;
                        if unit_index != NONE {
                            let unit = self.object_typed(unit_index, OBJECT_MASK_UNIT)?;
                            let f = |offset: usize| f32::from_bits(m.r32(unit + offset as u32));
                            let full = |maximum: f32| if maximum > 0.0 { 1.0f32 } else { 0.0 };
                            m.w32(unit + u::OBJECT_BODY_VITALITY as u32, full(f(u::OBJECT_MAXIMUM_BODY_VITALITY)).to_bits());
                            m.w32(unit + u::OBJECT_SHIELD_VITALITY as u32, full(f(u::OBJECT_MAXIMUM_SHIELD_VITALITY)).to_bits());
                            self.effect(call::EFFECT_RENEW_GRENADES, actor_index, 0, 0);
                        }
                        Some(())
                    })
                    .is_none()
                {
                    return false;
                }
                0
            }
            // ai_scripting_migrate and _migrate_and_speak (with the
            // maneuvering stimulus, advancing or not; its report of another
            // speech type left out)
            "ai_migrate" => {
                if self.migrate(argument(0), argument(1), None).is_none() {
                    return false;
                }
                0
            }
            "ai_migrate_and_speak" => {
                let advancing = self.c_string(argument(2) as u32).eq_ignore_ascii_case(b"advance");
                if self.migrate(argument(0), argument(1), Some(advancing)).is_none() {
                    return false;
                }
                0
            }
            // ai_scripting_migrate_by_unit: each listed unit's actor and those
            // of the units it carries, then the teams' and encounters' status
            "ai_migrate_by_unit" => {
                let (list, target) = (argument(0), argument(1));
                if list != NONE && target != NONE {
                    let Some(objects) = self.object_list(list) else { return false };
                    for object in objects {
                        let Some(unit) = self.object_typed(object, OBJECT_MASK_UNIT) else { continue };
                        if self.migrate_unit(unit, target).is_none() {
                            return false;
                        }
                        let mut child_index = m.r32(unit + u::OBJECT_FIRST_CHILD_OBJECT_INDEX as u32) as i32;
                        let mut count = 0;
                        while child_index != NONE {
                            let Some(header) = self.datum_get(self.context.object_header_data, child_index) else { return false };
                            let child = m.r32(header + crate::layout::object_header_datum::DATUM as u32);
                            if OBJECT_MASK_UNIT & (1 << m.r16(child + u::OBJECT_TYPE as u32)) != 0
                                && self.migrate_unit(child, target).is_none()
                            {
                                return false;
                            }
                            child_index = m.r32(child + u::OBJECT_NEXT_OBJECT_INDEX as u32) as i32;
                            count += 1;
                            if count > 0x10000 {
                                return false;
                            }
                        }
                    }
                    self.effect(call::EFFECT_UPDATE_TEAM_STATUS, 0, 0, 0);
                    self.effect(call::EFFECT_ENCOUNTERS_UPDATE_DIRTY_STATUS, 0, 0, 0);
                }
                0
            }
            // game_time_get, game_difficulty_level_get
            "game_time" => self.context.game_time,
            "game_difficulty_get_real" => short_value(self.context.difficulty),
            // main_set_difficulty: the next map's
            "game_difficulty_set" => {
                let difficulty = short(0);
                if (0..NUMBER_OF_GAME_DIFFICULTY_LEVELS).contains(&difficulty) {
                    let Some(level) = self.game_field(game_field::DIFFICULTY_LEVEL) else { return false };
                    m.w16(level, difficulty as u16);
                }
                0
            }
            // scripted_camera_time: the timer's ticks
            "camera_time" => {
                let Some(timer) = self.game_field(game_field::CAMERA_SCRIPT_TIMER) else { return false };
                short_value(c_long(f32::from_bits(m.r32(timer)) * 30.0) as i16 as i32)
            }
            // player_input_enable
            "player_enable_input" => {
                let Some(disabled) = self.game_field(game_field::PLAYERS_INPUT_DISABLED) else { return false };
                m.w8(disabled, (m.r8(arguments) == 0) as u8);
                0
            }
            // scripted_player_control_set_camera_control: the flag, and
            // the argument back
            "player_camera_control" => {
                let Some(globals) = self.game_field(game_field::PLAYER_CONTROL_GLOBALS) else { return false };
                let camera_control = m.r8(arguments);
                self.set_flag32(globals + 12, PLAYER_CONTROL_CAMERA_CONTROL_DISABLED, camera_control == 0);
                camera_control as i32
            }
            // player_control_action_test_*: the actions the players took
            // since the reset (accept, back and action noted as tested, and
            // suppressed)
            name @ ("player_action_test_reset"
            | "player_action_test_accept"
            | "player_action_test_back"
            | "player_action_test_action"
            | "player_action_test_jump"
            | "player_action_test_primary_trigger"
            | "player_action_test_grenade_trigger"
            | "player_action_test_zoom"
            | "player_action_test_look_relative_up"
            | "player_action_test_look_relative_down"
            | "player_action_test_look_relative_left"
            | "player_action_test_look_relative_right"
            | "player_action_test_look_relative_all_directions"
            | "player_action_test_move_relative_all_directions") => {
                let Some(globals) = self.game_field(game_field::PLAYER_CONTROL_GLOBALS) else { return false };
                let action_flags = m.r32(globals);
                let tested = |bit: u32| {
                    m.w32(globals + 4, m.r32(globals + 4) | bit);
                    m.w32(globals + 8, m.r32(globals + 8) | bit);
                    boolean(action_flags & bit != 0)
                };
                match name {
                    "player_action_test_reset" => {
                        m.w32(globals, 0);
                        m.w32(globals + 4, 0);
                        0
                    }
                    "player_action_test_accept" => tested(PLAYER_CONTROL_ACCEPT),
                    "player_action_test_back" => tested(PLAYER_CONTROL_BACK),
                    "player_action_test_action" => tested(PLAYER_CONTROL_ACTION),
                    "player_action_test_jump" => boolean(action_flags & PLAYER_CONTROL_JUMP != 0),
                    "player_action_test_primary_trigger" => boolean(action_flags & PLAYER_CONTROL_PRIMARY_TRIGGER != 0),
                    "player_action_test_grenade_trigger" => boolean(action_flags & PLAYER_CONTROL_GRENADE_TRIGGER != 0),
                    "player_action_test_zoom" => boolean(action_flags & PLAYER_CONTROL_ZOOM != 0),
                    "player_action_test_look_relative_up" => boolean(action_flags & PLAYER_CONTROL_LOOK_UP != 0),
                    "player_action_test_look_relative_down" => boolean(action_flags & PLAYER_CONTROL_LOOK_DOWN != 0),
                    "player_action_test_look_relative_left" => boolean(action_flags & PLAYER_CONTROL_LOOK_LEFT != 0),
                    "player_action_test_look_relative_right" => boolean(action_flags & PLAYER_CONTROL_LOOK_RIGHT != 0),
                    "player_action_test_look_relative_all_directions" => {
                        boolean(!action_flags & PLAYER_CONTROL_LOOK_ALL_DIRECTIONS == 0)
                    }
                    _ => boolean(!action_flags & PLAYER_CONTROL_MOVE_ALL_DIRECTIONS == 0),
                }
            }
            // players_unzoom_all: each local player's zoom
            "players_unzoom_all" => {
                let Some(zooms) = (0..4).map(|i| self.game_field(game_field::PLAYER_CONTROL_ZOOM_LEVEL0 + i)).collect::<Option<Vec<_>>>()
                else {
                    return false;
                };
                for zoom in zooms {
                    m.w16(zoom, NONE as u16);
                }
                0
            }
            // main_saving_map, and main.c's requests of the main loop: the
            // saves, won, lost, revert, reset and skip
            name @ ("game_saving"
            | "game_save"
            | "game_save_cancel"
            | "game_save_no_timeout"
            | "game_save_totally_unsafe"
            | "game_won"
            | "game_lost"
            | "game_revert"
            | "map_reset"
            | "game_skip_ticks") => {
                use game_field as g;
                let Some(saving) = self.game_field(g::MAIN_SAVING_MAP) else { return false };
                let Some(fields) = (0..15).map(|i| self.game_field(i)).collect::<Option<Vec<_>>>() else { return false };
                let field = |f: u32| fields[f as usize];
                // (main_save_map_safe's and _no_timeout's restart of the checks)
                let save_checks_restart = || {
                    m.w8(saving, 1);
                    m.w8(field(g::MAIN_SAVE_MAP_SAFELY), 1);
                    m.w32(field(g::MAIN_TICKS_UNTIL_NEXT_SAVE_CHECK), 0);
                    m.w32(field(g::MAIN_TICKS_UNABLE_TO_SAVE), 0);
                    m.w16(field(g::MAIN_SAFE_INTERVALS), 0);
                };
                let saving_or_waiting = m.r8(saving) == 0 || m.r8(field(g::MAIN_SAVE_MAP_TIMEOUT)) != 0;
                if name == "game_saving" {
                    return self.done_with(thread_index, m.r8(saving) as i32);
                }
                match name {
                    "game_save" => {
                        if saving_or_waiting {
                            save_checks_restart();
                            m.w8(field(g::MAIN_SAVE_MAP_TIMEOUT), 1);
                        }
                    }
                    "game_save_no_timeout" => {
                        if saving_or_waiting {
                            save_checks_restart();
                        }
                        m.w8(field(g::MAIN_SAVE_MAP_TIMEOUT), 0);
                    }
                    "game_save_totally_unsafe" => {
                        m.w8(saving, 1);
                        m.w8(field(g::MAIN_SAVE_MAP_SAFELY), 0);
                    }
                    "game_save_cancel" => m.w8(saving, 0),
                    "game_won" => {
                        m.w8(saving, 0);
                        m.w8(field(g::MAIN_WON_MAP), 1);
                    }
                    "game_lost" => {
                        if m.r8(field(g::MAIN_LOST_MAP)) == 0 {
                            m.w16(field(g::MAIN_LOSS_TIMER), 0);
                            m.w32(field(g::MAIN_LOSS_LAST_TICK), self.context.game_time as u32);
                        }
                        m.w8(saving, 0);
                        m.w8(field(g::MAIN_LOST_MAP), 1);
                    }
                    "game_revert" | "map_reset" => {
                        m.w16(field(g::MAIN_SWITCH_TO_STRUCTURE_BSP_INDEX), NONE as u16);
                        m.w8(saving, 0);
                        m.w8(field(if name == "game_revert" { g::MAIN_REVERT_MAP } else { g::MAIN_RESET_MAP }), 1);
                        m.w8(field(g::MAIN_LOST_MAP), 0);
                    }
                    _ => {
                        // (more than 15 is reported, the game's)
                        let ticks = short(0);
                        if ticks > 15 {
                            return false;
                        }
                        m.w16(field(g::MAIN_SKIP_TICKS), ticks as u16);
                        m.w8(field(g::MAIN_SKIP), 1);
                    }
                }
                0
            }
            // game_time_set_speed
            "game_speed" => {
                let Some(speed) = self.game_field(game_field::GAME_TIME_SPEED) else { return false };
                m.w32(speed, argument(0) as u32);
                0
            }
            // game_state_reverted: whether the revert was this tick
            "game_reverted" => {
                let Some(revert_time) = self.game_field(game_field::GAME_STATE_REVERT_TIME) else { return false };
                boolean(m.r32(revert_time) as i32 == self.context.game_time)
            }
            // scripted_player_effect_set_translation, _set_rotation (in
            // degrees), _start and _stop (in seconds), each noted for
            // network co-op's clients first
            name @ ("player_effect_set_max_translation"
            | "player_effect_set_max_rotation"
            | "player_effect_start"
            | "player_effect_stop") => {
                let Some(scripted) = self.game_field(game_field::PLAYER_EFFECT_SCRIPTED) else { return false };
                let Some(flags) = self.game_field(game_field::PLAYER_EFFECT_FLAGS) else { return false };
                let real = |index: u32| f32::from_bits(argument(index) as u32);
                let zero = 0.0f32.to_bits() as i32;
                match name {
                    "player_effect_set_max_translation" => {
                        self.effect(call::EFFECT_COOP_PLAYER_EFFECT_TRANSLATION, argument(0), argument(1), argument(2));
                        for i in 0..3 {
                            m.w32(scripted + 4 * i, argument(i) as u32);
                        }
                    }
                    "player_effect_set_max_rotation" => {
                        self.effect(call::EFFECT_COOP_PLAYER_EFFECT_ROTATION, argument(0), argument(1), argument(2));
                        for i in 0..3 {
                            // (DEGREES_TO_RADIANS)
                            m.w32(scripted + 12 + 4 * i, (real(i) * std::f32::consts::PI / 180.0).to_bits());
                        }
                    }
                    "player_effect_start" => {
                        self.effect(call::EFFECT_COOP_PLAYER_EFFECT_START, argument(0), argument(1), zero);
                        m.w32(scripted + 24, argument(0) as u32);
                        let ticks = crate::actor_combat::fast_ftol(real(1) * 30.0) as u16;
                        m.w16(scripted + 28, ticks);
                        m.w16(scripted + 30, ticks);
                        self.set_flag32(flags, SCRIPTED_PLAYER_EFFECT_STOPPING, false);
                        self.set_flag32(flags, SCRIPTED_PLAYER_EFFECT_ACTIVE, true);
                    }
                    _ => {
                        self.effect(call::EFFECT_COOP_PLAYER_EFFECT_STOP, argument(0), zero, zero);
                        let ticks = crate::actor_combat::fast_ftol(real(0) * 30.0) as u16;
                        m.w16(scripted + 28, ticks);
                        m.w16(scripted + 30, ticks);
                        self.set_flag32(flags, SCRIPTED_PLAYER_EFFECT_STOPPING, true);
                    }
                }
                0
            }
            // game_difficulty_level_get_ignore_easy: normal at the least
            "game_difficulty_get" => self.context.difficulty.max(DIFFICULTY_NORMAL),
            _ => return false,
        };
        self.hs_return(thread_index, value);
        true
    }

    /// `hs_script_evaluate`.
    fn script_evaluate(&self, script_index: i16, thread_index: i32, initialize: bool) {
        let m = self.m();
        let script = self.script(script_index as i32);
        let Some(result) = self.stack_allocate(thread_index, 4) else { return };
        if self.call(call::SCRIPT_DISABLED, script_index as i32, 0, 0) != 0 {
            let return_type = m.r16(script + SCRIPT_RETURN_TYPE) as i16;
            self.hs_return(thread_index, type_default_value(return_type, self.context.string_default));
        } else if initialize {
            self.evaluate(thread_index, m.r32(script + SCRIPT_ROOT_EXPRESSION_INDEX) as i32, result);
        } else {
            self.hs_return(thread_index, m.r32(result) as i32);
        }
    }

    fn initialized(&self) -> bool {
        self.m().r8(self.context.runtime_globals + RUNTIME_INITIALIZED) != 0
    }

    /// `hs_thread_main`.
    fn thread_main(&self, thread_index: i32) {
        let m = self.m();
        let thread = self.thread(thread_index);
        m.w16(self.context.runtime_globals + RUNTIME_EXECUTING_THREAD_INDEX, thread_index as i16 as u16);
        let thread_type = m.r8(thread + THREAD_TYPE);
        let mut script = None;
        if thread_type == THREAD_TYPE_SCRIPT {
            let script_index = m.r32(thread + THREAD_SCRIPT_INDEX) as i32;
            script = Some(self.script(script_index));
            if self.call(call::SCRIPT_DISABLED, script_index as i16 as i32, 0, 0) != 0 {
                m.w32(thread + THREAD_SLEEP_UNTIL, NONE as u32);
                m.w16(self.context.runtime_globals + RUNTIME_EXECUTING_THREAD_INDEX, NONE as u16);
                return;
            }
        }

        m.w32(thread + THREAD_SLEEP_UNTIL, 0);
        let base = thread + THREAD_STACK_DATA;
        if self.stack(thread) == base {
            m.w16(base + FRAME_SIZE, 0);
            if let Some(root_result) = self.stack_allocate(thread_index, 4) {
                let root = script.map_or(NONE, |s| m.r32(s + SCRIPT_ROOT_EXPRESSION_INDEX) as i32);
                self.evaluate(thread_index, root, root_result);
            }
        }

        loop {
            if self.stack(thread) == base || self.host.stopped() {
                break;
            }
            let sleep_until = m.r32(thread + THREAD_SLEEP_UNTIL) as i32;
            if sleep_until < 0 {
                break;
            }
            if self.call(call::GAME_IN_PROGRESS, 0, 0, 0) != 0 && sleep_until > self.context.game_time {
                break;
            }
            if !self.initialized() {
                break;
            }
            let stack = self.stack(thread);
            let expression = m.r32(stack + FRAME_EXPRESSION_INDEX) as i32;
            let node = self.syntax(expression).unwrap_or(0);
            let initialize = self.flags(thread) & THREAD_IN_FUNCTION_CALL != 0;
            m.w16(stack + FRAME_SIZE, 0);
            self.set_flag(thread, THREAD_IN_FUNCTION_CALL, false);

            let flags = m.r16(node + NODE_FLAGS) as i16;
            let index = m.r16(node + NODE_INDEX) as i16;
            let index_count = if flags & NODE_SCRIPT != 0 { self.context.script_count } else { self.context.function_count };
            if index < 0 || index as i32 >= index_count {
                self.syntax_error(thread_index);
            } else if flags & NODE_SCRIPT == 0 {
                self.evaluate_function(index, thread_index, initialize);
            } else {
                self.script_evaluate(index, thread_index, initialize);
            }

            if self.flags(thread) & THREAD_STACK_OVERFLOW != 0 {
                m.w32(thread + THREAD_STACK, base);
                m.w8(thread + THREAD_FLAGS, 0);
                m.w32(
                    thread + THREAD_SLEEP_UNTIL,
                    if m.r8(thread + THREAD_TYPE) == THREAD_TYPE_SCRIPT { NONE as u32 } else { 0 },
                );
            }
        }

        if self.stack(thread) == base {
            if thread_type == THREAD_TYPE_SCRIPT {
                if let Some(script) = script {
                    let script_type = m.r16(script + SCRIPT_TYPE) as i16;
                    if script_type == SCRIPT_STARTUP || script_type == SCRIPT_DORMANT {
                        m.w32(thread + THREAD_SLEEP_UNTIL, NONE as u32);
                    }
                }
            } else if thread_type == THREAD_TYPE_CONSOLE_COMMAND {
                self.thread_delete(thread_index);
            }
        }
        m.w16(self.context.runtime_globals + RUNTIME_EXECUTING_THREAD_INDEX, NONE as u16);
    }

    /// `hs_runtime_initialize_for_new_map`: the scenario's globals
    /// initialized (each in an internal thread of its own), and a thread for
    /// each of its scripts that runs.
    fn initialize_for_new_map(&self) {
        let m = self.m();
        self.data_make_valid(self.context.thread_data);
        m.w8(self.context.runtime_globals + RUNTIME_INITIALIZED, 1);
        m.w16(self.context.runtime_globals + RUNTIME_EXECUTING_THREAD_INDEX, NONE as u16);
        let internal_thread_index = self.thread_new(THREAD_TYPE_GLOBAL_INITIALIZE, NONE);
        if !self.context.scenario_loaded {
            return;
        }
        let internal_thread = self.thread(internal_thread_index);
        for global_index in 0..self.context.global_count as i16 {
            let global = self.context.globals + GLOBAL_SIZE * global_index as u32;
            let global_datum_index = global_index as i32 + self.context.external_global_count;
            self.datum_new_at_index(self.context.global_data, (GLOBAL_SALT << 16) | global_datum_index);
            let global_datum = self.datum_get(self.context.global_data, global_datum_index).unwrap_or(0);
            m.w32(internal_thread + THREAD_SCRIPT_INDEX, NONE as u32);
            m.w16(self.stack(internal_thread) + FRAME_SIZE, 0);
            let global_type = m.r16(global + GLOBAL_TYPE) as i16;
            if self.call(call::GLOBAL_INITIALIZER_DISABLED, global_index as i32, 0, 0) != 0 {
                m.w32(global_datum + 4, type_default_value(global_type, self.context.string_default) as u32);
            } else {
                let expression = m.r32(global + GLOBAL_INITIALIZATION_EXPRESSION_INDEX) as i32;
                self.evaluate(internal_thread_index, expression, global_datum + 4);
            }
            if self.flags(internal_thread) & THREAD_IN_FUNCTION_CALL != 0 {
                self.thread_main(internal_thread_index);
                if global_type == HS_TYPE_OBJECT_LIST {
                    self.call(call::OBJECT_LIST_ADD_REFERENCE, self.global_evaluate(global_index), 0, 0);
                }
                if m.r32(internal_thread + THREAD_SLEEP_UNTIL) != 0 {
                    self.call(call::ERROR, ERROR_GLOBAL_INITIALIZATION_SLEPT, internal_thread_index, 0);
                }
            }
            self.global_reconcile_write(global_index);
        }
        self.thread_delete(internal_thread_index);
        for script_index in 0..self.context.script_count {
            let script_type = m.r16(self.script(script_index) + SCRIPT_TYPE) as i16;
            if script_type != SCRIPT_STATIC
                && script_type != SCRIPT_STUB
                && self.call(call::SCRIPT_DISABLED, script_index as i16 as i32, 0, 0) == 0
                && self.thread_new(THREAD_TYPE_SCRIPT, script_index) == NONE
            {
                self.call(call::ERROR, ERROR_OUT_OF_SCRIPT_THREADS, 0, 0);
            }
        }
    }

    /// `hs_runtime_evaluate`: a console command's expression, in a thread of
    /// its own; its value, or NONE if it is left running.
    fn evaluate_command(&self, expression_index: i32) -> i32 {
        let m = self.m();
        if !self.initialized() || expression_index == NONE {
            return NONE;
        }
        let thread_index = self.datum_new(self.context.thread_data);
        if thread_index == NONE {
            self.call(call::ERROR, ERROR_OUT_OF_COMMAND_THREADS, 0, 0);
            return NONE;
        }
        let thread = self.thread(thread_index);
        let base = thread + THREAD_STACK_DATA;
        m.w32(thread + THREAD_STACK, base);
        m.w32(base + FRAME_PREVIOUS, 0);
        m.w16(base + FRAME_SIZE, 0);
        m.w32(base + FRAME_EXPRESSION_INDEX, NONE as u32);
        m.w8(thread + THREAD_TYPE, THREAD_TYPE_CONSOLE_COMMAND);
        m.w32(thread + THREAD_SCRIPT_INDEX, NONE as u32);
        m.w8(thread + THREAD_FLAGS, 0);
        m.w32(thread + THREAD_SLEEP_UNTIL, 0);
        self.evaluate(thread_index, expression_index, thread + THREAD_RESULT);
        if self.flags(thread) & THREAD_IN_FUNCTION_CALL != 0 {
            self.thread_main(thread_index);
            return NONE;
        }
        m.r32(thread + THREAD_RESULT) as i32
    }

    /// `hs_runtime_update`.
    fn update(&self) {
        if !self.initialized() {
            return;
        }
        let m = self.m();
        let time = self.context.game_time;
        let mut console_command_running = false;
        let mut thread_index = self.data_next_index(self.context.thread_data, NONE);
        while self.initialized() && thread_index != NONE {
            if self.host.stopped() {
                return;
            }
            let thread = self.thread(thread_index);
            if m.r8(thread + THREAD_TYPE) == THREAD_TYPE_CONSOLE_COMMAND {
                console_command_running = true;
            }
            let sleep_until = m.r32(thread + THREAD_SLEEP_UNTIL) as i32;
            if sleep_until >= 0 && sleep_until <= time {
                self.thread_main(thread_index);
            }
            thread_index = self.data_next_index(self.context.thread_data, thread_index);
        }
        self.call(call::OBJECT_LIST_GC, 0, 0, 0);
        if !console_command_running && time % 16 == 0 {
            self.call(call::NODE_GC, 0, 0, 0);
        }
    }
}

/// How much of a global the reconciliation with an external variable
/// copies (its switch's widths).
fn external_width(ty: i16) -> u32 {
    match ty {
        HS_TYPE_BOOLEAN => 1,
        HS_TYPE_REAL | HS_TYPE_LONG_INTEGER | HS_TYPE_STRING | HS_TYPE_AI | HS_TYPE_OBJECT_LIST => 4,
        24..=31 | HS_TYPE_OBJECT..=HS_TYPE_SCENERY => 4,
        _ => 2,
    }
}

/// `hs_type_default_value`.
fn type_default_value(ty: i16, string_default: u32) -> i32 {
    match ty {
        HS_TYPE_VOID | HS_TYPE_BOOLEAN | HS_TYPE_REAL | HS_TYPE_SHORT_INTEGER | HS_TYPE_LONG_INTEGER => 0,
        HS_TYPE_STRING => string_default as i32,
        HS_TYPE_SCRIPT => NONE,
        _ => NONE,
    }
}

/// `hs_runtime_initialize_for_new_map`, on the game's memory.
pub fn hs_runtime_initialize_for_new_map(context: &HsContext, host: &impl HsHost) {
    Runtime::new(context, host).initialize_for_new_map();
}

/// `hs_runtime_evaluate`, on the game's memory: the command's value, or NONE.
pub fn hs_runtime_evaluate(context: &HsContext, host: &impl HsHost, expression_index: i32) -> i32 {
    Runtime::new(context, host).evaluate_command(expression_index)
}

/// The library function's body, if it runs here (`arguments` evaluated, as
/// the runtime leaves them): false if it doesn't, or leaves this one to the
/// game's.
pub fn hs_runtime_library_body(
    context: &HsContext,
    host: &impl HsHost,
    function_index: i16,
    thread_index: i32,
    arguments: u32,
) -> bool {
    let runtime = Runtime::new(context, host);
    runtime.rust_body(function_index)
        && runtime.library_body(runtime.function_definition(function_index), thread_index, arguments)
}

/// `hs_runtime_update`: every thread due runs, on the game's memory.
pub fn hs_runtime_update(context: &HsContext, host: &impl HsHost) {
    Runtime::new(context, host).update();
}
