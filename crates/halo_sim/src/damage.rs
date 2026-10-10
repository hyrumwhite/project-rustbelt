//! Damage (`object_cause_damage`) in Rust, on the running game: its roll,
//! the objects it reaches, and what their resistance makes of it (shield
//! and body) are `halo_map::damage`'s; what damage does to the rest of the
//! game is the game's, through damage.c's `damage_call`
//! ([`DamageCallbacks`]).
//!
//! In check mode the game's own damage runs first, recording each of its
//! calls with the damage and the object before and after; Rust replays it
//! from the objects as they were when it began.

use std::cell::{Cell, RefCell};
use std::sync::Arc;

use halo_map::damage::{
    DamageCall, DamageData, DamageDefinition, DamageHost, DamageInputs, DamageResistance, ObjectDamageState, object_cause_damage,
    object_collision_model, unit_child_damage_fraction,
};

use crate::TAGS;

/// The game's side of damage (halo_sim.c).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct DamageCallbacks {
    /// An object's damage state: 1 if it is there.
    pub object: unsafe extern "C" fn(index: u32, out: *mut ObjectDamageState) -> i32,
    /// A call: the damage and the objects written to the game first, read
    /// back after.
    pub call: unsafe extern "C" fn(damage: *mut DamageData, objects: *mut ObjectDamageState, count: u32, call: *mut DamageCall) -> i32,
}

/// A call of the game's damage, recorded in check mode.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct DamageEventC {
    pub call: DamageCall,
    pub result: i32,
    pub seed: u32,
    pub damage_before: DamageData,
    pub damage_after: DamageData,
    pub object_before: ObjectDamageState,
    pub object_after: ObjectDamageState,
}

/// What damage reads, and where it leaves the damage data.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct DamageCallC {
    pub damage: DamageData,
    pub object_index: u32,
    pub node_index: i16,
    pub region_index: i16,
    pub material_index: i16,
    pub has_normal: u16,
    pub game_engine_running: u32,
    pub distributed_damage_authorized: u32,
    pub cheat_omnipotent: u32,
    pub cheat_deathless_player: u32,
    /// In check mode: the seed it began with, the objects as they were, and
    /// the game's calls.
    pub random_seed: u32,
    pub snapshot: *const ObjectDamageState,
    pub snapshot_count: u32,
    pub events: *const DamageEventC,
    pub event_count: u32,
    /// In check mode: the first call that differs (-1 for none) and why
    /// (1 its kind, 2 what it was given, 3 the damage or object before it,
    /// 4 more calls than the game's, 5 fewer, 6 an object not snapshotted).
    /// Each object's last change precedes its aftermath, whose object is
    /// compared; the damage data Rust leaves is compared by the game.
    pub difference_event: i32,
    pub difference_reason: i32,
}

impl crate::Tags {
    fn damage_definition(&mut self, datum: u32) -> Option<Arc<DamageDefinition>> {
        let cache = &self.cache;
        self.damage_definitions.entry(datum).or_insert_with(|| DamageDefinition::load(cache, datum).ok().map(Arc::new)).clone()
    }

    fn damage_resistance(&mut self, object_definition: u32) -> Option<Arc<DamageResistance>> {
        let cache = &self.cache;
        self.damage_resistances
            .entry(object_definition)
            .or_insert_with(|| {
                let model = object_collision_model(cache, object_definition).ok()??;
                DamageResistance::load(cache, model).ok().map(Arc::new)
            })
            .clone()
    }

    fn child_damage_fraction(&mut self, object_definition: u32) -> f32 {
        let cache = &self.cache;
        *self.child_damage_fractions.entry(object_definition).or_insert_with(|| unit_child_damage_fraction(cache, object_definition).unwrap_or(0.0))
    }
}

fn tags<R>(f: impl FnOnce(&mut crate::Tags) -> R) -> Option<R> {
    let mut tags = TAGS.lock().unwrap_or_else(|e| e.into_inner());
    tags.as_mut().map(f)
}

/// The tags, as both hosts read them (the lock held only to read).
trait TagHost {
    fn definition(&self, datum: u32) -> Option<Arc<DamageDefinition>> {
        tags(|t| t.damage_definition(datum)).flatten()
    }
    fn resistance(&self, object_definition: u32) -> Option<Arc<DamageResistance>> {
        tags(|t| t.damage_resistance(object_definition)).flatten()
    }
    fn child_damage_fraction(&self, object_definition: u32) -> f32 {
        tags(|t| t.child_damage_fraction(object_definition)).unwrap_or(0.0)
    }
}

/// The running game.
struct LiveHost {
    callbacks: DamageCallbacks,
    random: crate::units::UnitAnimationCallbacks,
}

impl TagHost for LiveHost {}

impl DamageHost for LiveHost {
    fn random_seed(&self) -> u32 {
        // SAFETY: the game's callback
        unsafe { (self.random.random_seed)() }
    }
    fn set_random_seed(&self, seed: u32) {
        // SAFETY: as above
        unsafe { (self.random.set_random_seed)(seed) };
    }
    fn object(&self, index: u32) -> Option<ObjectDamageState> {
        let mut state = ObjectDamageState::default();
        // SAFETY: as above
        (unsafe { (self.callbacks.object)(index, &mut state) } != 0).then_some(state)
    }
    fn definition(&self, datum: u32) -> Option<Arc<DamageDefinition>> {
        TagHost::definition(self, datum)
    }
    fn resistance(&self, object_definition: u32) -> Option<Arc<DamageResistance>> {
        TagHost::resistance(self, object_definition)
    }
    fn child_damage_fraction(&self, object_definition: u32) -> f32 {
        TagHost::child_damage_fraction(self, object_definition)
    }
    fn call(&self, damage: &mut DamageData, objects: &mut [ObjectDamageState], call: &mut DamageCall) -> i32 {
        // SAFETY: as above
        unsafe { (self.callbacks.call)(damage, objects.as_mut_ptr(), objects.len() as u32, call) }
    }
}

