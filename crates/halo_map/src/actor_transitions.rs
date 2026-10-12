//! actions.c's transition handlers, by which the actor types
//! ([`crate::actor_types`]) choose an actor's action: its initial action and
//! pending command list, what denies a transition, the panics and
//! berserking its stimuli set off, surprise, the panic transition, and
//! whether a guard or a conversation can stop.
//!
//! What they ask of the rest of the game goes through [`AiHost::call`]
//! (actions.c's `actions_port_handler_call`, shared with the C).

use crate::actions::dot;
use crate::actor_types::{handler, pursuit_settings};
use crate::ai_actor::{AiCall, AiHost, AiMemoryExt, AiStepContext, NONE, PropIterator, prop_get};
use crate::layout::{
    actor_datum as a, actor_debug_info as debug, actor_definition as d, actor_variant_definition as var,
    ai_values as v, ai_vehicle_enterable as enterable, prop_datum as p,
};

/// What the handlers ask (halo_sim.h's `_actions_port_handler_call_*`).
pub mod call {
    const BASE: i32 = 0x1040;
    /// `actor_action_set_default_state(actor, NONE)`, the result.
    pub const SET_DEFAULT_STATE: i32 = BASE + 1;
    /// `action_obey_command_list_setup` of the command list `a` and, if it
    /// sets up, `actor_action_change` to obey: whether it did, the result.
    pub const OBEY_COMMAND_LIST: i32 = BASE + 2;
    /// Whether the game's local and `ai_debug.flee_always` is set, the
    /// result.
    pub const FLEE_ALWAYS: i32 = BASE + 3;
    /// `actor_perception_find_recent_damaging_prop_index(actor, TRUE)`, the
    /// result.
    pub const RECENT_DAMAGING_PROP: i32 = BASE + 4;
    /// The actor's prop of the unit responsible for its delayed attached
    /// projectile (or NONE), the result.
    pub const ATTACHED_PROJECTILE_PROP: i32 = BASE + 5;
    /// The actor's prop of the unit responsible for its unit's flaming
    /// death (or NONE), the result.
    pub const BURNING_PROP: i32 = BASE + 6;
    /// Whether the actor's squad's delay timer runs, the result.
    pub const SQUAD_DELAYED: i32 = BASE + 7;
    /// `encounter_squad_timer_expire` of the actor's squad.
    pub const SQUAD_TIMER_EXPIRE: i32 = BASE + 8;
    /// The flags of the conversation `a`'s definition, the result.
    pub const CONVERSATION_FLAGS: i32 = BASE + 9;
    /// `actor_move_animation_impulse(actor, a, input[0..2])`, the result.
    pub const ANIMATION_IMPULSE: i32 = BASE + 10;
    /// `ai_communication_event(a, unit, b, input[0], NONE, NONE, NULL)`.
    pub const COMMUNICATION: i32 = BASE + 11;
    /// `actor_combat_fire_wildly(actor, a)`.
    pub const FIRE_WILDLY: i32 = BASE + 12;
    /// `actor_combat_disable_bursts(actor, a)`.
    pub const DISABLE_BURSTS: i32 = BASE + 13;
    /// `actor_stimulus_was_surprised(actor)`.
    pub const WAS_SURPRISED: i32 = BASE + 14;
    /// `actor_situation_try_new_target(actor, a)`.
    pub const TRY_NEW_TARGET: i32 = BASE + 15;
    /// `actor_combat_get_firing_variant_definition(actor)`, its address the
    /// result.
    pub const FIRING_VARIANT: i32 = BASE + 16;
    /// `real_seed_random(global seed)`, in `output[0]`.
    pub const RANDOM: i32 = BASE + 17;
    /// `actor_action_try_to_panic(actor, a, b, input[0])`, the result.
    pub const TRY_TO_PANIC: i32 = BASE + 18;
    /// `actor_action_handle_combat_selection(actor)`, the result.
    pub const COMBAT_SELECTION: i32 = BASE + 19;
    /// `actor_action_handle_lost_contact(actor)`, the result.
    pub const LOST_CONTACT: i32 = BASE + 20;
    /// `action_guard_setup_from_fleeing` and `actor_action_change` to guard.
    pub const GUARD_FROM_FLEEING: i32 = BASE + 21;
    /// `actor_berserk(actor, TRUE)`.
    pub const BERSERK: i32 = BASE + 22;
    /// `action_guard_setup_from_combat_transition` and, if it sets up,
    /// `actor_action_change` to guard: whether it did, the result.
    pub const GUARD_FROM_COMBAT_TRANSITION: i32 = BASE + 23;
    /// `actor_perception_tried_to_uncover(actor, a)`.
    pub const TRIED_TO_UNCOVER: i32 = BASE + 24;
    /// `actor_perception_tried_to_search(actor, a)`.
    pub const TRIED_TO_SEARCH: i32 = BASE + 25;
    /// `actor_perception_abandoned_search(actor, a)`.
    pub const ABANDONED_SEARCH: i32 = BASE + 26;
    /// `actor_action_try_to_seek_cover(actor, a, b)`, the result.
    pub const TRY_TO_SEEK_COVER: i32 = BASE + 27;
    /// `actor_has_ranged_weapon(actor)`, the result.
    pub const HAS_RANGED_WEAPON: i32 = BASE + 28;
    /// The game's difficulty value `a` (the melee delay's scale, 0, or its
    /// base, 1), in `output[0]`.
    pub const DIFFICULTY: i32 = BASE + 29;
    /// `action_charge_setup` for the goal `a` and, if it sets up,
    /// `actor_action_change` to charge: whether it did, the result.
    pub const CHARGE: i32 = BASE + 30;
    /// The definition of the actor's vehicle, its address the result.
    pub const VEHICLE_DEFINITION: i32 = BASE + 31;
    /// `action_fight_setup` and `actor_action_change` to fight.
    pub const FIGHT: i32 = BASE + 32;
    /// Whether the actor's encounter is standing down, the result.
    pub const ENCOUNTER_STAND_DOWN: i32 = BASE + 33;
    /// `encounter_modify_pursuit_desires` of the actor's squad: in `input`
    /// and `output`, whether to pursue tenaciously, the group's restriction,
    /// whether it controls the group's pursuit, and the desires to search at
    /// the target, to pursue and to search the pursuit.
    pub const PURSUIT_DESIRES: i32 = BASE + 34;
    /// `encounter_determine_pursuit_availability` (`a` the group's
    /// restriction, `b` whether it controls the pursuit): its eight flags in
    /// `input` and, by bit, the result.
    pub const PURSUIT_AVAILABILITY: i32 = BASE + 35;
    /// `actor_perception_find_prop_pathfinding_location(actor, a)`.
    pub const FIND_PROP_PATHFINDING_LOCATION: i32 = BASE + 36;
    /// `actor_nearby_firing_positions` around the prop `a`'s pathfinding
    /// point, for searching: the result.
    pub const NEARBY_FIRING_POSITIONS: i32 = BASE + 37;
    /// `action_uncover_setup_target(actor, a)` and, if it sets up,
    /// `actor_action_change` to uncover: whether it did, the result.
    pub const UNCOVER_TARGET: i32 = BASE + 38;
    /// `action_search_setup_target(actor, a)` and the change to search: the
    /// result.
    pub const SEARCH_TARGET: i32 = BASE + 39;
    /// `actor_target_unit_index(actor)`, the result.
    pub const TARGET_UNIT: i32 = BASE + 40;
    /// `action_search_setup_undirected(actor, a)` and the change to search:
    /// the result.
    pub const SEARCH_UNDIRECTED: i32 = BASE + 41;
    /// `actor_select_firing_position` of a pursuit position (`a` tenacious,
    /// `b` the orphan's last perceived time): its index, the result.
    pub const SELECT_PURSUIT_POSITION: i32 = BASE + 42;
    /// `action_uncover_setup_pursuit(actor, a)` and the change to uncover:
    /// the result.
    pub const UNCOVER_PURSUIT: i32 = BASE + 43;
    /// `action_search_setup_pursuit(actor, a, b)` and the change to search:
    /// the result.
    pub const SEARCH_PURSUIT: i32 = BASE + 44;
    /// `encounter_mark_examined_pursuit_position(encounter, actor, a, b)`,
    /// the result.
    pub const MARK_EXAMINED: i32 = BASE + 45;
    /// `action_wait_setup(actor, a)` and the change to wait: the result.
    pub const WAIT: i32 = BASE + 46;
    /// `action_guard_setup_postcombat` and the change to guard: the result.
    pub const GUARD_POSTCOMBAT: i32 = BASE + 47;
    /// `action_guard_setup_find_position(actor, a)` and the change to guard.
    pub const GUARD_FIND_POSITION: i32 = BASE + 48;
    /// `actor_action_allow_cover_seeking(actor, a)`, the result.
    pub const ALLOW_COVER_SEEKING: i32 = BASE + 49;
    /// `unit_try_and_exit_seat` of the actor's unit, the result.
    pub const EXIT_SEAT: i32 = BASE + 50;
    /// `actor_action_consider_grenade(actor)`, the result.
    pub const CONSIDER_GRENADE: i32 = BASE + 51;
    /// `actor_action_try_to_evade(actor)`, the result.
    pub const TRY_TO_EVADE: i32 = BASE + 52;
    /// `actor_move_animation_busy(actor)`, the result.
    pub const ANIMATION_BUSY: i32 = BASE + 53;
    /// `actor_path_has_path(actor)`, the result.
    pub const HAS_PATH: i32 = BASE + 54;
    /// `actor_discard_firing_position(actor, a, TRUE)`.
    pub const DISCARD_FIRING_POSITION: i32 = BASE + 55;
    /// `actor_action_find_escape_from_danger(actor)`: whether it found one,
    /// the result; in `output`, the escape's direction, distance, alignment
    /// (two) and whether it's a ledge.
    pub const FIND_ESCAPE: i32 = BASE + 56;
    /// `actor_action_try_to_dive(actor, a, input[0], input[1..3], input[3])`,
    /// the result.
    pub const TRY_TO_DIVE: i32 = BASE + 57;
    /// `actor_path_at_destination(actor)`, the result.
    pub const AT_DESTINATION: i32 = BASE + 58;
    /// `action_avoid_setup` and, if it sets up, the change to avoid: the
    /// result.
    pub const AVOID: i32 = BASE + 59;
    /// Whether the AI may throw grenades (`ai_globals->grenades_enabled`),
    /// the result.
    pub const GRENADES_ENABLED: i32 = BASE + 60;
    /// The difficulty's grenade chance scale for the actor's team, in
    /// `output[0]`.
    pub const GRENADE_CHANCE_SCALE: i32 = BASE + 61;
    /// `actor_action_test_grenade(actor)`, the result.
    pub const TEST_GRENADE: i32 = BASE + 62;
    /// `actor_action_try_to_throw_grenade(actor, a)`.
    pub const TRY_TO_THROW_GRENADE: i32 = BASE + 63;
    /// `actor_action_allowed_to_enter_vehicle(actor, a)`, the result.
    pub const ALLOWED_TO_ENTER_VEHICLE: i32 = BASE + 64;
    /// Whether the vehicle `a` is one, the result, and its driver, `result2`.
    pub const VEHICLE_DRIVER: i32 = BASE + 65;
    /// `object_get_origin(a)`, in `output`.
    pub const OBJECT_ORIGIN: i32 = BASE + 66;
    /// The AI's enterable vehicles: the address of their count, the result,
    /// and of the first, `result2`.
    pub const ENTERABLE_VEHICLES: i32 = BASE + 67;
    /// Whether the vehicle `a` is one, the result.
    pub const VEHICLE_EXISTS: i32 = BASE + 68;
    /// `action_vehicle_setup_impromptu` of the vehicle `a` (`input` the
    /// attempt's and continuing's distances) and, if it sets up, the change to
    /// the vehicle action: the result.
    pub const VEHICLE_SETUP: i32 = BASE + 69;
    /// Whether the actor's unit can throw (not busy, not hurt this tick), the
    /// result.
    pub const UNIT_READY_TO_THROW: i32 = BASE + 70;
    /// The actor's encounter's last grenade throw is now.
    pub const MARK_GRENADE_THROWN: i32 = BASE + 71;
    /// The actor's encounter's last grenade throw time, the result, and the
    /// difficulty's grenade timer scale for its team, `output[0]`.
    pub const ENCOUNTER_GRENADE_TIMER: i32 = BASE + 72;
    /// `actor_combat_find_grenade_target(actor)`: whether it found one, the
    /// result; the target in `output[0..3]`, its prop `result2`, and the
    /// object to ignore in `output[3]`'s bits.
    pub const FIND_GRENADE_TARGET: i32 = BASE + 73;
    /// `actor_combat_check_collateral_damage(actor, input[0], input[1],
    /// input[2..5])`, the result, and the enemies counted, `result2`.
    pub const COLLATERAL_DAMAGE: i32 = BASE + 74;
    /// `actor_combat_plan_grenade_trajectory(actor, a, input[0..3], b,
    /// input[3]'s bits)`, the result.
    pub const PLAN_GRENADE_TRAJECTORY: i32 = BASE + 75;
    /// The actor's unit's evade distance, in `output[0]`.
    pub const UNIT_EVADE_DISTANCE: i32 = BASE + 76;
    /// `actor_move_try_evasion_direction(actor, input[0..2], input[2])` (a
    /// random direction, no ledge avoidance), the result; the alignment it
    /// leaves in `output[0..2]` and the direction in `output[2]`.
    pub const TRY_EVASION_DIRECTION: i32 = BASE + 77;
    /// `unit_test_animation_impulse` of the actor's unit and `a`, the result.
    pub const TEST_ANIMATION_IMPULSE: i32 = BASE + 78;
    /// `action_guard_setup_current_position` and the change to guard: the
    /// result.
    pub const GUARD_CURRENT_POSITION: i32 = BASE + 79;
    /// `action_guard_setup_find_position(actor, a)` and, if it sets up, the
    /// change to guard: the result.
    pub const GUARD_FIND_POSITION_IF: i32 = BASE + 80;
    /// `action_alert_setup(actor, a, NONE)` and, if it sets up, the change to
    /// alert: the result.
    pub const ALERT: i32 = BASE + 81;
    /// `actor_action_change(actor, sleep, NULL)`.
    pub const SLEEP: i32 = BASE + 82;
    /// The actor's unit's dive distance, in `output[0]`.
    pub const UNIT_DIVE_DISTANCE: i32 = BASE + 83;
    /// `actor_move_try_evasion_vector(actor, input[0..2], input[2],
    /// input[3])`, the result, and whether it's a ledge, `result2`.
    pub const TRY_EVASION_VECTOR: i32 = BASE + 84;
    /// `actor_move_try_evasion_direction(actor, input[0..2], input[2], a,
    /// input[3])`, the result; the alignment it leaves in `output[0..2]` and
    /// the direction in `output[2]`.
    pub const EVASION_DIRECTION: i32 = BASE + 85;
    /// The start of `actor_action_change`: its profile meter, and the end of
    /// the actor's action `a`.
    pub const ACTION_END: i32 = BASE + 86;
    /// `actor_clear_discarded_firing_positions(actor, FALSE)`.
    pub const CLEAR_DISCARDED_FIRING_POSITIONS: i32 = BASE + 87;
    /// The begin of the action `a`.
    pub const ACTION_BEGIN: i32 = BASE + 88;
    /// `seed_random_range(global seed, a, b)`, the result.
    pub const SEED_RANDOM_RANGE: i32 = BASE + 89;
    /// `actor_nearby_firing_positions` around the point `input[0..3]` on the
    /// surface `a`, for searching: the result.
    pub const NEARBY_FIRING_POSITIONS_AT: i32 = BASE + 90;
    /// `real_seed_random_range(global seed, input[0], input[1])`, in
    /// `output[0]`.
    pub const REAL_SEED_RANDOM_RANGE: i32 = BASE + 91;
    /// action_flee.c's `action_flee_find_flee_position(actor, FALSE)` for a
    /// flee being set up (its panic type `a`, prop `b`, forced ticks
    /// `input[0]`, and whether it flees from the last visible location,
    /// `input[1]`, and allows occluded points, `input[2]`): the position, the
    /// result; whether it was found randomly (bit 0) and has an approach
    /// point (bit 1), `result2`; the approach point in `output[0..3]`.
    pub const FIND_FLEE_POSITION: i32 = BASE + 92;
    /// Whether the actor's unit's melee attack is inhibited, the result.
    pub const MELEE_INHIBITED: i32 = BASE + 93;
    /// `unit_get_melee_range_and_ticks` of the actor's unit (`a` leaping),
    /// with the game's warning of an animation with no damage keyframe
    /// (unless `b`, suicidal): the result; the start and end ticks in
    /// `result2`'s low and high halves, their ranges in `output[0..2]`.
    pub const MELEE_RANGE_AND_TICKS: i32 = BASE + 94;
    /// action_charge.c's `action_charge_valid_melee_destination` for the goal
    /// `a` and the point `input[0..3]`, the result.
    pub const VALID_MELEE_DESTINATION: i32 = BASE + 95;
    /// `actor_move_to_prop(actor, a, input[0])`, the result.
    pub const MOVE_TO_PROP: i32 = BASE + 96;
    /// `actor_move_keep_moving_past_destination(actor)`.
    pub const KEEP_MOVING: i32 = BASE + 97;
    /// action_vehicle.c's `action_vehicle_desirable` of the vehicle `a` to an
    /// actor about to try it (the attempt's and continuing's distances
    /// `input[0..2]`), the result.
    pub const VEHICLE_DESIRABLE: i32 = BASE + 98;
    /// action_vehicle.c's `action_vehicle_find_impromptu_seat` in the vehicle
    /// `a`: the seat, the result (its entry kept by the game for
    /// [`FIND_VEHICLE_DESTINATION`]).
    pub const FIND_IMPROMPTU_SEAT: i32 = BASE + 99;
    /// `unit_has_animation_to_enter_seat` of the actor's unit, the vehicle `a`
    /// and the seat `b`, the result.
    pub const HAS_ENTER_ANIMATION: i32 = BASE + 100;
    /// action_vehicle.c's `action_vehicle_find_destination` to the vehicle
    /// `a`'s seat [`FIND_IMPROMPTU_SEAT`] found: whether it found one, the
    /// result; the point in `output[0..3]` and its surface `result2`.
    pub const FIND_VEHICLE_DESTINATION: i32 = BASE + 101;
    /// `actor_move_to_point(actor, input[0..3], a, b)`, the result.
    pub const MOVE_TO_POINT: i32 = BASE + 102;
}

/// The difficulty values [`call::DIFFICULTY`] reads.
mod difficulty {
    pub const MELEE_DELAY_SCALE: i32 = 0;
    pub const MELEE_DELAY_BASE: i32 = 1;
}

/// actions.c's vehicle definition's AI fields (its local layout).
const VEHICLE_AI_CHARGE_REPEAT_TIME: u32 = 0x390;
const VEHICLE_AI_STRAFING_STOP_RANGE: u32 = 0x394;

/// The actions (actions.h's `_actor_action_*`).
const ACTION_NONE: i16 = 0;
const ACTION_SLEEP: i16 = 1;
const ACTION_ALERT: i16 = 2;
const ACTION_FIGHT: i16 = 3;
const ACTION_FLEE: i16 = 4;
const ACTION_UNCOVER: i16 = 5;
const ACTION_GUARD: i16 = 6;
const ACTION_SEARCH: i16 = 7;
const ACTION_WAIT: i16 = 8;
const ACTION_VEHICLE: i16 = 9;
const ACTION_CHARGE: i16 = 10;
const ACTION_OBEY: i16 = 11;
const ACTION_AVOID: i16 = 13;

/// Each action's class (actions.c's `global_action_functions`).
const ACTION_CLASSES: [i16; 14] = [0, 0, 0, 4, 2, 3, 1, 3, 3, 2, 4, 2, 2, 2];

/// actions.c's panics (`_actor_panic_*`).
const PANIC_NONE: i16 = 0;
const PANIC_DAMAGE: i16 = 1;
const PANIC_SURPRISE: i16 = 7;
const PANIC_DELAYED_PROJECTILE_ATTACHED_TO_US: i16 = 10;
const PANIC_MELEE_ATTACHED_TO_US: i16 = 11;
const PANIC_BURNING_TO_DEATH: i16 = 12;

/// actions.c's berserking (`_actor_berserk_*`).
const BERSERK_ATTACKING_MODE: i16 = 1;
const BERSERK_PROXIMITY: i16 = 2;
const BERSERK_DAMAGE: i16 = 3;
const BERSERK_ATTACHED_PROJECTILE: i16 = 4;

/// actions.c's conversation flags (`_ai_conversation_stop_if_*_bit`).
const CONVERSATION_STOP_IF_DAMAGED_BIT: u32 = 1;
const CONVERSATION_STOP_IF_VISIBLE_ENEMY_BIT: u32 = 2;
const CONVERSATION_STOP_IF_ALERTED_TO_ENEMY_BIT: u32 = 3;

/// actions.c's surprise animations (`_actor_animation_surprise_*`).
const ANIMATION_SURPRISE_FRONT: i32 = 4;
const ANIMATION_SURPRISE_BACK: i32 = 5;

fn call_full(context: &AiStepContext, host: &impl AiHost, kind: i32, a: i32, b: i32, input: &[f32]) -> AiCall {
    let mut call = AiCall { kind, actor: context.actor_index, a, b, ..AiCall::default() };
    call.input[..input.len()].copy_from_slice(input);
    host.call(&mut call);
    call
}

fn call_simple(context: &AiStepContext, host: &impl AiHost, kind: i32, a: i32) -> i32 {
    call_full(context, host, kind, a, 0, &[]).result
}

fn normalize2(v: &mut [f32; 2]) -> f32 {
    let magnitude = (v[0] * v[0] + v[1] * v[1]).sqrt();
    if !(0.0001 > magnitude.abs()) {
        let s = 1.0 / magnitude;
        *v = [s * v[0], s * v[1]];
        magnitude
    } else {
        0.0
    }
}

