//! Halo's vehicle physics shapes (ROADMAP phase 7, objects and physics): a
//! physics tag's mass points, the spheres a vehicle's simulation moves, as
//! the ray test sees them with `_collision_test_use_vehicle_physics_bit`
//! (`physics_instance_new` and `physics_test_vector` in OpenCE's
//! `source/physics/physics.c`, with `sphere_test_vector3d` from
//! `source/math/real_math.c`), ported as they are, in the same
//! single-precision arithmetic.
//!
//! The game itself sets that bit only for debugging
//! (`collision_debug_flag_use_vehicle_physics`); vehicles' simulation
//! (`physics_update`) is built on these shapes and not ported yet.

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
