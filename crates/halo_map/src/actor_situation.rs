//! An actor's situation (`actor_perception.c`'s `actor_situation_update`,
//! every tick of an actor's update): its count of the enemies it knows and
//! the threats they pose, of the friends near it, in view and close by type,
//! the prop it targets (the heaviest by target weight, and that target's
//! status), and its combat status and the timers that follow it.
//!
//! The actor's datum and props are the game's memory, worked on in place
//! ([`crate::ai_actor`]); a unit's actor, player and active regions and a
//! prop's target weight are the game's, through [`AiHost::call`]
//! (actor_perception.c's `actor_situation_call`, shared with the C).

use crate::ai_actor::{AiCall, AiHost, AiMemory, AiMemoryExt, AiStepContext, NONE, PropIterator, datum_get, prop_get};
use crate::layout::{actor_datum as a, actor_situation as s, ai_values as v, prop_datum as p};

/// The situation update's calls (halo_sim.h's `_actor_situation_call_*`).
pub mod call {
    /// `unit_get(a)->unit.actor_index`, the result.
    pub const UNIT_ACTOR: i32 = 1;
    /// `unit_get(a)->unit.player_index`, the result.
    pub const UNIT_PLAYER: i32 = 2;
    /// The unit `a`'s active region flags, the result.
    pub const UNIT_ACTIVE_REGION_FLAGS: i32 = 3;
    /// `actor_compute_prop_target_weight(actor, a)`, in `output[0]`.
    pub const TARGET_WEIGHT: i32 = 4;
}

/// The local enums (actor_perception.c's).
const MODE_COMBAT: i16 = 3;
const COMBAT_STATUS_NONE: i16 = 0;
const COMBAT_STATUS_CERTAIN: i16 = 4;
const COMBAT_STATUS_VISIBLE: i16 = 7;

/// `global_combat_status_table`: the combat status each target type gives.
const COMBAT_STATUS_TABLE: [i16; v::NUMBER_OF_ACTOR_TARGET_TYPES] = [0, 0, 0, 1, 2, 2, 3, 4, 5, 5, 7, 7];

fn call_simple(context: &AiStepContext, host: &impl AiHost, kind: i32, a: i32) -> AiCall {
    let mut call = AiCall { kind, actor: context.actor_index, a, ..AiCall::default() };
    host.call(&mut call);
    call
}

/// `char` counters, incremented as the C's.
fn increment(m: &dyn AiMemory, address: u32) {
    m.w8(address, m.r8(address).wrapping_add(1));
}

/// `prop_acknowledged`.
fn acknowledged(state: i16) -> bool {
    state >= v::_PROP_STATE_BECOMING_UNACKNOWLEDGED as i16 && state <= v::_PROP_STATE_ACKNOWLEDGED as i16
}

