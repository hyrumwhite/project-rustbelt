//! A flying vehicle's vector avoidance (actor_moving.c's
//! `actor_move_vector_avoidance`, with `actor_move_test_avoidance_vector`,
//! `actor_move_vector_avoidance_find_direction`,
//! `actor_move_transform_avoidance_vector` and the rays of
//! `actor_move_initialize`): sense rays ahead and avoidance rays all round
//! test the structure and the objects nearby, weigh the eight directions,
//! and turn the vehicle away from what is in its way.
//!
//! The actor's datum, its debug info and the vehicle are the game's memory,
//! worked on in place ([`crate::ai_actor`]); the objects in the way (the
//! avoidance data's cylinders, `actor_move_avoidance_setup`) are gathered by
//! the game through [`AiHost::call`], and the structure's rays through
//! [`AiHost::structure_test_vector`].

// (the tables are the C's own values, as it writes them)
#![allow(clippy::approx_constant, clippy::excessive_precision)]

use crate::ai_actor::{AiCall, AiHost, AiMemory, AiMemoryExt, AiStepContext, NONE, object_get};
use crate::collision::flags as test_flags;
use crate::layout::{
    actor_datum as a, actor_debug_info as dbg, unit_datum as u, vector_avoidance_data as data,
    vehicle_avoidance_cylinder as cylinder,
};

type Vector = [f32; 3];

const DIRECTIONS: usize = 8;
const REAL_MAX: f32 = f32::MAX;
const EPSILON: f32 = 0.0001;
const PI: f32 = std::f32::consts::PI;

const CLEAR: i16 = 0;
const OBSTRUCTED_OBJECT: i16 = 1;
const OBSTRUCTED_STRUCTURE: i16 = 2;

const SENSE_RAY_LENGTH: f32 = 1.0;
const SENSE_RAY_OFFSET: f32 = 0.7;
const SENSE_RAY_DIVERGENCE: f32 = 0.052359879;
const SENSE_RAY_OFFSETS: [f32; 9] = [0.0, 0.5, 0.5, 0.5, 0.5, 1.0, 1.0, 1.0, 1.0];
const SENSE_RAY_DIVERGENCES: [f32; 9] = [0.0, 0.3, 0.3, 0.3, 0.3, 1.0, 1.0, 1.0, 1.0];
const SENSE_RAY_ANGLES: [f32; 9] = [0.0, 0.0, 1.5707964, 3.1415927, 4.712389, 0.0, 1.5707964, 3.1415927, 4.712389];
const AVOIDANCE_RAY_LENGTH: f32 = 0.7;
const AVOIDANCE_RAY_ANGLES: [f32; DIRECTIONS] =
    [0.0, 0.78539819, 1.5707964, 2.3561945, 3.1415927, 3.9269909, 4.712389, 5.4977875];
const AVOIDANCE_RAY_OFFSETS: [f32; 2] = [0.7, 1.0];
const AVOIDANCE_RAY_DIVERGENCE: [f32; 2] = [0.5235988, 0.95993108];
const SENSE_RAY_AVOIDANCE_WEIGHTS: [[f32; DIRECTIONS]; 9] = [
    [0.0; DIRECTIONS],
    [0.0; DIRECTIONS],
    [0.0; DIRECTIONS],
    [0.0; DIRECTIONS],
    [0.0; DIRECTIONS],
    [-0.5, -0.5, 0.0, 0.7, 1.0, 0.7, 0.0, -0.5],
    [0.0, -0.5, -0.5, -0.5, 0.0, 0.7, 1.0, 0.7],
    [1.0, 0.7, 0.0, -0.5, -0.5, -0.5, 0.0, 0.7],
    [0.0, 0.7, 1.0, 0.7, 0.0, -0.5, -0.5, -0.5],
];
const AVOID_RAY_AVOIDANCE_WEIGHTS: [f32; 2] = [0.8, 1.2];
const AVOID_RAY_CLEAR_BIAS_TIME: i32 = 75;
const AVOID_RAY_ADJACENT_FRACTIONS: [f32; 2] = [0.8, 0.5];

/// `struct vector_avoidance_ray`.
#[derive(Clone, Copy, Default)]
struct Ray {
    length: f32,
    offset: Vector,
    divergence: Vector,
}

/// sine and cosine: the C's double sin and cos.
fn sine(angle: f32) -> f32 {
    (angle as f64).sin() as f32
}

