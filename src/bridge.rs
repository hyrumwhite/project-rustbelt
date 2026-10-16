//! `--bridge`: draw a running OpenCE game (ROADMAP phase 4).
//!
//! OpenCE, built from its `bevy-bridge` branch and started with
//! `HALO_BEVY_BRIDGE=1`, shares each frame's camera and every object's node
//! matrices ([`halo_bridge`]). The level's BSPs, sky and lights load from the
//! map file as usual; the objects come from the game: spawned when they
//! appear, posed by their node matrices every frame, despawned when they go.
//! The camera follows the game's (F8 detaches it into a free camera).
//!
//! While following, a click grabs the cursor and this window's keyboard and
//! mouse play the game (forwarded through the bridge; F12 lets go), so the
//! game's own window can stay in the background.

use crate::level::{halo_affine, halo_to_bevy_affine, AnimatedModel, LevelAssets, LevelBuilder, TracedMesh};
use crate::material::{HaloEnvironmentMaterial, HaloTransparentMaterial, NeutralDetail};
use bevy_solarik::prelude::RaytracingMesh3d;
use bevy::{
    camera_controller::free_camera::FreeCamera,
    math::Affine3A,
    platform::collections::HashMap,
    prelude::*,
};
use halo_bridge::{Bridge, Snapshot, OBJECT_FIRST_PERSON, OBJECT_HIDDEN, OBJECT_MODEL};
use std::{sync::Arc, time::Duration};

/// The simulation the game runs: the systems in Rust (its
/// game.rust_simulation: "all", "none", or names) and the Rust simulation's
/// DLL (halo_sim.dll beside the game when None).
pub struct Simulation<'a> {
    pub systems: &'a str,
    pub dll: Option<&'a std::path::Path>,
}

/// Starts OpenCE on the map at `map_path`: its data root is the folder that
/// holds the maps folder, the level is the map's scenario, the bridge is on
/// and (unless `show`) its window is hidden. With `overlay`, it shares its
/// overlay (`crate::overlay`), drawing at the viewer's window size, and
/// with `viewer_glass` leaves glass and water out of it (the viewer draws
/// them), with `viewer_fog` its fog, with `viewer_decals` its decals and
/// with `viewer_effects` its sprites, which it shares instead. With `simulation`, those systems run in Rust. Its output goes to
/// `halo_rt_opence-<viewer process id>.log` in the temporary folder.
pub fn launch_game(
    exe: &std::path::Path,
    map_path: &std::path::Path,
    start_on_map: bool,
    bridge_name: &str,
    show: bool,
    overlay: bool,
    viewer_glass: bool,
    viewer_fog: bool,
    viewer_decals: bool,
    viewer_effects: bool,
    simulation: Option<Simulation>,
) -> std::io::Result<std::process::Child> {
    use std::io::{Error, ErrorKind};
    let cache = halo_map::CacheFile::open(map_path).map_err(|e| Error::new(ErrorKind::InvalidData, e.to_string()))?;
    let scenario = cache.tag(cache.scenario_datum).map_err(|e| Error::new(ErrorKind::InvalidData, e.to_string()))?.name.clone();
    let data_root = map_path
        .parent()
        .and_then(|maps| maps.parent())
        .ok_or_else(|| Error::new(ErrorKind::NotFound, "the map is not in a maps folder"))?;
    let log_path = std::env::temp_dir().join(format!("halo_rt_opence-{}.log", std::process::id()));
    let log = std::fs::File::create(&log_path)?;
    let mut command = std::process::Command::new(exe);
    command
        .current_dir(exe.parent().unwrap_or(std::path::Path::new(".")))
        .env("HALO_BEVY_BRIDGE", "1")
        .env("HALO_BEVY_BRIDGE_NAME", bridge_name)
        .env("HALO_DATA_ROOT", data_root)
        .stdout(log.try_clone()?)
        .stderr(log);
    if start_on_map {
        command.env("HALO_BEVY_BRIDGE_MAP", &scenario);
    }
    if let Some(simulation) = &simulation {
        let systems = if simulation.systems == "none" { "" } else { simulation.systems };
        command.env("HALO_RUST_SIMULATION", systems);
        if let Some(dll) = simulation.dll {
            command.env("HALO_SIM_DLL", dll);
        }
        eprintln!("the simulation in Rust: {}", if systems.is_empty() { "none" } else { systems });
    }
    if !show {
        // hidden, and small and windowed so it never takes the display; it
        // stays vsynced, so its frames (and the poses it shares) come at the
        // display's rate
        command
            .env("HALO_HIDDEN_WINDOW", "1")
            .env("HALO_DISPLAY_MODE", "windowed")
            .env("HALO_WINDOW_SIZE", "640x480");
    }
    if overlay {
        let (width, height) = crate::overlay::WINDOW_SIZE;
        command.env("HALO_BEVY_BRIDGE_OVERLAY", "1").env("HALO_WINDOW_SIZE", format!("{width}x{height}"));
        // (glass and water: the viewer's, ray traced)
        if viewer_glass {
            command.env("HALO_BEVY_BRIDGE_VIEWER_GLASS", "1");
        }
        // (and the fog: the viewer's, from what each frame shares)
        if viewer_fog {
            command.env("HALO_BEVY_BRIDGE_VIEWER_FOG", "1");
        }
        if viewer_decals {
            command.env("HALO_BEVY_BRIDGE_VIEWER_DECALS", "1");
        }
        if viewer_effects {
            command.env("HALO_BEVY_BRIDGE_VIEWER_EFFECTS", "1");
        }
    }
    let start = if start_on_map { format!("on {scenario}") } else { "at its menus".to_string() };
    eprintln!("starting OpenCE {start} (data root {}; its log: {})", data_root.display(), log_path.display());
    command.spawn()
}

