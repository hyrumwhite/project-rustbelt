//! Halo's vehicle physics shapes (ROADMAP phase 7, objects and physics): a
//! physics tag's mass points, the spheres a vehicle's simulation moves, as
//! the ray test sees them with `_collision_test_use_vehicle_physics_bit`
//! (`physics_instance_new` and `physics_test_vector` in OpenCE's
//! `source/physics/physics.c`, with `sphere_test_vector3d` from
//! `source/math/real_math.c`), ported as they are, in the same
//! single-precision arithmetic.
//!
//! The game itself sets that bit only for debugging
//! (`collision_debug_flag_use_vehicle_physics`).
//!
//! And a vehicle's simulation on them (`physics_update`, the new physics:
//! `physics_compute_new`, `physics_update_new`, and the collision force
//! other vehicles' mass points push it with): each mass point's ground
//! plane (`collision_features_test_point` on the features around it), its
//! ground, water and air friction, water pressure, lift, thrust and
//! antigrav, then the integration of the total force and torque through the
//! inverse inertia, and the freeze that stops the mass points sweeping into
//! the world; and the old physics, of a physics with a radius
//! (`physics_update_old`), which most vehicles take. Its vehicle code (what
//! sets the powered mass points and the magic force) is not ported yet.

use crate::collision::transform_point;
use crate::features::{cross3, dot3};
use crate::model::Matrix4x3;
use crate::{CacheFile, Result, layout};

/// `MAXIMUM_MASS_POINTS_PER_PHYSICS`.
const MAXIMUM_MASS_POINTS: usize = 32;

/// A mass point's sphere, in the physics' space.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct MassPointShape {
    pub position: [f32; 3],
    pub radius: f32,
}

/// A physics tag, as its shape: its centre of mass and its mass points (no
/// more than the game keeps, `physics_mass_point_count`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PhysicsShape {
    pub center_of_mass: [f32; 3],
    pub mass_points: Vec<MassPointShape>,
}

impl PhysicsShape {
    pub fn load(cache: &CacheFile, datum: u32) -> Result<Self> {
        use layout::{mass_point_definition as m, physics_definition as p};
        let root = cache.tag_root(datum, crate::groups::PHYSICS)?;
        let mass_points = root
            .block(p::MASS_POINTS, m::SIZEOF)?
            .iter()
            .take(MAXIMUM_MASS_POINTS)
            .map(|point| Ok(MassPointShape { position: point.vec3(m::POSITION)?, radius: point.f32(m::RADIUS)? }))
            .collect::<Result<_>>()?;
        Ok(PhysicsShape { center_of_mass: root.vec3(p::CENTER_OF_MASS)?, mass_points })
    }

    /// The physics of an object definition, if it has one.
    pub fn load_for_object(cache: &CacheFile, definition: u32) -> Result<Option<Self>> {
        let root = cache.tag_root(definition, crate::groups::OBJECT)?;
        match root.reference(layout::_object_definition::PHYSICS)?.datum {
            Some(physics) => Ok(Some(Self::load(cache, physics)?)),
            None => Ok(None),
        }
    }
}

/// A physics shape placed in the world (`physics_instance`): its frame has
/// the object's axes and its origin moved back by the centre of mass.
#[derive(Debug, Clone)]
pub struct PhysicsInstance {
    pub shape: std::sync::Arc<PhysicsShape>,
    pub world_matrix: Matrix4x3,
}

/// `physics_instance_new`, from the object's origin and orientation
/// (`object_get_origin`, `object_get_orientation`: through its parent's
/// node for an object with a parent).
pub fn physics_instance_new(shape: std::sync::Arc<PhysicsShape>, origin: [f32; 3], forward: [f32; 3], up: [f32; 3]) -> PhysicsInstance {
    let mut world_matrix = Matrix4x3 { scale: 1.0, forward, left: cross3(up, forward), up, position: origin };
    let c = shape.center_of_mass;
    world_matrix.position = transform_point(&world_matrix, [-c[0], -c[1], -c[2]]);
    PhysicsInstance { shape, world_matrix }
}

/// `matrix4x3_inverse_transform_point`.
fn inverse_transform_point(m: &Matrix4x3, p: [f32; 3]) -> [f32; 3] {
    if m.scale == 0.0 {
        return [0.0; 3];
    }
    let (mut x, mut y, mut z) = (p[0] - m.position[0], p[1] - m.position[1], p[2] - m.position[2]);
    if m.scale != 1.0 {
        let scale = 1.0 / m.scale;
        x *= scale;
        y *= scale;
        z *= scale;
    }
    [
        x * m.forward[0] + y * m.forward[1] + z * m.forward[2],
        x * m.left[0] + y * m.left[1] + z * m.left[2],
        x * m.up[0] + y * m.up[1] + z * m.up[2],
    ]
}

/// `matrix4x3_inverse_transform_vector`.
fn inverse_transform_vector(m: &Matrix4x3, v: [f32; 3]) -> [f32; 3] {
    let [mut i, mut j, mut k] = v;
    if m.scale != 1.0 {
        let scale = 1.0 / m.scale;
        i *= scale;
        j *= scale;
        k *= scale;
    }
    [
        i * m.forward[0] + j * m.forward[1] + k * m.forward[2],
        i * m.left[0] + j * m.left[1] + k * m.left[2],
        i * m.up[0] + j * m.up[1] + k * m.up[2],
    ]
}

/// `sphere_test_vector3d`: where a ray first meets a sphere it starts
/// outside of and heads into (or 0, inside it), and the normal there.
fn sphere_test_vector3d(center: [f32; 3], radius: f32, point: [f32; 3], vector: [f32; 3]) -> Option<(f32, [f32; 3])> {
    let x = [point[0] - center[0], point[1] - center[1], point[2] - center[2]];
    let xv = dot3(x, vector);
    if !(xv < 0.0) {
        return None;
    }
    let xx = x[0] * x[0] + x[1] * x[1] + x[2] * x[2];
    let xx_rr = xx - radius * radius;
    if xx_rr <= 0.0 {
        // (reciprocal_square_root)
        let scale = 1.0 / xx.sqrt();
        return Some((0.0, [scale * x[0], scale * x[1], scale * x[2]]));
    }
    let d = vector[0] * vector[0] + vector[1] * vector[1] + vector[2] * vector[2];
    let vv = xv * xv - d * xx_rr;
    if !(vv >= 0.0) {
        return None;
    }
    let n = -(vv.sqrt() + xv);
    if !(n <= d) {
        return None;
    }
    let t = n / d;
    // (point_from_line3d, then fast_normalize3d)
    let mut normal = [vector[0] * t + x[0], vector[1] * t + x[1], vector[2] * t + x[2]];
    let magnitude_squared = normal[0] * normal[0] + normal[1] * normal[1] + normal[2] * normal[2];
    if magnitude_squared != 0.0 {
        let scale = 1.0 / magnitude_squared.sqrt();
        normal = [scale * normal[0], scale * normal[1], scale * normal[2]];
    }
    Some((t, normal))
}