/// The handler `kind` ([`crate::actor_types::handler`]), with its arguments:
/// what it returns.
pub fn run(kind: i32, a: i32, b: i32, c: i32, context: &AiStepContext, host: &impl AiHost) -> bool {
    match kind {
        handler::INITIAL_ACTION => handle_initial_action(context, host),
        handler::PENDING_COMMAND_LIST => handle_pending_command_list(context, host),
        handler::SURPRISE => handle_surprise(context, host, a as i16),
        handler::DENY_TRANSITION => deny_transition(context, host),
        handler::PANIC_FROM_SURPRISE => handle_panic_from_surprise(context, host),
        handler::PANIC_FROM_DAMAGE => handle_panic_from_damage(context, host),
        handler::PANIC_FROM_ATTACHED_PROJECTILES => handle_panic_from_attached_projectiles(context, host),
        handler::PANIC_FROM_ATTACHED_MELEE_ATTACKERS => handle_panic_from_attached_melee_attackers(context, host),
        handler::PANIC_FROM_BURNING_TO_DEATH => handle_panic_from_burning_to_death(context, host),
        handler::PANIC_TRANSITION => handle_panic_transition(context, host, a as i16, b != 0, c as i16),
        handler::CAN_STOP_GUARDING => can_stop_guarding(context, host, a as i16, b as i16),
        handler::CAN_STOP_CONVERSING => can_stop_conversing(context, host),
        handler::BERSERKING_FROM_ATTACKING_MODE => handle_berserking_from_attacking_mode(context, host),
        handler::BERSERKING_FROM_DAMAGE => handle_berserking_from_damage(context, host),
        handler::BERSERKING_FROM_PROXIMITY => handle_berserking_from_proximity(context, host),
        handler::BERSERKING_FROM_ATTACHED_PROJECTILES => handle_berserking_from_attached_projectiles(context, host),
        handler::DONE_FLEEING => handle_done_fleeing(context, host),
        handler::COMBAT_STATUS => handle_combat_status(context, host, a != 0, b != 0),
        handler::COMBAT_FAILURE => handle_combat_failure(context, host),
        handler::BERSERK_TRANSITION => handle_berserk_transition(context, host, a as i16),
        handler::COMBAT_TRANSITION => handle_combat_transition(context, host),
        handler::EXIT_PURSUIT => handle_exit_pursuit(context, host),
        handler::ACTIVE_COVER_SEEKING => handle_active_cover_seeking(context, host, a != 0, b != 0),
        handler::VEHICLE_EXIT => handle_vehicle_exit(context, host),
        handler::EVASION => handle_evasion(context, host),
        handler::DANGER_AVOIDANCE => handle_danger_avoidance(context, host),
        handler::GRENADE_THROWING => handle_grenade_throwing(context, host),
        handler::CONSIDER_GRENADE => consider_grenade(context, host),
        handler::VEHICLE_ENTRY => handle_vehicle_entry(context, host),
        _ => unreachable!("handler {kind}"),
    }
}

/// `actor_action_handle_initial_action`: an actor with no action takes its
/// default (unless it's asleep).
pub fn handle_initial_action(context: &AiStepContext, host: &impl AiHost) -> bool {
    let m = host.memory();
    let at = |offset: usize| context.actor + offset as u32;
    if m.i16(at(a::STATE_ACTION)) == ACTION_NONE && m.i16(at(a::STATE_MODE)) != 0 {
        set_default_state(context, host, NONE as i16)
    } else {
        false
    }
}

/// `actor_action_handle_pending_command_list`: a command list given the
/// actor starts obeying (unless the transition's denied or the actor's
/// asleep, for a list that isn't immediate).
pub fn handle_pending_command_list(context: &AiStepContext, host: &impl AiHost) -> bool {
    let m = host.memory();
    let at = |offset: usize| context.actor + offset as u32;
    let mut entered_obey = false;
    let command_list_index = m.i16(at(a::STATE_COMMAND_LIST_INDEX));
    if command_list_index != NONE as i16 {
        let blocked = !m.bool(at(a::STATE_COMMAND_LIST_IMMEDIATE))
            && (m.i16(at(a::STATE_MODE)) == 0 || deny_transition(context, host));
        if !blocked {
            let command_list_index = m.i16(at(a::STATE_COMMAND_LIST_INDEX));
            if call_simple(context, host, call::OBEY_COMMAND_LIST, command_list_index as i32) != 0 {
                entered_obey = true;
            }
            m.set_bool(at(a::STATE_COMMAND_LIST_IMMEDIATE), false);
            m.set_i16(at(a::STATE_COMMAND_LIST_INDEX), NONE as i16);
        }
    }
    entered_obey
}

/// Raises the actor's panic to `panic_type`, taking `panic_prop_index` as
/// what it panics from when it had no panic or no prop.
fn raise_panic(context: &AiStepContext, host: &impl AiHost, panic_type: i16, panic_prop_index: i32) {
    let m = host.memory();
    let at = |offset: usize| context.actor + offset as u32;
    if m.i16(at(a::STIMULI_PANIC_TYPE)) == PANIC_NONE || m.i32(at(a::STIMULI_PANIC_PROP_INDEX)) == NONE {
        m.w32(at(a::STIMULI_PANIC_PROP_INDEX), panic_prop_index as u32);
    }
    m.set_i16(at(a::STIMULI_PANIC_TYPE), m.i16(at(a::STIMULI_PANIC_TYPE)).max(panic_type));
}

/// `actor_action_handle_panic_from_attached_melee_attackers`.
pub fn handle_panic_from_attached_melee_attackers(context: &AiStepContext, host: &impl AiHost) -> bool {
    let m = host.memory();
    let at = |offset: usize| context.actor + offset as u32;
    if m.bool(at(a::INPUT_MELEE_ATTACKER_ATTACHED)) {
        m.set_i16(at(a::STIMULI_PANIC_TYPE), m.i16(at(a::STIMULI_PANIC_TYPE)).max(PANIC_MELEE_ATTACHED_TO_US));
        m.w32(at(a::STIMULI_PANIC_PROP_INDEX), NONE as u32);
        true
    } else {
        false
    }
}

/// `actor_action_handle_panic_from_surprise`.
pub fn handle_panic_from_surprise(context: &AiStepContext, host: &impl AiHost) -> bool {
    let m = host.memory();
    let at = |offset: usize| context.actor + offset as u32;
    let flags = m.r32(context.definition + d::FLAGS as u32);
    if m.bool(at(a::STIMULI_WAS_SURPRISED)) && flags & (1 << v::_ACTOR_DEFINITION_PANIC_ON_SURPRISE_BIT) != 0 {
        raise_panic(context, host, PANIC_SURPRISE, m.i32(at(a::STIMULI_SURPRISE_PROP_INDEX)));
        m.set_bool(at(a::STIMULI_WAS_SURPRISED), false);
        true
    } else {
        false
    }
}

/// `actor_action_handle_panic_from_damage`.
pub fn handle_panic_from_damage(context: &AiStepContext, host: &impl AiHost) -> bool {
    let m = host.memory();
    let at = |offset: usize| context.actor + offset as u32;
    if m.bool(at(a::STIMULI_WAS_DAMAGED))
        && (call_simple(context, host, call::FLEE_ALWAYS, 0) != 0
            || m.f32(at(a::INPUT_RECENT_BODY_DAMAGE)) > m.f32(context.definition + d::PANIC_PANIC_DAMAGE_THRESHOLD as u32))
    {
        if m.i16(at(a::STIMULI_PANIC_TYPE)) == PANIC_NONE || m.i32(at(a::STIMULI_PANIC_PROP_INDEX)) == NONE {
            let prop_index = call_simple(context, host, call::RECENT_DAMAGING_PROP, 0);
            m.w32(at(a::STIMULI_PANIC_PROP_INDEX), prop_index as u32);
        }
        m.set_i16(at(a::STIMULI_PANIC_TYPE), m.i16(at(a::STIMULI_PANIC_TYPE)).max(PANIC_DAMAGE));
        m.set_bool(at(a::STIMULI_WAS_DAMAGED), false);
        true
    } else {
        false
    }
}

/// `actor_action_handle_panic_from_attached_projectiles`.
pub fn handle_panic_from_attached_projectiles(context: &AiStepContext, host: &impl AiHost) -> bool {
    let m = host.memory();
    if m.i32(context.actor + a::INPUT_DELAYED_ATTACHED_PROJECTILE_INDEX as u32) != NONE {
        let prop_index = call_simple(context, host, call::ATTACHED_PROJECTILE_PROP, 0);
        raise_panic(context, host, PANIC_DELAYED_PROJECTILE_ATTACHED_TO_US, prop_index);
        true
    } else {
        false
    }
}

/// `actor_action_handle_panic_from_burning_to_death`.
pub fn handle_panic_from_burning_to_death(context: &AiStepContext, host: &impl AiHost) -> bool {
    let m = host.memory();
    let at = |offset: usize| context.actor + offset as u32;
    if m.bool(at(a::INPUT_BURNING_TO_DEATH)) {
        let mut prop_index = NONE;
        if m.i32(at(a::META_UNIT_INDEX)) != NONE {
            prop_index = call_simple(context, host, call::BURNING_PROP, 0);
        }
        raise_panic(context, host, PANIC_BURNING_TO_DEATH, prop_index);
        true
    } else {
        false
    }
}

/// Raises the actor's berserking to `berserk_type`.
fn raise_berserk(context: &AiStepContext, host: &impl AiHost, berserk_type: i16) {
    let m = host.memory();
    let address = context.actor + a::STIMULI_BERSERK_TYPE as u32;
    m.set_i16(address, m.i16(address).max(berserk_type));
}

/// `actor_action_handle_berserking_from_attacking_mode`.
pub fn handle_berserking_from_attacking_mode(context: &AiStepContext, host: &impl AiHost) -> bool {
    let m = host.memory();
    let at = |offset: usize| context.actor + offset as u32;
    let flags = m.r32(context.definition + d::FLAGS as u32);
    if flags & (1 << v::_ACTOR_DEFINITION_ALWAYS_BERSERK_IN_ATTACKING_MODE_BIT) != 0
        && !m.bool(at(a::EXTERNAL_ORDERS_DEFENDING))
        && m.i16(at(a::STATE_COMBAT_STATUS)) >= 5
    {
        raise_berserk(context, host, BERSERK_ATTACKING_MODE);
        true
    } else {
        false
    }
}

/// `actor_action_handle_berserking_from_damage`.
pub fn handle_berserking_from_damage(context: &AiStepContext, host: &impl AiHost) -> bool {
    let m = host.memory();
    let at = |offset: usize| context.actor + offset as u32;
    let definition = |offset: usize| m.f32(context.definition + offset as u32);
    if m.bool(at(a::STIMULI_WAS_DAMAGED))
        && m.f32(at(a::INPUT_RECENT_BODY_DAMAGE)) > definition(d::BERSERK_DAMAGE_BERSERK_AMOUNT)
        && m.f32(at(a::INPUT_BODY_VITALITY)) < definition(d::BERSERK_DAMAGE_BERSERK_THRESHOLD)
    {
        raise_berserk(context, host, BERSERK_DAMAGE);
        m.set_bool(at(a::STIMULI_WAS_DAMAGED), false);
        true
    } else {
        false
    }
}

/// `actor_action_handle_berserking_from_proximity`.
pub fn handle_berserking_from_proximity(context: &AiStepContext, host: &impl AiHost) -> bool {
    let m = host.memory();
    let at = |offset: usize| context.actor + offset as u32;
    if m.i16(at(a::STATE_COMBAT_STATUS)) >= 5 {
        let prop = prop_get(m, context, m.i32(at(a::TARGET_TARGET_PROP_INDEX)));
        if m.f32(prop + p::DISTANCE as u32) < m.f32(context.definition + d::BERSERK_PROXIMITY_BERSERK_DISTANCE as u32) {
            raise_berserk(context, host, BERSERK_PROXIMITY);
            return true;
        }
    }
    false
}

/// `actor_action_handle_berserking_from_attached_projectiles`.
pub fn handle_berserking_from_attached_projectiles(context: &AiStepContext, host: &impl AiHost) -> bool {
    let m = host.memory();
    let at = |offset: usize| context.actor + offset as u32;
    let firing_variant = call_simple(context, host, call::FIRING_VARIANT, 0) as u32;
    if m.i32(at(a::INPUT_DELAYED_ATTACHED_PROJECTILE_INDEX)) != NONE && m.i16(at(a::STATE_COMBAT_STATUS)) >= 5 {
        let prop = prop_get(m, context, m.i32(at(a::TARGET_TARGET_PROP_INDEX)));
        if m.f32(prop + p::DISTANCE as u32) < m.f32(firing_variant + var::RANGED_COMBAT_BERSERK_FIRING_RANGE_UPPER_BOUND as u32)
            && call_full(context, host, call::RANDOM, 0, 0, &[]).output[0]
                < m.f32(context.definition + d::BERSERK_GRENADE_ATTACHED_CHANCE as u32)
        {
            raise_berserk(context, host, BERSERK_ATTACHED_PROJECTILE);
            return true;
        }
    }
    false
}

/// `actor_action_deny_transition`: an actor waiting on its command list's
/// delay or its squad's (unless it's in combat, which ends the squad's), or
/// obeying without initiative, keeps its action.
pub fn deny_transition(context: &AiStepContext, host: &impl AiHost) -> bool {
    let m = host.memory();
    let at = |offset: usize| context.actor + offset as u32;
    let mut deny = false;
    if m.i16(at(a::STATE_COMMAND_LIST_INDEX)) != NONE as i16 && m.i16(at(a::STATE_COMMAND_LIST_DELAY_TIMER)) > 0 {
        deny = true;
    }
    if m.i32(at(a::META_ENCOUNTER_INDEX)) != NONE && call_simple(context, host, call::SQUAD_DELAYED, 0) != 0 {
        if m.i16(at(a::STATE_COMBAT_STATUS)) >= 5 {
            call_simple(context, host, call::SQUAD_TIMER_EXPIRE, 0);
        } else {
            deny = true;
        }
    }
    if m.i16(at(a::STATE_ACTION)) == ACTION_OBEY
        && !m.bool(at(a::STATE_OBEY_INITIATIVE))
        && !m.bool(at(a::STATE_OBEY_FINISHED))
    {
        deny = true;
    }
    deny
}

/// `actor_action_can_stop_guarding`.
pub fn can_stop_guarding(
    context: &AiStepContext,
    host: &impl AiHost,
    guard_investigate_threshold: i16,
    cower_investigate_threshold: i16,
) -> bool {
    let m = host.memory();
    let at = |offset: usize| context.actor + offset as u32;
    let combat_status = m.i16(at(a::STATE_COMBAT_STATUS));
    if m.bool(at(a::STATE_GUARD_COWER)) {
        return combat_status >= cower_investigate_threshold;
    }
    if m.i16(at(a::STATE_GUARD_WAIT_TICKS)) <= 0 {
        return true;
    }
    combat_status >= guard_investigate_threshold
        || (m.i16(at(a::EXTERNAL_ORDERS_POSTCOMBAT_TYPE)) > 0 && !m.bool(at(a::STATE_GUARD_POST_COMBAT)))
}

/// `actor_action_can_stop_conversing`: by the conversation's flags, when
/// damaged, seeing an enemy or alerted to one.
pub fn can_stop_conversing(context: &AiStepContext, host: &impl AiHost) -> bool {
    let m = host.memory();
    let at = |offset: usize| context.actor + offset as u32;
    let conversation_index = m.i32(at(a::EXTERNAL_ORDERS_CONVERSATION_INDEX));
    if conversation_index == NONE {
        return true;
    }
    let flags = call_simple(context, host, call::CONVERSATION_FLAGS, conversation_index) as u32 & 0xFFFF;
    let target_type = m.i16(at(a::TARGET_TARGET_TYPE));
    (flags & (1 << CONVERSATION_STOP_IF_DAMAGED_BIT) != 0 && m.i8(at(a::SITUATION_SPECIFIC_THREATS) + 8) != 0)
        || (flags & (1 << CONVERSATION_STOP_IF_VISIBLE_ENEMY_BIT) != 0 && target_type >= 9)
        || (flags & (1 << CONVERSATION_STOP_IF_ALERTED_TO_ENEMY_BIT) != 0 && target_type >= 6)
}

/// `actor_action_handle_surprise`: an actor surprised enough (and not a
/// passenger) flinches toward or away from what surprised it, shouts, fires
/// wildly or holds its fire as its variant says, and takes what surprised
/// it as a target; its surprise then clears.
pub fn handle_surprise(context: &AiStepContext, host: &impl AiHost, surprise_level: i16) -> bool {
    let m = host.memory();
    let at = |offset: usize| context.actor + offset as u32;
    let mut result = false;
    if !m.bool(at(a::INPUT_VEHICLE_PASSENGER)) && m.i16(at(a::STIMULI_SURPRISE_LEVEL)) >= surprise_level {
        let (direction, animation) = if m.bool(at(a::STIMULI_SURPRISE_HAS_VECTOR)) {
            let vector = m.vector(at(a::STIMULI_SURPRISE_VECTOR));
            let mut direction = [vector[0], vector[1]];
            normalize2(&mut direction);
            let facing = m.vector(at(a::CONTROL_DESIRED_FACING_VECTOR));
            if !(direction[1] * facing[1] + direction[0] * facing[0] >= 0.0) {
                ([-direction[0], -direction[1]], ANIMATION_SURPRISE_BACK)
            } else {
                (direction, ANIMATION_SURPRISE_FRONT)
            }
        } else {
            let facing = m.vector(at(a::INPUT_FACING_VECTOR));
            let mut direction = [facing[0], facing[1]];
            normalize2(&mut direction);
            (direction, ANIMATION_SURPRISE_FRONT)
        };
        call_full(context, host, call::ANIMATION_IMPULSE, animation, 0, &direction);

        let mut unit_index = NONE;
        let mut weapon_state = 0;
        let surprise_prop_index = m.i32(at(a::STIMULI_SURPRISE_PROP_INDEX));
        if surprise_prop_index != NONE {
            let prop = prop_get(m, context, surprise_prop_index);
            unit_index = m.i32(prop + p::UNIT_INDEX as u32);
            weapon_state = m.bool(prop + p::ENEMY as u32) as i32 + 2;
        }
        call_full(
            context,
            host,
            call::COMMUNICATION,
            v::_AI_COMMUNICATION_SURPRISE as i32,
            unit_index,
            &[weapon_state as f32],
        );

        let variant = |offset: usize| m.f32(context.variant_definition + offset as u32);
        let wildfire_time = variant(var::RANGED_COMBAT_SURPRISE_WILDFIRE_TIME);
        if wildfire_time > 0.0 {
            call_simple(context, host, call::FIRE_WILDLY, (wildfire_time * 30.0) as i32);
        }
        let delay_time = variant(var::RANGED_COMBAT_SURPRISE_DELAY_TIME);
        if delay_time > 0.0 {
            call_simple(context, host, call::DISABLE_BURSTS, (delay_time * 30.0) as i32);
        }
        call_simple(context, host, call::WAS_SURPRISED, 0);
        let surprise_prop_index = m.i32(at(a::STIMULI_SURPRISE_PROP_INDEX));
        if surprise_prop_index != NONE {
            call_simple(context, host, call::TRY_NEW_TARGET, surprise_prop_index);
        }
        result = true;
    }
    m.set_i16(at(a::STIMULI_SURPRISE_LEVEL), 0);
    result
}

/// `actor_action_handle_panic_transition`: a panic of at least
/// `minimum_panic_type` (not in a passenger) raises a fleeing actor's panic,
/// or (unless a flee just failed) makes it panic; one below
/// `force_panic_type` only shouts that it wants to when
/// `communicate_failure`. The actor's panic then clears.
pub fn handle_panic_transition(
    context: &AiStepContext,
    host: &impl AiHost,
    minimum_panic_type: i16,
    communicate_failure: bool,
    force_panic_type: i16,
) -> bool {
    let m = host.memory();
    let at = |offset: usize| context.actor + offset as u32;
    let panic_type = m.i16(at(a::STIMULI_PANIC_TYPE));
    let mut result = false;
    'done: {
        if panic_type >= minimum_panic_type && !m.bool(at(a::INPUT_VEHICLE_PASSENGER)) {
            let flee_panic_type = m.i16(at(a::STATE_FLEE_PANIC_TYPE));
            if m.i16(at(a::STATE_ACTION)) == ACTION_FLEE && flee_panic_type > PANIC_NONE {
                m.set_i16(at(a::STATE_FLEE_PANIC_TYPE), flee_panic_type.max(panic_type));
                break 'done;
            }
            let last_flee_failed_time = m.i32(at(a::EMOTIONS_LAST_FLEE_FAILED_TIME));
            if last_flee_failed_time != NONE && last_flee_failed_time.wrapping_add(7) >= context.game_time {
                break 'done;
            }
            let force = m.i16(at(a::STIMULI_PANIC_TYPE)) >= force_panic_type;
            if communicate_failure && !force {
                call_full(
                    context,
                    host,
                    call::COMMUNICATION,
                    v::_AI_COMMUNICATION_ATTEMPTED_FLEE as i32,
                    NONE,
                    &[NONE as f32],
                );
                break 'done;
            }
            result = try_to_panic(context, host, m.i16(at(a::STIMULI_PANIC_TYPE)), m.i32(at(a::STIMULI_PANIC_PROP_INDEX)), force);
        }
    }
    m.set_i16(at(a::STIMULI_PANIC_TYPE), PANIC_NONE);
    result
}

fn combat_selection(context: &AiStepContext, host: &impl AiHost) -> bool {
    handle_combat_selection(context, host)
}

fn lost_contact(context: &AiStepContext, host: &impl AiHost) -> bool {
    handle_lost_contact(context, host)
}

/// `actor_action_handle_done_fleeing`: a fled actor guards.
pub fn handle_done_fleeing(context: &AiStepContext, host: &impl AiHost) -> bool {
    let m = host.memory();
    let at = |offset: usize| context.actor + offset as u32;
    if m.i16(at(a::STATE_ACTION)) == ACTION_FLEE && m.bool(at(a::STATE_FLEE_DONE_FLEEING)) {
        let data = guard_setup_from_fleeing(context, host);
        action_change(context, host, ACTION_GUARD, Some(&data));
        true
    } else {
        false
    }
}

