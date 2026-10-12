//! An actor's movement (actor_moving.c's `actor_move_update`, every tick of
//! an actor's update that controls it): its avoidance while flying a
//! vehicle, its movement type, its crouch, how its vehicle (driving,
//! hovering, flying, stuck) or its state (riding, animating, asleep, in
//! midair, throwing a grenade, keeping out of a player's fire) holds it
//! still or frees it, its facing when still, its berserk shout and its
//! jumps and leaps.
//!
//! The actor's datum, definition and vehicle are the game's memory, worked
//! on in place ([`crate::ai_actor`]); the vector avoidance, the movement's
//! calculation, the vehicle's definition and stuckness, the path, the
//! firing positions, the unit's crouch, jump and leap, its animation and
//! the shouts are the game's, through [`AiHost::call`] (actor_moving.c's
//! `actor_move_call`, shared with the C).

use crate::ai_actor::{AiCall, AiHost, AiMemory, AiMemoryExt, AiStepContext, NONE, object_get, prop_get};
use crate::layout::{actor_animation_orders, actor_datum as a, actor_definition as d, ai_values as v, prop_datum as p, vehicle_datum};

/// The move update's calls (halo_sim.h's `_actor_move_call_*`).
pub mod call {
    /// `actor_move_vector_avoidance` along the movement (`a`: the facing
    /// three units ahead, not moving): the rotation in `output[0..3]`, the
    /// emergency in `output[3]`.
    pub const VECTOR_AVOIDANCE: i32 = 1;
    /// `actor_path_has_path(actor)`, the result.
    pub const HAS_PATH: i32 = 2;
    /// The vehicle `a`'s definition: its steering angle, throttle, oversteer
    /// bounds and sideslip distance in `output[0..5]`.
    pub const VEHICLE_DEFINITION: i32 = 3;
    /// `vehicle_stuck(a)`: the result, its direction in `output`.
    pub const VEHICLE_STUCK: i32 = 4;
    /// `actor_move_calculate_movement` into the actor (`a`: in 3d, free
    /// movement, all moving turns in bits 0-2; `b`: the facing override;
    /// `input`: the free distance squared, steering angle, oversteer bounds,
    /// rotation emergency and throttle).
    pub const CALCULATE_MOVEMENT: i32 = 5;
    /// `actor_clear_discarded_firing_positions(actor, TRUE)`.
    pub const CLEAR_DISCARDED_FIRING_POSITIONS: i32 = 6;
    /// Whether a local game's debug forces a crouch, the result.
    pub const FORCE_CROUCH: i32 = 7;
    /// `actor_unit_control_crouch(actor, a)`.
    pub const UNIT_CONTROL_CROUCH: i32 = 8;
    /// `actor_move_animation_busy(actor)`, the result.
    pub const ANIMATION_BUSY: i32 = 9;
    /// The berserk impulse along `input` and its shout (the cause `a`).
    pub const BERSERK: i32 = 10;
    /// `actor_unit_control_jump(actor)`.
    pub const UNIT_CONTROL_JUMP: i32 = 11;
    /// `unit_leap_begin(unit, input)`, the result.
    pub const LEAP_BEGIN: i32 = 12;
    /// The leap's shout.
    pub const LEAP_SHOUT: i32 = 13;
}

const EPSILON: f32 = 0.0001;

/// The local enums (actor_moving.c's).
const MODE_ASLEEP: i16 = 1;
const MODE_ALERT: i16 = 2;
const MODE_COMBAT: i16 = 3;

type Vector = [f32; 3];

fn call_full(context: &AiStepContext, host: &impl AiHost, kind: i32, a: i32, b: i32, input: &[f32]) -> AiCall {
    let mut call = AiCall { kind, actor: context.actor_index, a, b, ..AiCall::default() };
    call.input[..input.len()].copy_from_slice(input);
    host.call(&mut call);
    call
}

fn call_simple(context: &AiStepContext, host: &impl AiHost, kind: i32, a: i32) -> i32 {
    call_full(context, host, kind, a, 0, &[]).result
}

fn magnitude_squared(v: Vector) -> f32 {
    v[0] * v[0] + v[1] * v[1] + v[2] * v[2]
}

