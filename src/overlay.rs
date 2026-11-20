//! The game's overlay (ROADMAP phase 5): with `--opence`, OpenCE draws
//! everything but the opaque world (effects, transparent surfaces, decals,
//! fog, the HUD and menus) over a clear screen and shares that picture
//! through the bridge, premultiplied by its alpha
//! ([`halo_bridge::Bridge::read_overlay`]). It's drawn here over the
//! ray-traced picture, filling the window, together with the world frame it
//! was drawn for. The game draws at the window's shape (`launch_game` sizes
//! its hidden window to match), so it lines up with this camera, which takes
//! the game's field of view.
//!
//! (Protocol 56) Where the driver allows, the picture crosses on the GPU in
//! place of the pixels: the viewer makes a set of textures the game imports
//! and copies its picture into, and shows the one the overlay's header
//! names ([`SharedOverlay`], `shared`); the pixels stay the way whenever
//! that is not there.

#[cfg(windows)]
mod shared;

use crate::bridge::BridgeLink;
use bevy::{
    asset::{embedded_asset, uuid::Uuid, RenderAssetUsages},
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
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};

/// The window size the game draws at (logical pixels), and so the viewer's
/// window too: the two must share a shape (fixed mode; following the game's
/// display settings, the game draws at the size the viewer writes,
/// crate::display).
pub const WINDOW_SIZE: (u32, u32) = (1280, 720);

pub struct OverlayPlugin;

impl Plugin for OverlayPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "shaders/game_overlay.wesl");
        let exchange = SharedOverlayExchange::default();
        app.add_plugins((UiMaterialPlugin::<OverlayMaterial>::default(), ExtractResourcePlugin::<OverlayPicture>::default()))
            .init_resource::<OverlayStats>()
            .insert_resource(exchange.clone())
            .add_systems(Startup, spawn_overlay)
            .add_systems(First, (read_overlay.after(crate::bridge::ReadFrame), trace_frame.after(read_overlay)));
        app.sub_app_mut(RenderApp)
            .init_resource::<OverlayUploaded>()
            .insert_resource(exchange)
            .add_systems(Render, upload_overlay.in_set(RenderSystems::PrepareResources));
        // (the shared textures are made in the render world, after the
        // window's surface is configured and never beside it, and put under
        // the slots' images there)
        #[cfg(windows)]
        app.sub_app_mut(RenderApp).add_systems(
            Render,
            make_shared_textures.in_set(RenderSystems::Prepare).after(bevy::render::view::window::prepare_windows),
        );
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
    /// HALO_RT_OVERLAY_DEBUG: 1 shows the picture's coverage as a red tint, 2 the picture at half strength, 3 the picture alone (opaque, nothing of the world under it: its bytes as shared)
    #[uniform(2)]
    mode: u32,
}

