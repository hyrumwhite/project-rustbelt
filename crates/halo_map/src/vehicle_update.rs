//! Halo's vehicle update (ROADMAP phase 7, objects and physics):
//! `vehicle_update` in OpenCE's `source/units/vehicles.c`, up to its
//! animation, ported as it is, in the same single-precision arithmetic.
//!
//! Each tick a vehicle steers toward where its driver faces, rights itself
//! when upended, and moves its speed, slide and turn toward the driver's
//! throttle; its type's control code (the tank's treads, the jeep's
//! wheels, the boat's rudder and roll, the plane's lift and attitude, the
//! scout's hover, the fighter's flight, the turret's none) turns those into
//! its powered mass points' controls and a magic force and torque for its
//! physics ([`crate::vehicle_physics`]); then its suspensions follow the
//! ground, it counts its airborne and on-ground ticks, and planes and
//! fighters are held between the structure's floor and ceiling. A vehicle
//! at rest coasts to a stop instead.
//!
//! What has no bearing on its state is the game's, through a
//! [`VehicleUpdateHost`] in a running game (none when replaying): its
//! effects (slipping, crashing, the Ghost's and Pelican's thrusters), its
//! suspension sound, and the move of a vehicle coasting to a stop. Falling
//! damage to what it carries, its animation and its blur region follow.

use crate::collision::{World, transform_point, transform_vector};
use crate::features::{cross3, dot3};
use crate::model::Matrix4x3;
use crate::vehicle_physics::{
    MassPoint, PhysicsDefinition, PoweredMassPoint, VehiclePhysicsInputs, VehiclePhysicsState, VehiclePhysicsWorld, physics_update,
};
use crate::{CacheFile, Result, layout};

use std::f32::consts::PI;

const GLOBAL_GRAVITY: f32 = 0.0035651792;
const REAL_EPSILON: f32 = 0.0001;
/// `VEHICLE_ANGULAR_ACCELERATION`: 0.8 degrees in radians.
const VEHICLE_ANGULAR_ACCELERATION: f32 = 0.0139626344;

// `_vehicle_type`s
const TANK: i16 = 0;
const JEEP: i16 = 1;
const BOAT: i16 = 2;
const PLANE: i16 = 3;
const SCOUT: i16 = 4;
const FIGHTER: i16 = 5;
const TURRET: i16 = 6;

// object flags
const OBJECT_AT_REST: u32 = 1 << 5;
const OBJECT_OUTSIDE_OF_MAP: u32 = 1 << 21;
const OBJECT_NO_COLLISIONS: u32 = 1 << 24;
// unit control flags
const CONTROL_CROUCH_MODIFIER: u32 = 1 << 0;
const CONTROL_JUMP: u32 = 1 << 1;
// mass point flags
const POINT_ON_GROUND: u32 = 1 << 1;
const POINT_ANTIGRAVING: u32 = 1 << 4;

/// `physics_variable_speed_parameters`.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct SpeedParameters {
    pub positive_scale: f32,
    pub negative_scale: f32,
    pub acceleration: f32,
    pub deceleration: f32,
}

/// A suspension of the vehicle's graph: the mass point it follows, its
/// animation (NONE for none) and its travel.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Suspension {
    pub mass_point_index: i16,
    pub animation_index: i16,
    pub unknown4: f32,
    pub unknown8: f32,
}

/// A vehicle definition, what its update reads.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct VehicleDefinition {
    pub flags: u32,
    pub vehicle_type: i16,
    /// Its speed (`unknown2f8`: forward and reverse speeds, acceleration,
    /// deceleration).
    pub speed: SpeedParameters,
    /// Its turn limits (degrees; `unknown308`, `unknown30c`).
    pub turn_limits: [f32; 2],
    pub wheel_circumference: f32,
    pub unknown314: f32,
    pub unknown318: f32,
    /// Its slide (`unknown330` and what follows it).
    pub slide: SpeedParameters,
    pub unknown340: f32,
    pub unknown344: f32,
    pub unknown364: f32,
    pub has_physics: bool,
    /// Its graph's first vehicle animations' suspensions, if it has a graph
    /// with vehicle animations.
    pub suspensions: Option<Vec<Suspension>>,
}

impl VehicleDefinition {
    pub fn load(cache: &CacheFile, datum: u32) -> Result<Self> {
        use layout::{vehicle_animation as a, vehicle_definition as v, vehicle_suspension as s};
        let root = cache.tag_root(datum, crate::groups::OBJECT)?;
        let speed = |at: usize| -> Result<SpeedParameters> {
            Ok(SpeedParameters { positive_scale: root.f32(at)?, negative_scale: root.f32(at + 4)?, acceleration: root.f32(at + 8)?, deceleration: root.f32(at + 12)? })
        };
        let mut suspensions = None;
        if let Some(g) = root.reference(layout::_object_definition::ANIMATION_GRAPH)?.datum {
            let graph = cache.tag_root(g, crate::groups::ANIMATION_GRAPH)?;
            if let Some(first) = graph.block(layout::animation_graph::VEHICLE_ANIMATIONS, a::SIZEOF)?.iter().next() {
                suspensions = Some(
                    first
                        .block(a::SUSPENSIONS, s::SIZEOF)?
                        .iter()
                        .map(|e| {
                            Ok(Suspension {
                                mass_point_index: e.i16(s::MASS_POINT_INDEX)?,
                                animation_index: e.i16(s::ANIMATION_INDEX)?,
                                unknown4: e.f32(s::UNKNOWN4)?,
                                unknown8: e.f32(s::UNKNOWN8)?,
                            })
                        })
                        .collect::<Result<_>>()?,
                );
            }
        }
        Ok(VehicleDefinition {
            flags: root.u32(v::FLAGS)?,
            vehicle_type: root.i16(v::VEHICLE_TYPE)?,
            speed: speed(v::UNKNOWN2F8)?,
            turn_limits: [root.f32(v::UNKNOWN308)?, root.f32(v::UNKNOWN30C)?],
            wheel_circumference: root.f32(v::WHEEL_CIRCUMFERENCE)?,
            unknown314: root.f32(v::UNKNOWN314)?,
            unknown318: root.f32(v::UNKNOWN318)?,
            slide: speed(v::UNKNOWN330)?,
            unknown340: root.f32(v::UNKNOWN340)?,
            unknown344: root.f32(v::UNKNOWN344)?,
            unknown364: root.f32(v::UNKNOWN364)?,
            has_physics: root.reference(layout::_object_definition::PHYSICS)?.datum.is_some(),
            suspensions,
        })
    }
}

/// A vehicle's own datum (`_vehicle_datum`), laid out as the game's (88
/// bytes).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct VehicleDatum {
    pub flags: u16,
    pub stop_time: i16,
    pub airborne_ticks: u8,
    pub upending_type: u8,
    pub upending_ticks: u8,
    pub on_ground_ticks: u8,
    pub speed: f32,
    pub slide: f32,
    pub turn: f32,
    pub wheel: f32,
    pub left_tread: f32,
    pub right_tread: f32,
    pub hover: f32,
    pub thrust: f32,
    pub suspension: [u8; 8],
    pub hover_position: [f32; 3],
    pub collision_force: [f32; 3],
    pub collision_torque: [f32; 3],
    pub stuck_mass_point_flags: u32,
}

const _: () = assert!(std::mem::size_of::<VehicleDatum>() == 88);

