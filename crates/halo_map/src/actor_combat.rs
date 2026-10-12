//! An actor's combat (actor_combat.c's `actor_combat_update`, every tick of
//! an actor's update that controls it): its firing timers, the target it
//! fires at (its target prop or a manual point) and what it knows of it,
//! its grenade throw, its special fire (overcharge or the secondary
//! trigger), the decision whether to fire (and why not), the fire state
//! (holding, bursting, pausing, wild) and its bursts' tracking, leading,
//! origin, aim and line of fire, and its triggers' rate of fire.
//!
//! The actor's datum, its definition, its variant's and its firing
//! variant's definitions, its props and the head of its debug info are the
//! game's memory, worked on in place ([`crate::ai_actor`]); the weapon (its
//! definition, its aim, its time to target, the vector it points), the
//! grenades, the difficulty, the random draws, the situations of special
//! fire, the players' visibility, the line of sight and fire, the
//! animation, the bursts' timing, the gun's position and the triggers are
//! the game's, through [`AiHost::call`] (actor_combat.c's
//! `actor_combat_call`, shared with the C).

use crate::ai_actor::{AiCall, AiHost, AiMemory, AiMemoryExt, AiStepContext, NONE, prop_get};
use crate::layout::{
    actor_datum as a, actor_debug_info as dbg, actor_definition as d, actor_variant_definition as var, ai_values as v,
    prop_datum as p, weapon_definition,
};

/// The combat update's calls (halo_sim.h's `_actor_combat_call_*`).
pub mod call {
    /// `actor_combat_get_firing_variant_definition(actor)`, its address the result.
    pub const FIRING_VARIANT: i32 = 1;
    /// `actor_get_weapon_definition(actor)`, its address (or 0) the result.
    pub const WEAPON_DEFINITION: i32 = 2;
    /// `actor_get_weapon(actor)`, the result.
    pub const WEAPON: i32 = 3;
    /// `actor_has_ranged_weapon(actor)`, the result.
    pub const HAS_RANGED_WEAPON: i32 = 4;
    /// `unit_get_grenade_count(unit, a)`, the result.
    pub const GRENADE_COUNT: i32 = 5;
    /// `unit_add_grenade_type_to_inventory(unit, a, 1)`.
    pub const ADD_GRENADE: i32 = 6;
    /// `actor_unit_control_throw_grenade(actor)` and its shout.
    pub const THROW_GRENADE: i32 = 7;
    /// The count of the weapon's triggers, the result.
    pub const WEAPON_TRIGGERS: i32 = 8;
    /// `game_difficulty_get_team_value` of the value `a` ([`super::difficulty`])
    /// for the actor's team, in `output[0]`.
    pub const DIFFICULTY: i32 = 9;
    /// `actor_combat_enable_special_fire_situation(actor, a)`, the result.
    pub const ENABLE_SPECIAL_FIRE: i32 = 10;
    /// `real_random_range(input[0], input[1])`, in `output[0]`.
    pub const RANDOM_RANGE: i32 = 11;
    /// `real_random()`, in `output[0]`.
    pub const RANDOM: i32 = 12;
    /// `actor_combat_allow_special_fire_situation(actor, a)`, the result.
    pub const ALLOW_SPECIAL_FIRE: i32 = 13;
    /// Whether the players' combined visibility holds the cluster `a`, the result.
    pub const PLAYERS_SEE_CLUSTER: i32 = 14;
    /// (no longer asked: [`crate::sensing::ai_test_line_of_sight`]) The line
    /// of sight from the actor's head to the manual target, the result.
    pub const MANUAL_LINE_OF_SIGHT: i32 = 15;
    /// `weapon_aim` at the fire target, into the actor: the result.
    pub const AIM_AT_TARGET: i32 = 16;
    /// `actor_move_animation_busy(actor)`, the result.
    pub const ANIMATION_BUSY: i32 = 17;
    /// `actor_get_weapon_vector(actor)`, in `output`.
    pub const WEAPON_VECTOR: i32 = 18;
    /// `actor_start_first_burst_delay(actor, its firing variant)`, the result.
    pub const FIRST_BURST_DELAY: i32 = 19;
    /// `actor_start_burst(actor)`.
    pub const START_BURST: i32 = 20;
    /// `actor_start_pause(actor)`.
    pub const START_PAUSE: i32 = 21;
    /// `weapon_estimate_time_to_target(weapon, a, input[0])`, in `output[0]`.
    pub const TIME_TO_TARGET: i32 = 22;
    /// `unit_get_camera_position(unit)`, in `output`.
    pub const CAMERA_POSITION: i32 = 23;
    /// `unit_estimate_position` of the gun with the offset `a`
    /// ([`super::gun_offset`]) and the facing in `input`: in `output`.
    pub const GUN_POSITION: i32 = 24;
    /// `weapon_aim` of the burst from the origin in `input`, into the actor:
    /// whether the aim is linear, the result.
    pub const AIM_BURST: i32 = 25;
    /// `ai_test_line_of_fire(actor, a, input[0..3], input[3..6])`: the
    /// result, the blocking prop in `result2`.
    pub const LINE_OF_FIRE: i32 = 26;
    /// The blocked shout (the cause `a`).
    pub const BLOCKED_SHOUT: i32 = 27;
    /// The firing pattern's rate of fire modifier: whether there is a
    /// pattern the result, the modifier in `output[0]`.
    pub const FIRING_PATTERN: i32 = 28;
    /// `actor_unit_control_primary_trigger(actor, a, input[0])`.
    pub const PRIMARY_TRIGGER: i32 = 29;
    /// `actor_unit_control_secondary_trigger(actor, a)`.
    pub const SECONDARY_TRIGGER: i32 = 30;
}

