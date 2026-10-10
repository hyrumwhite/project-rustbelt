//! The reading side of OpenCE's Bevy bridge.
//!
//! OpenCE (branch `bevy-bridge`, `port/linux/game/bevy_bridge.c`) writes each
//! rendered frame's camera and every object's node matrices into shared memory,
//! laid out as `bevy_bridge_protocol.h`. This crate mirrors that layout
//! (`#[repr(C)]`, fixed-size types, the same in the 32-bit game and a 64-bit
//! reader) and copies out consistent snapshots: two frame buffers, each a
//! sequence lock.
//!
//! Positions and matrices are the game's: world units, Z up.
//!
//! The other way, [`Bridge::write_input`] forwards a window's keyboard and
//! mouse for the game to read as its own (the game's
//! `port/linux/src/bevy_bridge_input.c` turns them into SDL events).
//!
//! With the game's `game.bevy_bridge_overlay` on, it also shares its own
//! picture without the opaque world ([`Bridge::read_overlay`]): effects,
//! transparent surfaces, the HUD and menus, premultiplied, to draw over the
//! renderer's picture.

use std::sync::atomic::{fence, Ordering};

pub const MAGIC: u32 = 0x4252_4248; // "HBRB"
pub const VERSION: u32 = 16;
pub const PROJECTILE_RECORDS: usize = 1024;
pub const RANDOM_DIRECTIONS: usize = 4096;
/// The steps of each ray test's point physics.
pub const POINT_PHYSICS_STEPS: usize = 90;
pub const RAY_TESTS: usize = 16;
pub const MAXIMUM_LIGHTS: usize = 64;
pub const MAXIMUM_OBJECTS: usize = 4096;
pub const MAXIMUM_NODES: usize = 49152;
const FRAMES: usize = 2;
/// The largest overlay picture, in pixels.
pub const OVERLAY_MAXIMUM_PIXELS: usize = 2560 * 1440;
const OVERLAYS: usize = 2;
/// The game's default bridge name (`game.bevy_bridge_name`). Each game
/// shares under its own name, so several can run side by side.
pub const DEFAULT_NAME: &str = "HaloBevyBridge";

pub const OBJECT_HIDDEN: u32 = 1 << 0;
pub const OBJECT_FIRST_PERSON: u32 = 1 << 1;
/// Not an object but a model the game draws by itself (the first-person
/// weapon and hands): [`Object::definition_index`] is the model tag.
pub const OBJECT_MODEL: u32 = 1 << 2;

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct Camera {
    pub position: [f32; 3],
    pub forward: [f32; 3],
    pub up: [f32; 3],
    /// Radians.
    pub vertical_field_of_view: f32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct Object {
    /// The object's datum index: stable while the object lives.
    pub object_index: u32,
    /// Its definition tag's datum index.
    pub definition_index: u32,
    pub first_node: u32,
    pub node_count: u16,
    pub kind: i16,
    pub flags: u32,
    pub region_permutations: [u8; 8],
    /// The object's change colours A-D (team and armour colours); white for
    /// a model.
    pub change_colors: [[f32; 3]; 4],
    /// How camouflaged it is (0..1): a unit's active camouflage, shared by
    /// what it holds.
    pub active_camouflage: f32,
    /// The collision broad phase's state: the bounding sphere (as of the
    /// last tick), the object's flags (bit 0 invisible, 11 connected to the
    /// map, 25 with a collision model), its parent (`u32::MAX` for none) and
    /// its location's cluster (-1 outside the map).
    pub bounding_center: [f32; 3],
    pub bounding_radius: f32,
    pub object_flags: u32,
    pub parent_index: u32,
    pub location_cluster: i16,
    pub pad: i16,
}

/// `real_matrix4x3`: a node's world transform.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct Node {
    pub scale: f32,
    pub forward: [f32; 3],
    pub left: [f32; 3],
    pub up: [f32; 3],
    pub position: [f32; 3],
}

/// A dynamic light (a muzzle flash, plasma, an explosion, the flashlight),
/// as the game lit the frame before.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct Light {
    pub position: [f32; 3],
    pub forward: [f32; 3],
    /// 0..1 a channel.
    pub color: [f32; 3],
    /// How far it reaches (world units).
    pub radius: f32,
    /// A spotlight's cutoff angle (radians); pi or more shines every way.
    pub cutoff_angle: f32,
}

