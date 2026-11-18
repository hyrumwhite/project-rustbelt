//! Materials the application marks for the rays to treat in a way of their
//! own, each a bit in the material's flags (`binder.rs`, mirrored in
//! `raytracing_scene_bindings.wesl`).
use bevy_app::{App, Plugin};
use bevy_asset::AssetId;
use bevy_ecs::resource::Resource;
use bevy_pbr::StandardMaterial;
use bevy_platform::collections::HashSet;
use bevy_render::extract_resource::{ExtractResource, ExtractResourcePlugin};

/// The marked materials. The caller owns entry lifetime.
#[derive(Resource, Clone, Default, ExtractResource)]
#[extract_app(bevy_render::RenderApp)]
pub struct RayMaterialMarks {
    /// Thin panes (`ALPHA_BLEND`) of which the camera sees the nearest layer
    /// alone along each of its rays (`MATERIAL_FLAG_NEAREST_LAYER`,
    /// `primary_glass.wesl`): a closed body of many parts drawn as glass
    /// (an actively camouflaged first-person weapon and arms), whose inner
    /// parts and far faces would otherwise each add a reflection. Other
    /// panes behind it composite as before; other rays see every layer.
    pub nearest_layer: HashSet<AssetId<StandardMaterial>>,
    /// Emitters that only reflections see (`MATERIAL_FLAG_REFLECTED_GLOW`,
    /// `specular_gi.wesl`), on instances masked
    /// [`crate::scene::RaytracingMask::REFLECTED`]: an additive hologram the
    /// rays otherwise never meet. A reflection's first ray adds its emission
    /// and goes on through it.
    pub reflected_glow: HashSet<AssetId<StandardMaterial>>,
    /// Terrain, the blockers of the soft sun (`MATERIAL_FLAG_SOFT_SUN`,
    /// `sampling.wesl`'s `sun_soft_transmission`): with `SOLARIK_SUN_BLUR`
    /// or `SOLARIK_SUN_TERMINATOR` set, a terrain pixel's sun is tested
    /// past these from a point of the terrain near it, and past everything
    /// else from the pixel itself. A terrain receiver is a G-buffer pixel
    /// without Bevy's shadow-receiver bit (`NotShadowReceiver` on the
    /// entity the camera draws). Inert without the switches.
    pub soft_sun: HashSet<AssetId<StandardMaterial>>,
}

pub struct MaterialMarksPlugin;
impl Plugin for MaterialMarksPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<RayMaterialMarks>();
        app.add_plugins(ExtractResourcePlugin::<RayMaterialMarks>::default());
    }
}
