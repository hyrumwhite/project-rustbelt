//! The game's fog, drawn here (ROADMAP: effects in the viewer). With
//! `game.bevy_bridge_viewer_fog`, the game leaves its environment fog pass
//! out of the overlay and shares the fog it would have drawn with each frame
//! ([`halo_bridge::Fog`]), and its fog screen ([`halo_bridge::FogScreen`]),
//! whose layers it moves on each frame without drawing them;
//! `shaders/halo_fog.wesl` has the equations.
//!
//! The environment fog is laid on the lit picture at the render resolution,
//! each render pixel by its own depth, after Solarik's lighting and before the
//! transparent pass ([`draw_fog_before`]): the upscaler is given a fogged
//! picture, and the transparent surfaces and sprites are drawn over it, each
//! faded by its own fog ([`HaloFog::layers`]), as the game draws its effects
//! after its fog. The blend is the game's, in the display's encoding, taken
//! back to linear light through the tonemapper's curve ([`FogBefore`]) about
//! a local mean of the noisy picture ([`fog_before_mean`]). Lit fog's light
//! ([`LitFog`], Solarik's light scattered by the fog, undivided and not
//! composited by Solarik) is laid over in the same pass ([`FogBeforeLit`]).
//! The fog covers objects too, which in play the game's own model shaders fog
//! and the overlay never could.
//!
//! Where this viewer draws the water ([`ViewerWater`]), the planar fog seen
//! through it covers only the light that came through the surface: the game
//! fogs the seabed under its fog plane (the sea's surface, in b30) and draws
//! its water's reflection over that, but here the water is already in the
//! picture, its reflection and the seabed through it together.
//!
//! After tonemapping and the upscaler, the fog pass ([`draw_fog`]) lays the
//! fog screen, then the game's weather (`src/weather.rs`, drawn first into a
//! layer of its own, each flake faded by its own fog) and last the game's lens
//! flares (`src/lens_flares.rs`, added up first in a texture of their own),
//! in the display's encoding.

use crate::bridge::BridgeLink;
use bevy::render::diagnostic::RecordDiagnostics;
use bevy::{
    asset::{embedded_asset, load_embedded_asset},
    core_pipeline::{
        core_3d::main_transparent_pass_3d, prepass::ViewPrepassTextures, schedule::Core3d, tonemapping::tonemapping,
        Core3dSystems, FullscreenShader,
    },
    platform::collections::HashMap,
    prelude::*,
    render::{
        extract_component::{ExtractComponent, ExtractComponentPlugin},
        render_asset::RenderAssets,
        storage::ShaderBuffer,
        texture::{FallbackImage, FallbackImageZero, GpuImage, TextureCache},
        render_resource::{
            binding_types::{sampler, texture_2d, texture_depth_2d, texture_storage_2d, uniform_buffer},
            TextureDescriptor, TextureDimension, TextureUsages,
            encase::UniformBuffer,
            BindGroupEntries, BindGroupLayoutDescriptor, BindGroupLayoutEntries,
            BufferInitDescriptor, BufferUsages, CachedComputePipelineId, ComputePassDescriptor, ComputePipelineDescriptor,
            StorageTextureAccess,
            CachedRenderPipelineId, ColorTargetState, ColorWrites, FragmentState, Operations, PipelineCache,
            RenderPassColorAttachment, RenderPassDescriptor, RenderPipelineDescriptor, Sampler, SamplerBindingType,
            AddressMode, FilterMode, MipmapFilterMode, SamplerDescriptor, ShaderStages, ShaderType, TextureFormat,
            TextureSampleType,
        },
        renderer::{RenderContext, RenderDevice, ViewQuery},
        view::{ExtractedView, ViewDepthStencilTexture, ViewTarget, ViewUniform, ViewUniformOffset, ViewUniforms},
        RenderApp, RenderStartup,
    },
};
use bevy::diagnostic::FrameCount;
use bevy::shader::ShaderDefVal;
use bevy_solarik::realtime::SolarikLightingResources;
use halo_map::math::{halo_to_bevy, METERS_PER_WORLD_UNIT};

pub struct FogPlugin;

/// This viewer draws the water (and glass) itself, ray traced, not the
/// game's overlay.
#[derive(Resource)]
pub struct ViewerWater;

/// How near a water surface's height the fog plane is the water's (world
/// units).
const WATER_PLANE_TOLERANCE: f32 = 0.1;

/// The fog lit by the scene's lights (Solarik's lit fog: single scattering
/// of the lights along each view ray, through the game's fog): whether it's
/// on (F10's panel), its strength, and whether the sun lights it too (the
/// game's fog colour already stands for the daylight, so not by default).
/// Present wherever [`FogPlugin`] is.
#[derive(Resource, Clone, Copy, Debug, PartialEq)]
pub struct LitFog {
    pub on: bool,
    pub strength: f32,
    pub sun: bool,
    /// The scattered light alone, for looking at it.
    pub only: bool,
}

impl LitFog {
    /// "strength[,sun][,only]", on; "0" or "off", off (its strength 0, for
    /// the saved one to fill); none for anything else.
    pub fn parse(text: &str) -> Option<Self> {
        let mut parts = text.split(',').map(str::trim);
        let first = parts.next()?;
        if first.eq_ignore_ascii_case("off") {
            return Some(Self { on: false, strength: 0.0, sun: false, only: false });
        }
        let strength = first.parse::<f32>().ok().filter(|s| s.is_finite() && *s >= 0.0)?;
        if strength == 0.0 {
            return Some(Self { on: false, strength: 0.0, sun: false, only: false });
        }
        let (mut sun, mut only) = (false, false);
        for part in parts {
            sun |= part.eq_ignore_ascii_case("sun");
            only |= part.eq_ignore_ascii_case("only");
        }
        Some(Self { on: true, strength, sun, only })
    }

    /// The run's start: the override ([`read_lit_fog_override`]) if given
    /// (off at the saved strength), else the saved choice (on and strength;
    /// none saved, off at the default).
    pub fn starting(given: Option<Self>, saved: Option<(bool, f32)>) -> Self {
        let (saved_on, saved_strength) = saved.unwrap_or((false, crate::options::LIT_FOG_STRENGTH));
        match given {
            Some(lit) if lit.on => lit,
            Some(_) => Self { on: false, strength: saved_strength, sun: false, only: false },
            None => Self { on: saved_on, strength: saved_strength, sun: false, only: false },
        }
    }

    /// What Solarik is given (`SolarikRayFog::lit`): zero when off; w 1, the
    /// light left undivided and not composited by Solarik, for
    /// [`draw_fog_before`] lays it over the fog itself.
    fn shared(&self) -> Vec4 {
        if !self.on {
            return Vec4::ZERO;
        }
        Vec4::new(self.strength, f32::from(u8::from(self.sun)), f32::from(u8::from(self.only)), 1.0)
    }
}

static LIT_FOG_OVERRIDE: std::sync::OnceLock<Option<LitFog>> = std::sync::OnceLock::new();

