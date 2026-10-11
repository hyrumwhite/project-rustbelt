//! An actor's looking (actor_looking.c's `actor_look_update`, every tick of
//! an actor's update that controls it): which way it faces, aims and looks,
//! weighing its primary look (its orders, or its target while it fires a
//! burst) against its secondary look (an event it turned to) and its idle
//! looking, within its definition's aiming and looking limits; its fixed
//! stationary facing; and how fast it aims.
//!
//! The actor's datum and definition are the game's memory, worked on in
//! place ([`crate::ai_actor`]), with the limits' tests here
//! (`actor_look_valid_aim_vector`, `actor_look_valid_look_vector`); a
//! direction's decoding, the idle directions and timers, the weapon, the
//! burst, the action's class, forcing a stop, the unit's business, the
//! stimulus and the unit's exact facing are the game's, through
//! [`AiHost::call`] (actor_looking.c's `actor_look_call`, shared with the C).

use crate::ai_actor::{AiCall, AiHost, AiMemory, AiMemoryExt, AiStepContext, NONE};
use crate::layout::{actor_datum as a, actor_definition as d, actor_idle_looking as idle, ai_values as v, direction_specification};

/// The look update's calls (halo_sim.h's `_actor_look_call_*`).
pub mod call {
    /// `actor_get_weapon(actor) != NONE`, the result.
    pub const HAS_WEAPON: i32 = 1;
    /// `actor_combat_currently_firing_burst(actor)`, the result.
    pub const FIRING_BURST: i32 = 2;
    /// `actor_look_decode_direction` of the direction `a` ([`super::direction`]):
    /// the result, and the vector in `output`.
    pub const DECODE: i32 = 3;
    /// `actor_action_class(actor)`, the result.
    pub const ACTION_CLASS: i32 = 4;
    /// The secondary look's timer expired (a debug message).
    pub const LOOK_TIMER_EXPIRED: i32 = 5;
    /// `actor_move_force_stop(actor)`, the result.
    pub const MOVE_FORCE_STOP: i32 = 6;
    /// `actor_look_idle_timer(actor, its idle looking, a, b)`, the result.
    pub const IDLE_TIMER: i32 = 7;
    /// `actor_look_idle_new_major_direction` (`a`: aiming; `b`: free facing
    /// in bit 0, minor variation in bit 1), from the desired facing (aiming)
    /// or aiming (looking): the result.
    pub const IDLE_NEW_MAJOR: i32 = 8;
    /// `actor_look_idle_new_minor_direction` from the vector in `input`.
    pub const IDLE_NEW_MINOR: i32 = 9;
    /// `unit_is_busy(unit)`, the result.
    pub const UNIT_IS_BUSY: i32 = 10;
    /// `actor_stimulus_abandon_stationary_facing(actor)`.
    pub const ABANDON_STATIONARY_FACING: i32 = 11;
    /// `actor_unit_control_exact_facing(actor, control.face_exactly)`.
    pub const EXACT_FACING: i32 = 12;
}

/// The directions [`call::DECODE`] decodes.
pub mod direction {
    /// The target (a burst's).
    pub const TARGET: i32 = 0;
    pub const PRIMARY: i32 = 1;
    pub const SECONDARY: i32 = 2;
    pub const IDLE_MAJOR: i32 = 3;
    pub const IDLE_MINOR: i32 = 4;
}

const EPSILON: f32 = 0.0001;

/// The local enums (actor_looking.c's).
const IDLE_LOOK_NONE: i16 = 0;
const IDLE_LOOK_GUARDING: i16 = 2;
const IDLE_LOOK_COMBAT: i16 = 4;
const IDLE_TIMER_FACING: i32 = 0;
const IDLE_TIMER_LOOKING: i32 = 2;
const MODE_COMBAT: i16 = 3;

const PRIMARY_NONE: i16 = 0;
const PRIMARY_FACE_360: i16 = 1;
const PRIMARY_OPPORTUNITY_AIMING: i16 = 2;
const PRIMARY_FACING: i16 = 3;
const PRIMARY_EXACT_FACING: i16 = 4;
const PRIMARY_AIMING: i16 = 5;
const PRIMARY_LOCKED_FACING: i16 = 6;
const PRIMARY_LOCKED_AIMING: i16 = 7;

