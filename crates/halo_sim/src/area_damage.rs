//! Area damage (`area_of_effect_cause_damage`) in Rust, on the running
//! game: the objects a detonation reaches, its rays (Rust's ray test, on
//! the live structure), its falloff and the infection forms it pops are
//! `halo_map::area_damage`'s; the damage it deals, teams, the difficulty
//! and breakable surfaces are the game's, through damage.c's
//! `area_damage_call` ([`AreaDamageCallbacks`]).
//!
//! In check mode the game's own area damage runs first: the game notes the
//! clusters the sphere reaches with their objects, and the objects (with
//! their children and parents), and records each of its calls (ray tests
//! included) with the damage before and after, and the objects a dealing
//! of damage changed; Rust replays it from those.

use std::cell::{Cell, RefCell};
use std::sync::Arc;

use halo_map::area_damage::{AreaCall, AreaDamageHost, AreaObject, area_of_effect_cause_damage, call};
use halo_map::collision::World;
use halo_map::damage::{DamageData, DamageDefinition, DamageResistance};

use crate::TAGS;
use crate::world::GameObjects;

/// The game's side of area damage (halo_sim.c).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct AreaDamageCallbacks {
    /// A cluster's objects in a partition (0 collideable, 1 not).
    pub cluster_objects: unsafe extern "C" fn(cluster: i16, partition: u32, out: *mut u32, maximum: u32) -> u32,
    /// An object: 1 if it is there.
    pub object: unsafe extern "C" fn(index: u32, out: *mut AreaObject) -> i32,
    /// A call: the damage written to the game first, read back after.
    pub call: unsafe extern "C" fn(damage: *mut DamageData, call: *mut AreaCall) -> i32,
}

/// A call of the game's area damage, recorded in check mode: what a dealing
/// of damage changed is `updates[update_first..][..update_count]` (an
/// object gone has type NONE).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct AreaEventC {
    pub call: AreaCall,
    pub seed: u32,
    pub damage_before: DamageData,
    pub damage_after: DamageData,
    pub update_first: u32,
    pub update_count: u32,
}

/// What area damage reads, and where it leaves the damage data.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct AreaCallC {
    pub damage: DamageData,
    pub structure_bsp_index: i32,
    /// Which breakable surfaces stand (32 bytes; null for every one).
    pub breakable_surfaces: *const u8,
    /// In check mode: the seed it began with; the clusters the game found,
    /// each one's collideable and noncollideable object counts
    /// (`partition_counts[2 * n]`, `[2 * n + 1]`) and the objects in order;
    /// the objects as they were; the calls; and what they changed.
    pub random_seed: u32,
    pub clusters: *const i16,
    pub cluster_count: u32,
    pub partition_counts: *const u32,
    pub partition_objects: *const u32,
    pub snapshot: *const AreaObject,
    pub snapshot_count: u32,
    pub events: *const AreaEventC,
    pub event_count: u32,
    pub updates: *const AreaObject,
    pub update_count: u32,
    /// In check mode: the first call that differs (-1 for none) and why
    /// (1 its kind, 2 what it was given, 3 the damage before it, 4 more calls
    /// than the game's, 5 fewer, 6 an object not snapshotted, 7 the clusters).
    pub difference_event: i32,
    pub difference_reason: i32,
}

fn tags<R>(f: impl FnOnce(&mut crate::Tags) -> R) -> Option<R> {
    let mut tags = TAGS.lock().unwrap_or_else(|e| e.into_inner());
    tags.as_mut().map(f)
}

/// The tags and the structure, as both hosts read them.
struct Common {
    structure: Arc<crate::Structure>,
}

impl Common {
    fn definition(&self, datum: u32) -> Option<Arc<DamageDefinition>> {
        tags(|t| t.damage_definition(datum)).flatten()
    }
    fn resistance(&self, object_definition: u32) -> Option<Arc<DamageResistance>> {
        tags(|t| t.damage_resistance(object_definition)).flatten()
    }
    fn unit_definition_flags(&self, object_definition: u32) -> u32 {
        tags(|t| t.unit_flags(object_definition)).unwrap_or(0)
    }
    fn clusters_in_sphere(&self, cluster: i16, center: [f32; 3], radius: f32, maximum: usize) -> Vec<i16> {
        self.structure.clusters.clusters_in_sphere(&self.structure.collision, cluster, center, radius, maximum)
    }
}

/// The running game.
struct LiveHost {
    common: Common,
    host: crate::world::Host,
}

