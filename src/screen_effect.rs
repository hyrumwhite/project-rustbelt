//! The game's screen effect, drawn here: a script's cinematic filter (c10's
//! helmet recording, the light enhancement of a dark scene, its video
//! scanlines and noise) or a HUD's (night vision). With
//! `game.bevy_bridge_viewer_hud`, the game leaves its screen effect pass out
//! of its own picture, which holds only what is drawn over the world, and
//! shares the effect it would have drawn ([`halo_bridge::ScreenEffect`]);
//! this draws it over the traced picture, after tonemapping and the fog, in
//! the display's encoding, as the game does (`shaders/halo_screen_effect.wesl`
//! has the equations). The HUD is drawn after it, over it.

use crate::bridge::{BridgeLevel, BridgeLink};
use bevy::{
    asset::{embedded_asset, load_embedded_asset},
    core_pipeline::{schedule::Core3d, tonemapping::tonemapping, Core3dSystems, FullscreenShader},
    platform::collections::HashMap,
    prelude::*,
    render::{
        extract_resource::{ExtractResource, ExtractResourcePlugin},
        render_asset::RenderAssets,
        render_resource::{
            binding_types::{sampler, texture_2d, uniform_buffer},
            encase::UniformBuffer,
            AddressMode, BindGroupEntries, BindGroupLayoutDescriptor, BindGroupLayoutEntries, BufferInitDescriptor,
            BufferUsages, CachedRenderPipelineId, ColorTargetState, ColorWrites, FilterMode, FragmentState, Operations,
            PipelineCache, RenderPassColorAttachment, RenderPassDescriptor, RenderPipelineDescriptor, Sampler,
            SamplerBindingType, SamplerDescriptor, ShaderStages, ShaderType, TextureFormat, TextureSampleType,
        },
        renderer::{RenderContext, RenderDevice, ViewQuery},
        texture::{FallbackImage, GpuImage},
        view::{ExtractedView, ViewTarget},
        RenderApp, RenderStartup,
    },
};
use halo_bridge::{
    CONVOLUTION_WARP, SCREEN_EFFECT_DESATURATION_ADDITIVE, SCREEN_EFFECT_DESATURATION_MASKED,
    SCREEN_EFFECT_LIGHT_ENHANCEMENT_MASKED, SCREEN_EFFECT_ON, SCREEN_EFFECT_VIDEO,
};

/// The game's view's height in pixels: its screen effect's maps are a texel
/// to a pixel of it (852 by 480, drawn larger).
const GAME_VIEW_HEIGHT: f32 = 480.0;
/// The video effect's maps (the rasterizer's globals'): scanlines with a
/// vignette round a 640 by 480 window, and noise.
const SCANLINE_MAP: &str = "rasterizer\\video mask";
const NOISE_MAP: &str = "rasterizer\\video noise";
/// The overbright modes' scales (the combiner's output shift).
const OVERBRIGHT_SCALES: [f32; 3] = [1.0, 2.0, 4.0];

pub struct ScreenEffectPlugin;

impl Plugin for ScreenEffectPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "shaders/halo_screen_effect.wesl");
        app.init_resource::<HaloScreenEffect>()
            .add_plugins(ExtractResourcePlugin::<HaloScreenEffect>::default())
            .add_systems(Update, follow_screen_effect);
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else { return };
        render_app.add_systems(RenderStartup, init_pipeline).add_systems(
            Core3d,
            draw_screen_effect.in_set(Core3dSystems::PostProcess).after(tonemapping).after(crate::fog::draw_fog),
        );
    }
}

/// The effect's values as the shader takes them (`ScreenEffect`).
#[derive(Clone, Copy, Default, PartialEq, ShaderType)]
struct ScreenEffectUniform {
    /// light enhancement, desaturation, 1 if the desaturation is added, 1
    /// for the video effect
    filtering: Vec4,
    /// the desaturation's tint, the video's overbright scale
    tint: Vec4,
    /// the video's noise intensity, the noise map's offset (texels), the
    /// game's view's height
    video: Vec4,
    /// a HUD's zoom: 1 with its mask, its warp's radius (game pixels, 0
    /// none), 1 if the light enhancement and 1 if the desaturation fade
    /// with the mask
    zoom: Vec4,
}