const SECONDARY_NONE: i16 = 0;
const SECONDARY_IDLE_LOOK: i16 = 2;
const SECONDARY_IDLE_AIM: i16 = 3;
const SECONDARY_AIM: i16 = 4;
const SECONDARY_TURN_AND_AIM: i16 = 5;
const SECONDARY_STOP_AND_AIM: i16 = 6;
const SECONDARY_OVERRIDE: i16 = 7;
const SECONDARY_OVERRIDE_EXACT_FACING: i16 = 8;

/// The secondary look types that aim at the alert speed.
const ALERT_SECONDARY_LOOKS: [i16; 5] = [3, 6, 10, 11, 12];

type Vector = [f32; 3];

fn call_full(context: &AiStepContext, host: &impl AiHost, kind: i32, a: i32, b: i32, input: Vector) -> AiCall {
    let mut call = AiCall { kind, actor: context.actor_index, a, b, ..AiCall::default() };
    call.input[..3].copy_from_slice(&input);
    host.call(&mut call);
    call
}

fn call_simple(context: &AiStepContext, host: &impl AiHost, kind: i32, a: i32, b: i32) -> i32 {
    call_full(context, host, kind, a, b, [0.0; 3]).result
}

fn decode(context: &AiStepContext, host: &impl AiHost, which: i32) -> Option<Vector> {
    let call = call_full(context, host, call::DECODE, which, 0, [0.0; 3]);
    (call.result != 0).then_some(call.output)
}

/// `normalize2d`: the magnitude (0 below epsilon, the vector left).
fn normalize2(v: &mut [f32; 2]) -> f32 {
    let magnitude = (v[0] * v[0] + v[1] * v[1]).sqrt();
    if !(EPSILON > magnitude.abs()) {
        let scale = 1.0 / magnitude;
        *v = [scale * v[0], scale * v[1]];
        magnitude
    } else {
        0.0
    }
}

fn dot2(a: [f32; 2], b: [f32; 2]) -> f32 {
    a[0] * b[0] + a[1] * b[1]
}

fn dot3(a: Vector, b: Vector) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn planar(v: Vector) -> [f32; 2] {
    [v[0], v[1]]
}

fn cosine(angle: f32) -> f32 {
    (angle as f64).cos() as f32
}

/// `actor_look_valid_aim_vector`.
fn valid_aim(yaw_deviation: f32, facing: Vector, attempted_aiming: Vector) -> bool {
    let mut aiming = planar(attempted_aiming);
    normalize2(&mut aiming) > 0.0 && dot2(aiming, planar(facing)) > yaw_deviation
}

/// `actor_look_valid_look_vector`.
fn valid_look(yaw_deviation: f32, deltas: [f32; 2], aiming: Vector, attempted_looking: Vector, look: Vector) -> bool {
    let mut looking = planar(look);
    let mut attempted = planar(attempted_looking);
    if normalize2(&mut looking) > 0.0 && dot2(looking, planar(aiming)) > yaw_deviation && normalize2(&mut attempted) > 0.0
    {
        let side = looking[0] * attempted[1] - looking[1] * attempted[0] > 0.0;
        return dot2(attempted, looking) > deltas[side as usize];
    }
    false
}

struct Control<'a> {
    m: &'a dyn AiMemory,
    actor: u32,
}

impl Control<'_> {
    fn vector(&self, offset: usize) -> Vector {
        self.m.vector(self.actor + offset as u32)
    }
    fn set_vector(&self, offset: usize, vector: Vector) {
        for (i, value) in vector.into_iter().enumerate() {
            self.m.set_f32(self.actor + offset as u32 + 4 * i as u32, value);
        }
    }
    fn bool(&self, offset: usize) -> bool {
        self.m.bool(self.actor + offset as u32)
    }
    fn set_bool(&self, offset: usize, value: bool) {
        self.m.set_bool(self.actor + offset as u32, value);
    }
    fn i16(&self, offset: usize) -> i16 {
        self.m.i16(self.actor + offset as u32)
    }
    fn i32(&self, offset: usize) -> i32 {
        self.m.i32(self.actor + offset as u32)
    }
    fn set_i32(&self, offset: usize, value: i32) {
        self.m.w32(self.actor + offset as u32, value as u32);
    }
}