/// A ray the game tested against its structure's collision geometry
/// (front-facing surfaces), with `game.bevy_bridge_ray_tests` on.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct RayTest {
    pub point: [f32; 3],
    pub vector: [f32; 3],
    pub hit: u32,
    /// How far along `vector` (0..1).
    pub t: f32,
    pub surface_index: i32,
    /// A sphere tested half its radius short of the hit: what it touched
    /// (surfaces, edges, vertices, leaves) and an FNV-1a checksum of their
    /// indices in the order found.
    pub sphere_center: [f32; 3],
    pub sphere_radius: f32,
    pub sphere_counts: [u16; 4],
    pub sphere_checksum: u32,
    /// A sphere swept from the ray's start along `pill_vector` and where it
    /// first touched (`collision_bsp_test_pill`).
    pub pill_vector: [f32; 3],
    pub pill_radius: f32,
    pub pill_hit: u32,
    pub pill_t: f32,
    pub pill_normal: [f32; 3],
    pub pill_surface_index: i32,
    /// The game's whole ray test (`collision_test_vector`, the structure and
    /// every object type): the object it hit (`u32::MAX` for the structure
    /// or nothing; `object_surface_index` -1 for nothing), the collision
    /// node, bsp and surface, and how far along `vector`.
    pub object_index: u32,
    pub object_node_index: i16,
    pub object_bsp_index: i16,
    pub object_t: f32,
    pub object_surface_index: i32,
    /// A point launched from the ray's start along it at 15 world units a
    /// second (`point_physics_update`, no wind, under water if
    /// `point_underwater`): where it was after [`POINT_PHYSICS_STEPS`]
    /// steps of 1/30 s, its velocity, the steps' result flags or'd, and how
    /// many steps collided.
    pub point_radius: f32,
    pub point_underwater: u32,
    pub point_position: [f32; 3],
    pub point_velocity: [f32; 3],
    pub point_result_flags: u32,
    pub point_collisions: u32,
}

/// FNV-1a over 32-bit indices, as the game checksums a sphere test's.
pub fn feature_checksum(mut hash: u32, indices: &[i32]) -> u32 {
    for &index in indices {
        for byte in (index as u32).to_le_bytes() {
            hash ^= byte as u32;
            hash = hash.wrapping_mul(16_777_619);
        }
    }
    hash
}

#[repr(C)]
struct Frame {
    sequence: u32,
    frame_index: u32,
    game_tick: u32,
    structure_bsp_index: i32,
    scenario_name: [u8; 64],
    camera_valid: u32,
    camera: Camera,
    light_count: u32,
    lights: [Light; MAXIMUM_LIGHTS],
    ray_test_count: u32,
    ray_tests: [RayTest; RAY_TESTS],
    point_physics_definition: u32,
    object_count: u32,
    node_count: u32,
    objects: [Object; MAXIMUM_OBJECTS],
    nodes: [Node; MAXIMUM_NODES],
}

/// What a renderer's window receives, for the game to read as its own
/// (`struct bevy_bridge_input`).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct Input {
    /// Nonzero while input is forwarded.
    pub active: u32,
    /// The keys held, a bit per SDL scancode (USB HID usage).
    pub keys: [u32; 16],
    /// Cumulative relative mouse motion (pixels) and wheel steps.
    pub mouse_x: i32,
    pub mouse_y: i32,
    pub wheel: i32,
    /// The mouse buttons held, a bit per SDL button number (1 left, 2
    /// middle, 3 right, 4 and 5 the side buttons).
    pub mouse_buttons: u32,
    /// Nonzero while the renderer flies its own camera: the first player
    /// hears from it. The game's coordinates (world units, Z up).
    pub listener_valid: u32,
    pub listener_position: [f32; 3],
    pub listener_forward: [f32; 3],
    pub listener_up: [f32; 3],
    pub reserved: [u32; 1],
}

impl Input {
    pub fn set_key(&mut self, scancode: u32, down: bool) {
        if scancode < 512 {
            let (word, bit) = ((scancode / 32) as usize, scancode % 32);
            if down {
                self.keys[word] |= 1 << bit;
            } else {
                self.keys[word] &= !(1 << bit);
            }
        }
    }
}

/// A gamepad forwarded to the game (`struct bevy_bridge_gamepad`), in SDL's
/// terms: a bit per `SDL_GamepadButton`, axes in `SDL_GamepadAxis` order
/// and ranges (sticks -32768..32767 with y down, triggers 0..32767).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Gamepad {
    pub buttons: u32,
    pub axes: [i16; 6],
    /// The game's rumble (low and high frequency motors, 0..65535); only the
    /// game writes it.
    pub rumble: [u16; 2],
}