/// `actor_action_handle_combat_status`: by the action's class, a new
/// combat action when the actor can take the initiative (or must decide) and
/// is in combat, or the lost-contact decision when it isn't (or a pursuit's
/// target changed or is yet to be uncovered or searched).
pub fn handle_combat_status(
    context: &AiStepContext,
    host: &impl AiHost,
    allow_initiative: bool,
    force_decision: bool,
) -> bool {
    let m = host.memory();
    let at = |offset: usize| context.actor + offset as u32;
    let action_class = ACTION_CLASSES[m.i16(at(a::STATE_ACTION)) as usize];
    let allow_initiative = allow_initiative || force_decision;
    let combat_status = || m.i16(at(a::STATE_COMBAT_STATUS));
    let mut result = false;

    match action_class {
        1 | 2 => {
            if allow_initiative {
                if combat_status() >= 5 {
                    result = combat_selection(context, host);
                } else if combat_status() >= 2
                    || m.i16(at(a::STATE_ACTION)) == ACTION_ALERT
                    || (combat_status() == 0
                        && (m.bool(at(a::EXTERNAL_ORDERS_STAND_DOWN))
                            || m.i16(at(a::EXTERNAL_ORDERS_POSTCOMBAT_TYPE)) > 0))
                {
                    result = lost_contact(context, host);
                }
            }
        }
        4 => {
            result = if combat_status() >= 4 { combat_selection(context, host) } else { lost_contact(context, host) };
        }
        3 => {
            if allow_initiative && combat_status() >= 4 {
                result = combat_selection(context, host);
            } else if combat_status() < 2 {
                result = lost_contact(context, host);
            } else {
                let target_prop_index = m.i32(at(a::TARGET_TARGET_PROP_INDEX));
                if target_prop_index != NONE {
                    let prop = prop_get(m, context, target_prop_index);
                    let action = m.i16(at(a::STATE_ACTION));
                    // the game's reads the search's pursuit location for an
                    // uncover too (the two share it)
                    let pursuit_location_type = m.i16(at(a::STATE_SEARCH_PURSUIT_LOCATION_TYPE));
                    if target_prop_index != m.i32(at(a::FIRING_POSITIONS_PURSUIT_PROP_INDEX))
                        || (!m.bool(prop + p::TRIED_TO_UNCOVER as u32)
                            && (action != ACTION_UNCOVER || pursuit_location_type != 0))
                        || (!m.bool(prop + p::TRIED_TO_SEARCH as u32)
                            && (action != ACTION_UNCOVER || pursuit_location_type != 0)
                            && (action != ACTION_SEARCH || pursuit_location_type != 0))
                    {
                        result = lost_contact(context, host);
                    }
                }
            }
        }
        _ => {}
    }

    if !result && force_decision {
        result = lost_contact(context, host);
    }
    result
}

/// `actor_action_handle_combat_failure`: a charge's melee attack that ended
/// or couldn't advance, or a vehicle's charge that couldn't, chooses again.
pub fn handle_combat_failure(context: &AiStepContext, host: &impl AiHost) -> bool {
    let m = host.memory();
    let at = |offset: usize| context.actor + offset as u32;
    if m.i16(at(a::STATE_ACTION)) != ACTION_CHARGE {
        return false;
    }
    let goal = m.i16(at(a::STATE_CHARGE_GOAL));
    let failed = if goal == v::_CHARGE_GOAL_MELEE as i16 || goal == v::_CHARGE_GOAL_MELEE_LEAPING as i16 {
        m.bool(at(a::STATE_CHARGE_FINISHED_MELEE_ATTACK))
            || m.bool(at(a::STATE_CHARGE_ABORTED_MELEE_ATTACK))
            || m.bool(at(a::STATE_CHARGE_UNABLE_TO_ADVANCE))
    } else if goal == v::_CHARGE_GOAL_VEHICLE_STRAFING as i16 || goal == v::_CHARGE_GOAL_VEHICLE_RAMMING as i16 {
        m.bool(at(a::STATE_CHARGE_UNABLE_TO_ADVANCE))
    } else {
        false
    };
    failed && combat_selection(context, host)
}

/// `actor_action_handle_berserk_transition`: berserking of at least
/// `berserk_level` sends the actor berserk (choosing a combat action if it's
/// in combat); its berserking then clears.
pub fn handle_berserk_transition(context: &AiStepContext, host: &impl AiHost, berserk_level: i16) -> bool {
    let m = host.memory();
    let at = |offset: usize| context.actor + offset as u32;
    let mut result = false;
    if m.i16(at(a::STIMULI_BERSERK_TYPE)) >= berserk_level && !m.bool(at(a::EMOTIONS_BERSERK)) {
        call_simple(context, host, call::BERSERK, 0);
        if m.i16(at(a::STATE_COMBAT_STATUS)) >= 4 {
            result = combat_selection(context, host);
        }
    }
    m.set_i16(at(a::STIMULI_BERSERK_TYPE), 0);
    result
}

/// `actor_action_handle_combat_transition`: an actor going into combat
/// guards (or chooses a combat action).
pub fn handle_combat_transition(context: &AiStepContext, host: &impl AiHost) -> bool {
    let m = host.memory();
    let at = |offset: usize| context.actor + offset as u32;
    if m.i16(at(a::STATE_MODE)) < 3 && m.i16(at(a::STIMULI_COMBAT_TRANSITION)) != 0 {
        m.set_i16(at(a::STATE_MODE), 3);
        let data = guard_setup_from_combat_transition(context, host);
        action_change(context, host, ACTION_GUARD, Some(&data));
        m.set_i16(at(a::STIMULI_COMBAT_TRANSITION), 0);
        true
    } else {
        false
    }
}

/// `actor_action_handle_exit_pursuit`: a finished uncover, search or wait
/// reports it to the target's prop and takes the lost-contact decision.
pub fn handle_exit_pursuit(context: &AiStepContext, host: &impl AiHost) -> bool {
    let m = host.memory();
    let at = |offset: usize| context.actor + offset as u32;
    let target_prop_index = || m.i32(at(a::TARGET_TARGET_PROP_INDEX));
    let mut reported = false;
    match m.i16(at(a::STATE_ACTION)) {
        ACTION_UNCOVER => {
            if m.bool(at(a::STATE_UNCOVER_UNCOVER_DONE)) {
                if m.i16(at(a::STATE_UNCOVER_PURSUIT_LOCATION_TYPE)) == 0 {
                    call_simple(context, host, call::TRIED_TO_UNCOVER, target_prop_index());
                }
                reported = true;
            }
        }
        ACTION_SEARCH => {
            if m.bool(at(a::STATE_SEARCH_SEARCH_DONE)) {
                if m.i16(at(a::STATE_SEARCH_PURSUIT_LOCATION_TYPE)) == 0 {
                    call_simple(context, host, call::TRIED_TO_SEARCH, target_prop_index());
                }
                reported = true;
            }
        }
        ACTION_WAIT => {
            if m.bool(at(a::STATE_WAIT_WAIT_DONE)) {
                call_simple(context, host, call::ABANDONED_SEARCH, target_prop_index());
                reported = true;
            }
        }
        _ => {}
    }
    reported && lost_contact(context, host)
}

fn c_short(value: f32) -> i16 {
    let long = if (-2_147_483_648.0..2_147_483_648.0).contains(&value) { value as i32 } else { i32::MIN };
    long as i16
}

/// `actor_action_handle_combat_selection`: the actor's combat action. A
/// discovered stalker seeks cover or goes berserk; an actor near enough a
/// target (on foot, or with no gun) charges it to melee, as often as its
/// melee delay allows; a flying driver strafes a target it can't see; and
/// otherwise it charges when it wants to (forced, gunless, or still charging)
/// or fights.
pub fn handle_combat_selection(context: &AiStepContext, host: &impl AiHost) -> bool {
    let m = host.memory();
    let at = |offset: usize| context.actor + offset as u32;
    let definition = |offset: usize| context.definition + offset as u32;
    let variant = |offset: usize| context.variant_definition + offset as u32;
    let firing_variant = call_simple(context, host, call::FIRING_VARIANT, 0) as u32;
    let has_ranged_weapon = || call_simple(context, host, call::HAS_RANGED_WEAPON, 0) != 0;
    let action = || m.i16(at(a::STATE_ACTION));
    let goal = || m.i16(at(a::STATE_CHARGE_GOAL));
    let goal_melee = |goal: i16| goal == v::_CHARGE_GOAL_MELEE as i16 || goal == v::_CHARGE_GOAL_MELEE_LEAPING as i16;
    let definition_flag = |bit: usize| m.r32(definition(d::FLAGS)) & (1 << bit) != 0;
    let mut action_changed = false;
    let mut prop = 0;
    let mut prop_distance = f32::MAX;

    let target_prop_index = m.i32(at(a::TARGET_TARGET_PROP_INDEX));
    if target_prop_index != NONE {
        prop = prop_get(m, context, target_prop_index);
        prop_distance = m.f32(prop + p::DISTANCE as u32);

        if action() == ACTION_CHARGE && goal() == v::_CHARGE_GOAL_STALKING as i16 {
            let discovery_time = m.f32(definition(d::DEFENSIVE_STALKING_DISCOVERY_TIME));
            let discovered = m.bool(prop + p::CURRENTLY_DAMAGING_ME as u32)
                || (m.bool(prop + p::SHOOTING as u32) && m.i8(prop + p::QUANTIZED_DISTANCE as u32) <= 1)
                || (discovery_time > 0.0
                    && m.i16(at(a::STATE_CHARGE_STALKING_DISCOVERY_TIMER)) >= c_short(discovery_time * 30.0));
            if discovered {
                if prop_distance > m.f32(firing_variant + var::RANGED_COMBAT_COMBAT_RANGE_UPPER_BOUND as u32) {
                    action_changed = try_to_seek_cover(context, host, false, false);
                }
                if !action_changed {
                    call_simple(context, host, call::BERSERK, 0);
                }
            }
        }

        if (!has_ranged_weapon() || (m.i32(prop + p::VEHICLE_INDEX as u32) == NONE && !m.bool(prop + p::SWARM as u32)))
            && (action() != ACTION_CHARGE || !goal_melee(goal()))
            && !action_changed
            && !m.bool(at(a::META_SWARM))
            && m.i32(at(a::INPUT_VEHICLE_INDEX)) == NONE
            && m.i16(at(a::CONTROL_FIRE_STATE)) != v::_ACTOR_FIRE_STATE_BURSTING as i16
        {
            let current_time = context.game_time;
            let berserk = m.bool(at(a::EMOTIONS_BERSERK));
            let mut use_berserk_range = berserk;
            if !has_ranged_weapon() && !definition_flag(v::_ACTOR_DEFINITION_STALKING_BEHAVIOR_BIT) {
                use_berserk_range = true;
            }
            let charge_delay = if berserk { 0.0 } else { m.f32(definition(d::BERSERK_MELEE_ATTACK_DELAY_TIMER)) };
            let scale = call_full(context, host, call::DIFFICULTY, difficulty::MELEE_DELAY_SCALE, 0, &[]).output[0];
            let base = call_full(context, host, call::DIFFICULTY, difficulty::MELEE_DELAY_BASE, 0, &[]).output[0];
            let charge_delay = scale * charge_delay + base;
            let charge_range = m.f32(variant(if use_berserk_range {
                var::RANGED_COMBAT_BERSERK_MELEE_RANGE
            } else {
                var::RANGED_COMBAT_MELEE_RANGE
            }));
            let mut begin_charge = false;

            let last_melee_check_time = m.i32(at(a::EMOTIONS_LAST_MELEE_CHECK_TIME));
            if (last_melee_check_time == NONE || last_melee_check_time.wrapping_add(10) < current_time)
                && !(prop_distance > charge_range)
            {
                let mut in_range = true;
                if m.bool(at(a::EXTERNAL_ORDERS_DISABLE_CHARGING)) {
                    let melee_attack_range = m.f32(definition(d::BERSERK_MELEE_ATTACK_RANGE));
                    let point_blank_range = if 0.0 > melee_attack_range { 0.0 } else { melee_attack_range };
                    if prop_distance > point_blank_range + 0.8 {
                        in_range = false;
                    }
                }
                if in_range {
                    let last_charge_time = m.i32(at(a::EMOTIONS_LAST_MELEE_ATTACK_TIME));
                    if last_charge_time == NONE || !(charge_delay * 30.0 + last_charge_time as f32 >= current_time as f32) {
                        // the game's asks again, the answer unused
                        has_ranged_weapon();
                        begin_charge = true;
                    }
                }
            }

            if begin_charge {
                m.w32(at(a::EMOTIONS_LAST_MELEE_CHECK_TIME), current_time as u32);
                if charge(context, host, v::_CHARGE_GOAL_MELEE as i16) {
                    action_changed = true;
                }
            }
        }

        let driver_type = m.i16(at(a::INPUT_VEHICLE_DRIVER_TYPE));
        if action() != ACTION_CHARGE
            && !m.bool(at(a::EXTERNAL_ORDERS_DISABLE_CHARGING))
            && !action_changed
            && driver_type > v::_ACTOR_VEHICLE_DRIVER_NONE as i16
        {
            let vehicle_definition = call_simple(context, host, call::VEHICLE_DEFINITION, 0) as u32;
            let last_charge_time = m.i32(at(a::EMOTIONS_LAST_VEHICLE_CHARGE_TIME));
            let repeat_elapsed = last_charge_time == NONE
                || m.f32(vehicle_definition + VEHICLE_AI_CHARGE_REPEAT_TIME) * 30.0 + (last_charge_time as f32)
                    < context.game_time as f32;
            if repeat_elapsed {
                let begin_charge = driver_type == v::_ACTOR_VEHICLE_DRIVER_DIRECTIONAL_FLYING as i16
                    && prop_distance > m.f32(firing_variant + var::RANGED_COMBAT_MELEE_RANGE as u32)
                    && m.i16(prop + p::LINE_OF_SIGHT as u32) == 0;
                if begin_charge
                    && charge(context, host, v::_CHARGE_GOAL_VEHICLE_STRAFING as i16)
                {
                    action_changed = true;
                }
            }
        }
    }

    if !action_changed {
        let disable_charging = || m.bool(at(a::EXTERNAL_ORDERS_DISABLE_CHARGING));
        let mut desires_charge = m.bool(at(a::EMOTIONS_FORCED_TO_CHARGE)) && !disable_charging();
        let mut restart_charge = false;

        if !disable_charging()
            && !has_ranged_weapon()
            && definition_flag(v::_ACTOR_DEFINITION_CHARGE_IN_ATTACKING_MODE_BIT)
        {
            desires_charge = true;
        }

        if action() == ACTION_CHARGE {
            let goal = goal();
            let unable_to_advance = m.bool(at(a::STATE_CHARGE_UNABLE_TO_ADVANCE));
            if goal_melee(goal) {
                if !m.bool(at(a::STATE_CHARGE_FINISHED_MELEE_ATTACK))
                    && !m.bool(at(a::STATE_CHARGE_ABORTED_MELEE_ATTACK))
                    && !unable_to_advance
                {
                    desires_charge = true;
                } else {
                    restart_charge = true;
                }
            } else if disable_charging() {
                desires_charge = false;
                restart_charge = true;
            } else if goal == v::_CHARGE_GOAL_VEHICLE_STRAFING as i16 || goal == v::_CHARGE_GOAL_VEHICLE_RAMMING as i16 {
                if unable_to_advance || m.i16(at(a::INPUT_VEHICLE_DRIVER_TYPE)) <= v::_ACTOR_VEHICLE_DRIVER_UNKNOWN as i16 {
                    restart_charge = true;
                } else {
                    desires_charge = true;
                    if goal == v::_CHARGE_GOAL_VEHICLE_STRAFING as i16 {
                        let vehicle_definition = call_simple(context, host, call::VEHICLE_DEFINITION, 0) as u32;
                        let stop_range = m.f32(vehicle_definition + VEHICLE_AI_STRAFING_STOP_RANGE);
                        if m.bool(at(a::CONTROL_PATH_AT_DESTINATION))
                            && m.i16(at(a::CONTROL_PATH_DESTINATION_TYPE)) == v::_DESTINATION_PROP as i16
                            && m.i32(at(a::CONTROL_PATH_DESTINATION_PROP_INDEX)) == m.i32(at(a::TARGET_TARGET_PROP_INDEX))
                        {
                            desires_charge = false;
                        } else if prop_distance < stop_range {
                            desires_charge = false;
                        } else if prop_distance < stop_range * 2.0
                            && dot(m.vector(at(a::INPUT_FACING_VECTOR)), m.vector(prop + p::ACTOR_TO_PROP as u32)) < 0.5
                        {
                            desires_charge = false;
                        }
                    }
                }
            }
        }

        if desires_charge && (restart_charge || action() != ACTION_CHARGE) {
            if charge(context, host, v::_CHARGE_GOAL_CLOSE_RANGE as i16) {
                action_changed = true;
            } else {
                desires_charge = false;
            }
        }

        if !desires_charge && !action_changed && action() != ACTION_FIGHT {
            action_change(context, host, ACTION_FIGHT, Some(&ActionData::new()));
            action_changed = true;
        }
    }

    action_changed
}

/// The pursuit an actor that lost contact may take
/// (`actor_action_determine_pursuit_options`'s flags).
#[derive(Default)]
struct PursuitOptions {
    allow_target_uncover: bool,
    allow_indefinite_target_uncover: bool,
    allow_target_search: bool,
    allow_pursuit: bool,
    allow_pursuit_search: bool,
    wait_after_pursuit: bool,
}

/// actions.c's `actor_action_determine_pursuit_options`: the desires (2
/// never, 1 not when controlling a group's pursuit) and the target's prop
/// limit the pursuit (uncover what's yet to be, search what's yet to be
/// where there are positions to search from), and so does the vehicle.
#[allow(clippy::too_many_arguments)]
fn determine_pursuit_options(
    context: &AiStepContext,
    host: &impl AiHost,
    desire_target_search: i16,
    desire_pursuit: i16,
    desire_pursuit_search: i16,
    controlling_group: bool,
    should_return_to_idle: bool,
    force_charge: bool,
    options: &mut PursuitOptions,
) {
    let m = host.memory();
    let at = |offset: usize| context.actor + offset as u32;
    if desire_target_search == 2 || (desire_target_search == 1 && controlling_group) {
        options.allow_target_search = false;
    }
    if desire_pursuit == 2 || (desire_pursuit == 1 && controlling_group) {
        options.allow_pursuit = false;
        options.allow_pursuit_search = false;
    } else if desire_pursuit_search == 2 || (desire_pursuit_search == 1 && controlling_group) {
        options.allow_pursuit_search = false;
    }
    if should_return_to_idle {
        options.allow_pursuit = false;
        options.allow_pursuit_search = false;
        options.wait_after_pursuit = false;
    }

    let target_prop_index = m.i32(at(a::TARGET_TARGET_PROP_INDEX));
    if target_prop_index == NONE {
        options.allow_target_uncover = false;
        options.allow_target_search = false;
        return;
    }
    let prop = prop_get(m, context, target_prop_index);
    options.allow_target_uncover = !m.bool(prop + p::TRIED_TO_UNCOVER as u32);
    options.allow_target_search = !m.bool(prop + p::TRIED_TO_SEARCH as u32);
    if force_charge {
        options.allow_target_uncover = false;
    } else if options.allow_target_search {
        let state = m.i16(prop + p::STATE as u32);
        if (2..=3).contains(&state) {
            call_simple(context, host, call::FIND_PROP_PATHFINDING_LOCATION, target_prop_index);
        }
        options.allow_target_search = call_simple(context, host, call::NEARBY_FIRING_POSITIONS, target_prop_index) != 0;
    }

    if !options.allow_indefinite_target_uncover && m.i16(at(a::STATE_COMBAT_STATUS)) < 3 {
        options.allow_target_uncover = false;
    }
    let driver_type = m.i16(at(a::INPUT_VEHICLE_DRIVER_TYPE));
    if driver_type > 0 {
        options.wait_after_pursuit = false;
    }
    if driver_type == 4 {
        options.allow_target_search = false;
        options.allow_pursuit_search = false;
    }
}