const FACING: usize = a::CONTROL_DESIRED_FACING_VECTOR;
const AIMING: usize = a::CONTROL_DESIRED_AIMING_VECTOR;
const LOOKING: usize = a::CONTROL_DESIRED_LOOKING_VECTOR;

/// `actor_look_update`.
pub fn actor_look_update(context: &AiStepContext, host: &impl AiHost) {
    let m = host.memory();
    let c = Control { m, actor: context.actor };
    let definition = context.definition;
    let mut aiming_at_target = false;
    let movement_type = c.i16(a::OUTPUT_MOVEMENT_TYPE);

    let (can_look, looking_enabled) = if movement_type == v::_ACTOR_MOVEMENT_TYPE_ASLEEP as i16 {
        (false, false)
    } else if c.bool(a::INPUT_VEHICLE_GUNNER) {
        (true, true)
    } else if movement_type == v::_ACTOR_MOVEMENT_TYPE_ALERT as i16
        || movement_type == v::_ACTOR_MOVEMENT_TYPE_COMBAT as i16
    {
        (call_simple(context, host, call::HAS_WEAPON, 0, 0) != 0, true)
    } else {
        (false, true)
    };

    if !looking_enabled {
        c.set_vector(AIMING, c.vector(FACING));
        c.set_vector(LOOKING, c.vector(FACING));
    } else {
        aiming_at_target = look(context, host, &c, can_look);
    }

    // update_facing:
    if !c.bool(a::STATE_FLYING) {
        let mut facing = c.vector(FACING);
        if !((facing[2] - 0.0).abs() < EPSILON) {
            facing[2] = 0.0;
            let mut facing2 = planar(facing);
            let magnitude = normalize2(&mut facing2);
            c.set_vector(FACING, [facing2[0], facing2[1], 0.0]);
            if magnitude == 0.0 {
                c.set_vector(FACING, c.vector(a::INPUT_FACING_VECTOR));
            }
        }
    }

    if c.bool(a::CONTROL_DESIRE_STATIONARY_FACING) {
        if !c.bool(a::CONTROL_FIXED_STATIONARY_FACING) {
            if !c.bool(a::CONTROL_MOVING) && dot3(c.vector(a::INPUT_AIMING_VECTOR), c.vector(AIMING)) > 0.9 {
                c.set_bool(a::CONTROL_FIXED_STATIONARY_FACING, true);
                c.set_vector(a::CONTROL_FIXED_STATIONARY_FACING_VECTOR, c.vector(FACING));
            }
        } else {
            let angle = m.f32(definition + d::DEFENSIVE_STATIONARY_FACING_ANGLE as u32);
            if angle > 0.0 {
                let stationary_cosine = cosine(angle);
                let fixed = c.vector(a::CONTROL_FIXED_STATIONARY_FACING_VECTOR);
                let valid = if c.bool(a::STATE_FLYING) {
                    dot3(c.vector(FACING), fixed) > stationary_cosine && dot3(c.vector(AIMING), fixed) > stationary_cosine
                } else {
                    let mut desired_facing = planar(c.vector(FACING));
                    let mut desired_aiming = planar(c.vector(AIMING));
                    let mut stationary_facing = planar(fixed);
                    normalize2(&mut desired_facing) != 0.0
                        && normalize2(&mut desired_aiming) != 0.0
                        && normalize2(&mut stationary_facing) != 0.0
                        && dot2(stationary_facing, desired_facing) > stationary_cosine
                        && dot2(desired_aiming, stationary_facing) > stationary_cosine
                };
                if !valid {
                    c.set_bool(a::CONTROL_FIXED_STATIONARY_FACING, false);
                    call_simple(context, host, call::ABANDON_STATIONARY_FACING, 0, 0);
                }
            }
        }
    } else {
        c.set_bool(a::CONTROL_FIXED_STATIONARY_FACING, false);
    }

    c.set_vector(a::OUTPUT_FACING_VECTOR, c.vector(FACING));
    c.set_vector(a::OUTPUT_AIMING_VECTOR, c.vector(AIMING));
    c.set_vector(a::OUTPUT_LOOKING_VECTOR, c.vector(LOOKING));
    call_simple(context, host, call::EXACT_FACING, 0, 0);

    let alert = aiming_at_target
        || c.i16(a::ORDERS_LOOK_IDLE_LOOK_TYPE) == IDLE_LOOK_COMBAT
        || ALERT_SECONDARY_LOOKS.contains(&c.i16(a::CONTROL_SECONDARY_LOOK_TYPE));
    let speed = if alert { v::_UNIT_AIMING_SPEED_ALERT } else { v::_UNIT_AIMING_SPEED_CASUAL };
    m.set_i16(context.actor + a::OUTPUT_AIMING_SPEED as u32, speed as i16);
}