/// This run's lit fog in place of the saved choice, from `--lit-fog` or
/// `HALO_RT_LIT_FOG` (the text main resolves, given here; see
/// [`LitFog::parse`]): logged, and a value that doesn't parse left out with
/// a warning. Read once (the first call's text kept).
pub fn read_lit_fog_override(text: Option<&str>) -> Option<LitFog> {
    *LIT_FOG_OVERRIDE.get_or_init(|| {
        let text = text?;
        let lit = LitFog::parse(text);
        match lit {
            Some(lit) => info!("lit fog: {lit:?}"),
            None => warn!("lit fog: {text:?} isn't <strength>[,sun][,only] or off, left out"),
        }
        lit
    })
}

/// [`read_lit_fog_override`]'s value (none before it was read, as where the
/// viewer draws no fog).
pub fn lit_fog_override() -> Option<LitFog> {
    LIT_FOG_OVERRIDE.get().copied().flatten()
}

impl Plugin for FogPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "shaders/halo_fog.wesl");
        app.add_plugins((ExtractComponentPlugin::<HaloFog>::default(), ExtractComponentPlugin::<FogScreenMaps>::default()))
            .add_systems(Update, (follow_fog, fog_transparent_surfaces));
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else { return };
        render_app
            .add_systems(RenderStartup, init_pipeline)
            .add_systems(Core3d, draw_fog.in_set(Core3dSystems::PostProcess).after(tonemapping))
            // (the environment fog: over the lit picture, before the transparents)
            .add_systems(
                Core3d,
                draw_fog_before
                    .after(bevy_solarik::realtime::solarik_lighting::<true>)
                    .before(main_transparent_pass_3d)
                    .in_set(Core3dSystems::MainPass),
            );
    }
}

/// The fog the camera's picture is drawn with: the game's, in this world's
/// terms (metres, Y up); none (no density, its form 0) when the game has
/// none.
#[derive(Component, Clone, Copy, Default, PartialEq, ShaderType, ExtractComponent)]
#[extract_app(RenderApp)]
pub struct HaloFog {
    /// rgb, maximum density
    atmospheric_color: Vec4,
    /// minimum and maximum distance, the camera's density, 1 when the
    /// picture holds the water ([`ViewerWater`])
    atmospheric: Vec4,
    /// normal (out of the fog) and d
    plane: Vec4,
    /// rgb, maximum density
    planar_color: Vec4,
    /// maximum distance and depth, the camera's density, 1 when the level's
    /// sky has no model (the atmospheric fog's colour under the sky's
    /// pixels' picture, the game's clear), 2 the same painted over it
    /// ([`sky_clear_over`])
    planar: Vec4,
    /// the fog screen: rgb, mode (0 none, 1 added by depth, 2 blended)
    screen_color: Vec4,
    /// near and far distance, near over far density, 1 if its depth's
    /// density is flat
    screen: Vec4,
    /// each layer's texture coordinates from the picture's: u's (x, y, 1),
    /// then v's
    screen_transforms: [Vec4; 8],
    /// each layer's frame's weights, and 1 for a layer
    screen_weights: [Vec4; 4],
    /// x lit fog's light laid over by [`draw_fog_before`] (1 by the shoulder,
    /// 2 added, [`FogBeforeLit`]; set by that pass alone), y 1 to show it
    /// alone, z unused, w the blend's form ([`FogBefore::form`], set by
    /// [`follow_fog`]; 0 with no fog) (last: the weather's shader reads the
    /// fields before it)
    lit: Vec4,
}

/// The fog screen layers' frames, in the layers' order.
#[derive(Component, Clone, Default, ExtractComponent)]
#[extract_app(RenderApp)]
pub struct FogScreenMaps([Option<Handle<Image>>; 4]);

impl HaloFog {
    /// The environment fog's five values (atmospheric colour and density,
    /// its distances, the plane, planar colour and density, its distances),
    /// as the sprites' and transparent surfaces' shaders take them: without
    /// the camera's own place in the fog (the eye terms, which those shaders
    /// find from the view), so that they change only with the game's fog, not
    /// every frame the camera rises or falls (each change rewrites every
    /// material).
    pub fn layers(&self) -> [Vec4; 5] {
        let mut atmospheric = self.atmospheric;
        let mut planar = self.planar;
        // (atmosphere dominant: the eye's atmospheric term, 1, kept as a flag)
        atmospheric.z = if atmospheric.z >= 1.0 { 1.0 } else { 0.0 };
        // (the planar eye term those shaders find for themselves)
        planar.z = 0.0;
        [self.atmospheric_color, atmospheric, self.plane, self.planar_color, planar]
    }

    /// The game's fog as seen from its camera (in the game's coordinates):
    /// the camera's densities as `_rasterizer_environment_fog_begin` finds
    /// them, and its screen. None if neither has any density.
    fn new(fog: &halo_bridge::Fog, screen: Option<&halo_bridge::FogScreen>, camera: [f32; 3]) -> Option<Self> {
        let atmospheric_density = fog.atmospheric_maximum_density.clamp(0.0, 1.0);
        let planar_density = fog.planar_maximum_density.clamp(0.0, 1.0);
        if atmospheric_density <= 0.0 && planar_density <= 0.0 && screen.is_none() {
            return None;
        }
        let [i, j, k, d] = fog.plane;
        let normal = Vec3::new(i, j, k);
        let above = normal.dot(Vec3::from(camera)) - d;
        let mut atmospheric_eye = (above / fog.atmospheric_maximum_distance.max(1e-6)).clamp(0.0, 1.0);
        // (atmosphere dominant)
        if fog.flags & 2 != 0 {
            atmospheric_eye = 1.0;
        }
        let planar_eye = (-above / fog.planar_maximum_depth.max(1e-6)).clamp(0.0, 1.0);
        let metres = METERS_PER_WORLD_UNIT;
        let mut screen_transforms = [Vec4::ZERO; 8];
        let mut screen_weights = [Vec4::ZERO; 4];
        let (mut screen_color, mut screen_parameters) = (Vec4::ZERO, Vec4::ZERO);
        if let Some(screen) = screen {
            screen_color = Vec3::from(screen.color).extend(screen.mode as f32);
            screen_parameters = Vec4::new(
                screen.near_distance * metres,
                screen.far_distance * metres,
                screen.near_density,
                if screen.flags & 4 != 0 { 1.0 } else { 0.0 },
            );
            for (index, layer) in screen.layers.iter().take(screen.layer_count.clamp(0, 4) as usize).enumerate() {
                let [u_x, u_y, u, v_x, v_y, v] = layer.transform;
                screen_transforms[index * 2] = Vec4::new(u_x, u_y, u, 0.0);
                screen_transforms[index * 2 + 1] = Vec4::new(v_x, v_y, v, 0.0);
                screen_weights[index] = Vec3::from(layer.weights).extend(1.0);
            }
        }
        Some(Self {
            atmospheric_color: Vec3::from(fog.atmospheric_color).extend(atmospheric_density),
            atmospheric: Vec4::new(
                fog.atmospheric_minimum_distance * metres,
                fog.atmospheric_maximum_distance * metres,
                atmospheric_eye,
                0.0,
            ),
            plane: Vec3::from(halo_to_bevy([i, j, k], 1.0)).extend(d * metres),
            planar_color: Vec3::from(fog.planar_color).extend(planar_density),
            planar: Vec4::new(fog.planar_maximum_distance * metres, fog.planar_maximum_depth * metres, planar_eye, 0.0),
            screen_color,
            screen: screen_parameters,
            screen_transforms,
            screen_weights,
            lit: Vec4::ZERO,
        })
    }
}

