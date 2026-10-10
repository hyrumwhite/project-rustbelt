//! The running game's object world, as Rust's ray test and features read
//! it: the host's callbacks (`halo_sim_set_host`) give the objects in a
//! cluster (its collideable cluster partition), an object's state at the
//! moment it is read, and its children. And the game's whole ray test
//! (`collision_test_vector`) in Rust on it.

use std::sync::{Arc, Mutex};

use halo_map::collision::{CollisionKind, CollisionModel, ObjectSource, World, WorldObject};
use halo_map::model::Matrix4x3;
use halo_map::vehicle_physics::{PhysicsShape, physics_instance_new};

use crate::{TAGS, Tags};

/// An object as the host reports it.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct HostObject {
    pub object_index: u32,
    pub definition_index: u32,
    pub object_type: i16,
    pub location_cluster: i16,
    pub flags: u32,
    pub bounding_center: [f32; 3],
    pub bounding_radius: f32,
    pub parent: u32,
    pub region_permutations: [u8; 8],
    /// Its node matrices (13 floats each), in the game's memory.
    pub node_count: u32,
    pub matrices: *const f32,
    /// Its physics' placement (`object_get_origin`, `object_get_orientation`).
    pub origin: [f32; 3],
    pub forward: [f32; 3],
    pub up: [f32; 3],
    /// A projectile's own flags (0 for another object).
    pub projectile_flags: u32,
}

/// The host's callbacks into the running game.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct Host {
    pub cluster_objects: unsafe extern "C" fn(cluster: i16, out: *mut u32, maximum: u32) -> u32,
    pub object: unsafe extern "C" fn(index: u32, out: *mut HostObject) -> i32,
    pub children: unsafe extern "C" fn(index: u32, out: *mut u32, maximum: u32) -> u32,
    pub projectile: crate::projectiles::ProjectileCallbacks,
}

static HOST: Mutex<Option<Host>> = Mutex::new(None);

/// Takes the host's callbacks.
///
/// # Safety
/// `host` must point to the callbacks, which stay valid while the game runs.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn halo_sim_set_host(host: *const Host) -> i32 {
    let mut stored = HOST.lock().unwrap_or_else(|e| e.into_inner());
    // SAFETY: as above
    *stored = (!host.is_null()).then(|| unsafe { *host });
    stored.is_some() as i32
}

impl Tags {
    pub(crate) fn collision_model(&mut self, definition: u32) -> Option<Arc<CollisionModel>> {
        let cache = &self.cache;
        self.collision_models
            .entry(definition)
            .or_insert_with(|| {
                let reference = halo_map::TagRef { group: 0, datum: Some(definition) };
                let datum = halo_map::objects::object_collision_model(cache, reference).ok()??.datum?;
                CollisionModel::load(cache, datum).ok().map(Arc::new)
            })
            .clone()
    }

    pub(crate) fn physics_shape(&mut self, definition: u32) -> Option<Arc<PhysicsShape>> {
        let cache = &self.cache;
        self.physics_shapes
            .entry(definition)
            .or_insert_with(|| PhysicsShape::load_for_object(cache, definition).ok().flatten().map(Arc::new))
            .clone()
    }
}

/// The running game's objects, through the host (the tags locked only to
/// read an object's collision model and physics).
pub(crate) struct GameObjects {
    pub(crate) host: Host,
}

/// The host's callbacks, if the game gave them.
pub(crate) fn host() -> Option<Host> {
    *HOST.lock().unwrap_or_else(|e| e.into_inner())
}

const OBJECT_TYPE_VEHICLE: i16 = 1;

impl ObjectSource for GameObjects {
    fn cluster_objects(&self, cluster: i16, out: &mut Vec<u32>) {
        let mut buffer = [0u32; 1024];
        // SAFETY: the host's callback, with a buffer of its size
        let count = unsafe { (self.host.cluster_objects)(cluster, buffer.as_mut_ptr(), buffer.len() as u32) } as usize;
        out.extend_from_slice(&buffer[..count.min(buffer.len())]);
    }