/// The looking itself, when enabled: whether the actor aims at its target.
fn look(context: &AiStepContext, host: &impl AiHost, c: &Control, can_look: bool) -> bool {
    let m = c.m;
    let definition = context.definition;
    let mut aiming_at_target = false;
    let mut facing_free = c.bool(a::CONTROL_FREE_FACING_VECTOR);
    let mut aiming_free = true;
    let mut looking_free = can_look;
    let mut facing_optional = c.bool(a::CONTROL_OPTIONAL_FACING_VECTOR);
    let mut facing_locked = false;
    let mut looking_locked = false;
    let aiming_cosine = m.f32(definition + d::LOOKING_RUNTIME_MAXIMUM_AIMING_DEVIATION_COSINE_YAW as u32);
    let looking_cosine = m.f32(definition + d::LOOKING_RUNTIME_MAXIMUM_LOOKING_DEVIATION_COSINE_YAW as u32);
    let angles = if c.i16(a::STATE_MODE) == MODE_COMBAT {
        d::LOOKING_COMBAT_LOOKING_DELTA_ANGLES
    } else {
        d::LOOKING_NONCOMBAT_LOOKING_DELTA_ANGLES
    } as u32;
    let deltas = [cosine(m.f32(definition + angles)), cosine(m.f32(definition + angles + 4))];
    let mut primary_vector = [0.0; 3];
    let mut secondary_vector = [0.0; 3];
    let mut primary_priority = PRIMARY_NONE;
    let mut burst_direction = false;

    if call_simple(context, host, call::FIRING_BURST, 0, 0) != 0 && !c.bool(a::ORDERS_COMBAT_ABORT_BURST) {
        if let Some(vector) = decode(context, host, direction::TARGET) {
            primary_vector = vector;
            primary_priority = PRIMARY_LOCKED_AIMING;
            aiming_at_target = true;
            burst_direction = true;
        }
    }
    if !burst_direction {
        primary_priority = c.i16(a::ORDERS_LOOK_PRIMARY_PRIORITY);
        if primary_priority != PRIMARY_NONE && primary_priority != PRIMARY_FACE_360 {
            if let Some(vector) = decode(context, host, direction::PRIMARY) {
                primary_vector = vector;
                aiming_at_target =
                    c.i16(a::ORDERS_LOOK_PRIMARY_DIRECTION_TYPE) == v::_DIRECTION_SPECIFICATION_TARGET as i16;
            } else {
                primary_priority = PRIMARY_NONE;
            }
        }
    }

    let mut secondary_priority = SECONDARY_NONE;
    if c.i16(a::CONTROL_SECONDARY_LOOK_TYPE) >= v::_SECONDARY_LOOK_NONE as i16
        && c.i16(a::CONTROL_SECONDARY_LOOK_TIMER) > 0
        && let Some(vector) = decode(context, host, direction::SECONDARY)
    {
        secondary_vector = vector;
        secondary_priority = c.i16(a::CONTROL_SECONDARY_LOOK_PRIORITY);
    }
    if c.bool(a::CONTROL_MOVING)
        && call_simple(context, host, call::ACTION_CLASS, 0, 0) as i16 == v::_ACTION_CLASS_TRANSITORY as i16
    {
        secondary_priority = secondary_priority.min(SECONDARY_TURN_AND_AIM);
    }

    let timer = context.actor + a::CONTROL_SECONDARY_LOOK_TIMER as u32;
    if m.i16(timer) > 0 {
        m.set_i16(timer, m.i16(timer) - 1);
        if m.i16(timer) == 0 {
            call_simple(context, host, call::LOOK_TIMER_EXPIRED, 0, 0);
            m.set_i16(context.actor + a::CONTROL_SECONDARY_LOOK_TYPE as u32, v::_SECONDARY_LOOK_NONE as i16);
            m.set_i16(context.actor + a::CONTROL_SECONDARY_LOOK_PRIORITY as u32, SECONDARY_NONE);
        }
    }
    c.set_bool(a::CONTROL_AIMING_AWAY_FROM_PRIMARY, false);

    if primary_priority >= PRIMARY_OPPORTUNITY_AIMING {
        let mut primary_accepted = false;
        if primary_priority >= PRIMARY_AIMING
            && (facing_free || valid_aim(aiming_cosine, c.vector(FACING), primary_vector))
        {
            primary_accepted = true;
        } else if primary_priority >= PRIMARY_FACING && facing_optional {
            facing_optional = false;
            facing_free = true;
            primary_accepted = true;
        }
        if primary_accepted {
            c.set_vector(AIMING, primary_vector);
            aiming_free = false;
            looking_free = can_look;
            if primary_priority >= PRIMARY_LOCKED_AIMING {
                looking_locked = true;
                if can_look {
                    c.set_vector(LOOKING, primary_vector);
                }
            }
        }
        if primary_priority == PRIMARY_OPPORTUNITY_AIMING {
            primary_priority = if facing_free { PRIMARY_AIMING } else { PRIMARY_NONE };
        }
        if facing_free {
            facing_free = false;
            facing_optional = false;
            c.set_vector(FACING, primary_vector);
            let face_exactly = m.r8(context.actor + a::CONTROL_FACE_EXACTLY as u32)
                | (primary_priority == PRIMARY_EXACT_FACING) as u8;
            m.w8(context.actor + a::CONTROL_FACE_EXACTLY as u32, face_exactly);
        }
        facing_locked = (!c.bool(a::CONTROL_FREE_FACING_VECTOR) && !c.bool(a::CONTROL_OPTIONAL_FACING_VECTOR))
            || primary_priority >= PRIMARY_LOCKED_FACING;
    }

    match secondary_priority {
        SECONDARY_IDLE_LOOK | SECONDARY_IDLE_AIM | SECONDARY_AIM | SECONDARY_TURN_AND_AIM | SECONDARY_STOP_AND_AIM => {
            let secondary_aim_valid = valid_aim(aiming_cosine, c.vector(FACING), secondary_vector);
            if !facing_locked
                && !looking_locked
                && ((secondary_priority >= SECONDARY_STOP_AND_AIM
                    && call_simple(context, host, call::MOVE_FORCE_STOP, 0, 0) != 0)
                    || (secondary_priority >= SECONDARY_TURN_AND_AIM
                        && (facing_optional || c.bool(a::CONTROL_FREE_FACING_VECTOR)))
                    || (secondary_priority >= SECONDARY_AIM && !secondary_aim_valid && facing_free && aiming_free))
            {
                if !c.bool(a::CONTROL_FACE_EXACTLY) || !secondary_aim_valid {
                    c.set_vector(FACING, secondary_vector);
                    c.set_bool(a::CONTROL_FACE_EXACTLY, false);
                }
                c.set_vector(AIMING, secondary_vector);
                c.set_vector(LOOKING, secondary_vector);
                looking_free = can_look;
                facing_free = false;
                c.set_bool(a::CONTROL_AIMING_AWAY_FROM_PRIMARY, true);
                looking_locked = false;
                aiming_free = false;
            } else if !looking_locked
                && secondary_aim_valid
                && (secondary_priority >= SECONDARY_TURN_AND_AIM
                    || (secondary_priority >= SECONDARY_IDLE_AIM && aiming_free))
            {
                c.set_vector(AIMING, secondary_vector);
                c.set_vector(LOOKING, secondary_vector);
                looking_free = can_look;
                c.set_bool(a::CONTROL_AIMING_AWAY_FROM_PRIMARY, true);
                looking_locked = false;
                aiming_free = false;
            } else if can_look
                && valid_look(looking_cosine, deltas, c.vector(FACING), c.vector(AIMING), secondary_vector)
            {
                c.set_vector(LOOKING, secondary_vector);
                looking_free = false;
            } else if aiming_free && secondary_aim_valid {
                c.set_vector(AIMING, secondary_vector);
                c.set_vector(LOOKING, secondary_vector);
                looking_free = true;
                aiming_free = false;
            }
        }
        SECONDARY_OVERRIDE | SECONDARY_OVERRIDE_EXACT_FACING => {
            let mut succeeded = false;
            let mut change_facing = secondary_priority == SECONDARY_OVERRIDE_EXACT_FACING;
            let mut face_exactly = secondary_priority == SECONDARY_OVERRIDE_EXACT_FACING;
            if c.bool(a::CONTROL_FREE_FACING_VECTOR) {
                change_facing = true;
                succeeded = true;
            } else if valid_aim(aiming_cosine, c.vector(FACING), secondary_vector) {
                succeeded = true;
            } else if call_simple(context, host, call::MOVE_FORCE_STOP, 0, 0) != 0 {
                face_exactly = true;
                change_facing = true;
                succeeded = true;
            }
            if succeeded {
                if change_facing {
                    c.set_vector(FACING, secondary_vector);
                    c.set_bool(a::CONTROL_FACE_EXACTLY, face_exactly);
                }
                c.set_vector(AIMING, secondary_vector);
                c.set_vector(LOOKING, secondary_vector);
                looking_free = can_look;
                facing_free = false;
                c.set_bool(a::CONTROL_AIMING_AWAY_FROM_PRIMARY, true);
                looking_locked = false;
                aiming_free = false;
            }
        }
        _ => {}
    }

    if primary_priority == PRIMARY_OPPORTUNITY_AIMING
        && aiming_free
        && valid_aim(aiming_cosine, c.vector(FACING), primary_vector)
    {
        c.set_vector(AIMING, primary_vector);
        if looking_free {
            c.set_vector(LOOKING, primary_vector);
        }
        c.set_bool(a::CONTROL_AIMING_AWAY_FROM_PRIMARY, false);
        aiming_free = false;
    }

    idle_look(
        context,
        host,
        c,
        IdleState { facing_free, aiming_free, looking_free, looking_locked, primary_priority },
        aiming_cosine,
        looking_cosine,
        deltas,
    );

    if !c.bool(a::CONTROL_MOVING)
        && !c.bool(a::CONTROL_MOVING_FORCED_BY_AIMING)
        && call_simple(context, host, call::UNIT_IS_BUSY, 0, 0) == 0
        && c.i32(a::INPUT_VEHICLE_INDEX) == NONE
    {
        let facing = c.vector(FACING);
        let aiming = c.vector(AIMING);
        let looking = c.vector(LOOKING);
        let input_facing = c.vector(a::INPUT_FACING_VECTOR);
        if (valid_aim(aiming_cosine, facing, aiming) && !valid_aim(aiming_cosine, input_facing, aiming))
            || (can_look
                && valid_look(looking_cosine, deltas, facing, aiming, looking)
                && !valid_look(looking_cosine, deltas, input_facing, aiming, looking))
        {
            c.set_bool(a::CONTROL_FACE_EXACTLY, true);
        }
    }
    if !can_look {
        c.set_vector(LOOKING, c.vector(AIMING));
    }
    aiming_at_target
}

