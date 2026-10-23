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
        embedded_asset!(app, "shaders/glass_sky_mask.wesl");
        embedded_asset!(app, "shaders/sky_water.wesl");
        app.add_plugins((
            MaterialPlugin::<HaloEnvironmentMaterial>::default(),
            MaterialPlugin::<HaloTransparentMaterial>::default(),
            MaterialPlugin::<GlassSkyMask>::default(),
            MaterialPlugin::<SkyWaterMaterial>::default(),
        ));
        let mut masks = app.world_mut().resource_mut::<Assets<GlassSkyMask>>();
        let far = masks.add(GlassSkyMask { distance: GLASS_SKY_MASK_DISTANCE, ..default() });
        let near = masks.add(GlassSkyMask { distance: 0.0, ..default() });
        let self_occlusion = masks.add(GlassSkyMask { distance: 0.0, sort_bias: Some(-SELF_OCCLUSION_LEAD) });
        app.insert_resource(GlassSkyMaskMaterial { far, near, self_occlusion });
        let grey = app.world_mut().resource_mut::<Assets<Image>>().add(neutral_detail_image());
        app.insert_resource(NeutralDetail(grey));
        let tables = app.world_mut().resource_mut::<Assets<Image>>().add(periodic_tables_image());
        let _ = PERIODIC_TABLES.set(tables);
        let volume = Image::new(
            bevy::render::render_resource::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
            bevy::render::render_resource::TextureDimension::D3,
            vec![255; 4],
            bevy::render::render_resource::TextureFormat::Rgba8Unorm,
            bevy::asset::RenderAssetUsages::RENDER_WORLD,
        );
        let volume = app.world_mut().resource_mut::<Assets<Image>>().add(volume);
        let _ = NEUTRAL_VOLUME.set(volume);
        let fog = bevy::render::storage::ShaderBuffer::new(vec![Vec4::ZERO; 5], RenderAssetUsages::default());
        let fog = app.world_mut().resource_mut::<Assets<bevy::render::storage::ShaderBuffer>>().add(fog);
        let _ = crate::fog::FOG_LAYERS.set(fog);
        let none = bevy::render::storage::ShaderBuffer::new(vec![Vec4::ZERO; 5], RenderAssetUsages::default());
        let none = app.world_mut().resource_mut::<Assets<bevy::render::storage::ShaderBuffer>>().add(none);
        let _ = crate::fog::NO_FOG_LAYERS.set(none);
    }
}

/// The sky mask materials every pane shares: the level's, far beyond it,
/// and the first-person weapon's, at its own depth.
#[derive(Resource, Clone)]
pub struct GlassSkyMaskMaterial {
    pub far: Handle<GlassSkyMask>,
    pub near: Handle<GlassSkyMask>,
    /// A self-occluding object's transparent parts' depth, drawn just before
    /// them (`LevelBuilder::self_occluding`).
    pub self_occlusion: Handle<GlassSkyMask>,
}

/// How much sooner (metres of the transparent pass's sort) a self-occluding
/// object's depth is drawn than its parts, which share its position: then
/// each part shows only where it is nearest, as the game draws Cortana (her
/// eyelids over her eyes), while what is behind her, drawn before, shows
/// through.
const SELF_OCCLUSION_LEAD: f32 = 0.01;

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

/// The game's periodic functions' tables (`halo_map::object_functions::
/// periodic_function_tables`), a row of 1024 samples per function, for the
/// environment shaders' animations (`halo_environment.wesl`).
pub static PERIODIC_TABLES: std::sync::OnceLock<Handle<Image>> = std::sync::OnceLock::new();