/// `actor_situation_update`.
pub fn actor_situation_update(context: &AiStepContext, host: &impl AiHost) {
    let m = host.memory();
    let actor = context.actor;
    let situation = actor + a::SITUATION as u32;
    let threat = |kind: usize| situation + s::KNOWN_ENEMIES as u32 + 2 + kind as u32;
    let cumulative = |kind: usize| threat(kind) + v::NUMBER_OF_ACTOR_THREAT_TYPES as u32;
    let charging = m.bool(actor + a::EMOTIONS_BERSERK as u32)
        || m.i16(actor + a::STATE_ACTION as u32) == v::_ACTOR_ACTION_CHARGE as i16;
    let mut best_prop_index = NONE;
    let mut best_target_weight = 0.0f32;

    for offset in 0..s::SIZEOF as u32 {
        m.w8(situation + offset, 0);
    }
    let mut iterator = PropIterator::new(m, context);
    while let Some(prop) = iterator.next(m, context) {
        if acknowledged(m.i16(prop + p::STATE as u32)) && !m.bool(prop + p::DEAD as u32) {
            if m.bool(prop + p::ENEMY as u32) {
                let visible = m.i16(prop + p::VISIBILITY as u32) >= v::_ACTOR_PERCEPTION_FULL as i16;
                let shooting = m.bool(prop + p::SHOOTING as u32);
                let facing = m.i8(prop + p::QUANTIZED_FACING as u32);
                let mut priority = v::_ACTOR_THREAT_NONE;
                let mut threaten = |kind: usize| {
                    increment(m, cumulative(kind));
                    priority = priority.max(kind);
                };

                increment(m, situation + s::KNOWN_ENEMIES as u32);
                if visible && m.i16(prop + p::UNREACHABLE_TICKS as u32) == 0 {
                    increment(m, situation + s::VISIBLE_REACHABLE_ENEMIES as u32);
                }
                if visible
                    || (shooting && m.i16(prop + p::LINE_OF_SIGHT as u32) == v::_AI_LINE_OF_SIGHT_CLEAR as i16)
                {
                    if visible {
                        threaten(v::_ACTOR_THREAT_VISIBLE);
                    }
                    if m.bool(prop + p::CURRENTLY_DAMAGING_ME as u32) {
                        threaten(v::_ACTOR_THREAT_DAMAGING_ME);
                    }
                    if shooting {
                        threaten(v::_ACTOR_THREAT_SHOOTING);
                    }
                    if facing <= 2 {
                        if visible {
                            threaten(v::_ACTOR_THREAT_VISIBLE_FACING_ME);
                            if !charging && m.f32(prop + p::DISTANCE as u32) < 2.0 {
                                threaten(v::_ACTOR_THREAT_EXTREMELY_CLOSE_TO_ME);
                            }
                        }
                        if facing <= 1 {
                            if shooting {
                                threaten(v::_ACTOR_THREAT_SHOOTING_NEAR_ME);
                            }
                            if facing <= 0 {
                                if visible {
                                    threaten(v::_ACTOR_THREAT_VISIBLE_AIMING_AT_ME);
                                }
                                if shooting {
                                    threaten(v::_ACTOR_THREAT_SHOOTING_AT_ME);
                                }
                            }
                        }
                    }
                }
                increment(m, threat(priority));
            } else {
                friend_update(context, host, prop);
            }
        }

        let target_weight = m.f32(prop + p::TARGET_WEIGHT as u32);
        if target_weight > best_target_weight {
            best_prop_index = iterator.index;
            best_target_weight = target_weight;
        }
    }

    let target_prop_index = actor + a::TARGET_TARGET_PROP_INDEX as u32;
    if best_prop_index != m.i32(target_prop_index) {
        let old_target_prop_index = m.i32(target_prop_index);

        m.set_i16(actor + a::TARGET_TARGET_TYPE as u32, v::_ACTOR_TARGET_NONE as i16);
        m.w32(target_prop_index, best_prop_index as u32);
        m.w32(actor + a::TARGET_TARGET_LAST_VISIBLE_TIME as u32, NONE as u32);
        for prop_index in [old_target_prop_index, best_prop_index] {
            if prop_index != NONE {
                let prop = prop_get(m, context, prop_index);
                let weight = call_simple(context, host, call::TARGET_WEIGHT, prop_index).output[0];
                if prop != 0 {
                    m.set_f32(prop + p::TARGET_WEIGHT as u32, weight);
                }
            }
        }
    }

    target_status_update(context, host);
    combat_status_update(m, actor);
}