/// The camera's fog onto the transparent surfaces and sprites, which fade by
/// their own (`halo_transparent.wesl`, `halo_sprite.wesl`), drawn over the
/// fogged picture; `HALO_RT_NO_SPRITE_FOG` leaves them unfogged, for
/// comparison.
fn fog_transparent_surfaces(
    camera: Query<&HaloFog, With<Camera3d>>,
    mut buffers: ResMut<Assets<ShaderBuffer>>,
    mut current: Local<[Vec4; 5]>,
) {
    let fog = camera
        .iter()
        .next()
        .filter(|_| std::env::var_os("HALO_RT_NO_SPRITE_FOG").is_none())
        .map_or([Vec4::ZERO; 5], HaloFog::layers);
    if fog != *current
        && let Some(mut buffer) = buffers.get_mut(&fog_layers())
    {
        *current = fog;
        // (the same size: the GPU's buffer is rewritten in place)
        *buffer = ShaderBuffer::new(fog.to_vec(), bevy::asset::RenderAssetUsages::default());
    }
}

pub(crate) static FOG_LAYERS: std::sync::OnceLock<Handle<ShaderBuffer>> = std::sync::OnceLock::new();

/// The buffer holding the game's fog ([`HaloFog::layers`]) that every
/// transparent surface's and sprite's material reads: one write when the fog
/// changes (its distance follows the camera on some levels), rather than a
/// rewrite of every material. (Made by the material plugin: the level
/// viewer has no fog, and reads it as none.)
pub fn fog_layers() -> Handle<ShaderBuffer> {
    FOG_LAYERS.get().cloned().unwrap_or_default()
}

pub(crate) static NO_FOG_LAYERS: std::sync::OnceLock<Handle<ShaderBuffer>> = std::sync::OnceLock::new();

/// A fog buffer that stays empty: the sky's layers', which the game draws
/// unfogged (bridged, the sky took the full fog at its distance, ~100 km,
/// and a30's clouds, ring and sea went grey).
pub fn no_fog_layers() -> Handle<ShaderBuffer> {
    NO_FOG_LAYERS.get().cloned().unwrap_or_default()
}

/// `HALO_RT_SKY_CLEAR_OVER=1`: a modelless sky's fog colour painted over
/// its pixels, the old rule (b40's dome panes and the energy field behind
/// them hidden), and the camera's stand-in skybox left lit under it.
fn sky_clear_over() -> bool {
    static OVER: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *OVER.get_or_init(|| std::env::var("HALO_RT_SKY_CLEAR_OVER").is_ok_and(|v| v == "1"))
}

/// `HALO_RT_SEA_HORIZON_FOG=0`: the sky under the horizon left bare where
/// the picture holds the sea, the old (and the game's): past the level's sea
/// (a thousand world units out on b30) the sky's own water showed as a white
/// band between the fogged sea and the sky. Else it is fogged as the sea over
/// a deep floor would be.
fn sea_horizon_fog() -> bool {
    static FOG: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *FOG.get_or_init(|| std::env::var("HALO_RT_SEA_HORIZON_FOG").map_or(true, |v| v.trim() != "0"))
}

/// The environment fog's blend (`HALO_RT_FOG_BEFORE`), laid over the lit
/// picture before the upscaler by [`draw_fog_before`]: the game's blend in the
/// display's encoding, taken back to the linear light the tonemapper follows
/// with. Nothing, `tone` or `2`: through the tonemapper's curve along the grey
/// axis, so that what is shown is the game's blend of what was shown (the
/// default); `1`: the tonemapper taken as straight (bright fogs shown darker,
/// a diagnostic); anything else the default, with a warning.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum FogBefore {
    Straight,
    Curve,
}

impl FogBefore {
    /// The value and whether the text was understood.
    fn parse(text: Option<&str>) -> (Self, bool) {
        match text.map(str::trim) {
            None => (Self::Curve, true),
            Some("1") => (Self::Straight, true),
            Some(text) if text.eq_ignore_ascii_case("tone") || text == "2" => (Self::Curve, true),
            Some(_) => (Self::Curve, false),
        }
    }

    pub(crate) fn get() -> Self {
        static BEFORE: std::sync::OnceLock<FogBefore> = std::sync::OnceLock::new();
        *BEFORE.get_or_init(|| {
            let text = std::env::var("HALO_RT_FOG_BEFORE").ok();
            let (before, understood) = Self::parse(text.as_deref());
            if !understood {
                warn!("the fog's blend: {text:?} isn't 1 or tone, left at the default (tone)");
            }
            if before == Self::Straight {
                info!("the fog's blend: the tonemapper taken as straight (HALO_RT_FOG_BEFORE=1)");
            }
            before
        })
    }

    /// The fog's `lit.w`: 1 straight, 2 the curve.
    fn form(self) -> f32 {
        match self {
            Self::Straight => 1.0,
            Self::Curve => 2.0,
        }
    }
}

/// The fog before the upscaler's local mean's radius when nothing says
/// otherwise ([`fog_before_mean`]).
pub const FOG_BEFORE_MEAN_RADIUS: u32 = 2;

/// `HALO_RT_FOG_BEFORE_MEAN=<radius>`: the fog before the upscaler blended at
/// a local mean of the picture (the render pixels within the radius, 1 to 4,
/// at about the pixel's depth) and the pixel's difference from it carried
/// through the blend's slope, at most 1 (`halo_fog.wesl`'s `affine_about`), so that
/// the denoiser's average of the noisy picture is not darkened by the blend's
/// curve (c10's mid fog 3 to 5 of 255 darker without it); `0` each pixel
/// blended as it is; nothing, or anything else, [`FOG_BEFORE_MEAN_RADIUS`].
/// Read once, logged when set.
pub(crate) fn fog_before_mean() -> u32 {
    static MEAN: std::sync::OnceLock<u32> = std::sync::OnceLock::new();
    *MEAN.get_or_init(|| {
        let text = std::env::var("HALO_RT_FOG_BEFORE_MEAN").ok();
        let mean = parse_fog_before_mean(text.as_deref());
        if text.is_some() {
            info!("the fog's blend: the picture's local mean's radius {mean}, 0 none (HALO_RT_FOG_BEFORE_MEAN)");
        }
        mean
    })
}

/// `0`, none; a radius from 1 to 4; anything else, or nothing, the default.
fn parse_fog_before_mean(text: Option<&str>) -> u32 {
    text.and_then(|text| text.trim().parse::<u32>().ok()).filter(|radius| *radius <= 4).unwrap_or(FOG_BEFORE_MEAN_RADIUS)
}

/// How [`draw_fog_before`] lays lit fog's light over the fog
/// (`HALO_RT_FOG_BEFORE_LIT`): `Shoulder` (the default), 1 - exp(-light)
/// screened over what is shown, through the tonemapper's curve and back, the
/// look the user tuned its strength on; `Add` (`add`): added as linear light,
/// the tonemapper following (brighter walls, a glow's core less white).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum FogBeforeLit {
    Shoulder,
    Add,
}

