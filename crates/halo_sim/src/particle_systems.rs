//! Particle system updates (`particle_system_update`, each system's every
//! frame) in Rust, on the running game: a system's types, states, new
//! particles and its particles' states are `halo_map::particle_systems`';
//! what reads the world or allocates (its object, point physics, markers,
//! locations, particle datums, deleting the system) is the game's, through
//! particle_systems.c's `particle_system_call` ([`ParticleSystemCallbacks`]).
//! The game hands over the system with its particles, and writes back what
//! Rust leaves.
//!
//! In check mode the game's own update runs first, recording each call with
//! the local random seed before and after; Rust replays it, and the game
//! compares the system and particles Rust leaves with its own.

use std::cell::Cell;
use std::sync::Arc;

use halo_map::particle_systems::{
    NewSystem, ParticleSystemCall, ParticleSystemDefinition, ParticleSystemHost, ParticleSystemInputs,
    ParticleSystemState, SystemParticle, particle_system_new, particle_system_update, particles_in_order,
};

use crate::TAGS;

/// The game's side of particle system updates (halo_sim.c).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct ParticleSystemCallbacks {
    /// A call: the seed (and for a deletion the system and its particles)
    /// written to the game first, the seed read back after.
    pub call: unsafe extern "C" fn(
        state: *mut ParticleSystemState,
        particles: *const SystemParticle,
        particle_count: u32,
        call: *mut ParticleSystemCall,
    ),
}

/// A call of the game's update, recorded in check mode.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct ParticleSystemEventC {
    pub call: ParticleSystemCall,
    pub seed_before: u32,
    pub seed_after: u32,
}

/// What a system's update reads, and where it leaves the system.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct ParticleSystemUpdateC {
    pub state: ParticleSystemState,
    pub dt: f32,
    pub directions: *const [f32; 3],
    pub direction_count: u32,
    /// Its particles in, in any order; out, in the order the game walks them.
    pub particles: *mut SystemParticle,
    pub particle_count: u32,
    pub particle_capacity: u32,
    /// In check mode: the game's calls, and the first that differs (-1 for
    /// none) and why (1 its kind, 2 what it was given, 3 the seed before it,
    /// 4 more calls than the game's, 5 fewer).
    pub events: *const ParticleSystemEventC,
    pub event_count: u32,
    pub difference_event: i32,
    pub difference_reason: i32,
    /// Whether the system is still there after.
    pub still_there: u32,
}

impl crate::Tags {
    fn particle_system_definition(&mut self, datum: u32) -> Option<Arc<ParticleSystemDefinition>> {
        if let Some(definition) = self.particle_system_definitions.get(&datum) {
            return definition.clone();
        }
        let definition = ParticleSystemDefinition::load(&self.cache, datum).ok().map(Arc::new);
        self.particle_system_definitions.insert(datum, definition.clone());
        definition
    }
}

/// The running game.
struct LiveHost {
    callbacks: ParticleSystemCallbacks,
}

impl ParticleSystemHost for LiveHost {
    fn call(&self, state: &mut ParticleSystemState, particles: &[SystemParticle], call: &mut ParticleSystemCall) {
        // SAFETY: the game's callback
        unsafe { (self.callbacks.call)(state, particles.as_ptr(), particles.len() as u32, call) };
    }
}

/// The game's own update, as it recorded it.
struct CheckHost<'a> {
    events: &'a [ParticleSystemEventC],
    next: Cell<usize>,
    difference: Cell<Option<(usize, i32)>>,
}

impl CheckHost<'_> {
    fn differ(&self, event: usize, reason: i32) {
        if self.difference.get().is_none() {
            self.difference.set(Some((event, reason)));
        }
    }
}

impl ParticleSystemHost for CheckHost<'_> {
    fn call(&self, state: &mut ParticleSystemState, _particles: &[SystemParticle], call: &mut ParticleSystemCall) {
        let index = self.next.get();
        let Some(event) = self.events.get(index) else {
            self.differ(index, 4);
            return;
        };
        self.next.set(index + 1);
        let e = &event.call;
        if e.kind != call.kind {
            self.differ(index, 1);
        } else if e.a != call.a
            || e.b != call.b
            || e.c != call.c
            || e.d != call.d
            || e.f.map(f32::to_bits) != call.f.map(f32::to_bits)
        {
            self.differ(index, 2);
        } else if event.seed_before != state.local_random_seed {
            self.differ(index, 3);
        }
        state.local_random_seed = event.seed_after;
        call.result = e.result;
        call.out = e.out;
        call.out_location_leaf = e.out_location_leaf;
        call.out_location_cluster = e.out_location_cluster;
        call.out_location_bonus = e.out_location_bonus;
    }
}