/// The game the viewer started; stopped with the viewer.
#[derive(Resource)]
pub struct GameProcess(pub std::process::Child);

impl Drop for GameProcess {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// The bridge name for a game this viewer starts: its own, so that several
/// viewers (each with its game) can run at once, as system link needs.
pub fn own_bridge_name() -> String {
    format!("{}-{}", halo_bridge::DEFAULT_NAME, std::process::id())
}

/// Waits for the game sharing as `name` to share a frame, so the viewer
/// knows which map to load. `None` after `timeout`.
pub fn wait_for_game(name: &str, timeout: Duration) -> Option<(Bridge, Snapshot)> {
    let start = std::time::Instant::now();
    let mut announced = false;
    while start.elapsed() < timeout {
        match Bridge::open_named(name) {
            Ok(mut bridge) => {
                if let Some(snapshot) = bridge.read()
                    && !snapshot.scenario_name.is_empty()
                {
                    return Some((bridge, snapshot));
                }
            }
            Err(e) if !announced => {
                eprintln!("waiting for OpenCE: {e}");
                announced = true;
            }
            Err(_) => {}
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    None
}

pub struct BridgePlugin;

/// Reading the game's latest frame (in `First`).
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct ReadFrame;

impl Plugin for BridgePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<BridgeObjects>()
            .init_resource::<FollowGame>()
            .init_resource::<ForwardInput>()
            .add_systems(First, read_frame.in_set(ReadFrame))
            .add_systems(PreUpdate, (grab_cursor, forward_input, forward_gamepads).chain().after(bevy::input::InputSystems))
            .add_systems(Update, (toggle_follow, sync_objects, sync_camera, sync_active_bsp).chain());
    }
}

/// The open bridge and the latest frame read from it.
#[derive(Resource)]
pub struct BridgeLink {
    pub bridge: Bridge,
    pub latest: Option<Snapshot>,
    /// Frames read since the last overlay update (for its rate).
    pub frames: u32,
    /// With the game's overlay: the frames read but not yet shown, oldest
    /// first; the overlay shows the one its picture was drawn for.
    pub pending: std::collections::VecDeque<Snapshot>,
    pub sync_to_overlay: bool,
}

impl BridgeLink {
    pub fn new(bridge: Bridge, snapshot: Snapshot) -> Self {
        BridgeLink { bridge, latest: Some(snapshot), frames: 0, pending: default(), sync_to_overlay: false }
    }