impl FogBeforeLit {
    fn parse(text: Option<&str>) -> Self {
        match text.map(str::trim) {
            Some(text) if text.eq_ignore_ascii_case("add") => Self::Add,
            _ => Self::Shoulder,
        }
    }

    fn get() -> Self {
        static LIT: std::sync::OnceLock<FogBeforeLit> = std::sync::OnceLock::new();
        *LIT.get_or_init(|| {
            let lit = Self::parse(std::env::var("HALO_RT_FOG_BEFORE_LIT").ok().as_deref());
            if lit == Self::Add {
                info!("lit fog: its light added as linear light (HALO_RT_FOG_BEFORE_LIT=add)");
            }
            lit
        })
    }

    /// The fog's `lit.x` in [`draw_fog_before`] where the light is bound: 1
    /// the shoulder, 2 added.
    fn flag(self) -> f32 {
        match self {
            Self::Shoulder => 1.0,
            Self::Add => 2.0,
        }
    }
}

/// [`draw_fog_before`]'s shader defs: the fog pass's, and the local mean's
/// radius where it is on.
fn fog_before_shader_defs(sea_horizon: bool, mean: u32) -> Vec<ShaderDefVal> {
    let mut defs = fog_shader_defs(sea_horizon);
    if mean > 0 {
        defs.push(ShaderDefVal::UInt("FOG_BEFORE_MEAN".into(), mean));
    }
    defs
}

/// The fog's shader defs: the sea's horizon.
fn fog_shader_defs(sea_horizon: bool) -> Vec<ShaderDefVal> {
    let mut defs = Vec::new();
    if !sea_horizon {
        defs.push("SEA_HORIZON_UNFOGGED".into());
    }
    defs
}

/// The latest frame's fog onto the camera.
fn follow_fog(
    mut commands: Commands,
    link: Res<BridgeLink>,
    camera: Single<(Entity, Option<&HaloFog>), With<Camera3d>>,
    mut logged: Local<Option<halo_bridge::Fog>>,
    mut logged_screen: Local<Option<u32>>,
    mut logged_water: Local<Option<bool>>,
    mut modelless_sky: Local<Option<(String, bool)>>,
    mut level: Option<ResMut<crate::bridge::BridgeLevel>>,
    mut images: ResMut<Assets<Image>>,
    viewer_water: Option<Res<ViewerWater>>,
    lit_fog: Option<Res<LitFog>>,
    mut ray_fog: ResMut<bevy_solarik::scene::SolarikRayFog>,
    mut camera_sky: ResMut<crate::bridge::CameraSky>,
) {
    let (entity, current) = *camera;
    let Some(snapshot) = link.latest.as_ref() else { return };
    // (each fog the game changes to, in the log)
    if let Some(fog) = snapshot.fog.filter(|fog| {
        logged.is_none_or(|old| {
            (old.flags, old.planar_mode, old.atmospheric_color, old.planar_color, old.atmospheric_maximum_distance)
                != (fog.flags, fog.planar_mode, fog.atmospheric_color, fog.planar_color, fog.atmospheric_maximum_distance)
        })
    }) {
        info!("the game's fog: {fog:?}");
        *logged = Some(fog);
    }
    // (whether the level's skies have no model: the game clears its picture
    // to the atmospheric fog's colour (rasterizer_window_begin), and with
    // no sky drawn over it that is the sky: b40's snowy white, where a blue
    // stood in, flat, over its canyons; play feedback: the sky incomplete.
    // The pass adds the colour under what was drawn there, the camera's
    // skybox dark (CameraSky::clear): b40's dome, its panes and the energy
    // field behind them, with nothing opaque beyond, were painted over with
    // the fog's colour; HALO_RT_SKY_CLEAR_OVER=1 paints it over, the old)
    if let Some(level) = level.as_deref()
        && modelless_sky.as_ref().is_none_or(|(name, _)| *name != level.builder.stats.map_name)
    {
        let skies = halo_map::sky::skies(&level.builder.cache).unwrap_or_default();
        let modelless = !skies.is_empty() && skies.iter().all(|sky| sky.model.and_then(|m| m.datum).is_none());
        *modelless_sky = Some((level.builder.stats.map_name.clone(), modelless));
    }
    let modelless = modelless_sky.as_ref().is_some_and(|(_, modelless)| *modelless);
    let screen = snapshot.fog_screen.as_ref();
    if screen.map(|s| s.map) != *logged_screen {
        if let Some(screen) = screen {
            info!("the game's fog screen: {screen:?}");
        }
        *logged_screen = screen.map(|s| s.map);
    }
    let fog = snapshot
        .fog
        .as_ref()
        .zip(snapshot.camera.as_ref())
        .and_then(|(fog, camera)| HaloFog::new(fog, screen, camera.position))
        .map(|mut fog| {
            // (the fog plane the water's surface, as b30's sea: not c10's
            // mist above its swamps, under which its land was fogged as if
            // seen through water whenever the eye was over the plane)
            let on_water = snapshot.fog.as_ref().is_some_and(|shared| {
                let [i, j, k, d] = shared.plane;
                k > 0.99 && i.abs() < 0.01 && j.abs() < 0.01
                    && level.as_deref().is_some_and(|level| level.builder.water_heights.iter().any(|height| (height - d).abs() < WATER_PLANE_TOLERANCE))
            });
            if *logged_water != Some(on_water) {
                info!("the fog plane {} the water's surface", if on_water { "is" } else { "isn't" });
                *logged_water = Some(on_water);
            }
            fog.atmospheric.w = if viewer_water.is_some() && on_water { 1.0 } else { 0.0 };
            fog.planar.w = match (modelless, sky_clear_over()) {
                (false, _) => 0.0,
                (true, false) => 1.0,
                (true, true) => 2.0,
            };
            // (the blend's form: lit.w)
            fog.lit.w = FogBefore::get().form();
            fog
        });
    // (the clear under the picture only with the fog on the camera: else the
    // stand-in sky stays)
    let clear = fog.is_some_and(|fog| fog.planar.w == 1.0);
    if camera_sky.clear != clear {
        camera_sky.clear = clear;
    }
    // the same fog over what reflections see beyond their surfaces
    // (`HALO_RT_NO_RAY_FOG`: none, for comparison)
    let traced = fog.filter(|_| std::env::var_os("HALO_RT_NO_RAY_FOG").is_none()).map_or_else(Default::default, |fog| {
        bevy_solarik::scene::SolarikRayFog {
            atmospheric_color: fog.atmospheric_color,
            atmospheric: fog.atmospheric,
            plane: fog.plane,
            planar_color: fog.planar_color,
            // (w: 1, the picture fogged before the denoiser, for the DLSS-RR
            // guides' fade, SOLARIK_RR_FOG_GUIDE; Solarik reads nothing else
            // there, and the modelless sky's flag HaloFog keeps in it is not
            // for Solarik)
            planar: fog.planar.truncate().extend(1.0),
            lit: lit_fog.as_deref().map_or(Vec4::ZERO, LitFog::shared),
        }
    });
    if *ray_fog != traced {
        *ray_fog = traced;
    }
    match (fog, current) {
        (Some(fog), _) => {
            // the layers' frames, from the level's map
            let mut maps = FogScreenMaps::default();
            if let (Some(screen), Some(level)) = (screen, level.as_deref_mut()) {
                let builder = &mut level.builder;
                for (index, layer) in screen.layers.iter().take(screen.layer_count.clamp(0, 4) as usize).enumerate() {
                    maps.0[index] =
                        builder.textures.get(&builder.cache, screen.map, layer.bitmap_index as usize, false, &mut images);
                }
            }
            commands.entity(entity).insert((fog, maps));
        }
        // (none: no density; the passes leave the picture)
        (None, current) => {
            if current != Some(&HaloFog::default()) {
                commands.entity(entity).insert((HaloFog::default(), FogScreenMaps::default()));
            }
        }
    }
}