    fn object(&self, index: u32) -> Option<WorldObject> {
        let mut o = std::mem::MaybeUninit::<HostObject>::uninit();
        // SAFETY: the host's callback fills the object
        if unsafe { (self.host.object)(index, o.as_mut_ptr()) } == 0 {
            return None;
        }
        // SAFETY: filled above
        let o = unsafe { o.assume_init() };
        let matrices = (0..o.node_count as usize)
            .map(|n| {
                let mut f = [0.0f32; 13];
                // SAFETY: the host's matrices, node_count of them
                unsafe { std::ptr::copy_nonoverlapping(o.matrices.add(n * 13), f.as_mut_ptr(), 13) };
                Matrix4x3 { scale: f[0], forward: [f[1], f[2], f[3]], left: [f[4], f[5], f[6]], up: [f[7], f[8], f[9]], position: [f[10], f[11], f[12]] }
            })
            .collect();
        let (model, shape) = {
            let mut tags = TAGS.lock().unwrap_or_else(|e| e.into_inner());
            let tags = tags.as_mut()?;
            let shape = if o.object_type == OBJECT_TYPE_VEHICLE { tags.physics_shape(o.definition_index) } else { None };
            (tags.collision_model(o.definition_index), shape)
        };
        let physics = shape.map(|shape| physics_instance_new(shape, o.origin, o.forward, o.up));
        Some(WorldObject {
            index: o.object_index,
            kind: o.object_type,
            flags: o.flags,
            bounding_center: o.bounding_center,
            bounding_radius: o.bounding_radius,
            parent: o.parent,
            location_cluster: o.location_cluster,
            region_permutations: o.region_permutations,
            matrices,
            model,
            physics,
        })
    }

    fn children(&self, index: u32, out: &mut Vec<u32>) {
        let mut buffer = [0u32; 256];
        // SAFETY: the host's callback, with a buffer of its size
        let count = unsafe { (self.host.children)(index, buffer.as_mut_ptr(), buffer.len() as u32) } as usize;
        out.extend_from_slice(&buffer[..count.min(buffer.len())]);
    }
}

/// What the game's whole ray test reads.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct CollisionVectorCall {
    pub structure_bsp_index: i32,
    pub flags: u32,
    pub breakable_surfaces: *const u8,
    pub point: [f32; 3],
    pub vector: [f32; 3],
    pub ignore_object_index: u32,
}

/// What it found (`collision_result`'s fields Rust computes).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct CollisionVectorResult {
    pub hit: i32,
    /// `_collision_result_*` (media 0, structure 2, object 3), NONE for none.
    pub kind: i16,
    pub node_index: i16,
    pub t: f32,
    pub point: [f32; 3],
    pub plane: [f32; 4],
    pub object_index: u32,
    pub bsp_index: i16,
    pub material_type: i16,
    pub surface_index: i32,
    pub location_leaf: i32,
    pub location_cluster: i16,
    pub surface_flags: u16,
    pub region_index: i16,
    pub material_index: i16,
    pub plane_designator: i32,
    pub breakable_surface_index: u32,
}

/// The game's whole ray test (`collision_test_vector`) on the running
/// game's objects. Returns 0, or -1 if the level isn't there.
///
/// # Safety
/// `call` and `result` must be valid; `call.breakable_surfaces`, if not
/// null, must point to 32 bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn halo_sim_collision_test_vector(call: *const CollisionVectorCall, result: *mut CollisionVectorResult) -> i32 {
    if call.is_null() || result.is_null() {
        return -1;
    }
    // SAFETY: the caller's pointers
    let (call, result) = unsafe { (&*call, &mut *result) };
    let Some(host) = host() else { return -1 };
    let Some(structure) = crate::structure(call.structure_bsp_index, call.breakable_surfaces) else { return -1 };
    let source = GameObjects { host };
    let world = World { collision: &structure.collision, clusters: &structure.clusters, objects: &[], source: Some(&source) };
    let r = world.test(call.flags, call.point, call.vector, call.ignore_object_index);
    *result = CollisionVectorResult {
        hit: r.kind.is_some() as i32,
        kind: match r.kind {
            None => -1,
            Some(CollisionKind::Media) => 0,
            Some(CollisionKind::Structure) => 2,
            Some(CollisionKind::Object) => 3,
        },
        node_index: r.node,
        t: r.t,
        point: r.point,
        plane: r.plane,
        object_index: r.object.unwrap_or(u32::MAX),
        bsp_index: r.bsp,
        material_type: r.material_type,
        surface_index: r.surface,
        location_leaf: r.location_leaf,
        location_cluster: r.location_cluster,
        surface_flags: r.surface_flags as u16,
        region_index: r.region,
        material_index: r.material_index,
        plane_designator: r.plane_designator,
        breakable_surface_index: r.breakable_surface as u32,
    };
    0
}
