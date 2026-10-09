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

use std::sync::atomic::{fence, Ordering};

pub const MAGIC: u32 = 0x4252_4248; // "HBRB"
pub const VERSION: u32 = 1;
pub const MAXIMUM_OBJECTS: usize = 4096;
pub const MAXIMUM_NODES: usize = 49152;
const FRAMES: usize = 2;
#[cfg(windows)]
const MAPPING_NAME: &[u8] = b"Local\\HaloBevyBridge\0";

pub const OBJECT_HIDDEN: u32 = 1 << 0;
pub const OBJECT_FIRST_PERSON: u32 = 1 << 1;

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

#[repr(C)]
struct Frame {
    sequence: u32,
    frame_index: u32,
    game_tick: u32,
    structure_bsp_index: i32,
    scenario_name: [u8; 64],
    camera_valid: u32,
    camera: Camera,
    object_count: u32,
    node_count: u32,
    objects: [Object; MAXIMUM_OBJECTS],
    nodes: [Node; MAXIMUM_NODES],
}

#[repr(C)]
struct Shared {
    magic: u32,
    version: u32,
    size: u32,
    latest_frame: u32,
    writer_process_id: u32,
    reserved: [u32; 3],
    frames: [Frame; FRAMES],
}

/// The layout is the game's, measured there; keep these in step with
/// `bevy_bridge_protocol.h`.
const _: () = assert!(std::mem::size_of::<Object>() == 28);
const _: () = assert!(std::mem::size_of::<Node>() == 52);
const _: () = assert!(std::mem::size_of::<Camera>() == 40);

/// One frame of the game's world.
#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    pub frame_index: u32,
    pub game_tick: u32,
    pub structure_bsp_index: i32,
    /// The scenario tag's name, e.g. `levels\b30\b30`.
    pub scenario_name: String,
    pub camera: Option<Camera>,
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

/// The shared memory, mapped read-only.
pub struct Bridge {
    shared: *const Shared,
    #[cfg(windows)]
    _mapping: windows::Handle,
    last_frame_index: u32,
}

// The mapping is only read, through volatile reads and a sequence check.
unsafe impl Send for Bridge {}
unsafe impl Sync for Bridge {}

impl Bridge {
    /// Opens the game's shared memory.
    pub fn open() -> Result<Self, Error> {
        #[cfg(windows)]
        {
            let (mapping, view) = windows::open(MAPPING_NAME, std::mem::size_of::<Shared>())?;
            let bridge = Bridge { shared: view as *const Shared, _mapping: mapping, last_frame_index: 0 };
            bridge.check()?;
            Ok(bridge)
        }
        #[cfg(not(windows))]
        {
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
            let mapping = OpenFileMappingA(FILE_MAP_READ, 0, name.as_ptr());
            if mapping.is_null() {
                return Err(Error::NotRunning);
            }
            let view = MapViewOfFile(mapping, FILE_MAP_READ, 0, 0, size);
            if view.is_null() {
                CloseHandle(mapping);
                return Err(Error::NotRunning);
            }
            Ok((Handle { mapping, view }, view))
        }
    }
}