fn cosine(angle: f32) -> f32 {
    (angle as f64).cos() as f32
}

/// actor_move_initialize's sense rays.
fn sense_rays() -> [Ray; 9] {
    std::array::from_fn(|index| {
        let ray_sine = sine(SENSE_RAY_ANGLES[index]);
        let ray_cosine = cosine(SENSE_RAY_ANGLES[index]);
        let divergence = SENSE_RAY_DIVERGENCE * SENSE_RAY_DIVERGENCES[index];
        let divergence_sine = sine(divergence);
        let divergence_cosine = cosine(divergence);
        let offset = SENSE_RAY_OFFSET * SENSE_RAY_OFFSETS[index];
        Ray {
            length: SENSE_RAY_LENGTH,
            offset: [0.0, offset * ray_cosine, offset * ray_sine],
            divergence: [divergence_cosine, divergence_sine * ray_cosine, divergence_sine * ray_sine],
        }
    })
}

/// actor_move_initialize's avoidance directions.
fn avoidance_directions() -> [Vector; DIRECTIONS] {
    std::array::from_fn(|index| [0.0, cosine(AVOIDANCE_RAY_ANGLES[index]), sine(AVOIDANCE_RAY_ANGLES[index])])
}

/// actor_move_initialize's avoidance rays.
fn avoidance_rays(directions: &[Vector; DIRECTIONS]) -> [[Ray; 2]; DIRECTIONS] {
    std::array::from_fn(|direction| {
        std::array::from_fn(|ray| {
            let divergence_sine = sine(AVOIDANCE_RAY_DIVERGENCE[ray]);
            let divergence_cosine = cosine(AVOIDANCE_RAY_DIVERGENCE[ray]);
            let d = directions[direction];
            let offset = AVOIDANCE_RAY_OFFSETS[ray];
            Ray {
                length: AVOIDANCE_RAY_LENGTH,
                offset: [d[0] * offset, d[1] * offset, d[2] * offset],
                divergence: [divergence_cosine, d[1] * divergence_sine, d[2] * divergence_sine],
            }
        })
    })
}

fn dot(a: Vector, b: Vector) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn magnitude_squared(v: Vector) -> f32 {
    v[0] * v[0] + v[1] * v[1] + v[2] * v[2]
}

/// `normalize3d`.
fn normalize3(v: &mut Vector) -> f32 {
    let magnitude = magnitude_squared(*v).sqrt();
    if !(EPSILON > magnitude.abs()) {
        let s = 1.0 / magnitude;
        *v = [s * v[0], s * v[1], s * v[2]];
        magnitude
    } else {
        0.0
    }
}

/// C's `MIN`, `MAX` and `PIN`.
fn min(a: f32, b: f32) -> f32 {
    if a > b { b } else { a }
}

fn max(a: f32, b: f32) -> f32 {
    if a > b { a } else { b }
}

fn pin(n: f32, floor: f32, ceiling: f32) -> f32 {
    if n < floor { floor } else { min(n, ceiling) }
}

/// `fast_normalize2d` on the first two.
fn fast_normalize2(v: &mut Vector) {
    let magnitude_squared = v[0] * v[0] + v[1] * v[1];
    if magnitude_squared != 0.0 {
        let s = 1.0 / magnitude_squared.sqrt();
        v[0] *= s;
        v[1] *= s;
    }
}

/// `fast_normalize3d`.
fn fast_normalize3(v: &mut Vector) {
    let magnitude_squared = magnitude_squared(*v);
    if magnitude_squared != 0.0 {
        let s = 1.0 / magnitude_squared.sqrt();
        *v = [v[0] * s, v[1] * s, v[2] * s];
    }
}

/// real_math.c's `sphere_test_vector3d`: (t, normal).
fn sphere_test_vector(center: Vector, radius: f32, point: Vector, vector: Vector) -> Option<(f32, Vector)> {
    let x = [point[0] - center[0], point[1] - center[1], point[2] - center[2]];
    let xv = dot(x, vector);
    if xv < 0.0 {
        let xx = magnitude_squared(x);
        let xx_rr = xx - radius * radius;
        if xx_rr <= 0.0 {
            let s = 1.0 / xx.sqrt();
            return Some((0.0, [x[0] * s, x[1] * s, x[2] * s]));
        }
        let d = vector[0] * vector[0] + vector[1] * vector[1] + vector[2] * vector[2];
        let vv = xv * xv - d * xx_rr;
        if vv >= 0.0 {
            let n = -(vv.sqrt() + xv);
            if n <= d {
                let t = n / d;
                let mut normal = [vector[0] * t + x[0], vector[1] * t + x[1], vector[2] * t + x[2]];
                fast_normalize3(&mut normal);
                return Some((t, normal));
            }
        }
    }
    None
}

