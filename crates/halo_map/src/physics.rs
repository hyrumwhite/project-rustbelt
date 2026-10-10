//! Halo's point physics (ROADMAP phase 7, objects and physics):
//! `point_physics_update` in OpenCE's `source/physics/point_physics.c`,
//! ported as it is, in the same single-precision arithmetic. It moves
//! particles, contrails, antennas and flags: gravity and buoyancy in air or
//! water, friction towards the wind or current, and up to three bounces off
//! the structure and water a step (`collision::World::test`).
//!
//! The wind or current at the point (`scenario_get_current`) comes from the
//! caller: it is the game's runtime weather state.

use crate::{
    collision::{self, CollisionKind, World},
    CacheFile, Result,
};

/// `point_physics_definition` (a `pphy` tag).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct PointPhysicsDefinition {
    pub flags: u32,
    pub runtime_mass_over_radius_cubed: f32,
    pub runtime_water_buoyancy_scale: f32,
    pub runtime_air_buoyancy_scale: f32,
    pub density: f32,
    pub air_friction: f32,
    pub water_friction: f32,
    pub contact_friction: f32,
    pub elasticity: f32,
}

/// `point_physics_definition` flags.
pub mod definition_flags {
    pub const FLAMETHROWER_COLLISION: u32 = 1 << 0;
    pub const STRUCTURE_COLLISIONS: u32 = 1 << 1;
    pub const WATER_COLLISIONS: u32 = 1 << 2;
    pub const SIMPLE_WIND: u32 = 1 << 3;
    pub const DAMPED_WIND: u32 = 1 << 4;
    pub const NO_GRAVITY: u32 = 1 << 5;
}

/// What an update did (`point_physics_update`'s result).
pub mod result_flags {
    pub const IN_AIR: u32 = 1 << 0;
    pub const IN_WATER: u32 = 1 << 1;
    pub const COLLIDED_WITH_STRUCTURE: u32 = 1 << 2;
    pub const COLLIDED_WITH_WATER: u32 = 1 << 3;
}

/// The update's flags.
pub mod update_flags {
    pub const FORCE_NO_COLLISIONS: u32 = 1 << 2;
}

/// `global_gravity`: world units per tick squared.
pub const GLOBAL_GRAVITY: f32 = 0.0035651792;
pub const GLOBAL_WATER_DENSITY: f32 = 1.0;
pub const GLOBAL_AIR_DENSITY: f32 = 0.0011;
pub const TICKS_PER_SECOND: f32 = 30.0;
const MAXIMUM_POINT_PHYSICS_COLLISIONS: usize = 3;

impl PointPhysicsDefinition {
    pub fn load(cache: &CacheFile, datum: u32) -> Result<Self> {
        let root = cache.tag_root(datum, crate::groups::POINT_PHYSICS)?;
        Ok(PointPhysicsDefinition {
            flags: root.u32(0x00)?,
            runtime_mass_over_radius_cubed: root.f32(0x04)?,
            runtime_water_buoyancy_scale: root.f32(0x08)?,
            runtime_air_buoyancy_scale: root.f32(0x0c)?,
            density: root.f32(0x20)?,
            air_friction: root.f32(0x24)?,
            water_friction: root.f32(0x28)?,
            contact_friction: root.f32(0x2c)?,
            elasticity: root.f32(0x30)?,
        })
    }
}

/// A point's state, which an update moves.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PointState {
    pub position: [f32; 3],
    pub velocity: [f32; 3],
    /// The leaf and cluster it is in (NONE outside the world).
    pub location_leaf: i32,
    pub location_cluster: i16,
}

