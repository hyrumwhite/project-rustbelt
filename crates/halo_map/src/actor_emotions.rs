//! An actor's emotions (`actor_perception.c`'s `actor_emotion_update`, every
//! tick of an actor's update): its berserk state's end, the defending it was
//! ordered (and the shout it gives), whether it is forced to charge, the
//! danger it feels and how it settles, whether it blocks a friend's line of
//! fire or walks into a player's (`actor_perception_aiming_vector_test_blockage`
//! here too), and its defensive crouch.
//!
//! The actor's datum, props and definition are the game's memory, worked on
//! in place ([`crate::ai_actor`]); a friend's attack, a shout, berserk's end,
//! a discarded firing position and the unopposable retreat are the game's,
//! through [`AiHost::call`] (actor_perception.c's `actor_emotion_call`, shared
//! with the C). The flood's shamble (actor_type_flood.c's
//! `actor_type_flood_desire_shamble`) is here too.

use crate::ai_actor::{AiCall, AiHost, AiMemoryExt, AiStepContext, NONE, PropIterator, prop_get};
use crate::features::{dot3, magnitude_squared3, normalize3, sub3};
use crate::layout::{actor_datum as a, actor_defensive_properties as d, actor_definition, ai_values as v, prop_datum as p};

/// The emotion update's calls (halo_sim.h's `_actor_emotion_call_*`).
pub mod call {
    /// `actor_berserk(actor, FALSE)`.
    pub const BERSERK_END: i32 = 1;
    /// `ai_communication_event(a, b (the unit), NONE...)`.
    pub const COMMUNICATION_EVENT: i32 = 2;
    /// `actor_perception_friend_prop_is_attacking(actor, a)`: the result, and
    /// the attack's vector in `output`.
    pub const FRIEND_ATTACKING: i32 = 3;
    /// `actor_discard_firing_position(actor, a, TRUE)`.
    pub const DISCARD_FIRING_POSITION: i32 = 4;
    /// `actor_type_flood_desire_shamble(actor)`.
    pub const FLOOD_SHAMBLE: i32 = 5;
    /// `actor_emotion_unopposable_retreat(actor)`.
    pub const UNOPPOSABLE_RETREAT: i32 = 6;
    /// `unit_is_busy` of the actor's unit, the result.
    pub const UNIT_BUSY: i32 = 7;
    /// `actor_path_has_path(actor)`, the result.
    pub const HAS_PATH: i32 = 8;
    /// `actor_combat_get_firing_variant_definition(actor)`, its address the
    /// result.
    pub const FIRING_VARIANT: i32 = 9;
    /// Whether the actor's unit is in a hard ping, the result, and its melee
    /// attack inhibited, `result2`.
    pub const UNIT_SHAMBLE_STATE: i32 = 10;
    /// `actor_has_ranged_weapon(actor)`, the result.
    pub const HAS_RANGED_WEAPON: i32 = 11;
    /// `real_seed_random(global seed)`, in `output[0]`.
    pub const RANDOM: i32 = 12;
    /// `real_random_range(input[0], input[1])`, in `output[0]`.
    pub const RANDOM_RANGE: i32 = 13;
}

/// The local enums (actor_perception.c's).
const COMBAT_STATUS_NONE: i16 = 0;
const COMBAT_STATUS_DEFINITE: i16 = 3;
const MODE_COMBAT: i16 = 3;
const DEFENSIVE_CROUCH_DANGER: i16 = 1;
const DEFENSIVE_CROUCH_SHIELD_LOW: i16 = 2;
const DEFENSIVE_CROUCH_HIDE_BEHIND_SHIELD: i16 = 3;
const DEFENSIVE_CROUCH_ANY_TARGET: i16 = 4;
const DEFENSIVE_CROUCH_FLOOD_SHAMBLE: i16 = 5;

const EPSILON: f32 = 0.0001;

/// `(short)f` as x86 converts it (a float beyond a long gives 0x80000000).
fn c_short(value: f32) -> i16 {
    let long = if (-2_147_483_648.0..2_147_483_648.0).contains(&value) { value as i32 } else { i32::MIN };
    long as i16
}

fn point_from_line3(p: [f32; 3], v: [f32; 3], t: f32) -> [f32; 3] {
    [v[0] * t + p[0], v[1] * t + p[1], v[2] * t + p[2]]
}