/// real_math.c's `pill_test_vector3d`: the t at which a point moving along
/// `vector` meets the upright pill (its base, height and radius).
fn pill_test_vector(base: Vector, height: f32, radius: f32, point: Vector, vector: Vector) -> Option<f32> {
    let x = [point[0] - base[0], point[1] - base[1]];
    let xv = x[0] * vector[0] + x[1] * vector[1];
    let xx_rr = x[0] * x[0] + x[1] * x[1] - radius * radius;
    let n = if xx_rr <= 0.0 {
        0.0
    } else {
        let d = vector[0] * vector[0] + vector[1] * vector[1];
        let vv = xv * xv - d * xx_rr;
        if vv < 0.0 {
            return None;
        }
        let n = -(vv.sqrt() + xv);
        if n <= d {
            n / d
        } else {
            return None;
        }
    };

    let height_fraction = (point[2] - base[2] + n * vector[2]) / (height * height);
    let hit = if height_fraction < 0.0 {
        sphere_test_vector(base, radius, point, vector)
    } else if height_fraction > 1.0 {
        sphere_test_vector([base[0], base[1], base[2] + height], radius, point, vector)
    } else if xv < 0.0 {
        let mut normal = [x[0] + n * vector[0], x[1] + n * vector[1], 0.0];
        fast_normalize2(&mut normal);
        normal[2] = 0.0;
        Some((n, normal))
    } else {
        None
    };
    hit.filter(|&(_, normal)| !(dot(normal, vector) > 0.0)).map(|(t, _)| t)
}

/// real_math.c's `angle_between_vectors3d`.
fn angle_between_vectors(a: Vector, b: Vector) -> f32 {
    let mut angle = 0.0f32;
    let aa_bb = (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]) * (b[0] * b[0] + b[1] * b[1] + b[2] * b[2]);
    if aa_bb != 0.0 {
        let ab = dot(a, b);
        let c = pin((ab / aa_bb * ab) + (ab / aa_bb * ab) - 1.0, -1.0, 1.0);
        // arccosine: the C's double acos
        angle = ((c as f64).acos() as f32) * 0.5;
        if ab < 0.0 {
            angle = PI - angle;
        }
    }
    angle
}

/// The avoidance data (`struct vector_avoidance_data`) the game gathered.
struct Avoidance<'a> {
    m: &'a dyn AiMemory,
    address: u32,
    origin: Vector,
    forward: Vector,
    left: Vector,
    up: Vector,
    avoid_width: f32,
    avoid_distance: f32,
}