/// `actor_action_handle_lost_contact`: an actor that lost contact uncovers
/// or searches its target if it may, else pursues (to a position the
/// encounter hasn't examined, or undirected for a swarm), else waits; else
/// guards after combat, returns to idle when its encounter stands down, or
/// guards to find a position.
pub fn handle_lost_contact(context: &AiStepContext, host: &impl AiHost) -> bool {
    let m = host.memory();
    let at = |offset: usize| context.actor + offset as u32;
    let combat_status = || m.i16(at(a::STATE_COMBAT_STATUS));
    let action = || m.i16(at(a::STATE_ACTION));
    let target_prop_index = || m.i32(at(a::TARGET_TARGET_PROP_INDEX));
    let communicate = |kind: usize, unit_index: i32| {
        call_full(context, host, call::COMMUNICATION, kind as i32, unit_index, &[NONE as f32]);
    };
    let encounter_index = m.i32(at(a::META_ENCOUNTER_INDEX));
    let mut handled = false;
    let mut should_return_to_idle = false;
    let mut wants_postcombat_guard = false;

    if encounter_index != NONE
        && call_simple(context, host, call::ENCOUNTER_STAND_DOWN, 0) != 0
        && combat_status() <= 2
        && m.i16(at(a::STATE_ARTIFICIAL_COMBAT_STATUS)) == 0
        && m.i16(at(a::STATE_SUSPICION_COMBAT_STATUS)) == 0
    {
        should_return_to_idle = true;
    }
    if m.i16(at(a::EXTERNAL_ORDERS_POSTCOMBAT_TYPE)) > 0
        && combat_status() <= 2
        && m.i16(at(a::STATE_SUSPICION_COMBAT_STATUS)) == 0
    {
        wants_postcombat_guard = true;
    }
    if m.i16(at(a::STATE_MODE)) < 3 && ACTION_CLASSES[action() as usize] == 0 {
        handled = true;
    }

    if !handled
        && !m.bool(at(a::INPUT_VEHICLE_PASSENGER))
        && !wants_postcombat_guard
        && !should_return_to_idle
        && combat_status() >= 2
    {
        let orphan = if target_prop_index() == NONE { None } else { Some(prop_get(m, context, target_prop_index())) };
        let mut pursue_tenacious = false;
        let mut options = PursuitOptions::default();
        let swarm = m.bool(at(a::META_SWARM));

        if orphan.is_none_or(|orphan| !m.bool(orphan + p::ABANDONED_SEARCH as u32)) {
            let (mut desire_target_search, mut desire_pursuit, mut desire_pursuit_search, controller) =
                pursuit_settings(m.i16(at(a::META_TYPE)));
            let mut group_pursuit_controller = controller != 0;
            let mut group_pursuit_restriction: i16 = 0;
            let mut controlling_group_pursuit = false;

            if orphan.is_some() {
                options.allow_indefinite_target_uncover = true;
                options.allow_target_uncover = true;
            }
            options.allow_pursuit = true;
            options.allow_target_search = true;
            options.allow_pursuit_search = true;

            if encounter_index != NONE {
                let desires = call_full(
                    context,
                    host,
                    call::PURSUIT_DESIRES,
                    0,
                    0,
                    &[
                        pursue_tenacious as i32 as f32,
                        group_pursuit_restriction as f32,
                        group_pursuit_controller as i32 as f32,
                        desire_target_search as f32,
                        desire_pursuit as f32,
                        desire_pursuit_search as f32,
                    ],
                )
                .output;
                pursue_tenacious = desires[0] != 0.0;
                group_pursuit_restriction = desires[1] as i16;
                group_pursuit_controller = desires[2] != 0.0;
                desire_target_search = desires[3] as i16;
                desire_pursuit = desires[4] as i16;
                desire_pursuit_search = desires[5] as i16;

                if swarm {
                    options.allow_target_search = true;
                    options.allow_indefinite_target_uncover = true;
                    options.allow_target_uncover = true;
                    options.allow_pursuit_search = true;
                    options.allow_pursuit = true;
                } else {
                    let flags = [
                        options.allow_target_uncover,
                        options.allow_indefinite_target_uncover,
                        options.allow_target_search,
                        options.allow_pursuit,
                        options.allow_pursuit_search,
                        controlling_group_pursuit,
                        false,
                        options.wait_after_pursuit,
                    ]
                    .map(|flag| flag as i32 as f32);
                    let result = call_full(
                        context,
                        host,
                        call::PURSUIT_AVAILABILITY,
                        group_pursuit_restriction as i32,
                        group_pursuit_controller as i32,
                        &flags,
                    )
                    .result;
                    let bit = |n: u32| result & (1 << n) != 0;
                    options.allow_target_uncover = bit(0);
                    options.allow_indefinite_target_uncover = bit(1);
                    options.allow_target_search = bit(2);
                    options.allow_pursuit = bit(3);
                    options.allow_pursuit_search = bit(4);
                    controlling_group_pursuit = bit(5);
                    options.wait_after_pursuit = bit(7);
                }
            }

            determine_pursuit_options(
                context,
                host,
                desire_target_search,
                desire_pursuit,
                desire_pursuit_search,
                controlling_group_pursuit,
                should_return_to_idle,
                m.bool(at(a::EMOTIONS_FORCED_TO_CHARGE)),
                &mut options,
            );
        }

        if m.i32(at(a::FIRING_POSITIONS_PURSUIT_PROP_INDEX)) != target_prop_index() {
            m.set_i16(at(a::FIRING_POSITIONS_PURSUIT_POSITIONS_COUNT), 0);
            m.w32(at(a::FIRING_POSITIONS_PURSUIT_PROP_INDEX), target_prop_index() as u32);
            m.set_bool(at(a::FIRING_POSITIONS_PURSUIT_FIRED_AT_ORPHAN), false);
            m.set_bool(at(a::FIRING_POSITIONS_PURSUIT_COMMUNICATED_LOST_CONTACT), false);
        }

        if options.allow_target_uncover {
            if let Some(data) = uncover_setup_target(context, host, options.allow_target_search) {
                action_change(context, host, ACTION_UNCOVER, Some(&data));
                handled = true;
            }
        }

        if !handled {
            call_simple(context, host, call::TRIED_TO_UNCOVER, target_prop_index());
            if options.allow_target_search {
                if let Some(data) = search_setup_target(context, host, m.bool(at(a::EMOTIONS_FORCED_TO_CHARGE))) {
                    action_change(context, host, ACTION_SEARCH, Some(&data));
                    handled = true;
                }
            }

            if !handled {
                call_simple(context, host, call::TRIED_TO_SEARCH, target_prop_index());
                if m.bool(at(a::FIRING_POSITIONS_PURSUIT_FIRED_AT_ORPHAN))
                    && !m.bool(at(a::FIRING_POSITIONS_PURSUIT_COMMUNICATED_LOST_CONTACT))
                {
                    let target_unit_index = call_simple(context, host, call::TARGET_UNIT, 0);
                    communicate(13, target_unit_index);
                    m.set_bool(at(a::FIRING_POSITIONS_PURSUIT_COMMUNICATED_LOST_CONTACT), true);
                }

                if options.allow_pursuit {
                    let mut firing_position_index = NONE as i16;
                    let mut pursuit_from_uncover = false;
                    m.set_bool(at(a::STATE_SEARCHING), true);

                    if swarm {
                        if options.allow_pursuit_search {
                            if let Some(data) = search_setup_undirected(context, host) {
                                action_change(context, host, ACTION_SEARCH, Some(&data));
                                handled = true;
                            }
                        }
                    } else {
                        if action() == ACTION_UNCOVER
                            && options.allow_pursuit_search
                            && m.i16(at(a::STATE_UNCOVER_PURSUIT_LOCATION_TYPE)) == v::_PURSUIT_LOCATION_POSITION as i16
                        {
                            firing_position_index = m.i16(at(a::STATE_UNCOVER_PURSUIT_LOCATION_FIRING_POSITION_INDEX));
                            pursuit_from_uncover = true;
                        }

                        if firing_position_index == NONE as i16 {
                            let definition = |offset: usize| m.i16(context.definition + offset as u32);
                            let examined_threshold = if m.i32(at(a::EXTERNAL_ORDERS_PURSUIT_GROUP_PROP_INDEX)) == NONE {
                                definition(d::PURSUIT_UNCOORDINATED_PURSUIT_POSITIONS)
                            } else {
                                definition(d::PURSUIT_COORDINATED_PURSUIT_POSITIONS)
                            };
                            if pursue_tenacious
                                || m.i32(at(a::FIRING_POSITIONS_PURSUIT_PROP_INDEX)) != target_prop_index()
                                || m.i16(at(a::FIRING_POSITIONS_PURSUIT_POSITIONS_COUNT)) < examined_threshold
                            {
                                let last_perceived_time =
                                    orphan.map_or(NONE, |orphan| m.i32(orphan + p::LAST_PERCEIVED_TIME as u32));
                                firing_position_index = call_full(
                                    context,
                                    host,
                                    call::SELECT_PURSUIT_POSITION,
                                    pursue_tenacious as i32,
                                    last_perceived_time,
                                    &[],
                                )
                                .result as i16;
                            }
                        }

                        if firing_position_index != NONE as i16 {
                            let uncover =
                                if pursuit_from_uncover { None } else { uncover_setup_pursuit(context, host, firing_position_index) };
                            if let Some(data) = uncover {
                                action_change(context, host, ACTION_UNCOVER, Some(&data));
                                handled = true;
                            } else if options.allow_pursuit_search {
                                if let Some(data) = search_setup_pursuit(context, host, firing_position_index, pursue_tenacious) {
                                    action_change(context, host, ACTION_SEARCH, Some(&data));
                                    handled = true;
                                }
                            }
                        }
                    }

                    if handled {
                        let last_perceived_time =
                            orphan.map_or(NONE, |orphan| m.i32(orphan + p::LAST_PERCEIVED_TIME as u32));
                        if m.i32(at(a::META_ENCOUNTER_INDEX)) != NONE
                            && call_full(
                                context,
                                host,
                                call::MARK_EXAMINED,
                                firing_position_index as i32,
                                last_perceived_time,
                                &[],
                            )
                            .result
                                != 0
                        {
                            let count = at(a::FIRING_POSITIONS_PURSUIT_POSITIONS_COUNT);
                            if m.i16(count) == 0 {
                                communicate(16, NONE);
                            }
                            m.set_i16(count, m.i16(count).wrapping_add(1));
                        }
                    }
                }

                if !handled {
                    if m.i16(at(a::FIRING_POSITIONS_PURSUIT_POSITIONS_COUNT)) > 0 && m.i32(at(a::META_UNIT_INDEX)) != NONE {
                        communicate(19, NONE);
                    }
                    if !swarm && options.wait_after_pursuit {
                        if let Some(data) = wait_setup(context, host, options.allow_pursuit) {
                            action_change(context, host, ACTION_WAIT, Some(&data));
                            handled = true;
                        }
                    }
                }
            }
        }
    }

    if !handled && wants_postcombat_guard {
        if action() == ACTION_GUARD && m.bool(at(a::STATE_GUARD_POST_COMBAT)) {
            handled = true;
        } else if let Some(data) = guard_setup_postcombat(context, host) {
            action_change(context, host, ACTION_GUARD, Some(&data));
            handled = true;
        }
    }

    if !handled && should_return_to_idle {
        handled = set_default_state(context, host, NONE as i16);
    }

    if !handled && ACTION_CLASSES[action() as usize] != 1 {
        let find_position_delay =
            if (action() != ACTION_SEARCH || m.bool(at(a::STATE_SEARCH_SEARCH_FAILED))) && action() != ACTION_WAIT {
                90
            } else {
                0
            };
        call_simple(context, host, call::ABANDONED_SEARCH, target_prop_index());
        let data = guard_setup_find_position(context, host, find_position_delay);
        action_change(context, host, ACTION_GUARD, Some(&data));
        handled = true;
    }

    handled
}

/// The actor's debug record's cover-seeking fields (ai_debug.h's
/// `actor_debug_info` `field_B8` to `field_C0`).
const DEBUG_COVER_SEEKING: u32 = 0xB8;
const DEBUG_COVER_SEEKING_STAGE: u32 = 0xBA;
const DEBUG_COVER_SEEKING_HIDDEN_TICKS: u32 = 0xBC;
const DEBUG_COVER_SEEKING_SHIELD: u32 = 0xC0;

/// actions.c's grenade stimuli (`_actor_grenade_stimulus_*`): a visible
/// target, and seeking cover.
const GRENADE_STIMULUS_VISIBLE_TARGET: i16 = 1;
const GRENADE_STIMULUS_SEEK_COVER: i16 = 2;

/// `actor_action_handle_active_cover_seeking`: on its timeslice, a pursuing
/// or fighting actor (not berserk) whose shield is down to its fraction to
/// hide seeks cover (at most once a second), or (when allowed) flees to
/// defensive cover. Its debug record notes how far it got.
pub fn handle_active_cover_seeking(
    context: &AiStepContext,
    host: &impl AiHost,
    allow_defensive_cover: bool,
    force_flee_transition: bool,
) -> bool {
    let m = host.memory();
    let at = |offset: usize| context.actor + offset as u32;
    let debug = |offset: u32| context.debug_info + offset;
    let stage = |value: i16| m.set_i16(debug(DEBUG_COVER_SEEKING_STAGE), value);

    if !m.bool(at(a::META_TIMESLICE)) {
        return false;
    }
    m.set_bool(debug(DEBUG_COVER_SEEKING), true);
    stage(4);
    let last_visible_time = m.i32(at(a::TARGET_TARGET_LAST_VISIBLE_TIME));
    let target_hidden_ticks =
        if last_visible_time == NONE { 1000 } else { context.game_time.wrapping_sub(last_visible_time) };
    m.set_i16(debug(DEBUG_COVER_SEEKING_HIDDEN_TICKS), target_hidden_ticks as i16);
    let shield_vitality = m.f32(at(a::INPUT_SHIELD_VITALITY));
    m.set_f32(debug(DEBUG_COVER_SEEKING_SHIELD), shield_vitality);

    if shield_vitality > m.f32(context.definition + d::DEFENSIVE_SHIELD_FRACTION_HIDE as u32) {
        return false;
    }
    let action_class = ACTION_CLASSES[m.i16(at(a::STATE_ACTION)) as usize];
    stage(0);
    if m.bool(at(a::EMOTIONS_BERSERK)) || !(action_class == 4 || action_class == 3) {
        return false;
    }
    stage(1);
    if m.i16(at(a::STATE_COMBAT_STATUS)) < 2 {
        return false;
    }
    let current_time = context.game_time;
    stage(2);
    let last_seeking_time = m.i32(at(a::EMOTIONS_LAST_ACTIVE_COVER_SEEKING_TIME));
    if !(last_seeking_time == NONE || current_time >= last_seeking_time.wrapping_add(30)) {
        return false;
    }
    stage(3);
    m.w32(at(a::EMOTIONS_LAST_ACTIVE_COVER_SEEKING_TIME), current_time as u32);
    if !allow_cover_seeking(context, host, false) {
        return false;
    }
    stage(5);
    if try_to_seek_cover(context, host, true, false) {
        stage(6);
        return true;
    }
    if allow_defensive_cover
        && try_to_panic(context, host, 4, m.i32(at(a::TARGET_TARGET_PROP_INDEX)), force_flee_transition)
    {
        stage(7);
        return true;
    }
    false
}

/// `actor_action_handle_vehicle_exit`: an actor gets out of a vehicle an
/// enemy player's in (or one it's evicted from, or as a passenger with a
/// grenade stuck to it or under it), and ignores the vehicle for six
/// seconds.
pub fn handle_vehicle_exit(context: &AiStepContext, host: &impl AiHost) -> bool {
    let m = host.memory();
    let at = |offset: usize| context.actor + offset as u32;
    let mut exited = false;
    let vehicle_index = m.i32(at(a::INPUT_VEHICLE_INDEX));
    if vehicle_index != NONE {
        let mut want_exit = false;
        let mut forced_exit = false;
        let mut iterator = PropIterator::new(m, context);
        while let Some(prop) = iterator.next(m, context) {
            let state = m.i16(prop + p::STATE as u32);
            if state >= v::_PROP_STATE_BECOMING_UNACKNOWLEDGED as i16
                && state <= v::_PROP_STATE_ACKNOWLEDGED as i16
                && m.bool(prop + p::PLAYER as u32)
                && m.bool(prop + p::ENEMY as u32)
                && m.i32(prop + p::VEHICLE_INDEX as u32) == vehicle_index
            {
                want_exit = true;
                forced_exit = true;
                break;
            }
        }
        if m.bool(at(a::STIMULI_VEHICLE_EVICTION)) {
            want_exit = true;
        }
        if m.bool(at(a::INPUT_VEHICLE_PASSENGER))
            && (m.i32(at(a::INPUT_DELAYED_ATTACHED_PROJECTILE_INDEX)) != NONE
                || (m.i16(at(a::DANGER_ZONE_DANGER_TYPE)) == v::_ACTOR_DANGER_ZONE_PROJECTILE as i16
                    && m.bool(at(a::DANGER_ZONE_ATTACHED_TO_US))))
        {
            want_exit = true;
            forced_exit = true;
        }
        if want_exit {
            m.set_bool(at(a::EMOTIONS_LAST_VEHICLE_EXIT_FORCED), forced_exit);
            if call_simple(context, host, call::EXIT_SEAT, 0) != 0 {
                m.w32(at(a::EMOTIONS_VEHICLE_IGNORE_INDEX), m.r32(at(a::INPUT_VEHICLE_INDEX)));
                m.w32(at(a::EMOTIONS_VEHICLE_IGNORE_TIME), context.game_time.wrapping_add(180) as u32);
                exited = true;
            }
            m.set_bool(at(a::EMOTIONS_LAST_VEHICLE_EXIT_FORCED), false);
        }
    }
    m.set_bool(at(a::STIMULI_VEHICLE_EVICTION), false);
    exited
}

/// `actor_action_handle_evasion`: an actor retreating from an unopposable
/// enemy it can see seeks cover or panics (at most once a second); one in
/// more danger than it stands (less when defending, at most 1.1 when
/// playfighting with cover to seek) throws a grenade, seeks cover by chance
/// (shouting so), or evades.
pub fn handle_evasion(context: &AiStepContext, host: &impl AiHost) -> bool {
    let m = host.memory();
    let at = |offset: usize| context.actor + offset as u32;
    let definition = |offset: usize| m.f32(context.definition + offset as u32);
    let definition_flag = |bit: usize| m.r32(context.definition + d::FLAGS as u32) & (1 << bit) != 0;
    let current_time = context.game_time;
    let last_cover_seeking_elapsed = || {
        let time = m.i32(at(a::EMOTIONS_LAST_DEFENSIVE_COVER_SEEKING_TIME));
        time == NONE || time.wrapping_add(30) <= current_time
    };
    let mut result = false;

    if m.i16(at(a::EMOTIONS_UNOPPOSABLE_RETREAT_TIMER)) > 0 && m.i32(at(a::INPUT_VEHICLE_INDEX)) == NONE {
        let retreat_prop_index = m.i32(at(a::EMOTIONS_UNOPPOSABLE_RETREAT_PROP_INDEX));
        let prop = prop_get(m, context, retreat_prop_index);
        let line_of_sight = m.i16(prop + p::LINE_OF_SIGHT as u32);
        if m.bool(prop + p::UNOPPOSABLE_ENEMY as u32)
            && (line_of_sight == v::_AI_LINE_OF_SIGHT_CLEAR as i16 || line_of_sight == v::_AI_LINE_OF_SIGHT_OCCLUDED as i16)
            && last_cover_seeking_elapsed()
        {
            m.w32(at(a::EMOTIONS_LAST_DEFENSIVE_COVER_SEEKING_TIME), current_time as u32);
            if allow_cover_seeking(context, host, true) {
                if try_to_seek_cover(context, host, false, true) {
                    result = true;
                } else if definition_flag(v::_ACTOR_DEFINITION_PANICKED_BY_UNOPPOSABLE_ENEMY_BIT)
                    && try_to_panic(context, host, 5, m.i32(at(a::EMOTIONS_UNOPPOSABLE_RETREAT_PROP_INDEX)), false)
                {
                    result = true;
                }
            }
        }
    }

    if !result {
        let mut dive_threshold = if !m.bool(at(a::EMOTIONS_CURRENTLY_DEFENDING)) || m.bool(at(a::EMOTIONS_BERSERK)) {
            definition(d::DEFENSIVE_EVASION_DANGER_THRESHOLD_ATTACKING)
        } else {
            definition(d::DEFENSIVE_EVASION_DANGER_THRESHOLD_DEFENDING)
        };
        if m.bool(at(a::EXTERNAL_ORDERS_PLAYFIGHTING))
            && definition(d::DEFENSIVE_EVASION_SEEK_COVER_CHANCE) > 0.0
            && dive_threshold > 1.1
        {
            dive_threshold = 1.1;
        }

        if m.f32(at(a::EMOTIONS_PERCEIVED_DANGER)) > dive_threshold {
            if !m.bool(at(a::CONTROL_MOVING)) {
                call_full(context, host, call::RANDOM, 0, 0, &[]);
            }
            if m.i16(context.variant_definition + var::GRENADE_COMBAT_STIMULUS_TYPE as u32) == GRENADE_STIMULUS_SEEK_COVER
                && consider_grenade(context, host)
            {
                m.set_f32(at(a::EMOTIONS_PERCEIVED_DANGER), 0.0);
                result = true;
            }

            let mut prop_dangerous = true;
            let mut not_fleeing = true;
            if m.bool(at(a::EMOTIONS_DEFENSIVE_CROUCH)) && definition_flag(v::_ACTOR_DEFINITION_CROUCH_TRY_NOT_TO_MOVE_BIT) {
                prop_dangerous = false;
                let target_prop_index = m.i32(at(a::TARGET_TARGET_PROP_INDEX));
                if target_prop_index != NONE {
                    let prop = prop_get(m, context, target_prop_index);
                    if m.i8(prop + p::QUANTIZED_FACING as u32) <= 2 && m.i8(prop + p::QUANTIZED_DISTANCE as u32) <= 1 {
                        prop_dangerous = true;
                    }
                }
            }
            let goal = m.i16(at(a::STATE_CHARGE_GOAL));
            if m.i16(at(a::STATE_ACTION)) == ACTION_CHARGE
                && (goal == v::_CHARGE_GOAL_MELEE as i16 || goal == v::_CHARGE_GOAL_MELEE_LEAPING as i16)
            {
                not_fleeing = false;
            }

            if !result && prop_dangerous && last_cover_seeking_elapsed() {
                m.w32(at(a::EMOTIONS_LAST_DEFENSIVE_COVER_SEEKING_TIME), current_time as u32);
                if allow_cover_seeking(context, host, false)
                    && call_full(context, host, call::RANDOM, 0, 0, &[]).output[0]
                        < definition(d::DEFENSIVE_EVASION_SEEK_COVER_CHANCE)
                    && try_to_seek_cover(context, host, false, true)
                {
                    let target_unit_index = call_simple(context, host, call::TARGET_UNIT, 0);
                    call_full(
                        context,
                        host,
                        call::COMMUNICATION,
                        v::_AI_COMMUNICATION_COVER as i32,
                        target_unit_index,
                        &[NONE as f32],
                    );
                    m.set_f32(at(a::EMOTIONS_PERCEIVED_DANGER), 0.0);
                    result = true;
                }
            }

            if !result
                && not_fleeing
                && m.i16(at(a::EMOTIONS_EVASION_DELAY_TIMER)) == 0
                && try_to_evade(context, host)
            {
                m.set_f32(at(a::EMOTIONS_PERCEIVED_DANGER), 0.0);
                m.set_i16(at(a::EMOTIONS_EVASION_DELAY_TIMER), c_short(definition(d::DEFENSIVE_EVASION_DELAY_TIMER) * 30.0));
                m.set_bool(at(a::FIRING_POSITIONS_MOVED_AWAY_FROM_FIRING_POSITION), true);
                result = true;
            }
        }
    }
    result
}

/// real_math.h's `vector_from_points3d(a, b)`.
fn vector_from_points(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [b[0] - a[0], b[1] - a[1], b[2] - a[2]]
}

