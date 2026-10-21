use bevy_asset::Handle;
use bevy_derive::{Deref, DerefMut};
use bevy_ecs::{component::Component, prelude::ReflectComponent, template::FromTemplate};
use bevy_mesh::Mesh;
use bevy_pbr::{MeshMaterial3d, StandardMaterial};
use bevy_reflect::{Reflect, prelude::ReflectDefault};
use bevy_render::sync_world::SyncToRenderWorld;
use bevy_transform::components::Transform;
use derive_more::derive::From;

/// A mesh component used for raytracing.
///
/// The mesh used in this component must have [`Mesh::enable_raytracing`] set to true,
/// use the following set of vertex attributes: `{POSITION, NORMAL, UV_0, TANGENT}`, use [`bevy_mesh::PrimitiveTopology::TriangleList`],
/// and use [`bevy_mesh::Indices::U32`].
///
/// Uses [`MeshMaterial3d<StandardMaterial>`] by default. A custom raster material
/// can supply [`super::collimated::RaytracingMaterial3d`] as its ray-material override.
#[derive(
    Component, FromTemplate, Clone, Debug, Default, Deref, DerefMut, Reflect, PartialEq, Eq, From,
)]
#[reflect(Component, Default, Clone, PartialEq)]
#[require(MeshMaterial3d<StandardMaterial>, Transform, SyncToRenderWorld)]
pub struct RaytracingMesh3d(pub Handle<Mesh>);

/// The ray scene instance mask of a [`RaytracingMesh3d`] (`0xFF` without
/// one). An entity with a mask stays in the ray scene while hidden: a body
/// the camera doesn't show that still shades the world (a first-person
/// player's). Its emission is no light source (rays that meet it still see
/// it glow).
///
/// Rays leaving a point near [`super::SolarikNearRays`]'s centre see only
/// instances with [`RaytracingMask::NEAR`] set: [`RaytracingMask::AWAY`]
/// (the player's body) is seen from everywhere else.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq, Reflect)]
#[reflect(Component, Clone, PartialEq)]
#[require(SyncToRenderWorld)]
pub struct RaytracingMask(pub u8);

impl RaytracingMask {
    /// Seen by rays leaving points near [`super::SolarikNearRays`]'s centre.
    pub const NEAR: u8 = 1 << 0;
    /// Seen by rays from everywhere else only.
    pub const AWAY: RaytracingMask = RaytracingMask(1 << 1);
}
