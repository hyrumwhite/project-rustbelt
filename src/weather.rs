//! The game's weather (snow, rain), drawn after the fog. The game draws its
//! effects after its fog (render.c: `structure_render_fog`, then
//! `weather_particle_systems_render`, then the particles), each faded by the
//! fog at its own depth. The mesh path (`src/effects.rs`) draws sprites in
//! the transparent pass, before the fog pass, which then fogs each pixel by
//! the opaque depth behind it, flakes included: in c40's thick blue fog the
//! snow showed in full against the sky's opening and hardly at all against
//! the rock, a mask in the sky's shape, which the sprite shader's raise for
//! what lies behind (up to 4 times) cannot undo.
//!
//! So the weather's batches ([`halo_bridge::SPRITE_WEATHER`], which the game
//! marks) are drawn here as the lens flares are (`src/lens_flares.rs`): into
//! a texture of their own the picture's size, in the display's encoding, a
//! premultiplied layer over (`shaders/halo_weather.wesl`), each flake faded
//! by the fog at its own place and hidden where the scene's depth is nearer
//! (the first-person weapon's too); the fog pass lays that layer over the
//! picture after the fog and the fog screen, before the flares. Only the
//! blends a layer over can hold: alpha blend, alpha multiply add, add (and
//! max, as add, as the sprite shader takes it); a weather batch with any
//! other blend (or a zsprite's, or the first-person weapon's) stays on the
//! mesh path, as before.
//!
//! `HALO_RT_WEATHER_BEFORE_FOG=1`: every weather batch on the mesh path, the
//! old, and this pass draws nothing. Without the viewer's fog (no fog pass)
//! the weather stays on the mesh path too.

use crate::bridge::{BridgeLevel, BridgeLink};
use bevy::render::diagnostic::RecordDiagnostics;
use bevy::{
    asset::{embedded_asset, load_embedded_asset},
    mesh::VertexBufferLayout,
    platform::collections::HashMap,
    prelude::*,
    render::{
        extract_resource::{ExtractResource, ExtractResourcePlugin},
        render_asset::RenderAssets,
        render_resource::{
            binding_types::{sampler, texture_2d, texture_depth_2d, uniform_buffer},
            BindGroupEntries, BindGroupLayoutDescriptor, BindGroupLayoutEntries, BlendComponent, BlendFactor,
            BlendOperation, BlendState, Buffer, BufferInitDescriptor, BufferUsages, CachedRenderPipelineId,
            ColorTargetState, ColorWrites, Extent3d, FragmentState, LoadOp, Operations, PipelineCache,
            RenderPassColorAttachment, RenderPassDescriptor, RenderPipelineDescriptor, SamplerBindingType, ShaderStages,
            StoreOp, TextureDescriptor, TextureDimension, TextureFormat, TextureSampleType, TextureUsages, TextureView,
            VertexFormat, VertexState, VertexStepMode,
        },
        renderer::{RenderContext, RenderDevice},
        texture::{GpuImage, TextureCache},
        view::{ViewUniform, ViewUniforms},
        RenderApp, RenderStartup,
    },
};
use halo_bridge::{
    SpriteBatch, DECAL_BLEND_ADD, DECAL_BLEND_ALPHA, DECAL_BLEND_ALPHA_MULTIPLY_ADD, DECAL_BLEND_MAX,
    SPRITE_NONLINEAR_TINT, SPRITE_WEATHER,
};
use halo_map::math::{halo_to_bevy, METERS_PER_WORLD_UNIT};
use std::sync::atomic::{AtomicBool, Ordering};

pub struct WeatherPlugin;

/// Whether this pass is there to draw the weather (added with the fog pass):
/// without it the weather stays on the mesh path.
static ACTIVE: AtomicBool = AtomicBool::new(false);

impl Plugin for WeatherPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "shaders/halo_weather.wesl");
        ACTIVE.store(true, Ordering::Relaxed);
        app.init_resource::<Weather>()
            .add_plugins(ExtractResourcePlugin::<Weather>::default())
            .add_systems(Update, follow_weather);
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else { return };
        render_app.add_systems(RenderStartup, init_pipeline);
    }
}

/// `HALO_RT_WEATHER_BEFORE_FOG=1`: the weather on the mesh path, the old.
fn before_fog() -> bool {
    static BEFORE: std::sync::LazyLock<bool> =
        std::sync::LazyLock::new(|| std::env::var("HALO_RT_WEATHER_BEFORE_FOG").is_ok_and(|v| v == "1"));
    *BEFORE
}