pub const GAMEPADS: usize = 4;

/// `SDL_GamepadButton`.
pub mod sdl_button {
    pub const SOUTH: u32 = 0;
    pub const EAST: u32 = 1;
    pub const WEST: u32 = 2;
    pub const NORTH: u32 = 3;
    pub const BACK: u32 = 4;
    pub const GUIDE: u32 = 5;
    pub const START: u32 = 6;
    pub const LEFT_STICK: u32 = 7;
    pub const RIGHT_STICK: u32 = 8;
    pub const LEFT_SHOULDER: u32 = 9;
    pub const RIGHT_SHOULDER: u32 = 10;
    pub const DPAD_UP: u32 = 11;
    pub const DPAD_DOWN: u32 = 12;
    pub const DPAD_LEFT: u32 = 13;
    pub const DPAD_RIGHT: u32 = 14;
}

#[repr(C)]
struct Gamepads {
    active: u32,
    count: u32,
    pads: [Gamepad; GAMEPADS],
}

/// A projectile's state, as `projectile_update` reads and leaves it.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ProjectileState {
    pub position: [f32; 3],
    pub velocity: [f32; 3],
    pub projectile_flags: u32,
    pub object_flags: u32,
    pub action: i16,
    pub hit_material_type: i16,
    pub ignore_object_index: i32,
    pub target_object_index: i32,
    pub parent_object_index: i32,
    pub detonation_timer: f32,
    pub detonation_timer_delta: f32,
    pub arming_time: f32,
    pub arming_time_delta: f32,
    pub odometer: f32,
    pub deceleration_timer: f32,
    pub deceleration_timer_delta: f32,
    pub deceleration: f32,
    pub maximum_damage_distance: f32,
    pub location_leaf: i32,
    pub location_cluster: i16,
    pub pad: i16,
}

/// One `projectile_update` the game recorded (with
/// `game.bevy_bridge_ray_tests` on): the state before and after, the
/// global random seed as it began and as each collision response began,
/// and whether a collision touched an object.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ProjectileRecord {
    pub sequence: u32,
    pub projectile_index: u32,
    pub definition_index: u32,
    pub structure_bsp_index: i32,
    pub random_seed: u32,
    pub collision_count: u32,
    pub collision_seeds: [u32; 4],
    pub touched_object: u32,
    pub before: ProjectileState,
    pub after: ProjectileState,
}

const _: () = assert!(std::mem::size_of::<ProjectileState>() == 92);
const _: () = assert!(std::mem::size_of::<ProjectileRecord>() == 228);

#[repr(C)]
struct OverlayHeader {
    sequence: u32,
    frame_index: u32,
    width: u32,
    height: u32,
    reserved: [u32; 4],
}

#[repr(C)]
struct Shared {
    magic: u32,
    version: u32,
    size: u32,
    latest_frame: u32,
    writer_process_id: u32,
    reserved: [u32; 3],
    input: Input,
    gamepads: Gamepads,
    frames: [Frame; FRAMES],
    overlay_enabled: u32,
    latest_overlay: u32,
    overlay_reserved: [u32; 2],
    overlays: [OverlayHeader; OVERLAYS],
    projectile_record_count: u32,
    random_direction_count: u32,
    projectile_records: [ProjectileRecord; PROJECTILE_RECORDS],
    random_directions: [[f32; 3]; RANDOM_DIRECTIONS],
    overlay_pixels: [[u8; OVERLAY_MAXIMUM_PIXELS * 4]; OVERLAYS],
}

/// The layout is the game's, measured there; keep these in step with
/// `bevy_bridge_protocol.h`.
const _: () = assert!(std::mem::size_of::<Object>() == 108);
const _: () = assert!(std::mem::size_of::<Node>() == 52);
const _: () = assert!(std::mem::size_of::<Camera>() == 40);
const _: () = assert!(std::mem::size_of::<Input>() == 128);
const _: () = assert!(std::mem::size_of::<Gamepad>() == 20);

