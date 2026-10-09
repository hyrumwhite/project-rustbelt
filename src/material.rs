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
        embedded_asset!(app, "shaders/halo_transparent.wesl");
        app.add_plugins((
            MaterialPlugin::<HaloEnvironmentMaterial>::default(),
            MaterialPlugin::<HaloTransparentMaterial>::default(),
        ));
        let grey = app.world_mut().resource_mut::<Assets<Image>>().add(neutral_detail_image());
        app.insert_resource(NeutralDetail(grey));
    }
}

/// A flat normal map, for environment materials without a bump map.
pub fn flat_normal_image() -> Image {
    Image::new(
        Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
        TextureDimension::D2,
        vec![128, 128, 255, 255],
        TextureFormat::Rgba8Unorm,
        RenderAssetUsages::RENDER_WORLD,
    )
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
    /// Micro detail scale (xy), bump map scale (zw).
    pub micro_scale: Vec4,
    /// x: detail mixed by base alpha (blended types), y: detail function,
    /// z: micro detail function, w: flags (see `halo_environment.wesl`).
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
    /// A tangent-space normal map (linear), or the flat default.
    #[texture(109)]
    #[sampler(110)]
    pub bump_map: Handle<Image>,
}

/// `HaloEnvironmentParams::modes.w` bits.
pub const ENVIRONMENT_HAS_BUMP_MAP: u32 = 1 << 0;
/// The bump map's alpha cuts the surface out.
pub const ENVIRONMENT_ALPHA_TESTED: u32 = 1 << 1;

const SHADER: &str = "embedded://halo_rt/shaders/halo_environment.wesl";

impl MaterialExtension for HaloEnvironment {
    fn fragment_shader() -> ShaderRef {
        SHADER.into()
    }

    fn deferred_fragment_shader() -> ShaderRef {
        SHADER.into()
    }
}

/// `HaloCombiner` in `halo_transparent.wesl`.
#[derive(ShaderType, Debug, Clone, Default, Reflect)]
pub struct HaloCombinerParams {
    pub rgb_inputs: [UVec4; 2],
    pub rgb_outputs: [UVec4; 2],
    pub alpha_inputs: [UVec4; 2],
    pub alpha_outputs: [UVec4; 2],
    pub constant0: [Vec4; 8],
    pub constant1: [Vec4; 8],
    /// Per map: texture coordinate scale (xy) and offset (zw).
    pub map_transforms: [Vec4; 4],
    /// Stage count, combiner flags, final ABCD, final EFG.
    pub program: UVec4,
    /// Output mode (0 alpha blend, 1 add, 2 multiply, 3 premultiplied),
    /// maps present (a bit per stage).
    pub modes: UVec4,
}

impl HaloCombinerParams {
    pub fn from_program(program: &halo_map::combiner::CombinerProgram) -> Self {
        let words = |w: &[u32; 8]| [UVec4::new(w[0], w[1], w[2], w[3]), UVec4::new(w[4], w[5], w[6], w[7])];
        HaloCombinerParams {
            rgb_inputs: words(&program.rgb_inputs),
            rgb_outputs: words(&program.rgb_outputs),
            alpha_inputs: words(&program.alpha_inputs),
            alpha_outputs: words(&program.alpha_outputs),
            constant0: program.constant0.map(Vec4::from),
            constant1: program.constant1.map(Vec4::from),
            map_transforms: [Vec4::new(1.0, 1.0, 0.0, 0.0); 4],
            program: UVec4::new(program.stage_count as u32, program.flags, program.final_abcd, program.final_efg),
            modes: UVec4::ZERO,
        }
    }
}

/// A transparent chicago or generic shader, evaluated as its Xbox combiner
/// program (see `halo_transparent.wesl`). Raster only: Solari does not trace
/// transparent surfaces.
#[derive(Asset, AsBindGroup, Reflect, Debug, Clone)]
pub struct HaloTransparentMaterial {
    #[uniform(0)]
    pub params: HaloCombinerParams,
    #[texture(1)]
    #[sampler(2)]
    pub map0: Handle<Image>,
    #[texture(3)]
    #[sampler(4)]
    pub map1: Handle<Image>,
    #[texture(5)]
    #[sampler(6)]
    pub map2: Handle<Image>,
    #[texture(7)]
    #[sampler(8)]
    pub map3: Handle<Image>,
    pub alpha_mode: AlphaMode,
    /// Draws later (over) other transparent surfaces at the same distance.
    pub depth_bias: f32,
}

impl Material for HaloTransparentMaterial {
    fn fragment_shader() -> ShaderRef {
        "embedded://halo_rt/shaders/halo_transparent.wesl".into()
    }

    fn alpha_mode(&self) -> AlphaMode {
        self.alpha_mode
    }

    fn depth_bias(&self) -> f32 {
        self.depth_bias
    }

    /// Transparent layers are seen from both sides (skies from inside).
    fn specialize(
        _pipeline: &bevy::pbr::MaterialPipeline,
        descriptor: &mut bevy::render::render_resource::RenderPipelineDescriptor,
        _layout: &bevy::mesh::MeshVertexBufferLayoutRef,
        _key: bevy::pbr::MaterialPipelineKey<Self>,
    ) -> Result<(), bevy::render::render_resource::SpecializedMeshPipelineError> {
        descriptor.primitive.cull_mode = None;
        Ok(())
    }
}
