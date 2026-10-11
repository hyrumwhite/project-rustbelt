//! An actor's perception (`actor_perception.c`'s `actor_perception_update`,
//! every tick of an actor's update): the danger zone it notices (and whether
//! it may dive from it), then each of its props: their timers, which are in
//! use and refreshed this tick, their acknowledgement (the awareness that
//! builds to it, the orphans left when a prop is lost, the orphans' search
//! and expiry), and the stimuli and shouts a newly seen, killed or changed
//! prop gives; and last the actor's nearest orphan, its target's history
//! and its props' refresh pace.
//!
//! The actor's datum, props and definition are the game's memory, worked
//! on in place ([`crate::ai_actor`]); the refreshes, the random draws, the
//! knowledge, the props' acknowledgement, orphaning, switching and deletion,
//! the stimuli, the shouts, the encounter and the profile are the game's,
//! through [`AiHost::call`] (actor_perception.c's `actor_perception_call`,
//! shared with the C).

use crate::ai_actor::{AiCall, AiHost, AiMemory, AiMemoryExt, AiStepContext, NONE, PropIterator, prop_get};
use crate::layout::{actor_datum as a, actor_definition as d, ai_values as v, prop_datum as p};

/// The perception update's calls (halo_sim.h's `_actor_perception_call_*`).
pub mod call {
    /// `actor_perception_refresh(actor)`.
    pub const REFRESH: i32 = 1;
    /// `actor_perception_refresh_danger_zone(actor)`.
    pub const REFRESH_DANGER_ZONE: i32 = 2;
    /// `real_seed_random(get_global_random_seed_address())`, in `output[0]`.
    pub const RANDOM: i32 = 3;
    /// `actor_stimulus_noticed_danger_zone` with the actor's danger zone.
    pub const NOTICED_DANGER: i32 = 4;
    /// `prop_position_refresh(actor, a, ...)`, and with `b`
    /// `prop_status_refresh` from the position.
    pub const REFRESH_PROP: i32 = 5;
    /// The player prop `a` starts to become acknowledged (a debug message).
    pub const DEBUG_START_AWARE: i32 = 6;
    /// The player prop `a` stops becoming acknowledged (the debug awareness
    /// speed, and a message).
    pub const DEBUG_STOP_AWARE: i32 = 7;
    /// `actor_get_perception_knowledge(actor, a)`, the result.
    pub const KNOWLEDGE: i32 = 8;
    /// The player prop `a`'s awareness speed (`b`: the knowledge, and the
    /// speed in its second byte; the delta in `input[0]`): the debug speed
    /// and messages.
    pub const DEBUG_AWARENESS: i32 = 9;
    /// The player prop `a` became acknowledged (the debug speed, a message).
    pub const DEBUG_BECOME_AWARE: i32 = 10;
    /// `actor_perception_desire_prop` as an uninspected orphan of the prop
    /// `a`, the result.
    pub const DESIRE_ORPHAN: i32 = 11;
    /// The prop `a`'s orphan made (its position refreshed, its pathfinding
    /// location found, `prop_orphan_transition`): the orphan, the result.
    pub const ORPHAN_TRANSITION: i32 = 12;
    /// `actor_switch_props(actor, a, b)`.
    pub const SWITCH_PROPS: i32 = 13;
    /// `actor_perception_become_acknowledged(actor, a, &expected)`: the
    /// result in bit 0, the expectation in bit 1.
    pub const BECOME_ACKNOWLEDGED: i32 = 14;
    /// `actor_compute_prop_unopposable(actor, a)`, the result.
    pub const UNOPPOSABLE: i32 = 15;
    /// `actor_compute_prop_target_weight(actor, a)`, in `output[0]`.
    pub const TARGET_WEIGHT: i32 = 16;
    /// `prop_delete(actor, a)`.
    pub const PROP_DELETE: i32 = 17;
    /// `actor_stimulus_prop_just_killed(actor, a)`.
    pub const STIMULUS_JUST_KILLED: i32 = 18;
    /// `actor_stimulus_prop_sighted(actor, a, b)`.
    pub const STIMULUS_SIGHTED: i32 = 19;
    /// The shout on first sighting the friendly player of the prop `a`.
    pub const SIGHTED_FRIEND_PLAYER: i32 = 20;
    /// `game_team_is_enemy(a, b)`, the result.
    pub const TEAM_IS_ENEMY: i32 = 21;
    /// The shout that the prop `a`'s allegiance changed (`b`: to an enemy).
    pub const ALLEGIANCE_CHANGED: i32 = 22;
    /// `actor_stimulus_enter_combat_found_body(actor, a)`.
    pub const STIMULUS_FOUND_BODY: i32 = 23;
    /// `actor_stimulus_enter_combat_perceived_enemy(actor, a)`.
    pub const STIMULUS_PERCEIVED_ENEMY: i32 = 24;
    /// Whether the encounter `a` hasn't seen an enemy in six seconds (or
    /// ever), the result.
    pub const ENCOUNTER_ENEMY_UNSEEN: i32 = 25;
    /// The shout alerting to combat: from the prop `a` to the actor, or
    /// with `b` from the actor to it.
    pub const ALERT_NONCOMBAT: i32 = 26;
    /// `actor_in_combat(actor)`, the result.
    pub const IN_COMBAT: i32 = 27;
    /// `actor_is_fighting(actor)`, the result.
    pub const IS_FIGHTING: i32 = 28;
    /// The AI profile's count of the props of the kind `a` (dead, enemy,
    /// friendly by 0, 3, 6; acknowledged, orphaned, unacknowledged by 0, 1,
    /// 2).
    pub const PROFILE: i32 = 29;
}