/// The difficulty values [`call::DIFFICULTY`] reads.
pub mod difficulty {
    pub const TARGET_TRACKING: i32 = 0;
    pub const TARGET_LEADING: i32 = 1;
    pub const RATE_OF_FIRE: i32 = 2;
}

/// The gun offsets [`call::GUN_POSITION`] takes.
pub mod gun_offset {
    pub const VARIANT_CROUCH: i32 = 0;
    pub const DEFINITION_CROUCH: i32 = 1;
    pub const VARIANT_STAND: i32 = 2;
    pub const DEFINITION_STAND: i32 = 3;
}

const EPSILON: f32 = 0.0001;

/// The local enums (actor_combat.c's).
const FIRE_TARGET_NONE: i16 = 0;
const FIRE_TARGET_PROP: i16 = 1;
const FIRE_TARGET_MANUAL_POINT: i16 = 2;
const SPECIAL_FIRE_MODE_NONE: i16 = 0;
const SPECIAL_FIRE_MODE_OVERCHARGE: i16 = 1;
const SPECIAL_FIRE_MODE_SECONDARY: i16 = 2;
const SPECIAL_FIRE_SITUATION_STRAFING: i16 = 3;
const FLAGS2_MUST_STAND_TO_FIRE_BIT: u32 = 1;
const FLAGS2_MUST_STOP_TO_FIRE_BIT: u32 = 2;
const TICKS_PER_SECOND: f32 = 30.0;

type Vector = [f32; 3];

fn call_full(context: &AiStepContext, host: &impl AiHost, kind: i32, a: i32, input: &[f32]) -> AiCall {
    let mut call = AiCall { kind, actor: context.actor_index, a, ..AiCall::default() };
    call.input[..input.len()].copy_from_slice(input);
    host.call(&mut call);
    call
}

fn call_simple(context: &AiStepContext, host: &impl AiHost, kind: i32, a: i32) -> i32 {
    call_full(context, host, kind, a, &[]).result
}

fn call_real(context: &AiStepContext, host: &impl AiHost, kind: i32, a: i32, input: &[f32]) -> f32 {
    call_full(context, host, kind, a, input).output[0]
}

fn sub(a: Vector, b: Vector) -> Vector {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn magnitude_squared(v: Vector) -> f32 {
    v[0] * v[0] + v[1] * v[1] + v[2] * v[2]
}

fn dot(a: Vector, b: Vector) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// `point_from_line3d`.
fn point_from_line(p: Vector, v: Vector, t: f32) -> Vector {
    [v[0] * t + p[0], v[1] * t + p[1], v[2] * t + p[2]]
}

/// `fast_ftol`: rounds to nearest, ties to even.
pub(crate) fn fast_ftol(value: f32) -> i32 {
    let rounded = (value as f64).round_ties_even();
    if (-2_147_483_648.0..2_147_483_648.0).contains(&rounded) { rounded as i32 } else { i32::MIN }
}

/// `(short)value` through a long, as x86 converts it.
fn c_short(value: f32) -> i16 {
    let long = if (-2_147_483_648.0..2_147_483_648.0).contains(&value) { value as i32 } else { i32::MIN };
    long as i16
}

fn test_flag(flags: u32, bit: u32) -> bool {
    flags & (1 << bit) != 0
}

struct Memory<'a> {
    m: &'a dyn AiMemory,
    base: u32,
}

impl Memory<'_> {
    fn bool(&self, offset: usize) -> bool {
        self.m.bool(self.base + offset as u32)
    }
    fn set_bool(&self, offset: usize, value: bool) {
        self.m.set_bool(self.base + offset as u32, value);
    }
    fn i16(&self, offset: usize) -> i16 {
        self.m.i16(self.base + offset as u32)
    }
    fn set_i16(&self, offset: usize, value: i16) {
        self.m.set_i16(self.base + offset as u32, value);
    }
    fn i32(&self, offset: usize) -> i32 {
        self.m.i32(self.base + offset as u32)
    }
    fn set_i32(&self, offset: usize, value: i32) {
        self.m.w32(self.base + offset as u32, value as u32);
    }
    fn f32(&self, offset: usize) -> f32 {
        self.m.f32(self.base + offset as u32)
    }
    fn set_f32(&self, offset: usize, value: f32) {
        self.m.set_f32(self.base + offset as u32, value);
    }
    fn vector(&self, offset: usize) -> Vector {
        self.m.vector(self.base + offset as u32)
    }
    fn set_vector(&self, offset: usize, vector: Vector) {
        for (i, value) in vector.into_iter().enumerate() {
            self.m.set_f32(self.base + offset as u32 + 4 * i as u32, value);
        }
    }
    fn decrement(&self, offset: usize) {
        if self.i16(offset) > 0 {
            self.set_i16(offset, self.i16(offset) - 1);
        }
    }
}