/// One frame of the game's world.
#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    pub frame_index: u32,
    pub game_tick: u32,
    pub structure_bsp_index: i32,
    /// The scenario tag's name, e.g. `levels\b30\b30`.
    pub scenario_name: String,
    pub camera: Option<Camera>,
    pub lights: Vec<Light>,
    pub ray_tests: Vec<RayTest>,
    /// The point physics definition the ray tests' points use (`u32::MAX`
    /// for none).
    pub point_physics_definition: u32,
    pub objects: Vec<Object>,
    pub nodes: Vec<Node>,
}

impl Snapshot {
    /// The map file's name: the scenario name's last part.
    pub fn map_name(&self) -> &str {
        self.scenario_name.rsplit('\\').next().unwrap_or("")
    }

    pub fn object_nodes(&self, object: &Object) -> &[Node] {
        let first = object.first_node as usize;
        self.nodes.get(first..first + object.node_count as usize).unwrap_or(&[])
    }
}

/// An overlay picture's size and the frame it shows; its pixels are RGBA,
/// 8 bits a channel, sRGB, premultiplied, the top row first.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Overlay {
    pub frame_index: u32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug)]
pub enum Error {
    /// No game is sharing frames (not running, or game.bevy_bridge is off).
    NotRunning,
    Incompatible(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::NotRunning => write!(f, "no game is sharing its frames (start OpenCE with HALO_BEVY_BRIDGE=1)"),
            Error::Incompatible(s) => write!(f, "incompatible bridge: {s}"),
        }
    }
}

impl std::error::Error for Error {}

/// The shared memory: frames read, input written.
pub struct Bridge {
    shared: *const Shared,
    #[cfg(windows)]
    _mapping: windows::Handle,
    last_frame_index: u32,
    last_overlay: Option<(u32, u32)>,
    last_projectile_record: Option<u32>,
}

// Frames are read through volatile reads and a sequence check; the input
// block is only written by this side.
unsafe impl Send for Bridge {}
unsafe impl Sync for Bridge {}

impl Bridge {
    /// Opens the shared memory of the game using the default name.
    pub fn open() -> Result<Self, Error> {
        Self::open_named(DEFAULT_NAME)
    }

    /// Opens the shared memory of the game started with
    /// `HALO_BEVY_BRIDGE_NAME=name`.
    pub fn open_named(name: &str) -> Result<Self, Error> {
        #[cfg(windows)]
        {
            let mapping_name = format!("Local\\{name}\0");
            let (mapping, view) = windows::open(mapping_name.as_bytes(), std::mem::size_of::<Shared>())?;
            let bridge = Bridge { shared: view as *const Shared, _mapping: mapping, last_frame_index: 0, last_overlay: None, last_projectile_record: None };
            bridge.check()?;
            Ok(bridge)
        }
        #[cfg(not(windows))]
        {
            let _ = name;
            Err(Error::NotRunning)
        }
    }

    fn check(&self) -> Result<(), Error> {
        // SAFETY: the view is at least size_of::<Shared>() bytes (mapped so)
        let (magic, version, size) = unsafe {
            (
                std::ptr::read_volatile(&(*self.shared).magic),
                std::ptr::read_volatile(&(*self.shared).version),
                std::ptr::read_volatile(&(*self.shared).size),
            )
        };
        if magic != MAGIC {
            return Err(Error::NotRunning);
        }
        if version != VERSION || size as usize != std::mem::size_of::<Shared>() {
            return Err(Error::Incompatible(format!(
                "version {version} size {size}; this reader is version {VERSION} size {}",
                std::mem::size_of::<Shared>()
            )));
        }
        Ok(())
    }

    /// Forwards input to the game (see [`Input`]).
    pub fn write_input(&mut self, input: &Input) {
        // SAFETY: the view is writable and at least size_of::<Shared>()
        // bytes; the game only reads this block
        unsafe {
            let target = &raw const (*self.shared).input as *mut Input;
            std::ptr::write_volatile(target, *input);
        }
        fence(Ordering::Release);
    }

    /// Forwards gamepads to the game, which then takes them in place of its
    /// own (the first shares port 0 with the keyboard). Their rumble fields
    /// are the game's and are left alone.
    pub fn write_gamepads(&mut self, pads: &[Gamepad]) {
        let count = pads.len().min(GAMEPADS);
        // SAFETY: the view is writable and at least size_of::<Shared>()
        // bytes; the game only reads these fields
        unsafe {
            let block = &raw const (*self.shared).gamepads as *mut Gamepads;
            for (index, pad) in pads.iter().take(count).enumerate() {
                let target = &raw mut (*block).pads[index];
                std::ptr::write_volatile(&raw mut (*target).buttons, pad.buttons);
                std::ptr::write_volatile(&raw mut (*target).axes, pad.axes);
            }
            for index in count..GAMEPADS {
                let target = &raw mut (*block).pads[index];
                std::ptr::write_volatile(&raw mut (*target).buttons, 0);
                std::ptr::write_volatile(&raw mut (*target).axes, [0; 6]);
            }
            std::ptr::write_volatile(&raw mut (*block).count, count as u32);
            fence(Ordering::Release);
            std::ptr::write_volatile(&raw mut (*block).active, 1);
        }
    }