/// `actor_perception_aiming_vector_test_blockage`: how far a friend at
/// `friend_position` blocks the aim from `source_position` along
/// `source_vector` (0 none, 1 partly, 2 fully), and the friend's direction
/// to the line of fire (only when the friend is ahead).
pub fn aiming_vector_test_blockage(
    source_position: [f32; 3],
    source_vector: [f32; 3],
    friend_position: [f32; 3],
    friend_direction_to_aiming_vector: Option<&mut [f32; 3]>,
) -> i16 {
    let mut planar = [source_vector[0], source_vector[1]];
    let mut horizontal_aiming_magnitude = (planar[0] * planar[0] + planar[1] * planar[1]).sqrt();
    if !(EPSILON > horizontal_aiming_magnitude.abs()) {
        let inverse_magnitude = 1.0 / horizontal_aiming_magnitude;
        planar[0] *= inverse_magnitude;
        planar[1] *= inverse_magnitude;
    } else {
        horizontal_aiming_magnitude = 0.0;
    }
    if !(horizontal_aiming_magnitude > 0.0) {
        return 0;
    }
    let friend_vector = sub3(friend_position, source_position);
    let mut projection = friend_vector[0] * planar[0] + friend_vector[1] * planar[1];
    let mut friend_distance = (friend_vector[0] * friend_vector[0] + friend_vector[1] * friend_vector[1]).sqrt();
    friend_distance *= 0.866_025_4;
    if !(projection > friend_distance) {
        return 0;
    }
    projection = -projection;
    let blockage_vector = [
        friend_vector[0] + projection * source_vector[0],
        friend_vector[1] + projection * source_vector[1],
        friend_vector[2] + projection * source_vector[2],
    ];
    if let Some(out) = friend_direction_to_aiming_vector {
        *out = [-blockage_vector[0], -blockage_vector[1], -blockage_vector[2]];
    }
    let mut blockage = if blockage_vector[2] > -0.5 && blockage_vector[2] < 0.9 {
        2
    } else if blockage_vector[2] > -0.8 && blockage_vector[2] < 1.2 {
        1
    } else {
        return 0;
    };
    let horizontal_error_squared = blockage_vector[0] * blockage_vector[0] + blockage_vector[1] * blockage_vector[1];
    if !(horizontal_error_squared < 0.36) {
        blockage = if horizontal_error_squared < 1.21 { 1 } else { 0 };
    }
    blockage
}

