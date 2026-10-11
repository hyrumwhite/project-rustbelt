//! The actions ported to Rust (action_*.c): each action's perform, update
//! and control, which the dispatching steps ([`crate::actor_dispatch`]) run
//! in place of the game's (and actions.c's `actions_port_ported` names the
//! same, for the game's side of a check).
//!
//! An action's calls are its own: `(action << 8) | n`, answered by its C
//! file's call function (actions.c's `actions_port_call` forwards them).

use crate::ai_actor::{AiCall, AiHost, AiMemory, AiMemoryExt, AiStepContext, NONE, prop_get};
use crate::layout::{actor_datum as a, actor_definition as d, actor_variant_definition as var, ai_values as v, prop_datum as p};

/// An action's functions the dispatch runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Function {
    Perform,
    Update,
    Control,
}

/// The ported function of the action, if any: whether it ran, and for
/// `perform` its result.
pub fn run(action: i16, function: Function, context: &AiStepContext, host: &impl AiHost) -> Option<bool> {
    let action = action as usize;
    Some(match (action, function) {
        (v::_ACTOR_ACTION_SLEEP, Function::Control) => {
            sleep_control(context, host);
            false
        }
        (v::_ACTOR_ACTION_ALERT, Function::Perform) => alert_perform(context, host),
        (v::_ACTOR_ACTION_ALERT, Function::Update) => {
            alert_update(context, host);
            false
        }
        (v::_ACTOR_ACTION_ALERT, Function::Control) => {
            alert_control(context, host);
            false
        }
        (v::_ACTOR_ACTION_SEARCH, Function::Perform) => search_perform(context, host),
        (v::_ACTOR_ACTION_SEARCH, Function::Update) => {
            search_update(context, host);
            false
        }
        (v::_ACTOR_ACTION_SEARCH, Function::Control) => {
            search_control(context, host);
            false
        }
        (v::_ACTOR_ACTION_UNCOVER, Function::Perform) => uncover_perform(context, host),
        (v::_ACTOR_ACTION_UNCOVER, Function::Update) => {
            uncover_update(context, host);
            false
        }
        (v::_ACTOR_ACTION_UNCOVER, Function::Control) => {
            uncover_control(context, host);
            false
        }
        (v::_ACTOR_ACTION_GUARD, Function::Perform) => guard_perform(context, host),
        (v::_ACTOR_ACTION_GUARD, Function::Update) => {
            guard_update(context, host);
            false
        }
        (v::_ACTOR_ACTION_GUARD, Function::Control) => {
            guard_control(context, host);
            false
        }
        (v::_ACTOR_ACTION_FLEE, Function::Perform) => flee_perform(context, host),
        (v::_ACTOR_ACTION_FLEE, Function::Update) => {
            flee_update(context, host);
            false
        }
        (v::_ACTOR_ACTION_FLEE, Function::Control) => {
            flee_control(context, host);
            false
        }
        (v::_ACTOR_ACTION_CONVERSE, Function::Perform) => converse_perform(context, host),
        (v::_ACTOR_ACTION_CONVERSE, Function::Update) => false,
        (v::_ACTOR_ACTION_CONVERSE, Function::Control) => {
            converse_control(context, host);
            false
        }
        (v::_ACTOR_ACTION_VEHICLE, Function::Perform) => vehicle_perform(context, host),
        (v::_ACTOR_ACTION_VEHICLE, Function::Update) => false,
        (v::_ACTOR_ACTION_VEHICLE, Function::Control) => {
            vehicle_control(context, host);
            false
        }
        (v::_ACTOR_ACTION_CHARGE, Function::Perform) => charge_perform(context, host),
        (v::_ACTOR_ACTION_CHARGE, Function::Update) => {
            charge_update(context, host);
            false
        }
        (v::_ACTOR_ACTION_CHARGE, Function::Control) => {
            charge_control(context, host);
            false
        }
        (v::_ACTOR_ACTION_WAIT, Function::Perform) => wait_perform(context, host),
        (v::_ACTOR_ACTION_WAIT, Function::Update) => {
            wait_update(context, host);
            false
        }
        (v::_ACTOR_ACTION_WAIT, Function::Control) => {
            wait_control(context, host);
            false
        }
        (v::_ACTOR_ACTION_AVOID, Function::Perform) => avoid_perform(context, host),
        (v::_ACTOR_ACTION_AVOID, Function::Update) => false,
        (v::_ACTOR_ACTION_AVOID, Function::Control) => {
            avoid_control(context, host);
            false
        }
        (v::_ACTOR_ACTION_FIGHT, Function::Perform) => fight_perform(context, host),
        (v::_ACTOR_ACTION_FIGHT, Function::Update) => {
            fight_update(context, host);
            false
        }
        (v::_ACTOR_ACTION_FIGHT, Function::Control) => {
            fight_control(context, host);
            false
        }
        _ => return None,
    })
}

/// action_wait.c's calls.
pub mod wait_call {
    const BASE: i32 = (crate::layout::ai_values::_ACTOR_ACTION_WAIT as i32) << 8;
    /// `actor_pursuit_find_nearby_actors(actor, a)`.
    pub const FIND_NEARBY_ACTORS: i32 = BASE | 1;
    /// `actor_move_to_prop(actor, a, 8)`, the result.
    pub const MOVE_TO_PROP: i32 = BASE | 2;
    /// `actor_move_halt(actor)`.
    pub const MOVE_HALT: i32 = BASE | 3;
    /// `ai_communication_event(a, unit, NONE...)`.
    pub const COMMUNICATION: i32 = BASE | 4;
    /// `seed_random_range(global seed, 300, 600)`, the result.
    pub const QUERY_TIME: i32 = BASE | 5;
}

/// action_avoid.c's calls.
pub mod avoid_call {
    const BASE: i32 = (crate::layout::ai_values::_ACTOR_ACTION_AVOID as i32) << 8;
    /// A firing position selected (for avoiding) and changed to.
    pub const CHANGE_FIRING_POSITION: i32 = BASE | 1;
}

/// action_fight.c's calls.
pub mod fight_call {
    const BASE: i32 = (crate::layout::ai_values::_ACTOR_ACTION_FIGHT as i32) << 8;
    /// `actor_discard_firing_position(actor, a, FALSE)`.
    pub const DISCARD_FIRING_POSITION: i32 = BASE | 1;
    /// `actor_combat_get_firing_variant_definition(actor)`, its address the result.
    pub const FIRING_VARIANT: i32 = BASE | 2;
    /// `actor_find_pathfinding_location(actor)`.
    pub const FIND_PATHFINDING_LOCATION: i32 = BASE | 3;
    /// `actor_nearby_firing_positions` of the normal group at the actor's
    /// pathfinding point, the result.
    pub const NEARBY_FIRING_POSITIONS: i32 = BASE | 4;
    /// Whether the actor is within its destination tolerance of its current
    /// firing position, the result.
    pub const NEAR_CURRENT_FIRING_POSITION: i32 = BASE | 5;
    /// `actor_move_halt(actor)`.
    pub const MOVE_HALT: i32 = BASE | 6;
    /// A firing position selected (for fighting) and changed to: the new
    /// index the result.
    pub const CHANGE_FIRING_POSITION: i32 = BASE | 7;
    /// `real_seed_random_range(global seed, input[0], input[1])`, in `output[0]`.
    pub const RANDOM_RANGE: i32 = BASE | 8;
    /// The actor's vehicle's minimum firing position time, in `output[0]`.
    pub const VEHICLE_MINIMUM_FIRING_POSITION_TIME: i32 = BASE | 9;
    /// `actor_has_ranged_weapon(actor)`, the result.
    pub const HAS_RANGED_WEAPON: i32 = BASE | 10;
    /// Whether the actor is away from its current firing position and it is
    /// within weapon range of the prop `a`, the result.
    pub const FIRING_POSITION_REACHES: i32 = BASE | 11;
    /// `actor_perception_unreachable(actor, a, b)`.
    pub const PERCEPTION_UNREACHABLE: i32 = BASE | 12;
}

fn call_full(context: &AiStepContext, host: &impl AiHost, kind: i32, a: i32, b: i32, input: &[f32]) -> AiCall {
    let mut call = AiCall { kind, actor: context.actor_index, a, b, ..AiCall::default() };
    call.input[..input.len()].copy_from_slice(input);
    host.call(&mut call);
    call
}

fn call_simple(context: &AiStepContext, host: &impl AiHost, kind: i32, a: i32) -> i32 {
    call_full(context, host, kind, a, 0, &[]).result
}

/// The orders every action's control clears.
fn clear_move_orders(m: &dyn AiMemory, actor: u32) {
    m.set_bool(actor + a::ORDERS_MOVE_MOVING_CROUCH as u32, false);
    m.set_bool(actor + a::ORDERS_MOVE_PANICKED as u32, false);
    m.set_bool(actor + a::ORDERS_MOVE_DIVE_INTO_COVER as u32, false);
    m.set_bool(actor + a::ORDERS_MOVE_EMERGE_FROM_COVER as u32, false);
}

/// action_sleep.c's `action_sleep_control` (its constant names the
/// persistent control ticks; the offset is the look orders' idle look type).
fn sleep_control(context: &AiStepContext, host: &impl AiHost) {
    host.memory().set_i16(context.actor + a::ORDERS_LOOK_IDLE_LOOK_TYPE as u32, 0);
}

/// `action_wait_perform`.
fn wait_perform(context: &AiStepContext, host: &impl AiHost) -> bool {
    let m = host.memory();
    let at = |offset: usize| context.actor + offset as u32;
    if m.bool(at(a::META_TIMESLICE)) {
        m.set_bool(at(a::STATE_WAIT_DESIRE_MOVE), false);
        call_simple(context, host, wait_call::FIND_NEARBY_ACTORS, m.r8(at(a::EXTERNAL_ORDERS_PURSUIT_IS_COORDINATOR)) as i32);
        let pursuit_prop_index = m.i32(at(a::EXTERNAL_ORDERS_PURSUIT_GROUP_PROP_INDEX));
        if m.bool(at(a::STATE_WAIT_WAITING_AS_COORDINATOR)) {
            if pursuit_prop_index == NONE {
                if m.i16(at(a::STATE_WAIT_EXIT_TIMER)) == 0 {
                    m.set_i16(at(a::STATE_WAIT_EXIT_TIMER), 150);
                }
            } else if context.game_time >= m.i32(at(a::STATE_WAIT_ENTRY_TIME)).wrapping_add(2700) {
                m.set_bool(at(a::STATE_WAIT_WAIT_DONE), true);
            }
        } else {
            m.set_bool(at(a::STATE_WAIT_WAIT_DONE), true);
            if pursuit_prop_index != NONE {
                let prop = prop_get(m, context, pursuit_prop_index);
                let move_failed = m.bool(at(a::STATE_WAIT_MOVE_FAILED));
                if (!m.bool(at(a::STATE_WAIT_WAS_ACTIVELY_SEARCHING)) || move_failed)
                    && (m.i16(prop + p::VISIBILITY as u32) < 2 || !(m.f32(prop + p::DISTANCE as u32) < 8.0))
                {
                    m.set_bool(at(a::STATE_WAIT_WAIT_DONE), true);
                } else {
                    m.set_bool(at(a::STATE_WAIT_DESIRE_MOVE), !move_failed && m.f32(prop + p::DISTANCE as u32) > 3.5);
                    m.set_bool(at(a::STATE_WAIT_WAIT_DONE), false);
                }
            }
        }
        if !m.bool(at(a::META_SWARM)) {
            if m.bool(at(a::STATE_WAIT_DESIRE_MOVE)) {
                let moved = call_full(context, host, wait_call::MOVE_TO_PROP, pursuit_prop_index, 0, &[]).result != 0;
                if !moved {
                    m.set_bool(at(a::STATE_WAIT_MOVE_FAILED), true);
                }
            } else {
                call_simple(context, host, wait_call::MOVE_HALT, 0);
            }
        }
    }
    m.bool(at(a::STATE_WAIT_WAIT_DONE))
}

/// `action_wait_control`.
fn wait_control(context: &AiStepContext, host: &impl AiHost) {
    let m = host.memory();
    let at = |offset: usize| context.actor + offset as u32;
    let pursuit_prop_index = m.i32(at(a::EXTERNAL_ORDERS_PURSUIT_GROUP_PROP_INDEX));
    if m.bool(at(a::CONTROL_MOVING)) {
        m.set_i16(at(a::ORDERS_LOOK_PRIMARY_PRIORITY), 3);
        m.set_i16(at(a::ORDERS_LOOK_PRIMARY_DIRECTION_TYPE), v::_DIRECTION_SPECIFICATION_MOVEMENT as i16);
    } else if !m.bool(at(a::EXTERNAL_ORDERS_PURSUIT_IS_COORDINATOR))
        && pursuit_prop_index != NONE
        && m.i16(at(a::STATE_WAIT_LOOK_TIMER)) > 0
    {
        m.set_i16(at(a::ORDERS_LOOK_PRIMARY_PRIORITY), 5);
        m.set_i16(at(a::ORDERS_LOOK_PRIMARY_DIRECTION_TYPE), v::_DIRECTION_SPECIFICATION_PROP as i16);
        m.w32(at(a::ORDERS_LOOK_PRIMARY_DIRECTION_TYPE) + 4, pursuit_prop_index as u32);
    } else {
        m.set_i16(at(a::ORDERS_LOOK_PRIMARY_PRIORITY), 1);
    }
    m.set_i16(at(a::ORDERS_LOOK_IDLE_LOOK_TYPE), 3);
    m.set_bool(at(a::ORDERS_COMBAT_SHOOT_AT_TARGET), false);
    m.set_bool(at(a::ORDERS_MOVE_STATIONARY_CROUCH), false);
    clear_move_orders(m, context.actor);
}

/// `action_wait_update`.
fn wait_update(context: &AiStepContext, host: &impl AiHost) {
    let m = host.memory();
    let at = |offset: usize| context.actor + offset as u32;
    let unit_index = m.i32(at(a::META_UNIT_INDEX));
    let query_timer = at(a::STATE_WAIT_QUERY_TIMER);
    if m.i16(query_timer) > 0 {
        m.set_i16(query_timer, m.i16(query_timer) - 1);
        if m.i16(query_timer) == 0 {
            if unit_index != NONE {
                call_simple(context, host, wait_call::COMMUNICATION, 17);
            }
            m.set_i16(query_timer, call_simple(context, host, wait_call::QUERY_TIME, 0) as i16);
        }
    }
    let exit_timer = at(a::STATE_WAIT_EXIT_TIMER);
    if m.i16(exit_timer) > 0 {
        m.set_i16(exit_timer, m.i16(exit_timer) - 1);
        if m.i16(exit_timer) == 0 {
            if m.bool(at(a::STATE_WAIT_WAITING_AS_COORDINATOR)) && unit_index != NONE {
                call_simple(context, host, wait_call::COMMUNICATION, 20);
            }
            m.set_bool(at(a::STATE_WAIT_WAIT_DONE), true);
        }
    }
    let look_timer = at(a::STATE_WAIT_LOOK_TIMER);
    if !m.bool(at(a::STATE_WAIT_DESIRE_MOVE)) && m.i16(look_timer) > 0 {
        m.set_i16(look_timer, m.i16(look_timer) - 1);
    }
}

/// `action_avoid_perform`.
fn avoid_perform(context: &AiStepContext, host: &impl AiHost) -> bool {
    let m = host.memory();
    if m.bool(context.actor + a::META_TIMESLICE as u32) {
        call_simple(context, host, avoid_call::CHANGE_FIRING_POSITION, 0);
    }
    m.i16(context.actor + a::DANGER_ZONE_DANGER_TYPE as u32) == 0
}

/// `action_avoid_control`.
fn avoid_control(context: &AiStepContext, host: &impl AiHost) {
    let m = host.memory();
    let at = |offset: usize| context.actor + offset as u32;
    let direction = at(a::ORDERS_LOOK_PRIMARY_DIRECTION_TYPE);
    if m.i16(at(a::TARGET_TARGET_TYPE)) >= v::_ACTOR_TARGET_UNINSPECTED_ORPHAN as i16 {
        m.set_bool(at(a::ORDERS_COMBAT_SHOOT_AT_TARGET), true);
        m.set_i16(at(a::ORDERS_LOOK_PRIMARY_PRIORITY), 7);
        m.set_i16(direction, v::_DIRECTION_SPECIFICATION_TARGET as i16);
    } else {
        m.set_i16(at(a::ORDERS_LOOK_PRIMARY_PRIORITY), 5);
        if m.i16(at(a::DANGER_ZONE_DANGER_TYPE)) > 0 {
            m.set_i16(direction, v::_DIRECTION_SPECIFICATION_DANGER as i16);
        } else {
            m.set_i16(direction, v::_DIRECTION_SPECIFICATION_TARGET as i16);
        }
    }
    m.set_i16(at(a::ORDERS_LOOK_IDLE_LOOK_TYPE), 4);
    m.w8(at(a::ORDERS_MOVE_STATIONARY_CROUCH), m.r8(at(a::EMOTIONS_DEFENSIVE_CROUCH)));
    clear_move_orders(m, context.actor);
}