/// The latest frame's screen effect (none when `on` is false), and the
/// video effect's maps.
#[derive(Resource, Clone, Default, ExtractResource)]
#[extract_app(RenderApp)]
pub struct HaloScreenEffect {
    on: bool,
    uniform: ScreenEffectUniform,
    scanline_map: Option<Handle<Image>>,
    noise_map: Option<Handle<Image>>,
    mask_map: Option<Handle<Image>>,
}

fn follow_screen_effect(
    link: Res<BridgeLink>,
    mut effect: ResMut<HaloScreenEffect>,
    mut level: Option<ResMut<BridgeLevel>>,
    mut images: ResMut<Assets<Image>>,
    mut maps: Local<Option<(String, Option<Handle<Image>>, Option<Handle<Image>>)>>,
) {
    let Some(snapshot) = &link.latest else { return };
    let shared = snapshot.screen_effect;
    if shared.flags & SCREEN_EFFECT_ON == 0 {
        if effect.on {
            info!("screen effect: none");
            effect.on = false;
        }
        return;
    }
    if !effect.on {
        info!("screen effect: {shared:?}");
    }
    let video = shared.flags & SCREEN_EFFECT_VIDEO != 0;
    // (the maps, once a level: its tags')
    if video && maps.as_ref().is_none_or(|(scenario, ..)| *scenario != snapshot.scenario_name) {
        let mut get = |name: &str| {
            let level = level.as_deref_mut()?;
            let builder = &mut level.builder;
            let datum = builder.cache.find_tag(halo_map::groups::BITMAP, name)?.datum;
            builder.textures.get(&builder.cache, datum, 0, false, &mut images)
        };
        let scanline = get(SCANLINE_MAP);
        let noise = get(NOISE_MAP);
        if scanline.is_none() || noise.is_none() {
            warn!("screen effect: the video maps aren't in this map ({SCANLINE_MAP}, {NOISE_MAP})");
        }
        *maps = Some((snapshot.scenario_name.clone(), scanline, noise));
    }
    // (the noise moves on each game tick, by a hash of it)
    let hash = |seed: u32| {
        let mut h = seed.wrapping_mul(0x9e37_79b9) ^ 0x85eb_ca6b;
        h ^= h >> 16;
        h = h.wrapping_mul(0x7feb_352d);
        h ^= h >> 15;
        (h & 0xff) as f32
    };
    let (scanline_map, noise_map) = maps.as_ref().map_or((None, None), |(_, s, n)| (s.clone(), n.clone()));
    // a HUD's zoom mask (the pistol's, the sniper's)
    let mask_map = (shared.mask_bitmap != u32::MAX)
        .then(|| {
            let builder = &mut level.as_deref_mut()?.builder;
            builder.textures.get(&builder.cache, shared.mask_bitmap, shared.mask_bitmap_index as usize, false, &mut images)
        })
        .flatten();
    // (the game's only convolution here is the zoom's warp; a blur is a
    // cinematic one no level's script asks for)
    let warp = if shared.convolution_type == CONVOLUTION_WARP { shared.convolution_radius.max(0.0) } else { 0.0 };
    *effect = HaloScreenEffect {
        on: true,
        uniform: ScreenEffectUniform {
            filtering: Vec4::new(
                shared.light_enhancement,
                shared.desaturation,
                f32::from(u8::from(shared.flags & SCREEN_EFFECT_DESATURATION_ADDITIVE != 0)),
                f32::from(u8::from(video)),
            ),
            tint: Vec3::from(shared.desaturation_tint)
                .extend(OVERBRIGHT_SCALES[(shared.video_overbright_mode as usize).min(2)]),
            video: Vec4::new(
                shared.video_noise_intensity,
                hash(snapshot.game_tick.wrapping_mul(2)),
                hash(snapshot.game_tick.wrapping_mul(2) + 1),
                GAME_VIEW_HEIGHT,
            ),
            zoom: Vec4::new(
                f32::from(u8::from(mask_map.is_some())),
                warp,
                f32::from(u8::from(shared.flags & SCREEN_EFFECT_LIGHT_ENHANCEMENT_MASKED != 0)),
                f32::from(u8::from(shared.flags & SCREEN_EFFECT_DESATURATION_MASKED != 0)),
            ),
        },
        scanline_map,
        noise_map,
        mask_map,
    };
}