fn periodic_tables_image() -> Image {
    let tables = halo_map::object_functions::periodic_function_tables();
    let mut image = Image::new(
        Extent3d { width: tables[0].len() as u32, height: tables.len() as u32, depth_or_array_layers: 1 },
        TextureDimension::D2,
        tables.concat(),
        TextureFormat::R8Unorm,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.sampler = bevy::image::ImageSampler::nearest();
    image
}

/// How many samples of the plasma's band (`halo_map::combiner::
/// environment_plasma_band`) the environment material carries.
pub const PLASMA_BAND_SAMPLES: usize = 64;

#[derive(ShaderType, Debug, Clone, Default, Reflect)]
pub struct HaloEnvironmentParams {
    /// Primary detail scale (xy), secondary detail scale (zw).
    pub detail_scales: Vec4,
    /// Micro detail scale (xy), bump map scale (zw).
    pub micro_scale: Vec4,
    /// x: detail mixed by base alpha (blended types), y: detail function,
    /// z: micro detail function, w: flags (see `halo_environment.wesl`).
    pub modes: UVec4,
    /// The self-illumination's animation: the primary, secondary and plasma
    /// "on" and "off" colours, their periodic functions (xyz), periods and
    /// phases (seconds).
    pub illumination_on: [Vec4; 3],
    pub illumination_off: [Vec4; 3],
    pub illumination_functions: UVec4,
    pub illumination_periods: Vec4,
    pub illumination_phases: Vec4,
    /// The texture animation: u and v's periodic functions (xy); their
    /// periods (xy) and scales (zw).
    pub texture_functions: UVec4,
    pub texture_animation: Vec4,
    /// The plasma's band, `PLASMA_BAND_SAMPLES` of it, 4 to a vector.
    pub plasma_band: [Vec4; PLASMA_BAND_SAMPLES / 4],
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
    /// The self-illumination map (linear), with
    /// `ENVIRONMENT_ANIMATED_ILLUMINATION`.
    #[texture(111)]
    #[sampler(112)]
    pub self_illumination_map: Handle<Image>,
    /// [`PERIODIC_TABLES`].
    #[texture(113)]
    pub periodic_tables: Handle<Image>,
}

/// `HaloEnvironmentParams::modes.w` bits.
pub const ENVIRONMENT_HAS_BUMP_MAP: u32 = 1 << 0;
/// The bump map's alpha cuts the surface out.
pub const ENVIRONMENT_ALPHA_TESTED: u32 = 1 << 1;
/// The self-illumination animates (`HaloEnvironmentParams::illumination_*`).
pub const ENVIRONMENT_ANIMATED_ILLUMINATION: u32 = 1 << 2;
/// The textures move (`HaloEnvironmentParams::texture_*`).
pub const ENVIRONMENT_TEXTURE_ANIMATION: u32 = 1 << 3;
/// A `shader_model`'s detail map instead of the environment's layers: the
/// primary detail map is its detail map, the secondary its multipurpose map
/// (the mask: `modes.x`), `modes.y` its function.
pub const MODEL_DETAIL: u32 = 1 << 4;

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
    /// Per map, its texture coordinate transform's two rows (u, then v):
    /// u' = row . (u, v, 0, 1), as `shader_texture_animation_evaluate`
    /// gives them.
    pub map_rows: [Vec4; 8],
    /// Stage count, combiner flags, final ABCD, final EFG.
    pub program: UVec4,
    /// The framebuffer blend function (`BlendFunction` as the tag's), maps
    /// present (a bit per stage), `TransparentInfo::flags`, the fade mode.
    pub modes: UVec4,
    /// x: the fade's scale (its source and the camouflage), y: 1 to show
    /// the surface in magenta (debugging)
    pub fade: Vec4,
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
            map_rows: std::array::from_fn(|i| if i % 2 == 0 { Vec4::new(1.0, 0.0, 0.0, 0.0) } else { Vec4::new(0.0, 1.0, 0.0, 0.0) }),
            program: UVec4::new(program.stage_count as u32, program.flags, program.final_abcd, program.final_efg),
            modes: UVec4::ZERO,
            fade: Vec4::new(1.0, 0.0, 0.0, 0.0),
        }
    }
}