/// `action_fight_update`.
fn fight_update(context: &AiStepContext, host: &impl AiHost) {
    let m = host.memory();
    let at = |offset: usize| context.actor + offset as u32;
    let timer = at(a::STATE_FIGHT_FIRING_POSITION_TIMER);
    if m.i16(timer) > 0 && m.bool(at(a::CONTROL_PATH_AT_DESTINATION)) {
        m.set_i16(timer, m.i16(timer) - 1);
        let position_index = m.i16(at(a::FIRING_POSITIONS_CURRENT_POSITION_INDEX));
        if m.i16(timer) == 0
            && position_index != NONE as i16
            && !m.bool(at(a::FIRING_POSITIONS_CURRENT_POSITION_FOUND_OUTSIDE_RANGE))
        {
            call_simple(context, host, fight_call::DISCARD_FIRING_POSITION, position_index as i32);
        }
    }
}

/// `action_fight_control`.
fn fight_control(context: &AiStepContext, host: &impl AiHost) {
    let m = host.memory();
    let at = |offset: usize| context.actor + offset as u32;
    m.set_i16(at(a::ORDERS_LOOK_PRIMARY_PRIORITY), 5);
    m.set_i16(at(a::ORDERS_LOOK_PRIMARY_DIRECTION_TYPE), 2);
    m.set_i16(at(a::ORDERS_LOOK_IDLE_LOOK_TYPE), 4);
    m.w8(at(a::ORDERS_MOVE_STATIONARY_CROUCH), m.r8(at(a::EMOTIONS_DEFENSIVE_CROUCH)));
    clear_move_orders(m, context.actor);
    if m.i16(at(a::INPUT_VEHICLE_DRIVER_TYPE)) != 4 && m.i16(at(a::STATE_COMBAT_STATUS)) >= 5 {
        m.set_bool(at(a::ORDERS_COMBAT_SHOOT_AT_TARGET), true);
        m.set_i16(at(a::ORDERS_LOOK_PRIMARY_PRIORITY), 7);
    }
}

/// `(short)value` through a long, as x86 converts it.
fn c_short(value: f32) -> i16 {
    let long = if (-2_147_483_648.0..2_147_483_648.0).contains(&value) { value as i32 } else { i32::MIN };
    long as i16
}

/// `action_fight_perform`.
fn fight_perform(context: &AiStepContext, host: &impl AiHost) -> bool {
    let m = host.memory();
    let at = |offset: usize| context.actor + offset as u32;
    if !m.bool(at(a::META_TIMESLICE)) {
        return false;
    }
    let definition = context.definition;
    let firing_variant = call_simple(context, host, fight_call::FIRING_VARIANT, 0) as u32;

    'unreachable: {
        if m.bool(at(a::INPUT_VEHICLE_PASSENGER)) {
            break 'unreachable;
        }
        if m.bool(at(a::EMOTIONS_DEFENSIVE_CROUCH)) && m.r32(definition + d::FLAGS as u32) & (1 << 5) != 0 {
            call_simple(context, host, fight_call::FIND_PATHFINDING_LOCATION, 0);
            if call_simple(context, host, fight_call::NEARBY_FIRING_POSITIONS, 0) != 0 {
                let near_current_firing_position = m.i32(at(a::META_ENCOUNTER_INDEX)) != NONE
                    && m.i16(at(a::FIRING_POSITIONS_CURRENT_POSITION_INDEX)) != NONE as i16
                    && call_simple(context, host, fight_call::NEAR_CURRENT_FIRING_POSITION, 0) != 0;
                if m.bool(at(a::CONTROL_MOVING))
                    && m.r8(at(a::SITUATION_CUMULATIVE_THREATS) + 5) == 0
                {
                    let target_prop_index = m.i32(at(a::TARGET_TARGET_PROP_INDEX));
                    if target_prop_index == NONE {
                        break 'unreachable;
                    }
                    let prop = prop_get(m, context, target_prop_index);
                    if !(m.f32(prop + p::DISTANCE as u32)
                        < m.f32(firing_variant + var::RANGED_COMBAT_COMBAT_RANGE_UPPER_BOUND as u32))
                    {
                        break 'unreachable;
                    }
                } else if near_current_firing_position {
                    break 'unreachable;
                }
                m.set_i16(at(a::FIRING_POSITIONS_CURRENT_POSITION_INDEX), NONE as i16);
                call_simple(context, host, fight_call::MOVE_HALT, 0);
                break 'unreachable;
            }
        }

        let old_firing_position_index = m.i16(at(a::FIRING_POSITIONS_CURRENT_POSITION_INDEX));
        let new_firing_position_index = call_simple(context, host, fight_call::CHANGE_FIRING_POSITION, 0) as i16;
        let timer = at(a::STATE_FIGHT_FIRING_POSITION_TIMER);
        if new_firing_position_index == NONE as i16 {
            m.set_i16(timer, 0);
        } else if new_firing_position_index != old_firing_position_index {
            let upper = m.f32(definition + d::FIRING_POSITION_COMBAT_POSITION_TIME_UPPER_BOUND as u32);
            let lower = m.f32(definition + d::FIRING_POSITION_COMBAT_POSITION_TIME_LOWER_BOUND as u32);
            let mut time = call_full(context, host, fight_call::RANDOM_RANGE, 0, 0, &[lower, upper]).output[0];
            if m.i16(at(a::INPUT_VEHICLE_DRIVER_TYPE)) > 0 {
                let minimum = call_full(context, host, fight_call::VEHICLE_MINIMUM_FIRING_POSITION_TIME, 0, 0, &[]).output[0];
                if minimum > 0.0 && time > minimum {
                    time = minimum;
                }
            }
            m.set_i16(timer, c_short(time * 30.0));
        }
    }

    // update_unreachable:
    if m.i16(at(a::TARGET_TARGET_TYPE)) >= 7 {
        let target_prop_index = m.i32(at(a::TARGET_TARGET_PROP_INDEX));
        let prop = prop_get(m, context, target_prop_index);
        let mut unreachable = true;
        if call_simple(context, host, fight_call::HAS_RANGED_WEAPON, 0) != 0 {
            if m.f32(prop + p::DISTANCE as u32) < m.f32(at(a::CONTROL_WEAPON_MAXIMUM_RANGE)) {
                unreachable = false;
            } else if m.i32(at(a::META_ENCOUNTER_INDEX)) != NONE
                && m.i16(at(a::FIRING_POSITIONS_CURRENT_POSITION_INDEX)) != NONE as i16
                && call_simple(context, host, fight_call::FIRING_POSITION_REACHES, target_prop_index) != 0
            {
                unreachable = false;
            }
        }
        call_full(context, host, fight_call::PERCEPTION_UNREACHABLE, target_prop_index, unreachable as i32, &[]);
    }
    false
}

/// action_alert.c's calls.
pub mod alert_call {
    const BASE: i32 = (crate::layout::ai_values::_ACTOR_ACTION_ALERT as i32) << 8;
    /// `actor_path_at_destination(actor)`, the result.
    pub const AT_DESTINATION: i32 = BASE | 1;
    /// The target position's animation (its reference `a`) started on the unit.
    pub const PLAY_ANIMATION: i32 = BASE | 2;
    /// `actor_path_has_path(actor)`, the result.
    pub const HAS_PATH: i32 = BASE | 3;
    /// `actor_destination_tolerance(actor)`, in `output[0]`.
    pub const TOLERANCE: i32 = BASE | 4;
    /// The squad has more move positions than are used (`a`): its error, once.
    pub const TOO_MANY_POSITIONS: i32 = BASE | 5;
    /// `choose_random_array_element` of the squad's first `b` move positions
    /// by weight, those of the mask `a` unavailable: the result.
    pub const CHOOSE_RANDOM_POSITION: i32 = BASE | 6;
    /// `real_random_range(input[0], input[1])`, in `output[0]`.
    pub const RANDOM_RANGE: i32 = BASE | 7;
    /// `actor_move_to_move_position(actor, a)`, the result.
    pub const MOVE_TO_MOVE_POSITION: i32 = BASE | 8;
}

/// The local enum (action_alert.c's).
const MOVE_POSITION_ORDER_NONE: i16 = 0;
const MOVE_POSITION_ORDER_REPEAT: i16 = 1;
const MOVE_POSITION_ORDER_LOOP: i16 = 2;
const MOVE_POSITION_ORDER_LOOP_BACK_AND_FORTH: i16 = 3;
const MOVE_POSITION_ORDER_LOOP_RANDOMLY: i16 = 4;
const MOVE_POSITION_ORDER_RANDOM: i16 = 5;

/// `distance_squared3d(a, b)`.
fn distance_squared(m: &dyn AiMemory, a: u32, b: u32) -> f32 {
    let (a, b) = (m.vector(a), m.vector(b));
    let v = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    v[0] * v[0] + v[1] * v[1] + v[2] * v[2]
}

/// `action_alert_control`.
fn alert_control(context: &AiStepContext, host: &impl AiHost) {
    let m = host.memory();
    let at = |offset: usize| context.actor + offset as u32;
    m.set_i16(at(a::ORDERS_LOOK_IDLE_LOOK_TYPE), 1);
    if m.r32(context.definition + d::FLAGS as u32) & (1 << v::_ACTOR_DEFINITION_CROUCH_WHEN_NONCOMBAT_BIT) != 0 {
        m.set_bool(at(a::ORDERS_MOVE_STATIONARY_CROUCH), true);
        m.set_bool(at(a::ORDERS_MOVE_MOVING_CROUCH), true);
    }
}

/// `action_alert_update`.
fn alert_update(context: &AiStepContext, host: &impl AiHost) {
    use crate::layout::move_position_definition as mp;
    let m = host.memory();
    let at = |offset: usize| context.actor + offset as u32;
    let target_position = at(a::STATE_ALERT_TARGET_POSITION_DEFINITION);
    if m.bool(at(a::META_DORMANT)) || m.i16(at(a::STATE_ALERT_TARGET_MOVE_POSITION_INDEX)) == NONE as i16 {
        return;
    }
    let distance = distance_squared(m, at(a::INPUT_BODY_POSITION), target_position + mp::POSITION as u32);
    if call_simple(context, host, alert_call::AT_DESTINATION, 0) != 0 || distance < 0.25 {
        let wait_ticks = at(a::STATE_ALERT_WAIT_TICKS);
        if m.i16(wait_ticks) > 0 {
            m.set_i16(wait_ticks, m.i16(wait_ticks) - 1);
        }
        if m.bool(at(a::STATE_ALERT_MUST_PLAY_ANIMATION)) {
            let reference = m.i16(target_position + mp::ANIMATION_REFERENCE_INDEX as u32);
            if reference != NONE as i16 {
                call_simple(context, host, alert_call::PLAY_ANIMATION, reference as i32);
            }
            m.set_bool(at(a::STATE_ALERT_MUST_PLAY_ANIMATION), false);
        }
    }
}

/// `action_alert_next_position`: the next move position, or NONE.
fn alert_next_position(context: &AiStepContext, host: &impl AiHost, order: i16, current: i16) -> i16 {
    use crate::ai_actor::{PropIterator, squad_definition, tag_block_count, tag_block_element};
    use crate::layout::{move_position_definition as mp, squad_definition as squad};
    let m = host.memory();
    let at = |offset: usize| context.actor + offset as u32;
    let encounter_index = m.i32(at(a::META_ENCOUNTER_INDEX));
    if m.bool(at(a::INPUT_VEHICLE_PASSENGER)) || order == MOVE_POSITION_ORDER_NONE || encounter_index == NONE {
        return NONE as i16;
    }
    if order == MOVE_POSITION_ORDER_REPEAT && current != NONE as i16 {
        return current;
    }
    let squad = squad_definition(m, context, encounter_index, m.i16(at(a::META_SQUAD_INDEX)));
    let block = squad + squad::MOVE_POSITIONS as u32;
    let block_count = tag_block_count(m, block);
    let count = block_count.clamp(0, 32) as i16;
    if (count as i32) < block_count {
        call_simple(context, host, alert_call::TOO_MANY_POSITIONS, count as i32);
    }

    let mut unavailable: u32 = 0;
    let mut any_available = false;
    for index in 0..count {
        let definition = tag_block_element(m, block, index as i32, mp::SIZEOF);
        let position = definition + mp::POSITION as u32;
        let mut available = index != current;
        if current != NONE as i16 && distance_squared(m, at(a::INPUT_BODY_POSITION), position) < 0.25 {
            available = false;
        }
        let sequence_id = m.i8(definition + mp::SEQUENCE_ID as u32);
        if sequence_id != 0 && sequence_id != m.i8(at(a::STATE_NONCOMBAT_SEQUENCE_ID)) {
            available = false;
        }
        let mut iterator = PropIterator::new(m, context);
        while let Some(prop) = iterator.next(m, context) {
            let state = m.i16(prop + p::STATE as u32);
            if state >= v::_PROP_STATE_BECOMING_UNACKNOWLEDGED as i16
                && state <= v::_PROP_STATE_ACKNOWLEDGED as i16
                && distance_squared(m, prop + p::BODY_POSITION as u32, position) < 0.25
            {
                available = false;
                break;
            }
        }
        if available {
            any_available = true;
        } else {
            unavailable |= 1 << index;
        }
    }
    if !any_available {
        return NONE as i16;
    }
    if order == MOVE_POSITION_ORDER_RANDOM {
        return call_full(context, host, alert_call::CHOOSE_RANDOM_POSITION, unavailable as i32, count as i32, &[]).result
            as i16;
    }

    let increasing_address = at(a::STATE_ALERT_MOVE_POSITION_INCREASING);
    let mut next = if current < 0 || current >= count { 0 } else { current };
    loop {
        let increasing = match order {
            MOVE_POSITION_ORDER_LOOP => true,
            MOVE_POSITION_ORDER_LOOP_BACK_AND_FORTH => {
                if next == 0 {
                    true
                } else if next == count - 1 {
                    false
                } else {
                    m.bool(increasing_address)
                }
            }
            MOVE_POSITION_ORDER_LOOP_RANDOMLY => context.game_time & 1 != 0,
            _ => true,
        };
        m.set_bool(increasing_address, increasing);
        if increasing {
            next += 1;
            if next >= count {
                next = 0;
            }
        } else {
            next -= 1;
            if next < 0 {
                next = count - 1;
            }
        }
        if unavailable & (1 << next) == 0 {
            break;
        }
    }
    next
}