/// What a vehicle update reads and changes of the vehicle.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct VehicleUpdateState {
    pub position: [f32; 3],
    pub forward: [f32; 3],
    pub up: [f32; 3],
    pub translational_velocity: [f32; 3],
    pub angular_velocity: [f32; 3],
    pub object_flags: u32,
    pub location_leaf: i32,
    pub location_cluster: i16,
    pub vehicle: VehicleDatum,
}

/// What a vehicle update reads besides its state.
pub struct VehicleUpdateInputs<'a> {
    pub index: u32,
    pub parented: bool,
    pub throttle: [f32; 3],
    pub control_flags: u32,
    pub desired_facing: [f32; 3],
    pub seat_power: [f32; 2],
    pub driven_by_ai: bool,
    /// The structure's vehicle floor and ceiling (0 for none).
    pub vehicle_floor: f32,
    pub vehicle_ceiling: f32,
    /// The bounding sphere centre the vehicle's new pose gives it (where
    /// it finds its location when it moves by `object_set_position`).
    pub bounding_center_after: [f32; 3],
    /// The powered mass points' controls as the game's stack held them
    /// before the control code set what it sets (the physics reads only
    /// what its flags allow).
    pub powered_base: &'a [PoweredMassPoint],
}

/// The world a vehicle update tests: its physics' world (the objects and
/// vehicles its physics met) and the world its suspensions test; and, in a
/// running game, the host that makes its effects and sounds.
pub struct VehicleUpdateWorld<'a> {
    pub physics: Option<VehiclePhysicsWorld<'a>>,
    pub world: &'a World<'a>,
    pub host: Option<&'a dyn VehicleUpdateHost>,
}

/// A running game, as a vehicle update calls it. The state is the update's
/// as it stands (the host writes it to the vehicle first and reads back
/// what the call changed).
pub trait VehicleUpdateHost {
    /// A vehicle coasting to a stop moves (`object_set_position`).
    fn set_position(&self, state: &mut VehicleUpdateState, position: [f32; 3], forward: [f32; 3], up: [f32; 3]);
    /// Its suspensions moved, the most by `maximum_shift`
    /// (`update_suspension`'s sound): whether the sound played.
    fn suspension_shift(&self, state: &mut VehicleUpdateState, maximum_shift: f32) -> bool;
    /// `create_slipping_effects` from its mass points.
    fn slipping_effects(&self, state: &mut VehicleUpdateState, mass_points: &[MassPoint]);
    /// `create_crashing_effects`: its velocity's change since
    /// `previous_velocity`, where its mass points touch the ground.
    fn crashing_effects(&self, state: &mut VehicleUpdateState, previous_velocity: [f32; 3], mass_points: &[MassPoint]);
    /// Its thrusters' effect (`create_pelican_effect`, `create_ghost_effect`).
    fn thruster_effect(&self, state: &mut VehicleUpdateState, pelican: bool);
}

/// What the control code handed the physics, and what came of it.
#[derive(Debug, Clone, Default)]
pub struct VehicleUpdateOutcome {
    /// Whether the physics ran, with what powered mass points (none for
    /// none) and magic force and torque.
    pub physics_ran: bool,
    pub powered: Option<Vec<PoweredMassPoint>>,
    pub magic: Option<([f32; 3], [f32; 3])>,
    pub mass_points: Vec<MassPoint>,
    /// Whether it coasted to a stop instead, was upended or rode a parent.
    pub stopping: bool,
    pub upended: bool,
}

// ---------- the game's maths

