//! An actor's choice of a vehicle and its seat (action_vehicle.c's
//! `action_vehicle_desirable`, `action_vehicle_evaluate_seat`,
//! `action_vehicle_find_impromptu_seat` and
//! `action_vehicle_find_destination`), for the vehicle transition's
//! impromptu entry and the vehicle action: whether a vehicle is worth
//! entering, which seat (free, allowed, unclaimed by a nearer friend, a
//! gunner's or a passenger's by the variant's preference), and where to walk
//! to reach it around the vehicle.
//!
//! The actor, its props and its debug info are the game's memory, worked on
//! in place ([`crate::ai_actor`]); the seats (filled, for noncombatants, a
//! gunner's; their entrance points), the vehicle's shape and an object's
//! origin are the game's, through [`AiHost::call`] (action_vehicle.c's
//! `vehicle_choice_call`, shared with the C); the structure's ray is Rust's
//! own ([`AiHost::structure_test_vector`]).

use crate::ai_actor::{AiCall, AiHost, AiMemoryExt, AiStepContext, NONE, PropIterator};
use crate::layout::{
    actor_datum as a, actor_debug_info as dbg, actor_definition as d, actor_variant_definition as var, ai_values as v,
    prop_datum as p, unit_datum as u,
};

/// The vehicle choice's calls (halo_sim.h's `_vehicle_choice_call_*`), in a
/// range of their own that the dispatch step's call function passes on.
pub mod call {
    const BASE: i32 = 17 << 8;
    /// The seat `b` of the vehicle `a`: filled (bit 0), allowing
    /// noncombatants (bit 1), a gunner's (bit 2); the result.
    pub const SEAT_STATUS: i32 = BASE | 1;
    /// `unit_get_seat_entrance_point(unit, a, b, ...)`: the result, the
    /// entrance point in `output[0..3]` and the seat's in `output[3..6]`.
    pub const SEAT_ENTRANCE: i32 = BASE | 2;
    /// The same entrance's hint point, in `output[0..3]`.
    pub const SEAT_HINT: i32 = BASE | 3;
    /// The vehicle `a`'s shape: whether its entrance points lie inside its
    /// bounding sphere, the result; its seats' count, `result2`; its
    /// bounding sphere's center and radius in `output[0..4]`, its AI
    /// avoidance radius in `output[4]`.
    pub const VEHICLE_SHAPE: i32 = BASE | 4;
    /// `object_get_origin(a)`, in `output[0..3]`.
    pub const OBJECT_ORIGIN: i32 = BASE | 5;
}

const DISALLOW_VEHICLE_COMBAT_BIT: u32 = 3;
const SEAT_FILLED: i32 = 1 << 0;
const SEAT_ALLOW_NONCOMBATANTS: i32 = 1 << 1;
const SEAT_GUNNER: i32 = 1 << 2;

/// A seat's evaluation (`action_vehicle_evaluate_seat`'s outputs).
#[derive(Debug, Clone, Copy, Default)]
pub struct Seat {
    pub entry_point: [f32; 3],
    pub entry_facing: [f32; 3],
    pub hint_point: [f32; 3],
    pub weight: f32,
    pub within_range: bool,
    pub correct_facing: bool,
    pub could_potentially_fake: bool,
}

fn call(context: &AiStepContext, host: &impl AiHost, kind: i32, a: i32, b: i32) -> AiCall {
    let mut call = AiCall { kind, actor: context.actor_index, a, b, ..AiCall::default() };
    host.call(&mut call);
    call
}

fn vector3(output: &[f32]) -> [f32; 3] {
    [output[0], output[1], output[2]]
}

/// `normalize2d`.
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

/// `distance2d` (the C's double square root).
fn distance2(a: [f32; 3], b: [f32; 3]) -> f32 {
    let v = [b[0] - a[0], b[1] - a[1]];
    (v[0] * v[0] + v[1] * v[1]).sqrt()
}