/// `action_alert_perform`.
fn alert_perform(context: &AiStepContext, host: &impl AiHost) -> bool {
    use crate::ai_actor::{object_get, squad_definition, tag_block_count, tag_block_element};
    use crate::layout::{move_position_definition as mp, squad_definition as squad, unit_datum as u};
    let m = host.memory();
    let at = |offset: usize| context.actor + offset as u32;
    let order = m.i16(at(a::STATE_ALERT_MOVE_POSITION_ORDER));
    let target = at(a::STATE_ALERT_TARGET_MOVE_POSITION_INDEX);
    let pending = at(a::STATE_ALERT_PENDING_MOVE_POSITION_INDEX);
    let wait_ticks = at(a::STATE_ALERT_WAIT_TICKS);
    let must_play = at(a::STATE_ALERT_MUST_PLAY_ANIMATION);
    let target_position = at(a::STATE_ALERT_TARGET_POSITION_DEFINITION);

    if order != MOVE_POSITION_ORDER_NONE && m.i16(pending) == NONE as i16 {
        let mut reached = true;
        if m.i16(target) != NONE as i16 && call_simple(context, host, alert_call::HAS_PATH, 0) != 0 {
            let distance = distance_squared(m, at(a::INPUT_BODY_POSITION), target_position + mp::POSITION as u32);
            let mut tolerance = call_full(context, host, alert_call::TOLERANCE, 0, 0, &[]).output[0];
            tolerance = if tolerance > 0.5 { tolerance } else { 0.5 };
            if distance > tolerance * tolerance {
                reached = false;
            }
        }
        if reached {
            let unit = object_get(m, context, m.i32(at(a::META_UNIT_INDEX))).unwrap_or(0);
            if m.i16(wait_ticks) > 0
                || m.bool(must_play)
                || m.i8(unit + u::UNIT_ANIMATION_STATE as u32) as i32 == v::_UNIT_STATE_USER_ANIMATION as i32
            {
                reached = false;
            }
        }
        if reached {
            let next = alert_next_position(context, host, order, m.i16(target));
            m.set_i16(pending, next);
        }
    }

    if m.bool(at(a::META_TIMESLICE)) && !m.bool(at(a::META_DORMANT)) && m.i16(pending) != NONE as i16 {
        let mut moved = false;
        let encounter_index = m.i32(at(a::META_ENCOUNTER_INDEX));
        if encounter_index != NONE {
            let squad = squad_definition(m, context, encounter_index, m.i16(at(a::META_SQUAD_INDEX)));
            let block = squad + squad::MOVE_POSITIONS as u32;
            let pending_index = m.i16(pending);
            if pending_index >= 0 && (pending_index as i32) < tag_block_count(m, block) {
                let position = tag_block_element(m, block, pending_index as i32, mp::SIZEOF);
                let wait_time = call_full(
                    context,
                    host,
                    alert_call::RANDOM_RANGE,
                    0,
                    0,
                    &[m.f32(position + mp::TIME_LOWER_BOUND as u32), m.f32(position + mp::TIME_UPPER_BOUND as u32)],
                )
                .output[0];
                m.set_i16(target, pending_index);
                m.set_i16(pending, NONE as i16);
                for offset in 0..mp::SIZEOF as u32 {
                    m.w8(target_position + offset, m.r8(position + offset));
                }
                m.set_i16(wait_ticks, c_short(wait_time * 30.0));
                m.set_bool(must_play, true);
                moved = call_simple(context, host, alert_call::MOVE_TO_MOVE_POSITION, pending_index as i32) != 0;
            }
        }
        if !moved {
            m.set_i16(target, m.i16(pending));
            m.set_i16(pending, NONE as i16);
            m.set_i16(wait_ticks, 0);
            m.set_bool(must_play, false);
        }
    }
    false
}

/// action_search.c's calls.
pub mod search_call {
    const BASE: i32 = (crate::layout::ai_values::_ACTOR_ACTION_SEARCH as i32) << 8;
    /// `ai_communication_event(a, unit, the target's unit, NONE...)`.
    pub const COMMUNICATION: i32 = BASE | 1;
    /// Whether the actor's head sees the head it would have at the pursuit
    /// location (`unit_estimate_position`, `ai_test_line_of_sight` clear),
    /// the result.
    pub const SEES_PURSUIT_LOCATION: i32 = BASE | 2;
    /// `actors_searching_same_position(actor, a)`, the result.
    pub const SEARCHING_SAME_POSITION: i32 = BASE | 3;
    /// `encounter_mark_examined_pursuit_position(encounter, actor, a, b)`.
    pub const MARK_EXAMINED: i32 = BASE | 4;
    /// `actor_move_halt(actor)`.
    pub const MOVE_HALT: i32 = BASE | 5;
    /// `actor_move_to_prop(actor, a, 2.5)`, the result.
    pub const MOVE_TO_PROP: i32 = BASE | 6;
    /// `actor_move_to_firing_position(actor, a, NULL)`, the result.
    pub const MOVE_TO_FIRING_POSITION: i32 = BASE | 7;
}

/// action_uncover.c's calls.
pub mod uncover_call {
    const BASE: i32 = (crate::layout::ai_values::_ACTOR_ACTION_UNCOVER as i32) << 8;
    /// `actor_active_select_firing_position` for uncovering (its candidate
    /// kept for [`CHANGE_FIRING_POSITION`]): the index the result, the
    /// candidate's line of sight in `result2`, its path distance in `output[0]`.
    pub const SELECT_FIRING_POSITION: i32 = BASE | 1;
    /// `actor_destination_tolerance(actor)`, in `output[0]`.
    pub const TOLERANCE: i32 = BASE | 2;
    /// A debug message: the target's location can't be seen (`a` 0), or the
    /// pursuit location was inspected (`a` 1).
    pub const DEBUG: i32 = BASE | 3;
    /// `actor_change_firing_position` to the selected one: the result.
    pub const CHANGE_FIRING_POSITION: i32 = BASE | 4;
    /// `actor_discard_firing_position(actor, a, FALSE)`.
    pub const DISCARD_FIRING_POSITION: i32 = BASE | 5;
    /// The uncover's done (a debug message, its reason from the actor).
    pub const DONE: i32 = BASE | 6;
}

const PURSUIT_LOCATION_TARGET: i16 = 0;
const PURSUIT_LOCATION_POSITION: i16 = 1;

fn defensive_crouch_type(m: &dyn AiMemory, context: &AiStepContext) -> i16 {
    m.i16(context.definition + d::DEFENSIVE as u32 + crate::layout::actor_defensive_properties::DEFENSIVE_CROUCH_TYPE as u32)
}

fn definition_flag(m: &dyn AiMemory, context: &AiStepContext, bit: usize) -> bool {
    m.r32(context.definition + d::FLAGS as u32) & (1 << bit) != 0
}

/// `action_search_update`.
fn search_update(context: &AiStepContext, host: &impl AiHost) {
    let m = host.memory();
    let at = |offset: usize| context.actor + offset as u32;
    if m.bool(at(a::STATE_SEARCH_SEARCH_DONE)) {
        return;
    }
    let pursuit_type = m.i16(at(a::STATE_SEARCH_PURSUIT_TYPE));
    let sneaking = if defensive_crouch_type(m, context) == 4 {
        true
    } else {
        definition_flag(m, context, v::_ACTOR_DEFINITION_SNEAK_UNCOVERING_TARGET_BIT)
            && pursuit_type == 0
            && m.i16(at(a::TARGET_TARGET_TYPE)) == 5
            && m.i8(prop_get(m, context, m.i32(at(a::TARGET_TARGET_PROP_INDEX))) + p::QUANTIZED_DISTANCE as u32) <= 2
    };
    m.set_bool(at(a::STATE_SEARCH_SNEAKING), sneaking);

    let remaining = at(a::STATE_SEARCH_SEARCH_REMAINING_TIME);
    if m.bool(at(a::STATE_SEARCH_AT_DESTINATION)) {
        if m.i32(remaining) > 0 {
            m.w32(remaining, (m.i32(remaining) - 1) as u32);
        }
        if m.i32(remaining) == 0 {
            m.set_bool(at(a::STATE_SEARCH_SEARCH_DONE), true);
        }
        if m.i32(at(a::META_UNIT_INDEX)) != NONE {
            if pursuit_type == 0 {
                let communicated = at(a::FIRING_POSITIONS_PURSUIT_COMMUNICATED_LOST_CONTACT);
                if !m.bool(communicated)
                    && (m.bool(at(a::STATE_SEARCH_SEARCH_DONE))
                        || m.i32(remaining).wrapping_add(90) < m.i32(at(a::STATE_SEARCH_SEARCH_DESIRED_TIME)))
                {
                    call_simple(context, host, search_call::COMMUNICATION, 13);
                    m.set_bool(communicated, true);
                }
            } else if m.i32(remaining) == 0 {
                call_simple(context, host, search_call::COMMUNICATION, 18);
            }
        }
    } else if !m.bool(at(a::CONTROL_MOVING)) && !m.bool(at(a::META_SWARM)) {
        let timer = at(a::STATE_SEARCH_SEARCH_FAILURE_TIMER);
        m.w32(timer, m.r32(timer).wrapping_add(1));
        if m.i32(timer) > 120 {
            m.set_bool(at(a::STATE_SEARCH_SEARCH_FAILED), true);
            m.set_bool(at(a::STATE_SEARCH_SEARCH_DONE), true);
        }
    }
}

/// `action_search_control`.
fn search_control(context: &AiStepContext, host: &impl AiHost) {
    let m = host.memory();
    let at = |offset: usize| context.actor + offset as u32;
    let pursuit_type = m.i16(at(a::STATE_SEARCH_PURSUIT_TYPE));
    let priority = at(a::ORDERS_LOOK_PRIMARY_PRIORITY);
    let direction = at(a::ORDERS_LOOK_PRIMARY_DIRECTION_TYPE);
    if m.bool(at(a::CONTROL_MOVING)) {
        m.set_i16(priority, 3);
        m.set_i16(direction, 0);
    } else {
        let desired = m.i32(at(a::STATE_SEARCH_SEARCH_DESIRED_TIME));
        let minimum = (desired / 3).max(90);
        if desired.wrapping_sub(m.i32(at(a::STATE_SEARCH_SEARCH_REMAINING_TIME))) < minimum {
            if pursuit_type == 0 {
                m.set_i16(priority, 3);
                m.set_i16(direction, 2);
            } else if pursuit_type == 1 {
                m.set_i16(priority, 3);
                m.set_i16(direction, 3);
                for word in 0..3 {
                    m.w32(direction + 4 + word * 4, m.r32(at(a::STATE_SEARCH_PURSUIT_POSITION) + word * 4));
                }
            } else {
                m.set_i16(priority, 1);
            }
        } else {
            m.set_i16(priority, 1);
        }
    }
    m.set_i16(at(a::ORDERS_LOOK_IDLE_LOOK_TYPE), 3);
    if pursuit_type == 0 {
        let minimum_type = if definition_flag(m, context, v::_ACTOR_DEFINITION_SHOOT_AT_TARGETS_LAST_LOCATION_BIT) { 5 } else { 6 };
        m.set_bool(at(a::ORDERS_COMBAT_SHOOT_AT_TARGET), m.i16(at(a::TARGET_TARGET_TYPE)) >= minimum_type);
    }
    let sneaking = m.r8(at(a::STATE_SEARCH_SNEAKING));
    m.w8(at(a::ORDERS_MOVE_STATIONARY_CROUCH), sneaking);
    m.w8(at(a::ORDERS_MOVE_MOVING_CROUCH), sneaking);
    m.set_bool(at(a::ORDERS_MOVE_PANICKED), false);
    m.set_bool(at(a::ORDERS_MOVE_DIVE_INTO_COVER), false);
    m.set_bool(at(a::ORDERS_MOVE_EMERGE_FROM_COVER), true);
}

/// `action_search_perform`.
fn search_perform(context: &AiStepContext, host: &impl AiHost) -> bool {
    use crate::ai_actor::{PropIterator, datum_get};
    let m = host.memory();
    let at = |offset: usize| context.actor + offset as u32;
    let done = at(a::STATE_SEARCH_SEARCH_DONE);
    let at_destination = at(a::STATE_SEARCH_AT_DESTINATION);
    if m.bool(at(a::META_SWARM)) || !m.bool(at(a::META_TIMESLICE)) || m.bool(done) {
        return m.bool(done);
    }
    m.set_bool(at_destination, true);
    let pursuit_type = m.i16(at(a::STATE_SEARCH_PURSUIT_TYPE));
    let pursuit_index = m.i16(at(a::STATE_SEARCH_PURSUIT_FIRING_POSITION_INDEX));
    let target_prop_index = m.i32(at(a::TARGET_TARGET_PROP_INDEX));
    if pursuit_type == 0 && target_prop_index != NONE {
        let prop = prop_get(m, context, target_prop_index);
        let radius: f32 = if m.i16(prop + p::LINE_OF_SIGHT as u32) != 0 { 0.7 } else { 1.7 };
        let reached = distance_squared(m, prop + p::BODY_POSITION as u32, at(a::INPUT_BODY_POSITION)) < radius * radius;
        m.set_bool(at_destination, reached);
    } else if pursuit_type == 1 && pursuit_index != NONE as i16 {
        let distance = distance_squared(m, at(a::STATE_SEARCH_PURSUIT_POSITION), at(a::INPUT_BODY_POSITION));
        if distance < 0.7f32 * 0.7f32 {
            m.set_bool(at_destination, true);
        } else if distance > 2.5f32 * 2.5f32 {
            m.set_bool(at_destination, false);
        } else {
            let sees = call_simple(context, host, search_call::SEES_PURSUIT_LOCATION, 0) != 0;
            m.set_bool(at_destination, sees);
        }
    }

    if !m.bool(at_destination) && !m.bool(at(a::STATE_SEARCH_CHARGING)) {
        let mut searching_actor_count: i16 = 0;
        let mut nearby_searching_actor_count: i16 = 0;
        let mut iterator = PropIterator::new(m, context);
        while let Some(prop) = iterator.next(m, context) {
            let state = m.i16(prop + p::STATE as u32);
            let other_index = m.i32(prop + p::ACTOR_INDEX as u32);
            if state >= v::_PROP_STATE_BECOMING_UNACKNOWLEDGED as i16
                && state <= v::_PROP_STATE_ACKNOWLEDGED as i16
                && !m.bool(prop + p::ENEMY as u32)
                && !m.bool(prop + p::DEAD as u32)
                && other_index != NONE
                && call_simple(context, host, search_call::SEARCHING_SAME_POSITION, other_index) != 0
            {
                let other = datum_get(m, context.actor_data, other_index).unwrap_or(0);
                searching_actor_count += 1;
                if !m.bool(other + a::META_SWARM as u32)
                    && !m.bool(other + a::CONTROL_MOVING as u32)
                    && distance_squared(m, at(a::INPUT_BODY_POSITION), other + a::INPUT_BODY_POSITION as u32)
                        < 0.8f32 * 0.8f32
                {
                    nearby_searching_actor_count += 1;
                }
            }
        }
        let needed: i16 = if pursuit_type == 1 { 2 } else { 4 };
        if !m.bool(at(a::STATE_SEARCH_TENACIOUS)) && searching_actor_count >= needed {
            m.set_bool(done, true);
            let mut last_perceived_time = NONE;
            let target_prop_index = m.i32(at(a::TARGET_TARGET_PROP_INDEX));
            if target_prop_index != NONE {
                last_perceived_time = m.i32(prop_get(m, context, target_prop_index) + p::LAST_PERCEIVED_TIME as u32);
            }
            if m.i32(at(a::META_ENCOUNTER_INDEX)) != NONE {
                call_full(
                    context,
                    host,
                    search_call::MARK_EXAMINED,
                    m.i16(at(a::STATE_SEARCH_PURSUIT_FIRING_POSITION_INDEX)) as i32,
                    last_perceived_time,
                    &[],
                );
            }
        } else if nearby_searching_actor_count > 0 {
            m.set_bool(at_destination, true);
        }
    }

    if m.bool(at_destination) {
        call_simple(context, host, search_call::MOVE_HALT, 0);
    } else if pursuit_type == 0 {
        if call_simple(context, host, search_call::MOVE_TO_PROP, m.i32(at(a::TARGET_TARGET_PROP_INDEX))) == 0 {
            m.set_bool(done, true);
            m.set_bool(at(a::STATE_SEARCH_SEARCH_FAILED), true);
        }
    } else if pursuit_type == 1 {
        m.set_i16(at(a::FIRING_POSITIONS_CURRENT_POSITION_INDEX), NONE as i16);
        let index = m.i16(at(a::STATE_SEARCH_PURSUIT_FIRING_POSITION_INDEX)) as i32;
        if call_simple(context, host, search_call::MOVE_TO_FIRING_POSITION, index) == 0 {
            m.set_bool(done, true);
            m.set_bool(at(a::STATE_SEARCH_SEARCH_FAILED), true);
        }
    } else {
        call_simple(context, host, search_call::MOVE_HALT, 0);
    }
    m.bool(done)
}

/// `action_uncover_perform`.
fn uncover_perform(context: &AiStepContext, host: &impl AiHost) -> bool {
    let m = host.memory();
    let at = |offset: usize| context.actor + offset as u32;
    if !(m.bool(at(a::META_TIMESLICE)) && !m.bool(at(a::INPUT_VEHICLE_PASSENGER)) && !m.bool(at(a::STATE_UNCOVER_UNCOVER_DONE)))
    {
        return false;
    }
    let selected = call_full(context, host, uncover_call::SELECT_FIRING_POSITION, 0, 0, &[]);
    if selected.result as i16 != NONE as i16 {
        let line_of_sight = selected.result2 as i16;
        if m.i16(at(a::STATE_UNCOVER_PURSUIT_TYPE)) == PURSUIT_LOCATION_TARGET {
            if line_of_sight != v::_AI_LINE_OF_SIGHT_CLEAR as i16 && line_of_sight != v::_AI_LINE_OF_SIGHT_OCCLUDED as i16 {
                if !m.bool(at(a::STATE_UNCOVER_NO_TARGET_SIGHT_AVAILABLE)) {
                    call_simple(context, host, uncover_call::DEBUG, 0);
                }
                m.set_bool(at(a::STATE_UNCOVER_NO_TARGET_SIGHT_AVAILABLE), true);
            }
        } else if line_of_sight == v::_AI_LINE_OF_SIGHT_CLEAR as i16
            && call_full(context, host, uncover_call::TOLERANCE, 0, 0, &[]).output[0] > selected.output[0]
        {
            if !m.bool(at(a::STATE_UNCOVER_PURSUIT_LOCATION_INSPECTED)) {
                call_simple(context, host, uncover_call::DEBUG, 1);
            }
            m.set_bool(at(a::STATE_UNCOVER_PURSUIT_LOCATION_INSPECTED), true);
        }
    }
    if call_simple(context, host, uncover_call::CHANGE_FIRING_POSITION, 0) as i16 == NONE as i16 {
        m.set_bool(at(a::STATE_UNCOVER_UNCOVER_EXIT_FAILURE), true);
    }
    false
}