    /// Shows the frame `frame_index` (the overlay's) if it was read; the
    /// frames before it are dropped. False if it wasn't among them.
    pub fn show_frame(&mut self, frame_index: u32) -> bool {
        let Some(position) = self.pending.iter().position(|s| s.frame_index == frame_index) else { return false };
        self.pending.drain(..position);
        self.latest = self.pending.pop_front();
        true
    }
}

/// The level builder, kept to make objects' meshes as the game creates them.
#[derive(Resource)]
pub struct BridgeLevel {
    pub builder: LevelBuilder,
    pub raster: bool,
}

/// Whether the camera follows the game's.
#[derive(Resource)]
pub struct FollowGame(pub bool);

impl Default for FollowGame {
    fn default() -> Self {
        FollowGame(true)
    }
}

struct Tracked {
    root: Entity,
    definition: u32,
    permutations: [u8; 8],
    change_colors: [[f32; 3]; 4],
    camouflaged: bool,
    model: Arc<AnimatedModel>,
    nodes: Vec<Option<Entity>>,
    /// The rays' entities for its bending parts (`AnimatedModel::traced_skins`),
    /// and the node transforms they were last skinned to.
    skins: Vec<Entity>,
    skinned_to: Vec<Mat4>,
}

/// The game's objects drawn so far, by object datum index.
#[derive(Resource, Default)]
pub struct BridgeObjects {
    tracked: HashMap<u32, Tracked>,
    /// Definitions without a model (nothing to draw).
    modelless: HashMap<(u32, [u8; 8]), ()>,
    /// Models made again, still drawn until their replacements are ready
    /// (each with the frames it has left).
    retiring: Vec<(Entity, u8)>,
}

/// How many frames a model made again stays drawn: its replacement takes
/// its transforms the next frame and its new materials a frame or two later.
const RETIRING_FRAMES: u8 = 2;

/// An object's change colours in steps of 1/16: some follow the object's
/// functions (the plasma rifle's heat, the cyborg's shield), and each change
/// makes the model again with newly baked textures.
fn change_color_steps(colors: &[[f32; 3]; 4]) -> [[f32; 3]; 4] {
    colors.map(|color| color.map(|c| (c.clamp(0.0, 1.0) * 16.0).round() / 16.0))
}

impl BridgeObjects {
    pub fn count(&self) -> usize {
        self.tracked.len()
    }

