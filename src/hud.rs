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
//!
//! Where the port draws its own art in a bitmap's place (its high-res HUD
//! and menu art), the game shares that art's PNG once
//! ([`halo_bridge::Bridge::read_screen_textures`]), and the high-res text's
//! characters come as quads of its atlas
//! ([`halo_bridge::Bridge::read_text_atlas`]), in the same order.

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
use bevy::{render::render_resource::AsBindGroup, shader::ShaderRef, ui_render::prelude::{UiMaterial, UiMaterialPlugin}};
use halo_bridge::{
    ScreenDraw, SCREEN_DRAW_ALPHA_KILL, SCREEN_DRAW_BLENDED, SCREEN_DRAW_COLOR_ONLY, SCREEN_MAP_BORDER, SCREEN_MAP_POINT_SAMPLED,
    SCREEN_MAP_WRAPPED, SCREEN_TARGET_MOTION_SENSOR, SCREEN_TEXTURE_COVERAGE, SCREEN_TEXTURE_MOTION_SENSOR,
    SCREEN_TEXTURE_POINT_THRESHOLD, SCREEN_TEXTURE_TEXT_ATLAS, TEXT_ATLAS_SIZE,
};

pub struct HudPlugin;

impl Plugin for HudPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "shaders/halo_hud.wesl");
        embedded_asset!(app, "shaders/halo_flash.wesl");
        app.init_resource::<HudStats>()
            .add_plugins(ExtractResourcePlugin::<HudList>::default())
            .add_plugins(UiMaterialPlugin::<FlashMaterial>::default())
            .add_systems(Startup, (spawn_hud, spawn_flash))
            .add_systems(Update, ((size_hud, follow_hud).chain(), follow_flash));
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
    /// the bordered maps' border colour
    border: [f32; 4],
}

/// How a quad is blended: the game's factors and operation (GL's enums),
/// and whether it blends at all.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct BlendKey {
    source: u32,
    destination: u32,
    operation: u32,
    blended: bool,
    /// alpha not written (the motion sensor's target, as the game draws it)
    color_only: bool,
}

#[derive(Clone)]
struct HudItem {
    /// whether it is drawn into the motion sensor's target (first), not the
    /// screen
    motion_sensor: bool,
    /// whether map 0 is the motion sensor's target
    samples_motion_sensor: bool,
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

/// One of the screen flash's layers: a colour its blend multiplies the
/// picture by, adds to it, takes from it, or takes the greater or lesser of.
#[derive(AsBindGroup, Asset, TypePath, Debug, Clone)]
#[bind_group_data(FlashBlend)]
pub(crate) struct FlashMaterial {
    #[uniform(0)]
    colour: Vec4,
    blend: FlashBlend,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum FlashBlend {
    Multiply,
    Add,
    Subtract,
    Max,
    Min,
}

impl From<&FlashMaterial> for FlashBlend {
    fn from(material: &FlashMaterial) -> Self {
        material.blend
    }
}

impl UiMaterial for FlashMaterial {
    fn fragment_shader() -> ShaderRef {
        "embedded://halo_rt/shaders/halo_flash.wesl".into()
    }

