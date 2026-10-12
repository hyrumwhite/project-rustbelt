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
//! The tags (models, animations, object definitions) are read from the
//! game's own tag cache in its memory (`halo_sim_set_tag_cache`), as the
//! game holds them, without a copy.
//!
//! Systems handed over so far: point physics (`point_physics_update`),
//! poses (`object_compute_node_matrices`: the orientations from the
//! animation, overlays and scale, the object types' preprocess, and the
//! interpolation and node matrices), and projectiles (`projectile_update`,
//! calling the game back for what it does to the world). The game's whole
//! ray test (`collision_test_vector`) is checked against the game's on its
//! live objects ([`world`]).
//!
//! No lock is held while the game is called back: a callback can reach
//! game code that calls this DLL again.

use std::collections::HashMap;
use std::ffi::{CStr, c_char};
use std::sync::{Arc, Mutex};

use halo_map::CacheFile;
use halo_map::collision::{CollisionBsp, StructureClusters, World};
use halo_map::physics::{PointPhysicsDefinition, PointState, point_physics_update_full};
use halo_map::pose::{PoseAnimation, PoseModel, PoseObjectDefinition};

pub mod area_damage;
pub mod poses;
pub mod preprocess;
pub mod bipeds;
pub mod damage;
pub mod damage_update;
pub mod devices;
pub mod effects;
pub mod particles;
pub mod contrails;
pub mod particle_systems;
pub mod player_effects;
pub mod ai_steps;
pub mod ai_update;
pub mod game_tick;
pub mod hs;
pub mod items;
pub mod object_exports;
pub mod object_functions;
pub mod objects;
pub mod weapons;
pub mod projectiles;
pub mod unit_ik;
pub mod units;
pub mod vehicles;
pub mod world;

/// The interface's version: the game checks it before calling anything.
pub const VERSION: u32 = 87;

/// A level's structure BSP, as the simulation tests it.
pub(crate) struct Structure {
    pub(crate) collision: CollisionBsp,
    pub(crate) clusters: StructureClusters,
}

/// The loaded level: its map file's path, and its structure BSPs by index.
struct Level {
    path: String,
    structures: Vec<Option<Arc<Structure>>>,
}

static LEVEL: Mutex<Option<Level>> = Mutex::new(None);

/// A structure BSP of the loaded level, with the breakable surfaces that
/// stand (`breakable_surfaces`: 32 bytes, or null for every one). The lock
/// is let go: a call can reach the game, and the game this DLL again.
fn structure(index: i32, breakable_surfaces: *const u8) -> Option<Arc<Structure>> {
    let level = LEVEL.lock().unwrap_or_else(|e| e.into_inner());
    let structure = level.as_ref()?.structures.get(usize::try_from(index).ok()?)?.clone()?;
    drop(level);
    structure.collision.breakable_surfaces.set(if breakable_surfaces.is_null() {
        None
    } else {
        // SAFETY: the game's 32 bytes of breakable surface flags
        Some(unsafe { *(breakable_surfaces as *const [u8; 32]) })
    });
    Some(structure)
}

/// The game's tag cache, and what has been read from it (until the next
/// level replaces it).
pub(crate) struct Tags {
    cache: CacheFile,
    definitions: HashMap<u32, Option<Arc<PoseObjectDefinition>>>,
    models: HashMap<u32, Option<Arc<PoseModel>>>,
    animations: HashMap<(u32, i16), Option<Arc<PoseAnimation>>>,
    graphs: HashMap<u32, Option<Arc<halo_map::unit_animation::AnimationGraph>>>,
    graphs_loaded: std::collections::HashSet<u32>,
    unit_flags: HashMap<u32, u32>,
    vehicles: HashMap<u32, Option<Arc<halo_map::vehicle_pose::VehiclePoseDefinition>>>,
    devices: HashMap<u32, Option<Arc<halo_map::device_pose::DevicePoseDefinition>>>,
    collision_models: HashMap<u32, Option<Arc<halo_map::collision::CollisionModel>>>,
    physics_shapes: HashMap<u32, Option<Arc<halo_map::vehicle_physics::PhysicsShape>>>,
    projectiles: HashMap<u32, Option<Arc<halo_map::projectile::ProjectileDefinition>>>,
    physics_definitions: HashMap<u32, Option<Arc<halo_map::vehicle_physics::PhysicsDefinition>>>,
    material_physics: Option<Arc<Vec<halo_map::vehicle_physics::MaterialPhysics>>>,
    vehicle_definitions: HashMap<u32, Option<Arc<halo_map::vehicle_update::VehicleDefinition>>>,
    biped_definitions: HashMap<u32, Option<Arc<halo_map::biped_moving::BipedDefinition>>>,
    player_information: Option<Option<halo_map::biped_moving::PlayerInformation>>,
    unit_graphs: HashMap<u32, Option<Arc<halo_map::unit_animation::AnimationGraph>>>,
    melee_weapons: HashMap<u32, Option<halo_map::biped_update::MeleeWeapon>>,
    unit_update_definitions: HashMap<u32, Option<Arc<halo_map::unit_update::UnitUpdateDefinition>>>,
    weapon_camo_rates: HashMap<u32, f32>,
    damage_definitions: HashMap<u32, Option<Arc<halo_map::damage::DamageDefinition>>>,
    damage_resistances: HashMap<u32, Option<Arc<halo_map::damage::DamageResistance>>>,
    child_damage_fractions: HashMap<u32, f32>,
    device_definitions: HashMap<(u32, bool), Option<Arc<halo_map::devices::DeviceDefinition>>>,
    item_definitions: HashMap<u32, Option<Arc<halo_map::items::ItemDefinition>>>,
    weapon_definitions: HashMap<u32, Option<Arc<halo_map::weapons::WeaponDefinition>>>,
    effect_definitions: HashMap<u32, Option<Arc<halo_map::effects::EffectDefinition>>>,
    particle_definitions: HashMap<u32, Option<Arc<halo_map::particles::ParticleDefinition>>>,
    contrail_definitions: HashMap<u32, Option<Arc<halo_map::contrails::ContrailDefinition>>>,
    particle_system_definitions: HashMap<u32, Option<Arc<halo_map::particle_systems::ParticleSystemDefinition>>>,
    damage_effect_views: HashMap<u32, Option<Arc<halo_map::player_effects::DamageEffectView>>>,
    continuous_effect_views: HashMap<u32, Option<Arc<halo_map::player_effects::ContinuousEffectView>>>,
    object_functions_definitions: HashMap<u32, Option<Arc<halo_map::object_functions::ObjectFunctionsDefinition>>>,
    export_definitions: HashMap<(u32, i16), Option<Arc<halo_map::object_exports::ExportDefinition>>>,
    model_markers: HashMap<u32, Option<Arc<halo_map::markers::ModelMarkers>>>,
}