impl PhysicsInstance {
    /// `physics_test_vector`: the first mass point a ray meets, as its t
    /// along the vector and the world plane there.
    pub fn test_vector(&self, point: [f32; 3], vector: [f32; 3]) -> Option<(f32, [f32; 4])> {
        let local_point = inverse_transform_point(&self.world_matrix, point);
        let local_vector = inverse_transform_vector(&self.world_matrix, vector);
        let mut best: Option<(f32, [f32; 4])> = None;
        let mut best_t = f32::MAX;
        for m in &self.shape.mass_points {
            if let Some((t, normal)) = sphere_test_vector3d(m.position, m.radius, local_point, local_vector)
                && best_t > t
            {
                best_t = t;
                let intersection = [local_vector[0] * t + local_point[0], local_vector[1] * t + local_point[1], local_vector[2] * t + local_point[2]];
                // (plane3d_from_point_and_normal)
                best = Some((t, [normal[0], normal[1], normal[2], dot3(intersection, normal)]));
            }
        }
        best.map(|(t, plane)| (t, crate::collision::transform_plane(&self.world_matrix, plane)))
    }
}

// ---------- the simulation (physics_update)

/// `MAXIMUM_POWERED_MASS_POINTS_PER_PHYSICS`.
const MAXIMUM_POWERED_MASS_POINTS: usize = 32;
/// `global_gravity`, `global_physics_collision_depth`.
const GLOBAL_GRAVITY: f32 = 0.0035651792;
const GLOBAL_PHYSICS_COLLISION_DEPTH: f32 = 0.2;
/// `depths_of_hell`: the ground plane of a mass point touching nothing.
const DEPTHS_OF_HELL: [f32; 4] = [0.0, 0.0, 1.0, -256.0];
/// `_collision_test_for_bipeds_dead_flags`, `_collision_test_for_vehicles_flags`.
const COLLISION_TEST_FOR_BIPEDS_DEAD: u32 = 0xc0a0;
const COLLISION_TEST_FOR_VEHICLES: u32 = 0xc0a0;
const NONE: i16 = -1;

// `_point_` flags
pub mod point_flags {
    pub const AT_REST: u32 = 1 << 0;
    pub const ON_GROUND: u32 = 1 << 1;
    pub const ON_VOLATILE_SURFACE: u32 = 1 << 2;
    pub const IN_WATER: u32 = 1 << 3;
    pub const ANTIGRAVING: u32 = 1 << 4;
}

// `_powered_mass_point_` flags
const POWERED_GROUND_FRICTION: u32 = 1 << 0;
const POWERED_WATER_FRICTION: u32 = 1 << 1;
const POWERED_AIR_FRICTION: u32 = 1 << 2;
const POWERED_WATER_LIFT: u32 = 1 << 3;
const POWERED_AIR_LIFT: u32 = 1 << 4;
const POWERED_THRUST: u32 = 1 << 5;
const POWERED_ANTIGRAV: u32 = 1 << 6;

// object flags the update sets
pub mod object_flags {
    pub const ON_GROUND: u32 = 1 << 1;
    pub const ON_MEDIA: u32 = 1 << 2;
    pub const PARTIALLY_UNDER_MEDIA: u32 = 1 << 3;
    pub const WHOLLY_UNDER_MEDIA: u32 = 1 << 4;
    pub const AT_REST: u32 = 1 << 5;
    pub const OUTSIDE_OF_MAP: u32 = 1 << 21;
}
const OBJECT_TYPE_SCENERY: i16 = 6;
/// `_material_ice`.
const MATERIAL_ICE: i16 = 31;

/// A mass point's definition, what the simulation reads.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct MassPointDefinition {
    pub powered_mass_point_index: i16,
    pub mass: f32,
    pub density: f32,
    pub position: [f32; 3],
    pub forward: [f32; 3],
    pub up: [f32; 3],
    pub friction_type: i16,
    pub friction_parallel_scale: f32,
    pub friction_perpendicular_scale: f32,
    pub radius: f32,
}

/// A powered mass point's definition.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct PoweredMassPointDefinition {
    pub flags: u32,
    pub antigrav_strength: f32,
    pub antigrav_height: f32,
    pub antigrav_damp_fraction: f32,
    pub antigrav_normal_k1: f32,
    pub antigrav_normal_k0: f32,
}

/// A physics tag, what a vehicle's simulation reads (no more mass points
/// or powered ones than the game keeps).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PhysicsDefinition {
    /// A radius: the old physics (`physics_update_old`).
    pub radius: f32,
    pub moment: f32,
    pub mass: f32,
    pub center_of_mass: [f32; 3],
    pub gravity_scale: f32,
    pub ground_friction: f32,
    pub ground_depth: f32,
    pub ground_damp_fraction: f32,
    pub ground_normal_k1: f32,
    pub ground_normal_k0: f32,
    pub water_friction: f32,
    pub water_depth: f32,
    pub water_density: f32,
    pub air_friction: f32,
    pub xx_moment: f32,
    pub yy_moment: f32,
    pub zz_moment: f32,
    /// The inertia and inverse inertia matrices (forward, left, up rows).
    pub inertial_matrices: Vec<[[f32; 3]; 3]>,
    pub powered_mass_points: Vec<PoweredMassPointDefinition>,
    pub mass_points: Vec<MassPointDefinition>,
}

impl PhysicsDefinition {
    pub fn load(cache: &CacheFile, datum: u32) -> Result<Self> {
        use layout::{mass_point_definition as m, physics_definition as p, powered_mass_point_definition as w};
        let root = cache.tag_root(datum, crate::groups::PHYSICS)?;
        let inertial_matrices = root
            .block(p::INERTIAL_MATRIX, 36)?
            .iter()
            .map(|e| Ok([e.vec3(0)?, e.vec3(12)?, e.vec3(24)?]))
            .collect::<Result<_>>()?;
        let powered_mass_points = root
            .block(p::POWERED_MASS_POINTS, w::SIZEOF)?
            .iter()
            .take(MAXIMUM_POWERED_MASS_POINTS)
            .map(|e| {
                Ok(PoweredMassPointDefinition {
                    flags: e.u32(w::FLAGS)?,
                    antigrav_strength: e.f32(w::ANTIGRAV_STRENGTH)?,
                    antigrav_height: e.f32(w::ANTIGRAV_HEIGHT)?,
                    antigrav_damp_fraction: e.f32(w::ANTIGRAV_DAMP_FRACTION)?,
                    antigrav_normal_k1: e.f32(w::ANTIGRAV_NORMAL_K1)?,
                    antigrav_normal_k0: e.f32(w::ANTIGRAV_NORMAL_K0)?,
                })
            })
            .collect::<Result<_>>()?;
        let mass_points = root
            .block(p::MASS_POINTS, m::SIZEOF)?
            .iter()
            .take(MAXIMUM_MASS_POINTS)
            .map(|e| {
                Ok(MassPointDefinition {
                    powered_mass_point_index: e.i16(m::POWERED_MASS_POINT_INDEX)?,
                    mass: e.f32(m::MASS)?,
                    density: e.f32(m::DENSITY)?,
                    position: e.vec3(m::POSITION)?,
                    forward: e.vec3(m::FORWARD)?,
                    up: e.vec3(m::UP)?,
                    friction_type: e.i16(m::FRICTION_TYPE)?,
                    friction_parallel_scale: e.f32(m::FRICTION_PARALLEL_SCALE)?,
                    friction_perpendicular_scale: e.f32(m::FRICTION_PERPENDICULAR_SCALE)?,
                    radius: e.f32(m::RADIUS)?,
                })
            })
            .collect::<Result<_>>()?;
        Ok(PhysicsDefinition {
            radius: root.f32(p::RADIUS)?,
            moment: root.f32(p::MOMENT)?,
            mass: root.f32(p::MASS)?,
            center_of_mass: root.vec3(p::CENTER_OF_MASS)?,
            gravity_scale: root.f32(p::GRAVITY_SCALE)?,
            ground_friction: root.f32(p::GROUND_FRICTION)?,
            ground_depth: root.f32(p::GROUND_DEPTH)?,
            ground_damp_fraction: root.f32(p::GROUND_DAMP_FRACTION)?,
            ground_normal_k1: root.f32(p::GROUND_NORMAL_K1)?,
            ground_normal_k0: root.f32(p::GROUND_NORMAL_K0)?,
            water_friction: root.f32(p::WATER_FRICTION)?,
            water_depth: root.f32(p::WATER_DEPTH)?,
            water_density: root.f32(p::WATER_DENSITY)?,
            air_friction: root.f32(p::AIR_FRICTION)?,
            xx_moment: root.f32(p::XX_MOMENT)?,
            yy_moment: root.f32(p::YY_MOMENT)?,
            zz_moment: root.f32(p::ZZ_MOMENT)?,
            inertial_matrices,
            powered_mass_points,
            mass_points,
        })
    }