#[derive(Resource)]
pub(crate) struct FogPipeline {
    /// the fog pass's (draw_fog)
    layout: BindGroupLayoutDescriptor,
    /// the environment fog's (draw_fog_before): the picture's copy for the
    /// local mean, the depths, the view, the fog, lit fog's light and the
    /// picture fogged in place, at the shader's binding numbers
    before_layout: BindGroupLayoutDescriptor,
    sampler: Sampler,
    /// the fog screen's: its layers tile
    layer_sampler: Sampler,
    shader: Handle<Shader>,
}

fn init_pipeline(mut commands: Commands, device: Res<RenderDevice>, assets: Res<AssetServer>) {
    let layout = BindGroupLayoutDescriptor::new(
        "halo_fog",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::FRAGMENT,
            (
                texture_2d(TextureSampleType::Float { filterable: true }),
                sampler(SamplerBindingType::Filtering),
                texture_depth_2d(),
                uniform_buffer::<ViewUniform>(true),
                uniform_buffer::<HaloFog>(false),
                texture_2d(TextureSampleType::Float { filterable: true }),
                texture_2d(TextureSampleType::Float { filterable: true }),
                texture_2d(TextureSampleType::Float { filterable: true }),
                texture_2d(TextureSampleType::Float { filterable: true }),
                sampler(SamplerBindingType::Filtering),
                texture_2d(TextureSampleType::Float { filterable: true }),
                texture_depth_2d(),
                // (the weather's layer, src/weather.rs)
                texture_2d(TextureSampleType::Float { filterable: true }),
            ),
        ),
    );
    let before_layout = BindGroupLayoutDescriptor::new(
        "halo_fog_before",
        &BindGroupLayoutEntries::with_indices(
            ShaderStages::COMPUTE,
            (
                // (the picture's copy, for the local mean)
                (0, texture_2d(TextureSampleType::Float { filterable: true })),
                (2, texture_depth_2d()),
                (3, uniform_buffer::<ViewUniform>(true)),
                (4, uniform_buffer::<HaloFog>(false)),
                (11, texture_depth_2d()),
                // (lit fog's light, loaded)
                (14, texture_2d(TextureSampleType::Float { filterable: true })),
                // (the lit picture, fogged in place)
                (15, texture_storage_2d(TextureFormat::Rgba16Float, StorageTextureAccess::ReadWrite)),
            ),
        ),
    );
    commands.insert_resource(FogPipeline {
        layout,
        before_layout,
        sampler: device.create_sampler(&SamplerDescriptor::default()),
        layer_sampler: device.create_sampler(&SamplerDescriptor {
            address_mode_u: AddressMode::Repeat,
            address_mode_v: AddressMode::Repeat,
            mag_filter: FilterMode::Linear,
            min_filter: FilterMode::Linear,
            mipmap_filter: MipmapFilterMode::Linear,
            ..default()
        }),
        shader: load_embedded_asset!(assets.as_ref(), "shaders/halo_fog.wesl"),
    });
}