/// `action_vehicle_desirable`.
#[allow(clippy::too_many_arguments)]
pub fn desirable(
    context: &AiStepContext,
    host: &impl AiHost,
    vehicle_index: i32,
    scripted_request: bool,
    attempt_start_distance: f32,
    attempt_continue_distance: f32,
    already_inside: bool,
    already_attempting_entry: bool,
) -> bool {
    let m = host.memory();
    let vehicle = crate::ai_actor::object_get(m, context, vehicle_index).unwrap_or(0);
    let mut desirable;
    if m.r16(vehicle + u::OBJECT_DAMAGE_FLAGS as u32) & (1 << v::_OBJECT_DEAD_BIT) != 0 {
        desirable = false;
    } else if scripted_request {
        desirable = true;
    } else {
        desirable = false;
        let maximum_distance = if already_attempting_entry { attempt_start_distance } else { attempt_continue_distance };
        let origin = vector3(&call(context, host, call::OBJECT_ORIGIN, vehicle_index, 0).output);
        let body = m.vector(context.actor + a::INPUT_BODY_POSITION as u32);
        let d = [origin[0] - body[0], origin[1] - body[1], origin[2] - body[2]];
        if already_inside || d[0] * d[0] + d[1] * d[1] + d[2] * d[2] < maximum_distance * maximum_distance {
            desirable = true;
            let velocity = m.vector(vehicle + u::OBJECT_TRANSLATIONAL_VELOCITY as u32);
            if !already_inside
                && !already_attempting_entry
                && velocity[0] * velocity[0] + velocity[1] * velocity[1] + velocity[2] * velocity[2] > 1.0 / (60.0f32 * 60.0)
            {
                desirable = false;
            }
        }
    }
    if !already_inside && m.f32(vehicle + u::OBJECT_UP as u32 + 8) < 0.5 {
        desirable = false;
    }
    desirable
}

/// `action_vehicle_evaluate_seat`: the seat, if the actor may take it.
pub fn evaluate_seat(context: &AiStepContext, host: &impl AiHost, vehicle_index: i32, seat_index: i16) -> Option<Seat> {
    let m = host.memory();
    let at = |offset: usize| context.actor + offset as u32;
    let status = call(context, host, call::SEAT_STATUS, vehicle_index, seat_index as i32).result;
    let flags2 = m.r32(context.definition + d::FLAGS2 as u32);
    if status & SEAT_FILLED != 0 || (flags2 & (1 << DISALLOW_VEHICLE_COMBAT_BIT) != 0 && status & SEAT_ALLOW_NONCOMBATANTS == 0) {
        return None;
    }
    let entrance = call(context, host, call::SEAT_ENTRANCE, vehicle_index, seat_index as i32);
    if entrance.result == 0 {
        return None;
    }
    let entrance_point = vector3(&entrance.output[0..3]);
    let seat_point = vector3(&entrance.output[3..6]);
    let hint_point = vector3(&call(context, host, call::SEAT_HINT, vehicle_index, seat_index as i32).output);

    let mut facing2 = [seat_point[0] - entrance_point[0], seat_point[1] - entrance_point[1]];
    let facing = if normalize2(&mut facing2) == 0.0 {
        m.vector(at(a::INPUT_FACING_VECTOR))
    } else {
        [facing2[0], facing2[1], 0.0]
    };
    let body = m.vector(at(a::INPUT_BODY_POSITION));
    let to_entrance = distance2(body, entrance_point);
    let to_seat = distance2(body, seat_point);
    let distance_to_seat = if to_entrance > to_seat { to_seat } else { to_entrance };

    let mut props = PropIterator::new(m, context);
    while let Some(prop) = props.next(m, context) {
        let other_actor_index = m.i32(prop + p::ACTOR_INDEX as u32);
        if m.bool(prop + p::ENEMY as u32) || other_actor_index == NONE {
            continue;
        }
        let other = crate::ai_actor::datum_get(m, context.actor_data, other_actor_index).unwrap_or(0);
        if other == 0
            || m.i16(other + a::STATE_ACTION as u32) != v::_ACTOR_ACTION_VEHICLE as i16
            || m.i32(other + a::STATE_VEHICLE_VEHICLE_INDEX as u32) != vehicle_index
        {
            continue;
        }
        if m.i16(other + a::STATE_VEHICLE_SEAT_INDEX as u32) == seat_index {
            let destination = m.vector(other + a::STATE_VEHICLE_DESTINATION_POINT as u32);
            let position = m.vector(other + a::INPUT_BODY_POSITION as u32);
            let dx = destination[0] - position[0];
            let dy = destination[1] - position[1];
            if dy * dy + dx * dx < distance_to_seat * distance_to_seat {
                return None;
            }
        }
    }

    let mut to_seat_direction = [seat_point[0] - body[0], seat_point[1] - body[1]];
    normalize2(&mut to_seat_direction);
    let input_facing = m.vector(at(a::INPUT_FACING_VECTOR));
    let facing_dot = to_seat_direction[0] * input_facing[0] + to_seat_direction[1] * input_facing[1];
    let mut weight = 10.0 / (distance_to_seat + 1.0);
    let gunner = status & SEAT_GUNNER != 0;
    let prefer_passenger =
        m.r32(context.variant_definition + var::FLAGS as u32) & (1 << v::_ACTOR_VARIANT_DEFINITION_PREFER_PASSENGER_SEAT_BIT) != 0;
    if prefer_passenger != gunner {
        weight += 3.5;
    }
    Some(Seat {
        entry_point: entrance_point,
        entry_facing: facing,
        hint_point,
        weight,
        within_range: distance_to_seat < 0.7,
        correct_facing: facing_dot > 0.6,
        could_potentially_fake: distance_to_seat < 1.1 && facing_dot > 0.0,
    })
}

