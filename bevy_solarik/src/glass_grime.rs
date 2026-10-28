//! Grimy panes: a thin `ALPHA_BLEND` pane with a diffuse layer over it, as
//! Halo's glass shader blends its diffuse map (grime and smudges, lit by the
//! surroundings) over the pane (rasterizer_xbox_glass.c). The material's
//! base colour texture is the layer (its colour in RGB, its coverage in
//! alpha) and its base colour the pane's tint; the camera's rays see the
//! layer lit (`primary_glass.wesl`), other rays the bare pane.
use bevy_app::{App, Plugin};
use bevy_asset::AssetId;
use bevy_ecs::resource::Resource;
use bevy_pbr::StandardMaterial;
use bevy_platform::collections::HashSet;
use bevy_render::extract_resource::{ExtractResource, ExtractResourcePlugin};

/// The ray-traced thin panes whose base colour texture is a grime layer.
/// The caller owns entry lifetime.
#[derive(Resource, Clone, Default, ExtractResource)]
#[extract_app(bevy_render::RenderApp)]
pub struct GlassGrimeMaterials(pub HashSet<AssetId<StandardMaterial>>);

pub struct GlassGrimePlugin;
impl Plugin for GlassGrimePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<GlassGrimeMaterials>();
        app.add_plugins(ExtractResourcePlugin::<GlassGrimeMaterials>::default());
    }
}