pub(crate) fn draw_fog(
    view: ViewQuery<(
        &ViewTarget,
        &ViewPrepassTextures,
        &ViewDepthStencilTexture,
        &ViewUniformOffset,
        &ExtractedView,
        &HaloFog,
        Option<&FogScreenMaps>,
    )>,
    gpu_images: Res<RenderAssets<GpuImage>>,
    fallback: Res<FallbackImage>,
    fallback_zero: Res<FallbackImageZero>,
    (flares, flare_pipeline): (Option<Res<crate::lens_flares::LensFlares>>, Option<Res<crate::lens_flares::LensFlarePipeline>>),
    (weather, weather_pipeline): (Option<Res<crate::weather::Weather>>, Option<Res<crate::weather::WeatherPipeline>>),
    mut texture_cache: ResMut<TextureCache>,
    fog_pipeline: Res<FogPipeline>,
    fullscreen: Res<FullscreenShader>,
    cache: Res<PipelineCache>,
    device: Res<RenderDevice>,
    view_uniforms: Res<ViewUniforms>,
    mut pipelines: Local<HashMap<TextureFormat, CachedRenderPipelineId>>,
    mut ctx: RenderContext,
) {
    let (target, prepass, main_depth, view_offset, extracted, fog, maps) = view.into_inner();
    let layer = |index: usize| {
        maps.and_then(|maps| maps.0[index].as_ref())
            .and_then(|handle| gpu_images.get(handle))
            .map_or(&fallback.d2.texture_view, |image| &image.texture_view)
    };
    let (Some(depth), Some(main_depth), Some(view_binding)) =
        (prepass.depth_only_view(), main_depth.attachment.depth_stencil_views().depth_only_view(), view_uniforms.uniforms.binding())
    else {
        return;
    };
    let id = *pipelines.entry(extracted.target_format).or_insert_with(|| {
        cache.queue_render_pipeline(RenderPipelineDescriptor {
            label: Some("halo_fog".into()),
            layout: vec![fog_pipeline.layout.clone()],
            vertex: fullscreen.to_vertex_state(),
            fragment: Some(FragmentState {
                shader: fog_pipeline.shader.clone(),
                shader_defs: fog_shader_defs(sea_horizon_fog()),
                // (the module holds draw_fog_before's entry point too)
                entry_point: Some("fragment".into()),
                targets: vec![Some(ColorTargetState {
                    format: extracted.target_format,
                    blend: None,
                    write_mask: ColorWrites::ALL,
                })],
                ..default()
            }),
            ..default()
        })
    });
    let Some(pipeline) = cache.get_render_pipeline(id) else { return };
    let mut uniform = UniformBuffer::new(Vec::new());
    if uniform.write(fog).is_err() {
        return;
    }
    let buffer = device.create_buffer_with_data(&BufferInitDescriptor {
        label: Some("halo_fog"),
        contents: uniform.as_ref(),
        usage: BufferUsages::UNIFORM,
    });
    // the lens flares, added up (the picture's size: upscaled by now)
    let flare_view = flares.as_deref().zip(flare_pipeline.as_deref()).and_then(|(flares, flare_pipeline)| {
        crate::lens_flares::draw_lens_flares(
            &mut ctx,
            flares,
            flare_pipeline,
            &cache,
            &device,
            &gpu_images,
            &mut texture_cache,
            &view_uniforms,
            view_offset.offset,
            target.main_texture().size(),
        )
    });
    // the weather, a layer over (the picture's size), each flake faded by
    // its own fog and hidden behind the scene's depths
    let weather_view = weather.as_deref().zip(weather_pipeline.as_deref()).and_then(|(weather, weather_pipeline)| {
        crate::weather::draw_weather(
            &mut ctx,
            weather,
            weather_pipeline,
            &cache,
            &device,
            &gpu_images,
            &mut texture_cache,
            &view_uniforms,
            view_offset.offset,
            &buffer,
            depth,
            main_depth,
            target.main_texture().size(),
        )
    });
    let post_process = target.post_process_write();
    let bind_group = device.create_bind_group(
        "halo_fog",
        &cache.get_bind_group_layout(&fog_pipeline.layout),
        &BindGroupEntries::sequential((
            post_process.source,
            &fog_pipeline.sampler,
            depth,
            view_binding,
            buffer.as_entire_binding(),
            layer(0),
            layer(1),
            layer(2),
            layer(3),
            &fog_pipeline.layer_sampler,
            flare_view.as_ref().unwrap_or(&fallback_zero.texture_view),
            main_depth,
            // (none: transparent black, which lays nothing over)
            weather_view.as_ref().unwrap_or(&fallback_zero.texture_view),
        )),
    );
    let diagnostics = ctx.diagnostic_recorder();
    let diagnostics = diagnostics.as_deref();
    let mut pass = ctx.begin_tracked_render_pass(RenderPassDescriptor {
        label: Some("halo_fog"),
        color_attachments: &[Some(RenderPassColorAttachment {
            view: post_process.destination,
            depth_slice: None,
            resolve_target: None,
            ops: Operations::default(),
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    });
    let pass_span = diagnostics.pass_span(&mut pass, "halo_fog");
    pass.set_render_pipeline(pipeline);
    pass.set_bind_group(0, &bind_group, &[view_offset.offset]);
    pass.draw(0..3, 0..1);
    pass_span.end(&mut pass);
}

/// The environment fog (the fog's `lit.w` its blend's form, 0 with no fog):
/// the lit picture, at the render resolution, after Solarik's lighting and
/// before the transparent pass, fogged in place by `halo_fog.wesl`'s `before`,
/// each render pixel by its own depth, and lit fog's light laid over. Nothing
/// with no fog, or on a picture not of Rgba16Float (the format the pass binds
/// it as).
pub(crate) fn draw_fog_before(
    view: ViewQuery<(
        &ViewTarget,
        &ViewPrepassTextures,
        &ViewDepthStencilTexture,
        &ViewUniformOffset,
        &ExtractedView,
        &HaloFog,
        Option<&SolarikLightingResources>,
    )>,
    (ray_fog, frame_count): (Option<Res<bevy_solarik::scene::SolarikRayFog>>, Res<FrameCount>),
    fallback: Res<FallbackImage>,
    fallback_zero: Res<FallbackImageZero>,
    fog_pipeline: Res<FogPipeline>,
    cache: Res<PipelineCache>,
    device: Res<RenderDevice>,
    view_uniforms: Res<ViewUniforms>,
    mut texture_cache: ResMut<TextureCache>,
    mut pipeline_id: Local<Option<CachedComputePipelineId>>,
    mut ctx: RenderContext,
) {
    let (target, prepass, main_depth, view_offset, extracted, fog, lighting) = view.into_inner();
    if fog.lit.w <= 0.5 || target.main_texture_format() != TextureFormat::Rgba16Float {
        return;
    }
    let (Some(depth), Some(main_depth), Some(view_binding)) =
        (prepass.depth_only_view(), main_depth.attachment.depth_stencil_views().depth_only_view(), view_uniforms.uniforms.binding())
    else {
        return;
    };
    let label = "halo_fog_before";
    let id = *pipeline_id.get_or_insert_with(|| {
        cache.queue_compute_pipeline(ComputePipelineDescriptor {
            label: Some(label.into()),
            layout: vec![fog_pipeline.before_layout.clone()],
            shader: fog_pipeline.shader.clone(),
            shader_defs: fog_before_shader_defs(sea_horizon_fog(), fog_before_mean()),
            entry_point: Some("before".into()),
            ..default()
        })
    });
    let Some(pipeline) = cache.get_compute_pipeline(id) else { return };
    let mut fog = *fog;
    // (lit fog's light, where Solarik's passes ran this frame and left it
    // undivided, lit.w)
    let lit = ray_fog.as_deref().map_or(Vec4::ZERO, |ray_fog| ray_fog.lit);
    let lit_light = (lit.x > 0.0 && lit.w > 0.5)
        .then(|| lighting.and_then(|lighting| lighting.lit_fog_light(frame_count.0)))
        .flatten();
    if lit_light.is_some() {
        fog.lit.x = FogBeforeLit::get().flag();
        fog.lit.y = if lit.z > 0.5 { 1.0 } else { 0.0 };
    }
    let mut uniform = UniformBuffer::new(Vec::new());
    if uniform.write(&fog).is_err() {
        return;
    }
    let buffer = device.create_buffer_with_data(&BufferInitDescriptor {
        label: Some(label),
        contents: uniform.as_ref(),
        usage: BufferUsages::UNIFORM,
    });
    // (with the local mean, a copy of the picture as the source, read round
    // each pixel while the pass writes it)
    let picture = target.get_unsampled_color_attachment();
    let copy = (fog_before_mean() > 0).then(|| {
        let main = target.main_texture();
        let copy = texture_cache.get(
            &device,
            TextureDescriptor {
                label: Some("halo_fog_before_source"),
                size: main.size(),
                mip_level_count: 1,
                sample_count: 1,
                dimension: TextureDimension::D2,
                format: TextureFormat::Rgba16Float,
                usage: TextureUsages::COPY_DST | TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            },
        );
        ctx.command_encoder().copy_texture_to_texture(main.as_image_copy(), copy.texture.as_image_copy(), main.size());
        copy
    });
    let bind_group = device.create_bind_group(
        label,
        &cache.get_bind_group_layout(&fog_pipeline.before_layout),
        &BindGroupEntries::with_indices((
            (0, copy.as_ref().map_or(&fallback.d2.texture_view, |copy| &copy.default_view)),
            (2, depth),
            (3, view_binding),
            (4, buffer.as_entire_binding()),
            (11, main_depth),
            // (none: black, which adds nothing; the flag off besides)
            (14, lit_light.unwrap_or(&fallback_zero.texture_view)),
            (15, picture.view),
        )),
    );
    // (the main pass's viewport: the render resolution, in the corner below
    // DLAA)
    let size = extracted.viewport.zw();
    let diagnostics = ctx.diagnostic_recorder();
    let diagnostics = diagnostics.as_deref();
    let encoder = ctx.command_encoder();
    let mut pass = encoder.begin_compute_pass(&ComputePassDescriptor { label: Some(label), timestamp_writes: None });
    let span = diagnostics.time_span(&mut pass, label);
    pass.set_pipeline(pipeline);
    pass.set_bind_group(0, &bind_group, &[view_offset.offset]);
    pass.dispatch_workgroups(size.x.div_ceil(8), size.y.div_ceil(8), 1);
    span.end(&mut pass);
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::shader::{ShaderCache, ShaderCacheSource};

    #[test]
    fn lit_fog_parse() {
        assert_eq!(LitFog::parse("3"), Some(LitFog { on: true, strength: 3.0, sun: false, only: false }));
        assert_eq!(LitFog::parse("0.5, sun,ONLY"), Some(LitFog { on: true, strength: 0.5, sun: true, only: true }));
        let off = Some(LitFog { on: false, strength: 0.0, sun: false, only: false });
        assert_eq!(LitFog::parse("0"), off);
        assert_eq!(LitFog::parse("Off"), off);
        assert_eq!(LitFog::parse("bright"), None);
        assert_eq!(LitFog::parse("-1"), None);
        assert_eq!(LitFog::parse("nan"), None);
    }

    #[test]
    fn lit_fog_starting() {
        let on = |strength| LitFog { on: true, strength, sun: false, only: false };
        let off = |strength| LitFog { on: false, strength, sun: false, only: false };
        // (nothing given: the saved choice, or off at the default)
        assert_eq!(LitFog::starting(None, None), off(crate::options::LIT_FOG_STRENGTH));
        assert_eq!(LitFog::starting(None, Some((true, 5.0))), on(5.0));
        // (given on: that, flags and all; given off: off at the saved strength)
        let given = LitFog { on: true, strength: 2.0, sun: true, only: false };
        assert_eq!(LitFog::starting(Some(given), Some((false, 5.0))), given);
        assert_eq!(LitFog::starting(LitFog::parse("off"), Some((true, 5.0))), off(5.0));
        // (Solarik told to leave the light undivided and uncomposited, w 1)
        assert_eq!(off(5.0).shared(), Vec4::ZERO);
        assert_eq!(given.shared(), Vec4::new(2.0, 1.0, 0.0, 1.0));
    }

    #[test]
    fn fog_before_switch() {
        // (through the tonemapper's curve by default, 1 straight)
        assert_eq!(FogBefore::parse(None), (FogBefore::Curve, true));
        assert_eq!(FogBefore::parse(Some("0")), (FogBefore::Curve, false));
        assert_eq!(FogBefore::parse(Some(" 1 ")), (FogBefore::Straight, true));
        assert_eq!(FogBefore::parse(Some("tone")), (FogBefore::Curve, true));
        assert_eq!(FogBefore::parse(Some("TONE")), (FogBefore::Curve, true));
        assert_eq!(FogBefore::parse(Some("2")), (FogBefore::Curve, true));
        assert_eq!(FogBefore::parse(Some("")), (FogBefore::Curve, false));
        assert_eq!(FogBefore::parse(Some("yes")), (FogBefore::Curve, false));
        assert_eq!(FogBeforeLit::parse(None), FogBeforeLit::Shoulder);
        assert_eq!(FogBeforeLit::parse(Some("ADD")), FogBeforeLit::Add);
        assert_eq!(FogBeforeLit::parse(Some("0")), FogBeforeLit::Shoulder);
        assert_eq!([FogBeforeLit::Shoulder.flag(), FogBeforeLit::Add.flag()], [1.0, 2.0]);
        assert_eq!([FogBefore::Straight, FogBefore::Curve].map(FogBefore::form), [1.0, 2.0]);
        // (the layers: the camera's planar density left out, the atmosphere's
        // dominance kept as a flag)
        let mut fog = HaloFog { atmospheric_color: Vec4::new(0.2, 0.2, 0.2, 1.0), atmospheric: Vec4::new(0.0, 67.0, 1.0, 0.0), ..default() };
        fog.planar.z = 0.7;
        fog.lit = Vec4::new(1.0, 0.0, 0.5, 2.0);
        assert_eq!(fog.layers()[4].z, 0.0);
        assert_eq!(fog.layers()[1].z, 1.0);
    }

    #[test]
    fn fog_before_mean_switch() {
        // (radius 2 by default; 0 none)
        assert_eq!(parse_fog_before_mean(None), FOG_BEFORE_MEAN_RADIUS);
        assert_eq!(FOG_BEFORE_MEAN_RADIUS, 2);
        assert_eq!(parse_fog_before_mean(Some("0")), 0);
        assert_eq!(parse_fog_before_mean(Some(" 1 ")), 1);
        assert_eq!(parse_fog_before_mean(Some("4")), 4);
        assert_eq!(parse_fog_before_mean(Some("5")), FOG_BEFORE_MEAN_RADIUS);
        assert_eq!(parse_fog_before_mean(Some("x")), FOG_BEFORE_MEAN_RADIUS);
        assert!(!fog_before_shader_defs(true, 0).iter().any(|d| matches!(d, ShaderDefVal::UInt(name, _) if name == "FOG_BEFORE_MEAN")));
        assert!(fog_before_shader_defs(true, 2).iter().any(|d| matches!(d, ShaderDefVal::UInt(name, 2) if name == "FOG_BEFORE_MEAN")));
    }

    /// The fog before the upscaler's blend, mirrored from `halo_fog.wesl`
    /// (its tone tables read from the shader's source): taken about a mean
    /// and carried as a ratio (`affine_about`), it is the plain blend at the
    /// mean, monotone in the pixel, never negative, never steeper than 1 (a
    /// bright noisy sample no brighter than the blend at the mean plus
    /// itself, where the plain blend's per-channel inverse ran to tens), its
    /// average over any noise is the blend of the mean, and under a black fog
    /// a firefly raising the mean raises none of its neighbours.
    #[test]
    fn fog_before_blend_bounded() {
        let source = include_str!("shaders/halo_fog.wesl");
        let table = |name: &str| -> Vec<f32> {
            let start = source.find(&format!("var<private> {name}: array<f32, 64> = array<f32, 64>(")).expect(name);
            let body = &source[start..];
            let open = body.find(">(").unwrap() + 2;
            let close = body.find(");").unwrap();
            body[open..close].split(',').map(|v| v.trim().parse::<f32>().unwrap()).collect()
        };
        let (curve, inverse) = (table("TONE_CURVE"), table("TONE_INVERSE"));
        assert_eq!((curve.len(), inverse.len()), (64, 64));
        let lerp = |t: &[f32], at: f32| {
            let p = at.clamp(0.0, 1.0) * 63.0;
            let i = (p as usize).min(62);
            t[i] + (t[i + 1] - t[i]) * (p - i as f32)
        };
        let (tone_max, cap) = (0.998047_f32, 64.0_f32);
        let tone = |x: f32| lerp(&curve, x.max(0.0) / (x.max(0.0) + 1.0));
        let tone_inverse = |y: f32| {
            let u = lerp(&inverse, y.clamp(0.0, tone_max) / tone_max);
            (u / (1.0 - u).max(1e-6)).min(cap)
        };
        let enc = |x: f32| if x <= 0.0031308 { x * 12.92 } else { 1.055 * x.max(0.0).powf(1.0 / 2.4) - 0.055 };
        let dec = |e: f32| if e <= 0.04045 { e / 12.92 } else { ((e.max(0.0) + 0.055) / 1.055).powf(2.4) };
        // (form 2, one channel: f the fog's light added, b its coverage)
        let blend = |x: f32, f: f32, b: f32| tone_inverse(dec(f + enc(tone(x.max(0.0)).min(1.0)) * (1.0 - b)));
        let affine = |p: f32, m: f32, f: f32, b: f32| {
            let m = m.min(8.0);
            let at_mean = blend(m, f, b);
            let s = ((at_mean - blend(0.0, f, b)) / m.max(1e-6)).clamp(0.0, 1.0);
            (at_mean + (p - m) * s).max(0.0)
        };
        // (dark, mid and white fogs, thin to whole; d20's corridor, c10's
        // swamp, b40's snow)
        for (colour, b) in [(0.2_f32, 0.3_f32), (0.5, 0.5), (0.9, 0.5), (1.0, 0.8), (0.3, 1.0), (0.05, 0.05)] {
            let f = colour * b;
            for m in [0.0005_f32, 0.003, 0.02, 0.2, 1.0, 5.0] {
                // (exact at the mean, and the old tangent's slope for the record)
                assert!((affine(m, m, f, b) - blend(m, f, b)).abs() <= 1e-4 * blend(m, f, b).max(1.0), "{colour} {b} {m}");
                let mut last = -1.0_f32;
                for i in 0..=2000 {
                    let p = i as f32 * 0.5;
                    let out = affine(p, m, f, b);
                    assert!(out.is_finite() && out >= 0.0, "{colour} {b} {m} {p}: {out}");
                    assert!(out >= last - 1e-5, "not monotone at {p}: {out} < {last}");
                    assert!(out <= blend(m.min(8.0), f, b) + p + 1e-4, "{colour} {b} {m} {p}: {out} over the bound");
                    last = out;
                }
                // (any noise about the mean averages to the blend of the mean)
                let samples = [0.0, 0.0, 0.0, 4.0 * m, 0.5 * m, 1.5 * m, 0.0, m];
                let mean = samples.iter().sum::<f32>() / samples.len() as f32;
                if mean <= 8.0 {
                    let averaged = samples.iter().map(|&p| affine(p, mean, f, b)).sum::<f32>() / samples.len() as f32;
                    let exact = blend(mean, f, b);
                    // (the floor at 0 the only way off it, where s is the secant)
                    assert!((averaged - exact).abs() <= 1e-3 * exact.max(0.01), "{colour} {b} {m}: {averaged} against {exact}");
                }
            }
        }
        // (the plain blend of a lone bright sample, the squares' cause: a
        // white fog half over a sample of 30 gives the inverse's cap, over
        // twice the sample; carried from a dark mean, at most the sample)
        assert!(blend(30.0, 0.5, 0.5) > 60.0);
        assert!(affine(30.0, 0.02, 0.5, 0.5) <= 30.0 + blend(0.02, 0.5, 0.5));
        // (d20's black fog: a firefly raising the window's mean does not
        // raise its neighbours, the squares of the first form, which took g's
        // slope at the mean and added its offset to every pixel)
        for b in [0.3_f32, 0.6, 0.9] {
            for (m, raised) in [(0.002_f32, 0.006_f32), (0.02, 0.05), (0.2, 0.6)] {
                for p in [0.0_f32, 0.5 * m, m, 2.0 * m] {
                    assert!(affine(p, raised, 0.0, b) <= affine(p, m, 0.0, b) + 1e-6, "{b} {m} {p}");
                }
            }
        }
    }

    /// The fog's shader composed by Bevy's WESL compiler with and without
    /// `HALO_RT_SEA_HORIZON_FOG`'s def and the local mean's, and validated by
    /// naga, as the pipeline cache would (its two imports: Bevy's view, from
    /// Solarik's fork of bevy_render, and the fullscreen vertex output, from
    /// the registry): both entry points there, and nothing of the removed
    /// paths.
    #[test]
    fn fog_shader_validates() {
        let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let cargo_home = std::env::var_os("CARGO_HOME")
            .map(std::path::PathBuf::from)
            .or_else(|| std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")).map(|home| std::path::PathBuf::from(home).join(".cargo")))
            .expect("cargo's home");
        let core_pipeline = std::fs::read_dir(cargo_home.join("registry/src"))
            .expect("the registry")
            .filter_map(|index| Some(index.ok()?.path().join("bevy_core_pipeline-0.20.0/src/fullscreen_vertex_shader.wesl")))
            .find(|path| path.exists())
            .expect("bevy_core_pipeline 0.20.0's fullscreen shader in the registry");
        let modules = [
            (manifest.join("bevy_solarik/vendor/bevy_render/src/view.wesl"), "embedded://bevy_render/view.wesl"),
            (core_pipeline, "embedded://bevy_core_pipeline/fullscreen_vertex_shader.wesl"),
            (manifest.join("src/shaders/halo_fog.wesl"), "embedded://halo_rt/shaders/halo_fog.wesl"),
        ];
        let mut cache: ShaderCache<String, ()> = ShaderCache::new((), |_, source, _| match source {
            ShaderCacheSource::Wgsl(source) => Ok(source),
            _ => panic!("WESL must produce WGSL"),
        });
        let mut assets = Assets::<Shader>::default();
        let mut fog_id = None;
        for (file, path) in modules {
            let shader = Shader::from_wesl(std::fs::read_to_string(&file).unwrap(), path);
            let id = assets.add(shader.clone()).id();
            cache.set_shader(id, shader);
            fog_id = Some(id);
        }
        let fog_id = fog_id.unwrap();
        for sea_horizon in [true, false] {
            for mean in [0, 2] {
                let defs = fog_before_shader_defs(sea_horizon, mean);
                assert_eq!(fog_shader_defs(sea_horizon).len() + usize::from(mean > 0), defs.len());
                let source = cache.get(0, fog_id, &defs).unwrap_or_else(|e| panic!("{defs:?}: {e}"));
                let module = wgpu::naga::front::wgsl::parse_str(&source).unwrap_or_else(|e| panic!("{defs:?}: {}", e.emit_to_string(&source)));
                wgpu::naga::valid::Validator::new(wgpu::naga::valid::ValidationFlags::all(), wgpu::naga::valid::Capabilities::all())
                    .validate(&module)
                    .unwrap_or_else(|e| panic!("{defs:?}: {}", e.emit_to_string(&source)));
                // (the sea's horizon on or off by the constant)
                let constant = if sea_horizon { "SEA_HORIZON_FOG: bool = true" } else { "SEA_HORIZON_FOG: bool = false" };
                assert!(source.contains(constant), "{defs:?}");
                // (the two entry points, the blend's forms and lit fog's
                // shoulder in `before`, the local mean's loop by its def)
                for entry in ["before", "fragment"] {
                    assert!(module.entry_points.iter().any(|e| e.name == entry), "{defs:?}: {entry}");
                }
                assert_eq!(module.entry_points.len(), 2, "{defs:?}");
                assert!(source.contains("fog_blend_linear(") && source.contains("tone_inverse(") && source.contains("lit_shoulder("), "{defs:?}");
                assert_eq!(source.contains("FOG_MEAN_RADIUS"), mean > 0, "{defs:?}");
                // (nothing of the removed paths: the curtain, the edge guide,
                // lit fog laid over by the fog pass)
                for gone in ["curtain", "guide", "lit_light_at"] {
                    assert!(!source.contains(gone), "{defs:?}: {gone}");
                }
            }
        }
    }
}