/// Whether this pass draws a batch (and the mesh path leaves it out): the
/// weather's, in the world, not a zsprite, with a blend a layer over can
/// hold.
pub(crate) fn drawn_after_fog(batch: &SpriteBatch) -> bool {
    batch.kind == SPRITE_WEATHER
        && ACTIVE.load(Ordering::Relaxed)
        && !before_fog()
        && batch.first_person == 0
        && !(batch.secondary_anchor == 2 && batch.secondary_map != u32::MAX)
        && pass_mode(batch.blend_function).is_some()
}

/// How the pass draws a blend: 0 alpha blend, 1 alpha multiply add
/// (premultiplied), 2 added (add, and max as add); none for the blends it
/// can't (multiply, double multiply, min, subtract).
fn pass_mode(blend_function: u16) -> Option<f32> {
    match u8::try_from(blend_function).ok()? {
        DECAL_BLEND_ALPHA => Some(0.0),
        DECAL_BLEND_ALPHA_MULTIPLY_ADD => Some(1.0),
        DECAL_BLEND_ADD | DECAL_BLEND_MAX => Some(2.0),
        _ => None,
    }
}

/// A flake's corner: where it is (this world's terms), the bitmap's texture
/// coordinates, the colour (display-encoded; alpha its fade), how it's drawn
/// ([`pass_mode`]) and 1 for nonlinear tint.
#[repr(C)]
#[derive(Clone, Copy, Default, bytemuck::Pod, bytemuck::Zeroable)]
struct WeatherVertex {
    position: [f32; 3],
    uv: [f32; 2],
    color: [f32; 4],
    mode: f32,
    nonlinear: f32,
}

/// The latest frame's weather: two triangles a sprite, in the game's order,
/// and the bitmap each run of them is drawn with.
#[derive(Resource, Clone, Default, ExtractResource)]
#[extract_app(RenderApp)]
pub(crate) struct Weather {
    vertices: Vec<WeatherVertex>,
    /// (bitmap, first vertex, vertex count)
    runs: Vec<(Handle<Image>, u32, u32)>,
}

fn follow_weather(
    link: Res<BridgeLink>,
    mut weather: ResMut<Weather>,
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
    let weather = weather.as_mut();
    if weather.runs.is_empty() && !snapshot.sprite_batches.iter().any(drawn_after_fog) {
        return;
    }
    weather.vertices.clear();
    weather.runs.clear();
    for batch in snapshot.sprite_batches.iter().filter(|b| drawn_after_fog(b)) {
        let Some(mode) = pass_mode(batch.blend_function) else { continue };
        let first = batch.first_vertex as usize;
        let Some(corners) = snapshot.sprite_vertices.get(first..first + batch.quad_count as usize * 4) else { continue };
        // (its bitmap as the mesh path's material takes it: display-encoded,
        // with the image's own sampler)
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
        let nonlinear = if batch.shader_flags & SPRITE_NONLINEAR_TINT != 0 { 1.0 } else { 0.0 };
        let start = weather.vertices.len() as u32;
        // (each quad's corners as two triangles, as the mesh path's indices)
        for quad in corners.chunks_exact(4) {
            for index in [0, 1, 2, 0, 2, 3] {
                let v = &quad[index];
                let c = v.color;
                weather.vertices.push(WeatherVertex {
                    position: halo_to_bevy(v.position, METERS_PER_WORLD_UNIT),
                    uv: v.texcoord,
                    color: [
                        ((c >> 16) & 255) as f32 / 255.0,
                        ((c >> 8) & 255) as f32 / 255.0,
                        (c & 255) as f32 / 255.0,
                        (c >> 24) as f32 / 255.0,
                    ],
                    mode,
                    nonlinear,
                });
            }
        }
        let count = weather.vertices.len() as u32 - start;
        if count == 0 {
            continue;
        }
        match weather.runs.last_mut() {
            Some((last, _, last_count)) if *last == image => *last_count += count,
            _ => weather.runs.push((image, start, count)),
        }
    }
}

#[derive(Resource)]
pub(crate) struct WeatherPipeline {
    view_layout: BindGroupLayoutDescriptor,
    map_layout: BindGroupLayoutDescriptor,
    pipeline: CachedRenderPipelineId,
}

/// What the weather is drawn into: a premultiplied layer, display-encoded.
const WEATHER_FORMAT: TextureFormat = TextureFormat::Rgba16Float;