/// A transparent chicago or generic shader, evaluated as its Xbox combiner
/// program (see `halo_transparent.wesl`). Raster only: Solari does not trace
/// transparent surfaces.
#[derive(Asset, AsBindGroup, Reflect, Debug, Clone)]
#[bind_group_data(HaloTransparentKey)]
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
    /// plasma's two noise maps (3D; [`neutral_volume`] otherwise)
    #[texture(9, dimension = "3d")]
    #[sampler(10)]
    pub noise0: Handle<Image>,
    #[texture(11, dimension = "3d")]
    #[sampler(12)]
    pub noise1: Handle<Image>,
    /// The game's environment fog ([`crate::fog::fog_layers`], shared): the
    /// surface is fogged by its own depth, as the game's are.
    #[storage(13, read_only)]
    pub fog: Handle<bevy::render::storage::ShaderBuffer>,
    pub alpha_mode: AlphaMode,
    /// Draws later (over) other transparent surfaces at the same distance.
    pub depth_bias: f32,
    /// The framebuffer blend function (the tag's), as its pipeline's blend.
    pub blend: u16,
    /// Culled from behind (not two-sided).
    pub one_sided: bool,
    /// What changes it over time (its maps' animation, its stages' colours),
    /// for [`crate::level::animate_transparent`]; none when nothing does.
    #[reflect(ignore)]
    pub animation: Option<std::sync::Arc<halo_map::shader::TransparentInfo>>,
    /// The object whose functions and colours drive it (a copy of the
    /// material for that object); none for the level's and for shaders
    /// that take nothing from their object.
    pub object: Option<Entity>,
    /// A numeric counter's digit (an ammo counter's): its first map's frames.
    #[reflect(ignore)]
    pub numeric: Option<std::sync::Arc<NumericCounter>>,
    /// A meter (`shader_transparent_meter`): its combiner constants follow
    /// its object's functions.
    #[reflect(ignore)]
    pub meter: Option<std::sync::Arc<halo_map::shader::MeterInfo>>,
    /// Plasma (`shader_transparent_plasma`, a shield's): its noise moves and
    /// its colours follow its object's functions.
    #[reflect(ignore)]
    pub plasma: Option<std::sync::Arc<halo_map::shader::PlasmaInfo>>,
}

static NEUTRAL_VOLUME: std::sync::OnceLock<Handle<Image>> = std::sync::OnceLock::new();

/// A 1x1x1 white 3D texture, for the plasma noise bindings of the
/// transparent surfaces that aren't plasma.
pub fn neutral_volume() -> Handle<Image> {
    NEUTRAL_VOLUME.get().cloned().unwrap_or_default()
}

/// A numeric shader's digit (`_shader_transparent_flag_numeric_bit`): the
/// first map's frame is one digit, in base its frame count, of its object's
/// function (D for eight frames, else A) times its counter limit.
#[derive(Debug)]
pub struct NumericCounter {
    pub frames: Vec<Handle<Image>>,
    /// which digit (the shader's permutation: 0 the units)
    pub digit: u16,
    pub limit: u8,
}

impl NumericCounter {
    /// The frame showing this digit for the object's function values.
    pub fn frame(&self, values: &[f32; 4]) -> Option<&Handle<Image>> {
        let count = self.frames.len() as i64;
        if count == 0 {
            return None;
        }
        let function = if count == 8 { 3 } else { 0 };
        let limit = self.limit as i64;
        let mut value = ((limit as f32 * values[function] + 0.5).floor() as i64).clamp(0, limit);
        for _ in 0..self.digit {
            value /= count;
        }
        self.frames.get((value % count) as usize)
    }
}

impl HaloTransparentMaterial {
    /// Its maps' transforms and stages' colours at `time` (seconds), with
    /// the object's function values and change colours.
    pub fn animate(
        &self,
        values: Option<&[f32; 4]>,
        colors: Option<&[[f32; 3]; 4]>,
        time: f32,
        periodic: &impl Fn(i16, f32) -> f32,
    ) -> Option<HaloCombinerParams> {
        if let Some(plasma) = &self.plasma {
            let mut params = self.params.clone();
            for (index, noise) in plasma.noise.iter().enumerate() {
                let period = if noise.period == 0.0 { 1.0 } else { noise.period };
                let t = time / period;
                params.map_rows[index] =
                    Vec4::new(noise.scale, t * noise.direction[0], t * noise.direction[1], t * noise.direction[2]);
            }
            let (parallel, delta) = plasma.colors(values, colors);
            params.constant0[0] = Vec4::from(parallel);
            params.constant1[0] = Vec4::from(delta);
            return Some(params);
        }
        if let Some(meter) = &self.meter {
            let mut params = self.params.clone();
            let (c0, c1) = meter.constants(values);
            for stage in 0..4 {
                params.constant0[stage] = Vec4::from(c0[stage]);
                params.constant1[stage] = Vec4::from(c1[stage]);
            }
            return Some(params);
        }
        let info = self.animation.as_ref()?;
        let mut params = self.params.clone();
        for (stage, map) in info.maps.iter().enumerate().take(4) {
            let Some(map) = map else { continue };
            let (u, v) = map.animation.evaluate(values, map.uv_scale, map.uv_offset, map.rotation, time, periodic);
            params.map_rows[stage * 2] = Vec4::from(u);
            params.map_rows[stage * 2 + 1] = Vec4::from(v);
        }
        for (stage, color) in info.stage_colors.iter().enumerate().take(8) {
            params.constant0[stage] = Vec4::from(color.evaluate(values, colors, time, periodic));
        }
        if info.fade_source > 0
            && let Some(values) = values
        {
            params.fade.x = values.get(info.fade_source as usize - 1).copied().unwrap_or(1.0).clamp(0.0, 1.0);
        }
        Some(params)
    }