/// `action_uncover_control`.
fn uncover_control(context: &AiStepContext, host: &impl AiHost) {
    let m = host.memory();
    let at = |offset: usize| context.actor + offset as u32;
    let pursuit_type = m.i16(at(a::STATE_UNCOVER_PURSUIT_TYPE));
    let target_prop_index = m.i32(at(a::TARGET_TARGET_PROP_INDEX));
    if target_prop_index != NONE {
        let prop = prop_get(m, context, target_prop_index);
        let mut force_shoot = false;
        let target_type = m.i16(at(a::TARGET_TARGET_TYPE));
        if pursuit_type == PURSUIT_LOCATION_TARGET {
            if m.bool(at(a::INPUT_VEHICLE_GUNNER_BOMBARDMENT)) {
                m.set_bool(at(a::ORDERS_COMBAT_SHOOT_AT_TARGET), true);
                m.set_bool(at(a::ORDERS_COMBAT_BOMBARD_TARGET), true);
                force_shoot = true;
            } else {
                let minimum_type =
                    if definition_flag(m, context, v::_ACTOR_DEFINITION_SHOOT_AT_TARGETS_LAST_LOCATION_BIT) { 5 } else { 6 };
                m.set_bool(at(a::ORDERS_COMBAT_SHOOT_AT_TARGET), target_type >= minimum_type);
            }
        }
        let line_of_sight = m.i16(prop + p::LINE_OF_SIGHT as u32);
        let priority = if (m.bool(at(a::ORDERS_COMBAT_SHOOT_AT_TARGET))
            && (line_of_sight == v::_AI_LINE_OF_SIGHT_CLEAR as i16 || line_of_sight == v::_AI_LINE_OF_SIGHT_OCCLUDED as i16))
            || force_shoot
        {
            7
        } else if target_type < 5 {
            3
        } else if line_of_sight != v::_AI_LINE_OF_SIGHT_FROM_COVER as i16 && line_of_sight != v::_AI_LINE_OF_SIGHT_OBSTRUCTED as i16 {
            5
        } else {
            2
        };
        m.set_i16(at(a::ORDERS_LOOK_PRIMARY_PRIORITY), priority);
        let direction = at(a::ORDERS_LOOK_PRIMARY_DIRECTION_TYPE);
        if pursuit_type == PURSUIT_LOCATION_TARGET {
            m.set_i16(direction, 2);
        } else if pursuit_type == PURSUIT_LOCATION_POSITION {
            m.set_i16(direction, 3);
            for word in 0..3 {
                m.w32(direction + 4 + word * 4, m.r32(at(a::STATE_UNCOVER_PURSUIT_POSITION) + word * 4));
            }
        }
    }
    m.set_i16(at(a::ORDERS_LOOK_IDLE_LOOK_TYPE), 3);
    let sneaking = m.r8(at(a::STATE_UNCOVER_SNEAKING));
    m.w8(at(a::ORDERS_MOVE_STATIONARY_CROUCH), sneaking);
    m.w8(at(a::ORDERS_MOVE_MOVING_CROUCH), sneaking);
    m.set_bool(at(a::ORDERS_MOVE_PANICKED), false);
    m.set_bool(at(a::ORDERS_MOVE_DIVE_INTO_COVER), false);
    m.set_bool(at(a::ORDERS_MOVE_EMERGE_FROM_COVER), true);
}

/// `action_uncover_update`.
fn uncover_update(context: &AiStepContext, host: &impl AiHost) {
    let m = host.memory();
    let at = |offset: usize| context.actor + offset as u32;
    if m.bool(at(a::STATE_UNCOVER_UNCOVER_DONE)) {
        return;
    }
    let pursuit_type = m.i16(at(a::STATE_UNCOVER_PURSUIT_TYPE));
    let target_prop_index = m.i32(at(a::TARGET_TARGET_PROP_INDEX));
    let target_type = m.i16(at(a::TARGET_TARGET_TYPE));
    let mut done;
    let mut target_visible = false;
    let sneaking = at(a::STATE_UNCOVER_SNEAKING);
    m.set_bool(sneaking, false);
    if pursuit_type == PURSUIT_LOCATION_TARGET {
        if defensive_crouch_type(m, context) == 4 {
            m.set_bool(sneaking, target_type != 6);
        } else if definition_flag(m, context, v::_ACTOR_DEFINITION_SNEAK_UNCOVERING_TARGET_BIT)
            && target_type == 5
            && m.i8(prop_get(m, context, target_prop_index) + p::QUANTIZED_DISTANCE as u32) <= 2
        {
            m.set_bool(sneaking, true);
        }
    } else if pursuit_type == PURSUIT_LOCATION_POSITION
        && (defensive_crouch_type(m, context) == 4
            || (definition_flag(m, context, v::_ACTOR_DEFINITION_SNEAK_UNCOVERING_PURSUIT_POSITION_BIT)
                && distance_squared(m, at(a::INPUT_BODY_POSITION), at(a::STATE_UNCOVER_PURSUIT_POSITION)) < 100.0))
    {
        m.set_bool(sneaking, true);
    }

    let hold_timer = at(a::STATE_UNCOVER_CURRENT_POSITION_HOLD_TIMER);
    if m.bool(at(a::CONTROL_MOVING)) {
        m.w32(hold_timer, 0);
    } else {
        m.w32(hold_timer, m.r32(hold_timer).wrapping_add(1));
        if pursuit_type == PURSUIT_LOCATION_TARGET && m.i32(hold_timer) >= 30 {
            let index = m.i16(at(a::FIRING_POSITIONS_CURRENT_POSITION_INDEX)) as i32;
            call_simple(context, host, uncover_call::DISCARD_FIRING_POSITION, index);
        }
    }

    if pursuit_type == PURSUIT_LOCATION_TARGET {
        done = true;
        if target_prop_index != NONE {
            target_visible = m.i16(prop_get(m, context, target_prop_index) + p::VISIBILITY as u32) > 0;
            done = !(target_visible && target_type < 5);
        }
    } else {
        done = !m.bool(at(a::STATE_UNCOVER_PURSUIT_LOCATION_INSPECTED));
    }

    let remaining = at(a::STATE_UNCOVER_UNCOVER_REMAINING_TIME);
    let persistent = at(a::STATE_UNCOVER_UNCOVER_EXIT_PERSISTENT_TIMER);
    let reset = if m.i16(at(a::FIRING_POSITIONS_CURRENT_POSITION_INDEX)) == NONE as i16 {
        m.set_bool(at(a::STATE_UNCOVER_UNCOVER_EXIT_FAILURE), true);
        false
    } else {
        done && (m.bool(at(a::INPUT_VEHICLE_GUNNER_BOMBARDMENT)) || target_visible || m.bool(at(a::CONTROL_MOVING)))
    };
    if reset {
        m.w32(remaining, m.r32(at(a::STATE_UNCOVER_UNCOVER_DESIRED_TIME)));
    } else {
        if m.i32(remaining) > 0 {
            m.w32(remaining, (m.i32(remaining) - 1) as u32);
        }
        m.w32(persistent, m.r32(persistent).wrapping_add(1));
    }
    done = !(m.i32(remaining) != 0 && m.i32(persistent) < 360);
    if pursuit_type == PURSUIT_LOCATION_POSITION && m.bool(at(a::STATE_UNCOVER_PURSUIT_LOCATION_INSPECTED)) {
        done = true;
    }
    if done {
        call_simple(context, host, uncover_call::DONE, 0);
    }
    m.set_bool(at(a::STATE_UNCOVER_UNCOVER_DONE), done);
}

/// action_guard.c's calls.
pub mod guard_call {
    const BASE: i32 = (crate::layout::ai_values::_ACTOR_ACTION_GUARD as i32) << 8;
    /// `game_team_is_ally(a, b)`, the result.
    pub const TEAM_IS_ALLY: i32 = BASE | 1;
    /// `ai_communication_event(a, unit, b, input[0], NONE...)`.
    pub const COMMUNICATION: i32 = BASE | 2;
    /// `actor_set_dormant(actor, FALSE)`.
    pub const WAKE: i32 = BASE | 3;
    /// `ai_communication_event(a, unit, the target's unit, NONE...)`.
    pub const COMMUNICATION_TARGET: i32 = BASE | 4;
    /// `actor_discard_firing_position(actor, a, FALSE)`.
    pub const DISCARD_FIRING_POSITION: i32 = BASE | 5;
    /// A guard firing position selected and changed to: the new index the result.
    pub const CHANGE_FIRING_POSITION: i32 = BASE | 6;
    /// `real_seed_random_range(global seed, input[0], input[1])`, in `output[0]`.
    pub const RANDOM_RANGE: i32 = BASE | 7;
    /// `actor_move_halt(actor)`.
    pub const MOVE_HALT: i32 = BASE | 8;
    /// `actor_move_to_point` to the guard point.
    pub const MOVE_TO_GUARD_POINT: i32 = BASE | 9;
    /// `actor_move_to_firing_position(actor, a, NULL)`, the result.
    pub const MOVE_TO_FIRING_POSITION: i32 = BASE | 10;
    /// The shout about the dead friend of the prop `a` (with the suspicion it raises).
    pub const SHOUT_DEAD_FRIEND: i32 = BASE | 11;
}

/// action_guard.c's raw offsets (January's layout; the shared structures
/// name them otherwise): a prop's death, the actor's arrival at its
/// destination (the C names it moving), its cower retreat timer.
const GUARD_PROP_IS_DEAD: u32 = 0x60;
const GUARD_ACTOR_AT_DESTINATION: u32 = 0x484;
const GUARD_COWER_RETREAT_TIMER: u32 = 0x3A8;
const GUARD_CROUCH_WHEN_INACTIVE_BIT: u32 = 6;
const GUARD_CROUCH_WHEN_UNCOVERED_BIT: u32 = 7;
const GUARD_CROUCH_WHEN_COWERING_BIT: u32 = 23;

/// `action_guard_speak_post_combat`.
fn guard_speak_post_combat(context: &AiStepContext, host: &impl AiHost) -> bool {
    let m = host.memory();
    let at = |offset: usize| context.actor + offset as u32;
    let vocalized = at(a::STATE_GUARD_POST_COMBAT_VOCALIZED);
    if m.bool(vocalized) {
        return false;
    }
    let communication_type: i32 = match m.i16(at(a::EXTERNAL_ORDERS_POSTCOMBAT_TYPE)) {
        1 => 48,
        2 => 49,
        3 => 50,
        4 => 51,
        5 => 52,
        7 => 53,
        8 => 54,
        9 => 55,
        10 => 56,
        _ => NONE,
    };
    let mut result = false;
    if communication_type != NONE {
        let mut prop_unit_index = NONE;
        let mut prop_type = NONE;
        let look_prop_index = m.i32(at(a::STATE_GUARD_GUARD_LOOK_PROP_INDEX));
        if look_prop_index != NONE {
            let prop = prop_get(m, context, look_prop_index);
            prop_unit_index = m.i32(prop + p::UNIT_INDEX as u32);
            prop_type = if m.bool(prop + GUARD_PROP_IS_DEAD) {
                let ally = call_full(
                    context,
                    host,
                    guard_call::TEAM_IS_ALLY,
                    m.i16(at(a::META_TEAM_INDEX)) as i32,
                    m.i16(prop + p::TEAM_INDEX as u32) as i32,
                    &[],
                )
                .result
                    != 0;
                if ally { 4 } else { 3 }
            } else {
                2
            };
        }
        call_full(context, host, guard_call::COMMUNICATION, communication_type, prop_unit_index, &[prop_type as f32]);
        result = true;
    }
    m.set_bool(vocalized, true);
    result
}

/// `action_guard_update`.
fn guard_update(context: &AiStepContext, host: &impl AiHost) {
    let m = host.memory();
    let actor = context.actor;
    let at = |offset: usize| actor + offset as u32;
    let at_destination = || m.bool(actor + GUARD_ACTOR_AT_DESTINATION);
    let wait_ticks = at(a::STATE_GUARD_WAIT_TICKS);
    if !m.bool(at(a::META_DORMANT)) && at_destination() && m.i16(wait_ticks) > 0 {
        m.set_i16(wait_ticks, m.i16(wait_ticks) - 1);
        if m.i16(wait_ticks) == 0 && !m.bool(at(a::INPUT_VEHICLE_PASSENGER)) && !m.bool(at(a::META_SWARM)) {
            if m.bool(at(a::STATE_GUARD_POST_COMBAT)) {
                guard_speak_post_combat(context, host);
                m.set_i16(at(a::EXTERNAL_ORDERS_POSTCOMBAT_TYPE), v::_ACTOR_POSTCOMBAT_NONE as i16);
                m.w32(at(a::EXTERNAL_ORDERS_POSTCOMBAT_PROP_INDEX), NONE as u32);
                m.set_bool(at(a::STATE_GUARD_POST_COMBAT), false);
                m.set_bool(at(a::STATE_GUARD_POST_COMBAT_SHOOTING), false);
                m.w32(at(a::STATE_GUARD_GUARD_LOOK_PROP_INDEX), NONE as u32);
            }
            m.set_bool(at(a::STATE_GUARD_FIND_NEW_GUARD_POSITION), true);
        }
    }

    let look_ticks = at(a::STATE_GUARD_LOOK_TICKS);
    if m.i16(look_ticks) > 0 && (!m.bool(at(a::STATE_GUARD_GUARD_LOOK_UNTIL_REACHED_POINT)) || at_destination()) {
        m.set_i16(look_ticks, m.i16(look_ticks) - 1);
        if m.i16(look_ticks) == 0 {
            m.w32(at(a::STATE_GUARD_GUARD_LOOK_PROP_INDEX), NONE as u32);
        }
    }

    if m.bool(at(a::STATE_GUARD_PATH_BEGUN)) && at_destination() {
        let cower_from_retreat = at(a::STATE_GUARD_COWER_FROM_RETREAT);
        let cower_ticks = at(a::STATE_GUARD_COWER_TICKS);
        let cower_finished = if m.bool(cower_from_retreat) {
            let still = m.i16(actor + GUARD_COWER_RETREAT_TIMER) > 0;
            m.set_bool(cower_from_retreat, still);
            !still
        } else if m.i16(cower_ticks) > 0 {
            m.set_i16(cower_ticks, m.i16(cower_ticks) - 1);
            m.i16(cower_ticks) == 0
        } else {
            false
        };
        if cower_finished {
            call_simple(context, host, guard_call::WAKE, 0);
            m.set_bool(at(a::STATE_GUARD_COWER), false);
            m.set_bool(at(a::STATE_GUARD_COWER_PANICKED), false);
            m.set_bool(cower_from_retreat, false);
            m.set_i16(cower_ticks, 0);
            if m.i16(at(a::STATE_COMBAT_STATUS)) >= 2 && m.i32(at(a::META_UNIT_INDEX)) != NONE {
                call_simple(context, host, guard_call::COMMUNICATION_TARGET, 35);
            }
            let location_type = at(a::STATE_GUARD_GUARD_LOCATION_TYPE);
            if m.i16(location_type) == 3 {
                let index = m.i16(at(a::STATE_GUARD_GUARD_FIRING_POSITION_INDEX)) as i32;
                call_simple(context, host, guard_call::DISCARD_FIRING_POSITION, index);
                m.set_i16(at(a::STATE_GUARD_GUARD_FIRING_POSITION_INDEX), NONE as i16);
            }
            m.set_i16(at(a::FIRING_POSITIONS_CURRENT_POSITION_INDEX), NONE as i16);
            if m.bool(at(a::INPUT_VEHICLE_PASSENGER)) {
                m.set_i16(location_type, 1);
                return;
            }
            m.set_i16(location_type, 0);
            m.set_bool(at(a::STATE_GUARD_FIND_NEW_GUARD_POSITION), true);
        }
    }
}