    /// The physics of an object definition, if it has one.
    pub fn load_for_object(cache: &CacheFile, definition: u32) -> Result<Option<Self>> {
        let root = cache.tag_root(definition, crate::groups::OBJECT)?;
        match root.reference(layout::_object_definition::PHYSICS)?.datum {
            Some(physics) => Ok(Some(Self::load(cache, physics)?)),
            None => Ok(None),
        }
    }

    /// Its shape, as the ray test and collisions see it.
    pub fn shape(&self) -> PhysicsShape {
        PhysicsShape {
            center_of_mass: self.center_of_mass,
            mass_points: self.mass_points.iter().map(|m| MassPointShape { position: m.position, radius: m.radius }).collect(),
        }
    }

    /// `physics_powered_mass_point_index`: NONE for one the physics doesn't
    /// have.
    fn powered_index(&self, mass_point: &MassPointDefinition) -> Option<usize> {
        let index = mass_point.powered_mass_point_index;
        (index >= 0 && (index as usize) < self.powered_mass_points.len()).then_some(index as usize)
    }
}

/// A material type's physics scales (the globals' `material_definition`;
/// all 0, scaling nothing, for a type the globals don't have).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct MaterialPhysics {
    pub ground_friction_scale: f32,
    pub ground_friction_normal_k1_scale: f32,
    pub ground_friction_normal_k0_scale: f32,
    pub ground_depth_scale: f32,
    pub ground_damp_fraction_scale: f32,
}

/// The globals' (`globals\globals`) materials' physics scales.
pub fn load_material_physics(cache: &CacheFile) -> Result<Vec<MaterialPhysics>> {
    use layout::material_definition as m;
    let globals = cache.find_tag(crate::groups::GLOBALS, "globals\\globals").ok_or_else(|| crate::Error::BadTag("no globals\\globals".into()))?;
    let root = cache.tag_root(globals.datum, crate::groups::GLOBALS)?;
    root.block(layout::game_globals::MATERIALS, m::SIZEOF)?
        .iter()
        .map(|e| {
            Ok(MaterialPhysics {
                ground_friction_scale: e.f32(m::PHYSICS_GROUND_FRICTION_SCALE)?,
                ground_friction_normal_k1_scale: e.f32(m::PHYSICS_GROUND_FRICTION_NORMAL_K1_SCALE)?,
                ground_friction_normal_k0_scale: e.f32(m::PHYSICS_GROUND_FRICTION_NORMAL_K0_SCALE)?,
                ground_depth_scale: e.f32(m::PHYSICS_GROUND_DEPTH_SCALE)?,
                ground_damp_fraction_scale: e.f32(m::PHYSICS_GROUND_DAMP_FRACTION_SCALE)?,
            })
        })
        .collect()
}

/// A powered mass point's controls, as the vehicle's code sets them
/// (`powered_mass_point_datum`).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct PoweredMassPoint {
    pub ground_friction_velocity: f32,
    pub water_friction_velocity: f32,
    pub air_friction_velocity: f32,
    pub water_lift_ratio: f32,
    pub air_lift_ratio: f32,
    pub thrust_fraction: f32,
    pub antigrav_fraction: f32,
    /// i, j, k, w.
    pub rotation: [f32; 4],
}

/// `friction_datum`.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Friction {
    pub friction: [f32; 3],
    pub parallel: [f32; 3],
    pub perpendicular: [f32; 3],
}

/// A mass point as the simulation leaves it, laid out as the game's
/// `mass_point_datum` (304 bytes).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct MassPoint {
    pub flags: u32,
    pub position: [f32; 3],
    pub forward: [f32; 3],
    pub left: [f32; 3],
    pub up: [f32; 3],
    pub location_leaf: i32,
    pub location_cluster: i16,
    pub location_bonus: u16,
    pub radius: [f32; 3],
    pub velocity: [f32; 3],
    pub velocity_relative_to_ground: [f32; 3],
    pub ground_plane: [f32; 4],
    pub ground_material_type: i16,
    pub pad0: i16,
    pub ground_depth: f32,
    pub water_material_type: i16,
    pub pad1: i16,
    pub water_depth: f32,
    pub normal_force_magnitude: f32,
    pub normal_force: [f32; 3],
    pub ground_friction: Friction,
    pub water_pressure_magnitude: f32,
    pub water_pressure: [f32; 3],
    pub water_friction: Friction,
    pub air_friction: Friction,
    pub powered_force: [f32; 3],
    pub force: [f32; 3],
    pub torque: [f32; 3],
}

const _: () = assert!(std::mem::size_of::<MassPoint>() == 304);

/// What a vehicle's simulation reads and changes of it.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct VehiclePhysicsState {
    pub position: [f32; 3],
    pub forward: [f32; 3],
    pub up: [f32; 3],
    pub translational_velocity: [f32; 3],
    pub angular_velocity: [f32; 3],
    pub object_flags: u32,
    pub location_leaf: i32,
    pub location_cluster: i16,
    /// The force and torque other vehicles' collisions gave it.
    pub collision_force: [f32; 3],
    pub collision_torque: [f32; 3],
    pub stuck_mass_point_flags: u32,
}

/// Another vehicle a vehicle's unit collisions consider
/// (`objects_in_sphere`), placed as `physics_instance_new` places it.
#[derive(Debug, Clone)]
pub struct OtherVehicle {
    pub index: u32,
    pub at_rest: bool,
    pub physics: std::sync::Arc<PhysicsDefinition>,
    pub origin: [f32; 3],
    pub forward: [f32; 3],
    pub up: [f32; 3],
}