/// real_math.h's `magnitude_squared3d`.
fn magnitude_squared(v: [f32; 3]) -> f32 {
    v[0] * v[0] + v[1] * v[1] + v[2] * v[2]
}

/// real_math.c's `magnitude_squared3d_right_associated`.
fn magnitude_squared_right_associated(v: [f32; 3]) -> f32 {
    v[0] * v[0] + (v[1] * v[1] + v[2] * v[2])
}

/// real_math.h's `cross_product3d`.
fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    let k = a[0] * b[1] - a[1] * b[0];
    let j = a[2] * b[0] - a[0] * b[2];
    let i = a[1] * b[2] - a[2] * b[1];
    [i, j, k]
}

/// real_math.h's `point_from_line3d(p, v, t)`.
fn point_from_line(p: [f32; 3], v: [f32; 3], t: f32) -> [f32; 3] {
    [v[0] * t + p[0], v[1] * t + p[1], v[2] * t + p[2]]
}

/// cseries.h's `PIN(n, 0, 1)`.
fn pin_unit(n: f32) -> f32 {
    if n < 0.0 {
        0.0
    } else if n > 1.0 {
        1.0
    } else {
        n
    }
}

const REAL_EPSILON: f32 = 0.0001;

/// real_math.c's `point_to_line_distance_squared3d`: from the point to the
/// segment from `base` along `height`.
fn point_to_line_distance_squared(point: [f32; 3], base: [f32; 3], height: [f32; 3]) -> f32 {
    let v = vector_from_points(base, point);
    let t = pin_unit(dot(v, height) / magnitude_squared(height));
    magnitude_squared(point_from_line(v, height, -t))
}

/// real_math.c's `vector_to_line_distance_squared3d`: between the segment
/// from `point` along `vector` and the one from `line_point` along
/// `line_vector`.
fn vector_to_line_distance_squared(point: [f32; 3], vector: [f32; 3], line_point: [f32; 3], line_vector: [f32; 3]) -> f32 {
    let offset = vector_from_points(point, line_point);
    let mut cross_vector = cross(vector, line_vector);
    let denominator = magnitude_squared_right_associated(cross_vector);
    let t0;
    let t1;
    if !((denominator as f64).abs() < REAL_EPSILON as f64) {
        let inverse_denominator = 1.0 / denominator;
        cross_vector = [
            inverse_denominator * cross_vector[0],
            inverse_denominator * cross_vector[1],
            inverse_denominator * cross_vector[2],
        ];
        t0 = dot(cross(offset, line_vector), cross_vector);
        t1 = dot(cross(offset, vector), cross_vector);
        let t0_out_of_range = t0 < 0.0 || t0 > 1.0;
        let t1_out_of_range = t1 < 0.0 || t1 > 1.0;
        if t0_out_of_range || t1_out_of_range {
            let mut d0 = f32::MAX;
            let mut d1 = f32::MAX;
            if t0_out_of_range {
                let closest_point0 = point_from_line(point, vector, if t0 < 0.0 { 0.0 } else { 1.0 });
                d0 = point_to_line_distance_squared(closest_point0, line_point, line_vector);
            }
            if t1_out_of_range {
                let closest_point1 = point_from_line(line_point, line_vector, if t1 < 0.0 { 0.0 } else { 1.0 });
                d1 = point_to_line_distance_squared(closest_point1, point, vector);
            }
            return if d0 > d1 { d1 } else { d0 };
        }
    } else {
        let dot_vectors = dot(vector, line_vector);
        let vector_magnitude_squared = magnitude_squared(vector);
        t0 = if vector_magnitude_squared > REAL_EPSILON {
            let inverse = 1.0 / vector_magnitude_squared;
            let mut start = offset[2] * vector[2];
            start += offset[1] * vector[1];
            start += offset[0] * vector[0];
            start *= inverse;
            let end = inverse * dot_vectors + start;
            (pin_unit(start) + pin_unit(end)) * 0.5
        } else {
            0.0
        };
        let line_magnitude_squared = magnitude_squared(line_vector);
        t1 = if line_magnitude_squared > REAL_EPSILON {
            let inverse = 1.0 / line_magnitude_squared;
            let start = -dot(offset, line_vector) * inverse;
            let end = inverse * dot_vectors + start;
            (pin_unit(start) + pin_unit(end)) * 0.5
        } else {
            0.0
        };
    }
    let closest_point0 = point_from_line(point, vector, t0);
    let closest_point1 = point_from_line(line_point, line_vector, t1);
    magnitude_squared(vector_from_points(closest_point0, closest_point1))
}

/// real_math.c's `fast_vector_intersection_with_sphere`: the fraction of the
/// vector at which it enters the sphere (0 from inside, REAL_MAX if never).
fn fast_vector_intersection_with_sphere(point: [f32; 3], vector: [f32; 3], center: [f32; 3], radius: f32) -> f32 {
    let p = [point[0] - center[0], point[1] - center[1], point[2] - center[2]];
    let c = p[0] * p[0] + p[1] * p[1] + p[2] * p[2] - radius * radius;
    if c < 0.0 {
        return 0.0;
    }
    let b = vector[0] * p[0] + vector[1] * p[1] + vector[2] * p[2];
    if b >= 0.0 {
        return f32::MAX;
    }
    let a = vector[0] * vector[0] + vector[1] * vector[1] + vector[2] * vector[2];
    let discriminant = b * b - a * c;
    if discriminant <= 0.0 {
        return f32::MAX;
    }
    (-b - discriminant.sqrt()) / a
}

/// `actor_action_handle_danger_avoidance`: an actor in a danger it noticed
/// (not stuck to it, near enough, not busy animating or a passenger) gives
/// up a position or path into it, shouts of a grenade, and dives clear when
/// the danger allows and it's urgent enough; one near a danger that isn't
/// moving away from it avoids it. Its debug record notes the decision.
pub fn handle_danger_avoidance(context: &AiStepContext, host: &impl AiHost) -> bool {
    let m = host.memory();
    let at = |offset: usize| context.actor + offset as u32;
    let debug_at = |offset: usize| context.debug_info + offset as u32;
    let decision = |value: usize| m.set_i16(debug_at(debug::DANGER_DECISION), value as i16);
    let mut result = false;
    let mut within_escape_radius = false;
    let mut path_re_enters_danger = false;

    m.w32(debug_at(debug::DANGER_AVOIDANCE_TIME), context.game_time as u32);
    decision(0);
    m.set_i16(debug_at(debug::DIVE_DECISION), 0);
    m.set_bool(debug_at(debug::DANGER_ABANDONED_PATH), false);

    'decide: {
        let danger_type = m.i16(at(a::DANGER_ZONE_DANGER_TYPE));
        if danger_type == 0 {
            decision(0);
            break 'decide;
        }
        if !m.bool(at(a::DANGER_ZONE_NOTICED_DANGER)) {
            decision(v::_DANGER_AVOIDANCE_UNNOTICED);
            break 'decide;
        }
        if m.bool(at(a::DANGER_ZONE_ATTACHED_TO_US)) {
            decision(v::_DANGER_AVOIDANCE_ATTACHED_TO_US);
            break 'decide;
        }

        let body_position = m.vector(at(a::INPUT_BODY_POSITION));
        {
            let reference = m.vector(at(a::DANGER_ZONE_BOUNDING_SPHERE_CENTER));
            let x = reference[0] - body_position[0];
            let y = reference[1] - body_position[1];
            let z = reference[2] - body_position[2];
            let radius = m.f32(at(a::DANGER_ZONE_BOUNDING_SPHERE_RADIUS)) + 3.0;
            if y * y + (z * z + x * x) > radius * radius {
                decision(v::_DANGER_AVOIDANCE_FAR_AWAY);
                let distance = magnitude_squared(vector_from_points(body_position, reference)).sqrt();
                m.set_f32(debug_at(debug::DANGER_FAR_DIST), distance);
                m.set_f32(debug_at(debug::DANGER_FAR_RADIUS), m.f32(at(a::DANGER_ZONE_BOUNDING_SPHERE_RADIUS)) + 3.0);
                break 'decide;
            }
        }

        if call_simple(context, host, call::ANIMATION_BUSY, 0) != 0 {
            decision(v::_DANGER_AVOIDANCE_ANIMATION_BUSY);
            break 'decide;
        }
        if m.bool(at(a::INPUT_VEHICLE_PASSENGER)) {
            decision(v::_DANGER_AVOIDANCE_VEHICLE);
            break 'decide;
        }

        let danger_position = m.vector(at(a::DANGER_ZONE_POSITION));
        let danger_line_delta = vector_from_points(danger_position, m.vector(at(a::DANGER_ZONE_PREDICT_DANGER_POSITION)));
        let danger_radius = || m.f32(at(a::DANGER_ZONE_DANGER_RADIUS));
        let body_line_distance_squared = point_to_line_distance_squared(body_position, danger_position, danger_line_delta);
        let body_within_danger = body_line_distance_squared < danger_radius() * danger_radius();
        within_escape_radius = body_line_distance_squared < (danger_radius() + 3.5) * (danger_radius() + 3.5);

        let destination_within_danger;
        if call_simple(context, host, call::HAS_PATH, 0) != 0 {
            let destination_line_distance_squared = point_to_line_distance_squared(
                m.vector(at(a::CONTROL_PATH_PATH_ENDPOINT_POINT)),
                danger_position,
                danger_line_delta,
            );
            destination_within_danger = destination_line_distance_squared < danger_radius() * danger_radius();
            if m.bool(at(a::CONTROL_MOVING)) && !body_within_danger && !destination_within_danger {
                let towards = m.vector(at(a::CONTROL_MOVING_TOWARDS_VECTOR));
                let path_direction_scaled = [towards[0] * 3.0, towards[1] * 3.0, towards[2] * 3.0];
                let path_line_distance_squared = vector_to_line_distance_squared(
                    body_position,
                    path_direction_scaled,
                    danger_position,
                    danger_line_delta,
                );
                path_re_enters_danger = path_line_distance_squared < danger_radius() * danger_radius();
            }
        } else {
            destination_within_danger = body_within_danger;
            path_re_enters_danger = false;
        }

        if destination_within_danger || (path_re_enters_danger && !body_within_danger) {
            let firing_position_index = m.i16(at(a::FIRING_POSITIONS_CURRENT_POSITION_INDEX));
            if firing_position_index != NONE as i16 {
                call_simple(context, host, call::DISCARD_FIRING_POSITION, firing_position_index as i32);
                m.set_bool(debug_at(debug::DANGER_ABANDONED_PATH), true);
            }
        }

        if !body_within_danger {
            decision(v::_DANGER_AVOIDANCE_OUTSIDE_ZONE);
            m.set_f32(debug_at(debug::DANGER_ZONE_DIST), body_line_distance_squared.sqrt());
            m.set_f32(debug_at(debug::DANGER_ZONE_RADIUS), danger_radius());
            break 'decide;
        }

        let mut reaction_is_urgent = false;
        let mut urgency = fast_vector_intersection_with_sphere(danger_position, danger_line_delta, body_position, danger_radius());
        if urgency < f32::MAX {
            urgency *= 45.0;
        }
        let danger_type = m.i16(at(a::DANGER_ZONE_DANGER_TYPE));
        if danger_type == v::_ACTOR_DANGER_ZONE_SUICIDE as i16 {
            let time_until_death = m.i16(at(a::DANGER_ZONE_SUICIDE_TIME_UNTIL_DEATH));
            if urgency == 0.0 && time_until_death != NONE as i16 && time_until_death < 30 {
                reaction_is_urgent = true;
            }
        } else if danger_type == v::_ACTOR_DANGER_ZONE_PROJECTILE as i16 {
            let time_until_explosion = m.i16(at(a::DANGER_ZONE_PROJECTILE_TIME_UNTIL_EXPLOSION));
            if urgency == 0.0 && time_until_explosion != NONE as i16 && time_until_explosion < 20 {
                reaction_is_urgent = true;
            }
        } else if danger_type == v::_ACTOR_DANGER_ZONE_VEHICLE as i16 && urgency < 30.0 {
            reaction_is_urgent = true;
        }

        let hostility = m.i16(at(a::DANGER_ZONE_HOSTILITY));
        if (hostility == 0 || reaction_is_urgent) && !m.bool(at(a::DANGER_ZONE_COMMUNICATED)) {
            let communication_hostility = if hostility == v::_ACTOR_DANGER_HOSTILITY_ENEMY as i16 {
                v::_COMM_HOSTILITY_ENEMY as i32
            } else if hostility == v::_ACTOR_DANGER_HOSTILITY_FRIEND as i16 {
                v::_COMM_HOSTILITY_FRIEND as i32
            } else if hostility == v::_ACTOR_DANGER_HOSTILITY_SELF as i16 {
                v::_COMM_HOSTILITY_SELF as i32
            } else {
                NONE
            };
            if m.i16(at(a::DANGER_ZONE_DANGER_TYPE)) == v::_ACTOR_DANGER_ZONE_PROJECTILE as i16 {
                call_full(
                    context,
                    host,
                    call::COMMUNICATION,
                    v::_AI_COMMUNICATION_GRENADE_DANGER as i32,
                    NONE,
                    &[communication_hostility as f32],
                );
            }
            m.set_bool(at(a::DANGER_ZONE_COMMUNICATED), true);
        }

        let (escape_found, escape_direction, escape_distance, alignment_vector, escape_is_ledge) =
            find_escape_from_danger(context, host);
        if escape_direction == NONE as i16 {
            decision(v::_DANGER_AVOIDANCE_NO_SAFE_DIRECTION);
            break 'decide;
        }

        if m.bool(at(a::DANGER_ZONE_ALLOW_DIVE_EVASION)) && m.i32(at(a::INPUT_VEHICLE_INDEX)) == NONE {
            let mut should_dive = false;
            let danger_type = m.i16(at(a::DANGER_ZONE_DANGER_TYPE));
            if danger_type == v::_ACTOR_DANGER_ZONE_SUICIDE as i16 {
                if (escape_found || escape_is_ledge) && urgency == 0.0 {
                    decision(v::_DANGER_AVOIDANCE_CAN_AVOID);
                    should_dive = true;
                } else if reaction_is_urgent {
                    decision(v::_DANGER_AVOIDANCE_IMMINENT_EXPLOSION);
                    should_dive = true;
                }
            } else if danger_type == v::_ACTOR_DANGER_ZONE_PROJECTILE as i16 {
                if (escape_found || escape_is_ledge) && urgency < 7.0 {
                    decision(v::_DANGER_AVOIDANCE_CAN_AVOID);
                    should_dive = true;
                } else if reaction_is_urgent {
                    decision(v::_DANGER_AVOIDANCE_IMMINENT_EXPLOSION);
                    should_dive = true;
                }
            } else if danger_type == v::_ACTOR_DANGER_ZONE_VEHICLE as i16 && reaction_is_urgent {
                decision(v::_DANGER_AVOIDANCE_IMMINENT_IMPACT);
                should_dive = true;
            }

            if should_dive {
                let flags = m.r32(context.definition + d::FLAGS as u32);
                let ledge_avoidance_distance =
                    if flags & (1 << v::_ACTOR_DEFINITION_DIVE_OFF_LEDGES_BIT) != 0 { 8.0 } else { 0.0 };
                result = try_to_dive(
                    context,
                    host,
                    escape_direction,
                    escape_distance,
                    alignment_vector,
                    ledge_avoidance_distance,
                );
            } else {
                decision(v::_DANGER_AVOIDANCE_NO_DESIRE);
                m.set_f32(debug_at(debug::DANGER_INTERSECT_TIME), urgency);
            }
        } else {
            decision(v::_DANGER_AVOIDANCE_EVASION_DISALLOWED);
        }
    }

    if !result
        && within_escape_radius
        && (call_simple(context, host, call::AT_DESTINATION, 0) != 0 || !m.bool(at(a::CONTROL_MOVING)) || path_re_enters_danger)
        && !m.bool(at(a::EMOTIONS_FORCED_TO_CHARGE))
    {
        let action_class = ACTION_CLASSES[m.i16(at(a::STATE_ACTION)) as usize];
        if action_class == 1 || action_class == 3 {
            action_change(context, host, ACTION_AVOID, Some(&ActionData::new()));
            result = true;
        }
    }
    result
}

/// `actor_action_handle_grenade_throwing`: an actor with a target (not
/// fleeing in a panic) considers a grenade by its variant's stimulus (a
/// visible target in combat, or a swarm or a calm flee), and keeps trying to
/// throw one it's started.
pub fn handle_grenade_throwing(context: &AiStepContext, host: &impl AiHost) -> bool {
    let m = host.memory();
    let at = |offset: usize| context.actor + offset as u32;
    let fleeing_in_panic = || m.i16(at(a::STATE_ACTION)) == ACTION_FLEE && m.i16(at(a::STATE_FLEE_PANIC_TYPE)) > 0;
    let mut result = false;
    if m.i16(at(a::TARGET_TARGET_TYPE)) >= 5 && !fleeing_in_panic() {
        let prop = prop_get(m, context, m.i32(at(a::TARGET_TARGET_PROP_INDEX)));
        match m.i16(context.variant_definition + var::GRENADE_COMBAT_STIMULUS_TYPE as u32) {
            GRENADE_STIMULUS_VISIBLE_TARGET => {
                if m.i16(at(a::STATE_COMBAT_STATUS)) >= 5 {
                    result = consider_grenade(context, host);
                }
            }
            GRENADE_STIMULUS_SEEK_COVER => {
                if m.bool(prop + p::SWARM as u32)
                    || (m.i16(at(a::STATE_ACTION)) == ACTION_FLEE && m.i16(at(a::STATE_FLEE_PANIC_TYPE)) == PANIC_NONE)
                {
                    result = consider_grenade(context, host);
                }
            }
            _ => {}
        }
        if m.bool(at(a::CONTROL_GRENADE_TRYING_TO_THROW)) {
            try_to_throw_grenade(context, host, false);
        }
    } else {
        m.set_bool(at(a::CONTROL_GRENADE_TRYING_TO_THROW), false);
    }
    result
}

/// `actor_action_consider_grenade`: an actor already throwing goes on; one
/// with grenades (when the AI may throw them) checks no more often than its
/// variant's delay, and by its chance (scaled by the difficulty for its
/// team) and the throw's test starts throwing one.
pub fn consider_grenade(context: &AiStepContext, host: &impl AiHost) -> bool {
    let m = host.memory();
    let at = |offset: usize| context.actor + offset as u32;
    let variant = |offset: usize| context.variant_definition + offset as u32;
    if m.bool(at(a::CONTROL_GRENADE_TRYING_TO_THROW)) {
        return true;
    }
    let mut result = false;
    if call_simple(context, host, call::GRENADES_ENABLED, 0) != 0
        && m.i16(variant(var::GRENADE_COMBAT_GRENADE_TYPE)) != NONE as i16
        && m.i16(variant(var::GRENADE_COMBAT_TRAJECTORY_TYPE)) != NONE as i16
    {
        let current_time = context.game_time;
        let last_check_time = m.i32(at(a::CONTROL_GRENADE_LAST_CHECK_TIME));
        if last_check_time == NONE
            || current_time as f32
                >= last_check_time as f32 + m.f32(variant(var::GRENADE_COMBAT_THROW_GRENADE_DELAY)) * 30.0
        {
            let mut chance = m.f32(variant(var::GRENADE_COMBAT_THROW_GRENADE_CHANCE));
            chance *= call_full(context, host, call::GRENADE_CHANCE_SCALE, 0, 0, &[]).output[0];
            m.w32(at(a::CONTROL_GRENADE_LAST_CHECK_TIME), current_time as u32);
            if call_full(context, host, call::RANDOM, 0, 0, &[]).output[0] < chance
                && test_grenade(context, host)
            {
                m.set_bool(at(a::CONTROL_GRENADE_TRYING_TO_THROW), true);
                try_to_throw_grenade(context, host, true);
                result = true;
            }
        }
    }
    result
}

/// actions.c's AI index scopes (`_ai_index_*`).
const AI_INDEX_PLATOON: u32 = 1;
const AI_INDEX_SQUAD: u32 = 2;