impl Avoidance<'_> {
    /// `actor_move_transform_avoidance_vector`.
    fn transform(&self, v: Vector) -> Vector {
        let mut d = [0.0f32; 3];
        for (component, axis) in [(v[0], self.forward), (v[1], self.left), (v[2], self.up)] {
            d[0] += component * axis[0];
            d[1] += component * axis[1];
            d[2] += component * axis[2];
        }
        d
    }

    /// `actor_move_test_avoidance_vector`: (the result, its t, the ray's
    /// origin and direction).
    fn test(
        &self,
        context: &AiStepContext,
        host: &impl AiHost,
        ray: &Ray,
        collision_timer: Option<u32>,
    ) -> (i16, f32, Vector, Vector) {
        let m = self.m;
        let mut result = CLEAR;
        let mut collision_t = REAL_MAX;
        let offset = self.transform(ray.offset);
        let divergence = self.transform(ray.divergence);
        let ray_origin = [
            offset[0] * self.avoid_width + self.origin[0],
            offset[1] * self.avoid_width + self.origin[1],
            offset[2] * self.avoid_width + self.origin[2],
        ];
        let scale = self.avoid_distance * ray.length;
        let ray_direction = [divergence[0] * scale, divergence[1] * scale, divergence[2] * scale];
        let to_origin = [ray_origin[0] - self.origin[0], ray_origin[1] - self.origin[1], ray_origin[2] - self.origin[2]];
        let flags = test_flags::FRONT_FACING | test_flags::BACK_FACING;
        if host.structure_test_vector(context, false, flags, self.origin, to_origin, 1.0).is_some() {
            result = OBSTRUCTED_STRUCTURE;
            collision_t = 0.0;
        } else if let Some((t, _)) = host.structure_test_vector(context, false, flags, ray_origin, ray_direction, 1.0) {
            result = OBSTRUCTED_STRUCTURE;
            collision_t = t;
        }

        let count = m.i16(self.address + data::AVOIDANCE_OBJECT_COUNT as u32);
        for index in 0..count.max(0) as u32 {
            let object = self.address + data::AVOIDANCE_OBJECTS as u32 + index * cylinder::SIZEOF as u32;
            if let Some(object_t) = pill_test_vector(
                m.vector(object + cylinder::BASE as u32),
                m.f32(object + cylinder::HEIGHT as u32),
                m.f32(object + cylinder::WIDTH as u32),
                ray_origin,
                ray_direction,
            ) && object_t < collision_t
            {
                result = OBSTRUCTED_OBJECT;
                collision_t = object_t;
            }
        }

        if let Some(timer) = collision_timer {
            if result > CLEAR {
                m.w8(timer, 0);
            } else if m.r8(timer) < u8::MAX {
                m.w8(timer, m.r8(timer) + 1);
            }
        }

        (result, collision_t, ray_origin, ray_direction)
    }
}

/// `actor_move_vector_avoidance_find_direction`: where between the
/// directions `direction` lies, and its weight there.
fn find_direction(directions: &[Vector; DIRECTIONS], weights: &[f32; DIRECTIONS], direction: Vector) -> Option<(f32, f32)> {
    let count = DIRECTIONS;
    let mut previous_index = count - 1;
    let mut previous_cross = directions[previous_index][1] * direction[2] - directions[previous_index][2] * direction[1];
    for index in 0..count {
        let cross = directions[index][1] * direction[2] - directions[index][2] * direction[1];
        if previous_cross * cross <= 0.0 && dot(direction, directions[index]) > 0.0 {
            let approximate_direction = (previous_index as f32 * cross
                - (if index == 0 { count } else { index }) as f32 * previous_cross)
                / (cross - previous_cross);
            let approximate_weight =
                (weights[previous_index] * cross - weights[index] * previous_cross) / (cross - previous_cross);
            return Some((approximate_direction, approximate_weight));
        }
        previous_index = index;
        previous_cross = cross;
    }
    None
}

/// Spreads a weight to a direction and its neighbours.
fn spread(weights: &mut [f32; DIRECTIONS], direction: usize, weight: f32) {
    weights[direction] += weight;
    weights[(direction + 1) % DIRECTIONS] += AVOID_RAY_ADJACENT_FRACTIONS[0] * weight;
    weights[(direction + 2) % DIRECTIONS] += AVOID_RAY_ADJACENT_FRACTIONS[1] * weight;
    weights[(direction + DIRECTIONS - 1) % DIRECTIONS] += AVOID_RAY_ADJACENT_FRACTIONS[0] * weight;
    weights[(direction + DIRECTIONS - 2) % DIRECTIONS] += AVOID_RAY_ADJACENT_FRACTIONS[1] * weight;
}

fn set_vector(m: &dyn AiMemory, address: u32, v: Vector) {
    for (i, value) in v.into_iter().enumerate() {
        m.set_f32(address + 4 * i as u32, value);
    }
}