    /// Faded by its object's camouflage, as the game fades it (but chicago's
    /// flagged not to).
    pub fn camouflage_fade(&self, params: &mut HaloCombinerParams, camouflage: f32) {
        let keep = self.animation.as_ref().is_some_and(|a| a.extra_flags & 1 != 0);
        if !keep {
            params.fade.x *= (1.0 - camouflage).clamp(0.0, 1.0);
        }
    }
}

/// A transparent surface's pipeline: its blend and culling.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct HaloTransparentKey {
    blend: u16,
    one_sided: bool,
}

impl From<&HaloTransparentMaterial> for HaloTransparentKey {
    fn from(material: &HaloTransparentMaterial) -> Self {
        HaloTransparentKey { blend: material.blend, one_sided: material.one_sided }
    }
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

    /// Two-sided ones (and skies, seen from inside) are drawn from both
    /// sides; each blend function is its own blend state, as the game's
    /// framebuffer blends (the shader writes what each one takes).
    fn specialize(
        _pipeline: &bevy::pbr::MaterialPipeline,
        descriptor: &mut bevy::render::render_resource::RenderPipelineDescriptor,
        _layout: &bevy::mesh::MeshVertexBufferLayoutRef,
        key: bevy::pbr::MaterialPipelineKey<Self>,
    ) -> Result<(), bevy::render::render_resource::SpecializedMeshPipelineError> {
        use bevy::render::render_resource::{BlendComponent, BlendFactor, BlendOperation, BlendState, Face};
        descriptor.primitive.cull_mode = if key.bind_group_data.one_sided { Some(Face::Back) } else { None };
        // (a meter's blend takes a constant colour, its tint, which the
        // shader applies: tint mode 1, src times the tint's alpha plus what's
        // there times its colour, taken as its mean in the alpha; tint mode 2,
        // src times the tint's colour plus what's there times src's alpha)
        let component = |src_factor, dst_factor, operation| BlendComponent { src_factor, dst_factor, operation };
        let keep_alpha = component(BlendFactor::Zero, BlendFactor::One, BlendOperation::Add);
        let color = match key.bind_group_data.blend {
            // alpha blend
            0 => component(BlendFactor::SrcAlpha, BlendFactor::OneMinusSrcAlpha, BlendOperation::Add),
            // multiply, double multiply (the shader doubles)
            1 | 2 => component(BlendFactor::Dst, BlendFactor::Zero, BlendOperation::Add),
            // add
            3 => component(BlendFactor::One, BlendFactor::One, BlendOperation::Add),
            // subtract: what's there less the colour
            4 => component(BlendFactor::One, BlendFactor::One, BlendOperation::ReverseSubtract),
            5 => component(BlendFactor::One, BlendFactor::One, BlendOperation::Min),
            6 => component(BlendFactor::One, BlendFactor::One, BlendOperation::Max),
            // meter, tint mode 2
            9 => component(BlendFactor::One, BlendFactor::SrcAlpha, BlendOperation::Add),
            // plasma: added by its alpha
            10 => component(BlendFactor::SrcAlpha, BlendFactor::One, BlendOperation::Add),
            // alpha multiply add: premultiplied; meter, tint mode 1
            _ => component(BlendFactor::One, BlendFactor::OneMinusSrcAlpha, BlendOperation::Add),
        };
        if let Some(fragment) = descriptor.fragment.as_mut() {
            for target in fragment.targets.iter_mut().flatten() {
                target.blend = Some(BlendState { color, alpha: keep_alpha });
            }
        }
        Ok(())
    }
}