/// A friend's part in the situation: near the actor (or fighting its
/// target), in view, close by.
fn friend_update(context: &AiStepContext, host: &impl AiHost, prop: u32) {
    let m = host.memory();
    let actor = context.actor;
    let situation = actor + a::SITUATION as u32;
    let unit_index = m.i32(prop + p::UNIT_INDEX as u32);
    let friend_actor_index = call_simple(context, host, call::UNIT_ACTOR, unit_index).result;
    let friend_actor =
        if friend_actor_index == NONE { None } else { datum_get(m, context.actor_data, friend_actor_index) };
    let actor_type = if call_simple(context, host, call::UNIT_PLAYER, unit_index).result != NONE {
        v::_ACTOR_PLAYER as i16
    } else if let Some(friend_actor) = friend_actor {
        m.i16(friend_actor + a::META_TYPE as u32)
    } else {
        v::_ACTOR_NONE as i16
    };
    let distance = m.f32(prop + p::DISTANCE as u32);
    let fighting = m.bool(prop + p::FIGHTING as u32);
    let mut area = false;
    let mut visible = false;
    let mut close = false;

    if distance < 8.0 {
        area = true;
    } else if fighting
        && let Some(friend_actor) = friend_actor
        && m.i32(actor + a::TARGET_TARGET_PROP_INDEX as u32) != NONE
        && m.i32(friend_actor + a::TARGET_TARGET_PROP_INDEX as u32) != NONE
    {
        let target_prop = prop_get(m, context, m.i32(actor + a::TARGET_TARGET_PROP_INDEX as u32));
        let friend_target_prop = prop_get(m, context, m.i32(friend_actor + a::TARGET_TARGET_PROP_INDEX as u32));
        if m.i32(target_prop + p::UNIT_INDEX as u32) == m.i32(friend_target_prop + p::UNIT_INDEX as u32) {
            area = true;
        }
    }
    let line_of_sight = m.i16(prop + p::LINE_OF_SIGHT as u32);
    if line_of_sight == v::_AI_LINE_OF_SIGHT_CLEAR as i16 || line_of_sight == v::_AI_LINE_OF_SIGHT_OCCLUDED as i16 {
        visible = true;
        close = distance < 3.0;
    }

    let by_type = |offset: usize| situation + offset as u32 + actor_type as u32;
    if area {
        increment(m, situation + s::AREA_FRIENDS as u32);
        if fighting {
            increment(m, situation + s::AREA_FIGHTING_FRIENDS as u32);
        }
        if fighting && m.bool(prop + p::VEHICLE_GUNNER as u32) {
            increment(m, situation + s::AREA_FIRE_SUPPORT_FRIENDS as u32);
        }
        increment(m, by_type(s::AREA_FRIENDS_BY_TYPE));
        if fighting {
            increment(m, by_type(s::AREA_FIGHTING_FRIENDS_BY_TYPE));
        }
    }
    if visible {
        increment(m, situation + s::VISIBLE_FRIENDS as u32);
        if fighting {
            increment(m, situation + s::VISIBLE_FIGHTING_FRIENDS as u32);
        }
        increment(m, by_type(s::VISIBLE_FRIENDS_BY_TYPE));
        if fighting {
            increment(m, by_type(s::VISIBLE_FIGHTING_FRIENDS_BY_TYPE));
        }
    }
    if close {
        increment(m, situation + s::CLOSE_FRIENDS as u32);
        if fighting {
            increment(m, situation + s::CLOSE_FIGHTING_FRIENDS as u32);
        }
        increment(m, by_type(s::CLOSE_FRIENDS_BY_TYPE));
        if fighting {
            increment(m, by_type(s::CLOSE_FIGHTING_FRIENDS_BY_TYPE));
        }
    }
}

/// `actor_situation_update_target_status`: the target's type, when it was
/// last seen, and whether it is outside the actor's active area (the C's
/// view names it so; actors.h's `target_really_alive`).
fn target_status_update(context: &AiStepContext, host: &impl AiHost) {
    let m = host.memory();
    let actor = context.actor;
    let target_prop_index = actor + a::TARGET_TARGET_PROP_INDEX as u32;
    let last_visible_time = actor + a::TARGET_TARGET_LAST_VISIBLE_TIME as u32;
    let outside_active_area = actor + a::TARGET_TARGET_REALLY_ALIVE as u32;

    if m.i32(target_prop_index) == NONE {
        m.set_i16(actor + a::TARGET_TARGET_TYPE as u32, 0);
        m.w32(last_visible_time, NONE as u32);
        m.set_bool(outside_active_area, false);
        return;
    }
    let target_prop = prop_get(m, context, m.i32(target_prop_index));
    let state = m.i16(target_prop + p::STATE as u32);
    let dead = m.bool(target_prop + p::DEAD as u32);
    let target_type: i16 = match state as usize {
        v::_PROP_STATE_UNACKNOWLEDGED => {
            m.w32(target_prop_index, NONE as u32);
            m.w32(last_visible_time, NONE as u32);
            0
        }
        v::_PROP_STATE_BECOMING_ACKNOWLEDGED => 1,
        v::_PROP_STATE_BECOMING_UNACKNOWLEDGED | v::_PROP_STATE_ACKNOWLEDGED => {
            let line_of_sight = m.i16(target_prop + p::LINE_OF_SIGHT as u32);
            if dead {
                2
            } else if m.bool(target_prop + p::CURRENTLY_DAMAGING_ME as u32) {
                11
            } else if m.i16(target_prop + p::VISIBILITY as u32) >= 2 {
                10
            } else if line_of_sight != 0 && line_of_sight != 1 {
                7
            } else if m.i8(target_prop + p::QUANTIZED_FACING as u32) <= 2
                && m.f32(target_prop + p::DISTANCE as u32) < 6.0
            {
                9
            } else {
                8
            }
        }
        v::_PROP_STATE_INSPECTED_ORPHAN => {
            if dead {
                2
            } else if m.bool(target_prop + p::ABANDONED_SEARCH as u32) {
                3
            } else {
                4
            }
        }
        v::_PROP_STATE_UNINSPECTED_ORPHAN => m.bool(target_prop + p::DEFINITELY_LOCATED as u32) as i16 + 5,
        // (the C's assertion ends the game)
        _ => return,
    };
    m.set_i16(actor + a::TARGET_TARGET_TYPE as u32, target_type);

    if acknowledged(state) {
        m.set_bool(outside_active_area, !dead);
        if m.i16(target_prop + p::VISIBILITY as u32) > 0 {
            m.w32(last_visible_time, m.r32(target_prop + p::LAST_VISIBLE_TIME as u32));
        }
    } else {
        let unit_index = m.i32(target_prop + p::UNIT_INDEX as u32);
        let flags = call_simple(context, host, call::UNIT_ACTIVE_REGION_FLAGS, unit_index).result;
        m.set_bool(outside_active_area, flags & (1 << 2) == 0);
    }
}

