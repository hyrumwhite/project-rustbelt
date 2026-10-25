//! The game's fog, drawn here (ROADMAP: effects in the viewer, step 1). With
//! `game.bevy_bridge_viewer_fog`, the game leaves its environment fog pass
//! out of the overlay and shares the fog it would have drawn with each frame
//! ([`halo_bridge::Fog`]); this fogs the traced picture with it, after
//! tonemapping, in the display's encoding, as the game blends it
//! (`shaders/halo_fog.wesl` has the equations). The fog now covers objects
//! too, which in play the game's own model shaders fog and the overlay
//! never could.
//!
//! The fog screen too ([`halo_bridge::FogScreen`]): the game moves its
//! layers on each frame without drawing them (under the overlay it never
//! drew them at all), and this draws them, with their bitmap's frames.
//!
//! And last, the game's lens flares (`src/lens_flares.rs`), added up first
//! in a texture of their own, then added in the display's encoding.
//!
//! Where this viewer draws the water ([`ViewerWater`]), the planar fog seen
//! through it covers only the light that came through the surface: the game
//! fogs the seabed under its fog plane (the sea's surface, in b30) and draws
//! its water's reflection over that, but here the water is already in the
//! picture, its reflection and the seabed through it together, and under
//! the full fog the sea was a flat teal.

use crate::bridge::BridgeLink;
use bevy::{
    asset::{embedded_asset, load_embedded_asset},
    core_pipeline::{
        prepass::ViewPrepassTextures, schedule::Core3d, tonemapping::tonemapping, Core3dSystems, FullscreenShader,
    },
    platform::collections::HashMap,
    prelude::*,
    render::{
        extract_component::{ExtractComponent, ExtractComponentPlugin},
        render_asset::RenderAssets,
        storage::ShaderBuffer,
        texture::{FallbackImage, FallbackImageZero, GpuImage, TextureCache},
        render_resource::{
            binding_types::{sampler, texture_2d, texture_depth_2d, uniform_buffer},
            encase::UniformBuffer,
            BindGroupEntries, BindGroupLayoutDescriptor, BindGroupLayoutEntries, BufferInitDescriptor, BufferUsages,
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
use halo_map::math::{halo_to_bevy, METERS_PER_WORLD_UNIT};

pub struct FogPlugin;

/// This viewer draws the water (and glass) itself, ray traced, not the
/// game's overlay.
#[derive(Resource)]
pub struct ViewerWater;

/// The fog lit by the scene's lights (Solarik's lit fog: single scattering
/// of the lights along each view ray, through the game's fog): its strength,
/// and whether the sun lights it too (the game's fog colour already stands
/// for the daylight, so not by default).
#[derive(Resource, Clone, Copy, Debug)]
pub struct LitFog {
    pub strength: f32,
    pub sun: bool,
    /// The scattered light alone, for looking at it.
    pub only: bool,
}

impl LitFog {
    /// "strength[,sun][,only]"; none for a strength of 0 or one that isn't a number.
    pub fn parse(text: &str) -> Option<Self> {
        let mut parts = text.split(',').map(str::trim);
        let strength = parts.next()?.parse::<f32>().ok().filter(|s| *s > 0.0 && s.is_finite())?;
        let (mut sun, mut only) = (false, false);
        for part in parts {
            sun |= part.eq_ignore_ascii_case("sun");
            only |= part.eq_ignore_ascii_case("only");
        }
        Some(Self { strength, sun, only })
    }
}

impl Plugin for FogPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "shaders/halo_fog.wesl");
        app.add_plugins((ExtractComponentPlugin::<HaloFog>::default(), ExtractComponentPlugin::<FogScreenMaps>::default()))
            .add_systems(Update, (follow_fog, fog_transparent_surfaces));
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else { return };
        render_app
            .add_systems(RenderStartup, init_pipeline)
            .add_systems(Core3d, draw_fog.in_set(Core3dSystems::PostProcess).after(tonemapping));
    }
}

/// The fog the camera's picture is drawn with: the game's, in this world's
/// terms (metres, Y up); none (no density) when the game has none, for the
/// pass still does the multiply.
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
    /// maximum distance and depth, the camera's density
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
        })
    }
}

/// The camera's fog onto the transparent surfaces, which fade by their own
/// (`halo_transparent.wesl`); `HALO_RT_NO_SPRITE_FOG` leaves them, as the
/// sprites, to the fog pass.
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

/// The latest frame's fog onto the camera.
fn follow_fog(
    mut commands: Commands,
    link: Res<BridgeLink>,
    camera: Single<(Entity, Option<&HaloFog>), With<Camera3d>>,
    mut logged: Local<Option<halo_bridge::Fog>>,
    mut logged_screen: Local<Option<u32>>,
    mut level: Option<ResMut<crate::bridge::BridgeLevel>>,
    mut images: ResMut<Assets<Image>>,
    viewer_water: Option<Res<ViewerWater>>,
    lit_fog: Option<Res<LitFog>>,
    mut ray_fog: ResMut<bevy_solarik::scene::SolarikRayFog>,
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
            fog.atmospheric.w = if viewer_water.is_some() { 1.0 } else { 0.0 };
            fog
        });
    // the same fog over what reflections see beyond their surfaces
    // (`HALO_RT_NO_RAY_FOG`: none, for comparison)
    let traced = fog.filter(|_| std::env::var_os("HALO_RT_NO_RAY_FOG").is_none()).map_or_else(Default::default, |fog| {
        bevy_solarik::scene::SolarikRayFog {
            atmospheric_color: fog.atmospheric_color,
            atmospheric: fog.atmospheric,
            plane: fog.plane,
            planar_color: fog.planar_color,
            planar: fog.planar,
            lit: lit_fog.as_deref().map_or(Vec4::ZERO, |lit| Vec4::new(lit.strength, f32::from(u8::from(lit.sun)), f32::from(u8::from(lit.only)), 0.0)),
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
        // (none: no density, but the pass still multiplies)
        (None, current) => {
            if current != Some(&HaloFog::default()) {
                commands.entity(entity).insert((HaloFog::default(), FogScreenMaps::default()));
            }
        }
    }
}

#[derive(Resource)]
pub(crate) struct FogPipeline {
    layout: BindGroupLayoutDescriptor,
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
            ),
        ),
    );
    commands.insert_resource(FogPipeline {
        layout,
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
    let fog = *fog;

    let mut uniform = UniformBuffer::new(Vec::new());
    if uniform.write(&fog).is_err() {
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
        )),
    );
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
    pass.set_render_pipeline(pipeline);
    pass.set_bind_group(0, &bind_group, &[view_offset.offset]);
    pass.draw(0..3, 0..1);
}
