//! An actor's movement (actor_moving.c's `actor_move_update`, every tick of
//! an actor's update that controls it): its avoidance while flying a
//! vehicle, its movement type, its crouch, how its vehicle (driving,
//! hovering, flying, stuck) or its state (riding, animating, asleep, in
//! midair, throwing a grenade, keeping out of a player's fire) holds it
//! still or frees it, its facing when still, its berserk shout and its
//! jumps and leaps, and the movement's calculation (`actor_move_calculate_movement`:
//! the facing, the throttle and the steering towards the moving vector).
//!
//! The actor's datum, definition and vehicle are the game's memory, worked
//! on in place ([`crate::ai_actor`]); the vector avoidance is
//! [`crate::actor_avoidance`]'s; the vehicle's definition and stuckness, the path, the pathfinding location, the
//! destination tolerance and stopping distances, the firing positions, the
//! unit's crouch, jump and leap, its animation and the shouts are the
//! game's, through [`AiHost::call`] (actor_moving.c's `actor_move_call`,
//! shared with the C).

use crate::ai_actor::{AiCall, AiHost, AiMemory, AiMemoryExt, AiStepContext, NONE, object_get, prop_get};
use crate::layout::{actor_animation_orders, actor_datum as a, actor_definition as d, ai_values as v, prop_datum as p, vehicle_datum};

