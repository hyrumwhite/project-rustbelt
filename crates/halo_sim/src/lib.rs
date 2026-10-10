//! `halo_map`'s simulation ports as a DLL the game (OpenCE, 32-bit) loads
//! and calls in place of its own C, one system at a time (ROADMAP phase 7:
//! the object simulation handing off from the game to Rust).
//!
//! The game calls in with the state each update reads and takes back what
//! it leaves, through the C interface below (`port/linux/game/halo_sim.c`
//! on the game's side). The level comes from the game's own map file: at a
//! level's first call the DLL reads it and keeps each structure BSP's
//! collision geometry and clusters, then lets the rest go (the game's
//! 32-bit address space holds its own copy of the map).
//!
//! Systems handed over so far: point physics (`point_physics_update`).

use std::ffi::{CStr, c_char};
use std::sync::Mutex;

use halo_map::CacheFile;
use halo_map::collision::{CollisionBsp, StructureClusters, World};
use halo_map::physics::{PointPhysicsDefinition, PointState, point_physics_update_full};

/// The interface's version: the game checks it before calling anything.
pub const VERSION: u32 = 1;

/// A level's structure BSP, as the simulation tests it.
struct Structure {
    collision: CollisionBsp,
    clusters: StructureClusters,
}

/// The loaded level: its map file's path, and its structure BSPs by index.
struct Level {
    path: String,
    structures: Vec<Option<Structure>>,
}

static LEVEL: Mutex<Option<Level>> = Mutex::new(None);

/// The interface's version ([`VERSION`]).
#[unsafe(no_mangle)]
pub extern "C" fn halo_sim_version() -> u32 {
    VERSION
}

/// Loads the level of the map file at `path` (a NUL-terminated path), if it
/// isn't the one loaded already. Returns its structure BSP count, or -1 if
/// the file can't be read.
///
/// # Safety
/// `path` must point to a NUL-terminated string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn halo_sim_load_map(path: *const c_char) -> i32 {
    if path.is_null() {
        return -1;
    }
    // SAFETY: the caller passes a NUL-terminated string
    let path = unsafe { CStr::from_ptr(path) }.to_string_lossy().into_owned();
    let mut level = LEVEL.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(loaded) = level.as_ref()
        && loaded.path == path
    {
        return loaded.structures.len() as i32;
    }
    *level = None;
    let Ok(cache) = CacheFile::open(&path) else { return -1 };
    let Ok(references) = halo_map::bsp::references(&cache) else { return -1 };
    let count = references.iter().map(|r| r.index + 1).max().unwrap_or(0);
    let mut structures: Vec<Option<Structure>> = (0..count).map(|_| None).collect();
    for reference in &references {
        let Ok(bsp) = halo_map::bsp::Bsp::load(&cache, reference) else { continue };
        let (Ok(collision), Ok(clusters)) = (CollisionBsp::load(&bsp), StructureClusters::load(&cache, &bsp)) else { continue };
        structures[reference.index] = Some(Structure { collision, clusters });
    }
    drop(cache);
    *level = Some(Level { path, structures });
    count as i32
}

/// `point_physics_definition`, as the game lays it out (0x40 bytes; the
/// game may pass one it blended from two).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct PointPhysicsDefinitionC {
    pub flags: u32,
    pub runtime_mass_over_radius_cubed: f32,
    pub runtime_water_buoyancy_scale: f32,
    pub runtime_air_buoyancy_scale: f32,
    pub unused1: [i32; 4],
    pub density: f32,
    pub air_friction: f32,
    pub water_friction: f32,
    pub contact_friction: f32,
    pub elasticity: f32,
    pub unused2: [i32; 3],
}

const _: () = assert!(std::mem::size_of::<PointPhysicsDefinitionC>() == 0x40);

/// What a point physics update reads (`point_physics_update`, after the
/// game found the wind or current and whether the point is under water).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct PointPhysicsCall {
    pub structure_bsp_index: i32,
    pub flags: u32,
    pub definition: *const PointPhysicsDefinitionC,
    /// Which breakable surfaces stand (32 bytes; null for every one).
    pub breakable_surfaces: *const u8,
    pub position: [f32; 3],
    pub velocity: [f32; 3],
    pub location_leaf: i32,
    pub location_cluster: i16,
    pub underwater: i16,
    pub has_force: u32,
    pub force: [f32; 3],
    pub wind: [f32; 3],
    pub radius: f32,
    pub dt: f32,
}

/// What it leaves: the result flags, the point's place, velocity and
/// location, and the last collision's normal and material type, if it
/// collided.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct PointPhysicsResult {
    pub flags: u32,
    pub position: [f32; 3],
    pub velocity: [f32; 3],
    pub location_leaf: i32,
    pub location_cluster: i16,
    pub collided: i16,
    pub collision_normal: [f32; 3],
    pub collision_material_type: i32,
}

/// One `point_physics_update`. Returns 0 on success, or -1 if the level or
/// structure BSP isn't loaded (the game then runs its own).
///
/// # Safety
/// `call` and `result` must be valid; `call.definition` must point to a
/// definition, and `call.breakable_surfaces`, if not null, to 32 bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn halo_sim_point_physics_update(call: *const PointPhysicsCall, result: *mut PointPhysicsResult) -> i32 {
    if call.is_null() || result.is_null() {
        return -1;
    }
    // SAFETY: the caller passes valid pointers
    let call = unsafe { &*call };
    if call.definition.is_null() {
        return -1;
    }
    // SAFETY: as above
    let d = unsafe { &*call.definition };
    let mut level = LEVEL.lock().unwrap_or_else(|e| e.into_inner());
    let Some(level) = level.as_mut() else { return -1 };
    let Some(Some(structure)) = usize::try_from(call.structure_bsp_index).ok().and_then(|i| level.structures.get_mut(i)) else {
        return -1;
    };
    structure.collision.breakable_surfaces = if call.breakable_surfaces.is_null() {
        None
    } else {
        // SAFETY: 32 bytes, as above
        Some(unsafe { *(call.breakable_surfaces as *const [u8; 32]) })
    };
    let definition = PointPhysicsDefinition {
        flags: d.flags,
        runtime_mass_over_radius_cubed: d.runtime_mass_over_radius_cubed,
        runtime_water_buoyancy_scale: d.runtime_water_buoyancy_scale,
        runtime_air_buoyancy_scale: d.runtime_air_buoyancy_scale,
        density: d.density,
        air_friction: d.air_friction,
        water_friction: d.water_friction,
        contact_friction: d.contact_friction,
        elasticity: d.elasticity,
    };
    let mut state =
        PointState { position: call.position, velocity: call.velocity, location_leaf: call.location_leaf, location_cluster: call.location_cluster };
    let world = World { collision: &structure.collision, clusters: &structure.clusters, objects: &[] };
    let update = point_physics_update_full(
        &world,
        call.flags,
        &definition,
        &mut state,
        call.wind,
        call.underwater != 0,
        (call.has_force != 0).then_some(call.force),
        call.radius,
        call.dt,
    );
    // SAFETY: as above
    let result = unsafe { &mut *result };
    *result = PointPhysicsResult {
        flags: update.flags,
        position: state.position,
        velocity: state.velocity,
        location_leaf: state.location_leaf,
        location_cluster: state.location_cluster,
        collided: update.collision.is_some() as i16,
        collision_normal: update.collision.map_or([0.0; 3], |c| c.normal),
        collision_material_type: update.collision.map_or(-1, |c| c.material_type as i32),
    };
    0
}