/// `actor_action_handle_vehicle_entry`: at most every second and a half, an
/// actor (not obeying or fleeing in a panic) that helps players gets into
/// the vehicle of a friendly player driving within ten units; else one in
/// combat for two seconds takes the nearest enterable vehicle (the AI's
/// list: within its radius, and for its team, type and encounter, platoon
/// or squad).
pub fn handle_vehicle_entry(context: &AiStepContext, host: &impl AiHost) -> bool {
    let m = host.memory();
    let at = |offset: usize| context.actor + offset as u32;
    let current_time = context.game_time;
    let action = m.i16(at(a::STATE_ACTION));
    if action == ACTION_FLEE && m.i16(at(a::STATE_FLEE_PANIC_TYPE)) > PANIC_NONE {
        return false;
    }
    if action == ACTION_OBEY {
        return false;
    }
    let last_check_time = m.i32(at(a::EMOTIONS_LAST_VEHICLE_CHECK_TIME));
    if last_check_time != NONE && last_check_time.wrapping_add(45) >= current_time {
        return false;
    }
    m.w32(at(a::EMOTIONS_LAST_VEHICLE_CHECK_TIME), current_time as u32);

    let mut best_distance_squared = f32::MAX;
    let mut attempt_distance = f32::MAX;
    let mut continue_distance = f32::MAX;
    let mut best_vehicle_index = NONE;
    let allowed = |vehicle_index: i32| allowed_to_enter_vehicle(context, host, vehicle_index);
    let origin = |object_index: i32| call_full(context, host, call::OBJECT_ORIGIN, object_index, 0, &[]).vector();

    if m.r32(context.definition + d::FLAGS as u32) & (1 << v::_ACTOR_DEFINITION_HELPS_PLAYERS_IN_VEHICLES_BIT) != 0 {
        let mut iterator = PropIterator::new(m, context);
        while let Some(prop) = iterator.next(m, context) {
            let state = m.i16(prop + p::STATE as u32);
            let vehicle_index = m.i32(prop + p::VEHICLE_INDEX as u32);
            if state >= v::_PROP_STATE_BECOMING_UNACKNOWLEDGED as i16
                && state <= v::_PROP_STATE_ACKNOWLEDGED as i16
                && m.bool(prop + p::PLAYER as u32)
                && !m.bool(prop + p::ENEMY as u32)
                && vehicle_index != NONE
                && allowed(vehicle_index)
            {
                let driver = call_full(context, host, call::VEHICLE_DRIVER, vehicle_index, 0, &[]);
                if driver.result != 0 && driver.result2 == m.i32(prop + p::UNIT_INDEX as u32) {
                    let vehicle_origin = origin(vehicle_index);
                    let distance_squared =
                        magnitude_squared(vector_from_points(m.vector(at(a::INPUT_BODY_POSITION)), vehicle_origin));
                    if distance_squared < 100.0 && distance_squared < best_distance_squared {
                        let distance = m.f32(prop + p::DISTANCE as u32);
                        best_distance_squared = distance * distance;
                        best_vehicle_index = vehicle_index;
                        attempt_distance = 8.0;
                        continue_distance = 10.0;
                    }
                }
            }
        }
    }

    if best_vehicle_index == NONE {
        if m.i32(at(a::STATE_CERTAIN_COMBAT_TIMER)) < 60 {
            return false;
        }
        let vehicles = call_full(context, host, call::ENTERABLE_VEHICLES, 0, 0, &[]);
        let count = || m.i16(vehicles.result as u32);
        if 0 >= count() {
            return false;
        }
        let mut slot: i16 = 0;
        while slot < count() {
            let vehicle = vehicles.result2 as u32 + slot as u32 * enterable::SIZEOF as u32;
            slot = slot.wrapping_add(1);
            let field = |offset: usize| vehicle + offset as u32;
            let vehicle_index = m.i32(field(enterable::VEHICLE_INDEX));
            if call_simple(context, host, call::VEHICLE_EXISTS, vehicle_index) == 0 || !allowed(vehicle_index) {
                continue;
            }
            let position = origin(vehicle_index);
            let distance_squared = magnitude_squared(vector_from_points(position, m.vector(at(a::INPUT_BODY_POSITION))));
            let radius = m.f32(field(enterable::RADIUS));
            if !(distance_squared < best_distance_squared) || (radius != f32::MAX && distance_squared > radius * radius) {
                continue;
            }
            let team_bitmask = m.i16(field(enterable::TEAM_BITMASK));
            let team_index = m.i16(at(a::META_TEAM_INDEX));
            if team_bitmask > 0
                && (team_index == NONE as i16 || (team_bitmask as i32) & 1i32.wrapping_shl(team_index as u32) == 0)
            {
                continue;
            }
            let actor_type_bitmask = m.i16(field(enterable::ACTOR_TYPE_BITMASK));
            if actor_type_bitmask > 0
                && (actor_type_bitmask as i32) & 1i32.wrapping_shl(m.i16(at(a::META_TYPE)) as u32) == 0
            {
                continue;
            }
            let ai_indices_count = m.i16(field(enterable::AI_INDICES_COUNT));
            if ai_indices_count > 0 {
                let mut matched = false;
                let mut index: i16 = 0;
                while index < ai_indices_count {
                    let ai_index = m.i32(field(enterable::AI_INDICES) + index as u32 * 4);
                    index = index.wrapping_add(1);
                    if ai_index == NONE {
                        continue;
                    }
                    matched = (m.i32(at(a::META_ENCOUNTER_INDEX)) ^ ai_index) & 0xFFFF == 0;
                    if matched {
                        let sub_index = (ai_index as u32 >> 16) & 0xFF;
                        match ai_index as u32 >> 30 {
                            AI_INDEX_PLATOON => matched = m.i16(at(a::META_PLATOON_INDEX)) as i32 as u32 == sub_index,
                            AI_INDEX_SQUAD => matched = m.i16(at(a::META_SQUAD_INDEX)) as i32 as u32 == sub_index,
                            _ => {}
                        }
                    }
                    if matched {
                        break;
                    }
                }
                if !matched {
                    continue;
                }
            }
            best_vehicle_index = vehicle_index;
            best_distance_squared = distance_squared;
            attempt_distance = radius + 3.0;
            continue_distance = radius + 6.0;
        }
        if best_vehicle_index == NONE {
            return false;
        }
    }

    match vehicle_setup_impromptu(context, host, best_vehicle_index, attempt_distance, continue_distance) {
        Some(data) => {
            action_change(context, host, ACTION_VEHICLE, Some(&data));
            true
        }
        None => false,
    }
}

/// actions.c's evade directions (`_actor_evade_*`).
const EVADE_LEFT: i16 = 0;
const EVADE_RIGHT: i16 = 1;
const EVADE_FORWARD: i16 = 2;
const EVADE_BACK: i16 = 3;
const EVADE_RANDOM: i16 = 4;

/// actions.c's evade animations (`_actor_animation_evade_*`).
const ANIMATION_EVADE_LEFT: i32 = 6;
const ANIMATION_EVADE_RIGHT: i32 = 7;

/// actions.c's cosine an actor must face its grenade's target within
/// (`ACTOR_GRENADE_THROW_MINIMUM_COSINE`).
const GRENADE_THROW_MINIMUM_COSINE: f32 = 0.866_025_39;

/// Each default state's move position order (actions.c's
/// `global_state_move_position_orders`).
const STATE_MOVE_POSITION_ORDERS: [i32; 12] = [0, 0, 0, 1, 2, 3, 4, 5, 0, 0, 0, 0];

/// `actor_action_allow_cover_seeking`: not while berserk or a passenger, nor
/// (unless fleeing an unopposable enemy, or playfighting) while its target
/// was seen within the hiding time or is in view, or before enough damage.
pub fn allow_cover_seeking(context: &AiStepContext, host: &impl AiHost, unopposable: bool) -> bool {
    let m = host.memory();
    let at = |offset: usize| context.actor + offset as u32;
    let definition = |offset: usize| m.f32(context.definition + offset as u32);
    let mut allow = true;
    if !unopposable && !m.bool(at(a::EXTERNAL_ORDERS_PLAYFIGHTING)) {
        let hide_time = definition(d::DEFENSIVE_HIDE_TARGET_NOT_VISIBLE_TIME);
        if hide_time > 0.0 {
            if m.i16(at(a::STATE_COMBAT_STATUS)) >= 7 {
                allow = false;
            } else {
                let target_not_visible_ticks = c_short(hide_time * 30.0);
                let last_visible_time = m.i32(at(a::TARGET_TARGET_LAST_VISIBLE_TIME));
                if last_visible_time != NONE
                    && context.game_time < (target_not_visible_ticks as i32).wrapping_add(last_visible_time)
                {
                    allow = false;
                }
            }
        }
        let damage_threshold = definition(d::DEFENSIVE_COVER_DAMAGE_THRESHOLD);
        if damage_threshold > 0.0 && m.f32(at(a::INPUT_RECENT_BODY_DAMAGE)) < damage_threshold {
            allow = false;
        }
    }
    if m.bool(at(a::EMOTIONS_BERSERK)) {
        allow = false;
    }
    if m.bool(at(a::INPUT_VEHICLE_PASSENGER)) {
        allow = false;
    }
    allow
}

/// `actor_action_try_to_throw_grenade`: an actor whose unit can throw (and,
/// unless its trajectory's known, whose throw still tests) throws once it
/// faces the target closely enough, noting the throw in its encounter.
pub fn try_to_throw_grenade(context: &AiStepContext, host: &impl AiHost, known_trajectory: bool) -> bool {
    let m = host.memory();
    let at = |offset: usize| context.actor + offset as u32;
    let trying = at(a::CONTROL_GRENADE_TRYING_TO_THROW);
    if call_simple(context, host, call::UNIT_READY_TO_THROW, 0) == 0 {
        return false;
    }
    if !known_trajectory && !test_grenade(context, host) {
        m.set_bool(trying, false);
    }
    if !m.bool(trying) {
        return false;
    }
    let target = m.vector(at(a::CONTROL_GRENADE_CURRENT_TARGET));
    let body = m.vector(at(a::INPUT_BODY_POSITION));
    let mut to_target = [target[0] - body[0], target[1] - body[1]];
    let facing = m.vector(at(a::INPUT_FACING_VECTOR));
    if normalize2(&mut to_target) > 0.0
        && !(facing[1] * to_target[1] + to_target[0] * facing[0] < GRENADE_THROW_MINIMUM_COSINE)
    {
        m.set_bool(at(a::ORDERS_COMBAT_THROW_GRENADE), true);
        m.set_bool(trying, false);
        if m.i32(at(a::META_ENCOUNTER_INDEX)) != NONE {
            call_simple(context, host, call::MARK_GRENADE_THROWN, 0);
        }
        return true;
    }
    false
}

/// `actor_action_test_grenade`: whether an actor on foot may throw now (its
/// encounter's grenade timer, scaled by difficulty and doubled when
/// playfighting, has run out) at a target with enough enemies and no
/// friends near it, along a trajectory that works. Its debug record notes
/// the decision.
pub fn test_grenade(context: &AiStepContext, host: &impl AiHost) -> bool {
    let m = host.memory();
    let at = |offset: usize| context.actor + offset as u32;
    let variant = |offset: usize| context.variant_definition + offset as u32;
    let debug_at = |offset: usize| context.debug_info + offset as u32;
    let decision = |value: i16| m.set_i16(debug_at(debug::GRENADE_DECISION), value);
    let current_time = context.game_time;
    m.w32(debug_at(debug::GRENADE_EVAL_TIME), current_time as u32);

    if m.i32(at(a::INPUT_VEHICLE_INDEX)) != NONE {
        decision(0);
        return false;
    }

    if m.i32(at(a::META_ENCOUNTER_INDEX)) != NONE {
        let timer = call_full(context, host, call::ENCOUNTER_GRENADE_TIMER, 0, 0, &[]);
        let mut throw_delay = timer.output[0] * m.f32(variant(var::GRENADE_COMBAT_ENCOUNTER_GRENADE_TIMEOUT));
        if m.bool(at(a::EXTERNAL_ORDERS_PLAYFIGHTING)) {
            throw_delay *= 2.0;
        }
        let throw_delay_ticks = (throw_delay * 30.0) as i32;
        let last_throw_time = timer.result;
        if last_throw_time != NONE && (throw_delay_ticks as i16 as i32).wrapping_add(last_throw_time) > current_time {
            decision(5);
            let timeout = (throw_delay_ticks.wrapping_sub(current_time) as i16).wrapping_add(last_throw_time as i16);
            m.set_i16(debug_at(debug::GRENADE_ENCOUNTER_TIMEOUT_TICKS), timeout);
            return false;
        }
    }

    let target = call_full(context, host, call::FIND_GRENADE_TARGET, 0, 0, &[]);
    if target.result == 0 {
        decision(6);
        return false;
    }
    let grenade_target = [target.output[0], target.output[1], target.output[2]];
    let collateral = call_full(
        context,
        host,
        call::COLLATERAL_DAMAGE,
        0,
        0,
        &[
            m.f32(variant(var::GRENADE_COMBAT_ENEMY_RADIUS)),
            m.f32(variant(var::GRENADE_COMBAT_COLLATERAL_DAMAGE_RADIUS)),
            grenade_target[0],
            grenade_target[1],
            grenade_target[2],
        ],
    );
    if collateral.result == 0 {
        decision(8);
        return false;
    }
    let enemy_count = collateral.result2 as i16;
    let minimum_enemy_count = m.i16(variant(var::GRENADE_COMBAT_MINIMUM_ENEMY_COUNT));
    if enemy_count < minimum_enemy_count {
        m.set_i16(debug_at(debug::GRENADE_ENEMY_COUNT), enemy_count);
        decision(7);
        m.set_i16(debug_at(debug::GRENADE_REQUIRED_ENEMY_COUNT), minimum_enemy_count);
        return false;
    }
    if call_full(
        context,
        host,
        call::PLAN_GRENADE_TRAJECTORY,
        m.i16(variant(var::GRENADE_COMBAT_TRAJECTORY_TYPE)) as i32,
        target.result2,
        &[grenade_target[0], grenade_target[1], grenade_target[2], target.output[3]],
    )
    .result
        != 0
    {
        decision(10);
        return true;
    }
    decision(9);
    false
}

/// `actor_action_try_to_evade`: an actor on foot, standing, not animating,
/// with a target it faces (or one straight overhead), dodges left or right
/// of it as far as its unit evades, when the dodge has room and an
/// animation.
pub fn try_to_evade(context: &AiStepContext, host: &impl AiHost) -> bool {
    let m = host.memory();
    let at = |offset: usize| context.actor + offset as u32;
    if m.i32(at(a::INPUT_VEHICLE_INDEX)) != NONE
        || call_simple(context, host, call::ANIMATION_BUSY, 0) != 0
        || m.bool(at(a::CONTROL_MOVING))
        || m.i32(at(a::TARGET_TARGET_PROP_INDEX)) == NONE
    {
        return false;
    }
    let evade_distance = call_full(context, host, call::UNIT_EVADE_DISTANCE, 0, 0, &[]).output[0];
    if !(evade_distance > 0.0) {
        return false;
    }
    let prop = prop_get(m, context, m.i32(at(a::TARGET_TARGET_PROP_INDEX)));
    let actor_to_prop = m.vector(prop + p::ACTOR_TO_PROP as u32);
    let attractor = [actor_to_prop[0], actor_to_prop[1]];
    let facing = m.vector(at(a::INPUT_FACING_VECTOR));
    let flying = m.r32(context.definition + d::FLAGS as u32) & (1 << v::_ACTOR_DEFINITION_FLYING_BIT) != 0;
    let dot_check = if flying {
        Some(dot(actor_to_prop, facing))
    } else {
        let mut direction = attractor;
        if normalize2(&mut direction) > 0.0 { Some(direction[0] * facing[0] + direction[1] * facing[1]) } else { None }
    };
    if let Some(dot_facing) = dot_check {
        if !(dot_facing > 0.4) {
            return false;
        }
    }

    let mut alignment = attractor;
    normalize2(&mut alignment);
    let evasion = call_full(context, host, call::TRY_EVASION_DIRECTION, 0, 0, &[alignment[0], alignment[1], evade_distance]);
    if evasion.result == 0 {
        return false;
    }
    let alignment = [evasion.output[0], evasion.output[1]];
    let animation = if evasion.output[2] as i16 == EVADE_RIGHT { ANIMATION_EVADE_RIGHT } else { ANIMATION_EVADE_LEFT };
    call_simple(context, host, call::TEST_ANIMATION_IMPULSE, animation) != 0
        && call_full(context, host, call::ANIMATION_IMPULSE, animation, 0, &alignment).result != 0
}

/// `actor_action_set_default_state`: at most every second and a half (or
/// when told), the actor takes its initial state (once) or default state:
/// asleep, guarding where it is or finding a position, searching, fleeing,
/// or alert with its move position order; an actor with no action at all
/// is alert.
pub fn set_default_state(context: &AiStepContext, host: &impl AiHost, override_state: i16) -> bool {
    let m = host.memory();
    let at = |offset: usize| context.actor + offset as u32;
    let current_time = context.game_time;
    let action = || m.i16(at(a::STATE_ACTION));
    let guard_current_position = || {
        let data = guard_setup_current_position(context, host);
        action_change(context, host, ACTION_GUARD, Some(&data));
        true
    };
    let alert = |order: i32| {
        let data = alert_setup(context, host, order, NONE as i16);
        action_change(context, host, ACTION_ALERT, Some(&data));
        true
    };
    let mut changed = false;

    let last_time = m.i32(at(a::STATE_LAST_DEFAULT_STATE_TIME));
    if !(override_state != NONE as i16 || last_time == NONE || last_time.wrapping_add(45) < current_time) {
        return false;
    }
    m.w32(at(a::STATE_LAST_DEFAULT_STATE_TIME), current_time as u32);
    let initial_state = m.i16(at(a::STATE_INITIAL_STATE));
    let default_state = m.i16(at(a::STATE_DEFAULT_STATE));
    let state = if override_state != NONE as i16 {
        override_state
    } else if initial_state != NONE as i16 {
        m.set_i16(at(a::STATE_INITIAL_STATE), NONE as i16);
        initial_state
    } else if default_state == NONE as i16 {
        0
    } else {
        default_state
    };

    match state {
        1 => {
            if m.i16(at(a::STATE_MODE)) != 1 {
                m.set_i16(at(a::STATE_MODE), 1);
                action_change(context, host, ACTION_SLEEP, None);
                changed = true;
            }
        }
        8 => {
            if (action() != ACTION_GUARD || m.i16(at(a::STATE_GUARD_GUARD_LOCATION_TYPE)) != 1) && guard_current_position() {
                changed = true;
            }
        }
        9 => {
            if action() == ACTION_GUARD {
                if m.i16(at(a::STATE_GUARD_GUARD_LOCATION_TYPE)) != 3 {
                    m.set_bool(at(a::STATE_GUARD_FIND_NEW_GUARD_POSITION), true);
                }
            } else {
                let data = guard_setup_find_position(context, host, 0);
                action_change(context, host, ACTION_GUARD, Some(&data));
                changed = true;
            }
        }
        10 => {
            if ACTION_CLASSES[action() as usize] != 3 {
                m.set_i16(at(a::STATE_MODE), 3);
                m.set_i16(at(a::STATE_ARTIFICIAL_COMBAT_STATUS), 2);
                m.set_i16(at(a::STATE_COMBAT_STATUS), 2);
                if !handle_lost_contact(context, host) && guard_current_position() {
                    changed = true;
                }
            }
        }
        11 => {
            if action() != ACTION_FLEE {
                if try_to_panic(context, host, 13, NONE, true) {
                    changed = true;
                } else if action() != ACTION_GUARD && guard_current_position() {
                    changed = true;
                }
            }
        }
        0 | 2..=7 => {
            let order = STATE_MOVE_POSITION_ORDERS[state as usize];
            if (action() != ACTION_ALERT || m.i16(at(a::STATE_ALERT_MOVE_POSITION_ORDER)) as i32 != order) && alert(order) {
                changed = true;
            }
        }
        _ => {}
    }

    if !changed && action() == ACTION_NONE && alert(0) {
        changed = true;
    }
    changed
}

/// actions.c's dives (`global_dive_animation_table`): the animation, the
/// direction it dives (left, right, forward, back) and its bias.
const DIVE_ANIMATIONS: [(i32, usize, f32); 6] =
    [(10, 0, 1.5), (6, 0, 0.0), (11, 1, 1.5), (7, 1, 0.0), (8, 2, 1.5), (9, 3, 1.5)];

/// ai_debug.h's dive decisions (`_dive_*`).
const DIVE_CANNOT_MOVE: i16 = 1;
const DIVE_NO_ANIMATION: i16 = 2;
const DIVE_ANIMATION_FAILURE: i16 = 3;
const DIVE_SUCCESS: i16 = 4;

/// `actor_action_allowed_to_enter_vehicle`: not the vehicle the actor's
/// ignoring, until it stops ignoring it.
pub fn allowed_to_enter_vehicle(context: &AiStepContext, host: &impl AiHost, vehicle_index: i32) -> bool {
    let m = host.memory();
    let at = |offset: usize| context.actor + offset as u32;
    !(vehicle_index == m.i32(at(a::EMOTIONS_VEHICLE_IGNORE_INDEX))
        && context.game_time < m.i32(at(a::EMOTIONS_VEHICLE_IGNORE_TIME)))
}

/// actions.c's `actor_action_find_escape_from_danger`: the side (left or
/// right of the way the danger comes, or else of the way to it, or the
/// facing) the actor can dive as far as its unit dives, preferring a ledge,
/// one clear of the danger's line, or the farther; whether it's clear, the
/// direction (NONE if neither side has room, or random), the distance, the
/// alignment and whether it's a ledge.
fn find_escape_from_danger(context: &AiStepContext, host: &impl AiHost) -> (bool, i16, f32, [f32; 2], bool) {
    let m = host.memory();
    let at = |offset: usize| context.actor + offset as u32;
    let evade_distance = call_full(context, host, call::UNIT_DIVE_DISTANCE, 0, 0, &[]).output[0];
    let mut escape_direction = NONE as i16;
    let mut result = false;
    let mut selected_is_ledge = false;
    let mut alignment = [0.0f32; 2];

    if evade_distance > 0.0 {
        let velocity = m.vector(at(a::DANGER_ZONE_VELOCITY));
        let body = m.vector(at(a::INPUT_BODY_POSITION));
        let danger_position = m.vector(at(a::DANGER_ZONE_POSITION));
        alignment = [-velocity[0], -velocity[1]];
        if normalize2(&mut alignment) < 0.033_333_335 {
            alignment = [danger_position[0] - body[0], danger_position[1] - body[1]];
            if normalize2(&mut alignment) == 0.0 {
                let facing = m.vector(at(a::INPUT_FACING_VECTOR));
                alignment = [facing[0], facing[1]];
                if normalize2(&mut alignment) == 0.0 {
                    alignment = [1.0, 0.0];
                }
            }
        }
        let flags = m.r32(context.definition + d::FLAGS as u32);
        let ledge_avoidance_distance = if flags & (1 << v::_ACTOR_DEFINITION_DIVE_OFF_LEDGES_BIT) != 0 { 8.0 } else { 0.0 };
        let danger_motion = vector_from_points(danger_position, m.vector(at(a::DANGER_ZONE_PREDICT_DANGER_POSITION)));
        let left_vector = [-alignment[1], alignment[0], 0.0f32];
        let right_vector = [alignment[1], -alignment[0], 0.0f32];
        let position = |side: [f32; 3]| {
            [side[0] * evade_distance + body[0], side[1] * evade_distance + body[1], side[2] * evade_distance + body[2]]
        };
        let left_position = position(left_vector);
        let right_position = position(right_vector);
        let danger_radius = || m.f32(at(a::DANGER_ZONE_DANGER_RADIUS));

        let left = call_full(
            context,
            host,
            call::TRY_EVASION_VECTOR,
            0,
            0,
            &[left_vector[0], left_vector[1], evade_distance, ledge_avoidance_distance],
        );
        let left_found = left.result != 0;
        let left_is_ledge = left.result2 != 0;
        let left_distance = point_to_line_distance_squared(left_position, danger_position, danger_motion).sqrt();
        let left_clear = left_found && left_distance > danger_radius();

        let right = call_full(
            context,
            host,
            call::TRY_EVASION_VECTOR,
            0,
            0,
            &[right_vector[0], right_vector[1], evade_distance, ledge_avoidance_distance],
        );
        let right_found = right.result != 0;
        let right_is_ledge = right.result2 != 0;
        let right_distance = point_to_line_distance_squared(right_position, danger_position, danger_motion).sqrt();
        let right_clear = right_found && right_distance > danger_radius();

        if left_found {
            if right_found {
                let distance_difference = left_distance - right_distance;
                if left_is_ledge > right_is_ledge || left_clear > right_clear || distance_difference > 0.3 {
                    escape_direction = EVADE_LEFT;
                    selected_is_ledge = left_is_ledge;
                    result = true;
                } else if right_is_ledge > left_is_ledge || right_clear > left_clear || distance_difference < -0.3 {
                    escape_direction = EVADE_RIGHT;
                    selected_is_ledge = right_is_ledge;
                    result = true;
                } else {
                    escape_direction = EVADE_RANDOM;
                    selected_is_ledge = left_is_ledge;
                    result = left_clear;
                }
            } else {
                escape_direction = EVADE_LEFT;
                selected_is_ledge = left_is_ledge;
                result = left_clear;
            }
        } else if right_found {
            escape_direction = EVADE_RIGHT;
            selected_is_ledge = right_is_ledge;
            result = right_clear;
        }
    }
    (result, escape_direction, evade_distance, alignment, selected_is_ledge)
}