/// `actor_emotion_update`.
pub fn actor_emotion_update(context: &AiStepContext, host: &impl AiHost) {
    let m = host.memory();
    let actor = context.actor;
    let at = |offset: usize| actor + offset as u32;
    let definition = context.definition;
    let flags = m.r32(definition + actor_definition::FLAGS as u32);
    let flag = |bit: usize| flags & (1 << bit) != 0;
    let defensive = definition + actor_definition::DEFENSIVE as u32;
    let call = |kind: i32, x: i32, y: i32| {
        let mut call = AiCall { kind, actor: context.actor_index, a: x, b: y, ..Default::default() };
        host.call(&mut call);
        call
    };

    let combat_status = m.i16(at(a::STATE_COMBAT_STATUS));
    if m.bool(at(a::EMOTIONS_BERSERK))
        && (combat_status == COMBAT_STATUS_NONE
            || m.i16(at(a::STATE_MODE)) < MODE_COMBAT
            || (m.f32(at(a::INPUT_SHIELD_VITALITY)) == 1.0 && combat_status < COMBAT_STATUS_DEFINITE))
    {
        call(call::BERSERK_END, 0, 0);
    }

    if m.r8(at(a::EMOTIONS_CURRENTLY_DEFENDING)) != m.r8(at(a::EXTERNAL_ORDERS_DEFENDING)) {
        let defending = m.r8(at(a::EXTERNAL_ORDERS_DEFENDING));
        m.w8(at(a::EMOTIONS_CURRENTLY_DEFENDING), defending);
        let unit_index = m.i32(at(a::META_UNIT_INDEX));
        if unit_index != NONE {
            call(call::COMMUNICATION_EVENT, (defending != 0) as i32 + v::_AI_COMMUNICATION_ADVANCE as i32, unit_index);
        }
    }

    m.set_bool(
        at(a::EMOTIONS_FORCED_TO_CHARGE),
        m.bool(at(a::EMOTIONS_BERSERK)) || flag(v::_ACTOR_DEFINITION_ALWAYS_CHARGE_BIT),
    );
    if m.i32(at(a::INPUT_VEHICLE_INDEX)) != NONE {
        m.set_bool(at(a::EMOTIONS_FORCED_TO_CHARGE), false);
    } else if flag(v::_ACTOR_DEFINITION_CHARGE_IN_ATTACKING_MODE_BIT) && !m.bool(at(a::EMOTIONS_CURRENTLY_DEFENDING)) {
        m.set_bool(at(a::EMOTIONS_FORCED_TO_CHARGE), true);
    }

    // (the scan starts one past the specific threats, at the first of the
    // cumulative ones, as the game's does: nothing writes it, so it is 0)
    let mut priority = v::NUMBER_OF_ACTOR_THREAT_TYPES as i16;
    while priority > 0 && m.i8(at(a::SITUATION_SPECIFIC_THREATS) + priority as u32) <= 0 {
        priority -= 1;
    }
    let instantaneous_danger = if priority >= v::_ACTOR_THREAT_DAMAGING_ME as i16 {
        2.0
    } else if priority >= v::_ACTOR_THREAT_EXTREMELY_CLOSE_TO_ME as i16 {
        1.8
    } else if priority >= v::_ACTOR_THREAT_SHOOTING_AT_ME as i16 {
        1.6
    } else if priority >= v::_ACTOR_THREAT_SHOOTING_NEAR_ME as i16 {
        1.2
    } else if priority >= v::_ACTOR_THREAT_VISIBLE_AIMING_AT_ME as i16 {
        0.7
    } else {
        0.0
    };
    m.set_f32(at(a::EMOTIONS_INSTANTANEOUS_DANGER), instantaneous_danger);
    {
        let interpolation = 1.0f32 - (-0.046_209_812_164_306_640_625f64).exp() as f32;
        let perceived = m.f32(at(a::EMOTIONS_PERCEIVED_DANGER));
        m.set_f32(
            at(a::EMOTIONS_PERCEIVED_DANGER),
            (m.f32(at(a::EMOTIONS_INSTANTANEOUS_DANGER)) - perceived) * interpolation + perceived,
        );
    }

    if m.bool(at(a::EXTERNAL_ORDERS_STAND_DOWN)) {
        m.w32(at(a::EMOTIONS_ORIGINAL_BODY_VITALITY), m.r32(at(a::INPUT_BODY_VITALITY)));
    }

    let crouch_in_line_of_fire = flag(v::_ACTOR_DEFINITION_CROUCH_IN_LINE_OF_FIRE_BIT);
    let avoid_friend_line_of_fire = flag(v::_ACTOR_DEFINITION_AVOID_FRIEND_LINE_OF_FIRE_BIT);
    if crouch_in_line_of_fire || avoid_friend_line_of_fire {
        if m.i32(at(a::INPUT_VEHICLE_INDEX)) == NONE && m.i16(at(a::STATE_COMBAT_STATUS)) >= COMBAT_STATUS_DEFINITE {
            m.set_bool(at(a::EMOTIONS_CROUCH_BLOCKING_LINE_OF_FIRE), false);
            m.set_bool(at(a::EMOTIONS_CROUCH_BLOCKING_PLAYER_LINE_OF_FIRE), false);
            m.set_bool(at(a::EMOTIONS_CROUCH_FRIENDS_IN_LINE_OF_FIRE), false);
            m.set_bool(at(a::EMOTIONS_MOVING_INTO_PLAYER_LINE_OF_FIRE), false);
            let has_target_vector = m.i16(at(a::TARGET_TARGET_TYPE)) > 8;
            let mut attack_vector = [0.0; 3];
            if has_target_vector {
                let target_prop = prop_get(m, context, m.i32(at(a::TARGET_TARGET_PROP_INDEX)));
                attack_vector = m.vector(target_prop + p::ACTOR_TO_PROP as u32);
            }
            let mut iterator = PropIterator::new(m, context);
            while let Some(prop) = iterator.next(m, context) {
                let state = m.i16(prop + p::STATE as u32);
                let player = m.bool(prop + p::PLAYER as u32);
                if state >= v::_PROP_STATE_BECOMING_UNACKNOWLEDGED as i16
                    && state <= v::_PROP_STATE_ACKNOWLEDGED as i16
                    && !m.bool(prop + p::ENEMY as u32)
                    && !m.bool(prop + p::DEAD as u32)
                    && !m.bool(prop + p::SWARM as u32)
                    && (player || m.i32(prop + p::VEHICLE_INDEX as u32) == NONE)
                {
                    let attacking = call(call::FRIEND_ATTACKING, iterator.index, 0);
                    if attacking.result != 0 {
                        let friend_attack_vector = attacking.vector();
                        let prop_position = m.vector(prop + p::BODY_POSITION as u32);
                        let body_position = m.vector(at(a::INPUT_BODY_POSITION));
                        // (the port's: zeros where the game's was the stack's
                        // leftover, when the friend isn't ahead of the aim)
                        let mut vector_to_line_of_fire = [0.0; 3];
                        let blockage = aiming_vector_test_blockage(
                            prop_position,
                            friend_attack_vector,
                            body_position,
                            Some(&mut vector_to_line_of_fire),
                        );
                        if blockage >= 1 {
                            m.set_bool(at(a::EMOTIONS_CROUCH_BLOCKING_LINE_OF_FIRE), true);
                            if player {
                                m.set_bool(at(a::EMOTIONS_CROUCH_BLOCKING_PLAYER_LINE_OF_FIRE), true);
                            }
                        }
                        if avoid_friend_line_of_fire
                            && player
                            && m.bool(prop + p::SHOOTING as u32)
                            && magnitude_squared3(vector_to_line_of_fire) < 1.0
                            && (m.bool(at(a::CONTROL_MOVING)) || m.i16(at(a::EMOTIONS_MOVING_INTO_FIRE_TIMER)) > 0)
                        {
                            let mut movement_direction = m.vector(at(a::CONTROL_MOVING_TOWARDS_VECTOR));
                            if normalize3(&mut movement_direction) > 0.0 {
                                let future_point = point_from_line3(body_position, movement_direction, 0.4);
                                let mut secondary_blockage =
                                    aiming_vector_test_blockage(prop_position, friend_attack_vector, future_point, None);
                                if secondary_blockage <= blockage {
                                    secondary_blockage = blockage;
                                }
                                if secondary_blockage >= 1 {
                                    let dot = dot3(movement_direction, vector_to_line_of_fire);
                                    let threshold =
                                        if magnitude_squared3(vector_to_line_of_fire) < 0.25 { 0.0 } else { 0.866_025_39 };
                                    if dot > threshold {
                                        m.set_bool(at(a::EMOTIONS_MOVING_INTO_PLAYER_LINE_OF_FIRE), true);
                                    }
                                }
                            }
                        }
                    }
                    if has_target_vector
                        && aiming_vector_test_blockage(
                            m.vector(at(a::INPUT_BODY_POSITION)),
                            attack_vector,
                            m.vector(prop + p::BODY_POSITION as u32),
                            None,
                        ) >= 2
                    {
                        m.set_bool(at(a::EMOTIONS_CROUCH_FRIENDS_IN_LINE_OF_FIRE), true);
                    }
                }
            }
        } else {
            m.set_bool(at(a::EMOTIONS_CROUCH_BLOCKING_LINE_OF_FIRE), false);
            m.set_bool(at(a::EMOTIONS_CROUCH_BLOCKING_PLAYER_LINE_OF_FIRE), false);
            m.set_bool(at(a::EMOTIONS_CROUCH_FRIENDS_IN_LINE_OF_FIRE), false);
            m.set_bool(at(a::EMOTIONS_MOVING_INTO_PLAYER_LINE_OF_FIRE), false);
        }
    }

    if m.bool(at(a::EMOTIONS_MOVING_INTO_PLAYER_LINE_OF_FIRE)) {
        crate::firing_positions::discard_firing_position(m, context, m.i16(at(a::FIRING_POSITIONS_CURRENT_POSITION_INDEX)), true);
        m.set_i16(at(a::EMOTIONS_MOVING_INTO_FIRE_TIMER), 22);
    } else if m.i16(at(a::EMOTIONS_MOVING_INTO_FIRE_TIMER)) > 0 {
        m.set_i16(at(a::EMOTIONS_MOVING_INTO_FIRE_TIMER), m.i16(at(a::EMOTIONS_MOVING_INTO_FIRE_TIMER)) - 1);
    }

    let crouch_timer = m.i16(at(a::EMOTIONS_DEFENSIVE_CROUCH_TIMER));
    if crouch_timer > 0 {
        m.set_i16(at(a::EMOTIONS_DEFENSIVE_CROUCH_TIMER), crouch_timer - 1);
    } else {
        let threshold = if !m.bool(at(a::EMOTIONS_CURRENTLY_DEFENDING)) || m.bool(at(a::EMOTIONS_BERSERK)) {
            m.f32(defensive + d::DEFENSIVE_THRESHOLD_ATTACKING as u32)
        } else {
            m.f32(defensive + d::DEFENSIVE_THRESHOLD_DEFENDING as u32)
        };
        let mut crouch = match m.i16(defensive + d::DEFENSIVE_CROUCH_TYPE as u32) {
            DEFENSIVE_CROUCH_DANGER => m.f32(at(a::EMOTIONS_PERCEIVED_DANGER)) > threshold,
            DEFENSIVE_CROUCH_SHIELD_LOW => m.f32(at(a::INPUT_SHIELD_VITALITY)) < threshold,
            DEFENSIVE_CROUCH_HIDE_BEHIND_SHIELD => {
                m.f32(at(a::INPUT_SHIELD_VITALITY)) > threshold
                    && m.i8(at(a::SITUATION_CUMULATIVE_THREATS) + v::_ACTOR_THREAT_VISIBLE_FACING_ME as u32) > 0
            }
            DEFENSIVE_CROUCH_ANY_TARGET => m.i16(at(a::STATE_COMBAT_STATUS)) > 0,
            DEFENSIVE_CROUCH_FLOOD_SHAMBLE => flood_desire_shamble(context, host),
            _ => false,
        };
        if crouch_in_line_of_fire {
            if m.bool(at(a::EMOTIONS_CROUCH_BLOCKING_PLAYER_LINE_OF_FIRE)) {
                crouch = true;
            } else if m.bool(at(a::EMOTIONS_CROUCH_FRIENDS_IN_LINE_OF_FIRE)) {
                crouch = false;
            } else if m.bool(at(a::EMOTIONS_CROUCH_BLOCKING_LINE_OF_FIRE)) {
                crouch = true;
            }
        }
        let defensive_crouch = m.bool(at(a::EMOTIONS_DEFENSIVE_CROUCH));
        if defensive_crouch && !crouch {
            m.set_bool(at(a::EMOTIONS_DEFENSIVE_CROUCH), false);
            let time = m.f32(defensive + d::DEFENSIVE_CROUCH_MIN_STAND_TIME as u32);
            m.set_i16(at(a::EMOTIONS_DEFENSIVE_CROUCH_TIMER), if time > 0.0 { c_short(time * 30.0) } else { 45 });
        } else if !defensive_crouch && crouch {
            m.set_bool(at(a::EMOTIONS_DEFENSIVE_CROUCH), true);
            let time = m.f32(defensive + d::DEFENSIVE_CROUCH_MIN_CROUCH_TIME as u32);
            m.set_i16(at(a::EMOTIONS_DEFENSIVE_CROUCH_TIMER), if time > 0.0 { c_short(time * 30.0) } else { 45 });
        }
    }

    let evasion = m.i16(at(a::EMOTIONS_EVASION_DELAY_TIMER));
    if evasion > 0 {
        m.set_i16(at(a::EMOTIONS_EVASION_DELAY_TIMER), evasion - 1);
    }

    call(call::UNOPPOSABLE_RETREAT, 0, 0);
}