/// `actor_combat_update`.
pub fn actor_combat_update(context: &AiStepContext, host: &impl AiHost) {
    let m = host.memory();
    let actor = Memory { m, base: context.actor };
    let debug = Memory { m, base: context.debug_info };
    let variant = Memory { m, base: context.variant_definition };
    let definition = Memory { m, base: context.definition };
    let firing_variant = Memory { m, base: call_simple(context, host, call::FIRING_VARIANT, 0) as u32 };
    let weapon_definition_address = call_simple(context, host, call::WEAPON_DEFINITION, 0) as u32;
    let weapon_index = call_simple(context, host, call::WEAPON, 0);
    let mut hold_burst_start = false;
    let mut firing_decision = NONE as i16;
    let fire;

    actor.decrement(a::CONTROL_FIRE_STATE_TIMER);
    actor.decrement(a::CONTROL_BURST_DISABLE_TIMER);
    actor.decrement(a::CONTROL_TRIGGER_DELAY_TIMER);
    actor.decrement(a::CONTROL_SPECIAL_FIRE_DELAY);
    if actor.i16(a::CONTROL_CURRENT_FIRE_TARGET_TYPE) > FIRE_TARGET_NONE {
        actor.set_i32(a::CONTROL_CURRENT_FIRE_TARGET_TIMER, actor.i32(a::CONTROL_CURRENT_FIRE_TARGET_TIMER).wrapping_add(1));
    }

    let fire_state = |actor: &Memory| actor.i16(a::CONTROL_FIRE_STATE);
    if fire_state(&actor) != v::_ACTOR_FIRE_STATE_BURSTING as i16 {
        let mut fire_target_type = FIRE_TARGET_NONE;
        if actor.bool(a::ORDERS_COMBAT_SHOOT_AT_TARGET) {
            if actor.bool(a::ORDERS_COMBAT_USE_MANUAL_TARGET_POINT) {
                fire_target_type = FIRE_TARGET_MANUAL_POINT;
            } else if actor.i32(a::TARGET_TARGET_PROP_INDEX) != NONE {
                fire_target_type = FIRE_TARGET_PROP;
            }
        }
        let target_changed = if fire_target_type != actor.i16(a::CONTROL_CURRENT_FIRE_TARGET_TYPE) {
            true
        } else if fire_target_type == FIRE_TARGET_PROP {
            actor.i32(a::TARGET_TARGET_PROP_INDEX) != actor.i32(a::CONTROL_CURRENT_FIRE_TARGET_PROP_INDEX)
        } else if fire_target_type == FIRE_TARGET_MANUAL_POINT {
            magnitude_squared(sub(
                actor.vector(a::CONTROL_CURRENT_FIRE_TARGET_MANUAL_POINT),
                actor.vector(a::ORDERS_COMBAT_TARGET_POINT),
            )) > 0.25
        } else {
            false
        };
        if target_changed {
            actor.set_i32(a::CONTROL_CURRENT_FIRE_TARGET_TIMER, 0);
        }
        actor.set_i16(a::CONTROL_CURRENT_FIRE_TARGET_TYPE, fire_target_type);
        if fire_target_type == FIRE_TARGET_PROP {
            actor.set_i32(a::CONTROL_CURRENT_FIRE_TARGET_PROP_INDEX, actor.i32(a::TARGET_TARGET_PROP_INDEX));
        } else if fire_target_type == FIRE_TARGET_MANUAL_POINT {
            actor.set_vector(a::CONTROL_CURRENT_FIRE_TARGET_MANUAL_POINT, actor.vector(a::ORDERS_COMBAT_TARGET_POINT));
        }
    }

    actor.set_bool(a::CONTROL_AIMING_AT_FIRE_TARGET, false);
    let maximum_range = if call_simple(context, host, call::HAS_RANGED_WEAPON, 0) != 0 {
        firing_variant.f32(var::RANGED_COMBAT_MAXIMUM_FIRING_RANGE)
    } else {
        0.0
    };
    actor.set_f32(a::CONTROL_WEAPON_MAXIMUM_RANGE, maximum_range);

    let override_restrictions = actor.bool(a::ORDERS_COMBAT_OVERRIDE_FIRING_RESTRICTIONS);
    if actor.bool(a::ORDERS_COMBAT_THROW_GRENADE) {
        let grenade_type = variant.i16(var::GRENADE_COMBAT_GRENADE_TYPE);
        if grenade_type != NONE as i16 && call_simple(context, host, call::GRENADE_COUNT, grenade_type as i32) == 0 {
            call_simple(context, host, call::ADD_GRENADE, grenade_type as i32);
        }
        call_simple(context, host, call::THROW_GRENADE, 0);
        fire = false;
    } else if call_simple(context, host, call::HAS_RANGED_WEAPON, 0) == 0 {
        fire = false;
    } else if fire_state(&actor) == v::_ACTOR_FIRE_STATE_WILD as i16 {
        fire = true;
    } else {
        special_fire(context, host, &actor, &firing_variant);

        let target_type = actor.i16(a::CONTROL_CURRENT_FIRE_TARGET_TYPE);
        if target_type > FIRE_TARGET_NONE {
            if target_type == FIRE_TARGET_PROP {
                let prop = Memory { m, base: prop_get(m, context, actor.i32(a::CONTROL_CURRENT_FIRE_TARGET_PROP_INDEX)) };
                actor.set_f32(a::CONTROL_CURRENT_FIRE_TARGET_RANGE, prop.f32(p::DISTANCE));
                actor.set_vector(a::CONTROL_CURRENT_FIRE_TARGET_POSITION, prop.vector(p::CENTER_OF_MASS));
                actor.set_i16(a::CONTROL_CURRENT_FIRE_TARGET_LINE_OF_SIGHT, prop.i16(p::LINE_OF_SIGHT));
                m.w8(context.actor + a::CONTROL_CURRENT_FIRE_TARGET_UNDERWATER as u32, m.r8(prop.base + p::UNDERWATER as u32));
                actor.set_bool(a::CONTROL_CURRENT_FIRE_TARGET_OUTSIDE_ACTIVE_AREA, true);
                let cluster = prop.i16(p::BODY_LOCATION_CLUSTER_INDEX);
                if cluster != NONE as i16 {
                    let seen = call_simple(context, host, call::PLAYERS_SEE_CLUSTER, cluster as i32) != 0;
                    actor.set_bool(a::CONTROL_CURRENT_FIRE_TARGET_OUTSIDE_ACTIVE_AREA, !seen);
                }
            } else {
                let manual_point = actor.vector(a::CONTROL_CURRENT_FIRE_TARGET_MANUAL_POINT);
                actor.set_vector(a::CONTROL_CURRENT_FIRE_TARGET_POSITION, manual_point);
                let range = magnitude_squared(sub(manual_point, actor.vector(a::INPUT_POSITION_HEAD_POSITION))).sqrt();
                actor.set_f32(a::CONTROL_CURRENT_FIRE_TARGET_RANGE, range);
                actor.set_bool(a::CONTROL_CURRENT_FIRE_TARGET_UNDERWATER, false);
                actor.set_bool(a::CONTROL_CURRENT_FIRE_TARGET_OUTSIDE_ACTIVE_AREA, false);
                if actor.i32(a::CONTROL_CURRENT_FIRE_TARGET_TIMER) % 10 == 0 {
                    let m = host.memory();
                    let at = |offset: usize| context.actor + offset as u32;
                    let line_of_sight = crate::sensing::ai_test_line_of_sight(
                        context,
                        host,
                        m.vector(at(a::INPUT_POSITION_HEAD_POSITION)),
                        m.i16(at(a::INPUT_POSITION_BODY_LOCATION_CLUSTER_INDEX)),
                        m.vector(at(a::CONTROL_CURRENT_FIRE_TARGET_POSITION)),
                        NONE as i16,
                        crate::sensing::mode::NORMAL,
                        false,
                        NONE,
                        m.i32(at(a::INPUT_VEHICLE_INDEX)) != NONE,
                    );
                    actor.set_i16(a::CONTROL_CURRENT_FIRE_TARGET_LINE_OF_SIGHT, line_of_sight);
                }
            }

            let range = actor.f32(a::CONTROL_CURRENT_FIRE_TARGET_RANGE);
            let super_ballistic_range = firing_variant.f32(var::RANGED_COMBAT_WEAPON_SUPER_BALLISTIC_RANGE);
            actor.set_bool(
                a::CONTROL_CURRENT_FIRE_TARGET_SUPERBALLISTIC,
                super_ballistic_range > 0.0 && range > super_ballistic_range,
            );
            actor.set_bool(
                a::CONTROL_CURRENT_FIRE_TARGET_BOMBARDMENT,
                actor.bool(a::ORDERS_COMBAT_BOMBARD_TARGET)
                    && firing_variant.f32(var::RANGED_COMBAT_WEAPON_BOMBARDMENT_RANGE) > 0.0,
            );
            if call_simple(context, host, call::AIM_AT_TARGET, weapon_index) == 0 {
                actor.set_i16(a::CONTROL_CURRENT_FIRE_TARGET_TYPE, FIRE_TARGET_NONE);
            }
        }

        let restricted = |bit_set: bool| !override_restrictions && bit_set;
        let definition_flags = definition.i32(d::FLAGS) as u32;
        let definition_flags2 = definition.i32(d::FLAGS2) as u32;
        let range = actor.f32(a::CONTROL_CURRENT_FIRE_TARGET_RANGE);
        let mut decide = |decision: usize| {
            firing_decision = decision as i16;
            false
        };
        fire = if actor.i16(a::CONTROL_CURRENT_FIRE_TARGET_TYPE) == FIRE_TARGET_NONE {
            decide(v::_FIRING_NO_TARGET)
        } else if actor.bool(a::CONTROL_CURRENT_FIRE_TARGET_OUTSIDE_ACTIVE_AREA) {
            decide(v::_FIRING_OUTSIDE_ACTIVE_AREA)
        } else if restricted(actor.i16(a::CONTROL_BURST_DISABLE_TIMER) > 0) {
            decide(v::_FIRING_DISABLED)
        } else if call_simple(context, host, call::ANIMATION_BUSY, 0) != 0 {
            decide(v::_FIRING_BUSY)
        } else if restricted(
            actor.bool(a::INPUT_IN_MIDAIR)
                && !actor.bool(a::STATE_FLYING)
                && !test_flag(variant.i32(var::FLAGS) as u32, v::_ACTOR_VARIANT_DEFINITION_CAN_SHOOT_WHILE_FLYING_BIT as u32),
        ) {
            decide(v::_FIRING_NOT_IN_MIDAIR)
        } else if restricted(
            test_flag(definition_flags, v::_ACTOR_DEFINITION_MUST_CROUCH_TO_FIRE_BIT as u32) && !actor.bool(a::CONTROL_CROUCHING),
        ) {
            decide(v::_FIRING_NOT_CROUCHING)
        } else if restricted(test_flag(definition_flags2, FLAGS2_MUST_STAND_TO_FIRE_BIT) && actor.bool(a::CONTROL_CROUCHING)) {
            decide(v::_FIRING_NOT_STANDING)
        } else if restricted(test_flag(definition_flags2, FLAGS2_MUST_STOP_TO_FIRE_BIT) && actor.bool(a::CONTROL_MOVING)) {
            decide(v::_FIRING_NOT_STATIONARY)
        } else if actor.bool(a::CONTROL_CURRENT_FIRE_TARGET_UNDERWATER) || actor.bool(a::INPUT_UNDERWATER) {
            decide(v::_FIRING_UNDERWATER)
        } else if weapon_definition_address != 0 && {
            let minimum = m.f32(weapon_definition_address + weapon_definition::WEAPON_AI_MINIMUM_TARGET_RANGE as u32);
            minimum > 0.0 && range < minimum
        } {
            decide(v::_FIRING_MIN_RANGE)
        } else if actor.i16(a::ORDERS_LOOK_PRIMARY_PRIORITY) == v::_PRIMARY_PRIORITY_NONE as i16
            || actor.i16(a::ORDERS_LOOK_PRIMARY_DIRECTION_TYPE) != v::_DIRECTION_SPECIFICATION_TARGET as i16
            || actor.bool(a::CONTROL_AIMING_AWAY_FROM_PRIMARY)
        {
            decide(v::_FIRING_WRONG_TARGET)
        } else if fire_state(&actor) == v::_ACTOR_FIRE_STATE_BURSTING as i16 {
            true
        } else {
            let line_of_sight = actor.i16(a::CONTROL_CURRENT_FIRE_TARGET_LINE_OF_SIGHT);
            let visible = line_of_sight == v::_AI_LINE_OF_SIGHT_CLEAR as i16 || line_of_sight == v::_AI_LINE_OF_SIGHT_OCCLUDED as i16;
            actor.set_bool(a::CONTROL_CURRENT_FIRE_TARGET_VISIBLE, visible);
            if !visible && !actor.bool(a::CONTROL_CURRENT_FIRE_TARGET_BOMBARDMENT) {
                decide(v::_FIRING_NOT_VISIBLE)
            } else if restricted(range >= actor.f32(a::CONTROL_WEAPON_MAXIMUM_RANGE)) {
                decide(v::_FIRING_OUTSIDE_RANGE)
            } else {
                actor.set_bool(a::CONTROL_AIMING_AT_FIRE_TARGET, true);
                debug.set_i32(dbg::BURST_ALIGNMENT_TIME, context.game_time);
                if test_flag(definition_flags, v::_ACTOR_DEFINITION_START_FIRING_BEFORE_ALIGNED_BIT as u32) {
                    debug.set_bool(dbg::BURST_ALIGNMENT_ALIGNED, true);
                    debug.set_bool(dbg::BURST_ALIGNMENT_ALIGNED_IMMEDIATELY, true);
                } else {
                    let threshold = if range < 1.5 {
                        range * ((0.97f32 - 0.707_106_77f32) / 1.5f32) + 0.707_106_77
                    } else {
                        0.97
                    };
                    let weapon_vector = call_full(context, host, call::WEAPON_VECTOR, 0, &[]).vector();
                    let aim_vector = actor.vector(a::CONTROL_CURRENT_FIRE_TARGET_AIM_VECTOR);
                    let alignment = dot(weapon_vector, aim_vector);
                    if alignment < threshold {
                        hold_burst_start = true;
                    }
                    debug.set_vector(dbg::BURST_ALIGNMENT_WEAPON_VECTOR, weapon_vector);
                    debug.set_vector(dbg::BURST_ALIGNMENT_AIM_VECTOR, aim_vector);
                    debug.set_f32(dbg::BURST_ALIGNMENT_ALIGNMENT, alignment);
                    debug.set_f32(dbg::BURST_ALIGNMENT_THRESHOLD, threshold);
                    debug.set_bool(dbg::BURST_ALIGNMENT_ALIGNED, alignment >= threshold);
                    debug.set_bool(dbg::BURST_ALIGNMENT_ALIGNED_IMMEDIATELY, false);
                }
                true
            }
        };
    }

    if !fire {
        actor.set_i16(a::CONTROL_FIRE_STATE, v::_ACTOR_FIRE_STATE_NONE as i16);
    }
    let state = fire_state(&actor);
    let timer = actor.i16(a::CONTROL_FIRE_STATE_TIMER);
    if state != v::_ACTOR_FIRE_STATE_NONE as i16 {
        firing_decision = match state as usize {
            v::_ACTOR_FIRE_STATE_WILD => v::_FIRING_WILD,
            v::_ACTOR_FIRE_STATE_HOLDING => {
                if timer != 0 { v::_FIRING_HOLDING } else { v::_FIRING_HOLDING_FOR_LINE }
            }
            v::_ACTOR_FIRE_STATE_BURSTING => v::_FIRING_BURST,
            v::_ACTOR_FIRE_STATE_PAUSING => {
                if timer != 0 { v::_FIRING_PAUSING } else { v::_FIRING_PAUSING_FOR_LINE }
            }
            _ => firing_decision as usize,
        } as i16;
    }

    let mut new_state = NONE as i16;
    match state as usize {
        v::_ACTOR_FIRE_STATE_NONE if fire => new_state = v::_ACTOR_FIRE_STATE_HOLDING as i16,
        v::_ACTOR_FIRE_STATE_BURSTING if timer == 0 => new_state = v::_ACTOR_FIRE_STATE_PAUSING as i16,
        v::_ACTOR_FIRE_STATE_HOLDING | v::_ACTOR_FIRE_STATE_PAUSING if !hold_burst_start && timer == 0 => {
            new_state = v::_ACTOR_FIRE_STATE_BURSTING as i16
        }
        v::_ACTOR_FIRE_STATE_WILD if timer == 0 => new_state = v::_ACTOR_FIRE_STATE_NONE as i16,
        _ => {}
    }
    if new_state != NONE as i16 {
        match new_state as usize {
            v::_ACTOR_FIRE_STATE_HOLDING => {
                if call_simple(context, host, call::FIRST_BURST_DELAY, 0) == 0 && !hold_burst_start {
                    new_state = v::_ACTOR_FIRE_STATE_BURSTING as i16;
                    call_simple(context, host, call::START_BURST, 0);
                }
            }
            v::_ACTOR_FIRE_STATE_BURSTING => {
                call_simple(context, host, call::START_BURST, 0);
            }
            v::_ACTOR_FIRE_STATE_PAUSING => {
                call_simple(context, host, call::START_PAUSE, 0);
            }
            _ => {}
        }
        actor.set_i16(a::CONTROL_FIRE_STATE, new_state);
    }

    let mut fire_primary = false;
    let mut fire_secondary = false;
    actor.set_bool(a::CONTROL_BURST_AIM_BY_VECTOR, false);
    if fire_state(&actor) == v::_ACTOR_FIRE_STATE_WILD as i16 {
        fire_primary = true;
    } else if fire_state(&actor) == v::_ACTOR_FIRE_STATE_BURSTING as i16 {
        let clear = burst(context, host, &actor, &debug, &definition, &firing_variant, weapon_index, &mut firing_decision);
        if clear {
            if actor.bool(a::CONTROL_FIRE_BURST_SECONDARY) {
                fire_secondary = true;
            } else {
                fire_primary = true;
            }
        }
    }

    let mut primary_trigger = false;
    let mut secondary_trigger = false;
    let mut primary_trigger_value = 0.0f32;
    if fire_primary {
        let mut rate_of_fire = firing_variant.f32(var::RANGED_COMBAT_RATE_OF_FIRE);
        if actor.bool(a::CONTROL_OVERCHARGING_WEAPON) {
            actor.set_bool(a::CONTROL_OVERCHARGING_WEAPON, false);
        } else if rate_of_fire == 0.0 {
            primary_trigger = true;
            primary_trigger_value = 1.0;
        } else if actor.i16(a::CONTROL_TRIGGER_DELAY_TIMER) == 0 {
            primary_trigger = true;
            primary_trigger_value = 1.0;
            rate_of_fire *= call_real(context, host, call::DIFFICULTY, difficulty::RATE_OF_FIRE, &[]);
            let pattern = call_full(context, host, call::FIRING_PATTERN, 0, &[]);
            if pattern.result != 0 && pattern.output[0] > 0.0 {
                rate_of_fire *= pattern.output[0];
            }
            debug.set_f32(dbg::SHOOTING_ROF, rate_of_fire);
            let trigger_delay = fast_ftol(TICKS_PER_SECOND / rate_of_fire) as i16;
            actor.set_i16(a::CONTROL_TRIGGER_DELAY_TIMER, trigger_delay.max(2));
        }
    } else if fire_secondary {
        if actor.bool(a::CONTROL_OVERCHARGING_WEAPON) {
            actor.set_bool(a::CONTROL_OVERCHARGING_WEAPON, false);
        } else {
            secondary_trigger = true;
        }
    } else if actor.bool(a::CONTROL_OVERCHARGING_WEAPON) {
        primary_trigger = true;
        primary_trigger_value = 1.0;
    }
    call_full(context, host, call::PRIMARY_TRIGGER, primary_trigger as i32, &[primary_trigger_value]);
    call_simple(context, host, call::SECONDARY_TRIGGER, secondary_trigger as i32);

    debug.set_i16(dbg::FIRING_DECISION, firing_decision);
}