/// The move update's calls (halo_sim.h's `_actor_move_call_*`).
pub mod call {
    // 1 was `actor_move_vector_avoidance`, now Rust's (`crate::actor_avoidance`).
    /// `actor_path_has_path(actor)`, the result.
    pub const HAS_PATH: i32 = 2;
    /// The vehicle `a`'s definition: its steering angle, throttle, oversteer
    /// bounds and sideslip distance in `output[0..5]`.
    pub const VEHICLE_DEFINITION: i32 = 3;
    /// `vehicle_stuck(a)`: the result, its direction in `output`.
    pub const VEHICLE_STUCK: i32 = 4;
    // 5 was `actor_move_calculate_movement`, now Rust's ([`calculate_movement`]).
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
    /// `actor_find_pathfinding_location(actor)`.
    pub const FIND_PATHFINDING_LOCATION: i32 = 14;
    /// `actor_destination_tolerance(actor)`, in `output[0]`.
    pub const DESTINATION_TOLERANCE: i32 = 15;
    /// `actor_get_stopping_distances(actor)`: the current and maximum in
    /// `output[0..2]`.
    pub const STOPPING_DISTANCES: i32 = 16;
    /// Whether a local game's debug disables oversteer, the result.
    pub const OVERSTEER_DISABLE: i32 = 17;
    /// `actor_move_avoidance_setup` for the object `a` into the actor's debug
    /// info's avoidance data (its origin, axes and the objects' cylinders).
    pub const AVOIDANCE_SETUP: i32 = 18;
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
        let movement_direction = if m.bool(moving) { m.vector(towards) } else { scale(m.vector(at(a::INPUT_FACING_VECTOR)), 3.0) };
        let (instantaneous, emergency_amount) =
            crate::actor_avoidance::actor_move_vector_avoidance(context, host, movement_direction, call::AVOIDANCE_SETUP);
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
        calculate_movement(
            context,
            host,
            Movement {
                move_in_3d,
                override_facing,
                free_movement,
                free_movement_distance_squared,
                allow_all_moving_turns,
                steering_maximum_angle,
                oversteer_minimum_angle,
                oversteer_maximum_angle,
                rotation_emergency_amount,
                maximum_throttle,
            },
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

/// actor_moving.c's facings.
const FACING_FORWARD: i16 = 0;
const FACING_BACKWARD: i16 = 1;
const FACING_LEFT: i16 = 2;
const FACING_RIGHT: i16 = 3;
const FACING_FREE: i16 = 4;

/// What the move update asks of [`calculate_movement`].
struct Movement {
    move_in_3d: bool,
    override_facing: i16,
    free_movement: bool,
    free_movement_distance_squared: f32,
    allow_all_moving_turns: bool,
    steering_maximum_angle: f32,
    oversteer_minimum_angle: f32,
    oversteer_maximum_angle: f32,
    rotation_emergency_amount: f32,
    maximum_throttle: f32,
}

fn dot(a: Vector, b: Vector) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// C's `MIN`.
fn min(a: f32, b: f32) -> f32 {
    if a > b { b } else { a }
}

/// C's `MAX`.
fn max(a: f32, b: f32) -> f32 {
    if a > b { a } else { b }
}

/// `actor_move_calculate_movement`: the facing, its direction and the
/// throttle towards the actor's moving vector, whether the facing thwarts
/// the movement and whether it is complete, into the actor's control and
/// output.
fn calculate_movement(context: &AiStepContext, host: &impl AiHost, movement: Movement) {
    let m = host.memory();
    let actor = context.actor;
    let at = |offset: usize| actor + offset as u32;
    let move_in_3d = movement.move_in_3d;
    let mut maximum_throttle = movement.maximum_throttle;
    let mut oversteer_minimum_angle = movement.oversteer_minimum_angle;
    let mut oversteer_maximum_angle = movement.oversteer_maximum_angle;
    let desired_movement_vector = m.vector(at(a::CONTROL_MOVING_TOWARDS_VECTOR));
    let input_facing = m.vector(at(a::INPUT_FACING_VECTOR));
    let forced_by_aiming = m.bool(at(a::CONTROL_MOVING_FORCED_BY_AIMING));
    let mut facing_vector;
    let mut free_throttle = [0.0f32; 3];
    let mut minimum_facing_dot = 0.8660254f32;
    let mut facing_direction;

    if m.bool(at(a::ORDERS_MOVE_MOVE_FACE_EXACTLY)) {
        m.set_bool(at(a::CONTROL_FACE_EXACTLY), true);
    }

    if (FACING_FORWARD..=FACING_RIGHT).contains(&movement.override_facing) {
        facing_direction = movement.override_facing;
        let mut movement_vector = desired_movement_vector;
        if !move_in_3d {
            movement_vector[2] = 0.0;
        }
        if normalize3(&mut movement_vector) == 0.0 {
            movement_vector = input_facing;
        }
        facing_vector = match movement.override_facing {
            FACING_FORWARD => movement_vector,
            FACING_BACKWARD => [-movement_vector[0], -movement_vector[1], movement_vector[2]],
            FACING_LEFT => [-movement_vector[1], movement_vector[0], movement_vector[2]],
            _ => [movement_vector[1], -movement_vector[0], movement_vector[2]],
        };
        if movement.free_movement {
            free_throttle = calculate_free(move_in_3d, movement_vector, facing_vector);
            facing_direction = FACING_FREE;
        }
    } else {
        let movement_distance_squared = magnitude_squared(desired_movement_vector);
        if movement_distance_squared > 0.8f32 * 0.8f32 {
            minimum_facing_dot = m.f32(context.definition + d::MOVING_RUNTIME_BEGIN_MOVEMENT_ANGLE_COSINE as u32);
        }

        if movement.free_movement && movement_distance_squared < movement.free_movement_distance_squared {
            let mut movement_vector = desired_movement_vector;
            let mut face_actor_facing = false;

            if forced_by_aiming {
                facing_vector = m.vector(at(a::CONTROL_MOVING_FORCED_AIM_DIRECTION));
                if m.i16(at(a::INPUT_VEHICLE_DRIVER_TYPE)) > v::_ACTOR_VEHICLE_DRIVER_NONE as i16 {
                    face_actor_facing = true;
                }
            } else {
                facing_vector = input_facing;
            }

            if !move_in_3d {
                movement_vector[2] = 0.0;
                facing_vector[2] = 0.0;
            }
            if normalize3(&mut facing_vector) == 0.0 {
                facing_vector = input_facing;
            }
            if normalize3(&mut movement_vector) == 0.0 {
                movement_vector = facing_vector;
            }

            let free_facing_vector = if face_actor_facing {
                let mut actor_facing_vector = input_facing;
                if !move_in_3d {
                    actor_facing_vector[2] = 0.0;
                }
                if normalize3(&mut actor_facing_vector) == 0.0 {
                    actor_facing_vector = facing_vector;
                }
                actor_facing_vector
            } else {
                facing_vector
            };
            free_throttle = calculate_free(move_in_3d, movement_vector, free_facing_vector);
            facing_direction = FACING_FREE;
        } else if forced_by_aiming {
            (facing_vector, facing_direction) = calculate_controlled_by_aiming(
                move_in_3d,
                desired_movement_vector,
                m.vector(at(a::CONTROL_MOVING_FORCED_AIM_DIRECTION)),
                input_facing,
            );
        } else {
            facing_vector = desired_movement_vector;
            if !move_in_3d {
                facing_vector[2] = 0.0;
            }
            if normalize3(&mut facing_vector) == 0.0 {
                facing_vector = input_facing;
            }
            facing_direction = FACING_FORWARD;
        }
    }

    let facing_dot = dot(facing_vector, input_facing);

    let facing_allows_movement = if movement.allow_all_moving_turns
        || m.i16(at(a::OUTPUT_MOVEMENT_TYPE)) == v::_ACTOR_MOVEMENT_TYPE_FLAMING as i16
    {
        true
    } else {
        if !m.bool(at(a::STATE_FLYING)) {
            call_simple(context, host, call::FIND_PATHFINDING_LOCATION, 0);
            let pathfinding_surface_index = m.i32(at(a::INPUT_PATHFINDING_SURFACE_INDEX));
            if pathfinding_surface_index != NONE {
                let f = m.vector(at(a::INPUT_FACING_VECTOR));
                let movement_direction = match facing_direction {
                    FACING_FORWARD => Some([f[0], f[1]]),
                    FACING_BACKWARD => Some([-f[0], -f[1]]),
                    FACING_LEFT => Some([f[1], -f[0]]),
                    FACING_RIGHT => Some([-f[1], f[0]]),
                    _ => None,
                };
                if let Some(mut direction) = movement_direction
                    && normalize2(&mut direction) > 0.0
                {
                    let body = m.vector(at(a::INPUT_BODY_POSITION));
                    let test_point = [body[0] + 0.4 * direction[0], body[1] + 0.4 * direction[1]];
                    let structure = crate::path::Structure::new(m, context.structure, context.breakable_surface_flags);
                    let (blocked, _) = structure.test_line2d(
                        m.bool(at(a::EMOTIONS_IGNORANT_OF_BROKEN_SURFACES)),
                        [body[0], body[1]],
                        pathfinding_surface_index,
                        test_point,
                        NONE,
                    );
                    if blocked {
                        minimum_facing_dot = max(minimum_facing_dot, 0.95);
                    }
                }
            }
        }
        facing_dot > minimum_facing_dot
    };

    let destination_tolerance = call_simple_output(context, host, call::DESTINATION_TOLERANCE, 0)[0];
    let movement_distance_squared = magnitude_squared(desired_movement_vector);
    m.set_bool(
        at(a::CONTROL_MOVEMENT_COMPLETE),
        movement_distance_squared < destination_tolerance * destination_tolerance,
    );

    let stopping = call_simple_output(context, host, call::STOPPING_DISTANCES, 0);
    let (current_stopping_distance, maximum_stopping_distance) = (stopping[0], stopping[1]);
    if !m.bool(at(a::CONTROL_PATH_DESTINATION_KEEP_MOVING))
        && movement_distance_squared < current_stopping_distance * current_stopping_distance
    {
        let movement_distance = movement_distance_squared.sqrt();
        if movement_distance > maximum_stopping_distance + 0.05 && current_stopping_distance > maximum_stopping_distance {
            maximum_throttle = min(
                maximum_throttle,
                (movement_distance - maximum_stopping_distance) / (current_stopping_distance - maximum_stopping_distance),
            );
        } else {
            maximum_throttle = 0.0;
        }
    }

    let mut throttle = [0.0f32; 3];
    if facing_allows_movement {
        match facing_direction {
            FACING_FORWARD => throttle[0] = 1.0,
            FACING_BACKWARD => throttle[0] = -1.0,
            FACING_LEFT => throttle[1] = -1.0,
            FACING_RIGHT => throttle[1] = 1.0,
            FACING_FREE => throttle = free_throttle,
            _ => {}
        }
        throttle = [throttle[0] * maximum_throttle, throttle[1] * maximum_throttle, throttle[2] * maximum_throttle];
        m.set_bool(at(a::CONTROL_MOVEMENT_THWARTED), false);
    } else {
        m.set_bool(at(a::CONTROL_FACE_EXACTLY), true);
        m.set_bool(at(a::CONTROL_MOVEMENT_THWARTED), true);
    }

    if call_simple(context, host, call::OVERSTEER_DISABLE, 0) != 0 {
        oversteer_maximum_angle = 0.0;
        oversteer_minimum_angle = 0.0;
    }

    let steering_maximum_angle = movement.steering_maximum_angle;
    if steering_maximum_angle > 0.0 || oversteer_maximum_angle > 0.0 {
        let angle = if facing_dot >= 1.0 {
            0.0
        } else if facing_dot <= -1.0 {
            std::f32::consts::PI
        } else {
            // arccosine: the C's double acos
            (facing_dot as f64).acos() as f32
        };

        let mut steering_angle = angle;
        if steering_maximum_angle > 0.0 {
            let rotation_emergency_amount = movement.rotation_emergency_amount;
            let mut minimum_steering_angle = steering_maximum_angle * rotation_emergency_amount;
            let mut maximum_steering_angle = steering_maximum_angle;
            if rotation_emergency_amount > 1.0 {
                maximum_steering_angle = min(rotation_emergency_amount, 1.5) * steering_maximum_angle;
            }
            minimum_steering_angle = min(angle * 3.0, minimum_steering_angle);
            // C's PIN
            steering_angle = if steering_angle < minimum_steering_angle {
                minimum_steering_angle
            } else if steering_angle > maximum_steering_angle {
                maximum_steering_angle
            } else {
                steering_angle
            };
        }

        let oversteer = at(a::CONTROL_FACE_EXACTLY_OVERSTEER_ANGLE);
        if steering_angle > m.f32(oversteer) {
            if m.bool(at(a::CONTROL_FACE_EXACTLY)) && steering_angle > oversteer_minimum_angle {
                m.set_f32(oversteer, min(steering_angle, oversteer_maximum_angle));
            }
        } else if m.f32(oversteer) > 0.0 {
            if steering_angle < oversteer_minimum_angle {
                m.set_f32(oversteer, 0.0);
            } else {
                steering_angle = m.f32(oversteer);
            }
        }

        let angle_adjustment = steering_angle - angle;
        if angle_adjustment.abs() > EPSILON {
            let f = m.vector(at(a::INPUT_FACING_VECTOR));
            let mut rotation_axis = [
                f[1] * facing_vector[2] - f[2] * facing_vector[1],
                f[2] * facing_vector[0] - f[0] * facing_vector[2],
                f[0] * facing_vector[1] - f[1] * facing_vector[0],
            ];
            if normalize3(&mut rotation_axis) > 0.0 {
                // sine and cosine: the C's double sin and cos
                let adjustment = angle_adjustment as f64;
                facing_vector =
                    rotate_about_axis(facing_vector, rotation_axis, adjustment.sin() as f32, adjustment.cos() as f32);
            }
        }
    }

    m.set_i16(at(a::CONTROL_MOVING_FACING_DIRECTION), facing_direction);
    set_vector(m, at(a::CONTROL_DESIRED_FACING_VECTOR), facing_vector);
    set_vector(m, at(a::OUTPUT_THROTTLE), throttle);
}

/// `actor_move_calculate_controlled_by_aiming`: of forward, backward, left
/// and right of the movement, the direction nearest the forced aim that
/// turns least.
fn calculate_controlled_by_aiming(
    move_in_3d: bool,
    movement_vector: Vector,
    forced_aim_direction: Vector,
    current_facing_vector: Vector,
) -> (Vector, i16) {
    let mut directions = [[0.0f32; 3]; 4];
    directions[0] = movement_vector;
    if move_in_3d {
        if normalize3(&mut directions[0]) == 0.0 {
            directions[0] = current_facing_vector;
        }
        directions[2] = [0.0; 3];
    } else {
        directions[0][2] = 0.0;
        if normalize3(&mut directions[0]) == 0.0 {
            directions[0] = current_facing_vector;
        }
        directions[2] = [-directions[0][1], directions[0][0], 0.0];
    }
    directions[1] = [-directions[0][0], -directions[0][1], -directions[0][2]];
    directions[3] = [-directions[2][0], -directions[2][1], -directions[2][2]];

    let mut best_direction = NONE as i16;
    let mut best_aim_dot = 0.0f32;
    let mut best_facing_dot = 0.0f32;
    for direction in FACING_FORWARD..FACING_FREE {
        let d = directions[direction as usize];
        let (aim_dot, facing_dot) = if move_in_3d {
            (dot(d, forced_aim_direction), dot(d, current_facing_vector))
        } else {
            (
                d[0] * forced_aim_direction[0] + d[1] * forced_aim_direction[1],
                d[0] * current_facing_vector[0] + d[1] * current_facing_vector[1],
            )
        };
        let better = if aim_dot > best_aim_dot {
            facing_dot > best_facing_dot || best_facing_dot < 0.5
        } else {
            facing_dot > best_facing_dot && aim_dot > 0.5
        };
        if best_direction == NONE as i16 || better {
            best_direction = direction;
            best_aim_dot = aim_dot;
            best_facing_dot = facing_dot;
        }
    }

    (directions[best_direction as usize], best_direction)
}

/// `actor_move_calculate_free`: the throttle along the movement in the
/// facing's axes.
fn calculate_free(move_in_3d: bool, movement_direction: Vector, facing_direction: Vector) -> Vector {
    let mut throttle = if move_in_3d {
        let (left, up) = crate::biped::flying_axes(facing_direction);
        [dot(movement_direction, facing_direction), dot(movement_direction, left), dot(movement_direction, up)]
    } else {
        let perpendicular = [-facing_direction[1], facing_direction[0]];
        [
            movement_direction[0] * facing_direction[0] + movement_direction[1] * facing_direction[1],
            movement_direction[0] * perpendicular[0] + movement_direction[1] * perpendicular[1],
            0.0,
        ]
    };
    normalize3(&mut throttle);
    throttle
}