/// The local enums (actor_perception.c's).
const MODE_COMBAT: i16 = 3;
const COMBAT_STATUS_CERTAIN: i16 = 4;
const FIRE_TARGET_PROP: i16 = 1;
const PERCEPTION_FULL: i16 = v::_ACTOR_PERCEPTION_FULL as i16;
const AWARENESS_SPEED_NEVER: i16 = 0;
const AWARENESS_SPEED_NONCOMBAT: i16 = 1;
const AWARENESS_SPEED_GUARD: i16 = 2;
const AWARENESS_SPEED_COMBAT: i16 = 3;
const AWARENESS_SPEED_INSTANT: i16 = 4;

/// `global_acknowledgement_speeds[knowledge][perception]`.
const ACKNOWLEDGEMENT_SPEEDS: [[i16; 4]; 4] = [[0, 0, 1, 3], [0, 1, 2, 3], [0, 2, 3, 4], [0, 3, 4, 4]];

const UNACKNOWLEDGED: i16 = v::_PROP_STATE_UNACKNOWLEDGED as i16;
const BECOMING_ACKNOWLEDGED: i16 = v::_PROP_STATE_BECOMING_ACKNOWLEDGED as i16;
const BECOMING_UNACKNOWLEDGED: i16 = v::_PROP_STATE_BECOMING_UNACKNOWLEDGED as i16;
const ACKNOWLEDGED: i16 = v::_PROP_STATE_ACKNOWLEDGED as i16;
const UNINSPECTED_ORPHAN: i16 = v::_PROP_STATE_UNINSPECTED_ORPHAN as i16;
const INSPECTED_ORPHAN: i16 = v::_PROP_STATE_INSPECTED_ORPHAN as i16;

fn call_full(context: &AiStepContext, host: &impl AiHost, kind: i32, a: i32, b: i32, input: f32) -> AiCall {
    let mut call = AiCall { kind, actor: context.actor_index, a, b, ..AiCall::default() };
    call.input[0] = input;
    host.call(&mut call);
    call
}

fn call_simple(context: &AiStepContext, host: &impl AiHost, kind: i32, a: i32) -> i32 {
    call_full(context, host, kind, a, 0, 0.0).result
}

fn random(context: &AiStepContext, host: &impl AiHost) -> f32 {
    call_full(context, host, call::RANDOM, 0, 0, 0.0).output[0]
}

fn acknowledged(state: i16) -> bool {
    (BECOMING_UNACKNOWLEDGED..=ACKNOWLEDGED).contains(&state)
}

fn orphaned(state: i16) -> bool {
    (UNINSPECTED_ORPHAN..=INSPECTED_ORPHAN).contains(&state)
}

/// A short's `--x` and whether it reached zero.
fn decrement(m: &dyn AiMemory, address: u32) -> i16 {
    let value = m.i16(address).wrapping_sub(1);
    m.set_i16(address, value);
    value
}

fn increment(m: &dyn AiMemory, address: u32) -> i16 {
    let value = m.i16(address).wrapping_add(1);
    m.set_i16(address, value);
    value
}

