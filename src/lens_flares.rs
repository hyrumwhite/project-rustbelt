//! The game's lens flares, drawn here (ROADMAP: effects in the viewer, step
//! 3). With `game.bevy_bridge_viewer_effects` (and the viewer's fog), the
//! game tests each flare's occlusion against its depth as before, then
//! shares each reflection it would have drawn as a quad facing the camera
//! ([`halo_bridge::SPRITE_LENS_FLARE`] batches) instead of drawing it.
//!
//! The game adds its flares over its finished picture, in the display's
//! encoding, after everything else. So here they're added up in a texture of
//! their own (`shaders/halo_lens_flare.wesl`), which the fog pass adds to
//! the tonemapped picture after the fog and the fog screen, in the display's
//! encoding as well. Drawn into the HDR picture before tonemapping, they had
//! come out much fainter over a bright sky.

use crate::bridge::{BridgeLevel, BridgeLink};
use bevy::{
    asset::{embedded_asset, load_embedded_asset},
    mesh::VertexBufferLayout,
    platform::collections::HashMap,
    prelude::*,
    render::{
        extract_resource::{ExtractResource, ExtractResourcePlugin},
        render_asset::RenderAssets,
        render_resource::{
            binding_types::{sampler, texture_2d, uniform_buffer},
            AddressMode, BindGroupEntries, BindGroupLayoutDescriptor, BindGroupLayoutEntries, BlendComponent,
            BlendFactor, BlendOperation, BlendState, BufferInitDescriptor, BufferUsages, CachedRenderPipelineId,
            ColorTargetState, ColorWrites, Extent3d, FilterMode, FragmentState, LoadOp, Operations, PipelineCache,
            RenderPassColorAttachment, RenderPassDescriptor, RenderPipelineDescriptor, Sampler, SamplerBindingType,
            SamplerDescriptor, ShaderStages, StoreOp, TextureDescriptor, TextureDimension, TextureFormat,
            TextureSampleType, TextureUsages, TextureView, VertexFormat, VertexState, VertexStepMode,
        },
        renderer::{RenderContext, RenderDevice},
        texture::{GpuImage, TextureCache},
        view::{ViewUniform, ViewUniforms},
        RenderApp, RenderStartup,
    },
};
use halo_bridge::SPRITE_LENS_FLARE;
use halo_map::math::{halo_to_bevy, METERS_PER_WORLD_UNIT};

pub struct LensFlaresPlugin;

impl Plugin for LensFlaresPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "shaders/halo_lens_flare.wesl");
        app.init_resource::<LensFlares>()
            .add_plugins(ExtractResourcePlugin::<LensFlares>::default())
            .add_systems(Update, follow_lens_flares);
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else { return };
        render_app.add_systems(RenderStartup, init_pipeline);
    }
}

/// A reflection's corner: where it is (this world's terms), the bitmap's
/// texture coordinates, the colour (display-encoded; alpha its brightness)
/// and its tint factor.
#[repr(C)]
#[derive(Clone, Copy, Default, bytemuck::Pod, bytemuck::Zeroable)]
struct FlareVertex {
    position: [f32; 3],
    uv: [f32; 2],
    color: [f32; 4],
    tint: f32,
}

/// The latest frame's reflections: two triangles each, and the bitmap each
/// run of them is drawn with.
#[derive(Resource, Clone, Default, ExtractResource)]
#[extract_app(RenderApp)]
pub(crate) struct LensFlares {
    vertices: Vec<FlareVertex>,
    /// (bitmap, first vertex, vertex count)
    runs: Vec<(Handle<Image>, u32, u32)>,
}