    /// The game's rumble for each gamepad (low, high frequency motor).
    pub fn rumble(&self) -> [[u16; 2]; GAMEPADS] {
        // SAFETY: as in check()
        unsafe { std::array::from_fn(|i| std::ptr::read_volatile(&(*self.shared).gamepads.pads[i].rumble)) }
    }

    /// The projectile updates the game recorded since the last call (the
    /// first call starts from now). Records the ring overwrote meanwhile
    /// are skipped.
    pub fn read_projectile_records(&mut self) -> Vec<ProjectileRecord> {
        // SAFETY: as in check(); records are plain old data, checked by their
        // sequence number after copying
        unsafe {
            let count = std::ptr::read_volatile(&(*self.shared).projectile_record_count);
            fence(Ordering::Acquire);
            let first = match self.last_projectile_record {
                None => count,
                Some(last) => last.max(count.saturating_sub(PROJECTILE_RECORDS as u32 / 2)),
            };
            let mut out = Vec::new();
            for sequence in first..count {
                let record = std::ptr::read_volatile(&(*self.shared).projectile_records[sequence as usize % PROJECTILE_RECORDS]);
                if record.sequence == sequence {
                    out.push(record);
                }
            }
            self.last_projectile_record = Some(count);
            out
        }
    }

    /// The game's random direction table (`random_vector_in_cone3d`), once
    /// a projectile has been recorded.
    pub fn random_directions(&self) -> Vec<[f32; 3]> {
        // SAFETY: as in check()
        unsafe {
            let count = (std::ptr::read_volatile(&(*self.shared).random_direction_count) as usize).min(RANDOM_DIRECTIONS);
            fence(Ordering::Acquire);
            (0..count).map(|i| std::ptr::read_volatile(&(*self.shared).random_directions[i])).collect()
        }
    }

    /// Whether the game shares overlays.
    pub fn overlay_enabled(&self) -> bool {
        // SAFETY: as in check()
        unsafe { std::ptr::read_volatile(&(*self.shared).overlay_enabled) != 0 }
    }

    /// The latest overlay, copied into `pixels`, if it is newer than the last
    /// one returned.
    pub fn read_overlay(&mut self, pixels: &mut Vec<u8>) -> Option<Overlay> {
        for _ in 0..4 {
            // SAFETY: as in read(); the pixels are copied as bytes and
            // validated by the sequence afterwards
            unsafe {
                let latest = std::ptr::read_volatile(&(*self.shared).latest_overlay) as usize % OVERLAYS;
                fence(Ordering::Acquire);
                let header = &(*self.shared).overlays[latest] as *const OverlayHeader;
                let sequence = std::ptr::read_volatile(&(*header).sequence);
                if sequence & 1 != 0 || sequence == 0 {
                    continue;
                }
                fence(Ordering::Acquire);
                if self.last_overlay == Some((latest as u32, sequence)) {
                    return None;
                }
                let overlay = Overlay {
                    frame_index: std::ptr::read_volatile(&(*header).frame_index),
                    width: std::ptr::read_volatile(&(*header).width),
                    height: std::ptr::read_volatile(&(*header).height),
                };
                let bytes = overlay.width as usize * overlay.height as usize * 4;
                if bytes == 0 || bytes > OVERLAY_MAXIMUM_PIXELS * 4 {
                    return None;
                }
                pixels.resize(bytes, 0);
                std::ptr::copy_nonoverlapping((*self.shared).overlay_pixels[latest].as_ptr(), pixels.as_mut_ptr(), bytes);
                fence(Ordering::Acquire);
                if std::ptr::read_volatile(&(*header).sequence) != sequence {
                    continue;
                }
                self.last_overlay = Some((latest as u32, sequence));
                return Some(overlay);
            }
        }
        None
    }