#[derive(Resource)]
struct ScreenEffectPipeline {
    layout: BindGroupLayoutDescriptor,
    sampler: Sampler,
    /// the maps': they tile
    map_sampler: Sampler,
    /// the zoom's mask's and warp's: filtered, clamped
    linear_sampler: Sampler,
    shader: Handle<Shader>,
}

fn init_pipeline(mut commands: Commands, device: Res<RenderDevice>, assets: Res<AssetServer>) {
    let layout = BindGroupLayoutDescriptor::new(
        "halo_screen_effect",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::FRAGMENT,
            (
                texture_2d(TextureSampleType::Float { filterable: true }),
                sampler(SamplerBindingType::Filtering),
                uniform_buffer::<ScreenEffectUniform>(false),
                texture_2d(TextureSampleType::Float { filterable: true }),
                texture_2d(TextureSampleType::Float { filterable: true }),
                sampler(SamplerBindingType::Filtering),
                texture_2d(TextureSampleType::Float { filterable: true }),
                sampler(SamplerBindingType::Filtering),
            ),
        ),
    );
    commands.insert_resource(ScreenEffectPipeline {
        layout,
        sampler: device.create_sampler(&SamplerDescriptor::default()),
        map_sampler: device.create_sampler(&SamplerDescriptor {
            address_mode_u: AddressMode::Repeat,
            address_mode_v: AddressMode::Repeat,
            mag_filter: FilterMode::Nearest,
            min_filter: FilterMode::Nearest,
            ..default()
        }),
        linear_sampler: device.create_sampler(&SamplerDescriptor {
            mag_filter: FilterMode::Linear,
            min_filter: FilterMode::Linear,
            ..default()
        }),
        shader: load_embedded_asset!(assets.as_ref(), "shaders/halo_screen_effect.wesl"),
    });
}

fn draw_screen_effect(
    view: ViewQuery<(&ViewTarget, &ExtractedView)>,
    effect: Res<HaloScreenEffect>,
    gpu_images: Res<RenderAssets<GpuImage>>,
    fallback: Res<FallbackImage>,
    effect_pipeline: Res<ScreenEffectPipeline>,
    fullscreen: Res<FullscreenShader>,
    cache: Res<PipelineCache>,
    device: Res<RenderDevice>,
    mut pipelines: Local<HashMap<TextureFormat, CachedRenderPipelineId>>,
    mut ctx: RenderContext,
) {
    if !effect.on {
        return;
    }
    let (target, extracted) = view.into_inner();
    let id = *pipelines.entry(extracted.target_format).or_insert_with(|| {
        cache.queue_render_pipeline(RenderPipelineDescriptor {
            label: Some("halo_screen_effect".into()),
            layout: vec![effect_pipeline.layout.clone()],
            vertex: fullscreen.to_vertex_state(),
            fragment: Some(FragmentState {
                shader: effect_pipeline.shader.clone(),
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
    if uniform.write(&effect.uniform).is_err() {
        return;
    }
    let buffer = device.create_buffer_with_data(&BufferInitDescriptor {
        label: Some("halo_screen_effect"),
        contents: uniform.as_ref(),
        usage: BufferUsages::UNIFORM,
    });
    let map = |handle: &Option<Handle<Image>>| {
        handle.as_ref().and_then(|handle| gpu_images.get(handle)).map_or(&fallback.d2.texture_view, |image| &image.texture_view)
    };
    let post_process = target.post_process_write();
    let bind_group = device.create_bind_group(
        "halo_screen_effect",
        &cache.get_bind_group_layout(&effect_pipeline.layout),
        &BindGroupEntries::sequential((
            post_process.source,
            &effect_pipeline.sampler,
            buffer.as_entire_binding(),
            map(&effect.scanline_map),
            map(&effect.noise_map),
            &effect_pipeline.map_sampler,
            map(&effect.mask_map),
            &effect_pipeline.linear_sampler,
        )),
    );
    let mut pass = ctx.begin_tracked_render_pass(RenderPassDescriptor {
        label: Some("halo_screen_effect"),
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
    pass.set_bind_group(0, &bind_group, &[]);
    pass.draw(0..3, 0..1);
}