fn scale(v: Vector, s: f32) -> Vector {
    [s * v[0], s * v[1], s * v[2]]
}

/// `normalize3d`.
fn normalize3(v: &mut Vector) -> f32 {
    let magnitude = magnitude_squared(*v).sqrt();
    if !(EPSILON > magnitude.abs()) {
        *v = scale(*v, 1.0 / magnitude);
        magnitude
    } else {
        0.0
    }
}

/// `normalize2d`.
fn normalize2(v: &mut [f32; 2]) -> f32 {
    let magnitude = (v[0] * v[0] + v[1] * v[1]).sqrt();
    if !(EPSILON > magnitude.abs()) {
        let s = 1.0 / magnitude;
        *v = [s * v[0], s * v[1]];
        magnitude
    } else {
        0.0
    }
}

/// `rotate_vector_about_axis`.
fn rotate_about_axis(v: Vector, n: Vector, sine: f32, cosine: f32) -> Vector {
    let one_minus_cosine_times_v_dot_n = (n[0] * v[0] + n[1] * v[1] + n[2] * v[2]) * (1.0 - cosine);
    let cross_i = n[2] * v[1] - v[2] * n[1];
    let cross_j = n[0] * v[2] - v[0] * n[2];
    let cross_k = n[1] * v[0] - v[1] * n[0];
    [
        cosine * v[0] + one_minus_cosine_times_v_dot_n * n[0] - sine * cross_i,
        cosine * v[1] + one_minus_cosine_times_v_dot_n * n[1] - sine * cross_j,
        cosine * v[2] + one_minus_cosine_times_v_dot_n * n[2] - sine * cross_k,
    ]
}

fn set_vector(m: &dyn AiMemory, address: u32, vector: Vector) {
    for (i, value) in vector.into_iter().enumerate() {
        m.set_f32(address + 4 * i as u32, value);
    }
}

fn copy(m: &dyn AiMemory, to: u32, from: u32, size: u32) {
    for offset in 0..size {
        m.w8(to + offset, m.r8(from + offset));
    }
}

fn test_flag(flags: u32, bit: usize) -> bool {
    flags & (1 << bit) != 0
}