/// `actor_perception_update`.
pub fn actor_perception_update(context: &AiStepContext, host: &impl AiHost) {
    let m = host.memory();
    let actor = context.actor;
    let mut highest_prop_timer: i16 = 1;
    let mut prop_serviced = false;
    let mut interesting_orphan_index = NONE;
    let mut nearest_orphan_distance = f32::MAX;

    if !m.bool(actor + a::META_DORMANT as u32) {
        danger_zone_update(context, host);
    }

    let mut iterator = PropIterator::new(m, context);
    while let Some(prop) = iterator.next(m, context) {
        let index = iterator.index;
        let mut new_state = NONE as i16;
        let mut orphan_expired = false;
        let mut refresh_position = false;
        let mut refresh_status = false;
        let mut became_acknowledged = false;
        let mut expected_acknowledgement = false;
        let at = |offset: usize| prop + offset as u32;

        if m.i16(at(p::UNIT_EFFECT_DECAY_TICKS)) > 0 && decrement(m, at(p::UNIT_EFFECT_DECAY_TICKS)) == 0 {
            m.set_i16(at(p::UNIT_EFFECT), NONE as i16);
        }
        if m.i16(at(p::TICKS_SINCE_DAMAGE)) != NONE as i16 && increment(m, at(p::TICKS_SINCE_DAMAGE)) >= 45 {
            m.set_bool(at(p::CURRENTLY_DAMAGING_ME), false);
        }
        if m.i16(at(p::TICKS_SINCE_DEFINITELY_LOCATED)) != NONE as i16
            && increment(m, at(p::TICKS_SINCE_DEFINITELY_LOCATED)) >= 60
        {
            m.set_bool(at(p::DEFINITELY_LOCATED), false);
            m.w32(at(p::DEFINITE_KNOWLEDGE_SOURCE_ACTOR), NONE as u32);
        }
        if m.bool(at(p::DEAD)) {
            increment(m, at(p::DEAD_TICKS));
        } else {
            m.set_i16(at(p::DEAD_TICKS), 0);
        }
        if m.i16(at(p::TICKS_UNTIL_ORPHAN)) > 0 {
            decrement(m, at(p::TICKS_UNTIL_ORPHAN));
        }
        if m.i16(at(p::REQUIRED_TICKS)) > 0 && !m.bool(at(p::DELAY_REQUIREMENT_DECISION)) {
            decrement(m, at(p::REQUIRED_TICKS));
        }
        let unreachable_ticks = m.i16(at(p::UNREACHABLE_TICKS));
        if unreachable_ticks > 0 && unreachable_ticks < i16::MAX {
            increment(m, at(p::UNREACHABLE_TICKS));
        }
        if m.i16(at(p::UNOPPOSABLE_CASUALTY_DECAY_TIMER)) > 0
            && decrement(m, at(p::UNOPPOSABLE_CASUALTY_DECAY_TIMER)) == 0
            && decrement(m, at(p::UNOPPOSABLE_CASUALTIES_INFLICTED)) > 0
        {
            m.set_i16(at(p::UNOPPOSABLE_CASUALTY_DECAY_TIMER), 750);
        }
        if m.i16(at(p::VISIBILITY)) >= PERCEPTION_FULL {
            if m.i16(at(p::VISIBLE_TICKS)) < i16::MAX {
                increment(m, at(p::VISIBLE_TICKS));
            }
        } else {
            m.set_i16(at(p::VISIBLE_TICKS), 0);
        }

        if m.bool(actor + a::META_DORMANT as u32) {
            m.set_bool(at(p::IN_USE), false);
            m.set_i16(at(p::TIMER), 0);
        } else {
            let mut prop_timer = increment(m, at(p::TIMER));
            if !m.bool(at(p::ENEMY)) {
                prop_timer >>= 3;
            }
            if m.i8(at(p::QUANTIZED_DISTANCE)) >= 3 {
                prop_timer >>= 1;
            }
            if !prop_serviced && prop_timer >= m.i16(actor + a::META_HIGHEST_PROP_TIMER as u32) {
                refresh_status = true;
                refresh_position = true;
                prop_timer = 0;
                m.set_i16(at(p::TIMER), 0);
                prop_serviced = true;
            }
            if prop_timer > highest_prop_timer {
                highest_prop_timer = prop_timer;
            }

            let state = m.i16(at(p::STATE));
            if state < UNACKNOWLEDGED
                || state > BECOMING_ACKNOWLEDGED
                || m.i32(at(p::ORPHAN_PROP_INDEX)) == NONE
            {
                if m.bool(actor + a::META_SWARM as u32) {
                    m.set_bool(at(p::IN_USE), false);
                } else {
                    let direction_is = |active: bool, type_offset: usize, prop_offset: usize| {
                        active
                            && m.i16(actor + type_offset as u32) == v::_DIRECTION_SPECIFICATION_PROP as i16
                            && m.i32(actor + prop_offset as u32) == index
                    };
                    let in_use = m.i32(actor + a::TARGET_TARGET_PROP_INDEX as u32) == index
                        || m.i32(actor + a::META_INTERESTING_ORPHAN_INDEX as u32) == index
                        || m.i32(actor + a::EMOTIONS_UNOPPOSABLE_RETREAT_PROP_INDEX as u32) == index
                        || m.i32(actor + a::EXTERNAL_ORDERS_PURSUIT_GROUP_PROP_INDEX as u32) == index
                        || direction_is(
                            m.i16(actor + a::CONTROL_SECONDARY_LOOK_TYPE as u32) != v::_SECONDARY_LOOK_NONE as i16,
                            a::CONTROL_SECONDARY_LOOK_DIRECTION_TYPE,
                            a::CONTROL_SECONDARY_LOOK_DIRECTION_PROP_INDEX,
                        )
                        || direction_is(
                            m.bool(actor + a::CONTROL_IDLE_MAJOR_ACTIVE as u32),
                            a::CONTROL_IDLE_MAJOR_DIRECTION_TYPE,
                            a::CONTROL_IDLE_MAJOR_DIRECTION_PROP_INDEX,
                        )
                        || direction_is(
                            m.bool(actor + a::CONTROL_IDLE_MINOR_ACTIVE as u32),
                            a::CONTROL_IDLE_MINOR_DIRECTION_TYPE,
                            a::CONTROL_IDLE_MINOR_DIRECTION_PROP_INDEX,
                        );
                    m.set_bool(at(p::IN_USE), in_use);
                    if orphaned(state) {
                        let parent = prop_get(m, context, m.i32(at(p::ORPHAN_PROP_INDEX)));
                        m.set_bool(parent + p::IN_USE as u32, in_use);
                    }
                }
            }

            if m.bool(at(p::IN_USE)) && !(UNACKNOWLEDGED..=BECOMING_ACKNOWLEDGED).contains(&state) {
                refresh_position = true;
            }
            if refresh_position {
                call_full(context, host, call::REFRESH_PROP, index, refresh_status as i32, 0.0);
            }
        }

        let state = m.i16(at(p::STATE));
        let perception = m.i16(at(p::PERCEPTION));
        let none = v::_ACTOR_PERCEPTION_NONE as i16;
        match state {
            UNACKNOWLEDGED | BECOMING_ACKNOWLEDGED => {
                let mut becoming = state == BECOMING_ACKNOWLEDGED;
                if !becoming && perception > none {
                    new_state = BECOMING_ACKNOWLEDGED;
                    m.set_f32(at(p::AWARENESS), 0.0);
                    if m.bool(at(p::PLAYER)) {
                        call_simple(context, host, call::DEBUG_START_AWARE, index);
                    }
                    becoming = true;
                }
                if becoming {
                    if perception == none {
                        m.set_f32(at(p::AWARENESS), 0.0);
                        new_state = UNACKNOWLEDGED;
                        if m.bool(at(p::PLAYER)) {
                            call_simple(context, host, call::DEBUG_STOP_AWARE, index);
                        }
                    } else {
                        let knowledge = call_simple(context, host, call::KNOWLEDGE, index) as i16;
                        let speed = ACKNOWLEDGEMENT_SPEEDS
                            .get(knowledge as usize)
                            .and_then(|row| row.get(perception as usize))
                            .copied()
                            .unwrap_or(-1);
                        let definition = context.definition;
                        let delta = match speed {
                            AWARENESS_SPEED_NEVER => 0.0,
                            AWARENESS_SPEED_NONCOMBAT => {
                                m.f32(definition + d::PERCEPTION_RUNTIME_AWARENESS_DELTA_NON_COMBAT as u32)
                            }
                            AWARENESS_SPEED_GUARD => m.f32(definition + d::PERCEPTION_RUNTIME_AWARENESS_DELTA_GUARD as u32),
                            AWARENESS_SPEED_COMBAT => {
                                m.f32(definition + d::PERCEPTION_RUNTIME_AWARENESS_DELTA_COMBAT as u32)
                            }
                            AWARENESS_SPEED_INSTANT => 1.0,
                            _ => 0.0,
                        };
                        if m.bool(at(p::PLAYER)) {
                            call_full(
                                context,
                                host,
                                call::DEBUG_AWARENESS,
                                index,
                                (knowledge as i32 & 0xFF) | ((speed as i32 & 0xFF) << 8),
                                delta,
                            );
                        }
                        let awareness = m.f32(at(p::AWARENESS)) + delta;
                        m.set_f32(at(p::AWARENESS), awareness);
                        if awareness >= 1.0 {
                            new_state = ACKNOWLEDGED;
                            if m.bool(at(p::PLAYER)) {
                                call_simple(context, host, call::DEBUG_BECOME_AWARE, index);
                            }
                        }
                    }
                }
            }
            BECOMING_UNACKNOWLEDGED => {
                if perception > none {
                    new_state = ACKNOWLEDGED;
                } else {
                    let last = [m.f32(at(p::LAST_PERCEIVED_BODY_POSITION)), m.f32(at(p::LAST_PERCEIVED_BODY_POSITION) + 4)];
                    let body = [m.f32(at(p::BODY_POSITION)), m.f32(at(p::BODY_POSITION) + 4)];
                    let offset = [body[0] - last[0], body[1] - last[1]];
                    if m.i16(at(p::TICKS_UNTIL_ORPHAN)) == 0 || offset[0] * offset[0] + offset[1] * offset[1] > 1.0 {
                        let mut orphan_prop_index = NONE;
                        if call_simple(context, host, call::DESIRE_ORPHAN, index) != 0 {
                            orphan_prop_index = call_simple(context, host, call::ORPHAN_TRANSITION, index);
                        }
                        call_full(context, host, call::SWITCH_PROPS, index, orphan_prop_index, 0.0);
                        new_state = UNACKNOWLEDGED;
                    }
                }
            }
            ACKNOWLEDGED => {
                if perception == none {
                    if call_simple(context, host, call::DESIRE_ORPHAN, index) != 0 {
                        new_state = BECOMING_UNACKNOWLEDGED;
                    } else {
                        call_full(context, host, call::SWITCH_PROPS, index, NONE, 0.0);
                        new_state = UNACKNOWLEDGED;
                    }
                }
            }
            UNINSPECTED_ORPHAN | INSPECTED_ORPHAN => {
                if state == UNINSPECTED_ORPHAN {
                    let inspection_ticks: i16 =
                        if m.bool(actor + a::INPUT_VEHICLE_GUNNER_BOMBARDMENT as u32) { 300 } else { 45 };
                    if m.i16(at(p::VISIBILITY)) >= PERCEPTION_FULL
                        || (m.i16(actor + a::CONTROL_CURRENT_FIRE_TARGET_TYPE as u32) == FIRE_TARGET_PROP
                            && m.i32(actor + a::CONTROL_CURRENT_FIRE_TARGET_PROP_INDEX as u32) == index
                            && context.game_time % 3 == 0)
                    {
                        if increment(m, at(p::ORPHAN_INSPECTION_TICKS)) >= inspection_ticks {
                            new_state = INSPECTED_ORPHAN;
                        }
                    }
                }

                let lifespan_decay: i16 = if index == m.i32(actor + a::EMOTIONS_UNOPPOSABLE_RETREAT_PROP_INDEX as u32)
                    || (m.i16(actor + a::STATE_ACTION as u32) == v::_ACTOR_ACTION_FLEE as i16
                        && m.i32(actor + a::STATE_FLEE_PROP_INDEX as u32) == index)
                {
                    0
                } else if index == m.i32(actor + a::TARGET_TARGET_PROP_INDEX as u32) {
                    m.bool(at(p::ABANDONED_SEARCH)) as i16
                } else if index == m.i32(actor + a::META_INTERESTING_ORPHAN_INDEX as u32) {
                    if m.i16(actor + a::STATE_COMBAT_STATUS as u32) >= COMBAT_STATUS_CERTAIN { 6 } else { 1 }
                } else {
                    10
                };
                let lifespan = m.i16(at(p::ORPHAN_LIFESPAN_TICKS)).wrapping_sub(lifespan_decay);
                m.set_i16(at(p::ORPHAN_LIFESPAN_TICKS), lifespan);
                if lifespan < 0 {
                    orphan_expired = true;
                }
            }
            _ => {}
        }

        if new_state != NONE as i16 {
            match new_state {
                BECOMING_UNACKNOWLEDGED => {
                    let ticks = if m.i16(at(p::VISIBILITY)) >= PERCEPTION_FULL { 60 } else { 10 };
                    m.set_i16(at(p::TICKS_UNTIL_ORPHAN), ticks);
                }
                ACKNOWLEDGED => {
                    let result = call_simple(context, host, call::BECOME_ACKNOWLEDGED, index);
                    became_acknowledged = result & 1 != 0;
                    expected_acknowledgement = result & 2 != 0;
                    iterator.next_index = m.i32(at(p::NEXT_PROP_INDEX));
                }
                UNINSPECTED_ORPHAN | UNACKNOWLEDGED | INSPECTED_ORPHAN => {
                    m.set_bool(at(p::DEFINITELY_LOCATED), false);
                    m.w32(at(p::DEFINITE_KNOWLEDGE_SOURCE_ACTOR), NONE as u32);
                }
                _ => {}
            }
            m.set_i16(at(p::STATE), new_state);
            let unopposable = call_simple(context, host, call::UNOPPOSABLE, index);
            m.w8(at(p::UNOPPOSABLE_ENEMY), unopposable as u8);
            let target_weight = call_full(context, host, call::TARGET_WEIGHT, index, 0, 0.0).output[0];
            m.set_f32(at(p::TARGET_WEIGHT), target_weight);
        }

        if orphan_expired {
            let parent = prop_get(m, context, m.i32(at(p::ORPHAN_PROP_INDEX)));
            m.w32(parent + p::ORPHAN_PROP_INDEX as u32, NONE as u32);
            call_full(context, host, call::SWITCH_PROPS, index, NONE, 0.0);
            call_simple(context, host, call::PROP_DELETE, index);
        } else if m.bool(at(p::REFRESH_STIMULI)) && acknowledged(m.i16(at(p::STATE))) {
            stimuli_update(context, host, prop, index, became_acknowledged, expected_acknowledgement);
        } else if orphaned(m.i16(at(p::STATE))) && m.f32(at(p::DISTANCE)) < nearest_orphan_distance {
            interesting_orphan_index = index;
            nearest_orphan_distance = m.f32(at(p::DISTANCE));
        }

        let state = m.i16(at(p::STATE));
        let kind = if m.bool(at(p::DEAD)) {
            0
        } else if m.bool(at(p::ENEMY)) {
            3
        } else {
            6
        };
        let class = if acknowledged(state) {
            Some(0)
        } else if orphaned(state) {
            Some(1)
        } else if (UNACKNOWLEDGED..=BECOMING_ACKNOWLEDGED).contains(&state) {
            Some(2)
        } else {
            None
        };
        if let Some(class) = class {
            call_simple(context, host, call::PROFILE, kind + class);
        }
    }

    let target_prop_index = m.i32(actor + a::TARGET_TARGET_PROP_INDEX as u32);
    if target_prop_index != NONE {
        let target_prop = prop_get(m, context, target_prop_index);
        if orphaned(m.i16(target_prop + p::STATE as u32)) {
            interesting_orphan_index = NONE;
        }
    }
    let target_type = m.i16(actor + a::TARGET_TARGET_TYPE as u32);
    if target_type >= v::_ACTOR_TARGET_DEFINITE_ORPHAN as i16 {
        m.set_bool(actor + a::TARGET_ANY_TARGET_EVER as u32, true);
    }
    let since = actor + a::TARGET_SINCE_ANY_TARGET_VISIBLE_TIMER as u32;
    if target_type >= v::_ACTOR_TARGET_VISIBLE_ENEMY as i16 {
        m.w32(since, 0);
    } else if m.bool(actor + a::EXTERNAL_ORDERS_STAND_DOWN as u32) {
        m.w32(since, NONE as u32);
    } else if m.i32(since) != NONE {
        m.w32(since, m.r32(since).wrapping_add(1));
    }
    m.set_i16(actor + a::META_HIGHEST_PROP_TIMER as u32, highest_prop_timer);
    m.w32(actor + a::META_INTERESTING_ORPHAN_INDEX as u32, interesting_orphan_index as u32);
}

