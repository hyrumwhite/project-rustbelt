//! The game's HUD and menus, drawn here (ROADMAP: the HUD and menus in the
//! viewer, step 1). With `game.bevy_bridge_viewer_hud`, the game keeps its
//! HUD's and menus' logic but shares each screen quad it would have drawn
//! ([`halo_bridge::ScreenDraw`]: its vertices, bitmaps, the NV2A pixel shader
//! it built and its blend) instead of drawing it into its overlay.
//!
//! They're drawn in the game's order into a texture of their own, at the
//! window's resolution, in the display's encoding: each combiner program
//! evaluated as the game's GPU would (`shaders/halo_hud.wesl`), each blend
//! as the port's overlay takes it over a clear picture (blends that replace
//! what is under them add their coverage, additive blends keep it, and
//! multiplying blends, with nothing under them, are left out). The texture
//! is composited over the picture, premultiplied, as the game's overlay is.

use crate::bridge::{BridgeLevel, BridgeLink};
use crate::overlay::OverlayMaterial;
use bevy::{
    asset::{embedded_asset, load_embedded_asset, RenderAssetUsages},
    core_pipeline::{schedule::Core3d, Core3dSystems},
    mesh::VertexBufferLayout,
    platform::collections::HashMap,
    prelude::*,
    render::{
        extract_resource::{ExtractResource, ExtractResourcePlugin},
        render_asset::RenderAssets,
        render_resource::{
            binding_types::{sampler, storage_buffer_read_only_sized, texture_2d},
            AddressMode, BindGroupEntries, BindGroupLayoutDescriptor, BindGroupLayoutEntries, BlendComponent,
            BlendFactor, BlendOperation, BlendState, BufferInitDescriptor, BufferUsages, CachedRenderPipelineId,
            ColorTargetState, ColorWrites, Extent3d, FilterMode, FragmentState, LoadOp, MipmapFilterMode, Operations,
            PipelineCache, RenderPassColorAttachment, RenderPassDescriptor, RenderPipelineDescriptor, Sampler,
            SamplerBindingType, SamplerDescriptor, ShaderStages, StoreOp, TextureDimension, TextureFormat,
            TextureSampleType, TextureUsages, TextureViewDescriptor, VertexFormat, VertexState, VertexStepMode,
        },
        renderer::{RenderContext, RenderDevice},
        texture::{FallbackImage, GpuImage},
        view::ExtractedView,
        RenderApp, RenderStartup,
    },
    window::PrimaryWindow,
};
use halo_bridge::{ScreenDraw, SCREEN_DRAW_ALPHA_KILL, SCREEN_DRAW_BLENDED, SCREEN_MAP_POINT_SAMPLED, SCREEN_MAP_WRAPPED};

pub struct HudPlugin;

impl Plugin for HudPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "shaders/halo_hud.wesl");
        app.init_resource::<HudStats>()
            .add_plugins(ExtractResourcePlugin::<HudList>::default())
            .add_systems(Startup, spawn_hud)
            .add_systems(Update, (size_hud, follow_hud).chain());
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else { return };
        render_app
            .add_systems(RenderStartup, init_pipeline)
            .add_systems(Core3d, draw_hud.in_set(Core3dSystems::PostProcess));
    }
}

/// Screen quads drawn, for the status text.
#[derive(Resource, Default)]
pub struct HudStats {
    pub drawn: usize,
    /// quads whose bitmaps weren't to be had
    pub missing: usize,
}

/// A quad's corner: the viewport's pixels, each map's texture coordinates
/// and the colour (v0).
#[repr(C)]
#[derive(Clone, Copy, Default, bytemuck::Pod, bytemuck::Zeroable)]
struct HudVertex {
    position: [f32; 2],
    uv: [[f32; 2]; 3],
    color: [f32; 4],
}

/// A quad's program as the shader reads it (`HudDraw`).
#[repr(C)]
#[derive(Clone, Copy, Default, bytemuck::Pod, bytemuck::Zeroable)]
struct HudDrawGpu {
    viewport: [f32; 4],
    program: [u32; 4],
    rgb_inputs: [u32; 8],
    rgb_outputs: [u32; 8],
    alpha_inputs: [u32; 8],
    alpha_outputs: [u32; 8],
    constant0: [[f32; 4]; 8],
    constant1: [[f32; 4]; 8],
    modes: [u32; 4],
}

/// How a quad is blended: the game's factors and operation (GL's enums),
/// and whether it blends at all.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct BlendKey {
    source: u32,
    destination: u32,
    operation: u32,
    blended: bool,
}