fn follow_lens_flares(
    link: Res<BridgeLink>,
    mut flares: ResMut<LensFlares>,
    mut level: Option<ResMut<BridgeLevel>>,
    mut images: ResMut<Assets<Image>>,
    mut bitmaps: Local<HashMap<(u32, u16), Option<Handle<Image>>>>,
    mut scenario: Local<String>,
    mut last_frame_index: Local<u32>,
) {
    let Some(snapshot) = link.latest.as_ref() else { return };
    if snapshot.frame_index == *last_frame_index {
        return;
    }
    *last_frame_index = snapshot.frame_index;
    // a new level: its own bitmaps
    if snapshot.scenario_name != *scenario {
        bitmaps.clear();
        *scenario = snapshot.scenario_name.clone();
    }
    let flares = flares.as_mut();
    if flares.runs.is_empty() && !snapshot.sprite_batches.iter().any(|b| b.kind == SPRITE_LENS_FLARE) {
        return;
    }
    flares.vertices.clear();
    flares.runs.clear();
    for batch in snapshot.sprite_batches.iter().filter(|b| b.kind == SPRITE_LENS_FLARE) {
        let first = batch.first_vertex as usize;
        let Some(corners) = snapshot.sprite_vertices.get(first..first + batch.quad_count as usize * 4) else { continue };
        let image = bitmaps
            .entry((batch.map, batch.bitmap_index))
            .or_insert_with(|| {
                level.as_deref_mut().and_then(|level| {
                    let builder = &mut level.builder;
                    builder.textures.get(&builder.cache, batch.map, batch.bitmap_index as usize, true, &mut images)
                })
            })
            .clone();
        let Some(image) = image else { continue };
        let start = flares.vertices.len() as u32;
        for quad in corners.chunks_exact(4) {
            for index in [0, 1, 2, 0, 2, 3] {
                let v = &quad[index];
                let c = v.color;
                flares.vertices.push(FlareVertex {
                    position: halo_to_bevy(v.position, METERS_PER_WORLD_UNIT),
                    uv: v.texcoord,
                    color: [
                        ((c >> 16) & 255) as f32 / 255.0,
                        ((c >> 8) & 255) as f32 / 255.0,
                        (c & 255) as f32 / 255.0,
                        (c >> 24) as f32 / 255.0,
                    ],
                    tint: batch.tint,
                });
            }
        }
        let count = flares.vertices.len() as u32 - start;
        match flares.runs.last_mut() {
            Some((last, _, last_count)) if *last == image => *last_count += count,
            _ => flares.runs.push((image, start, count)),
        }
    }
}

#[derive(Resource)]
pub(crate) struct LensFlarePipeline {
    view_layout: BindGroupLayoutDescriptor,
    map_layout: BindGroupLayoutDescriptor,
    sampler: Sampler,
    pipeline: CachedRenderPipelineId,
}

/// What the flares are added up in: display-encoded light, added.
const FLARE_FORMAT: TextureFormat = TextureFormat::Rgba16Float;

fn init_pipeline(mut commands: Commands, device: Res<RenderDevice>, assets: Res<AssetServer>, cache: Res<PipelineCache>) {
    let view_layout = BindGroupLayoutDescriptor::new(
        "halo_lens_flare_view",
        &BindGroupLayoutEntries::single(ShaderStages::VERTEX, uniform_buffer::<ViewUniform>(true)),
    );
    let map_layout = BindGroupLayoutDescriptor::new(
        "halo_lens_flare_map",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::FRAGMENT,
            (texture_2d(TextureSampleType::Float { filterable: true }), sampler(SamplerBindingType::Filtering)),
        ),
    );
    let shader: Handle<Shader> = load_embedded_asset!(assets.as_ref(), "shaders/halo_lens_flare.wesl");
    let add = BlendComponent { src_factor: BlendFactor::One, dst_factor: BlendFactor::One, operation: BlendOperation::Add };
    let pipeline = cache.queue_render_pipeline(RenderPipelineDescriptor {
        label: Some("halo_lens_flare".into()),
        layout: vec![view_layout.clone(), map_layout.clone()],
        vertex: VertexState {
            shader: shader.clone(),
            entry_point: Some("vertex".into()),
            buffers: vec![VertexBufferLayout::from_vertex_formats(
                VertexStepMode::Vertex,
                [VertexFormat::Float32x3, VertexFormat::Float32x2, VertexFormat::Float32x4, VertexFormat::Float32],
            )],
            ..default()
        },
        fragment: Some(FragmentState {
            shader,
            entry_point: Some("fragment".into()),
            targets: vec![Some(ColorTargetState {
                format: FLARE_FORMAT,
                blend: Some(BlendState { color: add, alpha: add }),
                write_mask: ColorWrites::ALL,
            })],
            ..default()
        }),
        ..default()
    });
    commands.insert_resource(LensFlarePipeline {
        view_layout,
        map_layout,
        // (the game's: linear, its border outside)
        sampler: device.create_sampler(&SamplerDescriptor {
            address_mode_u: AddressMode::ClampToEdge,
            address_mode_v: AddressMode::ClampToEdge,
            mag_filter: FilterMode::Linear,
            min_filter: FilterMode::Linear,
            ..default()
        }),
        pipeline,
    });
}