/// `action_guard_perform`.
fn guard_perform(context: &AiStepContext, host: &impl AiHost) -> bool {
    let m = host.memory();
    let at = |offset: usize| context.actor + offset as u32;
    let location_type = at(a::STATE_GUARD_GUARD_LOCATION_TYPE);
    let find_new = at(a::STATE_GUARD_FIND_NEW_GUARD_POSITION);
    if m.bool(at(a::META_SWARM)) {
        m.set_i16(location_type, 1);
        return false;
    }
    if m.bool(at(a::INPUT_VEHICLE_PASSENGER)) {
        m.set_i16(location_type, 1);
        m.set_bool(find_new, true);
        return false;
    }
    let current = at(a::FIRING_POSITIONS_CURRENT_POSITION_INDEX);
    if m.i16(location_type) == 3 && m.i16(current) == NONE as i16 {
        m.set_i16(location_type, 0);
        m.set_bool(find_new, true);
    }
    if m.bool(at(a::META_TIMESLICE)) && m.bool(find_new) {
        if m.i16(location_type) == 3 && m.i16(current) != NONE as i16 {
            call_simple(context, host, guard_call::DISCARD_FIRING_POSITION, m.i16(current) as i32);
        }
        let firing_position_index = call_simple(context, host, guard_call::CHANGE_FIRING_POSITION, 0) as i16;
        m.set_bool(find_new, false);
        m.set_bool(at(a::STATE_GUARD_HAS_GUARD_DIRECTION), false);
        if firing_position_index == NONE as i16 {
            m.set_i16(location_type, 1);
        } else {
            m.set_i16(location_type, 3);
            m.set_i16(at(a::STATE_GUARD_GUARD_FIRING_POSITION_INDEX), firing_position_index);
        }
        let upper = m.f32(context.definition + d::FIRING_POSITION_GUARD_POSITION_TIME_UPPER_BOUND as u32);
        let lower = m.f32(context.definition + d::FIRING_POSITION_GUARD_POSITION_TIME_LOWER_BOUND as u32);
        let time = call_full(context, host, guard_call::RANDOM_RANGE, 0, 0, &[lower, upper]).output[0];
        m.set_i16(at(a::STATE_GUARD_WAIT_TICKS), c_short(time * 30.0));
    }
    false
}

/// `action_guard_control`.
fn guard_control(context: &AiStepContext, host: &impl AiHost) {
    let m = host.memory();
    let at = |offset: usize| context.actor + offset as u32;
    let flags = m.r32(context.definition + d::FLAGS as u32);
    let combat_status = m.i16(at(a::STATE_COMBAT_STATUS));
    let stationary = at(a::ORDERS_MOVE_STATIONARY_CROUCH);
    if flags & (1 << GUARD_CROUCH_WHEN_INACTIVE_BIT) != 0 && combat_status == 0 {
        m.set_bool(stationary, true);
        m.set_bool(at(a::ORDERS_MOVE_MOVING_CROUCH), true);
    } else {
        m.set_bool(at(a::ORDERS_MOVE_MOVING_CROUCH), false);
        if m.bool(at(a::STATE_GUARD_COWER)) {
            if m.bool(at(a::STATE_GUARD_COWER_FROM_RETREAT)) {
                m.set_bool(stationary, flags & (1 << GUARD_CROUCH_WHEN_COWERING_BIT) != 0);
            } else {
                m.set_bool(stationary, true);
            }
        } else {
            m.set_bool(stationary, flags & (1 << GUARD_CROUCH_WHEN_UNCOVERED_BIT) != 0 && combat_status > 0);
        }
    }
    m.set_bool(at(a::ORDERS_MOVE_PANICKED), false);
    m.set_bool(at(a::ORDERS_MOVE_DIVE_INTO_COVER), false);
    m.set_bool(at(a::ORDERS_MOVE_EMERGE_FROM_COVER), false);

    let location_type = m.i16(at(a::STATE_GUARD_GUARD_LOCATION_TYPE));
    if m.bool(at(a::META_TIMESLICE)) && !m.bool(at(a::META_SWARM)) {
        let path_succeeded = match location_type {
            2 => {
                let distance = distance_squared(m, at(a::INPUT_BODY_POSITION), at(a::STATE_GUARD_POINT_POSITION));
                let radius = m.f32(at(a::STATE_GUARD_POINT_RADIUS));
                if distance < radius * radius {
                    call_simple(context, host, guard_call::MOVE_HALT, 0);
                } else {
                    call_simple(context, host, guard_call::MOVE_TO_GUARD_POINT, 0);
                }
                distance < 9.0
            }
            3 => {
                let guard_index = m.i16(at(a::STATE_GUARD_GUARD_FIRING_POSITION_INDEX));
                if guard_index != NONE as i16 {
                    m.set_i16(at(a::FIRING_POSITIONS_CURRENT_POSITION_INDEX), guard_index);
                    m.set_bool(at(a::FIRING_POSITIONS_CURRENT_POSITION_FOUND_OUTSIDE_RANGE), false);
                    if call_simple(context, host, guard_call::MOVE_TO_FIRING_POSITION, guard_index as i32) == 0 {
                        call_simple(context, host, guard_call::DISCARD_FIRING_POSITION, guard_index as i32);
                        m.set_i16(at(a::FIRING_POSITIONS_CURRENT_POSITION_INDEX), NONE as i16);
                    }
                }
                !m.bool(at(a::CONTROL_PATH_PATH) + crate::layout::path_result::VALID as u32)
                    || distance_squared(m, at(a::CONTROL_PATH_PATH_ENDPOINT_POINT), at(a::INPUT_BODY_POSITION)) < 9.0
            }
            // (0 and 1; the C's assertion ends the game for any other)
            _ => {
                call_simple(context, host, guard_call::MOVE_HALT, 0);
                true
            }
        };
        m.set_bool(at(a::STATE_GUARD_PATH_BEGUN), true);
        if path_succeeded {
            if m.bool(at(a::STATE_GUARD_SHOUT_ABOUT_DEAD_FRIEND)) {
                let prop_index = m.i32(at(a::STATE_GUARD_SHOUT_DEAD_FRIEND_PROP_INDEX));
                m.set_bool(at(a::STATE_GUARD_SHOUT_ABOUT_DEAD_FRIEND), false);
                m.w32(at(a::STATE_GUARD_SHOUT_DEAD_FRIEND_PROP_INDEX), NONE as u32);
                call_simple(context, host, guard_call::SHOUT_DEAD_FRIEND, prop_index);
            }
            if m.bool(at(a::STATE_GUARD_POST_COMBAT)) {
                guard_speak_post_combat(context, host);
                if m.i16(at(a::EXTERNAL_ORDERS_POSTCOMBAT_TYPE)) == v::_ACTOR_POSTCOMBAT_SHOOT_CORPSE as i16
                    && m.i16(at(a::STATE_GUARD_GUARD_LOCATION_TYPE)) == 2
                {
                    m.set_bool(at(a::STATE_GUARD_POST_COMBAT_SHOOTING), true);
                }
            }
        }
    }

    let priority = at(a::ORDERS_LOOK_PRIMARY_PRIORITY);
    let direction = at(a::ORDERS_LOOK_PRIMARY_DIRECTION_TYPE);
    let look_prop_index = m.i32(at(a::STATE_GUARD_GUARD_LOOK_PROP_INDEX));
    if m.bool(at(a::STATE_GUARD_POST_COMBAT_SHOOTING)) {
        m.set_i16(priority, 7);
        m.set_i16(direction, v::_DIRECTION_SPECIFICATION_TARGET as i16);
        m.set_bool(at(a::ORDERS_COMBAT_SHOOT_AT_TARGET), true);
        m.set_bool(at(a::ORDERS_COMBAT_USE_MANUAL_TARGET_POINT), true);
        let point = m.vector(at(a::STATE_GUARD_POINT_POSITION));
        let up = [0.0f32, 0.0, 1.0];
        let target = at(a::ORDERS_COMBAT_TARGET_POINT);
        for i in 0..3 {
            m.set_f32(target + 4 * i as u32, point[i] + up[i] * 0.05);
        }
    } else if look_prop_index != NONE {
        m.set_i16(priority, 5);
        m.set_i16(direction, v::_DIRECTION_SPECIFICATION_PROP as i16);
        m.w32(direction + 4, look_prop_index as u32);
    } else if m.bool(at(a::STATE_GUARD_HAS_GUARD_DIRECTION)) {
        m.set_i16(priority, if m.bool(at(a::STATE_GUARD_AIM_IN_GUARD_DIRECTION)) { 5 } else { 3 });
        m.set_i16(direction, v::_DIRECTION_SPECIFICATION_VECTOR as i16);
        for word in 0..3 {
            m.w32(direction + 4 + word * 4, m.r32(at(a::STATE_GUARD_GUARD_DIRECTION) + word * 4));
        }
    } else if m.i16(at(a::STATE_COMBAT_STATUS)) > 0 && m.i32(at(a::TARGET_TARGET_PROP_INDEX)) != NONE {
        m.set_i16(priority, 3);
        m.set_i16(direction, v::_DIRECTION_SPECIFICATION_PROP as i16);
        m.w32(direction + 4, m.r32(at(a::TARGET_TARGET_PROP_INDEX)));
    } else {
        m.set_i16(priority, v::_PRIMARY_PRIORITY_NONE as i16);
    }
    m.set_i16(at(a::ORDERS_LOOK_IDLE_LOOK_TYPE), if m.i16(at(a::STATE_COMBAT_STATUS)) >= 4 { 4 } else { 2 });
}

/// action_flee.c's calls.
pub mod flee_call {
    const BASE: i32 = (crate::layout::ai_values::_ACTOR_ACTION_FLEE as i32) << 8;
    /// `unit_start_running_blindly(unit)`.
    pub const RUN_BLINDLY: i32 = BASE | 1;
    /// `actor_move_halt(actor)`.
    pub const MOVE_HALT: i32 = BASE | 2;
    /// `actor_move_to_firing_position(actor, a, NULL)`, the result.
    pub const MOVE_TO_FIRING_POSITION: i32 = BASE | 3;
    /// `actor_discard_firing_position(actor, a, FALSE)`.
    pub const DISCARD_FIRING_POSITION: i32 = BASE | 4;
    /// `actor_path_at_destination(actor)`, the result.
    pub const AT_DESTINATION: i32 = BASE | 5;
    /// `actor_destination_tolerance(actor)`, in `output[0]`.
    pub const TOLERANCE: i32 = BASE | 6;
    /// `actor_situation_update_target_status` and
    /// `actor_situation_combat_status_update` of the actor.
    pub const SITUATION_STATUS: i32 = BASE | 7;
    /// `action_flee_current_position_exposed(actor)`, the result (its
    /// approach point written to the actor).
    pub const POSITION_EXPOSED: i32 = BASE | 8;
    /// `action_flee_find_flee_position(actor, TRUE)`, into the actor.
    pub const FIND_FLEE_POSITION: i32 = BASE | 9;
    /// `unit_is_speaking(unit)`, the result.
    pub const UNIT_IS_SPEAKING: i32 = BASE | 10;
    /// `unit_scream`: burning to death (`a` 0) or a grenade attached (1).
    pub const SCREAM: i32 = BASE | 11;
    /// The flee shout (`a` 0, or its leader-died form by the panic) or its
    /// idle form (`a` 1), the cause `b`.
    pub const SHOUT: i32 = BASE | 12;
}

/// The panic types (action_flee.c's).
const PANIC_NONE: i16 = 0;
const PANIC_GRENADE_ATTACHED_TO_US: i16 = 9;
const PANIC_DELAYED_PROJECTILE_ATTACHED_TO_US: i16 = 10;
const PANIC_MELEE_ATTACHED_TO_US: i16 = 11;
const PANIC_BURNING_TO_DEATH: i16 = 12;

/// `action_flee_blind_panic`.
fn flee_blind_panic(panic_type: i16) -> bool {
    (PANIC_GRENADE_ATTACHED_TO_US..=PANIC_BURNING_TO_DEATH).contains(&panic_type)
}

/// `action_flee_update`.
fn flee_update(context: &AiStepContext, host: &impl AiHost) {
    let m = host.memory();
    let at = |offset: usize| context.actor + offset as u32;
    let timer = at(a::STATE_FLEE_FLEE_TIMER);
    m.w32(timer, m.r32(timer).wrapping_add(1));
    let forced = at(a::STATE_FLEE_FORCED_FLEE_TICKS);
    if m.i16(forced) > 0 {
        m.set_i16(forced, m.i16(forced) - 1);
    }
    let panic_type = m.i16(at(a::STATE_FLEE_PANIC_TYPE));
    let stationary = at(a::STATE_FLEE_FLEE_STATIONARY_TICKS);
    if m.i16(stationary) > 0 {
        m.set_i16(stationary, m.i16(stationary) - 1);
        if m.i16(stationary) == 0 && m.i32(at(a::META_UNIT_INDEX)) != NONE && flee_blind_panic(panic_type) {
            call_simple(context, host, flee_call::RUN_BLINDLY, 0);
        }
    }
    if m.i16(at(a::STATE_FLEE_PANIC_TYPE)) > PANIC_NONE {
        m.w32(at(a::EMOTIONS_FLEE_WITH_FRIENDS_DISABLE_TIME), context.game_time.wrapping_add(750) as u32);
    }
}

/// `action_flee_control`.
fn flee_control(context: &AiStepContext, host: &impl AiHost) {
    let m = host.memory();
    let at = |offset: usize| context.actor + offset as u32;
    let panic_type = m.i16(at(a::STATE_FLEE_PANIC_TYPE));
    let priority = at(a::ORDERS_LOOK_PRIMARY_PRIORITY);
    let direction = at(a::ORDERS_LOOK_PRIMARY_DIRECTION_TYPE);
    let flee_prop_index = m.i32(at(a::STATE_FLEE_FLEE_PROP_INDEX));
    let target_prop_index = m.i32(at(a::TARGET_TARGET_PROP_INDEX));
    if panic_type > PANIC_NONE {
        m.set_i16(priority, 6);
        m.set_i16(direction, v::_DIRECTION_SPECIFICATION_MOVEMENT as i16);
        m.set_bool(at(a::ORDERS_COMBAT_ABORT_BURST), true);
    } else if target_prop_index != NONE && m.i16(prop_get(m, context, target_prop_index) + p::VISIBILITY as u32) > 0 {
        m.set_i16(priority, 7);
        m.set_i16(direction, v::_DIRECTION_SPECIFICATION_TARGET as i16);
        m.set_bool(at(a::ORDERS_COMBAT_SHOOT_AT_TARGET), true);
    } else if flee_prop_index != NONE {
        m.set_i16(priority, 3);
        m.set_i16(direction, v::_DIRECTION_SPECIFICATION_PROP as i16);
        m.w32(direction + 4, flee_prop_index as u32);
    } else {
        m.set_i16(priority, v::_PRIMARY_PRIORITY_NONE as i16);
    }
    m.set_i16(at(a::ORDERS_LOOK_IDLE_LOOK_TYPE), 4);
    m.set_bool(at(a::ORDERS_MOVE_PANICKED), panic_type > PANIC_NONE);
    m.set_bool(at(a::ORDERS_MOVE_FLAMING), flee_blind_panic(panic_type));
    m.set_bool(at(a::ORDERS_MOVE_STATIONARY_CROUCH), true);
    m.set_bool(at(a::ORDERS_MOVE_MOVING_CROUCH), false);
    m.set_bool(at(a::ORDERS_MOVE_DIVE_INTO_COVER), true);
    m.set_bool(at(a::ORDERS_MOVE_EMERGE_FROM_COVER), false);

    let flee_position = at(a::STATE_FLEE_FLEE_FIRING_POSITION_INDEX);
    let current = at(a::FIRING_POSITIONS_CURRENT_POSITION_INDEX);
    if m.i16(flee_position) == NONE as i16 {
        call_simple(context, host, flee_call::MOVE_HALT, 0);
    } else if m.bool(at(a::META_TIMESLICE)) {
        if call_simple(context, host, flee_call::MOVE_TO_FIRING_POSITION, m.i16(flee_position) as i32) != 0 {
            m.set_i16(current, m.i16(flee_position));
            m.w8(
                at(a::FIRING_POSITIONS_CURRENT_POSITION_FOUND_OUTSIDE_RANGE),
                m.r8(at(a::STATE_FLEE_FLEE_FIRING_POSITION_FOUND_RANDOMLY)),
            );
        } else {
            if m.i16(current) != NONE as i16 {
                call_simple(context, host, flee_call::DISCARD_FIRING_POSITION, m.i16(current) as i32);
                call_simple(context, host, flee_call::MOVE_HALT, 0);
                m.set_i16(current, NONE as i16);
            }
            m.set_i16(flee_position, NONE as i16);
            m.set_bool(at(a::STATE_FLEE_FIND_NEW_FLEE_POSITION), true);
        }
    }
}