#[derive(Clone)]
struct HudItem {
    maps: [Option<Handle<Image>>; 3],
    /// per map: bit 0 wrapped, bit 1 point sampled
    samplers: [u8; 3],
    blend: BlendKey,
    /// the blend's constant colour (display-encoded, 0..1)
    blend_color: [f64; 4],
}

/// The latest frame's quads, in the game's order, and the texture they're
/// drawn into.
#[derive(Resource, Clone, ExtractResource)]
#[extract_app(RenderApp)]
pub(crate) struct HudList {
    image: Handle<Image>,
    material: Handle<OverlayMaterial>,
    size: (u32, u32),
    vertices: Vec<HudVertex>,
    draws: Vec<HudDrawGpu>,
    items: Vec<HudItem>,
}

/// The texture the HUD is drawn into: stored as sRGB (for the composite to
/// read as the overlay's picture), drawn through a plain view so that its
/// blends work on the display's encoding, as the game's did.
fn hud_image(width: u32, height: u32) -> Image {
    let mut image = Image::new_fill(
        Extent3d { width, height, depth_or_array_layers: 1 },
        TextureDimension::D2,
        &[0, 0, 0, 0],
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.texture_descriptor.usage |= TextureUsages::RENDER_ATTACHMENT;
    image.texture_descriptor.view_formats = &[TextureFormat::Rgba8Unorm];
    image
}

fn spawn_hud(mut commands: Commands, mut images: ResMut<Assets<Image>>, mut materials: ResMut<Assets<OverlayMaterial>>) {
    let image = images.add(hud_image(1, 1));
    let material = materials.add(OverlayMaterial::new(image.clone()));
    commands.spawn((
        Name::new("Game HUD"),
        MaterialNode(material.clone()),
        Node { position_type: PositionType::Absolute, width: percent(100), height: percent(100), ..default() },
        // over the game's overlay, under the viewer's own text
        GlobalZIndex(-1),
    ));
    commands.insert_resource(HudList { image, material, size: (1, 1), vertices: default(), draws: default(), items: default() });
}

/// The texture at the window's size (its physical pixels).
fn size_hud(
    window: Single<&Window, With<PrimaryWindow>>,
    mut list: ResMut<HudList>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<OverlayMaterial>>,
) {
    let size = (window.physical_width().max(1), window.physical_height().max(1));
    if list.size == size {
        return;
    }
    images.insert(list.image.id(), hud_image(size.0, size.1)).ok();
    // (the material's bind group made again with the new texture)
    let _ = materials.get_mut(list.material.id());
    list.size = size;
}

fn unpack_color(c: u32) -> [f32; 4] {
    [((c >> 16) & 255) as f32 / 255.0, ((c >> 8) & 255) as f32 / 255.0, (c & 255) as f32 / 255.0, (c >> 24) as f32 / 255.0]
}

fn follow_hud(
    link: Res<BridgeLink>,
    mut list: ResMut<HudList>,
    mut stats: ResMut<HudStats>,
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
    if list.items.is_empty() && snapshot.screen_draws.is_empty() {
        return;
    }
    let list = list.as_mut();
    list.vertices.clear();
    list.draws.clear();
    list.items.clear();
    stats.missing = 0;
    for draw in &snapshot.screen_draws {
        let first = draw.first_vertex as usize;
        let Some(corners) = snapshot.screen_vertices.get(first..first + draw.vertex_count as usize) else { continue };
        if corners.len() != 4 || multiplies(draw) {
            continue;
        }
        let mut maps: [Option<Handle<Image>>; 3] = default();
        let mut samplers = [0u8; 3];
        let mut present = 0u32;
        let mut missing = false;
        for (index, map) in draw.maps.iter().enumerate() {
            if map.bitmap == u32::MAX {
                continue;
            }
            let image = bitmaps
                .entry((map.bitmap, map.bitmap_index))
                .or_insert_with(|| {
                    level.as_deref_mut().and_then(|level| {
                        let builder = &mut level.builder;
                        // (the stored values: the combiners work on them)
                        builder.textures.get(&builder.cache, map.bitmap, map.bitmap_index as usize, false, &mut images)
                    })
                })
                .clone();
            if image.is_none() {
                missing = true;
            }
            maps[index] = image;
            present |= 1 << index;
            samplers[index] = u8::from(map.flags & SCREEN_MAP_WRAPPED != 0) | (u8::from(map.flags & SCREEN_MAP_POINT_SAMPLED != 0) << 1);
        }
        if missing {
            stats.missing += 1;
            continue;
        }
        for index in [0, 1, 2, 0, 2, 3] {
            let v = &corners[index];
            list.vertices.push(HudVertex { position: v.position, uv: v.texcoords, color: unpack_color(v.color) });
        }
        list.draws.push(HudDrawGpu {
            viewport: [draw.viewport[0].max(1.0), draw.viewport[1].max(1.0), 0.0, 0.0],
            program: [draw.combiner_count & 0xff, draw.combiner_count & !0xff, draw.final_combiner_inputs_abcd, draw.final_combiner_inputs_efg],
            rgb_inputs: draw.rgb_inputs,
            rgb_outputs: draw.rgb_outputs,
            alpha_inputs: draw.alpha_inputs,
            alpha_outputs: draw.alpha_outputs,
            constant0: draw.constant_0.map(unpack_color),
            constant1: draw.constant_1.map(unpack_color),
            modes: [present, u32::from(draw.flags & SCREEN_DRAW_ALPHA_KILL != 0), 0, 0],
        });
        let blend_color = unpack_color(draw.blend_color).map(f64::from);
        list.items.push(HudItem {
            maps,
            samplers,
            blend: BlendKey {
                source: draw.blend_source,
                destination: draw.blend_destination,
                operation: draw.blend_operation,
                blended: draw.flags & SCREEN_DRAW_BLENDED != 0,
            },
            blend_color,
        });
    }
    stats.drawn = list.items.len();
}

// GL's blend enums, as the Xbox's D3D uses them
const GL_ZERO: u32 = 0;
const GL_ONE: u32 = 1;
const GL_SRC_COLOR: u32 = 0x300;
const GL_ONE_MINUS_SRC_COLOR: u32 = 0x301;
const GL_SRC_ALPHA: u32 = 0x302;
const GL_ONE_MINUS_SRC_ALPHA: u32 = 0x303;
const GL_DST_ALPHA: u32 = 0x304;
const GL_ONE_MINUS_DST_ALPHA: u32 = 0x305;
const GL_DST_COLOR: u32 = 0x306;
const GL_ONE_MINUS_DST_COLOR: u32 = 0x307;
const GL_SRC_ALPHA_SATURATE: u32 = 0x308;
const GL_CONSTANT_COLOR: u32 = 0x8001;
const GL_ONE_MINUS_CONSTANT_COLOR: u32 = 0x8002;
const GL_FUNC_ADD: u32 = 0x8006;
const GL_MIN: u32 = 0x8007;
const GL_MAX: u32 = 0x8008;
const GL_FUNC_SUBTRACT: u32 = 0x800a;
const GL_FUNC_REVERSE_SUBTRACT: u32 = 0x800b;

/// Whether a quad's blend multiplies what is under it, which over the
/// overlay's clear picture has nothing to multiply (the port leaves those
/// out of its overlay too).
fn multiplies(draw: &ScreenDraw) -> bool {
    if draw.flags & SCREEN_DRAW_BLENDED == 0 || draw.blend_operation != GL_FUNC_ADD {
        return false;
    }
    let (source, destination) = (draw.blend_source, draw.blend_destination);
    (source == GL_DST_COLOR && destination == GL_ZERO)
        || (source == GL_ZERO && destination == GL_SRC_COLOR)
        || (source == GL_DST_COLOR && destination == GL_SRC_COLOR)
}

fn blend_factor(factor: u32) -> BlendFactor {
    match factor {
        GL_ZERO => BlendFactor::Zero,
        GL_ONE => BlendFactor::One,
        GL_SRC_COLOR => BlendFactor::Src,
        GL_ONE_MINUS_SRC_COLOR => BlendFactor::OneMinusSrc,
        GL_SRC_ALPHA => BlendFactor::SrcAlpha,
        GL_ONE_MINUS_SRC_ALPHA => BlendFactor::OneMinusSrcAlpha,
        GL_DST_ALPHA => BlendFactor::DstAlpha,
        GL_ONE_MINUS_DST_ALPHA => BlendFactor::OneMinusDstAlpha,
        GL_DST_COLOR => BlendFactor::Dst,
        GL_ONE_MINUS_DST_COLOR => BlendFactor::OneMinusDst,
        GL_SRC_ALPHA_SATURATE => BlendFactor::SrcAlphaSaturated,
        GL_CONSTANT_COLOR => BlendFactor::Constant,
        GL_ONE_MINUS_CONSTANT_COLOR => BlendFactor::OneMinusConstant,
        _ => BlendFactor::One,
    }
}

fn blend_operation(operation: u32) -> BlendOperation {
    match operation {
        GL_FUNC_SUBTRACT => BlendOperation::Subtract,
        GL_FUNC_REVERSE_SUBTRACT => BlendOperation::ReverseSubtract,
        GL_MIN => BlendOperation::Min,
        GL_MAX => BlendOperation::Max,
        _ => BlendOperation::Add,
    }
}

/// The game's blend, and the coverage the port's overlay keeps in alpha:
/// blends that replace what is under them add theirs, others leave it.
fn blend_state(key: BlendKey) -> Option<BlendState> {
    if !key.blended {
        return None;
    }
    let (source, destination) = (key.source, key.destination);
    let replaces = key.operation == GL_FUNC_ADD
        && (destination == GL_ONE_MINUS_SRC_ALPHA || (destination == GL_ZERO && source != GL_ZERO && source != GL_DST_COLOR));
    let alpha = if replaces {
        BlendComponent { src_factor: BlendFactor::One, dst_factor: blend_factor(destination), operation: BlendOperation::Add }
    } else {
        BlendComponent { src_factor: BlendFactor::Zero, dst_factor: BlendFactor::One, operation: BlendOperation::Add }
    };
    let color = BlendComponent {
        src_factor: blend_factor(source),
        dst_factor: blend_factor(destination),
        operation: blend_operation(key.operation),
    };
    // (min and max take no factors)
    let color = match color.operation {
        BlendOperation::Min | BlendOperation::Max => BlendComponent { src_factor: BlendFactor::One, dst_factor: BlendFactor::One, ..color },
        _ => color,
    };
    Some(BlendState { color, alpha })
}

#[derive(Resource)]
pub(crate) struct HudPipeline {
    draws_layout: BindGroupLayoutDescriptor,
    maps_layout: BindGroupLayoutDescriptor,
    shader: Handle<Shader>,
    /// by sampler bits: bit 0 wrapped, bit 1 point sampled
    samplers: [Sampler; 4],
}

/// What the HUD is drawn as: the display's encoding, stored as is.
const HUD_DRAW_FORMAT: TextureFormat = TextureFormat::Rgba8Unorm;

fn init_pipeline(mut commands: Commands, device: Res<RenderDevice>, assets: Res<AssetServer>) {
    let draws_layout = BindGroupLayoutDescriptor::new(
        "halo_hud_draws",
        &BindGroupLayoutEntries::single(ShaderStages::VERTEX_FRAGMENT, storage_buffer_read_only_sized(false, None)),
    );
    let map = || (texture_2d(TextureSampleType::Float { filterable: true }), sampler(SamplerBindingType::Filtering));
    let maps_layout = BindGroupLayoutDescriptor::new(
        "halo_hud_maps",
        &BindGroupLayoutEntries::sequential(ShaderStages::FRAGMENT, (map().0, map().1, map().0, map().1, map().0, map().1)),
    );
    let shader: Handle<Shader> = load_embedded_asset!(assets.as_ref(), "shaders/halo_hud.wesl");
    let samplers = std::array::from_fn(|bits: usize| {
        let address = if bits & 1 != 0 { AddressMode::Repeat } else { AddressMode::ClampToEdge };
        let (filter, mipmap) = if bits & 2 != 0 { (FilterMode::Nearest, MipmapFilterMode::Nearest) } else { (FilterMode::Linear, MipmapFilterMode::Linear) };
        device.create_sampler(&SamplerDescriptor {
            label: Some("halo_hud"),
            address_mode_u: address,
            address_mode_v: address,
            mag_filter: filter,
            min_filter: filter,
            mipmap_filter: mipmap,
            ..default()
        })
    });
    commands.insert_resource(HudPipeline { draws_layout, maps_layout, shader, samplers });
}

fn queue_pipeline(cache: &PipelineCache, hud: &HudPipeline, key: BlendKey) -> CachedRenderPipelineId {
    cache.queue_render_pipeline(RenderPipelineDescriptor {
        label: Some("halo_hud".into()),
        layout: vec![hud.draws_layout.clone(), hud.maps_layout.clone()],
        vertex: VertexState {
            shader: hud.shader.clone(),
            entry_point: Some("vertex".into()),
            buffers: vec![VertexBufferLayout::from_vertex_formats(
                VertexStepMode::Vertex,
                [VertexFormat::Float32x2, VertexFormat::Float32x2, VertexFormat::Float32x2, VertexFormat::Float32x2, VertexFormat::Float32x4],
            )],
            ..default()
        },
        fragment: Some(FragmentState {
            shader: hud.shader.clone(),
            entry_point: Some("fragment".into()),
            targets: vec![Some(ColorTargetState { format: HUD_DRAW_FORMAT, blend: blend_state(key), write_mask: ColorWrites::ALL })],
            ..default()
        }),
        ..default()
    })
}

/// The frame's quads, drawn in order into the HUD's texture (cleared first,
/// so a frame without any leaves none).
#[allow(clippy::too_many_arguments)]
fn draw_hud(
    _view: bevy::render::renderer::ViewQuery<&ExtractedView>,
    list: Option<Res<HudList>>,
    hud: Option<Res<HudPipeline>>,
    gpu_images: Res<RenderAssets<GpuImage>>,
    fallback: Res<FallbackImage>,
    cache: Res<PipelineCache>,
    device: Res<RenderDevice>,
    mut pipelines: Local<HashMap<BlendKey, CachedRenderPipelineId>>,
    frames: Option<Res<bevy::diagnostic::FrameCount>>,
    mut drawn: Local<Option<u32>>,
    mut ctx: RenderContext,
) {
    let (Some(list), Some(hud)) = (list, hud) else { return };
    let Some(target) = gpu_images.get(&list.image) else { return };
    // (once a frame, whatever the views)
    let frame = frames.map_or(0, |f| f.0);
    if *drawn == Some(frame) {
        return;
    }
    *drawn = Some(frame);
    let view = target.texture.create_view(&TextureViewDescriptor { format: Some(HUD_DRAW_FORMAT), ..default() });
    let mut runs = Vec::with_capacity(list.items.len());
    if !list.items.is_empty() {
        let vertices = device.create_buffer_with_data(&BufferInitDescriptor {
            label: Some("halo_hud_vertices"),
            contents: bytemuck::cast_slice(&list.vertices),
            usage: BufferUsages::VERTEX,
        });
        let draws = device.create_buffer_with_data(&BufferInitDescriptor {
            label: Some("halo_hud_draws"),
            contents: bytemuck::cast_slice(&list.draws),
            usage: BufferUsages::STORAGE,
        });
        let draws_group = device.create_bind_group(
            "halo_hud_draws",
            &cache.get_bind_group_layout(&hud.draws_layout),
            &BindGroupEntries::single(draws.as_entire_binding()),
        );
        let maps_layout = cache.get_bind_group_layout(&hud.maps_layout);
        for (index, item) in list.items.iter().enumerate() {
            let pipeline_id = *pipelines.entry(item.blend).or_insert_with(|| queue_pipeline(&cache, &hud, item.blend));
            let Some(pipeline) = cache.get_render_pipeline(pipeline_id) else { continue };
            let texture = |map: usize| {
                item.maps[map].as_ref().and_then(|image| gpu_images.get(image)).map_or(&fallback.d2.texture_view, |image| &image.texture_view)
            };
            let sampler = |map: usize| &hud.samplers[item.samplers[map] as usize];
            let maps = device.create_bind_group(
                "halo_hud_maps",
                &maps_layout,
                &BindGroupEntries::sequential((texture(0), sampler(0), texture(1), sampler(1), texture(2), sampler(2))),
            );
            runs.push((pipeline, maps, index as u32, item.blend_color));
        }
        let mut pass = ctx.begin_tracked_render_pass(RenderPassDescriptor {
            label: Some("halo_hud"),
            color_attachments: &[Some(RenderPassColorAttachment {
                view: &view,
                depth_slice: None,
                resolve_target: None,
                ops: Operations { load: LoadOp::Clear(default()), store: StoreOp::Store },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_vertex_buffer(0, vertices.slice(..));
        pass.set_bind_group(0, &draws_group, &[]);
        for (pipeline, maps, index, color) in &runs {
            pass.set_render_pipeline(pipeline);
            pass.set_bind_group(1, maps, &[]);
            pass.set_blend_constant(LinearRgba::new(color[0] as f32, color[1] as f32, color[2] as f32, color[3] as f32));
            pass.draw(index * 6..index * 6 + 6, *index..*index + 1);
        }
    } else {
        // nothing this frame: the texture cleared
        let _pass = ctx.begin_tracked_render_pass(RenderPassDescriptor {
            label: Some("halo_hud"),
            color_attachments: &[Some(RenderPassColorAttachment {
                view: &view,
                depth_slice: None,
                resolve_target: None,
                ops: Operations { load: LoadOp::Clear(default()), store: StoreOp::Store },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
    }
}