    /// Forgets every object (their entities went with the level).
    pub fn clear(&mut self) {
        self.tracked.clear();
        self.modelless.clear();
        self.retiring.clear();
    }
}

fn read_frame(mut link: ResMut<BridgeLink>) {
    while let Some(snapshot) = link.bridge.read() {
        link.frames += 1;
        if link.sync_to_overlay {
            // (a few frames: the overlay comes at most a frame or two behind)
            if link.pending.len() >= 8 {
                link.pending.pop_front();
            }
            link.pending.push_back(snapshot);
        } else {
            // only the newest frame matters; older ones were already
            // superseded
            link.latest = Some(snapshot);
        }
    }
}

/// F8: follow the game's camera, or fly free.
fn toggle_follow(
    keys: Res<ButtonInput<KeyCode>>,
    mut follow: ResMut<FollowGame>,
    camera: Single<Entity, With<Camera3d>>,
    mut commands: Commands,
) {
    if !keys.just_pressed(KeyCode::F8) {
        return;
    }
    follow.0 = !follow.0;
    if follow.0 {
        commands.entity(*camera).remove::<FreeCamera>();
    } else {
        commands.entity(*camera).insert(FreeCamera { walk_speed: 6.0, run_speed: 25.0, ..default() });
    }
}

fn sync_objects(
    link: Res<BridgeLink>,
    mut level: ResMut<BridgeLevel>,
    mut objects: ResMut<BridgeObjects>,
    mut commands: Commands,
    mut transforms: Query<&mut Transform>,
    mut visibility: Query<&mut Visibility>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut environment_materials: ResMut<Assets<HaloEnvironmentMaterial>>,
    mut transparent_materials: ResMut<Assets<HaloTransparentMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut inverse_bindposes: ResMut<Assets<bevy::mesh::skinning::SkinnedMeshInverseBindposes>>,
    neutral_detail: Res<NeutralDetail>,
    (recolourable, animated): (Query<(Entity, &crate::level::Recolourable)>, Query<(), With<crate::level::AnimatedSelfIllumination>>),
    mut flat_normal: Local<Option<Handle<Image>>>,
    mut timing: Local<SyncTiming>,
) {
    let Some(snapshot) = &link.latest else { return };
    let started = std::time::Instant::now();
    timing.frame(snapshot.frame_index);
    let flat_normal = flat_normal.get_or_insert_with(|| images.add(crate::material::flat_normal_image())).clone();
    let to_bevy = halo_to_bevy_affine();
    let from_bevy = to_bevy.inverse();
    let assets = &mut LevelAssets {
        meshes: &mut meshes,
        materials: &mut materials,
        environment_materials: &mut environment_materials,
        transparent_materials: &mut transparent_materials,
        images: &mut images,
        inverse_bindposes: &mut inverse_bindposes,
        neutral_detail: neutral_detail.0.clone(),
        flat_normal,
    };
    let BridgeLevel { builder, raster } = &mut *level;
    let raster = *raster;
    // (a frame of another level than the one loaded: the viewer is about to
    // follow the game there, main.rs follow_level)
    if !snapshot.map_name().eq_ignore_ascii_case(&builder.stats.map_name) {
        return;
    }

    // (models made again, gone once their replacements are drawn)
    objects.retiring.retain_mut(|(entity, frames)| {
        *frames -= 1;
        if *frames == 0 {
            commands.entity(*entity).despawn();
        }
        *frames > 0
    });

    let mut seen = HashMap::<u32, ()>::default();
    for object in &snapshot.objects {
        seen.insert(object.object_index, ());
        let change_colors = change_color_steps(&object.change_colors);
        // an object whose change colours alone changed takes new materials
        // (but one with animated self-illumination, whose own copies of its
        // materials the animation keeps)
        if let Some(tracked) = objects.tracked.get_mut(&object.object_index)
            && tracked.definition == object.definition_index
            && tracked.permutations == object.region_permutations
            && tracked.camouflaged == camouflaged(object)
            && tracked.change_colors != change_colors
            && !animated.contains(tracked.root)
        {
            let root = tracked.root;
            tracked.change_colors = change_colors;
            builder.recolour(&mut commands, recolourable.iter().filter(|(_, r)| r.root == root), &change_colors, assets);
        }
        // an object whose definition or permutations changed is made again
        let current = objects.tracked.get(&object.object_index);
        if current.is_some_and(|t| {
            t.definition != object.definition_index
                || t.permutations != object.region_permutations
                || t.change_colors != change_colors
                || t.camouflaged != camouflaged(object)
        }) {
            let old = objects.tracked.remove(&object.object_index).unwrap();
            objects.retiring.push((old.root, RETIRING_FRAMES));
        }
        if !objects.tracked.contains_key(&object.object_index) {
            let key = (object.definition_index, object.region_permutations);
            if objects.modelless.contains_key(&key) {
                continue;
            }
            // (a model the game draws by itself names its model tag directly)
            let model_datum = if object.flags & OBJECT_MODEL != 0 {
                Some(object.definition_index)
            } else {
                let definition = halo_map::TagRef { group: 0, datum: Some(object.definition_index) };
                halo_map::objects::object_model(&builder.cache, definition).ok().flatten().and_then(|m| m.datum)
            };
            let model = model_datum.and_then(|m| builder.animated_model(m, object.region_permutations, assets));
            let Some(model) = model else {
                objects.modelless.insert(key, ());
                continue;
            };
            let (root, nodes, skins) =
                builder.spawn_animated_model(&mut commands, &model, raster, &change_colors, camouflaged(object), object.object_index, assets);
            objects.tracked.insert(
                object.object_index,
                Tracked {
                    root,
                    definition: object.definition_index,
                    permutations: object.region_permutations,
                    change_colors,
                    camouflaged: camouflaged(object),
                    model,
                    nodes,
                    skins,
                    skinned_to: Vec::new(),
                },
            );
            // newly spawned entities take their transforms next frame
            continue;
        }
        let Some(tracked) = objects.tracked.get_mut(&object.object_index) else { continue };
        if let Ok(mut v) = visibility.get_mut(tracked.root) {
            // (the first-person unit as the game leaves it out: drawn, its
            // body would close round the camera and shade the first-person
            // weapon and hands, which sit inside it)
            let wanted = if object.flags & (OBJECT_HIDDEN | OBJECT_FIRST_PERSON) != 0 {
                Visibility::Hidden
            } else {
                Visibility::Inherited
            };
            if *v != wanted {
                *v = wanted;
            }
        }
        let nodes = snapshot.object_nodes(object);
        let mut node_matrices = Vec::with_capacity(tracked.nodes.len());
        for (index, entity) in tracked.nodes.iter().enumerate() {
            let (Some(node), Some(default_inverse)) = (nodes.get(index), tracked.model.default_inverses.get(index)) else {
                node_matrices.push(Mat4::IDENTITY);
                continue;
            };
            let matrix = node_matrix(node, default_inverse, &to_bevy, &from_bevy);
            node_matrices.push(matrix);
            if let Some(entity) = entity
                && let Ok(mut transform) = transforms.get_mut(*entity)
            {
                *transform = Transform::from_matrix(matrix);
            }
        }
        // the rays' copies of the bending parts, skinned again when the
        // nodes moved (and not for a hidden object, which the rays don't see)
        if !tracked.skins.is_empty()
            && object.flags & (OBJECT_HIDDEN | OBJECT_FIRST_PERSON) == 0
            && tracked.skinned_to != node_matrices
        {
            // (a new mesh each time: Solari queues each mesh's acceleration
            // structure for compaction once, and one changed in place every
            // frame is queued again before that is done)
            for (skin, &entity) in tracked.model.traced_skins.iter().zip(&tracked.skins) {
                timing.skins += 1;
                timing.skinned_vertices += skin.vertex_count() as u64;
                let mesh = assets.meshes.add(skin.mesh(&node_matrices));
                commands.entity(entity).insert((RaytracingMesh3d(mesh.clone()), TracedMesh(mesh)));
            }
            tracked.skinned_to = node_matrices;
        }
    }

    // the game deleted these
    objects.tracked.retain(|index, tracked| {
        let keep = seen.contains_key(index);
        if !keep {
            commands.entity(tracked.root).despawn();
        }
        keep
    });
    timing.busy += started.elapsed();
    timing.report();
}

/// `HALO_RT_FRAME_LOG`: logs every few seconds how often the viewer drew,
/// how many of the game's frames it showed, and what sync_objects cost.
#[derive(Default)]
struct SyncTiming {
    since: Option<std::time::Instant>,
    frames: u32,
    /// frames that showed the same game frame as the one before, and game
    /// frames never shown
    repeated: u32,
    skipped: u32,
    last_game_frame: u32,
    busy: Duration,
    skins: u32,
    skinned_vertices: u64,
}

impl SyncTiming {
    fn frame(&mut self, game_frame: u32) {
        self.frames += 1;
        if game_frame == self.last_game_frame {
            self.repeated += 1;
        } else if self.last_game_frame != 0 && game_frame > self.last_game_frame + 1 {
            self.skipped += game_frame - self.last_game_frame - 1;
        }
        self.last_game_frame = game_frame;
    }