fn run(call: &mut ParticleSystemUpdateC, check: bool) -> i32 {
    let Some(host) = crate::world::host() else { return -1 };
    let definition = {
        let mut tags = TAGS.lock().unwrap_or_else(|e| e.into_inner());
        let Some(tags) = tags.as_mut() else { return -1 };
        tags.particle_system_definition(call.state.definition_index)
    };
    let Some(definition) = definition else { return -1 };
    if call.particles.is_null() {
        return -1;
    }
    // SAFETY: the game's direction table, direction_count of them
    let directions = if call.directions.is_null() {
        &[][..]
    } else {
        unsafe { std::slice::from_raw_parts(call.directions, call.direction_count as usize) }
    };
    // SAFETY: the system's particles, particle_count of them in a buffer of
    // particle_capacity
    let mut particles = unsafe { std::slice::from_raw_parts(call.particles, call.particle_count as usize) }.to_vec();
    let inputs = ParticleSystemInputs { dt: call.dt, directions };
    let mut state = call.state;
    call.difference_event = -1;
    call.difference_reason = 0;
    let still_there = if check {
        // SAFETY: the game's recording, as described
        let events = if call.events.is_null() || call.event_count == 0 {
            &[][..]
        } else {
            unsafe { std::slice::from_raw_parts(call.events, call.event_count as usize) }
        };
        let checker = CheckHost { events, next: Cell::new(0), difference: Cell::new(None) };
        let still_there = particle_system_update(&definition, &inputs, &checker, &mut state, &mut particles);
        if checker.difference.get().is_none() && checker.next.get() < events.len() {
            checker.differ(checker.next.get(), 5);
        }
        (call.difference_event, call.difference_reason) = checker.difference.get().map_or((-1, 0), |(e, r)| (e as i32, r));
        still_there
    } else {
        let live = LiveHost { callbacks: host.particle_systems };
        particle_system_update(&definition, &inputs, &live, &mut state, &mut particles)
    };
    let ordered = if still_there { particles_in_order(&state, definition.types.len(), &particles) } else { Vec::new() };
    let count = ordered.len().min(call.particle_capacity as usize);
    // SAFETY: the buffer holds particle_capacity
    unsafe { std::ptr::copy_nonoverlapping(ordered.as_ptr(), call.particles, count) };
    call.particle_count = count as u32;
    call.still_there = still_there as u32;
    call.state = state;
    0
}

/// A particle system's update on the running game, in place of the game's.
/// Returns 0 (the system, its particles and the seed written to `call`), or
/// -1 if it can't run (the game then runs its own; nothing was done).
///
/// # Safety
/// `call` must be valid, its particle buffer holding `particle_capacity`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn halo_sim_particle_system_update(call: *mut ParticleSystemUpdateC) -> i32 {
    if call.is_null() {
        return -1;
    }
    // SAFETY: the caller's call
    run(unsafe { &mut *call }, false)
}

/// The check of an update the game ran: Rust's replay of it. Returns 0, or
/// -1.
///
/// # Safety
/// As [`halo_sim_particle_system_update`]; the events hold their count.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn halo_sim_particle_system_update_check(call: *mut ParticleSystemUpdateC) -> i32 {
    if call.is_null() {
        return -1;
    }
    // SAFETY: the caller's call
    run(unsafe { &mut *call }, true)
}

/// What `particle_system_new_unattached` (`attached` 0) or `_attached` is
/// given, and the system and particles it leaves.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct ParticleSystemNewC {
    pub attached: u32,
    pub definition_index: i32,
    pub position: [f32; 3],
    pub velocity: [f32; 3],
    /// alpha, red, green, blue.
    pub color: [f32; 4],
    pub scale: f32,
    pub object_index: i32,
    pub attachment_index: i32,
    /// The seed in (its index NONE); the new system out.
    pub state: ParticleSystemState,
    pub directions: *const [f32; 3],
    pub direction_count: u32,
    /// Its particles out, in the order the game walks them.
    pub particles: *mut SystemParticle,
    pub particle_count: u32,
    pub particle_capacity: u32,
    /// In check mode: the game's calls, and the first that differs.
    pub events: *const ParticleSystemEventC,
    pub event_count: u32,
    pub difference_event: i32,
    pub difference_reason: i32,
    /// Whether the new system is still there after its first update.
    pub still_there: u32,
    /// The new system (NONE for none).
    pub result: i32,
}