/// `actor_action_try_to_dive`: an actor on foot with room to dive that way
/// takes the dive animation (of those its unit has) best matching the way
/// it dives relative to its facing, and shouts. Its debug record notes the
/// decision.
pub fn try_to_dive(
    context: &AiStepContext,
    host: &impl AiHost,
    escape_direction: i16,
    dive_distance: f32,
    alignment_vector: [f32; 2],
    maximum_ledge_height: f32,
) -> bool {
    let m = host.memory();
    let at = |offset: usize| context.actor + offset as u32;
    let debug_at = |offset: usize| context.debug_info + offset as u32;
    let decision = |value: i16| m.set_i16(debug_at(debug::DIVE_DECISION), value);
    m.w32(debug_at(debug::DIVE_DECISION_TIME), context.game_time as u32);

    if m.i32(at(a::INPUT_VEHICLE_INDEX)) != NONE {
        decision(DIVE_CANNOT_MOVE);
        return false;
    }
    let evasion = call_full(
        context,
        host,
        call::EVASION_DIRECTION,
        escape_direction as i32,
        0,
        &[alignment_vector[0], alignment_vector[1], dive_distance, maximum_ledge_height],
    );
    if evasion.result == 0 {
        decision(DIVE_CANNOT_MOVE);
        return false;
    }
    let alignment = [evasion.output[0], evasion.output[1]];
    let evade_vector = match evasion.output[2] as i16 {
        EVADE_LEFT => [-alignment[1], alignment[0]],
        EVADE_RIGHT => [alignment[1], -alignment[0]],
        EVADE_FORWARD | EVADE_BACK => [alignment[0], alignment[1]],
        _ => [0.0, 0.0],
    };

    let facing = m.vector(at(a::INPUT_FACING_VECTOR));
    let left_vector = [-facing[1], facing[0]];
    let mut animation_desire = [0.0f32; 4];
    animation_desire[2] = evade_vector[0] * facing[0] + evade_vector[1] * facing[1];
    animation_desire[0] = evade_vector[0] * left_vector[0] + evade_vector[1] * left_vector[1];
    animation_desire[3] = -animation_desire[2];
    animation_desire[1] = -animation_desire[0];

    let mut best_animation = NONE;
    let mut best_direction = NONE as i16;
    let mut best_score = -0.5f32;
    for (animation, direction, bias) in DIVE_ANIMATIONS {
        if best_score < animation_desire[direction] + bias
            && call_simple(context, host, call::TEST_ANIMATION_IMPULSE, animation) != 0
        {
            best_animation = animation;
            best_direction = direction as i16;
            best_score = animation_desire[direction] + bias;
        }
    }

    if best_animation == NONE {
        decision(DIVE_NO_ANIMATION);
        return false;
    }
    let animation_alignment = match best_direction {
        0 => [evade_vector[1], -evade_vector[0]],
        1 => [-evade_vector[1], evade_vector[0]],
        _ => [evade_vector[0], evade_vector[1]],
    };
    let result = call_full(context, host, call::ANIMATION_IMPULSE, best_animation, 0, &animation_alignment).result != 0;
    if result {
        call_full(context, host, call::COMMUNICATION, v::_AI_COMMUNICATION_DIVE as i32, NONE, &[NONE as f32]);
        decision(DIVE_SUCCESS);
    } else {
        decision(DIVE_ANIMATION_FAILURE);
    }
    result
}

/// Each action's data's size, and whether it has a begin (actions.c's
/// `global_action_functions`).
const ACTION_DATA_SIZES: [usize; 14] = [0, 0, 92, 4, 48, 52, 68, 44, 24, 76, 56, 132, 20, 4];
const ACTION_HAS_BEGIN: [bool; 14] =
    [false, false, true, true, true, true, true, true, true, true, true, true, true, true];

/// An action's data, as a setup fills it in for [`action_change`] (the
/// game's `action_state_data`), from zeros.
pub struct ActionData {
    bytes: [u8; 132],
}

impl ActionData {
    pub fn new() -> Self {
        ActionData { bytes: [0; 132] }
    }

    /// The data's byte at the actor's field `actor_offset`
    /// (`actor_datum`'s `state.action_data` member).
    fn index(actor_offset: usize) -> usize {
        actor_offset - a::STATE_ACTION_DATA
    }

    fn set_bytes(&mut self, actor_offset: usize, bytes: &[u8]) {
        let index = Self::index(actor_offset);
        self.bytes[index..index + bytes.len()].copy_from_slice(bytes);
    }

    pub fn set_bool(&mut self, actor_offset: usize, value: bool) {
        self.set_bytes(actor_offset, &[value as u8]);
    }

    pub fn set_i16(&mut self, actor_offset: usize, value: i16) {
        self.set_bytes(actor_offset, &value.to_le_bytes());
    }

    pub fn set_i32(&mut self, actor_offset: usize, value: i32) {
        self.set_bytes(actor_offset, &value.to_le_bytes());
    }

    pub fn set_vector(&mut self, actor_offset: usize, value: [f32; 3]) {
        for (i, component) in value.into_iter().enumerate() {
            self.set_bytes(actor_offset + 4 * i, &component.to_bits().to_le_bytes());
        }
    }
}

impl Default for ActionData {
    fn default() -> Self {
        Self::new()
    }
}

/// actions.c's `actor_action_change`: the actor's action ends, its mode
/// follows the new action's class (out of combat for a noncombat action,
/// into it for the rest), its discarded firing positions clear, and the new
/// action (with its data) begins.
pub fn action_change(context: &AiStepContext, host: &impl AiHost, new_action: i16, data: Option<&ActionData>) {
    let m = host.memory();
    let at = |offset: usize| context.actor + offset as u32;
    let action = m.i16(at(a::STATE_ACTION));
    call_simple(context, host, call::ACTION_END, action as i32);
    let mode = at(a::STATE_MODE);
    if ACTION_CLASSES[new_action as usize] == 0 {
        if m.i16(mode) >= 3 {
            m.set_i16(mode, 2);
        }
    } else if m.i16(mode) < 3 {
        m.set_i16(mode, 3);
    }
    call_simple(context, host, call::CLEAR_DISCARDED_FIRING_POSITIONS, 0);
    let size = ACTION_DATA_SIZES[new_action as usize];
    if let Some(data) = data {
        for (i, &byte) in data.bytes[..size].iter().enumerate() {
            m.w8(at(a::STATE_ACTION_DATA) + i as u32, byte);
        }
    }
    m.set_i16(at(a::STATE_ACTION), new_action);
    m.set_bool(at(a::STATE_ACTION_CHANGED), true);
    if ACTION_HAS_BEGIN[new_action as usize] {
        call_simple(context, host, call::ACTION_BEGIN, new_action as i32);
    }
}

/// action_alert.c's `action_alert_setup`: the move position order (none
/// for a swarm) and the first move position.
pub fn alert_setup(context: &AiStepContext, host: &impl AiHost, move_position_order: i32, initial_move_position_index: i16) -> ActionData {
    let m = host.memory();
    let mut data = ActionData::new();
    let order = if m.bool(context.actor + a::META_SWARM as u32) { 0 } else { move_position_order };
    data.set_i16(a::STATE_ALERT_MOVE_POSITION_ORDER, order as i16);
    data.set_i16(a::STATE_ALERT_TARGET_MOVE_POSITION_INDEX, NONE as i16);
    data.set_i16(a::STATE_ALERT_PENDING_MOVE_POSITION_INDEX, initial_move_position_index);
    data.set_bool(a::STATE_ALERT_MOVE_POSITION_INCREASING, true);
    data.set_i16(a::STATE_ALERT_WAIT_TICKS, 0);
    data.set_bool(a::STATE_ALERT_MUST_PLAY_ANIMATION, false);
    data
}

/// action_wait.c's `action_wait_setup`: waiting (not as a passenger), its
/// look and query timers.
pub fn wait_setup(context: &AiStepContext, host: &impl AiHost, was_actively_searching: bool) -> Option<ActionData> {
    let m = host.memory();
    let at = |offset: usize| context.actor + offset as u32;
    if m.bool(at(a::INPUT_VEHICLE_PASSENGER)) {
        return None;
    }
    let mut data = ActionData::new();
    data.set_bool(a::STATE_WAIT_WAITING_AS_COORDINATOR, m.bool(at(a::EXTERNAL_ORDERS_PURSUIT_IS_COORDINATOR)));
    data.set_bool(a::STATE_WAIT_WAS_ACTIVELY_SEARCHING, was_actively_searching);
    data.set_i32(a::STATE_WAIT_ENTRY_TIME, context.game_time);
    data.set_i16(a::STATE_WAIT_EXIT_TIMER, 0);
    data.set_i16(a::STATE_WAIT_LOOK_TIMER, 120);
    data.set_bool(a::STATE_WAIT_DESIRE_MOVE, true);
    let query_timer = call_full(context, host, call::SEED_RANDOM_RANGE, 300, 600, &[]).result;
    data.set_i16(a::STATE_WAIT_QUERY_TIMER, query_timer as i16);
    Some(data)
}

/// action_guard.c's `action_guard_setup_current_position`: guarding where
/// the actor stands, facing as it does.
pub fn guard_setup_current_position(context: &AiStepContext, host: &impl AiHost) -> ActionData {
    let m = host.memory();
    let mut data = ActionData::new();
    data.set_i16(a::STATE_GUARD_GUARD_LOCATION_TYPE, 1);
    data.set_bool(a::STATE_GUARD_HAS_GUARD_DIRECTION, true);
    data.set_vector(a::STATE_GUARD_GUARD_DIRECTION, m.vector(context.actor + a::INPUT_FACING_VECTOR as u32));
    data.set_i32(a::STATE_GUARD_GUARD_LOOK_PROP_INDEX, NONE);
    data
}

/// action_guard.c's `action_guard_setup_find_position`: guarding a new
/// position (at once, or where it stands for `wait_ticks` first); a
/// passenger or swarm guards where it is.
pub fn guard_setup_find_position(context: &AiStepContext, host: &impl AiHost, wait_ticks: i32) -> ActionData {
    let m = host.memory();
    let at = |offset: usize| context.actor + offset as u32;
    let mut data = ActionData::new();
    if !m.bool(at(a::INPUT_VEHICLE_PASSENGER)) && !m.bool(at(a::META_SWARM)) {
        data.set_i16(a::STATE_GUARD_WAIT_TICKS, wait_ticks as i16);
        if wait_ticks as i16 == 0 {
            data.set_bool(a::STATE_GUARD_FIND_NEW_GUARD_POSITION, true);
            data.set_i16(a::STATE_GUARD_GUARD_LOCATION_TYPE, 0);
        } else {
            data.set_i16(a::STATE_GUARD_GUARD_LOCATION_TYPE, 1);
            data.set_bool(a::STATE_GUARD_HAS_GUARD_DIRECTION, true);
            data.set_vector(a::STATE_GUARD_GUARD_DIRECTION, m.vector(at(a::INPUT_FACING_VECTOR)));
        }
    } else {
        data.set_i16(a::STATE_GUARD_GUARD_LOCATION_TYPE, 1);
    }
    data.set_i32(a::STATE_GUARD_GUARD_LOOK_PROP_INDEX, NONE);
    data
}

/// actors.h's post-combat orders (`_actor_postcombat_*`) a guard goes to.
const POSTCOMBAT_RUN_TO: i16 = 6;
const POSTCOMBAT_CHECK_ENEMY: i16 = 7;
const POSTCOMBAT_CHECK_FRIEND: i16 = 8;
const POSTCOMBAT_SHOOT_CORPSE: i16 = 9;

/// real_math.h's `normalize3d`: the vector to unit length (left alone when
/// too short), and its length (0 then).
fn normalize3(v: &mut [f32; 3]) -> f32 {
    let magnitude = magnitude_squared(*v).sqrt();
    if !(REAL_EPSILON > magnitude.abs()) {
        let s = 1.0 / magnitude;
        *v = [s * v[0], s * v[1], s * v[2]];
        magnitude
    } else {
        0.0
    }
}

/// action_guard.c's `action_guard_setup_postcombat`: guarding after combat
/// (not a flying driver's), going to the post-combat prop's pathfinding
/// point to run to it, check it or shoot its corpse, and looking at it.
pub fn guard_setup_postcombat(context: &AiStepContext, host: &impl AiHost) -> Option<ActionData> {
    let m = host.memory();
    let at = |offset: usize| context.actor + offset as u32;
    let mut data = ActionData::new();
    data.set_i16(a::STATE_GUARD_WAIT_TICKS, 120);
    data.set_i16(a::STATE_GUARD_GUARD_LOCATION_TYPE, 1);
    data.set_bool(a::STATE_GUARD_POST_COMBAT, true);
    data.set_i32(a::STATE_GUARD_GUARD_LOOK_PROP_INDEX, NONE);

    let postcombat_prop_index = m.i32(at(a::EXTERNAL_ORDERS_POSTCOMBAT_PROP_INDEX));
    if m.i16(at(a::INPUT_VEHICLE_DRIVER_TYPE)) == 4 {
        return None;
    }
    if !m.bool(at(a::INPUT_VEHICLE_PASSENGER)) && !m.bool(at(a::META_SWARM)) && postcombat_prop_index != NONE {
        let prop = prop_get(m, context, postcombat_prop_index);
        data.set_i32(a::STATE_GUARD_GUARD_LOOK_PROP_INDEX, postcombat_prop_index);
        data.set_i16(a::STATE_GUARD_LOOK_TICKS, 120);
        data.set_bool(a::STATE_GUARD_GUARD_LOOK_UNTIL_REACHED_POINT, true);
        let radius = match m.i16(at(a::EXTERNAL_ORDERS_POSTCOMBAT_TYPE)) {
            POSTCOMBAT_RUN_TO => 2.0f32,
            POSTCOMBAT_CHECK_ENEMY | POSTCOMBAT_CHECK_FRIEND => 1.0,
            POSTCOMBAT_SHOOT_CORPSE => 1.5,
            _ => return Some(data),
        };
        data.set_i32(a::STATE_GUARD_GUARD_POINT_RADIUS, radius.to_bits() as i32);
        call_simple(context, host, call::FIND_PROP_PATHFINDING_LOCATION, postcombat_prop_index);
        data.set_i16(a::STATE_GUARD_GUARD_LOCATION_TYPE, 2);
        data.set_vector(a::STATE_GUARD_GUARD_POINT_POSITION, m.vector(prop + p::PATHFINDING_POINT as u32));
        data.set_i32(a::STATE_GUARD_GUARD_POINT_SURFACE_INDEX, m.i32(prop + p::PATHFINDING_SURFACE_INDEX as u32));
    }
    Some(data)
}

/// action_guard.c's `action_guard_setup_from_combat_transition`: guarding as
/// combat begins, at the point it was given (if it has positions to search
/// from), or where it stands facing what alerted it for a while, or a new
/// position; shouting about a dead friend, and looking at what alerted it.
pub fn guard_setup_from_combat_transition(context: &AiStepContext, host: &impl AiHost) -> ActionData {
    let m = host.memory();
    let at = |offset: usize| context.actor + offset as u32;
    let mut data = ActionData::new();
    let wait_ticks = m.i32(at(a::STIMULI_COMBAT_TRANSITION_GUARD_TIMER)) as i16;
    data.set_i16(a::STATE_GUARD_WAIT_TICKS, wait_ticks);

    if !m.bool(at(a::INPUT_VEHICLE_PASSENGER)) && !m.bool(at(a::META_SWARM)) {
        let guard_point = m.vector(at(a::STIMULI_COMBAT_TRANSITION_GUARD_POINT));
        let guard_surface_index = m.i32(at(a::STIMULI_COMBAT_TRANSITION_GUARD_POINT_SURFACE_INDEX));
        if m.bool(at(a::STIMULI_COMBAT_TRANSITION_GUARD_AT_POINT))
            && call_full(context, host, call::NEARBY_FIRING_POSITIONS_AT, guard_surface_index, 0, &guard_point).result != 0
        {
            data.set_i16(a::STATE_GUARD_GUARD_LOCATION_TYPE, 2);
            data.set_vector(a::STATE_GUARD_GUARD_POINT_POSITION, m.vector(at(a::STIMULI_COMBAT_TRANSITION_GUARD_POINT)));
            data.set_i32(
                a::STATE_GUARD_GUARD_POINT_SURFACE_INDEX,
                m.i32(at(a::STIMULI_COMBAT_TRANSITION_GUARD_POINT_SURFACE_INDEX)),
            );
            data.set_i32(a::STATE_GUARD_GUARD_POINT_RADIUS, m.r32(at(a::STIMULI_COMBAT_TRANSITION_GUARD_POINT_DISTANCE)) as i32);
        } else if wait_ticks > 0 {
            data.set_i16(a::STATE_GUARD_GUARD_LOCATION_TYPE, 1);
            let has_vector = m.bool(at(a::STIMULI_COMBAT_TRANSITION_HAS_VECTOR));
            data.set_bool(a::STATE_GUARD_HAS_GUARD_DIRECTION, has_vector);
            data.set_bool(a::STATE_GUARD_AIM_IN_GUARD_DIRECTION, false);
            if has_vector {
                let mut direction = m.vector(at(a::STIMULI_COMBAT_TRANSITION_VECTOR));
                let magnitude = normalize3(&mut direction);
                data.set_vector(a::STATE_GUARD_GUARD_DIRECTION, direction);
                if magnitude == 0.0 {
                    data.set_bool(a::STATE_GUARD_HAS_GUARD_DIRECTION, false);
                }
            }
        } else {
            data.set_i16(a::STATE_GUARD_GUARD_LOCATION_TYPE, 0);
            data.set_bool(a::STATE_GUARD_FIND_NEW_GUARD_POSITION, true);
        }
    } else {
        data.set_i16(a::STATE_GUARD_GUARD_LOCATION_TYPE, 1);
    }

    let transition_prop_index = m.i32(at(a::STIMULI_COMBAT_TRANSITION_PROP_INDEX));
    if m.i16(at(a::STIMULI_COMBAT_TRANSITION)) == 2 {
        data.set_bool(a::STATE_GUARD_SHOUT_ABOUT_DEAD_FRIEND, true);
        data.set_i32(a::STATE_GUARD_SHOUT_DEAD_FRIEND_PROP_INDEX, transition_prop_index);
    }
    data.set_i32(a::STATE_GUARD_GUARD_LOOK_PROP_INDEX, transition_prop_index);
    if transition_prop_index != NONE {
        data.set_i16(a::STATE_GUARD_LOOK_TICKS, m.i32(at(a::STIMULI_COMBAT_TRANSITION_PROP_LOOK_TIMER)) as i16);
        data.set_bool(
            a::STATE_GUARD_GUARD_LOOK_UNTIL_REACHED_POINT,
            m.bool(at(a::STIMULI_COMBAT_TRANSITION_PROP_LOOK_WHILE_MOVING)),
        );
    }
    data
}

/// action_guard.c's `action_guard_setup_from_fleeing`: a fled actor cowers
/// (as long as its panic or hiding time, unless it retreated from an
/// unopposable enemy) at the position it fled to, facing the way it came,
/// or at a new position.
pub fn guard_setup_from_fleeing(context: &AiStepContext, host: &impl AiHost) -> ActionData {
    let m = host.memory();
    let at = |offset: usize| context.actor + offset as u32;
    let definition = |offset: usize| m.f32(context.definition + offset as u32);
    let mut data = ActionData::new();
    if !m.bool(at(a::INPUT_VEHICLE_PASSENGER)) && !m.bool(at(a::META_SWARM)) {
        data.set_i16(a::STATE_GUARD_WAIT_TICKS, 0);
        data.set_bool(a::STATE_GUARD_COWER, true);
        data.set_i16(a::STATE_GUARD_COWER_TICKS, 0);
        let cower_panicked = m.i16(at(a::STATE_FLEE_PANIC_TYPE)) > 0;
        data.set_bool(a::STATE_GUARD_COWER_PANICKED, cower_panicked);
        let cower_from_retreat = m.i16(at(a::EMOTIONS_UNOPPOSABLE_RETREAT_TIMER)) > 0 && !cower_panicked;
        data.set_bool(a::STATE_GUARD_COWER_FROM_RETREAT, cower_from_retreat);
        if !cower_from_retreat {
            let (lower, upper) = if cower_panicked {
                (definition(d::PANIC_COWER_TIME_LOWER_BOUND), definition(d::PANIC_COWER_TIME_UPPER_BOUND))
            } else {
                (definition(d::DEFENSIVE_HIDE_TIME_LOWER_BOUND), definition(d::DEFENSIVE_HIDE_TIME_UPPER_BOUND))
            };
            let time = call_full(context, host, call::REAL_SEED_RANDOM_RANGE, 0, 0, &[lower, upper]).output[0];
            data.set_i16(a::STATE_GUARD_COWER_TICKS, c_short(time * 30.0));
        }

        let flee_firing_position_index = m.i16(at(a::STATE_FLEE_FLEE_FIRING_POSITION_INDEX));
        if flee_firing_position_index == NONE as i16 {
            data.set_bool(a::STATE_GUARD_FIND_NEW_GUARD_POSITION, true);
            data.set_i16(a::STATE_GUARD_GUARD_LOCATION_TYPE, 0);
        } else {
            data.set_bool(a::STATE_GUARD_FIND_NEW_GUARD_POSITION, false);
            data.set_i16(a::STATE_GUARD_GUARD_LOCATION_TYPE, 3);
            data.set_i16(a::STATE_GUARD_GUARD_FIRING_POSITION_INDEX, flee_firing_position_index);
            if m.bool(at(a::STATE_FLEE_HAS_APPROACH_POINT)) {
                data.set_bool(a::STATE_GUARD_HAS_GUARD_DIRECTION, true);
                data.set_bool(a::STATE_GUARD_AIM_IN_GUARD_DIRECTION, true);
                let mut direction =
                    vector_from_points(m.vector(at(a::INPUT_BODY_POSITION)), m.vector(at(a::STATE_FLEE_APPROACH_POINT)));
                let magnitude = normalize3(&mut direction);
                data.set_vector(a::STATE_GUARD_GUARD_DIRECTION, direction);
                if magnitude == 0.0 {
                    data.set_bool(a::STATE_GUARD_HAS_GUARD_DIRECTION, false);
                }
            }
        }
    } else {
        data.set_i16(a::STATE_GUARD_GUARD_LOCATION_TYPE, 1);
    }
    data.set_i32(a::STATE_GUARD_GUARD_LOOK_PROP_INDEX, NONE);
    data
}