/// Special fire: an overcharge or the secondary trigger, by chance, after
/// its delay, in its situation.
fn special_fire(context: &AiStepContext, host: &impl AiHost, actor: &Memory, firing_variant: &Memory) {
    let mode = firing_variant.i16(var::RANGED_COMBAT_SPECIAL_FIRE_MODE);
    let state = actor.i16(a::CONTROL_FIRE_STATE);
    if !(mode > SPECIAL_FIRE_MODE_NONE
        && state != v::_ACTOR_FIRE_STATE_BURSTING as i16
        && state != v::_ACTOR_FIRE_STATE_WILD as i16
        && actor.i16(a::CONTROL_SPECIAL_FIRE_DELAY) <= 0
        && actor.i16(a::CONTROL_SPECIAL_FIRE_DENY_ATTEMPTS) <= 0)
    {
        return;
    }
    let triggers = call_simple(context, host, call::WEAPON_TRIGGERS, 0);
    let special = match mode {
        SPECIAL_FIRE_MODE_OVERCHARGE => triggers > 0,
        SPECIAL_FIRE_MODE_SECONDARY => triggers > 1,
        _ => true,
    };
    let situation = firing_variant.i16(var::RANGED_COMBAT_SPECIAL_FIRE_SITUATION);
    if special && call_simple(context, host, call::ENABLE_SPECIAL_FIRE, situation as i32) != 0 {
        let delay = firing_variant.f32(var::RANGED_COMBAT_SPECIAL_FIRE_DELAY)
            + call_real(context, host, call::RANDOM_RANGE, 0, &[0.0, 1.5]);
        let random = call_real(context, host, call::RANDOM, 0, &[]);
        actor.set_i16(a::CONTROL_SPECIAL_FIRE_DELAY, c_short(delay * TICKS_PER_SECOND));
        if random < firing_variant.f32(var::RANGED_COMBAT_SPECIAL_FIRE_CHANCE)
            && call_simple(context, host, call::ALLOW_SPECIAL_FIRE, situation as i32) != 0
        {
            if situation == SPECIAL_FIRE_SITUATION_STRAFING {
                actor.set_i16(a::CONTROL_SPECIAL_FIRE_DENY_ATTEMPTS, 3);
            }
            match mode {
                SPECIAL_FIRE_MODE_OVERCHARGE => actor.set_bool(a::CONTROL_OVERCHARGING_WEAPON, true),
                SPECIAL_FIRE_MODE_SECONDARY => actor.set_bool(a::CONTROL_NEXT_BURST_SECONDARY, true),
                _ => {}
            }
        }
    }
}