/// `action_flee_at_flee_position`.
fn flee_at_flee_position(context: &AiStepContext, host: &impl AiHost) -> bool {
    use crate::ai_actor::tag_block_element;
    use crate::layout::{encounter_definition, firing_position_definition as fp, scenario};
    let m = host.memory();
    let at = |offset: usize| context.actor + offset as u32;
    let encounter_index = m.i32(at(a::META_ENCOUNTER_INDEX));
    let current = m.i16(at(a::FIRING_POSITIONS_CURRENT_POSITION_INDEX));
    if encounter_index == NONE || current == NONE as i16 {
        return false;
    }
    let encounter = tag_block_element(
        m,
        context.scenario + scenario::AI_ENCOUNTERS as u32,
        encounter_index & 0xFFFF,
        encounter_definition::SIZEOF,
    );
    let firing_position =
        tag_block_element(m, encounter + encounter_definition::FIRING_POSITIONS as u32, current as i32, fp::SIZEOF);
    if call_simple(context, host, flee_call::AT_DESTINATION, 0) != 0
        && m.i16(at(a::CONTROL_PATH_DESTINATION_TYPE)) == v::_DESTINATION_FIRING_POSITION as i16
        && m.i16(at(a::CONTROL_PATH_DESTINATION_FIRING_POSITION_INDEX)) == m.i16(at(a::FIRING_POSITIONS_CURRENT_POSITION_INDEX))
    {
        return true;
    }
    let tolerance = call_full(context, host, flee_call::TOLERANCE, 0, 0, &[]).output[0];
    if !(distance_squared(m, at(a::INPUT_BODY_POSITION), firing_position + fp::POSITION as u32) < tolerance * tolerance) {
        return false;
    }
    let prop_index = m.i32(at(a::STATE_FLEE_FLEE_PROP_INDEX));
    let mut seen = false;
    if prop_index != NONE {
        let line_of_sight = m.i16(prop_get(m, context, prop_index) + p::LINE_OF_SIGHT as u32);
        seen = line_of_sight == v::_AI_LINE_OF_SIGHT_CLEAR as i16 || line_of_sight == v::_AI_LINE_OF_SIGHT_OCCLUDED as i16;
    }
    !seen
}

/// `action_flee_perform`.
fn flee_perform(context: &AiStepContext, host: &impl AiHost) -> bool {
    let m = host.memory();
    let at = |offset: usize| context.actor + offset as u32;
    let panic_type = || m.i16(at(a::STATE_FLEE_PANIC_TYPE));
    let flee_position = at(a::STATE_FLEE_FLEE_FIRING_POSITION_INDEX);
    let find_new = at(a::STATE_FLEE_FIND_NEW_FLEE_POSITION);
    let done = at(a::STATE_FLEE_DONE_FLEEING);
    let unable = at(a::STATE_FLEE_UNABLE_TO_FLEE);
    let forced = at(a::STATE_FLEE_FORCED_FLEE_TICKS);
    let current = at(a::FIRING_POSITIONS_CURRENT_POSITION_INDEX);

    if !m.bool(at(a::META_SWARM)) {
        if flee_blind_panic(panic_type()) {
            m.set_i16(forced, 180);
        }
        if m.i16(at(a::STATE_FLEE_FLEE_STATIONARY_TICKS)) > 0 {
            m.set_i16(flee_position, NONE as i16);
        } else if m.i16(flee_position) == NONE as i16 {
            m.set_bool(find_new, true);
        } else if m.i16(current) == NONE as i16 {
            m.set_i16(flee_position, NONE as i16);
            m.set_bool(find_new, true);
        } else if flee_at_flee_position(context, host) {
            if m.i16(forced) != 0 {
                m.set_bool(find_new, true);
            } else {
                let flee_prop_index = m.i32(at(a::STATE_FLEE_FLEE_PROP_INDEX));
                m.set_i16(flee_position, m.i16(current));
                m.w8(
                    at(a::STATE_FLEE_FLEE_FIRING_POSITION_FOUND_RANDOMLY),
                    m.r8(at(a::FIRING_POSITIONS_CURRENT_POSITION_FOUND_OUTSIDE_RANGE)),
                );
                m.set_bool(done, true);
                m.set_bool(find_new, false);
                if flee_prop_index != NONE {
                    let prop = prop_get(m, context, flee_prop_index);
                    m.set_i16(prop + p::VISIBILITY as u32, 0);
                    let audibility = m.i16(prop + p::AUDIBILITY as u32);
                    let ineffability = m.i16(prop + p::INEFFABILITY as u32);
                    m.set_i16(prop + p::PERCEPTION as u32, if audibility > ineffability { audibility } else { ineffability });
                    m.set_bool(prop + p::CURRENTLY_DAMAGING_ME as u32, false);
                    m.set_i16(prop + p::LINE_OF_SIGHT as u32, v::_AI_LINE_OF_SIGHT_FROM_COVER as i16);
                    call_simple(context, host, flee_call::SITUATION_STATUS, 0);
                }
            }
        }

        match panic_type() {
            PANIC_GRENADE_ATTACHED_TO_US | PANIC_DELAYED_PROJECTILE_ATTACHED_TO_US => {
                if m.i32(at(a::INPUT_DELAYED_ATTACHED_PROJECTILE_INDEX)) == NONE {
                    m.set_bool(done, true);
                }
            }
            PANIC_MELEE_ATTACHED_TO_US => {
                if !m.bool(at(a::INPUT_MELEE_ATTACKER_ATTACHED)) {
                    m.set_bool(done, true);
                }
            }
            PANIC_BURNING_TO_DEATH => {
                if !m.bool(at(a::INPUT_BURNING_TO_DEATH)) {
                    m.set_bool(done, true);
                }
            }
            _ => {}
        }

        if m.bool(at(a::META_TIMESLICE)) && !m.bool(done) {
            if m.i16(flee_position) != NONE as i16
                && m.i16(forced) == 0
                && call_simple(context, host, flee_call::POSITION_EXPOSED, 0) != 0
            {
                m.set_i16(flee_position, NONE as i16);
                m.set_bool(find_new, true);
            }
            if m.bool(at(a::INPUT_VEHICLE_PASSENGER)) {
                m.set_bool(find_new, false);
                m.set_bool(unable, true);
                m.w32(at(a::EMOTIONS_LAST_FLEE_FAILED_TIME), context.game_time as u32);
            } else if m.bool(find_new) {
                call_simple(context, host, flee_call::FIND_FLEE_POSITION, 0);
                if m.i16(flee_position) == NONE as i16 {
                    m.set_bool(unable, true);
                    m.w32(at(a::EMOTIONS_LAST_FLEE_FAILED_TIME), context.game_time as u32);
                }
            }
        }
    }

    let communicated = at(a::STATE_FLEE_COMMUNICATED_FLEE);
    let unit_index = m.i32(at(a::META_UNIT_INDEX));
    if flee_blind_panic(panic_type())
        && unit_index != NONE
        && call_simple(context, host, flee_call::UNIT_IS_SPEAKING, 0) == 0
    {
        m.set_bool(communicated, false);
    }

    let panic = panic_type();
    if panic > PANIC_NONE && m.i16(flee_position) != NONE as i16 && !m.bool(unable) && unit_index != NONE {
        let now = context.game_time;
        let last_time = at(a::STATE_FLEE_LAST_COMMUNICATION_TIME);
        if !m.bool(communicated) || m.i32(last_time).wrapping_add(60) >= now {
            if panic == PANIC_BURNING_TO_DEATH || panic == PANIC_MELEE_ATTACHED_TO_US {
                call_simple(context, host, flee_call::SCREAM, 0);
            } else if panic == PANIC_GRENADE_ATTACHED_TO_US || panic == PANIC_DELAYED_PROJECTILE_ATTACHED_TO_US {
                call_simple(context, host, flee_call::SCREAM, 1);
            } else {
                let mut cause_unit_index = NONE;
                let flee_prop_index = m.i32(at(a::STATE_FLEE_FLEE_PROP_INDEX));
                if flee_prop_index != NONE {
                    cause_unit_index = m.i32(prop_get(m, context, flee_prop_index) + p::UNIT_INDEX as u32);
                }
                if !m.bool(communicated) {
                    call_full(context, host, flee_call::SHOUT, 0, cause_unit_index, &[]);
                    m.set_bool(communicated, true);
                } else {
                    call_full(context, host, flee_call::SHOUT, 1, cause_unit_index, &[]);
                }
            }
            m.w32(last_time, now as u32);
        }
    }
    m.bool(unable) || m.bool(done)
}

/// action_converse.c's calls.
pub mod converse_call {
    const BASE: i32 = (crate::layout::ai_values::_ACTOR_ACTION_CONVERSE as i32) << 8;
    /// `prop_get_base_by_unit_index(actor, a, TRUE, TRUE)`, the result.
    pub const BASE_PROP: i32 = BASE | 1;
    /// `actor_move_halt(actor)`.
    pub const MOVE_HALT: i32 = BASE | 2;
    /// `actor_move_to_prop(actor, a, input[0])`, the result.
    pub const MOVE_TO_PROP: i32 = BASE | 3;
    /// The actor's prop of its conversation's triggering player (or NONE),
    /// the result.
    pub const PLAYER_PROP: i32 = BASE | 4;
}

/// `action_converse_perform`.
fn converse_perform(context: &AiStepContext, host: &impl AiHost) -> bool {
    let m = host.memory();
    let at = |offset: usize| context.actor + offset as u32;
    let failed = at(a::STATE_CONVERSE_FAILED);
    if m.bool(at(a::META_TIMESLICE)) {
        let prop_index = at(a::STATE_CONVERSE_RUN_TO_PROP_INDEX);
        let unit_index = m.i32(at(a::STATE_CONVERSE_RUN_TO_UNIT_INDEX));
        if m.i32(prop_index) == NONE && unit_index != NONE {
            let prop = call_simple(context, host, converse_call::BASE_PROP, unit_index);
            m.w32(prop_index, prop as u32);
        }
        if m.i32(prop_index) == NONE {
            m.set_bool(failed, true);
        } else {
            let in_range = at(a::STATE_CONVERSE_IN_RANGE);
            let distance = m.f32(at(a::STATE_CONVERSE_RUN_TO_DISTANCE));
            if !m.bool(in_range) {
                let prop = prop_get(m, context, m.i32(prop_index));
                let prop_distance = m.f32(prop + p::DISTANCE as u32);
                if m.i16(prop + p::VISIBILITY as u32) >= v::_ACTOR_PERCEPTION_FULL as i16 && prop_distance < distance {
                    m.set_bool(in_range, true);
                } else if prop_distance < 0.7 {
                    m.set_bool(in_range, true);
                }
            }
            if m.bool(in_range) {
                call_simple(context, host, converse_call::MOVE_HALT, 0);
            } else if call_full(context, host, converse_call::MOVE_TO_PROP, m.i32(prop_index), 0, &[distance]).result == 0 {
                m.set_bool(failed, true);
            }
        }
    }
    m.bool(failed)
}

/// `action_converse_control`.
fn converse_control(context: &AiStepContext, host: &impl AiHost) {
    let m = host.memory();
    let at = |offset: usize| context.actor + offset as u32;
    let mut prop_index = m.i32(at(a::STATE_CONVERSE_RUN_TO_PROP_INDEX));
    if prop_index == NONE {
        prop_index = if m.i32(at(a::STATE_CONVERSE_CONVERSATION_INDEX)) != NONE {
            call_simple(context, host, converse_call::PLAYER_PROP, 0)
        } else {
            NONE
        };
    }
    m.set_i16(at(a::ORDERS_LOOK_IDLE_LOOK_TYPE), 1);
    if prop_index != NONE {
        m.set_i16(at(a::ORDERS_LOOK_PRIMARY_PRIORITY), v::_PRIMARY_PRIORITY_FACING as i16);
        m.set_i16(at(a::ORDERS_LOOK_PRIMARY_DIRECTION_TYPE), v::_DIRECTION_SPECIFICATION_PROP as i16);
        m.w32(at(a::ORDERS_LOOK_PRIMARY_DIRECTION_TYPE) + 4, prop_index as u32);
    }
}

/// action_vehicle.c's calls.
pub mod vehicle_call {
    const BASE: i32 = (crate::layout::ai_values::_ACTOR_ACTION_VEHICLE as i32) << 8;
    /// `actor_path_has_path(actor)`, the result.
    pub const HAS_PATH: i32 = BASE | 1;
    /// Whether the vehicle `a` exists (`vehicle_try_and_get`), the result.
    pub const VEHICLE_EXISTS: i32 = BASE | 2;
    /// `action_vehicle_desirable` of the actor's vehicle by its state, the result.
    pub const DESIRABLE: i32 = BASE | 3;
    /// `action_vehicle_evaluate_seat` of the actor's seat: whether it is
    /// valid, within range, facing correctly and could be faked in bits 0-3
    /// of the result, the entry point and facing in `output` (its hint point
    /// kept for [`FIND_DESTINATION`]).
    pub const EVALUATE_SEAT: i32 = BASE | 4;
    /// `unit_enter_seat(unit, its vehicle, its seat)`.
    pub const ENTER_SEAT: i32 = BASE | 5;
    /// `actor_move_halt(actor)`.
    pub const MOVE_HALT: i32 = BASE | 6;
    /// `action_vehicle_find_destination` from the evaluated seat, into the
    /// actor: the result.
    pub const FIND_DESTINATION: i32 = BASE | 7;
    /// `actor_move_to_point` to the actor's destination, the result.
    pub const MOVE_TO_DESTINATION: i32 = BASE | 8;
}

/// `action_vehicle_control`.
fn vehicle_control(context: &AiStepContext, host: &impl AiHost) {
    let m = host.memory();
    let at = |offset: usize| context.actor + offset as u32;
    let priority = at(a::ORDERS_LOOK_PRIMARY_PRIORITY);
    let direction = at(a::ORDERS_LOOK_PRIMARY_DIRECTION_TYPE);
    if m.bool(at(a::STATE_VEHICLE_LOCK_FACING)) {
        m.set_i16(priority, v::_PRIMARY_PRIORITY_EXACT_FACING as i16);
        m.set_i16(direction, v::_DIRECTION_SPECIFICATION_VECTOR as i16);
        for word in 0..3 {
            m.w32(direction + 4 + word * 4, m.r32(at(a::STATE_VEHICLE_DESTINATION_FACING) + word * 4));
        }
    } else if call_simple(context, host, vehicle_call::HAS_PATH, 0) != 0 {
        m.set_i16(priority, v::_PRIMARY_PRIORITY_FACING as i16);
        m.set_i16(direction, v::_DIRECTION_SPECIFICATION_MOVEMENT as i16);
    } else {
        m.set_i16(priority, v::_PRIMARY_PRIORITY_NONE as i16);
    }
    m.set_i16(at(a::ORDERS_LOOK_IDLE_LOOK_TYPE), 4);
    m.set_bool(at(a::ORDERS_COMBAT_SHOOT_AT_TARGET), false);
    m.set_bool(at(a::ORDERS_MOVE_STATIONARY_CROUCH), false);
    clear_move_orders(m, context.actor);
}