/// A host that notes a deletion of the system (its first update can delete
/// it).
struct Noting<'a, H: ParticleSystemHost> {
    host: &'a H,
    deleted: Cell<bool>,
}

impl<H: ParticleSystemHost> ParticleSystemHost for Noting<'_, H> {
    fn call(&self, state: &mut ParticleSystemState, particles: &[SystemParticle], call: &mut ParticleSystemCall) {
        if call.kind == halo_map::particle_systems::call::DELETE {
            self.deleted.set(true);
        }
        self.host.call(state, particles, call);
    }
}

fn run_new(call: &mut ParticleSystemNewC, check: bool) -> i32 {
    let Some(host) = crate::world::host() else { return -1 };
    if call.particles.is_null() {
        return -1;
    }
    let definition = {
        let mut tags = TAGS.lock().unwrap_or_else(|e| e.into_inner());
        let Some(tags) = tags.as_mut() else { return -1 };
        tags.particle_system_definition(call.definition_index as u32)
    };
    let Some(definition) = definition else { return -1 };
    // SAFETY: the game's direction table, direction_count of them
    let directions = if call.directions.is_null() {
        &[][..]
    } else {
        unsafe { std::slice::from_raw_parts(call.directions, call.direction_count as usize) }
    };
    let inputs = ParticleSystemInputs { dt: 0.0, directions };
    let new = if call.attached != 0 {
        NewSystem::Attached { object_index: call.object_index, attachment_index: call.attachment_index as i16 }
    } else {
        NewSystem::Unattached { position: call.position, velocity: call.velocity, color: call.color, scale: call.scale }
    };
    let mut state = call.state;
    let mut particles = Vec::new();
    call.difference_event = -1;
    call.difference_reason = 0;
    let definition_index = call.definition_index;
    let deleted = if check {
        // SAFETY: the game's recording, as described
        let events = if call.events.is_null() || call.event_count == 0 {
            &[][..]
        } else {
            unsafe { std::slice::from_raw_parts(call.events, call.event_count as usize) }
        };
        let checker = CheckHost { events, next: Cell::new(0), difference: Cell::new(None) };
        let noting = Noting { host: &checker, deleted: Cell::new(false) };
        call.result =
            particle_system_new(&definition, definition_index, &new, &inputs, &noting, &mut state, &mut particles);
        if checker.difference.get().is_none() && checker.next.get() < events.len() {
            checker.differ(checker.next.get(), 5);
        }
        (call.difference_event, call.difference_reason) = checker.difference.get().map_or((-1, 0), |(e, r)| (e as i32, r));
        noting.deleted.get()
    } else {
        let live = LiveHost { callbacks: host.particle_systems };
        let noting = Noting { host: &live, deleted: Cell::new(false) };
        call.result =
            particle_system_new(&definition, definition_index, &new, &inputs, &noting, &mut state, &mut particles);
        noting.deleted.get()
    };
    call.still_there = (call.result != -1 && !deleted) as u32;
    let ordered = if call.still_there != 0 {
        particles_in_order(&state, definition.types.len(), &particles)
    } else {
        Vec::new()
    };
    let count = ordered.len().min(call.particle_capacity as usize);
    // SAFETY: the buffer holds particle_capacity
    unsafe { std::ptr::copy_nonoverlapping(ordered.as_ptr(), call.particles, count) };
    call.particle_count = count as u32;
    call.state = state;
    0
}

/// `particle_system_new_*` on the running game, in place of the game's.
/// Returns 0 (`result` the new system; it, its particles and the seed in
/// `call`), or -1 if it can't run (the game then runs its own; nothing was
/// done).
///
/// # Safety
/// `call` must be valid, its particle buffer holding `particle_capacity`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn halo_sim_particle_system_new(call: *mut ParticleSystemNewC) -> i32 {
    if call.is_null() {
        return -1;
    }
    // SAFETY: the caller's call
    run_new(unsafe { &mut *call }, false)
}

/// The check of a system the game made: Rust's replay of it. Returns 0, or
/// -1.
///
/// # Safety
/// As [`halo_sim_particle_system_new`]; the events hold their count.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn halo_sim_particle_system_new_check(call: *mut ParticleSystemNewC) -> i32 {
    if call.is_null() {
        return -1;
    }
    // SAFETY: the caller's call
    run_new(unsafe { &mut *call }, true)
}