/// `actor_move_vector_avoidance` along `movement_direction`: the rotation
/// and its emergency. `setup_kind` is the call that gathers the avoidance
/// data into the actor's debug info (`actor_move_avoidance_setup`).
pub fn actor_move_vector_avoidance(
    context: &AiStepContext,
    host: &impl AiHost,
    movement_direction: Vector,
    setup_kind: i32,
) -> (Vector, f32) {
    let m = host.memory();
    let actor = context.actor;
    let at = |offset: usize| actor + offset as u32;
    let mut emergency = 0.0f32;
    let mut rotation = [0.0f32; 3];
    let mut object_index = m.i32(at(a::INPUT_VEHICLE_INDEX));
    if object_index == NONE {
        object_index = m.i32(at(a::META_UNIT_INDEX));
    }
    if object_index == NONE {
        return (rotation, emergency);
    }
    let Some(object) = object_get(m, context, object_index) else { return (rotation, emergency) };
    let debug = |offset: usize| context.debug_info + offset as u32;
    let mut direction_chosen = false;
    let mut sharp_turn = false;

    m.w32(debug(dbg::FIELD_19C), context.game_time as u32);
    let mut call = AiCall { kind: setup_kind, actor: context.actor_index, a: object_index, ..AiCall::default() };
    host.call(&mut call);
    let address = debug(dbg::AVOIDANCE_DATA);
    let avoidance = Avoidance {
        m,
        address,
        origin: m.vector(address + data::ORIGIN as u32),
        forward: m.vector(address + data::FORWARD as u32),
        left: m.vector(address + data::LEFT as u32),
        up: m.vector(address + data::UP as u32),
        avoid_width: m.f32(address + data::AVOID_WIDTH as u32),
        avoid_distance: m.f32(address + data::AVOID_DISTANCE as u32),
    };
    let sense_rays = sense_rays();
    let directions = avoidance_directions();
    let avoidance_rays = avoidance_rays(&directions);

    let mut maximum_sense_emergency = 0.0f32;
    let mut weights = [0.0f32; DIRECTIONS];

    let current_direction = m.i16(at(a::CONTROL_VECTOR_AVOIDANCE_CURRENT_DIRECTION));
    if (0..DIRECTIONS as i16).contains(&current_direction) {
        spread(&mut weights, current_direction as usize, 0.4);
    }

    for (ray_index, ray) in sense_rays.iter().enumerate() {
        let (avoidance_type, collision_t, ray_origin, ray_direction) = avoidance.test(context, host, ray, None);
        let i = ray_index as u32;
        set_vector(m, debug(dbg::RAY_ORIGIN) + 12 * i, ray_origin);
        set_vector(m, debug(dbg::RAY_DIRECTION) + 12 * i, ray_direction);
        m.set_i16(debug(dbg::AVOIDANCE_TYPE) + 2 * i, avoidance_type);
        m.set_f32(debug(dbg::COLLISION_T) + 4 * i, collision_t);
        if avoidance_type > CLEAR {
            let sense_emergency = 1.0 - collision_t;
            let sense_weight = 2.0 * sense_emergency;
            for (direction, weight) in weights.iter_mut().enumerate() {
                *weight += SENSE_RAY_AVOIDANCE_WEIGHTS[ray_index][direction] * min(sense_weight, 1.0);
            }
            maximum_sense_emergency = max(maximum_sense_emergency, sense_emergency);
        }
    }

    for direction in 0..DIRECTIONS {
        let mut results = [CLEAR; 2];
        let mut ts = [0.0f32; 2];
        for ray in 0..2 {
            let timer = at(a::CONTROL_VECTOR_AVOIDANCE_CLEAR_TIMES) + (direction * 2 + ray) as u32;
            let (result, t, ray_origin, ray_direction) =
                avoidance.test(context, host, &avoidance_rays[direction][ray], Some(timer));
            results[ray] = result;
            ts[ray] = t;
            let i = (direction * 2 + ray) as u32;
            set_vector(m, debug(dbg::FIELD_6358) + 12 * i, ray_origin);
            set_vector(m, debug(dbg::FIELD_6418) + 12 * i, ray_direction);
            m.set_i16(debug(dbg::FIELD_62F8) + 2 * i, result);
            m.set_f32(debug(dbg::AVOID_T) + 4 * i, t);
        }

        let mut direction_weight = 0.0f32;
        let mut obstructed = false;
        for ray in (0..2).rev() {
            if results[ray] == CLEAR {
                let mut clear_fraction = 1.0f32;
                if !obstructed {
                    let clear_time = m.r8(at(a::CONTROL_VECTOR_AVOIDANCE_CLEAR_TIMES) + (direction * 2 + ray) as u32);
                    clear_fraction = if (clear_time as i32) < AVOID_RAY_CLEAR_BIAS_TIME {
                        0.0
                    } else {
                        pin(1.0 - AVOID_RAY_CLEAR_BIAS_TIME as f32 / clear_time as f32, 0.0, 1.0)
                    };
                }
                direction_weight += AVOID_RAY_AVOIDANCE_WEIGHTS[ray] * clear_fraction;
            } else {
                direction_weight -= AVOID_RAY_AVOIDANCE_WEIGHTS[ray] * min(2.0 * (1.0 - ts[ray]), 1.0);
                obstructed = true;
            }
        }
        spread(&mut weights, direction, direction_weight);
    }

    let angular_velocity = m.vector(object + u::OBJECT_ANGULAR_VELOCITY as u32);
    let angular_speed = magnitude_squared(angular_velocity).sqrt();
    m.set_bool(debug(dbg::FIELD_6551), false);
    if angular_speed > 0.02 {
        let mut velocity_weight = min((angular_speed - 0.02) * 12.5, 1.0) * 0.8;
        let mut velocity_direction = [0.0, dot(avoidance.up, angular_velocity), -dot(avoidance.left, angular_velocity)];
        let mut velocity_approximate_weight = 0.0f32;
        let mut adjust = false;
        if normalize3(&mut velocity_direction) > 0.0
            && let Some((_, weight)) = find_direction(&directions, &weights, velocity_direction)
        {
            velocity_approximate_weight = weight;
            adjust = weight > 0.5;
        }
        if adjust {
            for (direction, weight) in weights.iter_mut().enumerate() {
                let velocity_dot = dot(directions[direction], velocity_direction);
                if velocity_dot < 0.0 {
                    *weight += velocity_dot * velocity_weight;
                }
            }
        } else {
            velocity_weight = 0.0;
        }
        m.set_f32(debug(dbg::FIELD_6554), velocity_weight);
        m.set_f32(debug(dbg::FIELD_6558), angular_speed);
        m.set_bool(debug(dbg::FIELD_6551), true);
        set_vector(m, debug(dbg::AVOIDANCE_VECTOR), velocity_direction);
        m.set_f32(debug(dbg::FIELD_6568), velocity_approximate_weight);
    }

    let mut best_weight = -REAL_MAX;
    let mut best = 0usize;
    for (direction, &weight) in weights.iter().enumerate() {
        if weight > best_weight {
            best_weight = weight;
            best = direction;
        }
    }
    for (direction, &weight) in weights.iter().enumerate() {
        m.set_f32(debug(dbg::FIELD_64D8) + 4 * direction as u32, weight);
    }

    let mut movement_vector = movement_direction;
    let mut forward_dot = 1.0f32;
    let mut local_movement_direction = [0.0f32; 3];
    let mut movement_direction_approximation = 0.0f32;
    let mut movement_approximate_weight = 0.0f32;
    if normalize3(&mut movement_vector) > 0.0 {
        local_movement_direction[0] = 0.0;
        forward_dot = dot(avoidance.forward, movement_vector);
        local_movement_direction[1] = dot(avoidance.left, movement_vector);
        local_movement_direction[2] = dot(avoidance.up, movement_vector);
        if normalize3(&mut local_movement_direction) > 0.0
            && let Some((approximation, weight)) = find_direction(&directions, &weights, local_movement_direction)
        {
            movement_direction_approximation = approximation;
            movement_approximate_weight = weight;
        }
    }

    let weight_difference = best_weight - movement_approximate_weight;
    set_vector(m, debug(dbg::FIELD_6524), avoidance.forward);
    set_vector(m, debug(dbg::FIELD_6530), movement_direction);
    m.set_f32(debug(dbg::FIELD_6504), movement_direction_approximation);
    m.set_f32(debug(dbg::FIELD_64FC), best_weight);
    m.set_i16(debug(dbg::FIELD_6500), best as i16);
    m.set_f32(debug(dbg::FIELD_6508), movement_approximate_weight);
    let emergency_scale = if maximum_sense_emergency > 0.6 {
        1.0 + min(1.0, (maximum_sense_emergency - 0.6) / (1.0f32 - 0.6f32))
    } else {
        min(1.0, maximum_sense_emergency / 0.3)
    };
    m.set_f32(debug(dbg::FIELD_6510), forward_dot);
    m.set_f32(debug(dbg::SIGN_NO_DANGER), weight_difference);

    let timer = at(a::CONTROL_VECTOR_AVOIDANCE_SHARP_TURN_TIMER);
    if forward_dot < -0.2 {
        let sharp_turn_timer = m.i16(timer);
        if sharp_turn_timer != NONE as i16 && sharp_turn_timer < 90 {
            m.set_i16(debug(dbg::FIELD_653C), 7);
            sharp_turn = true;
        } else if magnitude_squared(angular_velocity) > 0.05f32 * 0.05f32 {
            if weight_difference > 2.0 && best_weight > 2.0 {
                m.set_i16(debug(dbg::FIELD_653C), 6);
                sharp_turn = true;
            }
        } else if emergency_scale > 0.5 {
            m.set_i16(debug(dbg::FIELD_653C), 5);
            sharp_turn = true;
        }
    }

    if sharp_turn {
        if m.i16(timer) == NONE as i16 {
            m.set_i16(timer, 0);
        } else {
            m.set_i16(timer, m.i16(timer).wrapping_add(1));
        }
    } else {
        m.set_i16(timer, NONE as i16);
    }

    let third_pi = PI / 3.0;
    if sharp_turn {
        let best_direction = avoidance.transform(directions[best]);
        let md = movement_direction;
        let mut rotation_axis = [
            md[1] * best_direction[2] - md[2] * best_direction[1],
            md[2] * best_direction[0] - md[0] * best_direction[2],
            md[0] * best_direction[1] - md[1] * best_direction[0],
        ];
        if normalize3(&mut rotation_axis) > 0.0 {
            let rotation_angle = angle_between_vectors(md, best_direction);
            rotation = [rotation_axis[0] * rotation_angle, rotation_axis[1] * rotation_angle, rotation_axis[2] * rotation_angle];
        }
        emergency = pin((2.0 - movement_approximate_weight) * 0.5 - 0.5, 0.0, 1.0);
        emergency = max(emergency, emergency_scale);
        direction_chosen = true;
    } else if forward_dot < 0.5 {
        if weight_difference > 1.3 {
            let direction_dot = dot(directions[best], local_movement_direction);
            if direction_dot > 0.5 {
                emergency = pin(weight_difference / 1.3 - 0.5, 0.0, 1.0);
                emergency = max(emergency, emergency_scale);
                let mut rotation_angle = emergency * third_pi;
                if local_movement_direction[2] * directions[best][1] - local_movement_direction[1] * directions[best][2] > 0.0 {
                    rotation_angle = -rotation_angle;
                }
                let f = avoidance.forward;
                rotation = [f[0] * rotation_angle, f[1] * rotation_angle, f[2] * rotation_angle];
                direction_chosen = true;
                m.set_i16(debug(dbg::FIELD_653C), 4);
                m.set_f32(debug(dbg::SIGN_ROTATED), rotation_angle);
            } else {
                m.set_f32(debug(dbg::SIGN_TOO_FAR_COSANGLE), direction_dot);
                m.set_i16(debug(dbg::FIELD_653C), 3);
            }
        } else {
            m.set_i16(debug(dbg::FIELD_653C), 2);
        }
    } else if maximum_sense_emergency > 0.0 {
        let mut rotation_angle = 0.0f32;
        emergency = emergency_scale;
        rotation = [0.0; 3];
        let perpendicular_j = -directions[best][2];
        for k in 0..3 {
            rotation[k] += perpendicular_j * avoidance.left[k];
        }
        let perpendicular_k = directions[best][1];
        for k in 0..3 {
            rotation[k] += perpendicular_k * avoidance.up[k];
        }
        if normalize3(&mut rotation) > 0.0 {
            rotation_angle = emergency * third_pi;
            rotation = [rotation[0] * rotation_angle, rotation[1] * rotation_angle, rotation[2] * rotation_angle];
        }
        m.set_f32(debug(dbg::FIELD_6520), rotation_angle);
        direction_chosen = true;
        m.set_i16(debug(dbg::FIELD_653C), 1);
        m.set_f32(debug(dbg::FIELD_651C), maximum_sense_emergency);
    } else {
        m.set_i16(debug(dbg::FIELD_653C), 0);
    }

    m.set_bool(debug(dbg::FIELD_6550), direction_chosen);
    m.set_i16(
        at(a::CONTROL_VECTOR_AVOIDANCE_CURRENT_DIRECTION),
        if direction_chosen { best as i16 } else { NONE as i16 },
    );
    m.set_f32(debug(dbg::FIELD_654C), emergency);
    set_vector(m, debug(dbg::FIELD_6540), rotation);

    (rotation, emergency)
}