/// `action_vehicle_perform`.
fn vehicle_perform(context: &AiStepContext, host: &impl AiHost) -> bool {
    let m = host.memory();
    let at = |offset: usize| context.actor + offset as u32;
    let done = at(a::STATE_VEHICLE_VEHICLE_ENTRY_DONE);
    let failed = at(a::STATE_VEHICLE_VEHICLE_ENTRY_FAILED);
    let vehicle_index = at(a::STATE_VEHICLE_VEHICLE_INDEX);
    let vehicle_exists = call_simple(context, host, vehicle_call::VEHICLE_EXISTS, m.i32(vehicle_index)) != 0;

    if m.i32(at(a::INPUT_VEHICLE_INDEX)) != NONE {
        m.set_bool(done, true);
    } else if !m.bool(at(a::STATE_VEHICLE_STARTED_ENTRY)) {
        if !vehicle_exists {
            m.w32(vehicle_index, NONE as u32);
            m.set_bool(failed, true);
        } else if call_simple(context, host, vehicle_call::DESIRABLE, 0) == 0 {
            m.set_bool(failed, true);
        } else {
            let current_time = context.game_time;
            let detection_time = at(a::STATE_VEHICLE_STUCK_DETECTION_TIME);
            let counter = at(a::STATE_VEHICLE_STUCK_DETECTION_COUNTER);
            if current_time >= m.i32(detection_time).wrapping_add(150) {
                m.w32(detection_time, current_time as u32);
                if distance_squared(m, at(a::STATE_VEHICLE_STUCK_DETECTION_POINT), at(a::INPUT_BODY_POSITION)) < 25.0 {
                    m.set_i16(counter, m.i16(counter).wrapping_add(1));
                } else {
                    m.set_i16(counter, 0);
                    for word in 0..3 {
                        m.w32(at(a::STATE_VEHICLE_STUCK_DETECTION_POINT) + word * 4, m.r32(at(a::INPUT_BODY_POSITION) + word * 4));
                    }
                }
            }
            if m.i16(counter) < 8 {
                let seat = call_full(context, host, vehicle_call::EVALUATE_SEAT, 0, 0, &[]);
                if seat.result & 1 != 0 {
                    let mut within_range = seat.result & 2 != 0;
                    let mut correct_facing = seat.result & 4 != 0;
                    let could_fake = seat.result & 8 != 0;
                    let entry_point = [seat.output[0], seat.output[1], seat.output[2]];
                    let entry_facing = [seat.output[3], seat.output[4], seat.output[5]];
                    let fake_timer = at(a::STATE_VEHICLE_FAKE_ENTRY_POTENTIAL_TIMER);
                    if could_fake {
                        m.set_i16(fake_timer, m.i16(fake_timer).wrapping_add(1));
                        if m.i16(fake_timer) >= 30 {
                            correct_facing = true;
                            within_range = true;
                        }
                    } else {
                        m.set_i16(fake_timer, 0);
                    }
                    if within_range {
                        if correct_facing {
                            call_simple(context, host, vehicle_call::ENTER_SEAT, 0);
                            m.set_bool(at(a::STATE_VEHICLE_STARTED_ENTRY), true);
                        } else {
                            call_simple(context, host, vehicle_call::MOVE_HALT, 0);
                        }
                    } else if m.bool(at(a::META_TIMESLICE)) {
                        let mut moving = false;
                        if call_simple(context, host, vehicle_call::FIND_DESTINATION, 0) != 0 {
                            moving = call_simple(context, host, vehicle_call::MOVE_TO_DESTINATION, 0) != 0;
                        }
                        let failures = at(a::STATE_VEHICLE_PATHFINDING_FAILURES);
                        if moving {
                            m.set_i16(failures, 0);
                        } else {
                            let maximum: i16 = if m.bool(at(a::STATE_VEHICLE_IMPROMPTU)) { 5 } else { 50 };
                            m.set_i16(failures, m.i16(failures).wrapping_add(1));
                            if m.i16(failures) > maximum {
                                m.set_bool(failed, true);
                            }
                        }
                    }
                    let body = m.vector(at(a::INPUT_BODY_POSITION));
                    let offset = [entry_point[0] - body[0], entry_point[1] - body[1], entry_point[2] - body[2]];
                    m.set_bool(
                        at(a::STATE_VEHICLE_LOCK_FACING),
                        offset[0] * offset[0] + offset[1] * offset[1] + offset[2] * offset[2] < 1.0,
                    );
                    for (i, value) in entry_facing.into_iter().enumerate() {
                        m.set_f32(at(a::STATE_VEHICLE_DESTINATION_FACING) + 4 * i as u32, value);
                    }
                    m.set_bool(at(a::STATE_VEHICLE_CURRENTLY_CORRECT_FACING), correct_facing);
                    m.set_bool(at(a::STATE_VEHICLE_CURRENTLY_WITHIN_RANGE), within_range);
                } else {
                    m.set_bool(failed, true);
                }
            } else {
                m.set_bool(failed, true);
            }
        }
    }
    m.bool(done) || m.bool(failed)
}

/// action_charge.c's calls.
pub mod charge_call {
    const BASE: i32 = (crate::layout::ai_values::_ACTOR_ACTION_CHARGE as i32) << 8;
    /// `actor_path_has_path(actor)`, the result.
    pub const HAS_PATH: i32 = BASE | 1;
    /// `actor_combat_get_firing_variant_definition(actor)`, its address the result.
    pub const FIRING_VARIANT: i32 = BASE | 2;
    /// `actor_has_ranged_weapon(actor)`, the result.
    pub const HAS_RANGED_WEAPON: i32 = BASE | 3;
    /// `actor_get_weapon_definition(actor)`, its address (or 0) the result.
    pub const WEAPON_DEFINITION: i32 = BASE | 4;
    /// `unit_is_busy(unit)`, the result.
    pub const UNIT_IS_BUSY: i32 = BASE | 5;
    /// `projectile_aim_ballistic` of the leap at the target prop: the
    /// result, the aim vector in `output[0..3]`, the vertical and horizontal
    /// velocities in `output[3..5]`.
    pub const AIM_LEAP: i32 = BASE | 6;
    /// `unit_melee_attack_begin(unit, FALSE, input)`, and if it began its
    /// shout at the target's unit `a`: the result.
    pub const MELEE_ATTACK: i32 = BASE | 7;
    /// `actor_move_to_prop(actor, a, input[0])`, the result.
    pub const MOVE_TO_PROP: i32 = BASE | 8;
    /// `actor_move_keep_moving_past_destination(actor)`.
    pub const KEEP_MOVING: i32 = BASE | 9;
    /// `actor_move_halt(actor)`.
    pub const MOVE_HALT: i32 = BASE | 10;
    /// `actor_perception_unreachable(actor, a, b)`.
    pub const PERCEPTION_UNREACHABLE: i32 = BASE | 11;
}

const CHARGE_GOAL_CLOSE_RANGE: i16 = v::_CHARGE_GOAL_CLOSE_RANGE as i16;
const CHARGE_GOAL_STALKING: i16 = v::_CHARGE_GOAL_STALKING as i16;
const CHARGE_GOAL_MELEE: i16 = v::_CHARGE_GOAL_MELEE as i16;
const CHARGE_GOAL_MELEE_LEAPING: i16 = v::_CHARGE_GOAL_MELEE_LEAPING as i16;
const CHARGE_GOAL_VEHICLE_STRAFING: i16 = v::_CHARGE_GOAL_VEHICLE_STRAFING as i16;
const CHARGE_GOAL_VEHICLE_RAMMING: i16 = v::_CHARGE_GOAL_VEHICLE_RAMMING as i16;

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