/// A glass or water pane's sky mask (see `glass_sky_mask.wesl`): its depth,
/// far beyond the level but in front of the sky, and no colour. Solarik
/// composites the camera's panes over what is behind them before the
/// transparent pass, where the sky's layers are drawn; without it, they drew
/// over every pane with only the sky behind it (the needler's needles above
/// the horizon). It is drawn first in the transparent pass, before the sky's
/// layers: in the opaque pass it kept the skybox from behind the panes too,
/// and they were composited over black. The first-person weapon's panes
/// (the needler's needles) write their own depth instead, as the game draws
/// the weapon over everything: so the fog, which reads it, leaves them clear,
/// and the level's transparent surfaces (the sea) don't draw over them.
#[derive(Asset, AsBindGroup, Reflect, Debug, Clone, Default)]
pub struct GlassSkyMask {
    /// The depth written (metres), or 0 for the pane's own.
    #[uniform(0)]
    pub distance: f32,
    /// Its sort bias, if not the sky masks' (before the sky's layers).
    pub sort_bias: Option<f32>,
}

/// A sky's water (`shaders/sky_water.wesl`): its reflection cube map along
/// the view's reflection, bent by its ripples, opaque, as the game draws a
/// water shader on a sky model. Drawn among the sky's layers, in their order.
#[derive(Asset, AsBindGroup, Reflect, Debug, Clone)]
pub struct SkyWaterMaterial {
    #[texture(0, dimension = "cube")]
    #[sampler(1)]
    pub reflection: Handle<Image>,
    #[uniform(2)]
    pub params: SkyWaterParams,
    /// Each ripple's bump map (a flat normal for none).
    #[texture(3)]
    #[sampler(4)]
    pub ripple0: Handle<Image>,
    #[texture(5)]
    #[sampler(6)]
    pub ripple1: Handle<Image>,
    #[texture(7)]
    #[sampler(8)]
    pub ripple2: Handle<Image>,
    #[texture(9)]
    #[sampler(10)]
    pub ripple3: Handle<Image>,
    pub depth_bias: f32,
}

/// A sky water's ripples, as rasterizer_xbox_water.c moves them.
#[derive(ShaderType, Reflect, Debug, Clone, Copy, Default)]
pub struct SkyWaterParams {
    /// The ripples' sum: its tiling, its drift (texture units a second, u
    /// and v), and 1 if it has any.
    pub ripple: Vec4,
    /// Each ripple: its repeats, its offset (u, v) and its weight.
    pub ripples: [Vec4; 4],
    /// Each ripple's drift (texture units a second, u and v).
    pub drifts: [Vec4; 4],
    /// The sum's mip fade: its levels (at most four), its fade factor and
    /// its level-of-detail bias.
    pub mipmaps: Vec4,
}

/// A water shader's ripples as rasterizer_water_build_bumpmap sums them:
/// each one's repeats, offset (u, v) and weight in the sum, and its drift
/// (texture units a second); and whether there are any.
pub fn water_ripple_terms(water: &halo_map::shader::Water) -> ([Vec4; 4], [Vec2; 4], bool) {
    let mut ripples = water.ripples;
    for ripple in ripples.iter_mut().skip(water.ripple_count) {
        *ripple = halo_map::shader::WaterRipple { repeats: 1, ..default() };
    }
    // (the game's: a pair with no contribution takes its second whole)
    if ripples[0].contribution == 0.0 && ripples[1].contribution == 0.0 {
        ripples[1].contribution = 1.0;
    }
    if ripples[2].contribution == 0.0 && ripples[3].contribution == 0.0 {
        ripples[3].contribution = 1.0;
    }
    let total: f32 = ripples.iter().map(|r| r.contribution).sum();
    let present = water.ripple_count > 0 && water.ripple_maps.is_some() && total > 0.0;
    (
        ripples.map(|r| Vec4::new(r.repeats.max(1) as f32, r.offset[0], r.offset[1], if total > 0.0 { r.contribution / total } else { 0.0 })),
        ripples.map(|r| Vec2::new(r.angle.cos() * r.velocity, r.angle.sin() * r.velocity)),
        present,
    )
}

/// The sum's own drift (texture units a second).
pub fn water_ripple_drift(water: &halo_map::shader::Water) -> Vec2 {
    Vec2::new(water.ripple_angle.cos() * water.ripple_velocity, water.ripple_angle.sin() * water.ripple_velocity)
}