/// The game's own damage, as it recorded it.
struct CheckHost<'a> {
    snapshot: &'a [ObjectDamageState],
    events: &'a [DamageEventC],
    next: Cell<usize>,
    seed: Cell<u32>,
    difference: Cell<Option<(usize, i32)>>,
    /// Objects the recording changed since the snapshot.
    updated: RefCell<Vec<ObjectDamageState>>,
}

impl CheckHost<'_> {
    fn differ(&self, event: usize, reason: i32) {
        if self.difference.get().is_none() {
            self.difference.set(Some((event, reason)));
        }
    }
}

impl TagHost for CheckHost<'_> {}

impl DamageHost for CheckHost<'_> {
    fn random_seed(&self) -> u32 {
        self.seed.get()
    }
    fn set_random_seed(&self, seed: u32) {
        self.seed.set(seed);
    }
    fn object(&self, index: u32) -> Option<ObjectDamageState> {
        if let Some(state) = self.updated.borrow().iter().find(|s| s.object_index == index) {
            return Some(*state);
        }
        let found = self.snapshot.iter().find(|s| s.object_index == index).copied();
        if found.is_none() {
            self.differ(self.next.get(), 6);
        }
        found
    }
    fn definition(&self, datum: u32) -> Option<Arc<DamageDefinition>> {
        TagHost::definition(self, datum)
    }
    fn resistance(&self, object_definition: u32) -> Option<Arc<DamageResistance>> {
        TagHost::resistance(self, object_definition)
    }
    fn child_damage_fraction(&self, object_definition: u32) -> f32 {
        TagHost::child_damage_fraction(self, object_definition)
    }
    fn call(&self, damage: &mut DamageData, objects: &mut [ObjectDamageState], call: &mut DamageCall) -> i32 {
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
        } else if event.damage_before != *damage
            || (call.object_index != -1
                && objects.iter().find(|o| o.object_index == call.object_index as u32).is_some_and(|o| *o != event.object_before))
        {
            self.differ(index, 3);
        }
        *damage = event.damage_after;
        if call.object_index != -1 {
            if let Some(o) = objects.iter_mut().find(|o| o.object_index == call.object_index as u32) {
                *o = event.object_after;
            }
            let mut updated = self.updated.borrow_mut();
            match updated.iter_mut().find(|s| s.object_index == call.object_index as u32) {
                Some(s) => *s = event.object_after,
                None => updated.push(event.object_after),
            }
        }
        self.seed.set(event.seed);
        call.out = e.out;
        event.result
    }
}

fn run(call: &mut DamageCallC, check: bool) -> i32 {
    let Some(host) = crate::world::host() else { return -1 };
    let inputs = DamageInputs {
        game_engine_running: call.game_engine_running != 0,
        distributed_damage_authorized: call.distributed_damage_authorized != 0,
        cheat_omnipotent: call.cheat_omnipotent != 0,
        cheat_deathless_player: call.cheat_deathless_player != 0,
    };
    let mut damage = call.damage;
    call.difference_event = -1;
    call.difference_reason = 0;
    if check {
        // SAFETY: the game's recording, as described
        let (snapshot, events) = unsafe {
            (
                if call.snapshot.is_null() { &[][..] } else { std::slice::from_raw_parts(call.snapshot, call.snapshot_count as usize) },
                if call.events.is_null() { &[][..] } else { std::slice::from_raw_parts(call.events, call.event_count as usize) },
            )
        };
        let checker = CheckHost {
            snapshot,
            events,
            next: Cell::new(0),
            seed: Cell::new(call.random_seed),
            difference: Cell::new(None),
            updated: RefCell::new(Vec::new()),
        };
        object_cause_damage(
            &inputs,
            &checker,
            &mut damage,
            call.object_index,
            call.node_index,
            call.region_index,
            call.material_index,
            call.has_normal != 0,
        );
        if checker.difference.get().is_none() && checker.next.get() < events.len() {
            checker.differ(checker.next.get(), 5);
        }
        (call.difference_event, call.difference_reason) = checker.difference.get().map_or((-1, 0), |(e, r)| (e as i32, r));
    } else {
        let live = LiveHost { callbacks: host.damage, random: host.unit_animation };
        object_cause_damage(
            &inputs,
            &live,
            &mut damage,
            call.object_index,
            call.node_index,
            call.region_index,
            call.material_index,
            call.has_normal != 0,
        );
    }
    call.damage = damage;
    0
}

/// Damage on the running game, in place of the game's (its network
/// authority over the outermost call already the caller's). Returns 0 (the
/// damage data written), or -1 if it can't run (the game then deals its own;
/// nothing was done).
///
/// # Safety
/// `call` must be valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn halo_sim_object_cause_damage(call: *mut DamageCallC) -> i32 {
    if call.is_null() {
        return -1;
    }
    // SAFETY: the caller's call
    run(unsafe { &mut *call }, false)
}

/// The check of damage the game dealt: Rust's replay of it. Returns 0, or -1.
///
/// # Safety
/// As [`halo_sim_object_cause_damage`]; the snapshot and events hold their
/// counts.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn halo_sim_object_cause_damage_check(call: *mut DamageCallC) -> i32 {
    if call.is_null() {
        return -1;
    }
    // SAFETY: the caller's call
    run(unsafe { &mut *call }, true)
}
