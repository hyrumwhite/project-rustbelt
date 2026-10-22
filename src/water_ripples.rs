//! Rippling water panes: a thin `ALPHA_BLEND` pane's normal bent by up to
//! four drifting bump maps, summed by weight, as Halo's water shader sums
//! its ripples (rasterizer_xbox_water.c), with each mip level of the sum
//! faded toward flat. The rays see the rippled normal wherever they meet the
//! pane: the camera's view of it, reflections and refractions alike.
use bevy_app::{App, Plugin};
use bevy_asset::{AssetId, Handle};
use bevy_ecs::resource::Resource;
use bevy_image::Image;
use bevy_math::{UVec4, Vec2, Vec4};
use bevy_pbr::StandardMaterial;
use bevy_platform::collections::HashMap;
use bevy_render::{
    extract_resource::{ExtractResource, ExtractResourcePlugin},
    render_resource::ShaderType,
};

/// A water material's ripples.
#[derive(Clone, Debug, PartialEq)]
pub struct WaterRipples {
    /// Each ripple's bump map (tangent-space normals in RGB).
    pub maps: [Handle<Image>; 4],
    /// The sum's tiling over the material's texture coordinates, and its
    /// drift (texture units a second).
    pub scale: f32,
    pub drift: Vec2,
    /// Each ripple: its repeats over the sum, its offset (u, v) and its
    /// weight in the sum.
    pub ripples: [Vec4; 4],
    /// Each ripple's drift over the sum (its texture units a second).
    pub drifts: [Vec2; 4],
    /// The sum's mip levels: each faded toward flat by its level over the
    /// last times `fade`. Its levels are measured as those of a texture
    /// `size` texels wide, less `lod_bias`.
    pub levels: f32,
    pub fade: f32,
    pub lod_bias: f32,
    pub size: f32,
}

/// Map a ray-traced `StandardMaterial` (a thin pane) to its ripples. The
/// caller owns handle lifetimes and removes entries with their materials.
#[derive(Resource, Clone, Default, ExtractResource)]
#[extract_app(bevy_render::RenderApp)]
pub struct WaterRippleMaterials(pub HashMap<AssetId<StandardMaterial>, WaterRipples>);

/// Mirrors `WaterRipples` in raytracing_scene_bindings.wesl (carried in
/// each material's surface detail); `sum.w` 1 when enabled, 0 for a material
/// without ripples.
#[derive(Clone, Copy, Default, ShaderType)]
pub(crate) struct GpuWaterRipples {
    /// The four maps' texture ids.
    pub maps: UVec4,
    /// Scale, drift (u, v), 1 if enabled.
    pub sum: Vec4,
    pub ripples: [Vec4; 4],
    /// Each ripple's drift in xy.
    pub drifts: [Vec4; 4],
    /// Levels, fade, level-of-detail bias, size.
    pub mipmaps: Vec4,
}

impl GpuWaterRipples {
    pub(crate) fn new(ripples: &WaterRipples, maps: [u32; 4]) -> Self {
        Self {
            maps: UVec4::from_array(maps),
            sum: Vec4::new(ripples.scale, ripples.drift.x, ripples.drift.y, 1.0),
            ripples: ripples.ripples,
            drifts: ripples.drifts.map(|d| d.extend(0.0).extend(0.0)),
            mipmaps: Vec4::new(ripples.levels, ripples.fade, ripples.lod_bias, ripples.size),
        }
    }
}

pub struct WaterRipplesPlugin;
impl Plugin for WaterRipplesPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<WaterRippleMaterials>();
        app.add_plugins(ExtractResourcePlugin::<WaterRippleMaterials>::default());
    }
}