/// The game's ripple bump map's size (WATER_BUMPMAP_RESOLUTION): its levels
/// of detail are measured as this texture's.
pub const WATER_BUMPMAP_SIZE: f32 = 128.0;

impl SkyWaterParams {
    pub fn new(water: &halo_map::shader::Water) -> Self {
        let (ripples, drifts, present) = water_ripple_terms(water);
        let drift = water_ripple_drift(water);
        SkyWaterParams {
            ripple: Vec4::new(water.ripple_scale, drift.x, drift.y, if present { 1.0 } else { 0.0 }),
            ripples,
            drifts: drifts.map(|d| d.extend(0.0).extend(0.0)),
            mipmaps: Vec4::new(
                water.ripple_mipmap_levels.clamp(1, 4) as f32,
                water.ripple_mipmap_fade_factor,
                water.ripple_mipmap_lod_bias,
                0.0,
            ),
        }
    }
}

impl Material for SkyWaterMaterial {
    fn fragment_shader() -> ShaderRef {
        "embedded://halo_rt/shaders/sky_water.wesl".into()
    }

    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Blend
    }

    fn depth_bias(&self) -> f32 {
        self.depth_bias
    }

    fn enable_prepass() -> bool {
        false
    }

    fn enable_shadows() -> bool {
        false
    }

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

/// A cube map bitmap's first bitmap as a cube texture (sRGB), its faces in
/// Direct3D's order, which is wgpu's.
pub fn cube_map_image(cache: &halo_map::CacheFile, datum: u32) -> Option<Image> {
    let bitmap = halo_map::bitmap::bitmaps(cache, datum).ok()?.into_iter().next()?;
    let faces = bitmap.decode_cube_faces_rgba8(cache).ok()?;
    let data: Vec<u8> = faces.into_iter().flat_map(|face| face.data).collect();
    let mut image = Image::new(
        Extent3d { width: bitmap.width as u32, height: bitmap.height as u32, depth_or_array_layers: 6 },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.texture_view_descriptor = Some(bevy::render::render_resource::TextureViewDescriptor {
        dimension: Some(bevy::render::render_resource::TextureViewDimension::Cube),
        ..default()
    });
    Some(image)
}

/// The level's panes' mask distance (metres): beyond any level, nearer than
/// any sky part (100 m per sky unit). The fog takes a depth at it for none.
pub const GLASS_SKY_MASK_DISTANCE: f32 = 50000.0;

/// The mask's sort bias: behind the sky's layers (see `level::SKY_BIAS`), so
/// drawn before them (at -10^9, once the sky's bias was -2*10^9, the needler's
/// needles were masked after the sky's first layers had drawn over them). (A
/// material's bias is the GPU's depth bias for StandardMaterial alone.)
const GLASS_SKY_MASK_BIAS: f32 = -2.1e9;

impl Material for GlassSkyMask {
    fn vertex_shader() -> ShaderRef {
        "embedded://halo_rt/shaders/glass_sky_mask.wesl".into()
    }

    fn fragment_shader() -> ShaderRef {
        "embedded://halo_rt/shaders/glass_sky_mask.wesl".into()
    }

    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Blend
    }

    fn depth_bias(&self) -> f32 {
        self.sort_bias.unwrap_or(GLASS_SKY_MASK_BIAS)
    }

    fn enable_prepass() -> bool {
        false
    }

    fn enable_shadows() -> bool {
        false
    }

    fn specialize(
        _pipeline: &bevy::pbr::MaterialPipeline,
        descriptor: &mut bevy::render::render_resource::RenderPipelineDescriptor,
        _layout: &bevy::mesh::MeshVertexBufferLayoutRef,
        _key: bevy::pbr::MaterialPipelineKey<Self>,
    ) -> Result<(), bevy::render::render_resource::SpecializedMeshPipelineError> {
        descriptor.primitive.cull_mode = None;
        // (blended materials write no depth; this writes only depth)
        if let Some(depth_stencil) = descriptor.depth_stencil.as_mut() {
            depth_stencil.depth_write_enabled = Some(true);
        }
        if let Some(fragment) = descriptor.fragment.as_mut() {
            for target in fragment.targets.iter_mut().flatten() {
                target.write_mask = bevy::render::render_resource::ColorWrites::empty();
                target.blend = None;
            }
        }
        Ok(())
    }
}