/// `actor_move_update`.
pub fn actor_move_update(context: &AiStepContext, host: &impl AiHost) {
    let m = host.memory();
    let actor = context.actor;
    let definition = context.definition;
    let at = |offset: usize| actor + offset as u32;
    let flags = m.r32(definition + d::FLAGS as u32);
    let mut free_movement = false;
    let mut move_in_3d = false;
    let mut allow_all_moving_turns = false;
    let mut clear_firing_positions = false;
    let mut force_stationary_facing = false;
    let mut allow_jump = false;
    let mut free_movement_distance_squared = 0.0f32;
    let mut steering_maximum_angle = 0.0f32;
    let mut oversteer_maximum_angle = 0.0f32;
    let mut oversteer_minimum_angle = 0.0f32;
    let mut rotation_emergency_amount = 0.0f32;
    let mut maximum_throttle = 1.0f32;

    let facing_vector = m.vector(at(a::INPUT_FACING_VECTOR));
    let desired_facing = at(a::CONTROL_DESIRED_FACING_VECTOR);
    let moving = at(a::CONTROL_MOVING);
    let free_facing = at(a::CONTROL_FREE_FACING_VECTOR);
    let optional_facing = at(a::CONTROL_OPTIONAL_FACING_VECTOR);
    let towards = at(a::CONTROL_MOVING_TOWARDS_VECTOR);
    let rotation = at(a::CONTROL_VECTOR_AVOIDANCE_ROTATION);
    let emergency = at(a::CONTROL_VECTOR_AVOIDANCE_ROTATION_EMERGENCY);
    let emergency_instantaneous = at(a::CONTROL_VECTOR_AVOIDANCE_ROTATION_EMERGENCY_INSTANTANEOUS);
    let moving_facing_direction = at(a::CONTROL_MOVING_FACING_DIRECTION);
    let driver_type = m.i16(at(a::INPUT_VEHICLE_DRIVER_TYPE));

    set_vector(m, desired_facing, facing_vector);
    m.set_bool(at(a::CONTROL_FACE_EXACTLY), false);
    m.set_bool(free_facing, true);
    m.set_bool(optional_facing, true);

    if m.bool(at(a::ORDERS_MOVE_OVERRIDE_MOVEMENT_DIRECTION)) {
        m.set_bool(moving, true);
        m.set_bool(free_facing, false);
        copy(m, towards, at(a::ORDERS_MOVE_OVERRIDE_MOVEMENT_DIRECTION_VECTOR), 12);
        set_vector(m, rotation, [0.0; 3]);
        m.set_f32(emergency, 0.0);
        m.set_f32(emergency_instantaneous, 0.0);
    } else if driver_type == v::_ACTOR_VEHICLE_DRIVER_DIRECTIONAL_FLYING as i16 {
        let avoidance = call_full(context, host, call::VECTOR_AVOIDANCE, !m.bool(moving) as i32, 0, &[]);
        let instantaneous = avoidance.vector();
        let emergency_amount = avoidance.output[3];
        let blend = if magnitude_squared(instantaneous) > magnitude_squared(m.vector(rotation)) { 0.3 } else { 0.05 };
        let scaled = scale(m.vector(rotation), 1.0 - blend);
        let mut blended = [
            scaled[0] + instantaneous[0] * blend,
            scaled[1] + instantaneous[1] * blend,
            scaled[2] + instantaneous[2] * blend,
        ];
        if magnitude_squared(blended) < EPSILON {
            blended = [0.0; 3];
        }
        set_vector(m, rotation, blended);
        m.set_f32(emergency_instantaneous, emergency_amount);
        let mut settled = m.f32(emergency) * (1.0 - blend) + emergency_amount * blend;
        if settled < 0.001 {
            settled = 0.0;
        }
        m.set_f32(emergency, settled);

        if m.bool(moving) {
            let mut rotation_vector = m.vector(rotation);
            let angle_squared = magnitude_squared(rotation_vector);
            if angle_squared > EPSILON {
                let angle = angle_squared.sqrt();
                let inverse_angle = 1.0 / angle;
                rotation_vector = scale(rotation_vector, inverse_angle);
                let rotated = rotate_about_axis(
                    m.vector(towards),
                    rotation_vector,
                    (angle as f64).sin() as f32,
                    (angle as f64).cos() as f32,
                );
                set_vector(m, towards, rotated);
            }
            rotation_emergency_amount = m.f32(emergency);
        }
    }

    let mut movement_type = m.i16(at(a::ORDERS_MOVE_OVERRIDE_MOVEMENT_TYPE));
    if movement_type == NONE as i16 {
        movement_type = v::_ACTOR_MOVEMENT_TYPE_COMBAT as i16;
        if m.bool(at(a::ORDERS_MOVE_FLAMING)) {
            movement_type = v::_ACTOR_MOVEMENT_TYPE_FLAMING as i16;
        } else if m.bool(at(a::ORDERS_MOVE_PANICKED)) {
            movement_type = v::_ACTOR_MOVEMENT_TYPE_PANIC as i16;
        } else {
            match m.i16(at(a::STATE_MODE)) {
                MODE_ASLEEP => movement_type = v::_ACTOR_MOVEMENT_TYPE_ASLEEP as i16,
                MODE_ALERT => movement_type = v::_ACTOR_MOVEMENT_TYPE_ALERT as i16,
                MODE_COMBAT => movement_type = v::_ACTOR_MOVEMENT_TYPE_COMBAT as i16,
                _ => {}
            }
        }
    }
    m.set_i16(at(a::OUTPUT_MOVEMENT_TYPE), movement_type);

    let mut override_facing = m.i16(at(a::ORDERS_MOVE_OVERRIDE_MOVEMENT_FACING));
    let mut crouch = if call_simple(context, host, call::HAS_PATH, 0) != 0
        && m.f32(at(a::CONTROL_PATH_DESTINATION_ORIGINAL_DISTANCE))
            >= m.f32(definition + d::MOVING_STATIONARY_MOVING_DISTANCE as u32)
    {
        m.bool(at(a::ORDERS_MOVE_MOVING_CROUCH))
    } else {
        m.bool(at(a::ORDERS_MOVE_STATIONARY_CROUCH))
    };

    let still_facing_free = |m: &dyn AiMemory| {
        m.i16(at(a::META_TYPE)) == v::_ACTOR_MOUNTED_WEAPON as i16 || m.bool(at(a::INPUT_VEHICLE_GUNNER))
    };
    if driver_type > v::_ACTOR_VEHICLE_DRIVER_NONE as i16 {
        let vehicle_index = m.i32(at(a::INPUT_VEHICLE_INDEX));
        let steering = call_simple_output(context, host, call::VEHICLE_DEFINITION, vehicle_index);
        steering_maximum_angle = steering[0];
        if steering[1] > 0.0 {
            maximum_throttle = steering[1];
        }
        oversteer_minimum_angle = steering[2];
        oversteer_maximum_angle = steering[3];
        let sideslip_distance = steering[4];
        let vehicle = object_get(m, context, vehicle_index).unwrap_or(0);

        let mut nondirectional = false;
        if driver_type == v::_ACTOR_VEHICLE_DRIVER_HOVERING_GROUND as i16 {
            if m.r8(vehicle + vehicle_datum::VEHICLE_AIRBORNE_TICKS as u32) > 0 {
                allow_jump = true;
                m.set_bool(moving, false);
                m.set_bool(free_facing, true);
                crouch = false;
            } else if m.f32(vehicle + vehicle_datum::VEHICLE_HOVER as u32) < 0.7
                && {
                    allow_jump = true;
                    m.f32(vehicle + vehicle_datum::OBJECT_UP as u32 + 8) < 0.8
                }
            {
                let mut movement_vector = m.vector(vehicle + vehicle_datum::OBJECT_UP as u32);
                movement_vector[2] = 0.0;
                if normalize3(&mut movement_vector) > 0.0 {
                    m.set_bool(moving, true);
                    set_vector(m, towards, scale(movement_vector, 3.0));
                } else {
                    m.set_bool(moving, false);
                }
                crouch = false;
            } else {
                nondirectional = true;
            }
        } else if driver_type == v::_ACTOR_VEHICLE_DRIVER_NONDIRECTIONAL_GROUND as i16 {
            nondirectional = true;
        } else if driver_type == v::_ACTOR_VEHICLE_DRIVER_DIRECTIONAL_FLYING as i16 {
            let stuck = call_full(context, host, call::VEHICLE_STUCK, vehicle_index, 0, &[]);
            if stuck.result != 0 {
                let direction = stuck.vector();
                m.set_bool(moving, false);
                m.set_bool(free_facing, false);
                m.set_bool(optional_facing, false);
                set_vector(m, desired_facing, [-direction[0], -direction[1], -direction[2]]);
            } else {
                override_facing = 0;
                move_in_3d = true;
                allow_all_moving_turns = true;
            }
            crouch = false;
        } else {
            m.set_bool(moving, false);
            m.set_i16(moving_facing_direction, 0);
            m.set_bool(free_facing, still_facing_free(m));
            crouch = false;
        }
        if nondirectional {
            free_movement = true;
            free_movement_distance_squared = sideslip_distance * sideslip_distance;
            crouch = false;
        }
    } else if m.bool(at(a::INPUT_VEHICLE_PASSENGER)) {
        m.set_bool(moving, false);
        m.set_i16(moving_facing_direction, 0);
        m.set_bool(free_facing, still_facing_free(m));
        m.set_bool(optional_facing, false);
        crouch = false;
    } else if m.i16(at(a::ORDERS_MOVE_ANIMATION_IMPULSE)) != NONE as i16 {
        m.set_bool(moving, false);
        m.set_bool(free_facing, false);
        m.set_bool(optional_facing, false);
        crouch = false;
    } else if movement_type == v::_ACTOR_MOVEMENT_TYPE_ASLEEP as i16 {
        m.set_bool(moving, false);
        m.set_bool(free_facing, false);
        m.set_bool(optional_facing, false);
        force_stationary_facing = true;
        crouch = false;
    } else if m.bool(at(a::INPUT_IN_MIDAIR)) && !m.bool(at(a::STATE_FLYING)) {
        m.set_bool(moving, false);
        m.set_bool(free_facing, true);
        crouch = false;
    } else if m.bool(at(a::CONTROL_GRENADE_TRYING_TO_THROW)) {
        m.set_bool(moving, false);
        let target = m.vector(at(a::CONTROL_GRENADE_CURRENT_TARGET));
        let body = m.vector(at(a::INPUT_BODY_POSITION));
        let mut grenade_direction = [target[0] - body[0], target[1] - body[1], target[2] - body[2]];
        crouch = false;
        if normalize3(&mut grenade_direction) != 0.0 {
            set_vector(m, desired_facing, grenade_direction);
            m.set_bool(free_facing, false);
            m.set_bool(optional_facing, false);
            m.set_bool(at(a::CONTROL_FACE_EXACTLY), true);
        } else {
            m.set_bool(free_facing, true);
        }
    } else if m.i16(at(a::EMOTIONS_MOVING_INTO_FIRE_TIMER)) > 0 {
        m.set_bool(moving, false);
        m.set_bool(free_facing, true);
        crouch = test_flag(flags, v::_ACTOR_DEFINITION_CROUCH_IN_LINE_OF_FIRE_BIT);
    } else {
        clear_firing_positions = true;
        let must_move_forward = if crouch {
            v::_ACTOR_DEFINITION_CROUCHING_MUST_MOVE_FORWARD_BIT
        } else {
            v::_ACTOR_DEFINITION_STANDING_MUST_MOVE_FORWARD_BIT
        };
        if movement_type != v::_ACTOR_MOVEMENT_TYPE_COMBAT as i16 || test_flag(flags, must_move_forward) {
            m.set_bool(at(a::CONTROL_MOVING_FORCED_BY_AIMING), false);
        }
        if movement_type == v::_ACTOR_MOVEMENT_TYPE_FLAMING as i16 {
            force_stationary_facing = true;
        }
        if test_flag(flags, v::_ACTOR_DEFINITION_FLYING_BIT) {
            move_in_3d = true;
            free_movement = true;
            let sidestep = m.f32(definition + d::MOVING_FREE_FLYING_SIDESTEP_DISTANCE as u32);
            free_movement_distance_squared = sidestep * sidestep;
            if m.bool(at(a::CONTROL_MOVING_FORCED_BY_AIMING)) {
                free_movement_distance_squared *= 4.0;
            }
        }
    }

    let movement_complete = at(a::CONTROL_MOVEMENT_COMPLETE);
    if m.bool(moving) && !m.bool(movement_complete) {
        call_full(
            context,
            host,
            call::CALCULATE_MOVEMENT,
            move_in_3d as i32 | (free_movement as i32) << 1 | (allow_all_moving_turns as i32) << 2,
            override_facing as i32,
            &[
                free_movement_distance_squared,
                steering_maximum_angle,
                oversteer_minimum_angle,
                oversteer_maximum_angle,
                rotation_emergency_amount,
                maximum_throttle,
            ],
        );
        if m.bool(movement_complete) {
            m.set_bool(moving, false);
        }
    }

    if m.bool(moving) {
        m.set_bool(optional_facing, false);
        m.set_bool(free_facing, false);
    } else if force_stationary_facing {
        set_vector(m, desired_facing, m.vector(at(a::INPUT_FACING_VECTOR)));
        m.set_i16(moving_facing_direction, 0);
        m.set_bool(optional_facing, false);
        m.set_bool(free_facing, false);
    } else if m.bool(at(a::CONTROL_FIXED_STATIONARY_FACING)) {
        copy(m, desired_facing, at(a::CONTROL_FIXED_STATIONARY_FACING_VECTOR), 12);
        m.set_i16(moving_facing_direction, 0);
        m.set_bool(optional_facing, true);
        m.set_bool(free_facing, false);
    }

    if clear_firing_positions && !m.bool(moving) {
        crate::firing_positions::clear_discarded_firing_positions(m, context, true);
    }
    if m.bool(moving) && test_flag(flags, v::_ACTOR_DEFINITION_CANNOT_MOVE_CROUCHING_BIT) {
        crouch = false;
    }
    if call_simple(context, host, call::FORCE_CROUCH, 0) != 0 {
        crouch = true;
    }
    m.set_bool(
        at(a::CONTROL_DESIRE_STATIONARY_FACING),
        crouch && test_flag(flags, v::_ACTOR_DEFINITION_FIXED_CROUCH_FACING_BIT),
    );
    m.set_bool(at(a::CONTROL_CROUCHING), crouch);
    call_simple(context, host, call::UNIT_CONTROL_CROUCH, crouch as i32);

    if call_simple(context, host, call::ANIMATION_BUSY, 0) == 0
        && m.i32(at(a::INPUT_VEHICLE_INDEX)) == NONE
        && !m.bool(at(a::INPUT_IN_MIDAIR))
        && m.bool(at(a::EMOTIONS_BERSERK))
        && !m.bool(at(a::EMOTIONS_PLAYED_BERSERK_SOUND))
    {
        let mut cause_unit_index = NONE;
        let facing = m.vector(at(a::INPUT_FACING_VECTOR));
        let mut alignment = [facing[0], facing[1]];
        let target_prop_index = m.i32(at(a::TARGET_TARGET_PROP_INDEX));
        if target_prop_index != NONE {
            let prop = prop_get(m, context, target_prop_index);
            cause_unit_index = m.i32(prop + p::UNIT_INDEX as u32);
            alignment = [m.f32(prop + p::ACTOR_TO_PROP as u32), m.f32(prop + p::ACTOR_TO_PROP as u32 + 4)];
            if normalize2(&mut alignment) == 0.0 {
                alignment = [facing[0], facing[1]];
            }
        }
        call_full(context, host, call::BERSERK, cause_unit_index, 0, &alignment);
        m.set_bool(at(a::EMOTIONS_PLAYED_BERSERK_SOUND), true);
    }

    if allow_jump {
        call_simple(context, host, call::UNIT_CONTROL_JUMP, 0);
    } else if m.bool(at(a::INPUT_IN_MIDAIR)) || m.i32(at(a::INPUT_VEHICLE_INDEX)) != NONE {
        m.set_bool(at(a::CONTROL_JUMPING_TARGETED), false);
    } else if call_simple(context, host, call::ANIMATION_BUSY, 0) == 0 && m.bool(at(a::ORDERS_MOVE_JUMP)) {
        let mut leaped = false;
        if m.bool(at(a::ORDERS_MOVE_JUMP_LEAP)) {
            let alignment = if m.bool(at(a::ORDERS_MOVE_JUMP_TARGETED)) {
                [m.f32(at(a::ORDERS_MOVE_JUMP_ALIGNMENT_VECTOR)), m.f32(at(a::ORDERS_MOVE_JUMP_ALIGNMENT_VECTOR) + 4)]
            } else {
                let facing = m.vector(at(a::INPUT_FACING_VECTOR));
                let mut alignment = [facing[0], facing[1]];
                if normalize2(&mut alignment) == 0.0 {
                    alignment = [1.0, 0.0];
                }
                alignment
            };
            leaped = call_full(context, host, call::LEAP_BEGIN, 0, 0, &alignment).result != 0;
        }
        if leaped {
            call_simple(context, host, call::LEAP_SHOUT, 0);
        } else {
            call_simple(context, host, call::UNIT_CONTROL_JUMP, 0);
        }
        if m.bool(at(a::ORDERS_MOVE_JUMP_TARGETED)) {
            m.set_bool(at(a::CONTROL_JUMPING_TARGETED), true);
            copy(m, at(a::CONTROL_JUMP_ALIGNMENT_VECTOR), at(a::ORDERS_MOVE_JUMP_ALIGNMENT_VECTOR), 8);
            copy(m, at(a::CONTROL_JUMP_TARGET_HORIZONTAL_VEL), at(a::ORDERS_MOVE_JUMP_TARGET_HORIZONTAL_VEL), 4);
            copy(m, at(a::CONTROL_JUMP_TARGET_VERTICAL_VEL), at(a::ORDERS_MOVE_JUMP_TARGET_VERTICAL_VEL), 4);
        }
    }

    copy(m, at(a::OUTPUT_ANIMATION), at(a::ORDERS_MOVE_ANIMATION), actor_animation_orders::SIZEOF as u32);
}

fn call_simple_output(context: &AiStepContext, host: &impl AiHost, kind: i32, a: i32) -> [f32; 6] {
    call_full(context, host, kind, a, 0, &[]).output
}
