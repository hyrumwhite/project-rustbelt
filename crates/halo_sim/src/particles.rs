//! Particle updates (`particles_update`'s, each particle's) in Rust, on the
//! running game: a particle's age, death, animation and physics are
//! `halo_map::particles`'; what they do to the rest of the game (deleting it,
//! its effects, the material effect of its impact, the object it is attached
//! to, its point physics) is the game's, through particles.c's
//! `particle_call` ([`ParticleCallbacks`]).
//!
//! In check mode the game's own update runs first, recording each call with
//! the particle and the local random seed before and after; Rust replays it
//! from the particle and seed as they were.

use std::cell::Cell;
use std::sync::Arc;

use halo_map::particles::{
    NewParticleData, ParticleCall, ParticleDefinition, ParticleHost, ParticleInputs, ParticleState, particle_new,
    particle_update,
};

use crate::TAGS;

/// The game's side of particle updates (halo_sim.c).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct ParticleCallbacks {
    /// A call: the particle and seed written to the game first, read back
    /// after; 1 if the particle is still there.
    pub call: unsafe extern "C" fn(state: *mut ParticleState, call: *mut ParticleCall) -> i32,
}

/// A call of the game's update, recorded in check mode.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct ParticleEventC {
    pub call: ParticleCall,
    pub still_there: u32,
    pub before: ParticleState,
    pub after: ParticleState,
}

/// What a particle's update reads, and where it leaves the particle.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct ParticleUpdateC {
    pub state: ParticleState,
    pub dt: f32,
    pub frame_index: i32,
    pub oldest_tick_frame_index: i32,
    pub update_ticks: i32,
    /// In check mode: the game's calls, and the first that differs (-1 for
    /// none) and why (1 its kind, 2 what it was given, 3 the particle or seed
    /// before it, 4 more calls than the game's, 5 fewer).
    pub events: *const ParticleEventC,
    pub event_count: u32,
    pub difference_event: i32,
    pub difference_reason: i32,
    /// Whether the particle is still there after.
    pub still_there: u32,
}

impl crate::Tags {
    fn particle_definition(&mut self, datum: u32) -> Option<Arc<ParticleDefinition>> {
        if let Some(definition) = self.particle_definitions.get(&datum) {
            return definition.clone();
        }
        let definition = ParticleDefinition::load(&self.cache, datum).ok().map(Arc::new);
        self.particle_definitions.insert(datum, definition.clone());
        definition
    }
}

fn state_bytes(state: &ParticleState) -> &[u8] {
    // SAFETY: ParticleState is repr(C) plain data without padding (its size
    // is asserted)
    unsafe { std::slice::from_raw_parts((state as *const ParticleState).cast::<u8>(), size_of::<ParticleState>()) }
}

/// The running game.
struct LiveHost {
    callbacks: ParticleCallbacks,
    still_there: Cell<bool>,
}

impl ParticleHost for LiveHost {
    fn call(&self, state: &mut ParticleState, call: &mut ParticleCall) {
        // SAFETY: the game's callback
        if unsafe { (self.callbacks.call)(state, call) } == 0 {
            self.still_there.set(false);
        }
    }
}

/// The game's own update, as it recorded it.
struct CheckHost<'a> {
    events: &'a [ParticleEventC],
    next: Cell<usize>,
    still_there: Cell<bool>,
    difference: Cell<Option<(usize, i32)>>,
}

impl CheckHost<'_> {
    fn differ(&self, event: usize, reason: i32) {
        if self.difference.get().is_none() {
            self.difference.set(Some((event, reason)));
        }
    }
}

impl ParticleHost for CheckHost<'_> {
    fn call(&self, state: &mut ParticleState, call: &mut ParticleCall) {
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
        } else if state_bytes(&event.before) != state_bytes(state) {
            self.differ(index, 3);
        }
        *state = event.after;
        call.result = e.result;
        call.out = e.out;
        call.out_material_type = e.out_material_type;
        call.out_location_leaf = e.out_location_leaf;
        call.out_location_cluster = e.out_location_cluster;
        call.out_location_bonus = e.out_location_bonus;
        if event.still_there == 0 {
            self.still_there.set(false);
        }
    }
}