impl OverlayMaterial {
    /// A premultiplied picture drawn over the window (the HUD's,
    /// `crate::hud`; the overlay's debug mode is the game's picture's alone).
    pub fn new(picture: Handle<Image>) -> Self {
        Self { picture, mode: 0 }
    }
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
/// texture. The texture keeps its identity (so the materials' bind groups stay valid) and
/// changes only when the picture's size does.
#[derive(Resource, Clone, ExtractResource)]
#[extract_app(RenderApp)]
pub(crate) struct OverlayPicture {
    pub(crate) image: Handle<Image>,
    material: Handle<OverlayMaterial>,
    size: (u32, u32),
    pixels: Arc<Vec<u8>>,
    /// Counts pictures, so each is copied once.
    generation: u64,
    /// The picture last copied was clear (the game drew nothing into it).
    empty: bool,
}

/// The overlay's texture for a picture of this size.
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

fn spawn_overlay(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<OverlayMaterial>>,
) {
    let (width, height) = (1, 1);
    let image = images.add(overlay_image(width, height));
    let mode = std::env::var("HALO_RT_OVERLAY_DEBUG").ok().and_then(|v| v.parse().ok()).unwrap_or(0);
    let material = materials.add(OverlayMaterial { picture: image.clone(), mode });
    let node = || {
        (
            // (the picture's rectangle where it's scaled into the window)
            crate::display::PictureRect,
            Node { position_type: PositionType::Absolute, width: percent(100), height: percent(100), ..default() },
            // under the HUD (crate::hud) and the viewer's own text
            GlobalZIndex(-3),
        )
    };
    let pixel_node = commands.spawn((Name::new("Game overlay"), MaterialNode(material.clone()), node())).id();
    // (protocol 56) a node and material a shared texture, the same in all
    // but the picture, hidden until the overlay's header names its slot
    let mut slot_nodes = Vec::new();
    let mut slot_materials = Vec::new();
    if cfg!(windows) {
        for slot in 0..halo_bridge::OVERLAY_SHARED_SLOTS {
            let slot_material = materials.add(OverlayMaterial { picture: shared_slot_image(slot), mode });
            slot_nodes.push(commands.spawn((Name::new(format!("Game overlay, shared texture {slot}")), MaterialNode(slot_material.clone()), node(), Visibility::Hidden)).id());
            slot_materials.push(slot_material);
        }
    }
    let hold = std::env::var("HALO_RT_OVERLAY_SHARED_HOLD").ok().and_then(|v| v.parse::<usize>().ok()).unwrap_or(4).clamp(1, 6);
    commands.insert_resource(SharedOverlay {
        state: SharedState::Undecided,
        pixel_node,
        slot_nodes,
        slot_materials,
        serial: 0,
        requested: None,
        current: None,
        refused: Vec::new(),
        accepted: 0,
        size: None,
        shown: Shown::Pixels,
        history: VecDeque::new(),
        hold,
    });
    commands.insert_resource(OverlayPicture { image, material, size: (width, height), pixels: default(), generation: 0, empty: false });
}

/// The image a shared slot's material binds: an id no asset in the main
/// world has, so no asset event ever touches it; the render world puts the
/// slot's texture under it (`make_shared_textures`).
fn shared_slot_image(slot: usize) -> Handle<Image> {
    Handle::Uuid(Uuid::from_u128(0x6f76_6572_6c61_7973_6861_7265_6400_0000 + slot as u128), std::marker::PhantomData)
}

/// Which of the overlay's nodes is shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Shown {
    /// the pixel path's texture
    Pixels,
    /// a shared texture
    Slot(u32),
    /// none (a set just made, until the next overlay)
    Nothing,
}

enum SharedState {
    /// until the first frame with the game's overlay
    Undecided,
    Off,
    On,
}

/// (Protocol 56) The overlay's shared textures, the main world's side:
/// made only when `HALO_RT_OVERLAY_SHARED` is not `0`, on Windows, for a
/// game this viewer launched that shares overlays, and when the device can
/// export the images; a new set (and serial) whenever the overlay's size or
/// the game's process changes, the old one kept a few frames; none again
/// for a size the game refused. The game copies into a slot that is not
/// `held`: the one shown and those shown in the last
/// `HALO_RT_OVERLAY_SHARED_HOLD` frames (4), for the frames still in flight
/// here.
#[derive(Resource)]
struct SharedOverlay {
    state: SharedState,
    pixel_node: Entity,
    slot_nodes: Vec<Entity>,
    slot_materials: Vec<Handle<OverlayMaterial>>,
    /// the last serial given
    serial: u32,
    /// the set asked of the render world and not yet made: its serial, size
    /// and process
    requested: Option<(u32, (u32, u32), u32)>,
    /// the set offered to the game now: its serial, size and process
    current: Option<(u32, (u32, u32), u32)>,
    /// sizes and processes the game refused a set for
    refused: Vec<((u32, u32), u32)>,
    /// the serial whose acceptance was logged
    accepted: u32,
    /// the overlay's size as its headers last said
    size: Option<(u32, u32)>,
    shown: Shown,
    /// the slot shown in each of the last frames (a bit), the latest last
    history: VecDeque<u32>,
    hold: usize,
}

