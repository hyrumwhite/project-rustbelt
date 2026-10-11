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
