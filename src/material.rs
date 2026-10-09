//! Halo shaders that `StandardMaterial` cannot express, as material
//! extensions drawn by the rasterizer.
//!
//! Solari takes a pixel's material from the G-buffer the rasterizer writes,
//! and only `StandardMaterial` for the rays it traces. So a surface with a
//! Halo material is two entities: a raster-only one with the extended
//! material (what the camera sees, detail maps and all), and a ray-traced-only
//! one with the plain `StandardMaterial` (what bounces and shadows see).

use bevy::{
    asset::{embedded_asset, RenderAssetUsages},
    pbr::{ExtendedMaterial, MaterialExtension},
    prelude::*,
    render::render_resource::{AsBindGroup, Extent3d, ShaderType, TextureDimension, TextureFormat},
    shader::ShaderRef,
};

pub type HaloEnvironmentMaterial = ExtendedMaterial<StandardMaterial, HaloEnvironment>;

pub struct HaloMaterialsPlugin;

impl Plugin for HaloMaterialsPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "shaders/halo_environment.wesl");
        app.add_plugins(MaterialPlugin::<HaloEnvironmentMaterial>::default());
        let grey = app.world_mut().resource_mut::<Assets<Image>>().add(neutral_detail_image());
        app.insert_resource(NeutralDetail(grey));
    }
}

/// The texture the game draws with when a detail map is missing (its
/// default detail bitmap): 50% grey, which the double biased functions leave
/// the base alone with.
#[derive(Resource, Clone)]
pub struct NeutralDetail(pub Handle<Image>);

fn neutral_detail_image() -> Image {
    Image::new(
        Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
        TextureDimension::D2,
        // sRGB 128 samples as linear ~0.5^2.2, which the shader's gamma step
        // takes back to 0.5
        vec![128, 128, 128, 255],
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    )
}

#[derive(ShaderType, Debug, Clone, Default, Reflect)]
pub struct HaloEnvironmentParams {
    /// Primary detail scale (xy), secondary detail scale (zw).
    pub detail_scales: Vec4,
    /// Micro detail scale (xy).
    pub micro_scale: Vec4,
    /// x: detail mixed by base alpha (blended types), y: detail function,
    /// z: micro detail function (see `halo_environment.wesl`).
    pub modes: UVec4,
}

/// `shader_environment`'s diffuse layers (see `halo_environment.wesl`).
#[derive(Asset, AsBindGroup, Reflect, Debug, Clone)]
pub struct HaloEnvironment {
    #[uniform(100)]
    pub params: HaloEnvironmentParams,
    #[texture(101)]
    #[sampler(102)]
    pub base_map: Handle<Image>,
    #[texture(103)]
    #[sampler(104)]
    pub primary_detail_map: Handle<Image>,
    #[texture(105)]
    #[sampler(106)]
    pub secondary_detail_map: Handle<Image>,
    #[texture(107)]
    #[sampler(108)]
    pub micro_detail_map: Handle<Image>,
}

const SHADER: &str = "embedded://halo_rt/shaders/halo_environment.wesl";

impl MaterialExtension for HaloEnvironment {
    fn fragment_shader() -> ShaderRef {
        SHADER.into()
    }

    fn deferred_fragment_shader() -> ShaderRef {
        SHADER.into()
    }
}