impl AreaDamageHost for LiveHost {
    fn random_seed(&self) -> u32 {
        // SAFETY: the game's callback
        unsafe { (self.host.unit_animation.random_seed)() }
    }
    fn set_random_seed(&self, seed: u32) {
        // SAFETY: as above
        unsafe { (self.host.unit_animation.set_random_seed)(seed) };
    }
    fn definition(&self, datum: u32) -> Option<Arc<DamageDefinition>> {
        self.common.definition(datum)
    }
    fn resistance(&self, object_definition: u32) -> Option<Arc<DamageResistance>> {
        self.common.resistance(object_definition)
    }
    fn unit_definition_flags(&self, object_definition: u32) -> u32 {
        self.common.unit_definition_flags(object_definition)
    }
    fn clusters_in_sphere(&self, cluster: i16, center: [f32; 3], radius: f32, maximum: usize) -> Vec<i16> {
        self.common.clusters_in_sphere(cluster, center, radius, maximum)
    }
    fn cluster_objects(&self, cluster: i16, partition: u32, out: &mut Vec<u32>) {
        let mut buffer = vec![0u32; halo_map::area_damage::MAXIMUM_OBJECTS_PER_MAP];
        // SAFETY: the game's callback, with a buffer of its size
        let count = unsafe { (self.host.area_damage.cluster_objects)(cluster, partition, buffer.as_mut_ptr(), buffer.len() as u32) }
            as usize;
        out.extend_from_slice(&buffer[..count.min(buffer.len())]);
    }
    fn object(&self, index: u32) -> Option<AreaObject> {
        let mut object = AreaObject::default();
        // SAFETY: as above
        (unsafe { (self.host.area_damage.object)(index, &mut object) } != 0).then_some(object)
    }
    fn call(&self, damage: &mut DamageData, call: &mut AreaCall) -> i32 {
        if call.kind == call::RAY_TEST {
            // the game's whole ray test, in Rust, on the live world
            let source = GameObjects { host: self.host };
            let world = World {
                collision: &self.common.structure.collision,
                clusters: &self.common.structure.clusters,
                objects: &[],
                source: Some(&source),
            };
            let f = call.f;
            let r = world.test(call.a as u32, [f[0], f[1], f[2]], [f[3], f[4], f[5]], call.object_index as u32);
            call.out = r.point;
            call.result = r.kind.is_some() as i32;
            return call.result;
        }
        // SAFETY: as above
        unsafe { (self.host.area_damage.call)(damage, call) }
    }
}

/// The game's own area damage, as it recorded it.
struct CheckHost<'a> {
    common: Common,
    clusters: &'a [i16],
    partition_counts: &'a [u32],
    partition_objects: &'a [u32],
    snapshot: &'a [AreaObject],
    events: &'a [AreaEventC],
    updates: &'a [AreaObject],
    next: Cell<usize>,
    seed: Cell<u32>,
    difference: Cell<Option<(usize, i32)>>,
    /// Objects the recording changed since the snapshot.
    updated: RefCell<Vec<AreaObject>>,
}

impl CheckHost<'_> {
    fn differ(&self, event: usize, reason: i32) {
        if self.difference.get().is_none() {
            self.difference.set(Some((event, reason)));
        }
    }
}