    fn report(&mut self) {
        static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        if !*ON.get_or_init(|| std::env::var_os("HALO_RT_FRAME_LOG").is_some()) {
            return;
        }
        let since = *self.since.get_or_insert_with(std::time::Instant::now);
        let elapsed = since.elapsed().as_secs_f32();
        if elapsed < 3.0 {
            return;
        }
        info!(
            "frames: {:.0}/s drawn, {} repeated a game frame, {} game frames not shown; sync_objects {:.2} ms a frame; {:.0} skins ({:.0}k vertices) a second",
            self.frames as f32 / elapsed,
            self.repeated,
            self.skipped,
            self.busy.as_secs_f32() * 1000.0 / self.frames.max(1) as f32,
            self.skins as f32 / elapsed,
            self.skinned_vertices as f32 / elapsed / 1000.0
        );
        *self = SyncTiming { last_game_frame: self.last_game_frame, ..default() };
    }
}

/// Whether an object draws as camouflaged (the game's own fades in; this
/// switches halfway).
fn camouflaged(object: &halo_bridge::Object) -> bool {
    object.active_camouflage >= 0.5
}

/// A node's draw transform in Bevy space: the game's node matrix times the
/// node's default inverse (model space to world), conjugated by the Halo to
/// Bevy change of space (the meshes are already in Bevy space).
fn node_matrix(node: &halo_bridge::Node, default_inverse: &Affine3A, to_bevy: &Affine3A, from_bevy: &Affine3A) -> Mat4 {
    let world = halo_affine(&halo_map::model::Matrix4x3 {
        scale: node.scale,
        forward: node.forward,
        left: node.left,
        up: node.up,
        position: node.position,
    });
    Mat4::from(*to_bevy * world * *default_inverse * *from_bevy)
}

/// Shows only the BSP the game is in (and the placed light fixtures of it),
/// as the game draws only that one.
fn sync_active_bsp(
    link: Res<BridgeLink>,
    level: Res<BridgeLevel>,
    mut visibility: Query<&mut Visibility>,
    fixtures: Query<(Entity, &crate::level::OnBsps)>,
    mut shown: Local<Option<usize>>,
    mut reset: ResMut<crate::ResetHistory>,
) {
    let Some(snapshot) = &link.latest else { return };
    let builder = &level.builder;
    if builder.bsp_roots.len() < 2 || snapshot.structure_bsp_index < 0 {
        return;
    }
    if !snapshot.map_name().eq_ignore_ascii_case(&builder.stats.map_name) {
        return;
    }
    let active = snapshot.structure_bsp_index as usize;
    if *shown != Some(active) {
        info!("the game is in BSP {active}");
        // (the other BSP's light and surfaces are gone from the history)
        reset.0 |= shown.is_some();
        *shown = Some(active);
    }
    let mut show = |entity: Entity, shown: bool| {
        if let Ok(mut v) = visibility.get_mut(entity) {
            let wanted = if shown { Visibility::Inherited } else { Visibility::Hidden };
            if *v != wanted {
                *v = wanted;
            }
        }
    };
    for &(index, root) in &builder.bsp_roots {
        show(root, index == active);
    }
    for (entity, bsps) in &fixtures {
        show(entity, active < 32 && bsps.0 & (1 << active) != 0);
    }
}

fn sync_camera(
    link: Res<BridgeLink>,
    follow: Res<FollowGame>,
    camera: Single<(&mut Transform, &mut Projection), With<Camera3d>>,
) {
    if !follow.0 {
        return;
    }
    let Some(game) = link.latest.as_ref().and_then(|s| s.camera) else { return };
    let (mut transform, mut projection) = camera.into_inner();
    let position = Vec3::from(halo_map::math::halo_to_bevy(game.position, halo_map::math::METERS_PER_WORLD_UNIT));
    let forward = Vec3::from(halo_map::math::halo_to_bevy(game.forward, 1.0));
    let up = Vec3::from(halo_map::math::halo_to_bevy(game.up, 1.0));
    *transform = Transform::from_translation(position).looking_to(forward, up);
    if let Projection::Perspective(perspective) = &mut *projection
        && game.vertical_field_of_view > 0.0
    {
        perspective.fov = game.vertical_field_of_view;
    }
}

/// Whether this window's keyboard and mouse go to the game: while following
/// the game's camera, focused, with the cursor grabbed, or free over the
/// game's menus, which take a pointer (F12 or losing the focus lets go, a
/// click takes it back).
#[derive(Resource, Default)]
pub struct ForwardInput {
    pub active: bool,
    released: bool,
    /// the cursor over the menus, a fraction of the window
    pointer: Option<Vec2>,
    state: halo_bridge::Input,
}

/// A run condition: the viewer's own hotkeys work only while input is not
/// going to the game.
pub fn not_forwarding(forward: Option<Res<ForwardInput>>) -> bool {
    forward.is_none_or(|f| !f.active)
}

/// Keys the viewer keeps for itself even while forwarding.
const VIEWER_KEYS: [KeyCode; 3] = [KeyCode::F8, KeyCode::F10, KeyCode::F12];

/// Click to play (grab the cursor; over the game's menus, free, its place
/// forwarded), F12 to let go.
fn grab_cursor(
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    follow: Res<FollowGame>,
    window: Single<(&Window, &mut bevy::window::CursorOptions), With<bevy::window::PrimaryWindow>>,
    mut forward: ResMut<ForwardInput>,
    panel: Option<Res<crate::options::OptionsPanel>>,
    link: Res<BridgeLink>,
) {
    use bevy::window::CursorGrabMode;
    let (window, mut cursor) = window.into_inner();
    // (the options panel has the mouse while it's open)
    let panel_open = panel.is_some_and(|p| p.open);
    let attending = follow.0 && window.focused && !panel_open;
    if keys.just_pressed(KeyCode::F12) || !window.focused {
        forward.released = true;
    } else if attending && buttons.just_pressed(MouseButton::Left) {
        forward.released = false;
    }
    let playing = attending && !forward.released;
    let menus = link.bridge.menus_want_pointer();
    let grab = playing && !menus;
    if grab != (cursor.grab_mode != CursorGrabMode::None) {
        cursor.grab_mode = if grab { CursorGrabMode::Locked } else { CursorGrabMode::None };
        cursor.visible = !grab;
    }
    forward.active = playing;
    forward.pointer = (playing && menus)
        .then(|| window.cursor_position())
        .flatten()
        .map(|at| at / Vec2::new(window.width(), window.height()).max(Vec2::ONE));
}

fn forward_input(
    follow: Res<FollowGame>,
    camera: Single<&GlobalTransform, With<Camera3d>>,
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    motion: Res<bevy::input::mouse::AccumulatedMouseMotion>,
    scroll: Res<bevy::input::mouse::AccumulatedMouseScroll>,
    mut forward: ResMut<ForwardInput>,
    mut link: ResMut<BridgeLink>,
) {
    let ForwardInput { active, pointer, state, .. } = &mut *forward;
    state.active = u32::from(*active);
    state.keys = [0; 16];
    state.mouse_buttons = 0;
    state.pointer = pointer.map_or(0, |at| halo_bridge::Input::pointer_at(at.x, at.y));
    if *active {
        for key in keys.get_pressed().filter(|k| !VIEWER_KEYS.contains(k)) {
            if let Some(scancode) = scancode(*key) {
                state.set_key(scancode, true);
            }
        }
        for button in buttons.get_pressed() {
            let bit = match button {
                MouseButton::Left => 1,
                MouseButton::Middle => 2,
                MouseButton::Right => 3,
                MouseButton::Back => 4,
                MouseButton::Forward => 5,
                MouseButton::Other(n) if *n < 32 => *n as u32,
                MouseButton::Other(_) => continue,
            };
            state.mouse_buttons |= 1 << bit;
        }
        state.mouse_x = state.mouse_x.wrapping_add(motion.delta.x.round() as i32);
        state.mouse_y = state.mouse_y.wrapping_add(motion.delta.y.round() as i32);
        let steps = match scroll.unit {
            bevy::input::mouse::MouseScrollUnit::Line => scroll.delta.y,
            bevy::input::mouse::MouseScrollUnit::Pixel => scroll.delta.y / 100.0,
        };
        state.wheel = state.wheel.wrapping_add(steps.round() as i32);
    }
    // flying free, the game hears from this camera
    state.listener_valid = u32::from(!follow.0);
    if !follow.0 {
        let to_halo = |v: Vec3, scale: f32| [v.x / scale, -v.z / scale, v.y / scale];
        let scale = halo_map::math::METERS_PER_WORLD_UNIT;
        state.listener_position = to_halo(camera.translation(), scale);
        state.listener_forward = to_halo(*camera.forward(), 1.0);
        state.listener_up = to_halo(*camera.up(), 1.0);
    }
    link.bridge.write_input(state);
}

/// Forwards the gamepads to the game while it has this window's attention
/// (focused, following the game's camera, the options closed), and plays
/// the game's rumble on them.
fn forward_gamepads(
    gamepads: Query<(Entity, &bevy::input::gamepad::Gamepad)>,
    follow: Res<FollowGame>,
    window: Single<&Window, With<bevy::window::PrimaryWindow>>,
    panel: Option<Res<crate::options::OptionsPanel>>,
    mut link: ResMut<BridgeLink>,
    mut rumble_requests: MessageWriter<bevy::input::gamepad::GamepadRumbleRequest>,
    mut playing: Local<Vec<(Entity, [u16; 2])>>,
    time: Res<Time>,
    mut since_refresh: Local<f32>,
) {
    use bevy::input::gamepad::{GamepadAxis, GamepadButton, GamepadRumbleIntensity, GamepadRumbleRequest};
    use halo_bridge::sdl_button as b;
    const BUTTONS: [(GamepadButton, u32); 15] = [
        (GamepadButton::South, b::SOUTH),
        (GamepadButton::East, b::EAST),
        (GamepadButton::West, b::WEST),
        (GamepadButton::North, b::NORTH),
        (GamepadButton::Select, b::BACK),
        (GamepadButton::Mode, b::GUIDE),
        (GamepadButton::Start, b::START),
        (GamepadButton::LeftThumb, b::LEFT_STICK),
        (GamepadButton::RightThumb, b::RIGHT_STICK),
        (GamepadButton::LeftTrigger, b::LEFT_SHOULDER),
        (GamepadButton::RightTrigger, b::RIGHT_SHOULDER),
        (GamepadButton::DPadUp, b::DPAD_UP),
        (GamepadButton::DPadDown, b::DPAD_DOWN),
        (GamepadButton::DPadLeft, b::DPAD_LEFT),
        (GamepadButton::DPadRight, b::DPAD_RIGHT),
    ];
    let attending = window.focused && follow.0 && panel.is_none_or(|p| !p.open);
    let mut pads = Vec::new();
    let mut entities = Vec::new();
    if attending {
        for (entity, gamepad) in gamepads.iter().take(halo_bridge::GAMEPADS) {
            let mut pad = halo_bridge::Gamepad::default();
            for (button, bit) in BUTTONS {
                if gamepad.pressed(button) {
                    pad.buttons |= 1 << bit;
                }
            }
            let stick = |v: Option<f32>| (v.unwrap_or(0.0).clamp(-1.0, 1.0) * 32767.0).round() as i16;
            let trigger = |v: Option<f32>| (v.unwrap_or(0.0).clamp(0.0, 1.0) * 32767.0).round() as i16;
            // (SDL's y axes point down, Bevy's up)
            pad.axes = [
                stick(gamepad.get(GamepadAxis::LeftStickX)),
                stick(gamepad.get(GamepadAxis::LeftStickY).map(|v| -v)),
                stick(gamepad.get(GamepadAxis::RightStickX)),
                stick(gamepad.get(GamepadAxis::RightStickY).map(|v| -v)),
                trigger(gamepad.get(GamepadButton::LeftTrigger2)),
                trigger(gamepad.get(GamepadButton::RightTrigger2)),
            ];
            pads.push(pad);
            entities.push(entity);
        }
    }
    link.bridge.write_gamepads(&pads);

    // the game's rumble: started or stopped as it changes, and renewed
    // while it lasts (each request plays for a second)
    *since_refresh += time.delta_secs();
    let refresh = *since_refresh > 0.5;
    if refresh {
        *since_refresh = 0.0;
    }
    let rumble = link.bridge.rumble();
    let mut now = Vec::new();
    for (index, &entity) in entities.iter().enumerate() {
        let motors = rumble[index];
        let before = playing.iter().find(|(e, _)| *e == entity).map(|(_, m)| *m).unwrap_or([0, 0]);
        if motors != before || (refresh && motors != [0, 0]) {
            rumble_requests.write(GamepadRumbleRequest::Stop { gamepad: entity });
            if motors != [0, 0] {
                rumble_requests.write(GamepadRumbleRequest::Add {
                    gamepad: entity,
                    duration: Duration::from_secs(1),
                    intensity: GamepadRumbleIntensity {
                        strong_motor: motors[0] as f32 / 65535.0,
                        weak_motor: motors[1] as f32 / 65535.0,
                    },
                });
            }
        }
        now.push((entity, motors));
    }
    // (pads no longer forwarded stop)
    for (entity, motors) in playing.iter() {
        if *motors != [0, 0] && !entities.contains(entity) {
            rumble_requests.write(GamepadRumbleRequest::Stop { gamepad: *entity });
        }
    }
    *playing = now;
}

/// A key's SDL scancode: its USB HID keyboard usage.
fn scancode(key: KeyCode) -> Option<u32> {
    use KeyCode::*;
    Some(match key {
        KeyA => 4, KeyB => 5, KeyC => 6, KeyD => 7, KeyE => 8, KeyF => 9, KeyG => 10, KeyH => 11, KeyI => 12,
        KeyJ => 13, KeyK => 14, KeyL => 15, KeyM => 16, KeyN => 17, KeyO => 18, KeyP => 19, KeyQ => 20, KeyR => 21,
        KeyS => 22, KeyT => 23, KeyU => 24, KeyV => 25, KeyW => 26, KeyX => 27, KeyY => 28, KeyZ => 29,
        Digit1 => 30, Digit2 => 31, Digit3 => 32, Digit4 => 33, Digit5 => 34, Digit6 => 35, Digit7 => 36,
        Digit8 => 37, Digit9 => 38, Digit0 => 39,
        Enter => 40, Escape => 41, Backspace => 42, Tab => 43, Space => 44, Minus => 45, Equal => 46,
        BracketLeft => 47, BracketRight => 48, Backslash => 49, Semicolon => 51, Quote => 52, Backquote => 53,
        Comma => 54, Period => 55, Slash => 56, CapsLock => 57,
        F1 => 58, F2 => 59, F3 => 60, F4 => 61, F5 => 62, F6 => 63, F7 => 64, F8 => 65, F9 => 66, F10 => 67,
        F11 => 68, F12 => 69,
        PrintScreen => 70, ScrollLock => 71, Pause => 72, Insert => 73, Home => 74, PageUp => 75, Delete => 76,
        End => 77, PageDown => 78, ArrowRight => 79, ArrowLeft => 80, ArrowDown => 81, ArrowUp => 82,
        NumLock => 83, NumpadDivide => 84, NumpadMultiply => 85, NumpadSubtract => 86, NumpadAdd => 87,
        NumpadEnter => 88, Numpad1 => 89, Numpad2 => 90, Numpad3 => 91, Numpad4 => 92, Numpad5 => 93,
        Numpad6 => 94, Numpad7 => 95, Numpad8 => 96, Numpad9 => 97, Numpad0 => 98, NumpadDecimal => 99,
        ControlLeft => 224, ShiftLeft => 225, AltLeft => 226, SuperLeft => 227,
        ControlRight => 228, ShiftRight => 229, AltRight => 230, SuperRight => 231,
        _ => return None,
    })
}