impl SharedOverlay {
    fn active(&self) -> bool {
        matches!(self.state, SharedState::On)
    }

    /// A frame begins: the slot shown goes into the history; the slots
    /// held, written for the game.
    fn advance(&mut self, link: &BridgeLink) -> u32 {
        if !self.active() {
            return 0;
        }
        self.history.push_back(match self.shown {
            Shown::Slot(slot) => 1 << slot,
            _ => 0,
        });
        while self.history.len() > self.hold {
            self.history.pop_front();
        }
        let held = self.history.iter().fold(0, |held, bits| held | bits);
        link.bridge.write_overlay_held(held);
        held
    }

    fn show(&mut self, shown: Shown, visibility: &mut Query<&mut Visibility>) {
        if self.shown == shown {
            return;
        }
        self.shown = shown;
        let set = |visibility: &mut Query<&mut Visibility>, entity: Entity, visible: bool| {
            if let Ok(mut v) = visibility.get_mut(entity) {
                v.set_if_neq(if visible { Visibility::Inherited } else { Visibility::Hidden });
            }
        };
        set(visibility, self.pixel_node, shown == Shown::Pixels);
        for (slot, &entity) in self.slot_nodes.iter().enumerate() {
            set(visibility, entity, shown == Shown::Slot(slot as u32));
        }
    }
}

/// (Protocol 56) Between the two worlds: the set the main world asks for
/// (serial, size, game process; `None` for none) and what the render world
/// made of the last request (the offer for the game, or why not).
#[derive(Resource, Clone, Default)]
struct SharedOverlayExchange(Arc<Mutex<Exchange>>);

#[derive(Default)]
struct Exchange {
    request: Option<(u32, (u32, u32), u32)>,
    result: Option<(u32, Result<halo_bridge::OverlaySet, String>)>,
}

/// The set offered: asked for, offered once made, answered, replaced on a
/// size or process change, withdrawn on a refusal; the materials told to
/// bind again with a new set.
#[cfg(windows)]
fn update_shared_set(
    shared: &mut SharedOverlay,
    link: &BridgeLink,
    launched: bool,
    exchange: &SharedOverlayExchange,
    materials: &mut Assets<OverlayMaterial>,
    visibility: &mut Query<&mut Visibility>,
) {
    if matches!(shared.state, SharedState::Undecided) {
        shared.state = if std::env::var("HALO_RT_OVERLAY_SHARED").is_ok_and(|v| v == "0") {
            info!("overlay: shared textures off (HALO_RT_OVERLAY_SHARED=0)");
            SharedState::Off
        } else if launched && link.bridge.overlay_enabled() {
            SharedState::On
        } else {
            SharedState::Off
        };
    }
    if !shared.active() {
        return;
    }
    let Ok(mut exchange) = exchange.0.lock() else { return };
    // a set made (or not) for the request
    if let Some((serial, result)) = exchange.result.take() {
        match (result, shared.requested) {
            (Ok(offer), Some(requested)) if requested.0 == serial => {
                link.bridge.offer_overlay_set(&offer);
                for material in &shared.slot_materials {
                    if let Some(material) = materials.get_mut(material) {
                        material.into_inner();
                    }
                }
                info!("overlay: shared textures, {} slots of {}x{}, serial {}", halo_bridge::OVERLAY_SHARED_SLOTS, offer.width, offer.height, serial);
                shared.current = Some(requested);
                shared.requested = None;
                // (a slot node shows the new set's texture once bound again:
                // nothing until the game names one)
                if matches!(shared.shown, Shown::Slot(_)) {
                    shared.show(Shown::Nothing, visibility);
                }
            }
            (Err(reason), _) => {
                info!("overlay: shared textures unavailable: {reason}; the pixels as before");
                shared.state = SharedState::Off;
                shared.requested = None;
                exchange.request = None;
                return;
            }
            // (a request since replaced)
            _ => {}
        }
    }
    let (imported, refused) = link.bridge.overlay_set_answers();
    if let Some((serial, size, process_id)) = shared.current {
        if refused == serial {
            info!("overlay: the game refused the shared textures, serial {serial} ({}x{}); the pixels as before", size.0, size.1);
            shared.refused.push((size, process_id));
            link.bridge.withdraw_overlay_set();
            shared.current = None;
            exchange.request = None;
            if matches!(shared.shown, Shown::Slot(_)) {
                shared.show(Shown::Nothing, visibility);
            }
        } else if imported == serial && shared.accepted != serial {
            info!("overlay: the game imported the shared textures, serial {serial}");
            shared.accepted = serial;
        }
    }
    let Some(size) = shared.size else { return };
    let process_id = link.bridge.writer_process_id();
    let wanted = |set: Option<(u32, (u32, u32), u32)>| set.is_some_and(|(_, s, p)| s == size && p == process_id);
    if wanted(shared.current) || wanted(shared.requested) || shared.refused.contains(&(size, process_id)) {
        return;
    }
    shared.serial += 1;
    shared.requested = Some((shared.serial, size, process_id));
    exchange.request = shared.requested;
}

