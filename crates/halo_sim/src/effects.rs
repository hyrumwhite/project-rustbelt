//! Effect updates (`effect_update`) in Rust, on the running game: an
//! effect's attachment, visibility, events, parts and particles are
//! `halo_map::effects`'; what they do to the rest of the game (the objects it
//! reads, visibility and water, making particle systems, sounds, objects,
//! decals, damage, lights and particles, deleting it) is the game's, through
//! effects.c's `effect_call` ([`EffectCallbacks`]).
//!
//! In check mode the game's own update runs first, recording each call with
//! the effect and both random seeds before and after; Rust replays it from
//! the effect and seeds as they were.

use std::cell::Cell;
use std::sync::Arc;

use halo_map::effects::{
    EffectCall, EffectDefinition, EffectHost, EffectInputs, EffectInstance, EffectLocations, EffectState,
    MAXIMUM_EFFECT_LOCATIONS, effect_update,
};

use crate::TAGS;

/// The game's side of effect updates (halo_sim.c).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct EffectCallbacks {
    /// A call: the effect and seeds written to the game first, read back
    /// after; 1 if the effect is still there.
    pub call: unsafe extern "C" fn(state: *mut EffectState, call: *mut EffectCall) -> i32,
}

/// A call of the game's update, recorded in check mode.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct EffectEventC {
    pub call: EffectCall,
    pub still_there: u32,
    pub before: EffectState,
    pub after: EffectState,
}

/// What an effect's update reads, and where it leaves the effect.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct EffectUpdateC {
    pub state: EffectState,
    pub dt: f32,
    pub local_player_count: i32,
    /// `random_direction_table_get`'s table.
    pub directions: *const [f32; 3],
    pub direction_count: u32,
    /// Each location's first instance (NONE for none), and the instances.
    pub first_instances: [i32; MAXIMUM_EFFECT_LOCATIONS],
    pub instances: *const EffectInstance,
    pub instance_count: u32,
    /// In check mode: the game's calls.
    pub events: *const EffectEventC,
    pub event_count: u32,
    /// In check mode: the first call that differs (-1 for none) and why
    /// (1 its kind, 2 what it was given, 3 the effect or seeds before it,
    /// 4 more calls than the game's, 5 fewer); the effect Rust leaves is
    /// compared by the game.
    pub difference_event: i32,
    pub difference_reason: i32,
    /// Whether the effect is still there after.
    pub still_there: u32,
}

impl crate::Tags {
    fn effect_definition(&mut self, datum: u32) -> Option<Arc<EffectDefinition>> {
        if let Some(definition) = self.effect_definitions.get(&datum) {
            return definition.clone();
        }
        let definition = EffectDefinition::load(&self.cache, datum).ok().map(Arc::new);
        self.effect_definitions.insert(datum, definition.clone());
        definition
    }
}

fn state_bytes(state: &EffectState) -> &[u8] {
    // SAFETY: EffectState is repr(C) plain data without padding (its size is
    // asserted)
    unsafe { std::slice::from_raw_parts((state as *const EffectState).cast::<u8>(), size_of::<EffectState>()) }
}

/// The running game.
struct LiveHost {
    callbacks: EffectCallbacks,
    still_there: Cell<bool>,
}

impl EffectHost for LiveHost {
    fn call(&self, state: &mut EffectState, call: &mut EffectCall) {
        // SAFETY: the game's callback
        if unsafe { (self.callbacks.call)(state, call) } == 0 {
            self.still_there.set(false);
        }
    }
}

/// The game's own update, as it recorded it.
struct CheckHost<'a> {
    events: &'a [EffectEventC],
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

impl EffectHost for CheckHost<'_> {
    fn call(&self, state: &mut EffectState, call: &mut EffectCall) {
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
        call.out_location_leaf = e.out_location_leaf;
        call.out_location_cluster = e.out_location_cluster;
        call.out_location_bonus = e.out_location_bonus;
        if event.still_there == 0 {
            self.still_there.set(false);
        }
    }
}

fn run(call: &mut EffectUpdateC, check: bool) -> i32 {
    let Some(host) = crate::world::host() else { return -1 };
    let definition = {
        let mut tags = TAGS.lock().unwrap_or_else(|e| e.into_inner());
        let Some(tags) = tags.as_mut() else { return -1 };
        tags.effect_definition(call.state.definition_index)
    };
    let Some(definition) = definition else { return -1 };
    // SAFETY: the game's direction table, direction_count of them
    let directions = if call.directions.is_null() {
        &[][..]
    } else {
        unsafe { std::slice::from_raw_parts(call.directions, call.direction_count as usize) }
    };
    // SAFETY: the effect's instances, instance_count of them
    let instances = if call.instances.is_null() {
        &[][..]
    } else {
        unsafe { std::slice::from_raw_parts(call.instances, call.instance_count as usize) }
    };
    let inputs = EffectInputs { local_player_count: call.local_player_count as i16, directions };
    let locations = EffectLocations { first: call.first_instances, instances };
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
        effect_update(&definition, &inputs, &locations, &checker, &mut state, call.dt);
        if checker.difference.get().is_none() && checker.next.get() < events.len() {
            checker.differ(checker.next.get(), 5);
        }
        (call.difference_event, call.difference_reason) = checker.difference.get().map_or((-1, 0), |(e, r)| (e as i32, r));
        call.still_there = checker.still_there.get() as u32;
    } else {
        let live = LiveHost { callbacks: host.effects, still_there: Cell::new(true) };
        effect_update(&definition, &inputs, &locations, &live, &mut state, call.dt);
        call.still_there = live.still_there.get() as u32;
    }
    call.state = state;
    0
}

/// An effect's update on the running game, in place of the game's. Returns 0
/// (the effect and seeds written to `call`), or -1 if it can't run (the game
/// then runs its own; nothing was done).
///
/// # Safety
/// `call` must be valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn halo_sim_effect_update(call: *mut EffectUpdateC) -> i32 {
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
/// As [`halo_sim_effect_update`]; the events hold their count.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn halo_sim_effect_update_check(call: *mut EffectUpdateC) -> i32 {
    if call.is_null() {
        return -1;
    }
    // SAFETY: the caller's call
    run(unsafe { &mut *call }, true)
}