/// `PIN`.
fn pin(n: f32, a: f32, b: f32) -> f32 {
    if n < a {
        a
    } else if n > b {
        b
    } else {
        n
    }
}
/// `MIN`, `MAX`, as the macros compare.
fn min(a: f32, b: f32) -> f32 {
    if a > b { b } else { a }
}
fn max(a: f32, b: f32) -> f32 {
    if a > b { a } else { b }
}
fn sine(a: f32) -> f32 {
    (a as f64).sin() as f32
}
fn cosine(a: f32) -> f32 {
    (a as f64).cos() as f32
}
fn arctangent(y: f32, x: f32) -> f32 {
    (y as f64).atan2(x as f64) as f32
}
/// `square_root`: the double square root of a float.
fn square_root(x: f32) -> f32 {
    (x as f64).sqrt() as f32
}
fn add3(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn sub3(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn scale3(v: [f32; 3], c: f32) -> [f32; 3] {
    [c * v[0], c * v[1], c * v[2]]
}
fn magnitude_squared3(v: [f32; 3]) -> f32 {
    v[0] * v[0] + v[1] * v[1] + v[2] * v[2]
}
fn magnitude3(v: [f32; 3]) -> f32 {
    magnitude_squared3(v).sqrt()
}
fn normalize3(v: &mut [f32; 3]) -> f32 {
    crate::features::normalize3(v)
}
/// `point_from_line3d`: `v * t + p`.
fn point_from_line(p: [f32; 3], v: [f32; 3], t: f32) -> [f32; 3] {
    [v[0] * t + p[0], v[1] * t + p[1], v[2] * t + p[2]]
}
/// `DEGREES_TO_RADIANS`.
fn degrees_to_radians(angle: f32) -> f32 {
    angle * PI / 180.0
}

/// `physics_variable_speed_update`.
fn speed_update(speed: &mut f32, p: &SpeedParameters, delta: f32) {
    let magnitude = (delta as f64).abs() as f32;
    let acceleration = magnitude * p.acceleration;
    let deceleration = magnitude * p.deceleration;
    if delta > 0.0 {
        if *speed <= -deceleration {
            *speed += deceleration;
        } else if *speed >= 0.0 {
            *speed += acceleration;
        } else {
            *speed = (*speed / deceleration + 1.0) * acceleration;
        }
        *speed = min(*speed, magnitude * p.positive_scale);
        return;
    }
    if delta < 0.0 {
        if *speed >= deceleration {
            *speed -= deceleration;
        } else if *speed <= 0.0 {
            *speed -= acceleration;
        } else {
            *speed = (*speed / deceleration - 1.0) * acceleration;
        }
        *speed = max(-magnitude * p.negative_scale, *speed);
    }
}

/// `physics_variable_speed_update_seek`.
fn speed_update_seek(speed: &mut f32, p: &SpeedParameters, target: f32, delta: f32) {
    if *speed > target {
        speed_update(speed, p, -delta);
        if *speed <= target {
            *speed = target;
        }
    } else if *speed < target {
        speed_update(speed, p, delta);
        if *speed >= target {
            *speed = target;
        }
    }
}

/// `physics_variable_position_get_seek_direction` (not wrapping).
fn seek_direction(position: f32, target: f32) -> f32 {
    let direction = target - position;
    if direction != 0.0 { if direction > 0.0 { 1.0 } else { -1.0 } } else { direction }
}

/// `physics_variable_position_update_seek` (not wrapping).
fn position_update_seek(position: &mut f32, limits: [f32; 2], target: f32, delta: f32) {
    let direction = seek_direction(*position, target);
    if direction != 0.0 {
        // (physics_variable_position_update)
        *position += direction * delta;
        if *position < limits[1] {
            *position = limits[1];
        } else if *position > limits[0] {
            *position = limits[0];
        }
        if seek_direction(*position, target) == direction {
            return;
        }
    }
    *position = target;
}

/// `limit3d`.
fn limit3d(v: [f32; 3], length: f32) -> [f32; 3] {
    let dot = v[0] * v[0] + v[1] * v[1] + v[2] * v[2];
    if dot > length * length { scale3(v, length * (1.0 / dot.sqrt())) } else { v }
}

/// `yaw_vectors`: `forward` turned about `up`.
fn yaw_vectors(forward: &mut [f32; 3], up: [f32; 3], sine: f32, cosine: f32) {
    let cross = cross3(up, *forward);
    for c in 0..3 {
        forward[c] = forward[c] * cosine + cross[c] * sine;
    }
}

/// `pitch_vectors`.
fn pitch_vectors(forward: &mut [f32; 3], up: &mut [f32; 3], sine: f32, cosine: f32) {
    let backward = [-forward[0], -forward[1], -forward[2]];
    for c in 0..3 {
        forward[c] = forward[c] * cosine + up[c] * sine;
    }
    for c in 0..3 {
        up[c] = up[c] * cosine + backward[c] * sine;
    }
}

/// `rotate_vector_about_axis`.
fn rotate_vector_about_axis(v: [f32; 3], n: [f32; 3], sine: f32, cosine: f32) -> [f32; 3] {
    let one_minus_cosine_times_v_dot_n = dot3(n, v) * (1.0 - cosine);
    let v_cross_n = [n[2] * v[1] - v[2] * n[1], n[0] * v[2] - v[0] * n[2], n[1] * v[0] - v[1] * n[0]];
    [
        cosine * v[0] + one_minus_cosine_times_v_dot_n * n[0] - sine * v_cross_n[0],
        cosine * v[1] + one_minus_cosine_times_v_dot_n * n[1] - sine * v_cross_n[1],
        cosine * v[2] + one_minus_cosine_times_v_dot_n * n[2] - sine * v_cross_n[2],
    ]
}

/// `angle_between_vectors3d`.
fn angle_between_vectors3d(a: [f32; 3], b: [f32; 3]) -> f32 {
    let aa_bb = (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]) * (b[0] * b[0] + b[1] * b[1] + b[2] * b[2]);
    if aa_bb == 0.0 {
        return 0.0;
    }
    let ab = dot3(a, b);
    let c = pin((ab / aa_bb * ab) + (ab / aa_bb * ab) - 1.0, -1.0, 1.0);
    let angle = ((c as f64).acos() as f32) * 0.5;
    if ab < 0.0 { PI - angle } else { angle }
}

type Rows = [[f32; 3]; 3];

/// `matrix4x3_rotation_to_quaternion` (and the 3x3 one, `trace_first`
/// adding the diagonal from `n[1][1]`): i, j, k, w.
fn rotation_to_quaternion(n: &Rows, three_by_three: bool) -> [f32; 4] {
    let trace = if three_by_three { n[1][1] + n[0][0] + n[2][2] } else { n[0][0] + n[1][1] + n[2][2] };
    if trace > 0.0 {
        let mut s = square_root(trace + 1.0);
        let w = 0.5 * s;
        s = 0.5 / s;
        [(n[2][1] - n[1][2]) * s, (n[0][2] - n[2][0]) * s, (n[1][0] - n[0][1]) * s, w]
    } else {
        const NEXT: [usize; 3] = [1, 2, 0];
        let mut i = 0usize;
        if n[1][1] > n[0][0] {
            i = 1;
        }
        if n[2][2] > n[i][i] {
            i = 2;
        }
        let j = NEXT[i];
        let k = NEXT[j];
        let mut s = square_root(n[i][i] - (n[k][k] + n[j][j]) + 1.0);
        let mut q = [0.0f32; 3];
        q[i] = s * 0.5;
        if s != 0.0 {
            s = 0.5 / s;
        }
        q[j] = (n[i][j] + n[j][i]) * s;
        q[k] = (n[i][k] + n[k][i]) * s;
        [q[0], q[1], q[2], (n[k][j] - n[j][k]) * s]
    }
}

/// `quaternion_to_angle_and_vector`: (angle, axis).
fn quaternion_to_angle_and_vector(q: [f32; 4]) -> (f32, [f32; 3]) {
    let mut v = [q[0], q[1], q[2]];
    let magnitude = normalize3(&mut v);
    let mut a = 2.0 * arctangent(magnitude, q[3]);
    if a > PI {
        v = [-v[0], -v[1], -v[2]];
        a = PI * 2.0 - a;
    }
    (a, v)
}

/// `matrix3x3_multiply`, summed in the game's order.
fn matrix3_multiply(a: &Rows, b: &Rows) -> Rows {
    let (af, al, au) = (a[0], a[1], a[2]);
    let (bf, bl, bu) = (b[0], b[1], b[2]);
    [
        [
            bf[0] * af[0] + bf[1] * al[0] + au[0] * bf[2],
            bf[1] * al[1] + bf[0] * af[1] + bf[2] * au[1],
            bf[1] * al[2] + bf[0] * af[2] + bf[2] * au[2],
        ],
        [
            bl[1] * al[0] + af[0] * bl[0] + au[0] * bl[2],
            bl[2] * au[1] + bl[1] * al[1] + bl[0] * af[1],
            bl[2] * au[2] + bl[1] * al[2] + bl[0] * af[2],
        ],
        [
            bu[0] * af[0] + bu[1] * al[0] + bu[2] * au[0],
            bu[2] * au[1] + bu[1] * al[1] + bu[0] * af[1],
            bu[2] * au[2] + bu[1] * al[2] + bu[0] * af[2],
        ],
    ]
}

/// `matrix4x3_rotation_from_vectors`.
fn rotation_from_vectors(forward: [f32; 3], up: [f32; 3]) -> Matrix4x3 {
    Matrix4x3 { scale: 1.0, forward, left: cross3(up, forward), up, position: [0.0; 3] }
}

/// The attitude torque the plane and the old fighter steer by: the
/// rotation from the vehicle's frame to the desired one, as an angular
/// velocity (`angle_scale` of the angle a tick), less the angular velocity,
/// scaled by `scale`.
fn attitude_torque(state: &VehicleUpdateState, desired_forward: [f32; 3], desired_up: [f32; 3], angle_scale: f32, scale: f32) -> [f32; 3] {
    let vehicle_rotation = rotation_from_vectors(state.forward, state.up);
    let desired_rotation = crate::collision::matrix_inverse(&rotation_from_vectors(desired_forward, desired_up));
    let rotation = crate::pose::matrix_multiply(&vehicle_rotation, &desired_rotation);
    let quaternion = rotation_to_quaternion(&[rotation.forward, rotation.left, rotation.up], false);
    let (angle, axis) = quaternion_to_angle_and_vector(quaternion);
    let scaled = scale3(axis, angle * angle_scale);
    [
        (scaled[0] - state.angular_velocity[0]) * scale,
        (scaled[1] - state.angular_velocity[1]) * scale,
        (scaled[2] - state.angular_velocity[2]) * scale,
    ]
}

/// `compute_acceleration`.
fn compute_acceleration(a: [f32; 3], b: [f32; 3], maximum: f32, minimum: f32) -> [f32; 3] {
    let mut result = sub3(a, b);
    result[2] += GLOBAL_GRAVITY;
    let dot = dot3(a, result);
    let maximum = if dot > REAL_EPSILON {
        (maximum - minimum) * ((dot * dot / magnitude_squared3(result)) / magnitude_squared3(a)) + minimum
    } else {
        minimum
    };
    limit3d(result, maximum)
}

/// `normalize2d`.
fn normalize2(v: &mut [f32; 2]) -> f32 {
    let magnitude = (v[0] * v[0] + v[1] * v[1]).sqrt();
    if !(REAL_EPSILON > magnitude.abs()) {
        let s = 1.0 / magnitude;
        *v = [s * v[0], s * v[1]];
        magnitude
    } else {
        0.0
    }
}

fn dot2(a: [f32; 2], b: [f32; 2]) -> f32 {
    a[0] * b[0] + a[1] * b[1]
}

fn sign(x: f32) -> f32 {
    if x != 0.0 { if x < 0.0 { -1.0 } else { 1.0 } } else { 0.0 }
}

/// An identity rotation (i, j, k, w).
const IDENTITY: [f32; 4] = [0.0, 0.0, 0.0, 1.0];

// ---------- the update

/// The physics step a vehicle's control code asks for.
struct PhysicsCall {
    powered: Option<Vec<PoweredMassPoint>>,
    magic: Option<([f32; 3], [f32; 3])>,
}

/// `vehicle_update`, up to its animation.
pub fn vehicle_update(
    definition: &VehicleDefinition,
    physics: Option<&PhysicsDefinition>,
    state: &mut VehicleUpdateState,
    inputs: &VehicleUpdateInputs<'_>,
    env: &VehicleUpdateWorld<'_>,
) -> VehicleUpdateOutcome {
    let mut outcome = VehicleUpdateOutcome::default();
    let v = &mut state.vehicle;
    if inputs.parented {
        state.angular_velocity = [0.0; 3];
        state.translational_velocity = [0.0; 3];
        state.object_flags &= !OBJECT_AT_REST;
        return outcome;
    }
    let set16 = |flags: &mut u16, bit: u16, on: bool| if on { *flags |= 1 << bit } else { *flags &= !(1 << bit) };
    set16(&mut v.flags, 2, inputs.control_flags & CONTROL_CROUCH_MODIFIER != 0);
    set16(
        &mut v.flags,
        3,
        inputs.control_flags & CONTROL_JUMP != 0
            || (definition.flags & (1 << 4) != 0
                && ((inputs.throttle[0] > 0.0 && v.speed < 0.0) || (inputs.throttle[0] < 0.0 && v.speed > 0.0))),
    );
    let left = cross3(state.up, state.forward);
    let steering_angle = arctangent(dot3(left, inputs.desired_facing), dot3(inputs.desired_facing, state.forward));

    // righting itself when upended
    if v.flags & (1 << 4) != 0 && v.upending_type != 0 && v.upending_ticks < 30 && state.up[2] <= 0.9 {
        outcome.upended = true;
        let mut torque: f32 = if v.upending_type == 2 || v.upending_type == 4 { 0.3 } else { -0.3 };
        let mut torque_axis =
            if v.upending_type == 4 || v.upending_type == 3 { cross3(state.forward, state.up) } else { state.forward };
        let mut roll = state.up[2] * -2.0;
        if roll < definition.unknown340 {
            roll = definition.unknown340;
        } else if roll > definition.unknown344 {
            roll = definition.unknown344;
        }
        torque *= roll;
        state.object_flags &= !OBJECT_AT_REST;
        if v.upending_type == 2 || v.upending_type == 1 {
            let cross = cross3(state.forward, state.up);
            torque_axis = point_from_line(torque_axis, cross, -state.forward[2]);
        }
        state.angular_velocity = scale3(torque_axis, torque);
        match definition.vehicle_type {
            FIGHTER => state.translational_velocity[2] = min(-0.01, state.translational_velocity[2]),
            TANK => state.translational_velocity = scale3(state.forward, dot3(state.forward, state.translational_velocity)),
            _ => {}
        }
        v.upending_ticks = v.upending_ticks.wrapping_add(1);
    } else {
        v.upending_ticks = 0;
        v.upending_type = 0;
        v.flags &= !(1 << 4);
    }

    // speed, slide and turn
    if v.flags & (1 << 3) != 0 {
        speed_update_seek(&mut v.speed, &definition.speed, 0.0, 1.0);
    } else {
        speed_update_seek(&mut v.speed, &definition.speed, inputs.throttle[0], 1.0);
        speed_update_seek(&mut v.slide, &definition.slide, inputs.throttle[1], 1.0);
    }
    if definition.vehicle_type != TANK {
        let mut desired_position = if v.speed < 0.0 { -steering_angle } else { steering_angle };
        if desired_position < degrees_to_radians(definition.turn_limits[1]) {
            desired_position = degrees_to_radians(definition.turn_limits[1]);
        } else if desired_position > degrees_to_radians(definition.turn_limits[0]) {
            desired_position = degrees_to_radians(definition.turn_limits[0]);
        }
        position_update_seek(&mut v.turn, definition.turn_limits, desired_position, degrees_to_radians(definition.unknown314) * (1.0 / 30.0));
    } else if v.speed == 0.0 {
        speed_update_seek(&mut v.turn, &definition.speed, 0.0, 1.0);
    } else {
        speed_update_seek(&mut v.turn, &definition.speed, pin(steering_angle * 0.63661975, -1.0, 1.0) * definition.speed.positive_scale, 2.0);
    }

    if definition.has_physics
        && ((definition.flags & 1 != 0 && v.speed != 0.0)
            || (definition.flags & 2 != 0 && v.turn != 0.0)
            || (definition.flags & 4 != 0 && inputs.seat_power[0] != 0.0)
            || (definition.flags & 8 != 0 && inputs.seat_power[1] != 0.0)
            || (definition.flags & 32 != 0 && v.slide != 0.0))
    {
        state.object_flags &= !OBJECT_AT_REST;
    }

    let fits = physics.is_some_and(|p| p.mass_points.len() <= 32 && p.powered_mass_points.len() <= 32);
    if let Some(physics) = physics.filter(|_| definition.has_physics && state.object_flags & OBJECT_AT_REST == 0 && fits) {
        let previous_velocity = state.translational_velocity;
        let mut mass_points: Option<Vec<MassPoint>> = None;
        let powered_count = physics.powered_mass_points.len();
        let base = |count: usize| -> Vec<PoweredMassPoint> {
            (0..count).map(|i| inputs.powered_base.get(i).copied().unwrap_or_default()).collect()
        };
        let none = PhysicsCall { powered: None, magic: None };
        match definition.vehicle_type {
            TANK => {
                let v = &mut state.vehicle;
                let left = v.speed - v.turn;
                let right = v.turn + v.speed;
                v.left_tread = left + v.left_tread;
                v.left_tread = (v.left_tread as f64 % definition.wheel_circumference as f64) as f32;
                if v.left_tread < 0.0 {
                    v.left_tread += definition.wheel_circumference;
                }
                v.right_tread = right + v.right_tread;
                v.right_tread = (v.right_tread as f64 % definition.wheel_circumference as f64) as f32;
                if v.right_tread < 0.0 {
                    v.right_tread += definition.wheel_circumference;
                }
                if powered_count == 2 {
                    let mut p = base(2);
                    p[0].ground_friction_velocity = left;
                    p[0].rotation = IDENTITY;
                    p[1].ground_friction_velocity = right;
                    p[1].rotation = IDENTITY;
                    run(&mut mass_points, physics, inputs, env, state, PhysicsCall { powered: Some(p), magic: None }, &mut outcome);
                } else {
                    run(&mut mass_points, physics, inputs, env, state, none, &mut outcome);
                }
            }
            JEEP => {
                let v = &mut state.vehicle;
                v.wheel = v.speed + v.wheel;
                v.wheel = (v.wheel as f64 % definition.wheel_circumference as f64) as f32;
                if v.wheel < 0.0 {
                    v.wheel += definition.wheel_circumference;
                }
                if powered_count == 2 {
                    let angle = v.turn * 0.5;
                    let (cs, sn) = (cosine(angle), sine(angle));
                    let mut p = base(2);
                    p[0].ground_friction_velocity = v.speed;
                    p[0].rotation = [0.0, 0.0, sn, cs];
                    p[1].ground_friction_velocity = v.speed;
                    p[1].rotation = [0.0, 0.0, -sn, cs];
                    run(&mut mass_points, physics, inputs, env, state, PhysicsCall { powered: Some(p), magic: None }, &mut outcome);
                } else {
                    run(&mut mass_points, physics, inputs, env, state, none, &mut outcome);
                }
            }
            BOAT => {
                if powered_count == 3 {
                    let call = boat_control(physics, state, base(3));
                    run(&mut mass_points, physics, inputs, env, state, call, &mut outcome);
                } else {
                    run(&mut mass_points, physics, inputs, env, state, none, &mut outcome);
                }
            }
            PLANE => {
                if state.vehicle.flags & (1 << 1) != 0 {
                    mass_points = Some(vec![MassPoint::default(); physics.mass_points.len()]);
                } else {
                    let call = plane_control(definition, physics, state, inputs);
                    run(&mut mass_points, physics, inputs, env, state, call, &mut outcome);
                }
                if let Some(host) = env.host {
                    host.thruster_effect(state, true);
                }
            }
            SCOUT => {
                let water_depth = env.world.clusters.water_depth(state.location_cluster, state.position);
                let call = scout_control(definition, physics, state, inputs, steering_angle, water_depth, base(powered_count));
                run(&mut mass_points, physics, inputs, env, state, call, &mut outcome);
                // (the hover, by how many powered mass points antigrav)
                let points = mass_points.as_deref().unwrap_or(&[]);
                let maximum = max(state.up[2], 0.4);
                let (mut powered, mut grounded) = (0i32, 0i32);
                for (definition, point) in physics.mass_points.iter().zip(points) {
                    if definition.powered_mass_point_index != -1 {
                        powered += 1;
                        if point.flags & POINT_ANTIGRAVING != 0 {
                            grounded += 1;
                        }
                    }
                }
                let ratio = if powered as i16 > 0 { grounded as i16 as f32 / powered as i16 as f32 } else { 0.0 };
                let mut target = pin(ratio * maximum, 0.0, 1.0);
                let hover = state.vehicle.hover;
                if target - hover > 0.1 {
                    target = hover + 0.1;
                } else if target - hover < -0.1 {
                    target = hover - 0.1;
                }
                state.vehicle.hover = target;
                if let Some(host) = env.host {
                    host.thruster_effect(state, false);
                }
            }
            FIGHTER => {
                let call = if powered_count != 2 {
                    none
                } else if physics.radius > 0.0 {
                    fighter_control_old(definition, physics, state, inputs, base(2))
                } else {
                    fighter_control_new(definition, physics, state, inputs, base(2))
                };
                run(&mut mass_points, physics, inputs, env, state, call, &mut outcome);
                if let Some(host) = env.host {
                    host.thruster_effect(state, false);
                }
            }
            TURRET => run(&mut mass_points, physics, inputs, env, state, none, &mut outcome),
            _ => {}
        }
        let mass_points = mass_points.unwrap_or_default();

        if let Some(host) = env.host {
            host.slipping_effects(state, &mass_points);
        }
        let maximum_shift = update_suspension(definition, Some(physics), state, inputs.index, env.world);
        if let Some(host) = env.host
            && !host.suspension_shift(state, maximum_shift)
        {
            host.crashing_effects(state, previous_velocity, &mass_points);
        }
        compute_airborne_ticks(physics, &mut state.vehicle, &mass_points);
        if state.object_flags & OBJECT_AT_REST != 0 {
            state.vehicle.stop_time = 15;
        }
        if state.object_flags & OBJECT_NO_COLLISIONS == 0 && (definition.vehicle_type == PLANE || definition.vehicle_type == FIGHTER) {
            let (floor, ceiling) = (inputs.vehicle_floor, inputs.vehicle_ceiling);
            if floor != 0.0 && state.position[2] < floor {
                state.translational_velocity[2] +=
                    ((floor - state.position[2]) * 0.015625 - state.translational_velocity[2] * 0.0625) * inputs.seat_power[0];
            }
            if ceiling != 0.0 && state.position[2] > ceiling {
                state.translational_velocity[2] -=
                    ((state.position[2] - ceiling) * 0.015625 + state.translational_velocity[2] * 0.0625) * inputs.seat_power[0];
            }
        }
        outcome.mass_points = mass_points;
    } else if state.vehicle.stop_time > 0 {
        outcome.stopping = true;
        slowly_stop(state, inputs, env);
        let maximum_shift = update_suspension(definition, physics, state, inputs.index, env.world);
        if let Some(host) = env.host {
            host.suspension_shift(state, maximum_shift);
        }
    }
    outcome
}

/// Runs the vehicle's physics with what its control code gave it
/// (`physics_update`), and copies back what it changed.
fn run(
    mass_points: &mut Option<Vec<MassPoint>>,
    physics: &PhysicsDefinition,
    inputs: &VehicleUpdateInputs<'_>,
    env: &VehicleUpdateWorld<'_>,
    state: &mut VehicleUpdateState,
    call: PhysicsCall,
    outcome: &mut VehicleUpdateOutcome,
) {
    let Some(physics_env) = &env.physics else { return };
    let mut physics_state = VehiclePhysicsState {
        position: state.position,
        forward: state.forward,
        up: state.up,
        translational_velocity: state.translational_velocity,
        angular_velocity: state.angular_velocity,
        object_flags: state.object_flags,
        location_leaf: state.location_leaf,
        location_cluster: state.location_cluster,
        collision_force: state.vehicle.collision_force,
        collision_torque: state.vehicle.collision_torque,
        stuck_mass_point_flags: state.vehicle.stuck_mass_point_flags,
    };
    let physics_inputs =
        VehiclePhysicsInputs { index: inputs.index, powered: call.powered.as_deref(), magic: call.magic, bounding_center_after: inputs.bounding_center_after };
    let result = physics_update(physics, &mut physics_state, &physics_inputs, physics_env);
    state.position = physics_state.position;
    state.forward = physics_state.forward;
    state.up = physics_state.up;
    state.translational_velocity = physics_state.translational_velocity;
    state.angular_velocity = physics_state.angular_velocity;
    state.object_flags = physics_state.object_flags;
    state.location_leaf = physics_state.location_leaf;
    state.location_cluster = physics_state.location_cluster;
    state.vehicle.collision_force = physics_state.collision_force;
    state.vehicle.collision_torque = physics_state.collision_torque;
    state.vehicle.stuck_mass_point_flags = physics_state.stuck_mass_point_flags;
    outcome.physics_ran = true;
    outcome.powered = call.powered;
    outcome.magic = call.magic;
    *mass_points = Some(result.mass_points);
}

/// `compute_airborne_ticks`.
fn compute_airborne_ticks(physics: &PhysicsDefinition, v: &mut VehicleDatum, mass_points: &[MassPoint]) {
    if v.airborne_ticks < 0xff {
        v.airborne_ticks += 1;
    }
    for index in 0..physics.mass_points.len() {
        let flags = mass_points.get(index).map_or(0, |m| m.flags);
        if flags & POINT_ON_GROUND != 0 {
            v.airborne_ticks = 0;
            if v.on_ground_ticks < 0xff {
                v.on_ground_ticks += 1;
            }
            return;
        }
        if flags & POINT_ANTIGRAVING != 0 {
            v.airborne_ticks = 0;
        }
    }
    v.on_ground_ticks = 0;
}

/// `slowly_stop_vehicle`: a vehicle at rest coasting to a stop.
fn slowly_stop(state: &mut VehicleUpdateState, inputs: &VehicleUpdateInputs<'_>, env: &VehicleUpdateWorld<'_>) {
    let world = env.world;
    state.vehicle.stop_time -= 1;
    state.translational_velocity = state.translational_velocity.map(|c| c * 0.835);
    state.angular_velocity = state.angular_velocity.map(|c| c * 0.835);
    let position = add3(state.translational_velocity, state.position);
    let mut axis = state.angular_velocity;
    let magnitude = normalize3(&mut axis);
    let (forward, up) = if magnitude != 0.0 {
        let rotation = crate::vehicle_physics::rotation_from_axis_and_angle(axis, sine(magnitude), cosine(magnitude));
        (transform_vector(&rotation, state.forward), transform_vector(&rotation, state.up))
    } else {
        (state.forward, state.up)
    };
    if state.vehicle.stop_time == 0 {
        state.translational_velocity = [0.0; 3];
        state.angular_velocity = [0.0; 3];
    }
    if let Some(host) = env.host {
        host.set_position(state, position, forward, up);
        return;
    }
    // (object_set_position: its location from the new pose's bounding
    // sphere centre, else its position)
    state.position = position;
    state.forward = forward;
    state.up = up;
    let mut location = world.clusters.location_from_point(world.collision, inputs.bounding_center_after);
    if location.1 == -1 {
        location = world.clusters.location_from_point(world.collision, position);
    }
    if location.1 != -1 {
        (state.location_leaf, state.location_cluster) = location;
        state.object_flags &= !OBJECT_OUTSIDE_OF_MAP;
    } else {
        state.object_flags |= OBJECT_OUTSIDE_OF_MAP;
    }
}

/// `update_suspension`: each suspension's travel, from a ray along its
/// mass point's up from the top of its travel. Returns the most one moved
/// (what its sound is played at).
fn update_suspension(
    definition: &VehicleDefinition,
    physics: Option<&PhysicsDefinition>,
    state: &mut VehicleUpdateState,
    vehicle_index: u32,
    world: &World<'_>,
) -> f32 {
    let mut maximum_shift = 0.0f32;
    let (Some(suspensions), Some(physics)) = (&definition.suspensions, physics) else { return maximum_shift };
    let matrix = Matrix4x3 { scale: 1.0, forward: state.forward, left: cross3(state.up, state.forward), up: state.up, position: state.position };
    for (index, suspension) in suspensions.iter().take(8).enumerate() {
        if suspension.mass_point_index < 0 || suspension.mass_point_index as usize >= physics.mass_points.len() || suspension.animation_index == -1 {
            continue;
        }
        let mass_point = &physics.mass_points[suspension.mass_point_index as usize];
        let byte = state.vehicle.suspension[index];
        let current = if byte == 0xff { 1.0 } else { byte as f32 * (1.0 / 255.0) };
        let point = transform_point(&matrix, mass_point.position);
        let n = mass_point.up;
        let normal = [n[0] * matrix.forward[0] + n[1] * matrix.left[0] + n[2] * matrix.up[0], n[0] * matrix.forward[1] + n[1] * matrix.left[1] + n[2] * matrix.up[1], n[0] * matrix.forward[2] + n[1] * matrix.left[2] + n[2] * matrix.up[2]];
        let extent = suspension.unknown4 - suspension.unknown8;
        let offset = (suspension.unknown8 - physics.center_of_mass[2]) - extent;
        let start = point_from_line(point, normal, offset);
        let vector = scale3(normal, extent + extent);
        let collision = world.test(0xc0a0, start, vector, vehicle_index);
        let shift = pin((1.0 - collision.t) + (1.0 - collision.t), 0.0, 1.0);
        if shift - current > maximum_shift {
            maximum_shift = shift - current;
        }
        // (quantize_real_to_byte_lower_bound(0, 1, ...))
        state.vehicle.suspension[index] = (((shift + current) * 0.5 / 1.0) * 255.0) as i32 as u8;
    }
    maximum_shift
}

/// `update_human_boat_physics`' controls: the rudder, its water lift, and
/// the torque that rolls it level against its turn.
fn boat_control(physics: &PhysicsDefinition, state: &mut VehicleUpdateState, mut p: Vec<PoweredMassPoint>) -> PhysicsCall {
    let v = &state.vehicle;
    let speed = ((magnitude3(state.translational_velocity) * 2.5) as f64).abs() as f32;
    let maximum_angle = v.turn * 0.5;
    let mut angle = 1.0 - min(speed, 1.0);
    p[0].water_friction_velocity = v.speed;
    p[0].water_lift_ratio = 0.003;
    angle *= maximum_angle;
    p[0].rotation = [0.0, 0.0, sine(angle), cosine(angle)];
    p[1].water_lift_ratio = 0.003;
    p[1].rotation = IDENTITY;
    p[2].water_lift_ratio = 0.005;
    p[2].rotation = IDENTITY;
    let f = state.forward;
    let negative_k = -f[2];
    let mut up_relative = [f[0] * negative_k + 0.0, f[1] * negative_k + 0.0, f[2] * negative_k + 1.0];
    let thrust = if normalize3(&mut up_relative) != 0.0 {
        // (vehicle_cross_product3d_target(up, forward))
        let (a, b) = (state.up, state.forward);
        let cross = [b[2] * a[1] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], b[1] * a[0] - a[1] * b[0]];
        // (vehicle_triple_product3d_target(velocity, forward, up))
        let tv = state.translational_velocity;
        let c = [tv[2] * f[1] - tv[1] * f[2], f[2] * tv[0] - tv[2] * f[0], tv[1] * f[0] - f[1] * tv[0]];
        let n = [0.0f32, 0.0, 1.0];
        let j_term = c[1] * n[1];
        let triple = c[0] * n[0] + j_term + c[2] * n[2];
        let spin = triple * 2.0 * PI;
        up_relative = rotate_vector_about_axis(up_relative, f, sine(spin), cosine(spin));
        let mut angle = angle_between_vectors3d(up_relative, state.up);
        if dot3(cross, up_relative) > 0.0 {
            angle = -angle;
        }
        let av = state.angular_velocity;
        let dot = av[0] * f[0] + av[1] * f[1] + av[2] * f[2];
        let root = square_root(((angle as f64).abs() as f32 as f64 * 2.0 * VEHICLE_ANGULAR_ACCELERATION as f64) as f32);
        scale3(f, pin(root * sign(angle) - dot, -VEHICLE_ANGULAR_ACCELERATION, VEHICLE_ANGULAR_ACCELERATION) * physics.xx_moment)
    } else {
        [0.0; 3]
    };
    PhysicsCall { powered: Some(p), magic: Some(([0.0; 3], thrust)) }
}