/// The danger zone: refreshed, acknowledged, noticed (by chance for a
/// vehicle or projectile) and whether the actor may dive from it.
fn danger_zone_update(context: &AiStepContext, host: &impl AiHost) {
    let m = host.memory();
    let actor = context.actor;
    let field = |offset: usize| actor + offset as u32;

    if m.bool(field(a::META_TIMESLICE)) {
        call_simple(context, host, call::REFRESH, 0);
    }
    call_simple(context, host, call::REFRESH_DANGER_ZONE, 0);

    let danger_type = m.i16(field(a::DANGER_ZONE_DANGER_TYPE));
    if danger_type <= v::_ACTOR_DANGER_ZONE_NONE as i16 {
        return;
    }
    let timer = field(a::DANGER_ZONE_ACKNOWLEDGEMENT_TIMER);
    let attached = field(a::DANGER_ZONE_ATTACHED_TO_US);
    let hostility = field(a::DANGER_ZONE_HOSTILITY);
    let noticed = field(a::DANGER_ZONE_NOTICED_DANGER);
    let allow_dive = field(a::DANGER_ZONE_ALLOW_DIVE_EVASION);
    let mut acknowledge_danger = false;

    if m.bool(attached) || m.i16(hostility) != 0 {
        m.set_bool(noticed, true);
        acknowledge_danger = m.i16(timer) > 0;
        m.set_i16(timer, 0);
    } else if m.i16(timer) > 0 && m.bool(field(a::DANGER_ZONE_CURRENTLY_PERCEIVED)) {
        let uncertain = m.i32(field(a::STATE_UNCERTAIN_COMBAT_TIMER));
        if m.i16(hostility) == 0 && !m.bool(attached) && (uncertain == NONE || uncertain >= 60) {
            acknowledge_danger = decrement(m, timer) == 0;
        } else {
            m.set_i16(timer, 0);
            acknowledge_danger = true;
        }
    }

    if acknowledge_danger {
        let definition = context.definition;
        let chance = if danger_type == v::_ACTOR_DANGER_ZONE_VEHICLE as i16 {
            Some(m.f32(definition + d::PERCEPTION_NOTICE_VEHICLE_CHANCE as u32))
        } else if danger_type == v::_ACTOR_DANGER_ZONE_PROJECTILE as i16 {
            Some(m.f32(definition + d::PERCEPTION_NOTICE_PROJECTILE_CHANCE as u32))
        } else {
            if danger_type == v::_ACTOR_DANGER_ZONE_SUICIDE as i16 {
                m.set_bool(noticed, true);
            }
            None
        };
        if let Some(chance) = chance
            && chance > 0.0
            && random(context, host) < chance
        {
            m.set_bool(noticed, true);
        }

        if m.bool(noticed) {
            if m.bool(attached) {
                m.set_bool(allow_dive, false);
            } else if m.i16(hostility) != 0
                || danger_type == v::_ACTOR_DANGER_ZONE_VEHICLE as i16
                || danger_type == v::_ACTOR_DANGER_ZONE_SUICIDE as i16
            {
                m.set_bool(allow_dive, true);
            } else {
                let dive = random(context, host) < m.f32(definition + d::MOVING_GRENADE_DIVE_CHANCE as u32);
                m.set_bool(allow_dive, dive);
            }
            call_simple(context, host, call::NOTICED_DANGER, 0);
        }
    }

    if m.i16(timer) == 0 {
        if m.i16(field(a::CONTROL_SECONDARY_LOOK_TYPE)) == v::_SECONDARY_LOOK_DANGEROUS_OBJECT as i16 {
            let priority = field(a::CONTROL_SECONDARY_LOOK_PRIORITY);
            m.set_i16(priority, m.i16(priority).min(v::_SECONDARY_LOOK_PRIORITY_TURN_AND_AIM as i16));
        }
        if m.bool(attached) {
            m.set_bool(noticed, true);
            m.set_bool(allow_dive, false);
        }
    }
}