/// The render world's side of the shared textures: the set the main world
/// asks for made (`shared::make`) and each texture put under its slot's
/// image with the default sampler, as the pixel path's image has it; the
/// sets replaced kept a few frames.
#[cfg(windows)]
#[derive(Default)]
struct SharedRender {
    /// whether the device can (found at the first request)
    capability: Option<Result<shared::Capability, String>>,
    /// the serial last made (or tried)
    made: u32,
    current: Option<shared::Slots>,
    retiring: Vec<(shared::Slots, u32)>,
}

#[cfg(windows)]
fn make_shared_textures(
    exchange: Res<SharedOverlayExchange>,
    device: Res<bevy::render::renderer::RenderDevice>,
    sampler: Res<bevy::render::render_resource::DefaultImageSampler>,
    mut images: ResMut<RenderAssets<GpuImage>>,
    mut state: Local<SharedRender>,
) {
    // (well past the frames that may still sample a set replaced)
    const RETIRE_FRAMES: u32 = 12;
    state.retiring.retain_mut(|(_, frames)| {
        *frames = frames.saturating_sub(1);
        *frames > 0
    });
    let Ok(mut exchange) = exchange.0.lock() else { return };
    let Some((serial, size, process_id)) = exchange.request else {
        if let Some(old) = state.current.take() {
            state.retiring.push((old, RETIRE_FRAMES));
        }
        return;
    };
    if serial == state.made {
        return;
    }
    state.made = serial;
    let capability = state.capability.get_or_insert_with(|| shared::capability(device.wgpu_device())).clone();
    let result = capability.and_then(|capability| shared::make(device.wgpu_device(), &capability, serial, size, process_id));
    match result {
        Ok((slots, offer)) => {
            for (slot, texture) in slots.textures.iter().enumerate() {
                let view = texture.create_view(&default());
                images.insert(
                    shared_slot_image(slot).id(),
                    GpuImage {
                        texture: texture.clone().into(),
                        texture_view: view.into(),
                        sampler: (**sampler).clone(),
                        texture_descriptor: bevy::render::render_resource::TextureDescriptor {
                            label: None,
                            size: Extent3d { width: size.0, height: size.1, depth_or_array_layers: 1 },
                            mip_level_count: 1,
                            sample_count: 1,
                            dimension: TextureDimension::D2,
                            format: TextureFormat::Rgba8UnormSrgb,
                            usage: TextureUsages::TEXTURE_BINDING,
                            view_formats: &[],
                        },
                        texture_view_descriptor: None,
                        had_data: true,
                    },
                );
            }
            if let Some(old) = state.current.replace(slots) {
                state.retiring.push((old, RETIRE_FRAMES));
            }
            exchange.result = Some((serial, Ok(offer)));
        }
        Err(reason) => exchange.result = Some((serial, Err(reason))),
    }
}