/// `update_human_plane_physics`' controls: hover and thrust, lift and
/// drive, and the torque toward the attitude the driver faces.
fn plane_control(definition: &VehicleDefinition, physics: &PhysicsDefinition, state: &mut VehicleUpdateState, inputs: &VehicleUpdateInputs<'_>) -> PhysicsCall {
    let maximum_speed = definition.speed.positive_scale;
    let v = &mut state.vehicle;
    let mut throttle = pin(v.speed, 0.0, maximum_speed) / maximum_speed;
    throttle = throttle * throttle;
    let mut factor = if v.flags & (1 << 2) == 0 { if v.flags & (1 << 3) != 0 { 1.0 } else { 0.75 } } else { 0.25 };
    factor *= 1.0 - throttle;
    // (interpolate_scalar)
    let difference = factor * inputs.seat_power[0] - v.hover;
    v.hover += pin(difference, -0.05, 0.05);
    v.thrust = throttle * inputs.seat_power[0];
    let mut desired_forward = inputs.desired_facing;
    let df = desired_forward;
    let mut desired_up = [-(df[2] * df[0]), -(df[2] * df[1]), 1.0 - df[2] * df[2]];
    if normalize3(&mut desired_up) == 0.0 {
        desired_up = [1.0, 0.0, 0.0];
    }
    let dot = dot3(state.translational_velocity, state.forward);
    let drive = (v.speed - dot) * v.thrust * physics.mass * 0.05;
    let mut lift = ((dot / maximum_speed) as f64).abs() as f32 * 1.05 + v.hover * 1.3;
    lift *= GLOBAL_GRAVITY;
    lift *= physics.mass;
    let (u, f) = (state.up, state.forward);
    let force = [lift * u[0] + drive * f[0], lift * u[1] + drive * f[1], lift * u[2] + drive * f[2]];
    let velocity = [state.translational_velocity[0], state.translational_velocity[1]];
    let yaw = (velocity[1] * desired_forward[0] - velocity[0] * desired_forward[1]) * (PI / 2.0) / (maximum_speed as f64).abs() as f32;
    yaw_vectors(&mut desired_up, desired_forward, sine(yaw), cosine(yaw));
    let _ = &mut desired_forward;
    let scale = physics.radius * physics.radius * physics.mass * 0.05;
    let torque = attitude_torque(state, desired_forward, desired_up, 1.0 / 30.0, scale);
    let seat = inputs.seat_power[0];
    PhysicsCall { powered: None, magic: Some((scale3(force, seat), scale3(torque, seat))) }
}