fn init_pipeline(mut commands: Commands, assets: Res<AssetServer>, cache: Res<PipelineCache>) {
    // the view, the fog pass's fog, and the scene's depths (the prepass's,
    // and the main pass's for the first-person weapon's panes), as the fog
    // pass reads them
    let view_layout = BindGroupLayoutDescriptor::new(
        "halo_weather_view",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::VERTEX_FRAGMENT,
            (
                uniform_buffer::<ViewUniform>(true),
                uniform_buffer::<crate::fog::HaloFog>(false),
                texture_depth_2d(),
                texture_depth_2d(),
            ),
        ),
    );
    let map_layout = BindGroupLayoutDescriptor::new(
        "halo_weather_map",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::FRAGMENT,
            (texture_2d(TextureSampleType::Float { filterable: true }), sampler(SamplerBindingType::Filtering)),
        ),
    );
    let shader: Handle<Shader> = load_embedded_asset!(assets.as_ref(), "shaders/halo_weather.wesl");
    // (a premultiplied layer over: an added sprite writes no alpha)
    let over =
        BlendComponent { src_factor: BlendFactor::One, dst_factor: BlendFactor::OneMinusSrcAlpha, operation: BlendOperation::Add };
    let pipeline = cache.queue_render_pipeline(RenderPipelineDescriptor {
        label: Some("halo_weather".into()),
        layout: vec![view_layout.clone(), map_layout.clone()],
        vertex: VertexState {
            shader: shader.clone(),
            entry_point: Some("vertex".into()),
            buffers: vec![VertexBufferLayout::from_vertex_formats(
                VertexStepMode::Vertex,
                [
                    VertexFormat::Float32x3,
                    VertexFormat::Float32x2,
                    VertexFormat::Float32x4,
                    VertexFormat::Float32,
                    VertexFormat::Float32,
                ],
            )],
            ..default()
        },
        fragment: Some(FragmentState {
            shader,
            entry_point: Some("fragment".into()),
            targets: vec![Some(ColorTargetState {
                format: WEATHER_FORMAT,
                blend: Some(BlendState { color: over, alpha: over }),
                write_mask: ColorWrites::ALL,
            })],
            ..default()
        }),
        ..default()
    });
    commands.insert_resource(WeatherPipeline { view_layout, map_layout, pipeline });
}

/// The frame's weather in a texture the picture's size, a premultiplied
/// layer for the fog pass to lay over the fogged picture; none when there is
/// none (or it isn't ready).
#[allow(clippy::too_many_arguments)]
pub(crate) fn draw_weather(
    ctx: &mut RenderContext,
    weather: &Weather,
    weather_pipeline: &WeatherPipeline,
    cache: &PipelineCache,
    device: &RenderDevice,
    gpu_images: &RenderAssets<GpuImage>,
    texture_cache: &mut TextureCache,
    view_uniforms: &ViewUniforms,
    view_offset: u32,
    fog: &Buffer,
    depth: &TextureView,
    main_depth: &TextureView,
    size: Extent3d,
) -> Option<TextureView> {
    if weather.runs.is_empty() {
        return None;
    }
    let pipeline = cache.get_render_pipeline(weather_pipeline.pipeline)?;
    let view_binding = view_uniforms.uniforms.binding()?;
    let map_layout = cache.get_bind_group_layout(&weather_pipeline.map_layout);
    let map_groups: Vec<_> = weather
        .runs
        .iter()
        .filter_map(|(image, first, count)| {
            let image = gpu_images.get(image)?;
            let group = device.create_bind_group(
                "halo_weather_map",
                &map_layout,
                &BindGroupEntries::sequential((&image.texture_view, &image.sampler)),
            );
            Some((group, *first, *count))
        })
        .collect();
    if map_groups.is_empty() {
        return None;
    }

    let texture = texture_cache.get(
        device,
        TextureDescriptor {
            label: Some("halo_weather"),
            size: Extent3d { depth_or_array_layers: 1, ..size },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: WEATHER_FORMAT,
            usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        },
    );
    let vertices = device.create_buffer_with_data(&BufferInitDescriptor {
        label: Some("halo_weather"),
        contents: bytemuck::cast_slice(&weather.vertices),
        usage: BufferUsages::VERTEX,
    });
    let view_group = device.create_bind_group(
        "halo_weather_view",
        &cache.get_bind_group_layout(&weather_pipeline.view_layout),
        &BindGroupEntries::sequential((view_binding, fog.as_entire_binding(), depth, main_depth)),
    );

    let diagnostics = ctx.diagnostic_recorder();
    let diagnostics = diagnostics.as_deref();
    let mut pass = ctx.begin_tracked_render_pass(RenderPassDescriptor {
        label: Some("halo_weather"),
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
    let pass_span = diagnostics.pass_span(&mut pass, "halo_weather");
    pass.set_render_pipeline(pipeline);
    pass.set_bind_group(0, &view_group, &[view_offset]);
    pass.set_vertex_buffer(0, vertices.slice(..));
    for (group, first, count) in &map_groups {
        pass.set_bind_group(1, group, &[]);
        pass.draw(*first..*first + *count, 0..1);
    }
    pass_span.end(&mut pass);
    drop(pass);
    Some(texture.default_view)
}
