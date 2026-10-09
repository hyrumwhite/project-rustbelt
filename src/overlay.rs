//! The game's overlay (ROADMAP phase 5): with `--opence`, OpenCE draws
//! everything but the opaque world (effects, transparent surfaces, decals,
//! fog, the HUD and menus) over a clear screen and shares that picture
//! through the bridge, premultiplied by its alpha
//! ([`halo_bridge::Bridge::read_overlay`]). It's drawn here over the
//! ray-traced picture, filling the window, together with the world frame it
//! was drawn for. The game draws at the window's shape (`launch_game` sizes
//! its hidden window to match), so it lines up with this camera, which takes
//! the game's field of view.

use crate::bridge::BridgeLink;
use bevy::{
    asset::{embedded_asset, RenderAssetUsages},
    prelude::*,
    render::{
        extract_resource::{ExtractResource, ExtractResourcePlugin},
        render_asset::RenderAssets,
        render_resource::{
            AsBindGroup, BlendState, Extent3d, Origin3d, RenderPipelineDescriptor, TexelCopyBufferLayout,
            TexelCopyTextureInfo, TextureAspect, TextureDimension, TextureFormat, TextureUsages,
        },
        renderer::RenderQueue,
        texture::GpuImage,
        Render, RenderApp, RenderSystems,
    },
    shader::ShaderRef,
    ui_render::{
        prelude::{MaterialNode, UiMaterial, UiMaterialPlugin},
        ui_material::UiMaterialKey,
    },
};
use std::sync::Arc;

/// The window size the game draws at (logical pixels), and so the viewer's
/// window too: the two must share a shape.
pub const WINDOW_SIZE: (u32, u32) = (1280, 720);

pub struct OverlayPlugin;

impl Plugin for OverlayPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "shaders/game_overlay.wesl");
        app.add_plugins((UiMaterialPlugin::<OverlayMaterial>::default(), ExtractResourcePlugin::<OverlayPicture>::default()))
            .init_resource::<OverlayStats>()
            .add_systems(Startup, spawn_overlay)
            .add_systems(First, read_overlay.after(crate::bridge::ReadFrame));
        app.sub_app_mut(RenderApp)
            .init_resource::<OverlayUploaded>()
            .add_systems(Render, upload_overlay.in_set(RenderSystems::PrepareResources));
    }
}

/// Whether the game shares its overlay (and so draws the transparent
/// surfaces itself).
#[derive(Resource, Clone, Copy)]
pub struct OverlayMode(pub bool);

/// Overlays shown, for the status text.
#[derive(Resource, Default)]
pub struct OverlayStats {
    pub shown: u64,
    pub size: (u32, u32),
}

#[derive(AsBindGroup, Asset, TypePath, Debug, Clone)]
pub struct OverlayMaterial {
    #[texture(0)]
    #[sampler(1)]
    picture: Handle<Image>,
    /// HALO_RT_OVERLAY_DEBUG: 1 shows the picture's coverage as a red tint, 2 the picture at half strength
    #[uniform(2)]
    mode: u32,
}

impl UiMaterial for OverlayMaterial {
    fn fragment_shader() -> ShaderRef {
        "embedded://halo_rt/shaders/game_overlay.wesl".into()
    }

    // the colours are premultiplied: additive effects have no alpha
    fn specialize(descriptor: &mut RenderPipelineDescriptor, _key: UiMaterialKey<Self>) {
        if let Some(fragment) = &mut descriptor.fragment {
            for target in fragment.targets.iter_mut().flatten() {
                target.blend = Some(BlendState::PREMULTIPLIED_ALPHA_BLENDING);
            }
        }
    }
}

/// The latest picture, for the render world to copy into the overlay's
/// texture. The texture keeps its identity (so the material's bind group
/// stays valid) and changes only when the picture's size does.
#[derive(Resource, Clone, ExtractResource)]
#[extract_app(RenderApp)]
struct OverlayPicture {
    image: Handle<Image>,
    material: Handle<OverlayMaterial>,
    size: (u32, u32),
    pixels: Arc<Vec<u8>>,
    /// Counts pictures, so each is copied once.
    generation: u64,
}

fn overlay_image(width: u32, height: u32) -> Image {
    let mut image = Image::new_fill(
        Extent3d { width, height, depth_or_array_layers: 1 },
        TextureDimension::D2,
        &[0, 0, 0, 0],
        // the game's picture is display-encoded
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.texture_descriptor.usage |= TextureUsages::COPY_DST;
    image
}

fn spawn_overlay(mut commands: Commands, mut images: ResMut<Assets<Image>>, mut materials: ResMut<Assets<OverlayMaterial>>) {
    let (width, height) = (1, 1);
    let image = images.add(overlay_image(width, height));
    let material = materials.add(OverlayMaterial { picture: image.clone(), mode: std::env::var("HALO_RT_OVERLAY_DEBUG").ok().and_then(|v| v.parse().ok()).unwrap_or(0) });
    commands.spawn((
        Name::new("Game overlay"),
        MaterialNode(material.clone()),
        Node { position_type: PositionType::Absolute, width: percent(100), height: percent(100), ..default() },
        // under the viewer's own text
        GlobalZIndex(-1),
    ));
    commands.insert_resource(OverlayPicture { image, material, size: (width, height), pixels: default(), generation: 0 });
}

fn read_overlay(
    mut link: ResMut<BridgeLink>,
    mut picture: ResMut<OverlayPicture>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<OverlayMaterial>>,
    mut stats: ResMut<OverlayStats>,
    mut buffer: Local<Vec<u8>>,
) {
    let Some(overlay) = link.bridge.read_overlay(&mut buffer) else { return };
    let size = (overlay.width, overlay.height);
    if picture.size != size {
        // a new texture at the new size (and the material's bind group made
        // again with it)
        images.insert(picture.image.id(), overlay_image(size.0, size.1)).ok();
        let _ = materials.get_mut(picture.material.id());
        picture.size = size;
    }
    // (the pixels go to the render world without copying; the buffer is
    // reused once the render world lets go of it)
    let pixels = std::mem::take(&mut *buffer);
    let old = std::mem::replace(&mut picture.pixels, Arc::new(pixels));
    if let Ok(old) = Arc::try_unwrap(old) {
        *buffer = old;
    }
    picture.generation += 1;
    // the world as the picture saw it
    link.show_frame(overlay.frame_index);
    stats.shown += 1;
    stats.size = size;
}

#[derive(Resource, Default)]
struct OverlayUploaded(u64);

fn upload_overlay(
    picture: Option<Res<OverlayPicture>>,
    images: Res<RenderAssets<GpuImage>>,
    queue: Res<RenderQueue>,
    mut uploaded: ResMut<OverlayUploaded>,
) {
    let Some(picture) = picture else { return };
    if picture.generation == uploaded.0 {
        return;
    }
    let Some(image) = images.get(&picture.image) else { return };
    let (width, height) = picture.size;
    let size = image.texture_descriptor.size;
    if size.width != width || size.height != height || picture.pixels.len() != (width * height * 4) as usize {
        // (the texture at the new size isn't ready yet)
        return;
    }
    queue.write_texture(
        TexelCopyTextureInfo { texture: &image.texture, mip_level: 0, origin: Origin3d::ZERO, aspect: TextureAspect::All },
        &picture.pixels,
        TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(width * 4), rows_per_image: None },
        size,
    );
    uploaded.0 = picture.generation;
}