/// `update_alien_fighter_physics_old`'s controls.
fn fighter_control_old(
    definition: &VehicleDefinition,
    physics: &PhysicsDefinition,
    state: &mut VehicleUpdateState,
    inputs: &VehicleUpdateInputs<'_>,
    mut p: Vec<PoweredMassPoint>,
) -> PhysicsCall {
    let facing = inputs.desired_facing;
    let mut perpendicular = [-facing[2] * facing[0], -(facing[2] * facing[1]), 1.0 - facing[2] * facing[2]];
    if normalize3(&mut perpendicular) == 0.0 {
        perpendicular = [1.0, 0.0, 0.0];
    }
    let speed = dot3(state.forward, state.translational_velocity);
    let thrust = (state.vehicle.speed - speed) * physics.mass * 0.05;
    let lift = (((speed / definition.speed.positive_scale) as f64).abs() as f32 * physics.mass) * GLOBAL_GRAVITY * 1.05;
    let (u, f) = (state.up, state.forward);
    let force = [lift * u[0] + thrust * f[0], lift * u[1] + thrust * f[1], lift * u[2] + thrust * f[2]];
    let velocity = [state.translational_velocity[0], state.translational_velocity[1]];
    let yaw = (facing[0] * velocity[1] - facing[1] * velocity[0]) * (PI * 0.5) / (definition.speed.positive_scale as f64).abs() as f32;
    yaw_vectors(&mut perpendicular, facing, sine(yaw), cosine(yaw));
    let scale = physics.radius * physics.radius * physics.mass * 0.05;
    let torque = attitude_torque(state, facing, perpendicular, 4.0 / 30.0, scale);
    for point in p.iter_mut() {
        point.antigrav_fraction = inputs.seat_power[0];
        point.rotation = IDENTITY;
    }
    let seat = inputs.seat_power[0];
    PhysicsCall { powered: Some(p), magic: Some((scale3(force, seat), scale3(torque, seat))) }
}