fn run(call: &mut ParticleUpdateC, check: bool) -> i32 {
    let Some(host) = crate::world::host() else { return -1 };
    let definition = {
        let mut tags = TAGS.lock().unwrap_or_else(|e| e.into_inner());
        let Some(tags) = tags.as_mut() else { return -1 };
        tags.particle_definition(call.state.definition_index)
    };
    let Some(definition) = definition else { return -1 };
    let inputs = ParticleInputs {
        frame_index: call.frame_index,
        oldest_tick_frame_index: call.oldest_tick_frame_index,
        update_ticks: call.update_ticks,
        local_player_count: 1,
    };
    let mut state = call.state;
    call.difference_event = -1;
    call.difference_reason = 0;
    if check {
        // SAFETY: the game's recording, as described
        let events = if call.events.is_null() || call.event_count == 0 {
            &[][..]
        } else {
            unsafe { std::slice::from_raw_parts(call.events, call.event_count as usize) }
        };
        let checker = CheckHost { events, next: Cell::new(0), still_there: Cell::new(true), difference: Cell::new(None) };
        particle_update(&definition, &inputs, &checker, &mut state, call.dt);
        if checker.difference.get().is_none() && checker.next.get() < events.len() {
            checker.differ(checker.next.get(), 5);
        }
        (call.difference_event, call.difference_reason) = checker.difference.get().map_or((-1, 0), |(e, r)| (e as i32, r));
        call.still_there = checker.still_there.get() as u32;
    } else {
        let live = LiveHost { callbacks: host.particles, still_there: Cell::new(true) };
        particle_update(&definition, &inputs, &live, &mut state, call.dt);
        call.still_there = live.still_there.get() as u32;
    }
    call.state = state;
    0
}

/// A particle's update on the running game, in place of the game's.
/// Returns 0 (the particle and seed written to `call`), or -1 if it can't
/// run (the game then runs its own; nothing was done).
///
/// # Safety
/// `call` must be valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn halo_sim_particle_update(call: *mut ParticleUpdateC) -> i32 {
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
/// As [`halo_sim_particle_update`]; the events hold their count.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn halo_sim_particle_update_check(call: *mut ParticleUpdateC) -> i32 {
    if call.is_null() {
        return -1;
    }
    // SAFETY: the caller's call
    run(unsafe { &mut *call }, true)
}

/// What `particle_new` is given, and the particle it leaves.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct ParticleNewC {
    pub data: NewParticleData,
    /// The seed in (its index NONE); the new particle out.
    pub state: ParticleState,
    pub frame_index: i32,
    pub local_player_count: i32,
    /// In check mode: the game's calls, and the first that differs (as
    /// [`ParticleUpdateC`]'s).
    pub events: *const ParticleEventC,
    pub event_count: u32,
    pub difference_event: i32,
    pub difference_reason: i32,
    /// Whether the new particle is still there after.
    pub still_there: u32,
    /// The new particle (NONE for none).
    pub result: i32,
}

fn run_new(call: &mut ParticleNewC, check: bool) -> i32 {
    let Some(host) = crate::world::host() else { return -1 };
    let definition = if call.data.definition_index == -1 {
        None
    } else {
        let mut tags = TAGS.lock().unwrap_or_else(|e| e.into_inner());
        let Some(tags) = tags.as_mut() else { return -1 };
        // (a definition Rust can't read is left to the game)
        let Some(definition) = tags.particle_definition(call.data.definition_index as u32) else { return -1 };
        Some(definition)
    };
    let inputs = ParticleInputs {
        frame_index: call.frame_index,
        oldest_tick_frame_index: 0,
        update_ticks: 0,
        local_player_count: call.local_player_count as i16,
    };
    let data = call.data;
    let mut state = call.state;
    call.difference_event = -1;
    call.difference_reason = 0;
    if check {
        // SAFETY: the game's recording, as described
        let events = if call.events.is_null() || call.event_count == 0 {
            &[][..]
        } else {
            unsafe { std::slice::from_raw_parts(call.events, call.event_count as usize) }
        };
        let checker = CheckHost { events, next: Cell::new(0), still_there: Cell::new(true), difference: Cell::new(None) };
        call.result = particle_new(definition.as_deref(), &inputs, &checker, &mut state, &data);
        if checker.difference.get().is_none() && checker.next.get() < events.len() {
            checker.differ(checker.next.get(), 5);
        }
        (call.difference_event, call.difference_reason) = checker.difference.get().map_or((-1, 0), |(e, r)| (e as i32, r));
        call.still_there = checker.still_there.get() as u32;
    } else {
        let live = LiveHost { callbacks: host.particles, still_there: Cell::new(true) };
        call.result = particle_new(definition.as_deref(), &inputs, &live, &mut state, &data);
        call.still_there = live.still_there.get() as u32;
    }
    call.state = state;
    0
}

/// `particle_new` on the running game, in place of the game's. Returns 0
/// (`result` the new particle, it and the seed in `state`), or -1 if it
/// can't run (the game then runs its own; nothing was done).
///
/// # Safety
/// `call` must be valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn halo_sim_particle_new(call: *mut ParticleNewC) -> i32 {
    if call.is_null() {
        return -1;
    }
    // SAFETY: the caller's call
    run_new(unsafe { &mut *call }, false)
}

/// The check of a particle the game made: Rust's replay of it. Returns 0,
/// or -1.
///
/// # Safety
/// As [`halo_sim_particle_new`]; the events hold their count.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn halo_sim_particle_new_check(call: *mut ParticleNewC) -> i32 {
    if call.is_null() {
        return -1;
    }
    // SAFETY: the caller's call
    run_new(unsafe { &mut *call }, true)
}