    fn specialize(descriptor: &mut RenderPipelineDescriptor, key: bevy::ui_render::ui_material::UiMaterialKey<Self>) {
        let (src_factor, dst_factor, operation) = match key.bind_group_data {
            FlashBlend::Multiply => (BlendFactor::Zero, BlendFactor::Src, BlendOperation::Add),
            FlashBlend::Add => (BlendFactor::One, BlendFactor::One, BlendOperation::Add),
            FlashBlend::Subtract => (BlendFactor::One, BlendFactor::One, BlendOperation::ReverseSubtract),
            FlashBlend::Max => (BlendFactor::One, BlendFactor::One, BlendOperation::Max),
            FlashBlend::Min => (BlendFactor::One, BlendFactor::One, BlendOperation::Min),
        };
        let keep = BlendComponent { src_factor: BlendFactor::Zero, dst_factor: BlendFactor::One, operation: BlendOperation::Add };
        if let Some(fragment) = &mut descriptor.fragment {
            for target in fragment.targets.iter_mut().flatten() {
                target.blend = Some(BlendState { color: BlendComponent { src_factor, dst_factor, operation }, alpha: keep });
            }
        }
    }
}

/// The screen flash's layers: a multiply, then an add or a subtract, then a
/// max and a min.
#[derive(Resource)]
struct FlashLayers([Handle<FlashMaterial>; 5]);

fn spawn_flash(mut commands: Commands, mut materials: ResMut<Assets<FlashMaterial>>) {
    let layers = [
        (FlashBlend::Multiply, Vec4::ONE),
        (FlashBlend::Add, Vec4::ZERO),
        (FlashBlend::Subtract, Vec4::ZERO),
        (FlashBlend::Max, Vec4::ZERO),
        (FlashBlend::Min, Vec4::ONE),
    ]
    .map(|(blend, colour)| materials.add(FlashMaterial { colour, blend }));
    for layer in &layers {
        commands.spawn((
            Name::new("Game screen flash"),
            MaterialNode(layer.clone()),
            Node { position_type: PositionType::Absolute, width: percent(100), height: percent(100), ..default() },
            // over the HUD, under the viewer's own text
            GlobalZIndex(-1),
        ));
    }
    commands.insert_resource(FlashLayers(layers));
}

/// The game's flash as the layers' colours. Its blends work on the
/// display's encoding and some take the picture or a constant colour as a
/// factor; each is a multiply by one colour and an add (or subtract) of
/// another: lighten f + d(1 - a), darken d - f, tint a + d(1 - inverse),
/// invert s + d(1 - f - s). Max and min are the blend's max and min, which
/// leave its factors out: max(f - a, d) and min(f + a, d) (the combiners'
/// c0 - c0.a and c0 + c0.a), so a flash as opaque as it is bright (the
/// needler's) leaves the picture as it is. A multiply in the display's
/// encoding is the same multiply in light by its colour to the 2.2; the
/// added colours are converted alike, and a max or min of them is the same
/// in light.
fn follow_flash(link: Res<BridgeLink>, layers: Res<FlashLayers>, mut materials: ResMut<Assets<FlashMaterial>>, mut last: Local<Option<[Vec4; 5]>>) {
    let Some(snapshot) = link.latest.as_ref() else { return };
    let (kind, intensity, [a, r, g, b]) = snapshot.screen_flash;
    let colour = Vec3::new(r, g, b);
    let flash = colour * intensity;
    let alpha = a * intensity;
    let (mut maximum, mut minimum) = (Vec3::ZERO, Vec3::ONE);
    let (multiply, add, subtract) = match kind {
        1 => (Vec3::splat(1.0 - alpha), flash, Vec3::ZERO),
        2 => (Vec3::ONE, Vec3::ZERO, flash),
        3 => {
            maximum = flash - Vec3::splat(alpha);
            (Vec3::ONE, Vec3::ZERO, Vec3::ZERO)
        }
        4 => {
            minimum = flash + Vec3::splat(alpha);
            (Vec3::ONE, Vec3::ZERO, Vec3::ZERO)
        }
        5 => {
            // the combiners' c0 * (2 c0 - 1) + c0.a
            let s = (flash * (2.0 * flash - 1.0) + alpha).clamp(Vec3::ZERO, Vec3::ONE);
            ((Vec3::ONE - flash - s).clamp(Vec3::ZERO, Vec3::ONE), s, Vec3::ZERO)
        }
        6 => (Vec3::ONE - (Vec3::ONE - colour) * intensity, Vec3::splat(alpha), Vec3::ZERO),
        _ => (Vec3::ONE, Vec3::ZERO, Vec3::ZERO),
    };
    let to_light = |c: Vec3| c.clamp(Vec3::ZERO, Vec3::ONE).powf(2.2);
    let colours = [multiply, add, subtract, maximum, minimum].map(|c| to_light(c).extend(1.0));
    if *last == Some(colours) {
        return;
    }
    *last = Some(colours);
    for (layer, colour) in layers.0.iter().zip(colours) {
        if let Some(mut material) = materials.get_mut(layer) {
            material.colour = colour;
        }
    }
}

fn spawn_hud(mut commands: Commands, mut images: ResMut<Assets<Image>>, mut materials: ResMut<Assets<OverlayMaterial>>) {
    let image = images.add(hud_image(1, 1));
    let material = materials.add(OverlayMaterial::new(image.clone()));
    commands.spawn((
        Name::new("Game HUD"),
        MaterialNode(material.clone()),
        Node { position_type: PositionType::Absolute, width: percent(100), height: percent(100), ..default() },
        // over the game's overlay, under its screen flash and the viewer's
        // own text
        GlobalZIndex(-2),
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

/// The port's art and the text's atlas, as the game has shared them.
#[derive(Default)]
struct ScreenTextures {
    /// by index: the image and its flags
    shared: Vec<Option<(Handle<Image>, u32)>>,
    atlas: Option<Handle<Image>>,
    atlas_generation: u32,
    coverage: Vec<u8>,
}

/// An RGBA8 picture with its mip levels (each a box filter of the last), as
/// the port draws its art: it is larger than it appears.
fn with_mips(width: u32, height: u32, mut pixels: Vec<u8>) -> Image {
    let (mut w, mut h) = (width as usize, height as usize);
    let mut level = pixels.clone();
    let mut levels = 1;
    while w > 1 || h > 1 {
        let (nw, nh) = ((w / 2).max(1), (h / 2).max(1));
        let mut next = vec![0u8; nw * nh * 4];
        for y in 0..nh {
            for x in 0..nw {
                for c in 0..4 {
                    let mut sum = 0u32;
                    for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                        let (sx, sy) = ((x * 2 + dx).min(w - 1), (y * 2 + dy).min(h - 1));
                        sum += u32::from(level[(sy * w + sx) * 4 + c]);
                    }
                    next[(y * nw + x) * 4 + c] = ((sum + 2) / 4) as u8;
                }
            }
        }
        pixels.extend_from_slice(&next);
        level = next;
        (w, h) = (nw, nh);
        levels += 1;
    }
    let mut image = Image::new(
        Extent3d { width, height, depth_or_array_layers: 1 },
        TextureDimension::D2,
        pixels,
        TextureFormat::Rgba8Unorm,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.texture_descriptor.mip_level_count = levels;
    image
}

/// A shared PNG as an image (its stored values, with mip levels).
fn decode_png(png: &[u8]) -> Option<Image> {
    // (decoded as sRGB only for the conversion to take it: the bytes are the
    // same, and stored as they are)
    let decoded = bevy::image::Image::from_buffer(
        png,
        bevy::image::ImageType::Extension("png"),
        bevy::image::CompressedImageFormats::NONE,
        true,
        bevy::image::ImageSampler::Default,
        RenderAssetUsages::RENDER_WORLD,
    )
    .ok()?
    .try_into_dynamic()
    .ok()?
    .to_rgba8();
    let (width, height) = decoded.dimensions();
    Some(with_mips(width, height, decoded.into_raw()))
}

fn follow_hud(
    link: Res<BridgeLink>,
    mut list: ResMut<HudList>,
    mut stats: ResMut<HudStats>,
    mut level: Option<ResMut<BridgeLevel>>,
    mut images: ResMut<Assets<Image>>,
    mut bitmaps: Local<HashMap<(u32, u16), Option<Handle<Image>>>>,
    mut textures: Local<ScreenTextures>,
    mut scenario: Local<String>,
    mut last_frame_index: Local<u32>,
) {
    // the port's art the game has shared since, and the text's atlas when it
    // has changed
    let textures = &mut *textures;
    for texture in link.bridge.read_screen_textures(textures.shared.len()) {
        let image = decode_png(&texture.png).map(|image| images.add(image));
        if image.is_none() {
            let error = bevy::image::Image::from_buffer(
                &texture.png,
                bevy::image::ImageType::Extension("png"),
                bevy::image::CompressedImageFormats::NONE,
                false,
                bevy::image::ImageSampler::Default,
                RenderAssetUsages::RENDER_WORLD,
            )
            .err();
            warn!(
                "hud: screen texture {} ({:#x}, {} bytes, starting {:02x?}) didn't decode: {error:?}",
                texture.index,
                texture.id,
                texture.png.len(),
                &texture.png[..texture.png.len().min(8)]
            );
        }
        textures.shared.resize(texture.index as usize + 1, None);
        textures.shared[texture.index as usize] = image.map(|image| (image, texture.flags));
    }
    if let Some(generation) = link.bridge.read_text_atlas(textures.atlas_generation, &mut textures.coverage) {
        textures.atlas_generation = generation;
        let pixels: Vec<u8> = textures.coverage.iter().flat_map(|&a| [255, 255, 255, a]).collect();
        let image = Image::new(
            Extent3d { width: TEXT_ATLAS_SIZE as u32, height: TEXT_ATLAS_SIZE as u32, depth_or_array_layers: 1 },
            TextureDimension::D2,
            pixels,
            TextureFormat::Rgba8Unorm,
            RenderAssetUsages::RENDER_WORLD,
        );
        match &textures.atlas {
            Some(handle) => {
                images.insert(handle.id(), image).ok();
            }
            None => textures.atlas = Some(images.add(image)),
        }
    }
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
        let mut texture_flags = 0u32;
        let mut missing = false;
        let mut bordered = 0u32;
        let mut samples_motion_sensor = false;
        for (index, map) in draw.maps.iter().enumerate() {
            if map.flags & SCREEN_MAP_BORDER != 0 {
                bordered |= 1 << index;
            }
            // the motion sensor's target (drawn first, here)
            if map.texture == SCREEN_TEXTURE_MOTION_SENSOR {
                samples_motion_sensor = index == 0;
                present |= 1 << index;
                continue;
            }
            // the port's art, or the text's atlas, in the bitmap's place
            if map.texture != u32::MAX {
                let image = if map.texture == SCREEN_TEXTURE_TEXT_ATLAS {
                    textures.atlas.clone()
                } else {
                    textures.shared.get(map.texture as usize).cloned().flatten().map(|(image, flags)| {
                        if index == 0 {
                            texture_flags = flags;
                        }
                        image
                    })
                };
                if image.is_none() {
                    missing = true;
                }
                maps[index] = image;
                present |= 1 << index;
                samplers[index] = u8::from(map.flags & SCREEN_MAP_WRAPPED != 0) | (u8::from(map.flags & SCREEN_MAP_POINT_SAMPLED != 0) << 1);
                continue;
            }
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
            modes: [
                present,
                u32::from(draw.flags & SCREEN_DRAW_ALPHA_KILL != 0),
                u32::from(texture_flags & SCREEN_TEXTURE_COVERAGE != 0) | (u32::from(texture_flags & SCREEN_TEXTURE_POINT_THRESHOLD != 0) << 1),
                bordered,
            ],
            border: unpack_color(draw.border_color),
        });
        let blend_color = unpack_color(draw.blend_color).map(f64::from);
        list.items.push(HudItem {
            motion_sensor: draw.target == SCREEN_TARGET_MOTION_SENSOR,
            samples_motion_sensor,
            maps,
            samplers,
            blend: BlendKey {
                source: draw.blend_source,
                destination: draw.blend_destination,
                operation: draw.blend_operation,
                blended: draw.flags & SCREEN_DRAW_BLENDED != 0,
                color_only: draw.flags & SCREEN_DRAW_COLOR_ONLY != 0,
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
    if key.color_only {
        let color = BlendComponent {
            src_factor: blend_factor(key.source),
            dst_factor: blend_factor(key.destination),
            operation: blend_operation(key.operation),
        };
        return Some(BlendState { color, alpha: BlendComponent { src_factor: BlendFactor::Zero, dst_factor: BlendFactor::One, operation: BlendOperation::Add } });
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

/// The motion sensor's target here (the game's has 64 texels).
const MOTION_SENSOR_SIZE: u32 = 256;

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
            targets: vec![Some(ColorTargetState {
                format: HUD_DRAW_FORMAT,
                blend: blend_state(key),
                write_mask: if key.color_only { ColorWrites::COLOR } else { ColorWrites::ALL },
            })],
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
    mut texture_cache: ResMut<bevy::render::texture::TextureCache>,
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
        // the motion sensor's target: its own draws, then sampled by the
        // screen's (larger than the game's 64 texels, as the window is)
        let motion_sensor = list.items.iter().any(|item| item.motion_sensor || item.samples_motion_sensor).then(|| {
            texture_cache.get(
                &device,
                bevy::render::render_resource::TextureDescriptor {
                    label: Some("halo_hud_motion_sensor"),
                    size: Extent3d { width: MOTION_SENSOR_SIZE, height: MOTION_SENSOR_SIZE, depth_or_array_layers: 1 },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: TextureDimension::D2,
                    format: HUD_DRAW_FORMAT,
                    usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                },
            )
        });
        let mut motion_runs = Vec::new();
        for (index, item) in list.items.iter().enumerate() {
            let pipeline_id = *pipelines.entry(item.blend).or_insert_with(|| queue_pipeline(&cache, &hud, item.blend));
            let Some(pipeline) = cache.get_render_pipeline(pipeline_id) else { continue };
            let texture = |map: usize| {
                if map == 0 && item.samples_motion_sensor {
                    if let Some(target) = &motion_sensor {
                        return &target.default_view;
                    }
                }
                item.maps[map].as_ref().and_then(|image| gpu_images.get(image)).map_or(&fallback.d2.texture_view, |image| &image.texture_view)
            };
            let sampler = |map: usize| &hud.samplers[item.samplers[map] as usize];
            let maps = device.create_bind_group(
                "halo_hud_maps",
                &maps_layout,
                &BindGroupEntries::sequential((texture(0), sampler(0), texture(1), sampler(1), texture(2), sampler(2))),
            );
            if item.motion_sensor {
                motion_runs.push((pipeline, maps, index as u32, item.blend_color));
            } else {
                runs.push((pipeline, maps, index as u32, item.blend_color));
            }
        }
        if let Some(target) = &motion_sensor {
            let mut pass = ctx.begin_tracked_render_pass(RenderPassDescriptor {
                label: Some("halo_hud_motion_sensor"),
                color_attachments: &[Some(RenderPassColorAttachment {
                    view: &target.default_view,
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
            for (pipeline, maps, index, color) in &motion_runs {
                pass.set_render_pipeline(pipeline);
                pass.set_bind_group(1, maps, &[]);
                pass.set_blend_constant(LinearRgba::new(color[0] as f32, color[1] as f32, color[2] as f32, color[3] as f32));
                pass.draw(index * 6..index * 6 + 6, *index..*index + 1);
            }
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