/// `update_alien_fighter_physics_new`'s controls (and its thrust).
fn fighter_control_new(
    definition: &VehicleDefinition,
    physics: &PhysicsDefinition,
    state: &mut VehicleUpdateState,
    inputs: &VehicleUpdateInputs<'_>,
    mut p: Vec<PoweredMassPoint>,
) -> PhysicsCall {
    let v = &state.vehicle;
    let desired_velocity = scale3(state.forward, v.speed);
    let throttle = if v.speed > 0.0 { v.speed / definition.speed.positive_scale } else { -(v.speed / definition.speed.negative_scale) };
    let acceleration = compute_acceleration(
        desired_velocity,
        state.translational_velocity,
        throttle * definition.speed.acceleration,
        throttle * definition.speed.deceleration,
    );
    let magic_force = scale3(scale3(acceleration, physics.mass), inputs.seat_power[0]);

    let current: Rows = [state.forward, cross3(state.up, state.forward), state.up];
    let mut desired_forward = inputs.desired_facing;
    let mut desired_up = point_from_line([0.0, 0.0, 1.0], desired_forward, -desired_forward[2]);
    if normalize3(&mut desired_up) == 0.0 {
        desired_up = [1.0, 0.0, 0.0];
    }
    if !inputs.driven_by_ai {
        pitch_vectors(&mut desired_forward, &mut desired_up, sine(definition.unknown364), cosine(definition.unknown364));
    }
    let tv = state.translational_velocity;
    let yaw = (desired_forward[0] * tv[1] - desired_forward[1] * tv[0]) / definition.speed.positive_scale * definition.turn_limits[0];
    yaw_vectors(&mut desired_up, desired_forward, sine(yaw), cosine(yaw));
    let desired: Rows = [desired_forward, cross3(desired_up, desired_forward), desired_up];
    let current_transposed: Rows = [
        [current[0][0], current[1][0], current[2][0]],
        [current[0][1], current[1][1], current[2][1]],
        [current[0][2], current[1][2], current[2][2]],
    ];
    let rotation = matrix3_multiply(&desired, &current_transposed);
    let quaternion = rotation_to_quaternion(&rotation, true);
    let (angle, axis) = quaternion_to_angle_and_vector(quaternion);
    let desired_angular_velocity = scale3(axis, (-angle) * definition.unknown314 * (1.0 / PI));
    let angular_acceleration = sub3(desired_angular_velocity, state.angular_velocity);
    let magic_torque =
        scale3(scale3(angular_acceleration, (physics.zz_moment + physics.yy_moment + physics.xx_moment) * (1.0 / 3.0)), inputs.seat_power[0]);

    let spin = magnitude3(state.angular_velocity) / definition.unknown314;
    let thrust = state.vehicle.thrust;
    let thrust_delta = if spin > thrust {
        min(spin - thrust, pin((1.0 - thrust) * (1.0 - thrust) * 0.2, 0.01, 0.05))
    } else {
        max(spin - thrust, -max(thrust * thrust * 0.05, 0.005))
    };
    state.vehicle.thrust += thrust_delta;
    for point in p.iter_mut() {
        point.antigrav_fraction = inputs.seat_power[0];
        point.rotation = IDENTITY;
    }
    PhysicsCall { powered: Some(p), magic: Some((magic_force, magic_torque)) }
}

