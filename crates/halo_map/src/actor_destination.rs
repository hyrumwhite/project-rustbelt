//! An actor's destination (actor_moving.c's `actor_destination_update`,
//! every tick of an actor's update that controls it): its path refreshed in
//! its timeslice, the steps of the path it has reached, and where it is
//! moving towards (the path's step, ahead of a flying vehicle, or nowhere).
//!
//! The actor's datum is the game's memory, worked on in place
//! ([`crate::ai_actor`]); the path's refresh and clearing, the destination's
//! test and the errors are the game's, through [`AiHost::call`]
//! (actor_moving.c's `actor_destination_call`, shared with the C).

use crate::ai_actor::{AiCall, AiHost, AiMemory, AiMemoryExt, AiStepContext};
use crate::layout::{actor_datum as a, ai_values as v, path_result, path_step};

/// The destination update's calls (halo_sim.h's `_actor_destination_call_*`).
pub mod call {
    /// `actor_path_refresh(actor, FALSE, NULL)`.
    pub const PATH_REFRESH: i32 = 1;
    /// `actor_test_destination(actor)`.
    pub const TEST_DESTINATION: i32 = 2;
    /// `actor_path_clear(actor)`.
    pub const PATH_CLEAR: i32 = 3;
    /// The debug error for falling off the end of an unfinished path.
    pub const UNFINISHED_PATH: i32 = 4;
    /// The error for a path walking to tau ceti.
    pub const TAU_CETI: i32 = 5;
}

fn call_simple(context: &AiStepContext, host: &impl AiHost, kind: i32) {
    let mut call = AiCall { kind, actor: context.actor_index, ..AiCall::default() };
    host.call(&mut call);
}

fn point2(m: &dyn AiMemory, address: u32) -> [f32; 2] {
    [m.f32(address), m.f32(address + 4)]
}

/// `vector_from_points2d(a, b)`.
fn vector2(a: [f32; 2], b: [f32; 2]) -> [f32; 2] {
    [b[0] - a[0], b[1] - a[1]]
}

fn dot2(a: [f32; 2], b: [f32; 2]) -> f32 {
    a[0] * b[0] + a[1] * b[1]
}

/// `actor_destination_update`.
pub fn actor_destination_update(context: &AiStepContext, host: &impl AiHost) {
    let m = host.memory();
    let actor = context.actor;
    let path = actor + a::CONTROL_PATH_PATH as u32;
    let valid = path + path_result::VALID as u32;
    let movement_complete = actor + a::CONTROL_MOVEMENT_COMPLETE as u32;
    let moving = actor + a::CONTROL_MOVING as u32;
    let body_position = actor + a::INPUT_BODY_POSITION as u32;
    let step_point = |index: i32| path + path_result::STEPS as u32 + index as u32 * path_step::SIZEOF as u32 + path_step::POINT as u32;

    if m.bool(actor + a::META_TIMESLICE as u32)
        && !m.bool(actor + a::CONTROL_PATH_REFRESHED_THIS_TICK as u32)
        && !m.bool(actor + a::META_DORMANT as u32)
    {
        call_simple(context, host, call::PATH_REFRESH);
    }
    call_simple(context, host, call::TEST_DESTINATION);

    if m.bool(valid) {
        loop {
            let step_index = m.i8(path + path_result::STEP_INDEX as u32) as i32;
            if step_index + 1 >= m.i8(path + path_result::STEP_COUNT as u32) as i32 {
                break;
            }
            let point = point2(m, step_point(step_index));
            let mut actor_to_point = vector2(point2(m, body_position), point);
            let next_step = vector2(point, point2(m, step_point(step_index + 1)));

            let step_reached = if m.bool(movement_complete) {
                true
            } else if m.bool(moving) && m.bool(actor + a::CONTROL_MOVEMENT_THWARTED as u32) {
                let facing_dot = dot2(point2(m, actor + a::INPUT_FACING_VECTOR as u32), next_step);
                let distance_along_step = dot2(actor_to_point, next_step);
                if facing_dot > 0.0 && distance_along_step < 0.0 {
                    let t = -distance_along_step;
                    actor_to_point = [t * next_step[0] + actor_to_point[0], t * next_step[1] + actor_to_point[1]];
                    dot2(actor_to_point, actor_to_point) < 0.25f32 * 0.25f32
                } else {
                    false
                }
            } else {
                dot2(actor_to_point, actor_to_point) < 0.15f32 * 0.15f32
            };

            if !step_reached {
                break;
            }
            m.w8(path + path_result::STEP_INDEX as u32, (step_index + 1) as u8);
            m.set_bool(movement_complete, false);
        }

        if m.bool(movement_complete) {
            if m.bool(path + path_result::STEPS_FINISH_PATH as u32) {
                call_simple(context, host, call::PATH_CLEAR);
            } else {
                call_simple(context, host, call::UNFINISHED_PATH);
            }
        }
    }

    let moving_towards_point = actor + a::CONTROL_MOVING_TOWARDS_POINT as u32;
    let moving_towards_vector = actor + a::CONTROL_MOVING_TOWARDS_VECTOR as u32;
    if m.bool(valid) && (m.bool(moving) || !m.bool(actor + a::CONTROL_PATH_AT_DESTINATION as u32)) {
        m.set_bool(moving, true);
        let step_index = m.i8(path + path_result::STEP_INDEX as u32) as i32;
        for word in 0..3 {
            m.w32(moving_towards_point + word * 4, m.r32(step_point(step_index) + word * 4));
        }
        let from = m.vector(body_position);
        let to = m.vector(moving_towards_point);
        let vector = [to[0] - from[0], to[1] - from[1], to[2] - from[2]];
        for (i, value) in vector.into_iter().enumerate() {
            m.set_f32(moving_towards_vector + 4 * i as u32, value);
        }
        let magnitude = (vector[0] * vector[0] + vector[1] * vector[1] + vector[2] * vector[2]).sqrt();
        if magnitude >= 1_000_000.0 {
            call_simple(context, host, call::TAU_CETI);
            m.set_bool(valid, false);
        }
    } else if m.i16(actor + a::INPUT_VEHICLE_DRIVER_TYPE as u32) == v::_ACTOR_VEHICLE_DRIVER_DIRECTIONAL_FLYING as i16 {
        let reverse = m.f32(actor + a::CONTROL_VECTOR_AVOIDANCE_ROTATION_EMERGENCY_INSTANTANEOUS as u32) > 0.9;
        m.set_bool(moving, true);
        m.set_bool(movement_complete, false);
        let distance = (if reverse { -1.0f32 } else { 1.0 }) * 3.0;
        let facing = m.vector(actor + a::INPUT_FACING_VECTOR as u32);
        let vector = [distance * facing[0], distance * facing[1], distance * facing[2]];
        let from = m.vector(body_position);
        for i in 0..3 {
            m.set_f32(moving_towards_vector + 4 * i as u32, vector[i]);
            m.set_f32(moving_towards_point + 4 * i as u32, from[i] + vector[i]);
        }
    } else {
        m.set_bool(moving, false);
        m.set_bool(movement_complete, false);
        m.set_bool(actor + a::CONTROL_PATH_AT_DESTINATION as u32, true);
        call_simple(context, host, call::PATH_CLEAR);
    }
}