fn set_vector(m: &dyn AiMemory, address: u32, v: [f32; 3]) {
    for (i, value) in v.into_iter().enumerate() {
        m.set_f32(address + 4 * i as u32, value);
    }
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// `point_from_line3d(p, global_up3d, 0.05)`.
fn above(p: [f32; 3]) -> [f32; 3] {
    [0.0 * 0.05 + p[0], 0.0 * 0.05 + p[1], 1.0 * 0.05 + p[2]]
}

/// `action_charge_find_target_range`.
fn charge_find_target_range(context: &AiStepContext, host: &impl AiHost, goal: i16) -> f32 {
    let m = host.memory();
    let at = |offset: usize| context.actor + offset as u32;
    let definition = context.definition;
    let mut range = 0.0f32;
    if goal == CHARGE_GOAL_MELEE || goal == CHARGE_GOAL_MELEE_LEAPING {
        let upper = m.f32(definition + d::BERSERK_MELEE_LEAP_RANGE_UPPER_BOUND as u32);
        if goal == CHARGE_GOAL_MELEE_LEAPING && !(0.0 > upper) {
            range = upper;
        }
        let attack_range = m.f32(definition + d::BERSERK_MELEE_ATTACK_RANGE as u32);
        if m.bool(at(a::STATE_CHARGE_MELEE_SUICIDE)) {
            if !(range > attack_range) {
                range = attack_range;
            }
        } else {
            let melee_range = attack_range + m.f32(at(a::STATE_CHARGE_MELEE_DANGER_RANGE));
            if !(range > melee_range) {
                range = melee_range;
            }
        }
    } else if (goal == CHARGE_GOAL_VEHICLE_STRAFING || goal == CHARGE_GOAL_CLOSE_RANGE)
        && call_simple(context, host, charge_call::HAS_RANGED_WEAPON, 0) != 0
        && m.i16(at(a::TARGET_TARGET_TYPE)) >= v::_ACTOR_TARGET_ACKNOWLEDGED_ENEMY as i16
    {
        let maximum = m.f32(at(a::CONTROL_WEAPON_MAXIMUM_RANGE));
        if !(0.0 > maximum) {
            range = maximum;
        }
    }
    range
}

/// `action_charge_update`.
fn charge_update(context: &AiStepContext, host: &impl AiHost) {
    let m = host.memory();
    let at = |offset: usize| context.actor + offset as u32;
    if m.i16(at(a::STATE_CHARGE_GOAL)) == CHARGE_GOAL_MELEE_LEAPING
        && m.bool(at(a::STATE_CHARGE_LAUNCHED_LEAP))
        && !m.bool(at(a::STATE_CHARGE_LAUNCHED_MELEE_ATTACK))
        && !m.bool(at(a::INPUT_IN_MIDAIR))
    {
        let timer = at(a::STATE_CHARGE_LEAP_FAILURE_TIMER);
        m.set_i16(timer, m.i16(timer).wrapping_add(1));
    }
}

/// `action_charge_control`.
fn charge_control(context: &AiStepContext, host: &impl AiHost) {
    let m = host.memory();
    let at = |offset: usize| context.actor + offset as u32;
    let flags = m.r32(context.definition + d::FLAGS as u32);
    let goal = m.i16(at(a::STATE_CHARGE_GOAL));
    m.set_i16(at(a::ORDERS_LOOK_PRIMARY_DIRECTION_TYPE), v::_DIRECTION_SPECIFICATION_TARGET as i16);
    m.set_i16(at(a::ORDERS_LOOK_IDLE_LOOK_TYPE), 4);
    let priority = at(a::ORDERS_LOOK_PRIMARY_PRIORITY);
    if (goal == CHARGE_GOAL_MELEE || goal == CHARGE_GOAL_MELEE_LEAPING)
        && m.bool(at(a::STATE_CHARGE_ALIGNMENT_INCORRECT))
        && !m.bool(at(a::CONTROL_MOVING))
        && call_simple(context, host, charge_call::HAS_PATH, 0) == 0
    {
        m.set_i16(priority, 4);
    } else if m.i16(at(a::STATE_COMBAT_STATUS)) >= 5 && goal != CHARGE_GOAL_STALKING {
        m.set_i16(priority, 7);
    } else {
        m.set_i16(priority, 5);
    }

    let stationary = at(a::ORDERS_MOVE_STATIONARY_CROUCH);
    let moving = at(a::ORDERS_MOVE_MOVING_CROUCH);
    if goal == CHARGE_GOAL_STALKING {
        let crouch = !m.bool(at(a::STATE_CHARGE_STALKING_CATCH_TARGET));
        m.set_bool(stationary, crouch);
        m.set_bool(moving, crouch);
    } else if !m.bool(at(a::ORDERS_MOVE_PANICKED))
        && flags & (1 << v::_ACTOR_DEFINITION_DEFENSIVE_CROUCH_WHILE_CHARGING_BIT) != 0
    {
        let crouch = m.r8(at(a::EMOTIONS_DEFENSIVE_CROUCH));
        m.w8(stationary, crouch);
        m.w8(moving, crouch);
    } else {
        m.set_bool(stationary, false);
        m.set_bool(moving, false);
    }

    if m.bool(at(a::STATE_CHARGE_LEAP_PENDING)) {
        let horizontal = m.f32(at(a::STATE_CHARGE_LEAP_HORIZONTAL_VELOCITY));
        let vertical = m.f32(at(a::STATE_CHARGE_LEAP_VERTICAL_VELOCITY));
        m.set_bool(at(a::ORDERS_MOVE_JUMP), true);
        m.set_bool(at(a::ORDERS_MOVE_JUMP_LEAP), vertical < horizontal * 0.7);
        m.set_bool(at(a::ORDERS_MOVE_JUMP_TARGETED), true);
        for word in 0..2 {
            m.w32(at(a::ORDERS_MOVE_JUMP_ALIGNMENT_VECTOR) + word * 4, m.r32(at(a::STATE_CHARGE_LEAP_ALIGNMENT_VECTOR) + word * 4));
        }
        m.set_f32(at(a::ORDERS_MOVE_JUMP_TARGET_HORIZONTAL_VEL), horizontal);
        m.set_f32(at(a::ORDERS_MOVE_JUMP_TARGET_VERTICAL_VEL), vertical);
        m.set_bool(at(a::STATE_CHARGE_LAUNCHED_LEAP), true);
        m.set_bool(at(a::STATE_CHARGE_LEAP_PENDING), false);
        m.w32(at(a::STATE_CHARGE_LEAP_START_TIME), context.game_time as u32);
        m.set_i16(at(a::STATE_CHARGE_LEAP_FAILURE_TIMER), 0);
    }

    if flags & (1 << v::_ACTOR_DEFINITION_BERSERK_USE_PANIC_MOVEMENT_BIT) != 0
        && (m.bool(at(a::EMOTIONS_BERSERK)) || goal == CHARGE_GOAL_MELEE || goal == CHARGE_GOAL_MELEE_LEAPING)
    {
        m.set_bool(at(a::ORDERS_MOVE_PANICKED), m.bool(at(a::STATE_CHARGE_ADVANCING)) && !m.bool(moving));
    }
    m.set_bool(at(a::ORDERS_MOVE_MOVE_FACE_EXACTLY), true);
    m.set_bool(at(a::ORDERS_MOVE_DIVE_INTO_COVER), false);
    m.set_bool(at(a::ORDERS_MOVE_EMERGE_FROM_COVER), false);
    m.set_bool(at(a::ORDERS_COMBAT_SHOOT_AT_TARGET), goal != CHARGE_GOAL_STALKING);
}

/// `action_charge_perform`.
fn charge_perform(context: &AiStepContext, host: &impl AiHost) -> bool {
    use crate::ai_actor::object_get;
    use crate::layout::{actor_debug_info as dbg, unit_datum as u, weapon_definition};
    let m = host.memory();
    let at = |offset: usize| context.actor + offset as u32;
    let definition = context.definition;
    let def = |offset: usize| m.f32(definition + offset as u32);
    let variant = context.variant_definition;
    let flags = m.r32(definition + d::FLAGS as u32);
    let firing_variant = call_simple(context, host, charge_call::FIRING_VARIANT, 0) as u32;
    let goal_address = at(a::STATE_CHARGE_GOAL);
    let advancing = at(a::STATE_CHARGE_ADVANCING);
    let aborted = at(a::STATE_CHARGE_ABORTED_MELEE_ATTACK);
    let launched_melee = at(a::STATE_CHARGE_LAUNCHED_MELEE_ATTACK);
    let launched_leap = at(a::STATE_CHARGE_LAUNCHED_LEAP);
    let leap_pending = at(a::STATE_CHARGE_LEAP_PENDING);
    let target_prop_index = m.i32(at(a::TARGET_TARGET_PROP_INDEX));
    let mut prop = 0u32;

    if target_prop_index != NONE {
        prop = prop_get(m, context, target_prop_index);
        let goal = m.i16(goal_address);
        if m.i32(at(a::INPUT_DELAYED_ATTACHED_PROJECTILE_INDEX)) != NONE {
            m.set_bool(advancing, true);
        } else if goal == CHARGE_GOAL_VEHICLE_RAMMING || goal == CHARGE_GOAL_VEHICLE_STRAFING {
            m.set_bool(advancing, true);
        } else if goal == CHARGE_GOAL_MELEE || goal == CHARGE_GOAL_MELEE_LEAPING {
            let mut abort_range = f32::MAX;
            let mut check_range = true;
            let mut berserk_ranges = m.bool(at(a::EMOTIONS_BERSERK));
            if call_simple(context, host, charge_call::HAS_RANGED_WEAPON, 0) == 0 {
                berserk_ranges = true;
            }
            if m.bool(launched_melee) || m.bool(launched_leap) || m.bool(leap_pending) {
                check_range = false;
            } else if m.bool(at(a::EMOTIONS_BERSERK)) || call_simple(context, host, charge_call::HAS_RANGED_WEAPON, 0) == 0 {
                abort_range = if berserk_ranges {
                    m.f32(variant + var::RANGED_COMBAT_BERSERK_MELEE_ABORT_RANGE as u32)
                } else {
                    m.f32(variant + var::RANGED_COMBAT_MELEE_ABORT_RANGE as u32)
                };
            }
            if m.bool(at(a::EXTERNAL_ORDERS_DISABLE_CHARGING)) {
                let attack_range = def(d::BERSERK_MELEE_ATTACK_RANGE);
                let maximum_abort_range = (if 0.0 > attack_range { 0.0 } else { attack_range }) + 0.8;
                abort_range = if abort_range < maximum_abort_range { abort_range } else { maximum_abort_range };
            }
            let distance = m.f32(prop + p::DISTANCE as u32);
            if check_range && distance > abort_range {
                m.set_bool(aborted, true);
            } else {
                m.w32(at(a::EMOTIONS_LAST_MELEE_ATTACK_TIME), context.game_time as u32);
                m.set_bool(advancing, true);
                if check_range {
                    let possible = at(a::STATE_CHARGE_LEAP_POSSIBLE_IF_AT_RANGE);
                    let lower = def(d::BERSERK_MELEE_LEAP_RANGE_LOWER_BOUND);
                    if goal == CHARGE_GOAL_MELEE {
                        let unreachable = m.i16(prop + p::UNREACHABLE_TICKS as u32);
                        if def(d::BERSERK_MELEE_LEAP_RANGE_UPPER_BOUND) == 0.0 || def(d::BERSERK_MELEE_LEAP_CHANCE) == 0.0 {
                            m.set_bool(possible, false);
                        } else if m.bool(prop + p::FLYING as u32) || unreachable > 0 {
                            m.set_bool(possible, true);
                        }
                        if m.bool(possible) && (unreachable > 0 || distance > lower * 1.5) {
                            m.set_i16(goal_address, CHARGE_GOAL_MELEE_LEAPING);
                        }
                    } else if distance < lower {
                        m.set_i16(goal_address, CHARGE_GOAL_MELEE);
                        m.set_bool(possible, true);
                    }
                }
            }
        } else {
            let stalking = flags & (1 << v::_ACTOR_DEFINITION_STALKING_BEHAVIOR_BIT) != 0
                && m.i16(at(a::STATE_COMBAT_STATUS)) >= 5
                && !m.bool(at(a::EMOTIONS_BERSERK));
            m.set_i16(goal_address, if stalking { CHARGE_GOAL_STALKING } else { CHARGE_GOAL_CLOSE_RANGE });
            let line_of_sight = m.i16(prop + p::LINE_OF_SIGHT as u32);
            let distance = m.f32(prop + p::DISTANCE as u32);
            if stalking {
                let exposed = (line_of_sight == v::_AI_LINE_OF_SIGHT_CLEAR as i16
                    || line_of_sight == v::_AI_LINE_OF_SIGHT_OCCLUDED as i16)
                    && m.i8(prop + p::QUANTIZED_FACING as u32) <= 2;
                m.set_bool(at(a::STATE_CHARGE_STALKING_CURRENTLY_EXPOSED), exposed);
                let freeze = exposed && flags & (1 << v::_ACTOR_DEFINITION_STALKING_FREEZE_WHEN_EXPOSED_BIT) != 0;
                m.set_bool(advancing, !freeze);
                if exposed {
                    let timer = at(a::STATE_CHARGE_STALKING_DISCOVERY_TIMER);
                    m.set_i16(timer, m.i16(timer).wrapping_add(1));
                }
                let catch = at(a::STATE_CHARGE_STALKING_CATCH_TARGET);
                m.set_bool(catch, false);
                let maximum = def(d::DEFENSIVE_STALKING_MAX_DISTANCE);
                if !exposed && m.i8(prop + p::QUANTIZED_CLOSING_SPEED as u32) <= 1 {
                    m.set_bool(catch, true);
                } else if maximum > 0.0 && distance >= maximum {
                    m.set_bool(catch, true);
                }
            } else if call_simple(context, host, charge_call::HAS_RANGED_WEAPON, 0) == 0 || m.bool(at(a::INPUT_UNDERWATER)) {
                m.set_bool(advancing, true);
            } else {
                let (mut minimum_range, maximum_range) = if m.bool(at(a::EMOTIONS_BERSERK)) {
                    (
                        m.f32(firing_variant + var::RANGED_COMBAT_BERSERK_FIRING_RANGE_LOWER_BOUND as u32),
                        m.f32(firing_variant + var::RANGED_COMBAT_BERSERK_FIRING_RANGE_UPPER_BOUND as u32),
                    )
                } else {
                    (
                        m.f32(firing_variant + var::RANGED_COMBAT_COMBAT_RANGE_LOWER_BOUND as u32),
                        m.f32(firing_variant + var::RANGED_COMBAT_COMBAT_RANGE_UPPER_BOUND as u32),
                    )
                };
                let weapon = call_simple(context, host, charge_call::WEAPON_DEFINITION, 0) as u32;
                if weapon != 0 {
                    let weapon_minimum = m.f32(weapon + weapon_definition::WEAPON_AI_MINIMUM_TARGET_RANGE as u32);
                    if weapon_minimum > 0.0 {
                        minimum_range = if minimum_range > weapon_minimum { minimum_range } else { weapon_minimum };
                    }
                }
                if m.bool(advancing) {
                    if distance < minimum_range {
                        m.set_bool(advancing, false);
                    }
                } else if distance > maximum_range {
                    m.set_bool(advancing, true);
                }
                if distance > 0.7
                    && line_of_sight != v::_AI_LINE_OF_SIGHT_CLEAR as i16
                    && line_of_sight != v::_AI_LINE_OF_SIGHT_OCCLUDED as i16
                {
                    m.set_bool(advancing, true);
                }
            }
        }
    } else {
        m.set_bool(advancing, false);
    }

    let goal = m.i16(goal_address);
    let melee_goal = |goal: i16| goal == CHARGE_GOAL_MELEE || goal == CHARGE_GOAL_MELEE_LEAPING;
    if m.bool(launched_melee) {
        let unit_index = m.i32(at(a::META_UNIT_INDEX));
        let finished = unit_index == NONE || call_simple(context, host, charge_call::UNIT_IS_BUSY, 0) == 0;
        m.set_bool(at(a::STATE_CHARGE_FINISHED_MELEE_ATTACK), finished);
    } else if !m.bool(leap_pending) && melee_goal(goal) && prop != 0 {
        let debug = context.debug_info;
        let mut melee = false;
        let body = m.vector(at(a::INPUT_BODY_POSITION));
        let prop_body = m.vector(prop + p::BODY_POSITION as u32);
        let actor_to_prop = m.vector(prop + p::ACTOR_TO_PROP as u32);
        m.w32(debug + dbg::LAST_MELEE_TIME as u32, context.game_time as u32);
        set_vector(m, debug + dbg::FIELD_108 as u32, above(body));
        set_vector(m, debug + dbg::FIELD_114 as u32, m.vector(at(a::INPUT_FACING_VECTOR)));
        set_vector(m, debug + dbg::FIELD_120 as u32, above(prop_body));
        let mut direction;
        if m.f32(prop + p::DISTANCE as u32) < 0.8 {
            m.set_bool(debug + dbg::FIELD_139 as u32, true);
            direction = actor_to_prop;
            melee = true;
        } else {
            let unit = object_get(m, context, m.i32(at(a::META_UNIT_INDEX))).unwrap_or(0);
            let unit_velocity = m.vector(unit + u::OBJECT_TRANSLATIONAL_VELOCITY as u32);
            m.set_bool(debug + dbg::FIELD_139 as u32, false);
            let velocity = m.vector(prop + p::VELOCITY as u32);
            let mut lead_fraction = 0.0f32;
            let speed = dot(velocity, velocity).sqrt();
            if speed > 0.0 {
                lead_fraction = (dot(velocity, actor_to_prop) / speed + 1.0) * 0.5;
            }
            let ticks = m.i16(at(a::STATE_CHARGE_MELEE_TICKS_UNTIL_DANGEROUS)) as f32;
            let t = lead_fraction * ticks;
            let target_point = [velocity[0] * t + prop_body[0], velocity[1] * t + prop_body[1], velocity[2] * t + prop_body[2]];
            direction = [target_point[0] - body[0], target_point[1] - body[1], target_point[2] - body[2]];
            set_vector(m, debug + dbg::FIELD_13C as u32, above(target_point));
            let mut distance;
            if dot(direction, actor_to_prop) < 0.0 {
                distance = 0.0;
                direction = actor_to_prop;
            } else {
                let magnitude = dot(direction, direction).sqrt();
                if !(0.0001 > magnitude.abs()) {
                    let s = 1.0 / magnitude;
                    direction = [s * direction[0], s * direction[1], s * direction[2]];
                    distance = magnitude;
                } else {
                    distance = 0.0;
                }
                if distance == 0.0 {
                    direction = actor_to_prop;
                }
            }

            let lower = def(d::BERSERK_MELEE_LEAP_RANGE_LOWER_BOUND);
            let upper = def(d::BERSERK_MELEE_LEAP_RANGE_UPPER_BOUND);
            let attack_range = def(d::BERSERK_MELEE_ATTACK_RANGE);
            if goal == CHARGE_GOAL_MELEE_LEAPING && !m.bool(launched_leap) {
                m.set_f32(debug + dbg::FIELD_148 as u32, lower);
                m.set_f32(debug + dbg::FIELD_14C as u32, upper);
                if distance < lower && m.i16(prop + p::UNREACHABLE_TICKS as u32) == 0 && !m.bool(prop + p::FLYING as u32) {
                    m.set_bool(aborted, true);
                    m.w32(at(a::EMOTIONS_LAST_MELEE_ATTACK_TIME), NONE as u32);
                } else if distance < upper {
                    let aim = call_full(context, host, charge_call::AIM_LEAP, 0, 0, &[]);
                    if aim.result != 0 {
                        let mut aim_vector = [aim.output[0], aim.output[1]];
                        if normalize2(&mut aim_vector) == 0.0 {
                            let facing = m.vector(at(a::INPUT_FACING_VECTOR));
                            aim_vector = [facing[0], facing[1]];
                            if normalize2(&mut aim_vector) == 0.0 {
                                aim_vector = [1.0, 0.0];
                            }
                        }
                        m.set_bool(leap_pending, true);
                        m.set_f32(at(a::STATE_CHARGE_LEAP_ALIGNMENT_VECTOR), aim_vector[0]);
                        m.set_f32(at(a::STATE_CHARGE_LEAP_ALIGNMENT_VECTOR) + 4, aim_vector[1]);
                        m.set_f32(at(a::STATE_CHARGE_LEAP_HORIZONTAL_VELOCITY), aim.output[4]);
                        m.set_f32(at(a::STATE_CHARGE_LEAP_VERTICAL_VELOCITY), aim.output[3]);
                    }
                }
            } else if m.bool(at(a::STATE_CHARGE_MELEE_SUICIDE)) {
                m.set_f32(debug + dbg::FIELD_148 as u32, 0.0);
                m.set_f32(debug + dbg::FIELD_14C as u32, attack_range);
                if distance < attack_range {
                    melee = true;
                } else if distance < def(d::BERSERK_SUICIDE_SENSING_DISTANCE) {
                    let relative = [velocity[0] - unit_velocity[0], velocity[1] - unit_velocity[1], velocity[2] - unit_velocity[2]];
                    if dot(relative, direction) > 0.023333333 {
                        melee = true;
                    }
                }
            } else {
                if goal == CHARGE_GOAL_MELEE_LEAPING && m.bool(launched_leap) {
                    distance -= dot(direction, unit_velocity) * ticks;
                }
                let danger_range = m.f32(at(a::STATE_CHARGE_MELEE_DANGER_RANGE));
                m.set_f32(debug + dbg::FIELD_148 as u32, danger_range);
                m.set_f32(debug + dbg::FIELD_14C as u32, danger_range + attack_range);
                if distance < danger_range + attack_range {
                    melee = true;
                }
            }
        }

        if m.bool(leap_pending) || (melee && !m.bool(at(a::STATE_CHARGE_MELEE_SUICIDE))) {
            let mut alignment = [direction[0], direction[1]];
            if normalize2(&mut alignment) > 0.0 {
                let minimum_alignment = if m.bool(launched_leap) { 0.0 } else { 0.866_025_4 };
                let facing = m.vector(at(a::INPUT_FACING_VECTOR));
                if alignment[0] * facing[0] + alignment[1] * facing[1] < minimum_alignment {
                    melee = false;
                    m.set_bool(leap_pending, false);
                    m.set_bool(at(a::STATE_CHARGE_ALIGNMENT_INCORRECT), true);
                }
            }
        }
        set_vector(m, debug + dbg::FIELD_12C as u32, direction);
        m.set_bool(debug + dbg::FIELD_138 as u32, melee);
        if melee {
            let mut alignment = [direction[0], direction[1]];
            if normalize2(&mut alignment) == 0.0 {
                let facing = m.vector(at(a::INPUT_FACING_VECTOR));
                alignment = [facing[0], facing[1]];
            }
            let began = call_full(context, host, charge_call::MELEE_ATTACK, m.i32(prop + p::UNIT_INDEX as u32), 0, &alignment);
            if began.result != 0 {
                m.set_bool(launched_melee, true);
            }
        }
    }

    let time = context.game_time;
    let goal = m.i16(goal_address);
    if melee_goal(goal) && !m.bool(launched_melee) && !m.bool(leap_pending) {
        if m.bool(launched_leap) {
            if m.i16(at(a::STATE_CHARGE_LEAP_FAILURE_TIMER)) > 15 {
                m.set_bool(aborted, true);
            }
        } else {
            let charge_timer = def(d::BERSERK_MELEE_ATTACK_CHARGE_TIMER);
            if charge_timer > 0.0 && (time as f32) >= m.i32(at(a::STATE_CHARGE_CHARGE_START_TIME)) as f32 + charge_timer * 30.0 {
                m.set_bool(aborted, true);
            }
        }
    }
    if goal == CHARGE_GOAL_VEHICLE_STRAFING || goal == CHARGE_GOAL_VEHICLE_RAMMING {
        m.w32(at(a::EMOTIONS_LAST_VEHICLE_CHARGE_TIME), time as u32);
    }
    let target_range = charge_find_target_range(context, host, goal);
    m.set_f32(at(a::STATE_CHARGE_ACCEPTABLE_TARGET_RANGE), target_range);

    let unable = at(a::STATE_CHARGE_UNABLE_TO_ADVANCE);
    if !m.bool(at(a::META_SWARM)) && m.bool(at(a::META_TIMESLICE)) {
        let mut moving = false;
        m.set_bool(unable, false);
        if !m.bool(launched_melee) && !m.bool(launched_leap) && !m.bool(leap_pending) && m.bool(advancing) {
            let minimum_move_range: f32 = if goal == CHARGE_GOAL_MELEE_LEAPING { 4.0 } else { 1.5 };
            let move_range = if minimum_move_range > target_range { minimum_move_range } else { target_range };
            let target_prop_index = m.i32(at(a::TARGET_TARGET_PROP_INDEX));
            if call_full(context, host, charge_call::MOVE_TO_PROP, target_prop_index, 0, &[move_range]).result != 0 {
                call_simple(context, host, charge_call::KEEP_MOVING, 0);
                moving = true;
            } else {
                m.set_bool(unable, true);
                m.set_bool(advancing, false);
            }
        }
        if !moving {
            call_simple(context, host, charge_call::MOVE_HALT, 0);
        }
        if m.i16(at(a::TARGET_TARGET_TYPE)) >= v::_ACTOR_TARGET_ACKNOWLEDGED_ENEMY as i16 {
            let target_prop_index = m.i32(at(a::TARGET_TARGET_PROP_INDEX));
            let target_prop = prop_get(m, context, target_prop_index);
            let acceptable = m.f32(at(a::STATE_CHARGE_ACCEPTABLE_TARGET_RANGE));
            let mut unreachable = false;
            let mut out_of_range = m.f32(target_prop + p::DISTANCE as u32) > acceptable;
            if melee_goal(goal) && (m.bool(launched_leap) || m.bool(leap_pending) || m.bool(launched_melee)) {
                out_of_range = false;
            }
            if out_of_range {
                if m.bool(unable) || call_simple(context, host, charge_call::HAS_PATH, 0) == 0 {
                    unreachable = true;
                } else if m.f32(at(a::CONTROL_PATH_PATH_ENDPOINT_TARGET_RADIUS)) > acceptable {
                    unreachable = true;
                }
            }
            call_full(context, host, charge_call::PERCEPTION_UNREACHABLE, target_prop_index, unreachable as i32, &[]);
        }
    }

    if melee_goal(goal) {
        m.bool(aborted) || m.bool(at(a::STATE_CHARGE_FINISHED_MELEE_ATTACK)) || m.bool(unable)
    } else if goal == CHARGE_GOAL_VEHICLE_STRAFING || goal == CHARGE_GOAL_VEHICLE_RAMMING {
        m.bool(unable)
    } else {
        false
    }
}