/// `update_alien_scout_physics`' controls: the hover's push toward the
/// throttle's velocity and its turn toward the steering, and the torque
/// that levels it in the air; all scaled by its antigrav.
#[allow(clippy::too_many_arguments)]
fn scout_control(
    definition: &VehicleDefinition,
    physics: &PhysicsDefinition,
    state: &mut VehicleUpdateState,
    inputs: &VehicleUpdateInputs<'_>,
    steering: f32,
    water_depth: f32,
    mut p: Vec<PoweredMassPoint>,
) -> PhysicsCall {
    let mut magic_force = [0.0f32; 3];
    let mut magic_torque = [0.0f32; 3];
    let antigrav = inputs.seat_power[0];
    for point in p.iter_mut() {
        point.antigrav_fraction = antigrav;
        point.rotation = IDENTITY;
    }
    let v = state.vehicle;
    let (forward, up, angular_velocity) = (state.forward, state.up, state.angular_velocity);
    let add = |total: &mut [f32; 3], v: [f32; 3], s: f32| {
        total[0] += v[0] * s;
        total[1] += v[1] * s;
        total[2] += v[2] * s;
    };
    if water_depth < 0.5 && up[2] > -0.2 {
        let vehicle_matrix = Matrix4x3 { scale: 1.0, forward, left: cross3(up, forward), up, position: state.position };
        // (matrix4x3_inverse_transform_vector)
        let tv = state.translational_velocity;
        let m = &vehicle_matrix;
        let local_velocity = [
            tv[0] * m.forward[0] + tv[1] * m.forward[1] + tv[2] * m.forward[2],
            tv[0] * m.left[0] + tv[1] * m.left[1] + tv[2] * m.left[2],
            tv[0] * m.up[0] + tv[1] * m.up[1] + tv[2] * m.up[2],
        ];
        if v.hover > 0.0 {
            let mut maximum_speed = definition.speed.positive_scale;
            if v.flags & (1 << 3) != 0 {
                maximum_speed *= 0.8;
            }
            let mut maximum_acceleration = definition.speed.acceleration;
            let target = [maximum_speed * inputs.throttle[0], maximum_speed * inputs.throttle[1]];
            let mut acceleration = [target[0] - local_velocity[0], target[1] - local_velocity[1], 0.0];
            if v.on_ground_ticks > 0 && (steering as f64).abs() > 0.785398185f32 as f64 {
                let reduction = min(v.on_ground_ticks as f32 * 0.05, 0.98);
                maximum_acceleration *= 1.0 - reduction;
            }
            acceleration = limit3d(acceleration, maximum_acceleration);
            acceleration = transform_vector(&vehicle_matrix, acceleration);
            let scale = physics.mass * v.hover;
            add(&mut magic_force, acceleration, scale);
        }
        if v.hover > 0.0 {
            let current = dot3(up, angular_velocity);
            let mut desired = square_root(((steering as f64).abs() * 0.0069813174f32 as f64) as f32) * sign(steering);
            if (desired as f64).abs() > REAL_EPSILON as f64 && steering / desired < 2.0 {
                desired = steering * 0.5;
            }
            let error = pin(desired - current, -0.0034906587, 0.0034906587);
            let mut torque = error * physics.zz_moment;
            torque *= v.hover;
            add(&mut magic_torque, up, torque);
        }
        if v.hover < 1.0 {
            // (vehicle_cross_product3d_target(up, forward))
            let (a, b) = (up, forward);
            let left = [b[2] * a[1] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], b[1] * a[0] - a[1] * b[0]];
            let mut forward2d = [forward[0], forward[1]];
            let mut left2d = [left[0], left[1]];
            normalize2(&mut forward2d);
            normalize2(&mut left2d);
            let mut control_torque = [0.0f32; 2];
            let (mut torque_a, mut torque_b);
            if up[2] > 0.0 {
                let up2 = [up[0], up[1]];
                let av2 = [angular_velocity[0], angular_velocity[1]];
                let alignment = [dot2(up2, forward2d), dot2(up2, left2d)];
                let rate = [dot2(av2, left2d), -dot2(av2, forward2d)];
                let mut level_torque = [0.0f32; 2];
                level_torque[0] -= alignment[0];
                level_torque[1] -= alignment[1];
                level_torque[0] -= 15.0 * rate[0];
                level_torque[1] -= 15.0 * rate[1];
                let weight_a = (level_torque[0] as f64).abs() as f32 * sign(inputs.throttle[0] * level_torque[0]);
                let weight_b = (level_torque[1] as f64).abs() as f32 * sign(inputs.throttle[1] * level_torque[1]);
                control_torque[0] += inputs.throttle[0] * pin(weight_a + 1.0, 0.3, 2.5) * 0.0015514038;
                control_torque[1] += inputs.throttle[1] * pin(weight_b + 1.0, 0.3, 2.5) * 0.0015514038;
                let level_scale = (1.0 - up[2]) * 0.0038785094;
                torque_a = level_scale * level_torque[0];
                torque_b = level_scale * level_torque[1];
                torque_a += control_torque[0];
            } else {
                torque_a = inputs.throttle[0] * 0.0015514038;
                torque_b = inputs.throttle[1] * 0.0015514038;
                torque_a += control_torque[0];
            }
            torque_b += control_torque[1];
            let mut torque = [0.0f32; 3];
            let left_scale = physics.yy_moment * torque_a;
            let forward_scale = -(physics.xx_moment * torque_b);
            let scale = 1.0 - v.hover;
            add(&mut torque, left, left_scale);
            add(&mut torque, forward, forward_scale);
            add(&mut magic_torque, torque, scale);
        }
        if v.flags & (1 << 3) != 0 {
            let speed = pin(dot3(forward, state.translational_velocity) / definition.speed.positive_scale, 0.0, 1.0);
            let left = cross3(up, forward);
            if speed > 0.0 {
                let torque = physics.yy_moment * speed * v.hover * -0.005817764;
                add(&mut magic_torque, left, torque);
                let lift = physics.mass * speed * v.hover * 0.004;
                add(&mut magic_force, [0.0, 0.0, 1.0], lift);
            }
            if v.airborne_ticks > 0 {
                let mut axis = cross3(left, [0.0, 0.0, 1.0]);
                if normalize3(&mut axis) > 0.0 {
                    let fade = pin(1.0 - v.airborne_ticks as f32 * (1.0 / 30.0), 0.0, 1.0);
                    let axis_scale = (1.0 - v.hover) * physics.mass * fade * 0.002;
                    add(&mut magic_force, axis, axis_scale);
                    let up_scale = (1.0 - v.hover) * physics.mass * fade * 0.001;
                    add(&mut magic_force, [0.0, 0.0, 1.0], up_scale);
                }
            }
        }
        magic_force = magic_force.map(|c| c * antigrav);
        magic_torque = magic_torque.map(|c| c * antigrav);
    }
    PhysicsCall { powered: Some(p), magic: Some((magic_force, magic_torque)) }
}

/// The game's flat physics placement for a coasting vehicle's move; the
/// structure's vehicle floor and ceiling (`structure_bsp`).
pub fn structure_vehicle_bounds(bsp: &crate::bsp::Bsp<'_>) -> Result<(f32, f32)> {
    Ok((bsp.root.f32(layout::structure_bsp::VEHICLE_FLOOR)?, bsp.root.f32(layout::structure_bsp::VEHICLE_CEILING)?))
}