/// `action_vehicle_find_impromptu_seat`: the best seat, if any, and its
/// evaluation.
pub fn find_impromptu_seat(context: &AiStepContext, host: &impl AiHost, vehicle_index: i32) -> (i16, Seat) {
    let seat_count = call(context, host, call::VEHICLE_SHAPE, vehicle_index, 0).result2;
    let mut best = Seat::default();
    let mut best_seat_index = NONE as i16;
    let mut best_weight = 0.0f32;
    let mut seat_index = 0i32;
    while (seat_index as i16 as i32) < seat_count {
        if let Some(seat) = evaluate_seat(context, host, vehicle_index, seat_index as i16)
            && seat.weight > best_weight
        {
            best_weight = seat.weight;
            best = seat;
            best_seat_index = seat_index as i16;
        }
        seat_index += 1;
    }
    (best_seat_index, best)
}

/// `perpendicular2d`.
fn perpendicular2(v: [f32; 2]) -> [f32; 2] {
    [-v[1], v[0]]
}

/// `action_vehicle_find_destination`: where to walk to reach the seat, off
/// the vehicle's side and onto the ground below (the surface found); whether
/// to ignore the hint is updated.
pub fn find_destination(
    context: &AiStepContext,
    host: &impl AiHost,
    vehicle_index: i32,
    seat: &Seat,
    ignore_hint: &mut bool,
) -> Option<([f32; 3], i32)> {
    let m = host.memory();
    let at = |offset: usize| context.actor + offset as u32;
    let debug = |offset: usize| context.debug_info + offset as u32;
    let entry_point = seat.entry_point;
    let hint_point = seat.hint_point;
    let mut modified_destination = entry_point;
    let shape = call(context, host, call::VEHICLE_SHAPE, vehicle_index, 0);
    if shape.result == 0 {
        let vehicle_center = vector3(&shape.output[0..3]);
        let mut vehicle_radius = shape.output[3];
        if shape.output[4] > 0.0 {
            vehicle_radius = shape.output[4];
        }
        if !*ignore_hint {
            let d = [hint_point[0] - vehicle_center[0], hint_point[1] - vehicle_center[1], hint_point[2] - vehicle_center[2]];
            let hint_distance = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
            if hint_distance < 0.5 {
                *ignore_hint = true;
            } else {
                let radius = hint_distance + 0.3;
                vehicle_radius = if vehicle_radius > radius { vehicle_radius } else { radius };
            }
        }
        let target_point = if *ignore_hint { entry_point } else { hint_point };
        let body = m.vector(at(a::INPUT_BODY_POSITION));
        let actor_to_vehicle = [vehicle_center[0] - body[0], vehicle_center[1] - body[1]];
        let actor_to_target_point = [target_point[0] - body[0], target_point[1] - body[1]];
        let target_point_to_vehicle = [vehicle_center[0] - target_point[0], vehicle_center[1] - target_point[1]];
        if !*ignore_hint {
            let hint_direction = [hint_point[0] - entry_point[0], hint_point[1] - entry_point[1]];
            let t = -(actor_to_target_point[0] * hint_direction[0] + actor_to_target_point[1] * hint_direction[1]);
            let perpendicular = [t * hint_direction[0] + actor_to_target_point[0], t * hint_direction[1] + actor_to_target_point[1]];
            if perpendicular[0] * perpendicular[0] + perpendicular[1] * perpendicular[1] < 0.35f32 * 0.35f32 {
                *ignore_hint = true;
            }
        }

        let set_vector = |address: u32, value: [f32; 3]| {
            for (i, component) in value.into_iter().enumerate() {
                m.set_f32(address + 4 * i as u32, component);
            }
        };
        m.w32(debug(dbg::LAST_VEHICLE_AVOIDANCE_TIME), context.game_time as u32);
        set_vector(debug(dbg::FIELD_C8), body);
        set_vector(debug(dbg::FIELD_D4), vehicle_center);
        m.set_f32(debug(dbg::FIELD_E0), vehicle_radius);
        set_vector(debug(dbg::FIELD_E4), target_point);
        m.set_f32(debug(dbg::FIELD_F0), f32::MAX);
        m.set_bool(debug(dbg::FIELD_F4), false);

        let distance_squared = actor_to_target_point[0] * actor_to_target_point[0] + actor_to_target_point[1] * actor_to_target_point[1];
        if distance_squared > 0.0 {
            let t = (actor_to_target_point[0] * actor_to_vehicle[0] + actor_to_target_point[1] * actor_to_vehicle[1]) / distance_squared;
            let mut avoid_direction = [0.0f32; 2];
            let mut avoid_vehicle = false;
            m.set_f32(debug(dbg::FIELD_F0), t);
            if t > 0.0 && t < 1.2 {
                avoid_direction = perpendicular2(actor_to_target_point);
                if actor_to_vehicle[0] * avoid_direction[0] + actor_to_vehicle[1] * avoid_direction[1] > 0.0 {
                    avoid_direction = [-avoid_direction[0], -avoid_direction[1]];
                }
                avoid_vehicle = true;
            } else if !*ignore_hint {
                avoid_direction = [-target_point_to_vehicle[0], -target_point_to_vehicle[1]];
                avoid_vehicle = true;
            }
            if avoid_vehicle && normalize2(&mut avoid_direction) > 0.0 {
                let reach = vehicle_radius * 1.1;
                modified_destination[0] = reach * avoid_direction[0] + vehicle_center[0];
                modified_destination[1] = reach * avoid_direction[1] + vehicle_center[1];
                let to_destination = [
                    modified_destination[0] - body[0],
                    modified_destination[1] - body[1],
                    modified_destination[2] - body[2],
                ];
                let destination_distance_squared =
                    to_destination[0] * to_destination[0] + to_destination[1] * to_destination[1] + to_destination[2] * to_destination[2];
                if destination_distance_squared > 0.0001 && destination_distance_squared < 4.0 {
                    let distance = destination_distance_squared.sqrt();
                    let mut tangential = perpendicular2(target_point_to_vehicle);
                    if tangential[0] * to_destination[0] + tangential[1] * to_destination[1] < 0.0 {
                        tangential = [-tangential[0], -tangential[1]];
                    }
                    if normalize2(&mut tangential) > 0.0 {
                        let offset = 2.0 - distance;
                        modified_destination = [
                            tangential[0] * offset + modified_destination[0],
                            tangential[1] * offset + modified_destination[1],
                            0.0 * offset + modified_destination[2],
                        ];
                    }
                }
                m.set_bool(debug(dbg::FIELD_F4), true);
                set_vector(debug(dbg::FIELD_F8), modified_destination);
            }
        }
    }

    let origin = [0.0 * 1.0 + modified_destination[0], 0.0 * 1.0 + modified_destination[1], 1.0 * 1.0 + modified_destination[2]];
    let vector = [0.0 * 4.0, 0.0 * 4.0, -1.0 * 4.0];
    let flags = crate::collision::flags::FRONT_FACING;
    host.structure_test_vector(context, false, flags, origin, vector, f32::MAX).map(|(t, surface_index)| {
        ([vector[0] * t + origin[0], vector[1] * t + origin[1], vector[2] * t + origin[2]], surface_index)
    })
}