/// `point_physics_update`: one step of `dt` seconds for a point of `radius`
/// world units, with the wind or current `current` there and whether the
/// point is under water. Returns the result flags and the last collision's
/// normal, if it collided.
#[allow(clippy::too_many_arguments)]
pub fn point_physics_update(
    world: &World<'_>,
    flags: u32,
    definition: &PointPhysicsDefinition,
    state: &mut PointState,
    current: [f32; 3],
    underwater: bool,
    force: Option<[f32; 3]>,
    radius: f32,
    mut dt: f32,
) -> (u32, Option<[f32; 3]>) {
    let mut result = 0u32;
    let mut collision_normal = None;
    // (a radius below zero is none)
    let radius = if radius >= 0.0 { radius } else { 0.0 };
    if dt == 0.0 {
        return (result, collision_normal);
    }
    let radius_squared = radius * radius;
    let radius_cubed = radius_squared * radius;
    let mut mass = definition.runtime_mass_over_radius_cubed;
    let air_mass_over_radius_cubed = GLOBAL_AIR_DENSITY * 118613.34;
    let water_mass_over_radius_cubed = GLOBAL_WATER_DENSITY * 118613.34;
    let mut buoyancy_scale;
    let friction;
    if underwater {
        mass += water_mass_over_radius_cubed;
        buoyancy_scale = definition.runtime_water_buoyancy_scale;
        friction = definition.water_friction * radius_squared;
        result |= result_flags::IN_WATER;
    } else {
        mass += air_mass_over_radius_cubed;
        buoyancy_scale = definition.runtime_air_buoyancy_scale;
        friction = definition.air_friction * radius_squared;
        result |= result_flags::IN_AIR;
    }
    mass *= radius_cubed;
    let dt_over_mass = dt / mass;
    if definition.flags & definition_flags::NO_GRAVITY != 0 {
        buoyancy_scale = 0.0;
    }
    let v = &mut state.velocity;
    if let Some(force) = force
        && mass != 0.0
    {
        v[0] += dt_over_mass * force[0];
        v[1] += dt_over_mass * force[1];
        v[2] += dt_over_mass * force[2];
    }
    v[2] = ((GLOBAL_GRAVITY * TICKS_PER_SECOND) * TICKS_PER_SECOND) * buoyancy_scale * dt + v[2];
    let t = if mass == 0.0 {
        if friction == 0.0 { 0.0 } else { 1.0 }
    } else {
        let t = dt_over_mass * friction;
        // (PIN, as the macro compares)
        if t < 0.0 {
            0.0
        } else if t > 1.0 {
            1.0
        } else {
            t
        }
    };
    v[0] += (current[0] - v[0]) * t;
    v[1] += (current[1] - v[1]) * t;
    v[2] += (current[2] - v[2]) * t;

    let mut collision_flags = collision::flags::FRONT_FACING;
    let no_collisions = flags & update_flags::FORCE_NO_COLLISIONS != 0;
    if definition.flags & definition_flags::WATER_COLLISIONS != 0 && !no_collisions {
        collision_flags |= collision::test_flags::MEDIA;
    }
    if definition.flags & definition_flags::STRUCTURE_COLLISIONS != 0 && !no_collisions {
        collision_flags |= collision::test_flags::STRUCTURE;
    }
    let mut i = 0;
    while dt != 0.0 && i < MAXIMUM_POINT_PHYSICS_COLLISIONS {
        i += 1;
        let v = state.velocity;
        let delta = [v[0] * dt, v[1] * dt, v[2] * dt];
        let collision = world.test(collision_flags, state.position, delta, u32::MAX);
        if collision.kind.is_none() {
            if collision.location_leaf != -1 {
                state.location_leaf = collision.location_leaf;
                state.location_cluster = collision.location_cluster;
            }
            state.position = collision.point;
            break;
        }
        // (MIN(radius, 0.005))
        let offset = if radius < 0.005 { radius } else { 0.005 };
        match collision.kind {
            Some(CollisionKind::Media) => result |= result_flags::COLLIDED_WITH_WATER,
            Some(CollisionKind::Structure) => result |= result_flags::COLLIDED_WITH_STRUCTURE,
            _ => {}
        }
        let n = [collision.plane[0], collision.plane[1], collision.plane[2]];
        collision_normal = Some(n);
        // (component_vectors_from_normal3d)
        let vn = v[0] * n[0] + v[1] * n[1] + v[2] * n[2];
        let parallel = [n[0] * vn, n[1] * vn, n[2] * vn];
        let perpendicular = [v[0] - parallel[0], v[1] - parallel[1], v[2] - parallel[2]];
        let keep = 1.0 - definition.contact_friction;
        state.velocity = [
            keep * perpendicular[0] - parallel[0] * definition.elasticity,
            keep * perpendicular[1] - parallel[1] * definition.elasticity,
            keep * perpendicular[2] - parallel[2] * definition.elasticity,
        ];
        if collision.location_leaf != -1 {
            state.location_leaf = collision.location_leaf;
            state.location_cluster = collision.location_cluster;
        }
        state.position = [
            n[0] * offset + collision.point[0],
            n[1] * offset + collision.point[1],
            n[2] * offset + collision.point[2],
        ];
        dt -= collision.t * dt;
    }
    (result, collision_normal)
}