#[allow(clippy::too_many_arguments)]
fn read_overlay(
    mut link: ResMut<BridgeLink>,
    mut picture: ResMut<OverlayPicture>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<OverlayMaterial>>,
    mut stats: ResMut<OverlayStats>,
    mut buffer: Local<Vec<u8>>,
    mut looks: Local<Option<(std::time::Instant, f32)>>,
    mut shared: ResMut<SharedOverlay>,
    exchange: Res<SharedOverlayExchange>,
    process: Option<Res<crate::bridge::GameProcess>>,
    mut visibility: Query<&mut Visibility>,
) {
    // how often this looks (microseconds, smoothed), for the game's pacing
    let now = std::time::Instant::now();
    let interval = match *looks {
        Some((last, interval)) => {
            let since = (now - last).as_secs_f32() * 1e6;
            // (a hitch, such as a load, isn't taken as the pace)
            if since < 100_000.0 { interval + (since - interval) / 8.0 } else { interval }
        }
        None => 0.0,
    };
    *looks = Some((now, interval));
    if interval > 0.0 && paced() {
        link.bridge.note_overlay_look(interval.round() as u32);
    }
    #[cfg(windows)]
    update_shared_set(&mut shared, &link, process.is_some(), &exchange, &mut materials, &mut visibility);
    #[cfg(not(windows))]
    let _ = (&exchange, &process);
    let held = shared.advance(&link);
    let Some(overlay) = link.bridge.read_overlay(&mut buffer, held) else { return };
    let size = (overlay.width, overlay.height);
    shared.size = Some(size);
    // (protocol 56) in a shared texture: its node shown, if the set is the
    // one offered now (a header of an older set leaves the picture as it is)
    if let Some((serial, slot)) = overlay.shared.filter(|_| !overlay.empty) {
        if serial != 0 && shared.current.is_some_and(|(current, _, _)| current == serial) {
            shared.show(Shown::Slot(slot), &mut visibility);
        }
        if !link.show_frame(overlay.frame_index) && std::env::var_os("HALO_RT_SYNC_LOG").is_some() {
            info!("overlay frame {} not among the frames read", overlay.frame_index);
        }
        stats.shown += 1;
        stats.size = size;
        return;
    }
    shared.show(Shown::Pixels, &mut visibility);
    // nothing drawn into it (the viewer drawing all of it): the picture
    // cleared once, then left
    if overlay.empty {
        if !picture.empty || picture.size != size {
            if picture.size != size {
                images.insert(picture.image.id(), overlay_image(size.0, size.1)).ok();
                rebind(&mut materials, picture.material.id());
                picture.size = size;
            }
            picture.pixels = Arc::new(vec![0; (size.0 * size.1 * 4) as usize]);
            picture.generation += 1;
            picture.empty = true;
        }
        if !link.show_frame(overlay.frame_index) && std::env::var_os("HALO_RT_SYNC_LOG").is_some() {
            info!("overlay frame {} not among the frames read", overlay.frame_index);
        }
        stats.shown += 1;
        stats.size = size;
        return;
    }
    picture.empty = false;
    if picture.size != size {
        // a new texture at the new size (and the material's bind group made
        // again with it)
        images.insert(picture.image.id(), overlay_image(size.0, size.1)).ok();
        rebind(&mut materials, picture.material.id());
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
    if !link.show_frame(overlay.frame_index) && std::env::var_os("HALO_RT_SYNC_LOG").is_some() {
        let pending: Vec<u32> = link.pending.iter().map(|s| s.frame_index).collect();
        info!("overlay frame {} not among the frames read: {pending:?}, showing {:?}", overlay.frame_index, link.latest.as_ref().map(|s| s.frame_index));
    }
    stats.shown += 1;
    stats.size = size;
}

/// `HALO_RT_FRAME_TRACE=<path>`: a line a frame in that file, once the
/// frame's overlay is read: the performance counter (microseconds, as the
/// game's `HALO_FRAME_TRACE` reads it), the game frame shown, its tick and
/// the game's camera; last, the vehicle nearest the camera in the snapshot
/// shown (`veh <object index> <root position> <bounding centre> <root
/// forward>`, or `veh none`), to set the vehicle as received against the
/// camera frame by frame (a Warthog seen to jump ahead of its chase camera
/// now and then; scripts/pace/vehicle.py).
fn trace_frame(
    link: Res<BridgeLink>,
    windows: Query<&Window, With<bevy::window::PrimaryWindow>>,
    mut file: Local<Option<Option<std::io::BufWriter<std::fs::File>>>>,
) {
    use std::io::Write;
    let file = file.get_or_insert_with(|| {
        let path = std::env::var_os("HALO_RT_FRAME_TRACE")?;
        Some(std::io::BufWriter::new(std::fs::File::create(path).ok()?))
    });
    let (Some(file), Some(snapshot), Some(now)) = (file.as_mut(), link.latest.as_ref(), halo_bridge::performance_microseconds()) else { return };
    let position = snapshot.camera.map(|c| c.position).unwrap_or_default();
    let forward = snapshot.camera.map(|c| c.forward).unwrap_or_default();
    // (Halo's object type 1 is the vehicle; the nearest by its bounding
    // centre, the camera's player's seat among them)
    let distance = |p: [f32; 3]| (0..3).map(|i| (p[i] - position[i]) * (p[i] - position[i])).sum::<f32>();
    let vehicle = snapshot
        .objects
        .iter()
        .filter(|o| o.kind == 1 && o.flags & halo_bridge::OBJECT_MODEL == 0)
        .filter_map(|o| Some((o, snapshot.object_nodes(o).first()?)))
        .min_by(|a, b| distance(a.0.bounding_center).total_cmp(&distance(b.0.bounding_center)));
    let vehicle = match vehicle {
        Some((object, root)) => {
            let (p, c, f) = (root.position, object.bounding_center, root.forward);
            format!(
                "{:x} {:.5} {:.5} {:.5} {:.5} {:.5} {:.5} {:.5} {:.5} {:.5}",
                object.object_index, p[0], p[1], p[2], c[0], c[1], c[2], f[0], f[1], f[2]
            )
        }
        None => "none".to_string(),
    };
    let _ = writeln!(
        file,
        "V {now} {} {} cam {:.4} {:.4} {:.4} fwd {:.5} {:.5} {:.5} pending {} focus {} veh {vehicle}",
        snapshot.frame_index,
        snapshot.game_tick,
        position[0],
        position[1],
        position[2],
        forward[0],
        forward[1],
        forward[2],
        link.pending.len(),
        windows.single().map_or(0, |w| w.focused as u8)
    );
}

/// Has a material's bind group made again with its image, after the image
/// was replaced (a new size): the bind group holds the texture it was made
/// with, and Bevy's `AssetMut` reports the material changed only when it is
/// borrowed mutably, so a guard dropped untouched told it nothing and the
/// picture stayed on the old texture (the menus blank or frozen after the
/// window's size changed). `HALO_RT_HUD_STALE_TEXTURE=1` the old (the
/// material not told), for the overlay and the HUD alike. Read once.
pub(crate) fn rebind(materials: &mut Assets<OverlayMaterial>, id: AssetId<OverlayMaterial>) {
    static STALE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    if *STALE.get_or_init(|| std::env::var_os("HALO_RT_HUD_STALE_TEXTURE").is_some()) {
        return;
    }
    if let Some(material) = materials.get_mut(id) {
        material.into_inner();
    }
}

/// Whether the game's frames are paced by this one's looks for an overlay
/// (`HALO_RT_NO_PACE=1`: drawn as the display allows, as before 2026-10-23).
pub fn paced() -> bool {
    std::env::var_os("HALO_RT_NO_PACE").is_none()
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