    /// The latest frame, if it is newer than the last one returned.
    pub fn read(&mut self) -> Option<Snapshot> {
        for _ in 0..8 {
            // SAFETY: as in check(); every field read is plain old data, read
            // volatile and validated by the sequence afterwards
            unsafe {
                let latest = std::ptr::read_volatile(&(*self.shared).latest_frame) as usize % FRAMES;
                fence(Ordering::Acquire);
                let frame = &(*self.shared).frames[latest] as *const Frame;
                let sequence = std::ptr::read_volatile(&(*frame).sequence);
                if sequence & 1 != 0 {
                    continue;
                }
                fence(Ordering::Acquire);
                let frame_index = std::ptr::read_volatile(&(*frame).frame_index);
                if frame_index == self.last_frame_index {
                    return None;
                }
                let object_count = (std::ptr::read_volatile(&(*frame).object_count) as usize).min(MAXIMUM_OBJECTS);
                let light_count = (std::ptr::read_volatile(&(*frame).light_count) as usize).min(MAXIMUM_LIGHTS);
                let ray_test_count = (std::ptr::read_volatile(&(*frame).ray_test_count) as usize).min(RAY_TESTS);
                let node_count = (std::ptr::read_volatile(&(*frame).node_count) as usize).min(MAXIMUM_NODES);
                let name = std::ptr::read_volatile(&(*frame).scenario_name);
                let end = name.iter().position(|&b| b == 0).unwrap_or(name.len());
                let snapshot = Snapshot {
                    frame_index,
                    game_tick: std::ptr::read_volatile(&(*frame).game_tick),
                    structure_bsp_index: std::ptr::read_volatile(&(*frame).structure_bsp_index),
                    scenario_name: String::from_utf8_lossy(&name[..end]).into_owned(),
                    camera: (std::ptr::read_volatile(&(*frame).camera_valid) != 0)
                        .then(|| std::ptr::read_volatile(&(*frame).camera)),
                    lights: (0..light_count).map(|i| std::ptr::read_volatile(&(*frame).lights[i])).collect(),
                    ray_tests: (0..ray_test_count).map(|i| std::ptr::read_volatile(&(*frame).ray_tests[i])).collect(),
                    point_physics_definition: std::ptr::read_volatile(&(*frame).point_physics_definition),
                    objects: (0..object_count).map(|i| std::ptr::read_volatile(&(*frame).objects[i])).collect(),
                    nodes: (0..node_count).map(|i| std::ptr::read_volatile(&(*frame).nodes[i])).collect(),
                };
                fence(Ordering::Acquire);
                if std::ptr::read_volatile(&(*frame).sequence) != sequence {
                    continue;
                }
                self.last_frame_index = frame_index;
                return Some(snapshot);
            }
        }
        None
    }
}

#[cfg(windows)]
mod windows {
    use super::Error;
    use std::ffi::c_void;

    const FILE_MAP_WRITE: u32 = 0x0002;
    const FILE_MAP_READ: u32 = 0x0004;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn OpenFileMappingA(desired_access: u32, inherit_handle: i32, name: *const u8) -> *mut c_void;
        fn MapViewOfFile(mapping: *mut c_void, desired_access: u32, offset_high: u32, offset_low: u32, bytes: usize) -> *mut c_void;
        fn UnmapViewOfFile(base: *const c_void) -> i32;
        fn CloseHandle(handle: *mut c_void) -> i32;
    }

    /// A file mapping handle and its view, closed together.
    pub struct Handle {
        mapping: *mut c_void,
        view: *mut c_void,
    }

    impl Drop for Handle {
        fn drop(&mut self) {
            // SAFETY: both came from the calls in open() and are released once
            unsafe {
                UnmapViewOfFile(self.view);
                CloseHandle(self.mapping);
            }
        }
    }

    pub fn open(name: &[u8], size: usize) -> Result<(Handle, *mut c_void), Error> {
        // SAFETY: `name` is NUL-terminated; the results are checked
        unsafe {
            let mapping = OpenFileMappingA(FILE_MAP_READ | FILE_MAP_WRITE, 0, name.as_ptr());
            if mapping.is_null() {
                return Err(Error::NotRunning);
            }
            let view = MapViewOfFile(mapping, FILE_MAP_READ | FILE_MAP_WRITE, 0, 0, size);
            if view.is_null() {
                CloseHandle(mapping);
                return Err(Error::NotRunning);
            }
            Ok((Handle { mapping, view }, view))
        }
    }
}
