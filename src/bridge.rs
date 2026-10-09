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

use crate::level::{halo_affine, halo_to_bevy_affine, AnimatedModel, LevelAssets, LevelBuilder};
use crate::material::{HaloEnvironmentMaterial, HaloTransparentMaterial, NeutralDetail};
use bevy::{
    camera_controller::free_camera::FreeCamera,
    math::Affine3A,
    platform::collections::HashMap,
    prelude::*,
};
use halo_bridge::{Bridge, Snapshot, OBJECT_HIDDEN, OBJECT_MODEL};
use std::{sync::Arc, time::Duration};

/// Starts OpenCE on the map at `map_path`: its data root is the folder that
/// holds the maps folder, the level is the map's scenario, the bridge is on
/// and (unless `show`) its window is hidden. With `overlay`, it shares its
/// overlay (`crate::overlay`), drawing at the viewer's window size. Its output
/// goes to `halo_rt_opence-<viewer process id>.log` in the temporary folder.
pub fn launch_game(
    exe: &std::path::Path,
    map_path: &std::path::Path,
    start_on_map: bool,
    bridge_name: &str,
    show: bool,
    overlay: bool,
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
            .add_systems(PreUpdate, (grab_cursor, forward_input).chain().after(bevy::input::InputSystems))
            .add_systems(Update, (toggle_follow, sync_objects, sync_camera).chain());
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
    /// frames before it are dropped.
    pub fn show_frame(&mut self, frame_index: u32) {
        let Some(position) = self.pending.iter().position(|s| s.frame_index == frame_index) else { return };
        self.pending.drain(..position);
        self.latest = self.pending.pop_front();
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
    model: Arc<AnimatedModel>,
    nodes: Vec<Option<Entity>>,
}

/// The game's objects drawn so far, by object datum index.
#[derive(Resource, Default)]
pub struct BridgeObjects {
    tracked: HashMap<u32, Tracked>,
    /// Definitions without a model (nothing to draw).
    modelless: HashMap<(u32, [u8; 8]), ()>,
}

impl BridgeObjects {
    pub fn count(&self) -> usize {
        self.tracked.len()
    }

    /// Forgets every object (their entities went with the level).
    pub fn clear(&mut self) {
        self.tracked.clear();
        self.modelless.clear();
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
    neutral_detail: Res<NeutralDetail>,
    mut flat_normal: Local<Option<Handle<Image>>>,
) {
    let Some(snapshot) = &link.latest else { return };
    let flat_normal = flat_normal.get_or_insert_with(|| images.add(crate::material::flat_normal_image())).clone();
    let to_bevy = halo_to_bevy_affine();
    let from_bevy = to_bevy.inverse();
    let assets = &mut LevelAssets {
        meshes: &mut meshes,
        materials: &mut materials,
        environment_materials: &mut environment_materials,
        transparent_materials: &mut transparent_materials,
        images: &mut images,
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

    let mut seen = HashMap::<u32, ()>::default();
    for object in &snapshot.objects {
        seen.insert(object.object_index, ());
        // an object whose definition or permutations changed is made again
        let current = objects.tracked.get(&object.object_index);
        if current.is_some_and(|t| t.definition != object.definition_index || t.permutations != object.region_permutations) {
            let old = objects.tracked.remove(&object.object_index).unwrap();
            commands.entity(old.root).despawn();
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
            let (root, nodes) = builder.spawn_animated_model(&mut commands, &model, raster);
            objects.tracked.insert(
                object.object_index,
                Tracked {
                    root,
                    definition: object.definition_index,
                    permutations: object.region_permutations,
                    model,
                    nodes,
                },
            );
            // newly spawned entities take their transforms next frame
            continue;
        }
        let tracked = &objects.tracked[&object.object_index];
        if let Ok(mut v) = visibility.get_mut(tracked.root) {
            let wanted = if object.flags & OBJECT_HIDDEN != 0 { Visibility::Hidden } else { Visibility::Inherited };
            if *v != wanted {
                *v = wanted;
            }
        }
        let nodes = snapshot.object_nodes(object);
        for (index, entity) in tracked.nodes.iter().enumerate() {
            let (Some(entity), Some(node), Some(default_inverse)) =
                (entity, nodes.get(index), tracked.model.default_inverses.get(index))
            else {
                continue;
            };
            if let Ok(mut transform) = transforms.get_mut(*entity) {
                *transform = node_transform(node, default_inverse, &to_bevy, &from_bevy);
            }
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
}

/// A node's draw transform in Bevy space: the game's node matrix times the
/// node's default inverse (model space to world), conjugated by the Halo to
/// Bevy change of space (the meshes are already in Bevy space).
fn node_transform(node: &halo_bridge::Node, default_inverse: &Affine3A, to_bevy: &Affine3A, from_bevy: &Affine3A) -> Transform {
    let world = halo_affine(&halo_map::model::Matrix4x3 {
        scale: node.scale,
        forward: node.forward,
        left: node.left,
        up: node.up,
        position: node.position,
    });
    Transform::from_matrix(Mat4::from(*to_bevy * world * *default_inverse * *from_bevy))
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
/// the game's camera with the cursor grabbed (click to grab, F12 releases).
#[derive(Resource, Default)]
pub struct ForwardInput {
    pub active: bool,
    state: halo_bridge::Input,
}

/// A run condition: the viewer's own hotkeys work only while input is not
/// going to the game.
pub fn not_forwarding(forward: Option<Res<ForwardInput>>) -> bool {
    forward.is_none_or(|f| !f.active)
}

/// Keys the viewer keeps for itself even while forwarding.
const VIEWER_KEYS: [KeyCode; 2] = [KeyCode::F8, KeyCode::F12];

/// Click to play (grab the cursor), F12 to let go.
fn grab_cursor(
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    follow: Res<FollowGame>,
    window: Single<(&Window, &mut bevy::window::CursorOptions), With<bevy::window::PrimaryWindow>>,
    mut forward: ResMut<ForwardInput>,
) {
    use bevy::window::CursorGrabMode;
    let (window, mut cursor) = window.into_inner();
    let grabbed = cursor.grab_mode != CursorGrabMode::None;
    let release = keys.just_pressed(KeyCode::F12) || !window.focused || !follow.0;
    if grabbed && release {
        cursor.grab_mode = CursorGrabMode::None;
        cursor.visible = true;
    } else if !grabbed && follow.0 && window.focused && buttons.just_pressed(MouseButton::Left) {
        cursor.grab_mode = CursorGrabMode::Locked;
        cursor.visible = false;
    }
    forward.active = follow.0 && window.focused && cursor.grab_mode != CursorGrabMode::None;
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
    let ForwardInput { active, state } = &mut *forward;
    state.active = u32::from(*active);
    state.keys = [0; 16];
    state.mouse_buttons = 0;
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