impl Tags {
    fn definition(&mut self, datum: u32) -> Option<Arc<PoseObjectDefinition>> {
        let cache = &self.cache;
        self.definitions.entry(datum).or_insert_with(|| PoseObjectDefinition::load(cache, datum).ok().map(Arc::new)).clone()
    }
    fn model(&mut self, datum: u32) -> Option<Arc<PoseModel>> {
        let cache = &self.cache;
        self.models.entry(datum).or_insert_with(|| PoseModel::load(cache, datum).ok().map(Arc::new)).clone()
    }
    fn animation(&mut self, graph: u32, index: i16) -> Option<Arc<PoseAnimation>> {
        let cache = &self.cache;
        self.animations
            .entry((graph, index))
            .or_insert_with(|| PoseAnimation::load(cache, graph, index).ok().flatten().map(Arc::new))
            .clone()
    }
}

static TAGS: Mutex<Option<Tags>> = Mutex::new(None);

/// Takes the game's tag cache: `size` bytes at `memory`, which the game's
/// tag pointers address from `base`. Called at each level's load, after the
/// game read its tags. Returns the tag count, or -1 if the tags can't be
/// read.
///
/// # Safety
/// `memory` must point to `size` bytes that stay mapped while the game
/// runs, and hold the level's tags until the next call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn halo_sim_set_tag_cache(memory: *const u8, size: u32, base: u32) -> i32 {
    let mut tags = TAGS.lock().unwrap_or_else(|e| e.into_inner());
    *tags = None;
    if memory.is_null() {
        return -1;
    }
    // SAFETY: the caller's memory, mapped while the game runs
    let bytes: &'static [u8] = unsafe { std::slice::from_raw_parts(memory, size as usize) };
    // SAFETY: as above
    let Ok(cache) = (unsafe { CacheFile::from_memory(bytes, base) }) else { return -1 };
    let count = cache.tags.len() as i32;
    *tags = Some(Tags {
        cache,
        definitions: HashMap::new(),
        models: HashMap::new(),
        animations: HashMap::new(),
        graphs: HashMap::new(),
        graphs_loaded: Default::default(),
        unit_flags: HashMap::new(),
        vehicles: HashMap::new(),
        devices: HashMap::new(),
        collision_models: HashMap::new(),
        physics_shapes: HashMap::new(),
        projectiles: HashMap::new(),
        physics_definitions: HashMap::new(),
        material_physics: None,
        vehicle_definitions: HashMap::new(),
        biped_definitions: HashMap::new(),
        player_information: None,
        unit_graphs: HashMap::new(),
        melee_weapons: HashMap::new(),
        unit_update_definitions: HashMap::new(),
        weapon_camo_rates: HashMap::new(),
        damage_definitions: HashMap::new(),
        damage_resistances: HashMap::new(),
        child_damage_fractions: HashMap::new(),
        device_definitions: HashMap::new(),
        item_definitions: HashMap::new(),
        weapon_definitions: HashMap::new(),
        effect_definitions: HashMap::new(),
        particle_definitions: HashMap::new(),
        contrail_definitions: HashMap::new(),
        particle_system_definitions: HashMap::new(),
        damage_effect_views: HashMap::new(),
        continuous_effect_views: HashMap::new(),
        object_functions_definitions: HashMap::new(),
        export_definitions: HashMap::new(),
        model_markers: HashMap::new(),
    });
    count
}

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
    let mut structures: Vec<Option<Arc<Structure>>> = (0..count).map(|_| None).collect();
    for reference in &references {
        let Ok(bsp) = halo_map::bsp::Bsp::load(&cache, reference) else { continue };
        let (Ok(collision), Ok(clusters)) = (CollisionBsp::load(&bsp), StructureClusters::load(&cache, &bsp)) else { continue };
        structures[reference.index] = Some(Arc::new(Structure { collision, clusters }));
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
    let Some(structure) = structure(call.structure_bsp_index, call.breakable_surfaces) else { return -1 };
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
    let world = World { collision: &structure.collision, clusters: &structure.clusters, objects: &[], source: None };
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