/// An acknowledged prop's stimuli: just killed, sighted, the friendly
/// player first seen, an ally's changed allegiance, combat entered, and
/// the alerts between friends.
fn stimuli_update(
    context: &AiStepContext,
    host: &impl AiHost,
    prop: u32,
    index: i32,
    became_acknowledged: bool,
    expected_acknowledgement: bool,
) {
    let m = host.memory();
    let actor = context.actor;
    let at = |offset: usize| prop + offset as u32;
    let none = v::_ACTOR_PERCEPTION_NONE as i16;

    if m.bool(at(p::JUST_KILLED)) {
        call_simple(context, host, call::STIMULUS_JUST_KILLED, index);
        m.set_bool(at(p::JUST_KILLED), false);
    }
    if m.bool(at(p::JUST_BECAME_VISIBLE)) || (became_acknowledged && m.i16(at(p::VISIBILITY)) > none) {
        let initial_sighting = became_acknowledged && !expected_acknowledgement;
        call_full(context, host, call::STIMULUS_SIGHTED, index, initial_sighting as i32, 0.0);
        m.set_bool(at(p::JUST_BECAME_VISIBLE), false);
    }
    let sighted = actor + a::EMOTIONS_SIGHTED_FRIENDLY_PLAYER as u32;
    if !m.bool(sighted)
        && !m.bool(at(p::ENEMY))
        && m.bool(at(p::PLAYER))
        && m.i16(at(p::VISIBILITY)) >= PERCEPTION_FULL
        && m.i8(at(p::QUANTIZED_FACING)) <= 2
        && m.f32(at(p::DISTANCE)) < 7.0
    {
        m.set_bool(sighted, true);
        call_simple(context, host, call::SIGHTED_FRIEND_PLAYER, index);
        call_full(context, host, call::STIMULUS_SIGHTED, index, 0, 0.0);
    }

    let unit_index = m.i32(actor + a::META_UNIT_INDEX as u32);
    if unit_index != NONE && !m.bool(at(p::DEAD)) && m.bool(at(p::ALLY)) && m.bool(at(p::ALLY_STATUS_CHANGED)) {
        let enemy = call_full(
            context,
            host,
            call::TEAM_IS_ENEMY,
            m.i16(actor + a::META_TEAM_INDEX as u32) as i32,
            m.i16(at(p::TEAM_INDEX)) as i32,
            0.0,
        )
        .result
            != 0;
        let distance = m.f32(at(p::DISTANCE));
        let limit = if enemy {
            15.0
        } else if m.i8(at(p::QUANTIZED_FACING)) <= 2 {
            10.0
        } else {
            3.0
        };
        if (enemy && m.bool(at(p::CURRENTLY_DAMAGING_ME))) || distance < limit {
            call_full(context, host, call::ALLEGIANCE_CHANGED, index, enemy as i32, 0.0);
        }
    }

    let mode = m.i16(actor + a::STATE_MODE as u32);
    if mode < MODE_COMBAT {
        if m.bool(at(p::DEAD)) && !m.bool(at(p::ENEMY)) {
            call_simple(context, host, call::STIMULUS_FOUND_BODY, index);
        } else if m.bool(at(p::ENEMY)) {
            call_simple(context, host, call::STIMULUS_PERCEIVED_ENEMY, index);
        }
    }

    if !m.bool(at(p::ENEMY)) && !m.bool(at(p::DEAD)) && !m.bool(at(p::PLAYER)) {
        let since = m.i32(actor + a::TARGET_SINCE_ANY_TARGET_VISIBLE_TIMER as u32);
        let encounter_index = m.i32(actor + a::META_ENCOUNTER_INDEX as u32);
        let unseen = (m.i32(actor + a::TARGET_TARGET_PROP_INDEX as u32) != NONE && (since == NONE || since >= 180))
            || (encounter_index != NONE && call_simple(context, host, call::ENCOUNTER_ENEMY_UNSEEN, encounter_index) != 0);
        if unseen && m.i32(actor + a::META_UNIT_INDEX as u32) != NONE {
            if m.i16(actor + a::STATE_MODE as u32) < MODE_COMBAT {
                if m.bool(at(p::IN_COMBAT)) {
                    call_full(context, host, call::ALERT_NONCOMBAT, index, 0, 0.0);
                }
            } else if call_simple(context, host, call::IN_COMBAT, 0) != 0
                && call_simple(context, host, call::IS_FIGHTING, 0) == 0
                && m.bool(at(p::NONCOMBAT))
                && m.i16(at(p::VISIBILITY)) >= PERCEPTION_FULL
            {
                call_full(context, host, call::ALERT_NONCOMBAT, index, 1, 0.0);
            }
        }
    }
}