/// The world a vehicle's simulation tests: the structure and objects, the
/// objects each `collision_get_features_in_sphere` call visited (its first
/// mass points' ground planes), the materials' physics, and the vehicles
/// its unit collisions consider.
pub struct VehiclePhysicsWorld<'a> {
    pub world: &'a crate::collision::World<'a>,
    pub feature_objects: &'a [Vec<crate::features::FeatureObject>],
    pub materials: &'a [MaterialPhysics],
    pub others: &'a [OtherVehicle],
}

/// What an update reads besides its state: the vehicle's index, the
/// powered mass points' controls and the magic force and torque its
/// vehicle code gave it (none for a vehicle that isn't driven), and the
/// bounding sphere centre the new pose gives it
/// (`object_compute_node_matrices`, where it finds its location).
pub struct VehiclePhysicsInputs<'a> {
    pub index: u32,
    pub powered: Option<&'a [PoweredMassPoint]>,
    pub magic: Option<([f32; 3], [f32; 3])>,
    pub bounding_center_after: [f32; 3],
}

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

/// `pin_fraction`.
pub fn pin_fraction(value: f32, begin: f32, end: f32) -> f32 {
    if begin < end {
        if value <= begin {
            0.0
        } else if value >= end {
            1.0
        } else {
            (value - begin) / (end - begin)
        }
    } else if value <= end {
        1.0
    } else if value >= begin {
        0.0
    } else {
        (begin - value) / (begin - end)
    }
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

/// `matrix4x3_transform_normal`.
fn transform_normal(m: &Matrix4x3, v: [f32; 3]) -> [f32; 3] {
    let [i, j, k] = v;
    [
        i * m.forward[0] + j * m.left[0] + k * m.up[0],
        i * m.forward[1] + j * m.left[1] + k * m.up[1],
        i * m.forward[2] + j * m.left[2] + k * m.up[2],
    ]
}

/// `component_vectors_from_normal3d`: (parallel, perpendicular).
fn components(vector: [f32; 3], normal: [f32; 3]) -> ([f32; 3], [f32; 3]) {
    let parallel = scale3(normal, dot3(vector, normal));
    (parallel, sub3(vector, parallel))
}

/// `friction_evaluate`.
fn friction_evaluate(friction_type: i16, parallel_scale: f32, perpendicular_scale: f32, friction: &mut Friction, forward: [f32; 3], up: [f32; 3]) {
    if friction_type == 0 {
        friction.parallel = friction.friction;
        friction.perpendicular = [0.0; 3];
        return;
    }
    let axis = match friction_type {
        1 => forward,
        2 => cross3(up, forward),
        3 => up,
        _ => return,
    };
    (friction.parallel, friction.perpendicular) = components(friction.friction, axis);
    friction.parallel = scale3(friction.parallel, parallel_scale);
    friction.perpendicular = scale3(friction.perpendicular, perpendicular_scale);
    friction.friction = add3(friction.parallel, friction.perpendicular);
}

/// `matrix4x3_rotation_from_quaternion`, transposed (a powered mass point's
/// rotation matrix).
fn powered_rotation_matrix(q: [f32; 4]) -> Matrix4x3 {
    let mut m = crate::pose::matrix_from_orientation(&crate::pose::Orientation { rotation: q, translation: [0.0; 3], scale: 1.0 });
    // (matrix4x3_transpose)
    let t = m.left[0];
    m.left[0] = m.forward[1];
    m.forward[1] = t;
    let t = m.up[0];
    m.up[0] = m.forward[2];
    m.forward[2] = t;
    let t = m.up[1];
    m.up[1] = m.left[2];
    m.left[2] = t;
    m
}

/// `physics_instance_new`'s frame: the axes, and the origin moved back by
/// the centre of mass.
fn instance_matrix(origin: [f32; 3], forward: [f32; 3], up: [f32; 3], center_of_mass: [f32; 3]) -> Matrix4x3 {
    let mut m = Matrix4x3 { scale: 1.0, forward, left: cross3(up, forward), up, position: origin };
    let c = center_of_mass;
    m.position = transform_point(&m, [-c[0], -c[1], -c[2]]);
    m
}

/// `physics_compute_new` (or the old physics' loop, `old`): each mass
/// point's ground, water and air forces and its powered ones, added to the
/// total force and torque. The old physics places the mass points by the
/// object's own frame less the centre of mass, scales no friction by the
/// ground's material but an eighth on ice, and marks no antigraving.
#[allow(clippy::too_many_arguments)]
fn compute(
    physics: &PhysicsDefinition,
    state: &VehiclePhysicsState,
    world_matrix: &Matrix4x3,
    powered: Option<&[PoweredMassPoint]>,
    env: &VehiclePhysicsWorld<'_>,
    vehicle_index: u32,
    old: bool,
    totals: ([f32; 3], [f32; 3]),
) -> ([f32; 3], [f32; 3], Vec<MassPoint>) {
    let gravity = physics.gravity_scale * GLOBAL_GRAVITY;
    let (mut total_force, mut total_torque) = totals;
    let rotation_matrices: Vec<Matrix4x3> = powered.map(|p| p.iter().map(|p| powered_rotation_matrix(p.rotation)).collect()).unwrap_or_default();
    let mut mass_points = vec![MassPoint::default(); physics.mass_points.len()];
    let clusters = env.world.clusters;
    let collision = env.world.collision;
    for (index, definition) in physics.mass_points.iter().enumerate() {
        let mass_point = &mut mass_points[index];
        let powered_index = physics.powered_index(definition).filter(|&p| powered.is_some_and(|q| p < q.len()));
        let powered_definition = powered_index.map(|p| physics.powered_mass_points[p]);
        let powered_point = powered_index.and_then(|p| powered.map(|q| q[p]));

        mass_point.flags = 0;
        mass_point.position = if old {
            let c = physics.center_of_mass;
            let local = [definition.position[0] - c[0], definition.position[1] - c[1], definition.position[2] - c[2]];
            transform_point(world_matrix, local)
        } else {
            transform_point(world_matrix, definition.position)
        };
        let axes = match powered_index {
            Some(p) => crate::pose::matrix_multiply(world_matrix, &rotation_matrices[p]),
            None => *world_matrix,
        };
        mass_point.forward = transform_normal(&axes, definition.forward);
        mass_point.up = transform_normal(&axes, definition.up);
        (mass_point.location_leaf, mass_point.location_cluster) = clusters.location_from_point(collision, mass_point.position);

        mass_point.radius = sub3(mass_point.position, state.position);
        mass_point.velocity = add3(state.translational_velocity, cross3(state.angular_velocity, mass_point.radius));

        // (compute_ground_plane)
        mass_point.ground_plane = DEPTHS_OF_HELL;
        mass_point.ground_material_type = NONE;
        let p = mass_point.position;
        let plane = mass_point.ground_plane;
        mass_point.ground_depth = definition.radius - (p[0] * plane[0] + p[1] * plane[1] + p[2] * plane[2] - plane[3]);
        let objects = env.feature_objects.get(index).map(Vec::as_slice).unwrap_or(&[]);
        let features = crate::features::features_in_sphere(
            collision,
            COLLISION_TEST_FOR_BIPEDS_DEAD,
            mass_point.position,
            definition.radius,
            0.0,
            definition.radius,
            vehicle_index as i32,
            objects,
        );
        if !features.is_empty()
            && let Some(hit) = features.test_point(mass_point.position)
        {
            mass_point.ground_plane = hit.plane;
            mass_point.ground_depth = hit.depth;
            let object = (hit.source.object != -1).then(|| objects.iter().find(|o| o.index == hit.source.object)).flatten();
            mass_point.ground_material_type = if hit.source.material == NONE {
                NONE
            } else if hit.source.object != -1 {
                object.and_then(|o| o.model.as_ref()).map_or(NONE, |m| m.material_type(hit.source.material))
            } else {
                clusters.structure_material_type(hit.source.material)
            };
            let volatile = hit.source.flags & crate::collision::surface_flags::BREAKABLE != 0
                || (hit.source.object != -1 && object.is_none_or(|o| o.kind != OBJECT_TYPE_SCENERY));
            if volatile {
                mass_point.flags |= point_flags::ON_VOLATILE_SURFACE;
            }
        }
        mass_point.water_depth = clusters.water_depth(mass_point.location_cluster, mass_point.position);

        if mass_point.ground_depth > 0.0 && (!old || physics.ground_depth > 0.0) {
            let material = if old {
                MaterialPhysics::default()
            } else {
                env.materials.get(mass_point.ground_material_type.max(0) as usize).filter(|_| mass_point.ground_material_type >= 0).copied().unwrap_or_default()
            };
            let mut ground_friction = physics.ground_friction;
            let mut ground_normal_k1 = physics.ground_normal_k1;
            let mut ground_normal_k0 = physics.ground_normal_k0;
            let mut ground_depth = physics.ground_depth;
            let mut ground_damp_fraction = physics.ground_damp_fraction;
            if material.ground_friction_scale > 0.0 && physics.mass <= 7500.0 {
                ground_friction *= material.ground_friction_scale;
            }
            if material.ground_friction_normal_k1_scale > 0.0 {
                ground_normal_k1 *= material.ground_friction_normal_k1_scale;
            }
            if material.ground_friction_normal_k0_scale > 0.0 {
                ground_normal_k0 *= material.ground_friction_normal_k0_scale;
            }
            if material.ground_depth_scale > 0.0 {
                ground_depth *= material.ground_depth_scale;
            }
            if material.ground_damp_fraction_scale > 0.0 {
                ground_damp_fraction *= material.ground_damp_fraction_scale;
            }
            let n = [mass_point.ground_plane[0], mass_point.ground_plane[1], mass_point.ground_plane[2]];
            let normal_velocity = dot3(mass_point.velocity, n);
            mass_point.normal_force_magnitude =
                physics.mass * (GLOBAL_GRAVITY / ground_depth * mass_point.ground_depth - normal_velocity * ground_damp_fraction);
            mass_point.normal_force = scale3(n, mass_point.normal_force_magnitude);
            mass_point.velocity_relative_to_ground = add3(scale3(n, -normal_velocity), mass_point.velocity);
            let ground_scale = -definition.mass * ground_friction;
            mass_point.ground_friction.friction = scale3(mass_point.velocity_relative_to_ground, ground_scale);
            if let (Some(pd), Some(pp)) = (powered_definition, powered_point)
                && pd.flags & POWERED_GROUND_FRICTION != 0
                && pp.ground_friction_velocity != 0.0
            {
                let fraction = pin_fraction(n[2], ground_normal_k0, ground_normal_k1);
                let alignment = pin(dot3(mass_point.up, n), 0.0, 1.0);
                let weight = alignment * alignment * fraction * fraction * ground_scale;
                let powered_velocity = scale3(mass_point.forward, -pp.ground_friction_velocity);
                let mut projected = scale3(n, -dot3(powered_velocity, n));
                projected = add3(projected, powered_velocity);
                mass_point.velocity_relative_to_ground = add3(mass_point.velocity_relative_to_ground, projected);
                projected = scale3(projected, weight);
                mass_point.ground_friction.friction = add3(mass_point.ground_friction.friction, projected);
            }
            let (parallel_scale, perpendicular_scale) = if old && mass_point.ground_material_type == MATERIAL_ICE {
                (definition.friction_parallel_scale * 0.125, definition.friction_perpendicular_scale * 0.125)
            } else {
                (definition.friction_parallel_scale, definition.friction_perpendicular_scale)
            };
            friction_evaluate(definition.friction_type, parallel_scale, perpendicular_scale, &mut mass_point.ground_friction, mass_point.forward, mass_point.up);
        }

        if mass_point.water_depth > 0.0 {
            let depth_fraction = if mass_point.water_depth < physics.water_depth { mass_point.water_depth / physics.water_depth } else { 1.0 };
            let water_scale = -definition.mass * physics.water_friction;
            if definition.density > 0.0 && physics.water_depth > 0.0 {
                mass_point.water_pressure_magnitude = definition.mass / definition.density * physics.water_density * depth_fraction * gravity;
                mass_point.water_pressure = [0.0, 0.0, mass_point.water_pressure_magnitude];
            }
            mass_point.water_friction.friction = match (powered_definition, powered_point) {
                (Some(pd), Some(pp)) if pd.flags & POWERED_WATER_FRICTION != 0 && pp.water_friction_velocity != 0.0 => {
                    let powered_velocity = add3(scale3(mass_point.forward, -pp.water_friction_velocity), mass_point.velocity);
                    scale3(powered_velocity, water_scale)
                }
                _ => scale3(mass_point.velocity, water_scale),
            };
            friction_evaluate(
                definition.friction_type,
                definition.friction_parallel_scale,
                definition.friction_perpendicular_scale,
                &mut mass_point.water_friction,
                mass_point.forward,
                mass_point.up,
            );
            if let (Some(pd), Some(pp)) = (powered_definition, powered_point)
                && pd.flags & POWERED_WATER_LIFT != 0
                && pp.water_lift_ratio != 0.0
            {
                // (fabs: in double)
                let lift = ((dot3(mass_point.forward, mass_point.velocity) as f64).abs()
                    * pp.water_lift_ratio as f64
                    * physics.mass as f64
                    * depth_fraction as f64) as f32;
                mass_point.powered_force = add3(mass_point.powered_force, scale3(mass_point.up, lift));
            }
        }

        {
            let air_scale = -definition.mass * physics.air_friction;
            mass_point.air_friction.friction = match (powered_definition, powered_point) {
                (Some(pd), Some(pp)) if pd.flags & POWERED_AIR_FRICTION != 0 && pp.air_friction_velocity != 0.0 => {
                    let powered_velocity = add3(scale3(mass_point.forward, -pp.air_friction_velocity), mass_point.velocity);
                    scale3(powered_velocity, air_scale)
                }
                _ => scale3(mass_point.velocity, air_scale),
            };
            friction_evaluate(
                definition.friction_type,
                definition.friction_parallel_scale,
                definition.friction_perpendicular_scale,
                &mut mass_point.air_friction,
                mass_point.forward,
                mass_point.up,
            );
            if let (Some(pd), Some(pp)) = (powered_definition, powered_point)
                && pd.flags & POWERED_AIR_LIFT != 0
                && pp.air_lift_ratio != 0.0
            {
                let lift = ((dot3(mass_point.forward, mass_point.velocity) as f64).abs() * physics.mass as f64 * pp.air_lift_ratio as f64) as f32;
                mass_point.powered_force = add3(mass_point.powered_force, scale3(mass_point.up, lift));
            }
        }

        let set = |flags: &mut u32, bit: u32, on: bool| if on { *flags |= bit } else { *flags &= !bit };
        set(&mut mass_point.flags, point_flags::AT_REST, magnitude_squared3(mass_point.velocity) < 0.0011111111);
        set(&mut mass_point.flags, point_flags::ON_GROUND, mass_point.ground_depth > 0.0);
        set(&mut mass_point.flags, point_flags::IN_WATER, mass_point.water_depth > 0.0);

        if let (Some(pd), Some(pp)) = (powered_definition, powered_point) {
            if pd.flags & POWERED_THRUST != 0 {
                mass_point.powered_force = add3(mass_point.powered_force, scale3(mass_point.forward, pp.thrust_fraction * physics.mass));
            }
            if pd.flags & POWERED_ANTIGRAV != 0 {
                let probe_length = definition.radius + pd.antigrav_height;
                let probe_vector = scale3([0.0, 0.0, -1.0], probe_length);
                let hit = env.world.test(COLLISION_TEST_FOR_BIPEDS_DEAD, mass_point.position, probe_vector, vehicle_index);
                if hit.kind.is_some() {
                    let height = probe_length * hit.t - definition.radius;
                    let alignment = pin_fraction(mass_point.up[2], pd.antigrav_normal_k0, pd.antigrav_normal_k1);
                    let ground_effect = if height > 0.0 { 1.0 - height / pd.antigrav_height } else { 1.0 };
                    let n = [hit.plane[0], hit.plane[1], hit.plane[2]];
                    let magnitude = (ground_effect * ground_effect * GLOBAL_GRAVITY - dot3(n, mass_point.velocity) * pd.antigrav_damp_fraction)
                        * pp.antigrav_fraction
                        * pd.antigrav_strength
                        * physics.mass
                        * alignment;
                    mass_point.powered_force = add3(mass_point.powered_force, scale3(n, magnitude));
                    if !old {
                        mass_point.flags |= point_flags::ANTIGRAVING;
                    }
                }
            }
        }

        mass_point.force = add3(mass_point.normal_force, mass_point.ground_friction.friction);
        mass_point.force = add3(mass_point.force, mass_point.water_pressure);
        mass_point.force = add3(mass_point.force, mass_point.water_friction.friction);
        mass_point.force = add3(mass_point.force, mass_point.air_friction.friction);
        mass_point.force = add3(mass_point.force, mass_point.powered_force);
        mass_point.torque = cross3(mass_point.radius, mass_point.force);
        total_force = add3(total_force, mass_point.force);
        total_torque = add3(total_torque, mass_point.torque);
    }
    (total_force, total_torque, mass_points)
}

/// `normalize3d`.
fn normalize3(v: &mut [f32; 3]) -> f32 {
    crate::features::normalize3(v)
}

/// `rotate_vectors3d_by_angular_velocity`.
fn rotate_by_angular_velocity(forward: [f32; 3], up: [f32; 3], angular_velocity: [f32; 3]) -> ([f32; 3], [f32; 3]) {
    let mut axis = angular_velocity;
    let magnitude = normalize3(&mut axis);
    if magnitude == 0.0 {
        return (forward, up);
    }
    let sine = (magnitude as f64).sin() as f32;
    let cosine = (magnitude as f64).cos() as f32;
    let rotation = rotation_from_axis_and_angle(axis, sine, cosine);
    let mut rotated_forward = crate::collision::transform_vector(&rotation, forward);
    let mut rotated_up = crate::collision::transform_vector(&rotation, up);
    normalize3(&mut rotated_forward);
    let dot = -dot3(rotated_up, rotated_forward);
    rotated_up = [dot * rotated_forward[0] + rotated_up[0], dot * rotated_forward[1] + rotated_up[1], dot * rotated_forward[2] + rotated_up[2]];
    normalize3(&mut rotated_up);
    (rotated_forward, rotated_up)
}

/// `matrix4x3_rotation_from_axis_and_angle`.
pub fn rotation_from_axis_and_angle(axis: [f32; 3], sine: f32, cosine: f32) -> Matrix4x3 {
    let squared = [axis[0] * axis[0], axis[1] * axis[1], axis[2] * axis[2]];
    let scaled = [sine * axis[0], sine * axis[1], sine * axis[2]];
    let mut n = [[0.0f32; 3]; 3];
    n[0][0] = (1.0 - squared[0]) * cosine + squared[0];
    let omc = 1.0 - cosine;
    n[0][1] = axis[1] * axis[0] * omc;
    n[1][0] = n[0][1] - scaled[2];
    n[0][1] = scaled[2] + n[0][1];
    n[1][1] = (1.0 - squared[1]) * cosine + squared[1];
    n[0][2] = axis[2] * axis[0] * omc;
    n[2][0] = n[0][2] + scaled[1];
    n[0][2] = n[0][2] - scaled[1];
    n[2][2] = (1.0 - squared[2]) * cosine + squared[2];
    n[1][2] = axis[2] * axis[1] * omc;
    n[2][1] = n[1][2] - scaled[0];
    n[1][2] = scaled[0] + n[1][2];
    Matrix4x3 { scale: 1.0, forward: n[0], left: n[1], up: n[2], position: [0.0; 3] }
}

type Matrix3 = [[f32; 3]; 3];

/// `matrix3x3_multiply`, summed in the game's order.
fn matrix3_multiply(a: &Matrix3, b: &Matrix3) -> Matrix3 {
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

/// `matrix3x3_transpose`.
fn matrix3_transpose(m: &Matrix3) -> Matrix3 {
    [[m[0][0], m[1][0], m[2][0]], [m[0][1], m[1][1], m[2][1]], [m[0][2], m[1][2], m[2][2]]]
}

/// `matrix3x3_transform_vector`.
fn matrix3_transform_vector(m: &Matrix3, v: [f32; 3]) -> [f32; 3] {
    let (f, l, u) = (m[0], m[1], m[2]);
    [
        u[0] * v[2] + l[0] * v[1] + f[0] * v[0],
        u[1] * v[2] + f[1] * v[0] + l[1] * v[1],
        u[2] * v[2] + f[2] * v[0] + l[2] * v[1],
    ]
}

/// What a vehicle's physics update did besides its state: its mass
/// points, the penetration passes it took, whether it moved, and how many
/// other vehicles it collided with.
#[derive(Debug, Clone, Default)]
pub struct VehiclePhysicsOutcome {
    pub mass_points: Vec<MassPoint>,
    pub passes: usize,
    pub moved: bool,
    pub vehicle_collisions: usize,
}

/// `physics_update` for a vehicle with the new physics (a physics of no
/// radius): the forces (`physics_compute_new`), the other vehicles'
/// collision force, the vehicle code's magic force, the integration and the
/// freeze against penetrating the world (`physics_update_new`), and the
/// collision forces it gives itself against other vehicles
/// (`physics_compute_unit_collisions`; what it does to bipeds and the
/// other vehicles is the game's).
pub fn physics_update(
    physics: &PhysicsDefinition,
    state: &mut VehiclePhysicsState,
    inputs: &VehiclePhysicsInputs<'_>,
    env: &VehiclePhysicsWorld<'_>,
) -> VehiclePhysicsOutcome {
    let mut outcome = VehiclePhysicsOutcome::default();
    if physics.radius > 0.0 {
        return physics_update_old(physics, state, inputs, env);
    }
    let world_matrix = instance_matrix(state.position, state.forward, state.up, physics.center_of_mass);
    let gravity = physics.gravity_scale * GLOBAL_GRAVITY;
    let (mut total_force, mut total_torque, mass_points) =
        compute(physics, state, &world_matrix, inputs.powered, env, inputs.index, false, ([0.0, 0.0, -physics.mass * gravity], [0.0; 3]));
    total_force = add3(total_force, state.collision_force);
    total_torque = add3(total_torque, state.collision_torque);
    state.collision_force = [0.0; 3];
    state.collision_torque = [0.0; 3];
    if let Some((force, torque)) = inputs.magic {
        total_force = add3(total_force, force);
        total_torque = add3(total_torque, torque);
    }

    // (physics_update_new)
    let linear_acceleration = scale3(total_force, 1.0 / physics.mass);
    let mut linear_velocity = add3(linear_acceleration, state.translational_velocity);
    let mut position = add3(state.position, linear_velocity);
    let angular_acceleration = match physics.inertial_matrices.get(1) {
        None => [0.0; 3],
        Some(inverse_inertia) => {
            let frame: Matrix3 = [state.forward, cross3(state.up, state.forward), state.up];
            let mut world_inverse_inertia = matrix3_multiply(&frame, inverse_inertia);
            world_inverse_inertia = matrix3_multiply(&world_inverse_inertia, &matrix3_transpose(&frame));
            matrix3_transform_vector(&world_inverse_inertia, total_torque)
        }
    };
    let mut angular_velocity = add3(angular_acceleration, state.angular_velocity);
    let (mut forward, mut up) = rotate_by_angular_velocity(state.forward, state.up, angular_velocity);
    state.translational_velocity = linear_velocity;
    state.angular_velocity = angular_velocity;

    let mut stuck_mass_point_flags = 0u32;
    for _ in 0..4 {
        outcome.passes += 1;
        stuck_mass_point_flags = 0;
        let swept_matrix = instance_matrix(position, forward, up, physics.center_of_mass);
        let mut worst: Option<([f32; 3], crate::collision::CollisionResult)> = None;
        for (index, definition) in physics.mass_points.iter().enumerate() {
            let mass_point = &mass_points[index];
            let swept_position = transform_point(&swept_matrix, definition.position);
            let delta = sub3(swept_position, mass_point.position);
            let collision = env.world.test(COLLISION_TEST_FOR_VEHICLES | crate::collision::flags::FRONT_FACING, mass_point.position, delta, inputs.index);
            if collision.kind.is_some() {
                stuck_mass_point_flags |= 1 << index;
                if worst.as_ref().is_none_or(|(_, w)| w.t > collision.t) {
                    worst = Some((delta, collision));
                }
            }
        }
        let Some((worst_delta, worst_collision)) = worst else {
            // (object_set_position: its location from the new pose's
            // bounding sphere centre, else its position)
            state.position = position;
            state.forward = forward;
            state.up = up;
            let clusters = env.world.clusters;
            let mut location = clusters.location_from_point(env.world.collision, inputs.bounding_center_after);
            if location.1 == NONE {
                location = clusters.location_from_point(env.world.collision, position);
            }
            if location.1 != NONE {
                (state.location_leaf, state.location_cluster) = location;
                state.object_flags &= !object_flags::OUTSIDE_OF_MAP;
            } else {
                state.object_flags |= object_flags::OUTSIDE_OF_MAP;
            }
            outcome.moved = true;
            break;
        };
        let n = [worst_collision.plane[0], worst_collision.plane[1], worst_collision.plane[2]];
        let normal_dot_delta = dot3(worst_delta, n);
        let epsilon = if normal_dot_delta != 0.0 { (0.0078125 / (normal_dot_delta as f64).abs()) as f32 } else { 0.03125 };
        let t = worst_collision.t - epsilon;
        let t = if t > 0.0 { t } else { 0.0 };
        let normal_dot_velocity = dot3(n, linear_velocity);
        if normal_dot_velocity < 0.0 {
            let distance = (t - 1.0) * normal_dot_velocity;
            linear_velocity = [n[0] * distance + linear_velocity[0], n[1] * distance + linear_velocity[1], n[2] * distance + linear_velocity[2]];
            state.translational_velocity = linear_velocity;
            position = add3(state.position, linear_velocity);
        }
        angular_velocity = scale3(angular_velocity, t);
        state.angular_velocity = angular_velocity;
        (forward, up) = rotate_by_angular_velocity(state.forward, state.up, angular_velocity);
    }
    state.stuck_mass_point_flags = stuck_mass_point_flags;

    let count = |bit: u32| mass_points.iter().filter(|m| m.flags & bit != 0).count();
    let (at_rest, on_ground, volatile, in_water) =
        (count(point_flags::AT_REST), count(point_flags::ON_GROUND), count(point_flags::ON_VOLATILE_SURFACE), count(point_flags::IN_WATER));
    let all = mass_points.len();
    let set = |flags: &mut u32, bit: u32, on: bool| if on { *flags |= bit } else { *flags &= !bit };
    set(
        &mut state.object_flags,
        object_flags::AT_REST,
        at_rest == all
            && on_ground >= 3
            && volatile == 0
            && magnitude_squared3(linear_velocity) <= 0.0011111111
            && magnitude_squared3(angular_velocity) <= 0.0027415568
            && magnitude_squared3(linear_acceleration) <= 0.00000030864197
            && magnitude_squared3(angular_acceleration) <= 0.0000030461742,
    );
    set(&mut state.object_flags, object_flags::ON_GROUND, on_ground > 0);
    set(&mut state.object_flags, object_flags::ON_MEDIA, in_water > 0);
    set(&mut state.object_flags, object_flags::PARTIALLY_UNDER_MEDIA, in_water > 0);
    set(&mut state.object_flags, object_flags::WHOLLY_UNDER_MEDIA, in_water == all);

    // (physics_compute_unit_collisions: its own collision force against
    // other vehicles)
    let self_matrix = instance_matrix(state.position, state.forward, state.up, physics.center_of_mass);
    let self_absolute = inputs.index & 0xffff;
    for other in env.others {
        if other.index == inputs.index {
            continue;
        }
        if !((other.index & 0xffff) < self_absolute || other.at_rest || other.physics.radius > 0.0) {
            continue;
        }
        let other_matrix = instance_matrix(other.origin, other.forward, other.up, other.physics.center_of_mass);
        if vehicle_collision(physics, &self_matrix, state, &other.physics, &other_matrix) {
            outcome.vehicle_collisions += 1;
        }
    }
    outcome.mass_points = mass_points;
    outcome
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

/// `physics_update_old`, the physics of a physics with a radius (the
/// Warthog's, among others): the same forces in the object's own frame, the
/// magic torque in place of none, a moment of inertia from each mass
/// point's distance from the torque's axis, and a move straight to the new
/// position (no freeze against the world, no unit collisions).
fn physics_update_old(
    physics: &PhysicsDefinition,
    state: &mut VehiclePhysicsState,
    inputs: &VehiclePhysicsInputs<'_>,
    env: &VehiclePhysicsWorld<'_>,
) -> VehiclePhysicsOutcome {
    let mut outcome = VehiclePhysicsOutcome::default();
    let gravity = physics.gravity_scale * GLOBAL_GRAVITY;
    let world_matrix = Matrix4x3 { scale: 1.0, forward: state.forward, left: cross3(state.up, state.forward), up: state.up, position: state.position };
    let mut total_force = [0.0, 0.0, -physics.mass * gravity];
    let mut total_torque = [0.0f32; 3];
    if let Some((force, torque)) = inputs.magic {
        total_force = add3(total_force, force);
        total_torque = torque;
    }
    let (total_force, total_torque, mass_points) =
        compute(physics, state, &world_matrix, inputs.powered, env, inputs.index, true, (total_force, total_torque));

    let translational_acceleration = if physics.mass != 0.0 { scale3(total_force, 1.0 / physics.mass) } else { [0.0; 3] };
    let mut angular_acceleration = [0.0f32; 3];
    let mut torque_axis = total_torque;
    if normalize3(&mut torque_axis) != 0.0 {
        let mut moment_of_inertia = 0.0f32;
        for (definition, mass_point) in physics.mass_points.iter().zip(&mass_points) {
            let projection = -dot3(mass_point.radius, torque_axis);
            let perpendicular_radius = add3(scale3(torque_axis, projection), mass_point.radius);
            moment_of_inertia +=
                (magnitude_squared3(perpendicular_radius) + definition.radius * definition.radius * 0.4) * definition.mass * physics.moment;
        }
        if moment_of_inertia != 0.0 {
            angular_acceleration = scale3(total_torque, 1.0 / moment_of_inertia);
        }
    }
    state.translational_velocity = add3(state.translational_velocity, translational_acceleration);
    state.angular_velocity = add3(state.angular_velocity, angular_acceleration);

    // (scenario_location_from_line: the end point's; object_translate)
    let new_position = add3(state.position, state.translational_velocity);
    let location = env.world.clusters.location_from_point(env.world.collision, new_position);
    state.position = new_position;
    if location.1 != NONE {
        (state.location_leaf, state.location_cluster) = location;
        state.object_flags &= !object_flags::OUTSIDE_OF_MAP;
    } else {
        state.object_flags |= object_flags::OUTSIDE_OF_MAP;
    }
    outcome.moved = true;
    let mut rotation_axis = state.angular_velocity;
    let angular_speed = normalize3(&mut rotation_axis);
    if angular_speed != 0.0 {
        let sine = (angular_speed as f64).sin() as f32;
        let cosine = (angular_speed as f64).cos() as f32;
        state.forward = rotate_vector_about_axis(state.forward, rotation_axis, sine, cosine);
        state.up = rotate_vector_about_axis(state.up, rotation_axis, sine, cosine);
        normalize3(&mut state.forward);
        let orthogonalization = -dot3(state.up, state.forward);
        state.up = [
            orthogonalization * state.forward[0] + state.up[0],
            orthogonalization * state.forward[1] + state.up[1],
            orthogonalization * state.forward[2] + state.up[2],
        ];
        normalize3(&mut state.up);
    }

    let count = |bit: u32| mass_points.iter().filter(|m| m.flags & bit != 0).count();
    let (at_rest, on_ground, volatile, in_water) =
        (count(point_flags::AT_REST), count(point_flags::ON_GROUND), count(point_flags::ON_VOLATILE_SURFACE), count(point_flags::IN_WATER));
    let all = mass_points.len();
    let set = |flags: &mut u32, bit: u32, on: bool| if on { *flags |= bit } else { *flags &= !bit };
    set(
        &mut state.object_flags,
        object_flags::AT_REST,
        at_rest == all
            && on_ground >= 3
            && volatile == 0
            && magnitude_squared3(state.translational_velocity) <= 0.0011111111
            && magnitude_squared3(state.angular_velocity) <= 0.0027415568
            && magnitude_squared3(translational_acceleration) <= 0.00000030864197
            && magnitude_squared3(angular_acceleration) <= 0.0000030461742,
    );
    set(&mut state.object_flags, object_flags::ON_GROUND, on_ground > 0);
    set(&mut state.object_flags, object_flags::ON_MEDIA, in_water > 0);
    set(&mut state.object_flags, object_flags::PARTIALLY_UNDER_MEDIA, in_water > 0);
    set(&mut state.object_flags, object_flags::WHOLLY_UNDER_MEDIA, in_water == all);
    outcome.mass_points = mass_points;
    outcome
}

/// `physics_compute_vehicle_collision`, for the first vehicle: the forces
/// its mass points and the other's push each other apart by.
fn vehicle_collision(physics0: &PhysicsDefinition, matrix0: &Matrix4x3, state0: &mut VehiclePhysicsState, physics1: &PhysicsDefinition, matrix1: &Matrix4x3) -> bool {
    let mut collision = false;
    let mass_scale = (physics0.mass * physics1.mass).sqrt();
    let mut force0 = [0.0f32; 3];
    let mut torque0 = [0.0f32; 3];
    for mass_point0 in &physics0.mass_points {
        let point0 = transform_point(matrix0, mass_point0.position);
        for mass_point1 in &physics1.mass_points {
            let radius = mass_point0.radius + mass_point1.radius;
            let point1 = transform_point(matrix1, mass_point1.position);
            let mut direction = sub3(point1, point0);
            let distance = normalize3(&mut direction);
            if distance < radius && distance > 0.0 {
                let penetration = (radius - distance) * 0.5;
                let force_magnitude = 2.0 * mass_scale * GLOBAL_GRAVITY / GLOBAL_PHYSICS_COLLISION_DEPTH * penetration;
                let collision_force0 = scale3(direction, -force_magnitude);
                let along = mass_point0.radius - penetration;
                let collision_point = [direction[0] * along + point0[0], direction[1] * along + point0[1], direction[2] * along + point0[2]];
                let radius0 = sub3(collision_point, state0.position);
                let collision_torque0 = cross3(radius0, collision_force0);
                force0 = add3(force0, collision_force0);
                torque0 = add3(torque0, collision_torque0);
                collision = true;
            }
        }
    }
    if collision {
        state0.collision_force = add3(state0.collision_force, force0);
        state0.collision_torque = add3(state0.collision_torque, torque0);
        state0.object_flags &= !object_flags::AT_REST;
    }
    collision
}