struct IdleState {
    facing_free: bool,
    aiming_free: bool,
    looking_free: bool,
    looking_locked: bool,
    primary_priority: i16,
}

/// The idle looking: a major direction to face, aim or look in (and when
/// to choose another), and minor variations on it.
fn idle_look(
    context: &AiStepContext,
    host: &impl AiHost,
    c: &Control,
    state: IdleState,
    aiming_cosine: f32,
    looking_cosine: f32,
    deltas: [f32; 2],
) {
    let m = c.m;
    let IdleState { facing_free, aiming_free, looking_free, looking_locked, primary_priority } = state;
    let idle_look_type = c.i16(a::ORDERS_LOOK_IDLE_LOOK_TYPE);
    let looking_definition = context.definition
        + if idle_look_type != IDLE_LOOK_GUARDING {
            if idle_look_type <= IDLE_LOOK_GUARDING || idle_look_type > IDLE_LOOK_COMBAT {
                d::LOOKING_IDLE_LOOK_NONCOMBAT
            } else {
                d::LOOKING_IDLE_LOOK_SEARCH_COMBAT
            }
        } else {
            d::LOOKING_IDLE_LOOK_GUARD
        } as u32;
    let idle_facing = m.f32(looking_definition + idle::IDLE_FACING_TIME_UPPER_BOUND as u32) > 0.0;
    let idle_aiming = m.f32(looking_definition + idle::IDLE_AIM_TIME_UPPER_BOUND as u32) > 0.0;
    let idle_looking = m.f32(looking_definition + idle::IDLE_LOOK_TIME_UPPER_BOUND as u32) > 0.0;

    if !(idle_look_type > IDLE_LOOK_NONE
        && !looking_locked
        && (aiming_free || looking_free)
        && (idle_facing || idle_aiming || idle_looking))
    {
        c.set_bool(a::CONTROL_IDLE_MAJOR_ACTIVE, false);
        c.set_bool(a::CONTROL_IDLE_MAJOR_DIRECTION_IS_INTERESTING, false);
        c.set_bool(a::CONTROL_IDLE_MINOR_ACTIVE, false);
        return;
    }

    let mut new_major = false;
    let mut aiming_variation = false;
    let mut major_valid = false;
    let mut free_facing = idle_facing
        && facing_free
        && primary_priority == PRIMARY_FACE_360
        && c.i32(a::CONTROL_IDLE_FACING_TIMER) == 0;
    let mut idle_major_vector = [0.0; 3];

    if c.i32(a::CONTROL_IDLE_FACING_TIMER) > 0 {
        c.set_i32(a::CONTROL_IDLE_FACING_TIMER, c.i32(a::CONTROL_IDLE_FACING_TIMER) - 1);
    }
    let major_direction = a::CONTROL_IDLE_MAJOR_DIRECTION;
    if c.bool(a::CONTROL_IDLE_MAJOR_ACTIVE) && c.bool(a::CONTROL_IDLE_MAJOR_IS_AIMING) && !aiming_free {
        c.set_bool(a::CONTROL_IDLE_MAJOR_ACTIVE, true);
        let timer = call_simple(context, host, call::IDLE_TIMER, IDLE_TIMER_LOOKING, 1);
        c.set_i32(a::CONTROL_IDLE_MAJOR_TIMER, timer);
        m.set_i16(context.actor + major_direction as u32, v::_DIRECTION_SPECIFICATION_VECTOR as i16);
        c.set_vector(major_direction + direction_specification::VECTOR, c.vector(AIMING));
    }

    if !c.bool(a::CONTROL_IDLE_MAJOR_ACTIVE) || c.i32(a::CONTROL_IDLE_MAJOR_TIMER) == 0 {
        let mut major_is_aiming = true;
        let mut major_possible = aiming_free && idle_aiming;
        if !major_possible {
            major_is_aiming = false;
            major_possible = looking_free && idle_looking;
        }
        if major_possible {
            let minor_variation = major_is_aiming && looking_free && idle_looking;
            let interesting = call_simple(
                context,
                host,
                call::IDLE_NEW_MAJOR,
                major_is_aiming as i32,
                free_facing as i32 | (minor_variation as i32) << 1,
            );
            m.w8(context.actor + a::CONTROL_IDLE_MAJOR_DIRECTION_IS_INTERESTING as u32, interesting as u8);
            new_major = true;
        }
    }

    if c.bool(a::CONTROL_IDLE_MAJOR_ACTIVE) {
        c.set_i32(a::CONTROL_IDLE_MAJOR_TIMER, c.i32(a::CONTROL_IDLE_MAJOR_TIMER).wrapping_sub(1));
        if let Some(vector) = decode(context, host, direction::IDLE_MAJOR) {
            idle_major_vector = vector;
            if aiming_free {
                if facing_free && idle_facing && c.bool(a::STATE_FLYING) {
                    free_facing = true;
                    aiming_variation = true;
                }
                if free_facing {
                    c.set_vector(FACING, vector);
                    c.set_vector(AIMING, vector);
                    c.set_bool(a::CONTROL_AIMING_AWAY_FROM_PRIMARY, true);
                    major_valid = true;
                } else if valid_aim(aiming_cosine, c.vector(FACING), vector) {
                    c.set_vector(AIMING, vector);
                    c.set_bool(a::CONTROL_AIMING_AWAY_FROM_PRIMARY, true);
                    major_valid = true;
                }
            } else if valid_look(looking_cosine, deltas, c.vector(FACING), c.vector(AIMING), vector) {
                c.set_vector(LOOKING, vector);
                major_valid = true;
            }
        }
    }

    if major_valid {
        if new_major && idle_looking {
            c.set_bool(a::CONTROL_IDLE_MINOR_ACTIVE, true);
            let interesting = c.bool(a::CONTROL_IDLE_MAJOR_DIRECTION_IS_INTERESTING) as i32;
            let timer = call_simple(context, host, call::IDLE_TIMER, IDLE_TIMER_LOOKING, interesting);
            c.set_i32(a::CONTROL_IDLE_MINOR_TIMER, timer);
            for offset in 0..direction_specification::SIZEOF as u32 {
                let byte = m.r8(context.actor + major_direction as u32 + offset);
                m.w8(context.actor + a::CONTROL_IDLE_MINOR_DIRECTION as u32 + offset, byte);
            }
            if free_facing {
                let timer = call_simple(context, host, call::IDLE_TIMER, IDLE_TIMER_FACING, interesting);
                c.set_i32(a::CONTROL_IDLE_FACING_TIMER, timer);
            }
        }
    } else {
        c.set_bool(a::CONTROL_IDLE_MAJOR_ACTIVE, false);
        idle_major_vector = c.vector(AIMING);
    }

    if aiming_free && ((looking_free && idle_looking) || (aiming_variation && idle_aiming)) {
        if c.i32(a::CONTROL_IDLE_MINOR_TIMER) == 0 {
            call_full(context, host, call::IDLE_NEW_MINOR, 0, 0, idle_major_vector);
        }
        c.set_i32(a::CONTROL_IDLE_MINOR_TIMER, c.i32(a::CONTROL_IDLE_MINOR_TIMER).wrapping_sub(1));
        if c.bool(a::CONTROL_IDLE_MINOR_ACTIVE) {
            let minor = decode(context, host, direction::IDLE_MINOR).filter(|&vector| {
                if aiming_variation {
                    valid_aim(aiming_cosine, c.vector(FACING), vector)
                } else {
                    valid_look(looking_cosine, deltas, c.vector(FACING), c.vector(AIMING), vector)
                }
            });
            if let Some(vector) = minor {
                if aiming_variation {
                    c.set_vector(AIMING, vector);
                }
                c.set_vector(LOOKING, vector);
            } else {
                c.set_bool(a::CONTROL_IDLE_MINOR_ACTIVE, false);
            }
        }
    } else {
        c.set_bool(a::CONTROL_IDLE_MINOR_ACTIVE, false);
    }
}