/// actor_type_flood.c's movement switching types (`_actor_movement_switching_*`).
const MOVEMENT_SWITCHING_ALWAYS_RUN: i16 = 0;
const MOVEMENT_SWITCHING_ALWAYS_CROUCH: i16 = 1;

/// actor_type_flood.c's `actor_type_flood_desire_shamble`: whether a flood
/// form shambles (its crouch) rather than runs: when idle or out of combat,
/// hard pinged or unable to melee; not when its target's out of its firing
/// range, when berserk or charging to melee, or without a gun; else by its
/// variant (always running, always crouching, crouching near its target),
/// or switching between the two on timers (keeping in step with its friends
/// around it when its variant says so).
pub fn flood_desire_shamble(context: &AiStepContext, host: &impl AiHost) -> bool {
    let m = host.memory();
    let at = |offset: usize| context.actor + offset as u32;
    let variant = |offset: usize| context.variant_definition + offset as u32;
    let call = |kind: i32, input: &[f32]| {
        let mut call = AiCall { kind, actor: context.actor_index, ..Default::default() };
        call.input[..input.len()].copy_from_slice(input);
        host.call(&mut call);
        call
    };
    use crate::layout::actor_variant_definition as var;

    let firing_variant = call(call::FIRING_VARIANT, &[]).result as u32;
    let firing = |offset: usize| m.f32(firing_variant + offset as u32);
    let mut result = false;
    let mut crouch_switching = false;

    if call(call::UNIT_BUSY, &[]).result == 0 && call(call::HAS_PATH, &[]).result == 0 {
        result = true;
    } else if m.i16(at(a::STATE_MODE)) < MODE_COMBAT {
        result = true;
    } else if m.i16(at(a::STATE_COMBAT_STATUS)) < 5 {
        result = false;
    } else {
        let unit_state = call(call::UNIT_SHAMBLE_STATE, &[]);
        let target_prop_index = m.i32(at(a::TARGET_TARGET_PROP_INDEX));
        let target_prop = if target_prop_index == NONE { None } else { Some(prop_get(m, context, target_prop_index)) };
        let target_distance = |prop: u32| m.f32(prop + p::DISTANCE as u32);
        let berserk = m.bool(at(a::EMOTIONS_BERSERK));
        let goal = m.i16(at(a::STATE_CHARGE_GOAL));
        if unit_state.result != 0 && !berserk {
            result = true;
        } else if target_prop.is_some_and(|prop| target_distance(prop) > firing(var::RANGED_COMBAT_MAXIMUM_FIRING_RANGE)) {
            result = false;
        } else if berserk
            && target_prop.is_some_and(|prop| target_distance(prop) > firing(var::RANGED_COMBAT_BERSERK_FIRING_RANGE_UPPER_BOUND))
        {
            result = false;
        } else if unit_state.result2 != 0 {
            result = true;
        } else if berserk
            || (m.i16(at(a::STATE_ACTION)) == 10
                && (goal == v::_CHARGE_GOAL_MELEE as i16 || goal == v::_CHARGE_GOAL_MELEE_LEAPING as i16))
        {
            result = false;
        } else if call(call::HAS_RANGED_WEAPON, &[]).result == 0 || m.bool(at(a::INPUT_UNDERWATER)) {
            result = false;
        } else if m.i16(variant(var::MOVEMENT_SWITCHING_MOVEMENT_TYPE)) == MOVEMENT_SWITCHING_ALWAYS_RUN {
            result = false;
        } else if m.i16(variant(var::MOVEMENT_SWITCHING_MOVEMENT_TYPE)) == MOVEMENT_SWITCHING_ALWAYS_CROUCH {
            result = true;
        } else if target_prop.is_some_and(|prop| target_distance(prop) < firing(var::RANGED_COMBAT_COMBAT_RANGE_UPPER_BOUND)) {
            result = true;
        } else {
            crouch_switching = true;
        }
    }

    let active = at(a::EMOTIONS_CROUCH_SWITCHING_ACTIVE);
    let current = at(a::EMOTIONS_CROUCH_SWITCHING_CURRENT_VALUE);
    let change_timer = at(a::EMOTIONS_CROUCH_SWITCHING_CHANGE_TIMER);
    let stay_timer = at(a::EMOTIONS_CROUCH_SWITCHING_STAY_TIMER);
    if !crouch_switching {
        m.set_bool(active, false);
        return result;
    }

    let friend_actor = |prop: u32| {
        let index = m.i32(prop + p::ACTOR_INDEX as u32);
        crate::ai_actor::datum_get(m, context.actor_data, index)
    };
    let friendly_seen = |prop: u32| {
        let state = m.i16(prop + p::STATE as u32);
        state >= v::_PROP_STATE_BECOMING_UNACKNOWLEDGED as i16
            && state <= v::_PROP_STATE_ACKNOWLEDGED as i16
            && !m.bool(prop + p::ENEMY as u32)
            && !m.bool(prop + p::DEAD as u32)
    };
    let mut value_changed = false;
    if m.bool(active) {
        let mut change_value = false;
        if m.i16(stay_timer) > 0 {
            m.set_i16(stay_timer, m.i16(stay_timer) - 1);
        } else if m.r32(variant(var::FLAGS)) & (1 << v::_ACTOR_VARIANT_DEFINITION_MOVEMENT_SWITCHING_TRY_TO_STAY_WITH_FRIENDS_BIT) != 0
            && m.i8(at(a::SITUATION_CLOSE_FRIENDS)) > 0
        {
            let target_prop = prop_get(m, context, m.i32(at(a::TARGET_TARGET_PROP_INDEX)));
            let actor_to_target = m.vector(target_prop + p::ACTOR_TO_PROP as u32);
            let body = m.vector(at(a::INPUT_BODY_POSITION));
            let (mut backward_count, mut middle_count, mut forward_count) = (0i16, 0i16, 0i16);
            let mut iterator = PropIterator::new(m, context);
            while let Some(prop) = iterator.next(m, context) {
                if friendly_seen(prop) && m.f32(prop + p::DISTANCE as u32) < 15.0 && m.i32(prop + p::ACTOR_INDEX as u32) != NONE {
                    let Some(friend) = friend_actor(prop) else { continue };
                    if m.bool(friend + a::EMOTIONS_CROUCH_SWITCHING_ACTIVE as u32) {
                        let friend_vector = sub3(m.vector(friend + a::INPUT_BODY_POSITION as u32), body);
                        let projection = dot3(friend_vector, actor_to_target);
                        if projection > 1.4 {
                            forward_count += 1;
                        } else if projection < -1.4 {
                            backward_count += 1;
                        } else {
                            middle_count += 1;
                        }
                    }
                }
            }
            if m.bool(current) {
                if backward_count == 0 && forward_count > middle_count {
                    change_value = true;
                }
            } else if forward_count == 0 && backward_count > middle_count {
                change_value = true;
            }
        }
        if !change_value {
            let timer = m.i16(change_timer) - 1;
            m.set_i16(change_timer, timer);
            if timer == 0 {
                change_value = true;
            }
        }
        if change_value {
            m.set_bool(current, !m.bool(current));
            value_changed = true;
        }
    } else {
        let mut crouch_chance = m.f32(variant(var::MOVEMENT_SWITCHING_INITIAL_CROUCH_CHANCE));
        if m.i8(at(a::SITUATION_AREA_FRIENDS)) > 0 {
            let (mut crouching_count, mut standing_count) = (0i16, 0i16);
            let actor_type = m.i16(at(a::META_TYPE));
            let mut iterator = PropIterator::new(m, context);
            while let Some(prop) = iterator.next(m, context) {
                if friendly_seen(prop) && m.i32(prop + p::ACTOR_INDEX as u32) != NONE {
                    let Some(friend) = friend_actor(prop) else { continue };
                    if m.i16(friend + a::META_TYPE as u32) == actor_type && m.i16(friend + a::STATE_COMBAT_STATUS as u32) >= 5 {
                        if m.bool(friend + a::EMOTIONS_DEFENSIVE_CROUCH as u32) {
                            crouching_count += 1;
                        } else {
                            standing_count += 1;
                        }
                    }
                }
            }
            let mut adjustment = crouching_count as f32 * (1.0 - crouch_chance);
            adjustment += standing_count as f32 * -crouch_chance;
            crouch_chance -= 0.5 * adjustment;
        }
        let random_value = call(call::RANDOM, &[]).output[0];
        m.set_bool(active, true);
        m.set_bool(current, random_value < crouch_chance);
        value_changed = true;
    }

    if value_changed {
        let (lower, upper) = if m.bool(current) {
            (m.f32(variant(var::MOVEMENT_SWITCHING_CROUCH_TIME_LOWER_BOUND)), m.f32(variant(var::MOVEMENT_SWITCHING_CROUCH_TIME_UPPER_BOUND)))
        } else {
            (m.f32(variant(var::MOVEMENT_SWITCHING_RUN_TIME_LOWER_BOUND)), m.f32(variant(var::MOVEMENT_SWITCHING_RUN_TIME_UPPER_BOUND)))
        };
        let change_time = call(call::RANDOM_RANGE, &[lower, upper]).output[0];
        let ticks = change_time * 30.0;
        m.set_i16(change_timer, c_short(if ticks > 31.0 { ticks } else { 31.0 }));
        m.set_i16(stay_timer, 30);
    }
    m.bool(current)
}