impl AreaDamageHost for CheckHost<'_> {
    fn random_seed(&self) -> u32 {
        self.seed.get()
    }
    fn set_random_seed(&self, seed: u32) {
        self.seed.set(seed);
    }
    fn definition(&self, datum: u32) -> Option<Arc<DamageDefinition>> {
        self.common.definition(datum)
    }
    fn resistance(&self, object_definition: u32) -> Option<Arc<DamageResistance>> {
        self.common.resistance(object_definition)
    }
    fn unit_definition_flags(&self, object_definition: u32) -> u32 {
        self.common.unit_definition_flags(object_definition)
    }
    fn clusters_in_sphere(&self, cluster: i16, center: [f32; 3], radius: f32, maximum: usize) -> Vec<i16> {
        let clusters = self.common.clusters_in_sphere(cluster, center, radius, maximum);
        if clusters != self.clusters {
            self.differ(self.next.get(), 7);
        }
        clusters
    }
    fn cluster_objects(&self, cluster: i16, partition: u32, out: &mut Vec<u32>) {
        let mut first = 0usize;
        for (n, &c) in self.clusters.iter().enumerate() {
            let counts = [self.partition_counts[2 * n] as usize, self.partition_counts[2 * n + 1] as usize];
            if c == cluster {
                let start = first + if partition == 0 { 0 } else { counts[0] };
                let count = counts[partition as usize & 1];
                out.extend_from_slice(&self.partition_objects[start..start + count]);
                return;
            }
            first += counts[0] + counts[1];
        }
        self.differ(self.next.get(), 7);
    }
    fn object(&self, index: u32) -> Option<AreaObject> {
        let found = match self.updated.borrow().iter().rev().find(|s| s.object_index == index) {
            Some(state) => Some(*state),
            None => self.snapshot.iter().find(|s| s.object_index == index).copied(),
        };
        match found {
            Some(state) if state.object_type == -1 => None,
            Some(state) => Some(state),
            None => {
                self.differ(self.next.get(), 6);
                None
            }
        }
    }
    fn call(&self, damage: &mut DamageData, call: &mut AreaCall) -> i32 {
        let index = self.next.get();
        let Some(event) = self.events.get(index) else {
            self.differ(index, 4);
            return 0;
        };
        self.next.set(index + 1);
        let e = &event.call;
        if e.kind != call.kind {
            self.differ(index, 1);
        } else if e.object_index != call.object_index
            || e.a != call.a
            || e.b != call.b
            || e.f.map(f32::to_bits) != call.f.map(f32::to_bits)
        {
            self.differ(index, 2);
        } else if event.damage_before != *damage {
            self.differ(index, 3);
        }
        *damage = event.damage_after;
        let first = event.update_first as usize;
        if let Some(updates) = self.updates.get(first..first + event.update_count as usize) {
            self.updated.borrow_mut().extend_from_slice(updates);
        }
        self.seed.set(event.seed);
        call.out = e.out;
        call.result = e.result;
        e.result
    }
}

/// # Safety
/// `pointer` must be null or point to `count` values.
unsafe fn slice<'a, T>(pointer: *const T, count: u32) -> &'a [T] {
    if pointer.is_null() || count == 0 {
        &[]
    } else {
        // SAFETY: as above
        unsafe { std::slice::from_raw_parts(pointer, count as usize) }
    }
}

fn run(call: &mut AreaCallC, check: bool) -> i32 {
    let Some(host) = crate::world::host() else { return -1 };
    let Some(structure) = crate::structure(call.structure_bsp_index, call.breakable_surfaces) else { return -1 };
    let common = Common { structure };
    let mut damage = call.damage;
    call.difference_event = -1;
    call.difference_reason = 0;
    if check {
        // SAFETY: the game's recording, as described
        let (clusters, partition_counts, snapshot, events, updates) = unsafe {
            (
                slice(call.clusters, call.cluster_count),
                slice(call.partition_counts, call.cluster_count * 2),
                slice(call.snapshot, call.snapshot_count),
                slice(call.events, call.event_count),
                slice(call.updates, call.update_count),
            )
        };
        let partition_total: u32 = partition_counts.iter().sum();
        // SAFETY: as above
        let partition_objects = unsafe { slice(call.partition_objects, partition_total) };
        let checker = CheckHost {
            common,
            clusters,
            partition_counts,
            partition_objects,
            snapshot,
            events,
            updates,
            next: Cell::new(0),
            seed: Cell::new(call.random_seed),
            difference: Cell::new(None),
            updated: RefCell::new(Vec::new()),
        };
        area_of_effect_cause_damage(&checker, &mut damage);
        if checker.difference.get().is_none() && checker.next.get() < events.len() {
            checker.differ(checker.next.get(), 5);
        }
        (call.difference_event, call.difference_reason) = checker.difference.get().map_or((-1, 0), |(e, r)| (e as i32, r));
    } else {
        let live = LiveHost { common, host };
        area_of_effect_cause_damage(&live, &mut damage);
    }
    call.damage = damage;
    0
}

/// Area damage on the running game, in place of the game's. Returns 0 (the
/// damage data written), or -1 if it can't run (the game then deals its own;
/// nothing was done).
///
/// # Safety
/// `call` must be valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn halo_sim_area_of_effect_cause_damage(call: *mut AreaCallC) -> i32 {
    if call.is_null() {
        return -1;
    }
    // SAFETY: the caller's call
    run(unsafe { &mut *call }, false)
}

/// The check of area damage the game dealt: Rust's replay of it. Returns 0,
/// or -1.
///
/// # Safety
/// As [`halo_sim_area_of_effect_cause_damage`]; the recording holds its
/// counts.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn halo_sim_area_of_effect_cause_damage_check(call: *mut AreaCallC) -> i32 {
    if call.is_null() {
        return -1;
    }
    // SAFETY: the caller's call
    run(unsafe { &mut *call }, true)
}