/// A burst's shot: its target tracked and led, its origin, its aim and its
/// line of fire. Whether the line is clear.
#[allow(clippy::too_many_arguments)]
fn burst(
    context: &AiStepContext,
    host: &impl AiHost,
    actor: &Memory,
    debug: &Memory,
    definition: &Memory,
    firing_variant: &Memory,
    weapon_index: i32,
    firing_decision: &mut i16,
) -> bool {
    let m = actor.m;
    let mut ignore_unit_index = NONE;
    let initial = actor.vector(a::CONTROL_BURST_INITIAL_POSITION);
    let mut origin_point = initial;
    actor.set_vector(a::CONTROL_BURST_ORIGIN, origin_point);
    debug.set_vector(dbg::BURST_TRACKED_POSITION, initial);
    debug.set_vector(dbg::BURST_LEAD_VECTOR, [0.0; 3]);

    if actor.i16(a::CONTROL_CURRENT_FIRE_TARGET_TYPE) == FIRE_TARGET_PROP {
        let prop = Memory { m, base: prop_get(m, context, actor.i32(a::CONTROL_CURRENT_FIRE_TARGET_PROP_INDEX)) };
        let center_of_mass = prop.vector(p::CENTER_OF_MASS);
        debug.set_vector(dbg::BURST_LAST_KNOWN_POSITION, center_of_mass);
        ignore_unit_index = prop.i32(p::ATTACHED_TO_UNIT_INDEX);

        let track_fraction = firing_variant.f32(var::RANGED_COMBAT_TARGET_TRACK_FRACTION);
        let tracking = track_fraction + call_real(context, host, call::DIFFICULTY, difficulty::TARGET_TRACKING, &[]);
        if (if 1.0 < tracking { 1.0 } else { tracking }) > 0.0 && !actor.bool(a::CONTROL_CURRENT_FIRE_TARGET_BOMBARDMENT) {
            origin_point = point_from_line(origin_point, sub(center_of_mass, initial), track_fraction);
            actor.set_vector(a::CONTROL_BURST_ORIGIN, origin_point);
        }
        debug.set_vector(dbg::BURST_TRACKED_POSITION, origin_point);

        let lead_fraction = firing_variant.f32(var::RANGED_COMBAT_TARGET_LEAD_FRACTION);
        let leading = lead_fraction + call_real(context, host, call::DIFFICULTY, difficulty::TARGET_LEADING, &[]);
        if (if 1.0 < leading { 1.0 } else { leading }) > 0.0 {
            let time_to_target = call_real(
                context,
                host,
                call::TIME_TO_TARGET,
                actor.bool(a::CONTROL_FIRE_BURST_SECONDARY) as i32,
                &[actor.f32(a::CONTROL_CURRENT_FIRE_TARGET_DISTANCE)],
            );
            let velocity = prop.vector(p::VELOCITY);
            let lead = [time_to_target * velocity[0], time_to_target * velocity[1], time_to_target * velocity[2]];
            debug.set_vector(dbg::BURST_LEAD_VECTOR, lead);
            origin_point = point_from_line(origin_point, lead, lead_fraction);
            actor.set_vector(a::CONTROL_BURST_ORIGIN, origin_point);
        }
    } else {
        debug.set_vector(dbg::BURST_LAST_KNOWN_POSITION, origin_point);
    }

    let adjustment = actor.vector(a::CONTROL_BURST_ADJUSTMENT);
    let relative = actor.vector(a::CONTROL_BURST_RELATIVE_POSITION);
    let relative = [adjustment[0] + relative[0], adjustment[1] + relative[1], adjustment[2] + relative[2]];
    actor.set_vector(a::CONTROL_BURST_RELATIVE_POSITION, relative);
    let target = [relative[0] + origin_point[0], relative[1] + origin_point[1], relative[2] + origin_point[2]];
    actor.set_vector(a::CONTROL_BURST_TARGET, target);

    let origin = if actor.i32(a::INPUT_VEHICLE_INDEX) != NONE {
        call_full(context, host, call::CAMERA_POSITION, 0, &[]).vector()
    } else {
        let offsets = if actor.bool(a::CONTROL_CROUCHING) {
            [
                (firing_variant.vector(var::RANGED_COMBAT_GUN_OFFSET_CROUCH), gun_offset::VARIANT_CROUCH),
                (definition.vector(d::PERCEPTION_GUN_OFFSET_CROUCH), gun_offset::DEFINITION_CROUCH),
            ]
        } else {
            [
                (firing_variant.vector(var::RANGED_COMBAT_GUN_OFFSET_STAND), gun_offset::VARIANT_STAND),
                (definition.vector(d::PERCEPTION_GUN_OFFSET_STAND), gun_offset::DEFINITION_STAND),
            ]
        };
        match offsets.iter().find(|(offset, _)| magnitude_squared(*offset) > EPSILON) {
            None => actor.vector(a::INPUT_POSITION_HEAD_POSITION),
            Some(&(_, which)) => {
                let facing3 = sub(actor.vector(a::INPUT_BODY_POSITION), target);
                let mut planar = [facing3[0], facing3[1]];
                let magnitude = (planar[0] * planar[0] + planar[1] * planar[1]).sqrt();
                let facing = if !(EPSILON > magnitude.abs()) {
                    let s = 1.0 / magnitude;
                    planar = [s * planar[0], s * planar[1]];
                    if magnitude > 0.0 { [planar[0], planar[1], 0.0] } else { actor.vector(a::INPUT_FACING_VECTOR) }
                } else {
                    actor.vector(a::INPUT_FACING_VECTOR)
                };
                call_full(context, host, call::GUN_POSITION, which, &facing).vector()
            }
        }
    };

    let linear = call_full(context, host, call::AIM_BURST, weapon_index, &origin).result != 0;
    actor.set_bool(a::CONTROL_BURST_AIM_BY_VECTOR, !linear);

    let fire_vector = sub(target, origin);
    let line = call_full(
        context,
        host,
        call::LINE_OF_FIRE,
        ignore_unit_index,
        &[origin[0], origin[1], origin[2], fire_vector[0], fire_vector[1], fire_vector[2]],
    );
    if line.result != 0 {
        actor.set_i16(a::CONTROL_BLOCKED_COMMUNICATION_TIMER, 0);
        return true;
    }
    let blocked = actor.i16(a::CONTROL_BLOCKED_COMMUNICATION_TIMER).wrapping_add(1);
    actor.set_i16(a::CONTROL_BLOCKED_COMMUNICATION_TIMER, blocked);
    actor.set_i16(a::CONTROL_FIRE_STATE_TIMER, actor.i16(a::CONTROL_FIRE_STATE_TIMER).wrapping_add(1));
    *firing_decision = v::_FIRING_BLOCKED as i16;
    if blocked >= 45 && actor.i16(a::TARGET_TARGET_TYPE) >= v::_ACTOR_TARGET_ACKNOWLEDGED_ENEMY as i16 {
        let blocking_prop_index = line.result2;
        let cause_unit_index = if blocking_prop_index != NONE {
            m.i32(prop_get(m, context, blocking_prop_index) + p::UNIT_INDEX as u32)
        } else {
            NONE
        };
        call_simple(context, host, call::BLOCKED_SHOUT, cause_unit_index);
        actor.set_i16(a::CONTROL_BLOCKED_COMMUNICATION_TIMER, 0);
    }
    false
}