/// `actor_situation_combat_status_update`.
fn combat_status_update(m: &dyn AiMemory, actor: u32) {
    let stimulus_status = actor + a::STIMULI_SUSPICION_COMBAT_STATUS as u32;
    let suspicion_status = actor + a::STATE_SUSPICION_COMBAT_STATUS as u32;
    let suspicion_timer = actor + a::STATE_SUSPICION_TIMER as u32;
    let combat_status = actor + a::STATE_COMBAT_STATUS as u32;
    let increment_long = |address: u32| m.w32(address, m.r32(address).wrapping_add(1));

    if m.i16(stimulus_status) > COMBAT_STATUS_NONE {
        let stimulus_timer = m.i32(actor + a::STIMULI_SUSPICION_TIMER as u32);
        if m.i16(suspicion_status) < m.i16(stimulus_status) {
            m.set_i16(suspicion_status, m.i16(stimulus_status));
            m.w32(suspicion_timer, stimulus_timer as u32);
        } else if m.i16(suspicion_status) == m.i16(stimulus_status) {
            m.w32(suspicion_timer, m.i32(suspicion_timer).max(stimulus_timer) as u32);
        }
        m.set_i16(stimulus_status, COMBAT_STATUS_NONE);
    }

    let target_type = m.i16(actor + a::TARGET_TARGET_TYPE as u32);
    let table = COMBAT_STATUS_TABLE.get(target_type as usize).copied().unwrap_or(0);
    let status = m
        .i16(suspicion_status)
        .max(m.i16(actor + a::STATE_ARTIFICIAL_COMBAT_STATUS as u32).max(table));
    m.set_i16(combat_status, status);
    if status > m.i16(suspicion_status) {
        m.set_i16(suspicion_status, COMBAT_STATUS_NONE);
    }

    let combat_mode_timer = actor + a::STATE_COMBAT_MODE_TIMER as u32;
    if m.i16(actor + a::STATE_MODE as u32) < MODE_COMBAT {
        m.w32(combat_mode_timer, 0);
    } else {
        increment_long(combat_mode_timer);
    }
    let in_combat_timer = actor + a::STATE_IN_COMBAT_TIMER as u32;
    if status == COMBAT_STATUS_NONE {
        m.w32(in_combat_timer, 0);
    } else {
        increment_long(in_combat_timer);
    }
    let certain_timer = actor + a::STATE_CERTAIN_COMBAT_TIMER as u32;
    let uncertain_timer = actor + a::STATE_UNCERTAIN_COMBAT_TIMER as u32;
    if status >= COMBAT_STATUS_CERTAIN {
        increment_long(certain_timer);
        m.w32(uncertain_timer, 0);
    } else {
        m.w32(certain_timer, 0);
        if m.i32(uncertain_timer) != NONE {
            increment_long(uncertain_timer);
        }
    }
    if status >= COMBAT_STATUS_VISIBLE {
        m.set_bool(actor + a::STATE_HAD_VISIBLE_ENEMY as u32, true);
    }
}