/// The frame's flares added up in a texture the picture's size, for the fog
/// pass to add to the picture; none when there are none (or they aren't
/// ready).
#[allow(clippy::too_many_arguments)]
pub(crate) fn draw_lens_flares(
    ctx: &mut RenderContext,
    flares: &LensFlares,
    flare_pipeline: &LensFlarePipeline,
    cache: &PipelineCache,
    device: &RenderDevice,
    gpu_images: &RenderAssets<GpuImage>,
    texture_cache: &mut TextureCache,
    view_uniforms: &ViewUniforms,
    view_offset: u32,
    size: Extent3d,
) -> Option<TextureView> {
    if flares.runs.is_empty() {
        return None;
    }
    let pipeline = cache.get_render_pipeline(flare_pipeline.pipeline)?;
    let view_binding = view_uniforms.uniforms.binding()?;

    let texture = texture_cache.get(
        device,
        TextureDescriptor {
            label: Some("halo_lens_flares"),
            size: Extent3d { depth_or_array_layers: 1, ..size },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: FLARE_FORMAT,
            usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        },
    );
    let vertices = device.create_buffer_with_data(&BufferInitDescriptor {
        label: Some("halo_lens_flares"),
        contents: bytemuck::cast_slice(&flares.vertices),
        usage: BufferUsages::VERTEX,
    });
    let view_group =
        device.create_bind_group("halo_lens_flare_view", &cache.get_bind_group_layout(&flare_pipeline.view_layout), &BindGroupEntries::single(view_binding));
    let map_layout = cache.get_bind_group_layout(&flare_pipeline.map_layout);
    let map_groups: Vec<_> = flares
        .runs
        .iter()
        .filter_map(|(image, first, count)| {
            let image = gpu_images.get(image)?;
            let group = device.create_bind_group(
                "halo_lens_flare_map",
                &map_layout,
                &BindGroupEntries::sequential((&image.texture_view, &flare_pipeline.sampler)),
            );
            Some((group, *first, *count))
        })
        .collect();

    let mut pass = ctx.begin_tracked_render_pass(RenderPassDescriptor {
        label: Some("halo_lens_flares"),
        color_attachments: &[Some(RenderPassColorAttachment {
            view: &texture.default_view,
            depth_slice: None,
            resolve_target: None,
            ops: Operations { load: LoadOp::Clear(default()), store: StoreOp::Store },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    });
    pass.set_render_pipeline(pipeline);
    pass.set_bind_group(0, &view_group, &[view_offset]);
    pass.set_vertex_buffer(0, vertices.slice(..));
    for (group, first, count) in &map_groups {
        pass.set_bind_group(1, group, &[]);
        pass.draw(*first..*first + *count, 0..1);
    }
    drop(pass);
    Some(texture.default_view)
}