/// The encounter's firing position `firing_position_index`'s definition.
fn firing_position_definition(context: &AiStepContext, host: &impl AiHost, firing_position_index: i16) -> u32 {
    use crate::ai_actor::tag_block_element;
    use crate::layout::{encounter_definition, firing_position_definition, scenario};
    let m = host.memory();
    let encounter = tag_block_element(
        m,
        context.scenario + scenario::AI_ENCOUNTERS as u32,
        m.i32(context.actor + a::META_ENCOUNTER_INDEX as u32) & 0xFFFF,
        encounter_definition::SIZEOF,
    );
    tag_block_element(
        m,
        encounter + encounter_definition::FIRING_POSITIONS as u32,
        firing_position_index as i32,
        firing_position_definition::SIZEOF,
    )
}

/// The pursuit location of the firing position `firing_position_index`, in
/// the search's or uncover's data (they share it).
fn set_pursuit_position(context: &AiStepContext, host: &impl AiHost, data: &mut ActionData, firing_position_index: i16) {
    use crate::layout::firing_position_definition as position;
    let m = host.memory();
    let definition = firing_position_definition(context, host, firing_position_index);
    data.set_i16(a::STATE_SEARCH_PURSUIT_LOCATION_FIRING_POSITION_INDEX, firing_position_index);
    data.set_i16(a::STATE_SEARCH_PURSUIT_LOCATION_TYPE, v::_PURSUIT_LOCATION_POSITION as i16);
    data.set_vector(a::STATE_SEARCH_PURSUIT_LOCATION_POSITION, m.vector(definition + position::POSITION as u32));
    data.set_i32(a::STATE_SEARCH_PURSUIT_LOCATION_SURFACE_INDEX, m.i32(definition + position::SURFACE_INDEX as u32));
    data.set_i16(a::STATE_SEARCH_PURSUIT_LOCATION_CLUSTER_INDEX, m.i16(definition + position::CLUSTER_INDEX as u32));
}

/// action_search.c's `action_search_setup_target`: searching for the target
/// (charging when it must), not as a passenger.
pub fn search_setup_target(context: &AiStepContext, host: &impl AiHost, must_charge: bool) -> Option<ActionData> {
    let m = host.memory();
    let at = |offset: usize| context.actor + offset as u32;
    if m.bool(at(a::INPUT_VEHICLE_PASSENGER)) {
        return None;
    }
    let mut data = ActionData::new();
    data.set_i16(a::STATE_SEARCH_PURSUIT_LOCATION_TYPE, 0);
    data.set_bool(a::STATE_SEARCH_CHARGING, must_charge);
    m.set_bool(at(a::STATE_SEARCHING), true);
    Some(data)
}

/// action_search.c's `action_search_setup_undirected`: a swarm searches
/// without a direction.
pub fn search_setup_undirected(context: &AiStepContext, host: &impl AiHost) -> Option<ActionData> {
    let m = host.memory();
    let at = |offset: usize| context.actor + offset as u32;
    if !m.bool(at(a::META_SWARM)) {
        return None;
    }
    let mut data = ActionData::new();
    data.set_i16(a::STATE_SEARCH_PURSUIT_LOCATION_TYPE, 2);
    m.set_bool(at(a::STATE_SEARCHING), true);
    Some(data)
}

/// action_search.c's `action_search_setup_pursuit`: searching from a pursuit
/// position of the encounter (not as a passenger or swarm).
pub fn search_setup_pursuit(
    context: &AiStepContext,
    host: &impl AiHost,
    firing_position_index: i16,
    tenacious: bool,
) -> Option<ActionData> {
    let m = host.memory();
    let at = |offset: usize| context.actor + offset as u32;
    if m.bool(at(a::INPUT_VEHICLE_PASSENGER))
        || m.bool(at(a::META_SWARM))
        || m.i32(at(a::META_ENCOUNTER_INDEX)) == NONE
        || firing_position_index == NONE as i16
    {
        return None;
    }
    let mut data = ActionData::new();
    data.set_bool(a::STATE_SEARCH_TENACIOUS, tenacious);
    set_pursuit_position(context, host, &mut data, firing_position_index);
    m.set_bool(at(a::STATE_SEARCHING), true);
    Some(data)
}

/// action_uncover.c's `action_uncover_setup_target`: uncovering the target
/// (not as a passenger or swarm).
pub fn uncover_setup_target(context: &AiStepContext, host: &impl AiHost, able_to_search: bool) -> Option<ActionData> {
    let m = host.memory();
    let at = |offset: usize| context.actor + offset as u32;
    if m.bool(at(a::INPUT_VEHICLE_PASSENGER)) || m.bool(at(a::META_SWARM)) {
        return None;
    }
    let mut data = ActionData::new();
    data.set_i16(a::STATE_UNCOVER_PURSUIT_LOCATION_TYPE, 0);
    data.set_bool(a::STATE_UNCOVER_ABLE_TO_SEARCH, able_to_search);
    Some(data)
}

/// action_uncover.c's `action_uncover_setup_pursuit`: uncovering from a
/// pursuit position of the encounter (not as a passenger or swarm).
pub fn uncover_setup_pursuit(context: &AiStepContext, host: &impl AiHost, firing_position_index: i16) -> Option<ActionData> {
    let m = host.memory();
    let at = |offset: usize| context.actor + offset as u32;
    if m.bool(at(a::INPUT_VEHICLE_PASSENGER))
        || m.bool(at(a::META_SWARM))
        || m.i32(at(a::META_ENCOUNTER_INDEX)) == NONE
        || firing_position_index == NONE as i16
    {
        return None;
    }
    let mut data = ActionData::new();
    set_pursuit_position(context, host, &mut data, firing_position_index);
    data.set_bool(a::STATE_UNCOVER_PURSUIT_LOCATION_INSPECTED, false);
    data.set_bool(a::STATE_UNCOVER_ABLE_TO_SEARCH, true);
    m.set_bool(at(a::STATE_SEARCHING), true);
    Some(data)
}

/// action_flee.c's `action_flee_setup`: fleeing (not as a passenger), from
/// `panic_prop_index` (taken as a target), in a blind panic standing still
/// by chance, or to a flee position (a swarm has none).
pub fn flee_setup(
    context: &AiStepContext,
    host: &impl AiHost,
    panic_type: i16,
    panic_prop_index: i32,
    force_state_entry: bool,
    flee_from_last_visible_location: bool,
    allow_occluded_points: bool,
) -> Option<ActionData> {
    let m = host.memory();
    let at = |offset: usize| context.actor + offset as u32;
    if m.bool(at(a::INPUT_VEHICLE_PASSENGER)) {
        return None;
    }
    let mut data = ActionData::new();
    let forced_flee_ticks: i16 = if force_state_entry { 6 * 30 } else { 0 };
    data.set_i16(a::STATE_FLEE_FORCED_FLEE_TICKS, forced_flee_ticks);
    data.set_i16(a::STATE_FLEE_FLEE_FIRING_POSITION_INDEX, NONE as i16);
    data.set_i16(a::STATE_FLEE_PANIC_TYPE, panic_type);
    data.set_bool(a::STATE_FLEE_FLEE_FROM_LAST_VISIBLE_LOCATION, flee_from_last_visible_location);
    data.set_bool(a::STATE_FLEE_ALLOW_OCCLUDED_POINTS, allow_occluded_points);
    data.set_i32(a::STATE_FLEE_FLEE_PROP_INDEX, panic_prop_index);
    if panic_prop_index != NONE {
        call_simple(context, host, call::TRY_NEW_TARGET, panic_prop_index);
    }

    if crate::actions::flee_blind_panic(panic_type) && call_full(context, host, call::RANDOM, 0, 0, &[]).output[0] < 0.4 {
        data.set_i16(a::STATE_FLEE_FLEE_STATIONARY_TICKS, 45);
        return Some(data);
    }
    if m.bool(at(a::META_SWARM)) {
        return None;
    }
    let found = call_full(
        context,
        host,
        call::FIND_FLEE_POSITION,
        panic_type as i32,
        panic_prop_index,
        &[forced_flee_ticks as f32, flee_from_last_visible_location as i32 as f32, allow_occluded_points as i32 as f32],
    );
    let flee_firing_position_index = found.result as i16;
    data.set_i16(a::STATE_FLEE_FLEE_FIRING_POSITION_INDEX, flee_firing_position_index);
    data.set_bool(a::STATE_FLEE_FLEE_FIRING_POSITION_FOUND_RANDOMLY, found.result2 & 1 != 0);
    data.set_bool(a::STATE_FLEE_HAS_APPROACH_POINT, found.result2 & 2 != 0);
    data.set_vector(a::STATE_FLEE_APPROACH_POINT, found.vector());
    data.set_bool(a::STATE_FLEE_FIND_NEW_FLEE_POSITION, false);
    if flee_firing_position_index != NONE as i16 {
        Some(data)
    } else {
        None
    }
}

/// actions.c's `actor_action_try_to_panic`: fleeing in a panic.
pub fn try_to_panic(context: &AiStepContext, host: &impl AiHost, panic_type: i16, panic_prop_index: i32, force: bool) -> bool {
    match flee_setup(context, host, panic_type, panic_prop_index, force, false, false) {
        Some(data) => {
            action_change(context, host, ACTION_FLEE, Some(&data));
            true
        }
        None => false,
    }
}

/// actions.c's `actor_action_try_to_seek_cover`: fleeing to cover from the
/// target.
pub fn try_to_seek_cover(
    context: &AiStepContext,
    host: &impl AiHost,
    cover_from_last_visible_location: bool,
    allow_occluded_points: bool,
) -> bool {
    let target_prop_index = host.memory().i32(context.actor + a::TARGET_TARGET_PROP_INDEX as u32);
    match flee_setup(
        context,
        host,
        PANIC_NONE,
        target_prop_index,
        false,
        cover_from_last_visible_location,
        allow_occluded_points,
    ) {
        Some(data) => {
            action_change(context, host, ACTION_FLEE, Some(&data));
            true
        }
        None => false,
    }
}

/// ai_debug.h's charge decisions (`_charge_*`).
mod charge_decision {
    pub const VEHICLE_SUCCESS: i16 = 0;
    pub const VEHICLE_NOT_DRIVER: i16 = 1;
    pub const MELEE_SWARM_CANT: i16 = 2;
    pub const MELEE_INHIBITED: i16 = 3;
    pub const MELEE_NOTARGET: i16 = 4;
    pub const MELEE_NO_ANIMATION: i16 = 5;
    pub const MELEE_CANNOT_MOVE: i16 = 6;
    pub const MELEE_SUCCESS: i16 = 7;
    pub const STALKING_SUCCESS: i16 = 8;
    pub const CLOSE_SUCCESS: i16 = 9;
}

/// action_charge.c's `action_charge_setup` and the change to charge: whether
/// it charges.
fn charge(context: &AiStepContext, host: &impl AiHost, goal: i16) -> bool {
    match charge_setup(context, host, goal) {
        Some(data) => {
            action_change(context, host, ACTION_CHARGE, Some(&data));
            true
        }
        None => false,
    }
}

/// action_charge.c's `action_charge_setup`: a vehicle's charge (for a
/// driver); a melee charge (not a swarm's, not inhibited, at a target),
/// leaping when it can and by chance (and at range), moving in to the
/// target's danger range, if the melee animation has the ticks and the
/// destination can reach the target; or a close-range charge, stalking for a
/// stalker in combat. Its debug record notes the decision.
pub fn charge_setup(context: &AiStepContext, host: &impl AiHost, goal: i16) -> Option<ActionData> {
    let m = host.memory();
    let at = |offset: usize| context.actor + offset as u32;
    let definition = |offset: usize| m.f32(context.definition + offset as u32);
    let definition_flag = |bit: usize| m.r32(context.definition + d::FLAGS as u32) & (1 << bit) != 0;
    let debug_at = |offset: usize| context.debug_info + offset as u32;
    let decision = |value: i16| m.set_i16(debug_at(debug::CHARGE_DECISION), value);
    m.w32(debug_at(debug::CHARGE_LAST_TIME), context.game_time as u32);
    let mut data = ActionData::new();
    data.set_i32(a::STATE_CHARGE_CHARGE_START_TIME, context.game_time);
    let mut goal = goal;
    let mut result = true;

    if goal == v::_CHARGE_GOAL_VEHICLE_RAMMING as i16 || goal == v::_CHARGE_GOAL_VEHICLE_STRAFING as i16 {
        result = m.i16(at(a::INPUT_VEHICLE_DRIVER_TYPE)) > 1;
        decision(if result { charge_decision::VEHICLE_SUCCESS } else { charge_decision::VEHICLE_NOT_DRIVER });
    } else if goal == v::_CHARGE_GOAL_MELEE as i16 {
        result = false;
        if m.bool(at(a::META_SWARM)) {
            decision(charge_decision::MELEE_SWARM_CANT);
        } else if call_simple(context, host, call::MELEE_INHIBITED, 0) != 0 {
            decision(charge_decision::MELEE_INHIBITED);
        } else if m.i32(at(a::TARGET_TARGET_PROP_INDEX)) == NONE {
            decision(charge_decision::MELEE_NOTARGET);
        } else {
            let target_prop_index = m.i32(at(a::TARGET_TARGET_PROP_INDEX));
            let prop = prop_get(m, context, target_prop_index);
            let leap_upper = definition(d::BERSERK_MELEE_LEAP_RANGE_UPPER_BOUND);
            let leap_chance = definition(d::BERSERK_MELEE_LEAP_CHANCE);
            let leap = if leap_upper == 0.0 || leap_chance == 0.0 {
                data.set_bool(a::STATE_CHARGE_LEAP_POSSIBLE_IF_AT_RANGE, false);
                false
            } else if m.bool(prop + p::FLYING as u32) || m.i16(prop + p::UNREACHABLE_TICKS as u32) > 0 {
                data.set_bool(a::STATE_CHARGE_LEAP_POSSIBLE_IF_AT_RANGE, true);
                true
            } else {
                let possible = call_full(context, host, call::RANDOM, 0, 0, &[]).output[0] < leap_chance;
                data.set_bool(a::STATE_CHARGE_LEAP_POSSIBLE_IF_AT_RANGE, possible);
                possible && !(m.f32(prop + p::DISTANCE as u32) < definition(d::BERSERK_MELEE_LEAP_RANGE_LOWER_BOUND))
            };
            if leap {
                goal = v::_CHARGE_GOAL_MELEE_LEAPING as i16;
            }
            m.set_bool(debug_at(debug::FIELD_198), leap);

            let suicidal = definition_flag(v::_ACTOR_DEFINITION_SUICIDAL_MELEE_ATTACK_BIT);
            let melee = call_full(context, host, call::MELEE_RANGE_AND_TICKS, leap as i32, suicidal as i32, &[]);
            if melee.result != 0 {
                let mut start_tick = melee.result2 as i16;
                let end_tick = (melee.result2 >> 16) as i16;
                let mut start_range = melee.output[0];
                let end_range = melee.output[1];
                let (melee_suicide, melee_danger_range) = if suicidal {
                    data.set_i16(a::STATE_CHARGE_MELEE_TICKS_UNTIL_DANGEROUS, end_tick);
                    data.set_i32(a::STATE_CHARGE_MELEE_DANGER_RANGE, 0.0f32.to_bits() as i32);
                    data.set_bool(a::STATE_CHARGE_MELEE_SUICIDE, true);
                    (true, 0.0f32)
                } else {
                    if start_tick == 0 {
                        start_range = end_range * 0.5;
                        start_tick = end_tick / 2;
                    }
                    data.set_i16(a::STATE_CHARGE_MELEE_TICKS_UNTIL_DANGEROUS, start_tick);
                    let danger_range = end_range - start_range;
                    data.set_i32(a::STATE_CHARGE_MELEE_DANGER_RANGE, danger_range.to_bits() as i32);
                    (false, danger_range)
                };

                // action_charge.c's `action_charge_find_target_range` for a
                // melee goal, from the data being set up
                let mut target_range = 0.0f32;
                if goal == v::_CHARGE_GOAL_MELEE_LEAPING as i16 && !(0.0 > leap_upper) {
                    target_range = leap_upper;
                }
                let attack_range = definition(d::BERSERK_MELEE_ATTACK_RANGE);
                let melee_range = if melee_suicide { attack_range } else { attack_range + melee_danger_range };
                if !(target_range > melee_range) {
                    target_range = melee_range;
                }
                data.set_i32(a::STATE_CHARGE_ACCEPTABLE_TARGET_RANGE, target_range.to_bits() as i32);

                let minimum_move_range = if goal == v::_CHARGE_GOAL_MELEE_LEAPING as i16 { 4.0f32 } else { 1.5 };
                let move_range = if minimum_move_range > target_range { minimum_move_range } else { target_range };
                let mut valid = false;
                if call_full(context, host, call::MOVE_TO_PROP, target_prop_index, 0, &[move_range]).result != 0 {
                    call_simple(context, host, call::KEEP_MOVING, 0);
                    let center_of_mass = m.vector(prop + p::CENTER_OF_MASS as u32);
                    valid = call_full(context, host, call::VALID_MELEE_DESTINATION, goal as i32, 0, &center_of_mass).result != 0;
                }
                if valid {
                    result = true;
                    decision(charge_decision::MELEE_SUCCESS);
                } else {
                    decision(charge_decision::MELEE_CANNOT_MOVE);
                    m.set_f32(debug_at(debug::FIELD_194), move_range);
                }
            } else {
                decision(charge_decision::MELEE_NO_ANIMATION);
            }
        }
    } else if goal == v::_CHARGE_GOAL_CLOSE_RANGE as i16
        && definition_flag(v::_ACTOR_DEFINITION_STALKING_BEHAVIOR_BIT)
        && m.i16(at(a::STATE_COMBAT_STATUS)) >= 5
        && !m.bool(at(a::EMOTIONS_BERSERK))
    {
        goal = v::_CHARGE_GOAL_STALKING as i16;
        decision(charge_decision::STALKING_SUCCESS);
    } else {
        decision(charge_decision::CLOSE_SUCCESS);
    }

    data.set_i16(a::STATE_CHARGE_GOAL, goal);
    if result {
        Some(data)
    } else {
        None
    }
}

/// action_vehicle.c's `action_vehicle_setup_impromptu`: an actor on foot
/// (not a swarm, not already at it) gets into a vehicle it wants, at the
/// best seat it has an animation for, moving to the seat's entry.
pub fn vehicle_setup_impromptu(
    context: &AiStepContext,
    host: &impl AiHost,
    vehicle_index: i32,
    attempt_distance: f32,
    continue_distance: f32,
) -> Option<ActionData> {
    let m = host.memory();
    let at = |offset: usize| context.actor + offset as u32;
    let mut data = ActionData::new();
    data.set_i32(a::STATE_VEHICLE_ATTEMPT_DISTANCE, attempt_distance.to_bits() as i32);
    data.set_i32(a::STATE_VEHICLE_CONTINUE_DISTANCE, continue_distance.to_bits() as i32);
    if m.i32(at(a::INPUT_VEHICLE_INDEX)) != NONE
        || m.bool(at(a::META_SWARM))
        || m.i16(at(a::STATE_ACTION)) == ACTION_VEHICLE
        || call_full(context, host, call::VEHICLE_DESIRABLE, vehicle_index, 0, &[attempt_distance, continue_distance]).result
            == 0
    {
        return None;
    }
    data.set_i32(a::STATE_VEHICLE_VEHICLE_INDEX, vehicle_index);
    let seat_index = call_simple(context, host, call::FIND_IMPROMPTU_SEAT, vehicle_index) as i16;
    data.set_i16(a::STATE_VEHICLE_SEAT_INDEX, seat_index);
    if seat_index == NONE as i16 {
        return None;
    }
    data.set_bool(a::STATE_VEHICLE_IMPROMPTU, true);
    if call_full(context, host, call::HAS_ENTER_ANIMATION, vehicle_index, seat_index as i32, &[]).result == 0 {
        return None;
    }
    let destination = call_full(context, host, call::FIND_VEHICLE_DESTINATION, vehicle_index, 0, &[]);
    if destination.result == 0 {
        return None;
    }
    let point = destination.vector();
    data.set_vector(a::STATE_VEHICLE_DESTINATION_POINT, point);
    data.set_i32(a::STATE_VEHICLE_DESTINATION_SURFACE_INDEX, destination.result2);
    if call_full(context, host, call::MOVE_TO_POINT, destination.result2, vehicle_index, &point).result == 0 {
        return None;
    }
    Some(data)
}
