//! Contrail updates (`contrails_update`'s, each contrail's) in Rust, on the
//! running game: a contrail's density, new points, animation, texture scroll
//! and its points' states are `halo_map::contrails`'; what reads the world or
//! allocates (function values, markers, velocity, locations, point physics,
//! point datums, deleting the contrail) is the game's, through contrails.c's
//! `contrail_call` ([`ContrailCallbacks`]). The game hands over the contrail
//! with its points, and writes back the points Rust leaves.
//!
//! In check mode the game's own update runs first, recording each call with
//! the local random seed before and after; Rust replays it, and the game
//! compares the contrail and points Rust leaves with its own.

use std::cell::Cell;
use std::sync::Arc;

use halo_map::contrails::{
    ContrailCall, ContrailDefinition, ContrailHost, ContrailInputs, ContrailPoint, ContrailState, contrail_new,
    contrail_update, points_in_order,
};

use crate::TAGS;

/// The game's side of contrail updates (halo_sim.c).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct ContrailCallbacks {
    /// A call: the seed (and for a deletion the contrail) written to the game
    /// first, the seed read back after.
    pub call: unsafe extern "C" fn(state: *mut ContrailState, call: *mut ContrailCall),
}

/// A call of the game's update, recorded in check mode.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct ContrailEventC {
    pub call: ContrailCall,
    pub seed_before: u32,
    pub seed_after: u32,
}

/// What a contrail's update reads, and where it leaves the contrail.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct ContrailUpdateC {
    pub state: ContrailState,
    pub dt: f32,
    pub point_dt: f32,
    pub directions: *const [f32; 3],
    pub direction_count: u32,
    /// Its points in, in any order; out, in the order the game walks them.
    pub points: *mut ContrailPoint,
    pub point_count: u32,
    pub point_capacity: u32,
    /// In check mode: the game's calls, and the first that differs (-1 for
    /// none) and why (1 its kind, 2 what it was given, 3 the seed before it,
    /// 4 more calls than the game's, 5 fewer).
    pub events: *const ContrailEventC,
    pub event_count: u32,
    pub difference_event: i32,
    pub difference_reason: i32,
    /// Whether the contrail is still there after.
    pub still_there: u32,
}

impl crate::Tags {
    fn contrail_definition(&mut self, datum: u32) -> Option<Arc<ContrailDefinition>> {
        if let Some(definition) = self.contrail_definitions.get(&datum) {
            return definition.clone();
        }
        let definition = ContrailDefinition::load(&self.cache, datum).ok().map(Arc::new);
        self.contrail_definitions.insert(datum, definition.clone());
        definition
    }
}

/// The running game.
struct LiveHost {
    callbacks: ContrailCallbacks,
}

impl ContrailHost for LiveHost {
    fn call(&self, state: &mut ContrailState, call: &mut ContrailCall) {
        // SAFETY: the game's callback
        unsafe { (self.callbacks.call)(state, call) };
    }
}

/// The game's own update, as it recorded it.
struct CheckHost<'a> {
    events: &'a [ContrailEventC],
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

impl ContrailHost for CheckHost<'_> {
    fn call(&self, state: &mut ContrailState, call: &mut ContrailCall) {
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

fn run(call: &mut ContrailUpdateC, check: bool) -> i32 {
    let Some(host) = crate::world::host() else { return -1 };
    let definition = {
        let mut tags = TAGS.lock().unwrap_or_else(|e| e.into_inner());
        let Some(tags) = tags.as_mut() else { return -1 };
        tags.contrail_definition(call.state.definition_index)
    };
    let Some(definition) = definition else { return -1 };
    if call.points.is_null() {
        return -1;
    }
    // SAFETY: the game's direction table, direction_count of them
    let directions = if call.directions.is_null() {
        &[][..]
    } else {
        unsafe { std::slice::from_raw_parts(call.directions, call.direction_count as usize) }
    };
    // SAFETY: the contrail's points, point_count of them in a buffer of
    // point_capacity
    let mut points = unsafe { std::slice::from_raw_parts(call.points, call.point_count as usize) }.to_vec();
    let inputs = ContrailInputs { dt: call.dt, point_dt: call.point_dt, directions };
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
        let still_there = contrail_update(&definition, &inputs, &checker, &mut state, &mut points);
        if checker.difference.get().is_none() && checker.next.get() < events.len() {
            checker.differ(checker.next.get(), 5);
        }
        (call.difference_event, call.difference_reason) = checker.difference.get().map_or((-1, 0), |(e, r)| (e as i32, r));
        still_there
    } else {
        let live = LiveHost { callbacks: host.contrails };
        contrail_update(&definition, &inputs, &live, &mut state, &mut points)
    };
    let ordered = if still_there { points_in_order(&state, &points) } else { Vec::new() };
    let count = ordered.len().min(call.point_capacity as usize);
    // SAFETY: the buffer holds point_capacity
    unsafe { std::ptr::copy_nonoverlapping(ordered.as_ptr(), call.points, count) };
    call.point_count = count as u32;
    call.still_there = still_there as u32;
    call.state = state;
    0
}

/// A contrail's update on the running game, in place of the game's. Returns
/// 0 (the contrail, its points and the seed written to `call`), or -1 if it
/// can't run (the game then runs its own; nothing was done).
///
/// # Safety
/// `call` must be valid, its points buffer holding `point_capacity`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn halo_sim_contrail_update(call: *mut ContrailUpdateC) -> i32 {
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
/// As [`halo_sim_contrail_update`]; the events hold their count.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn halo_sim_contrail_update_check(call: *mut ContrailUpdateC) -> i32 {
    if call.is_null() {
        return -1;
    }
    // SAFETY: the caller's call
    run(unsafe { &mut *call }, true)
}

/// What `contrail_new` is given, and the contrail and points it leaves.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct ContrailNewC {
    pub definition_index: i32,
    pub object_index: i32,
    pub attachment_index: i32,
    /// The seed in (its index NONE); the new contrail out.
    pub state: ContrailState,
    pub directions: *const [f32; 3],
    pub direction_count: u32,
    /// Its points out, in the order the game walks them.
    pub points: *mut ContrailPoint,
    pub point_count: u32,
    pub point_capacity: u32,
    /// In check mode: the game's calls, and the first that differs.
    pub events: *const ContrailEventC,
    pub event_count: u32,
    pub difference_event: i32,
    pub difference_reason: i32,
    /// The new contrail (NONE for none).
    pub result: i32,
}

fn run_new(call: &mut ContrailNewC, check: bool) -> i32 {
    let Some(host) = crate::world::host() else { return -1 };
    if call.definition_index == -1 || call.points.is_null() {
        return -1;
    }
    let definition = {
        let mut tags = TAGS.lock().unwrap_or_else(|e| e.into_inner());
        let Some(tags) = tags.as_mut() else { return -1 };
        tags.contrail_definition(call.definition_index as u32)
    };
    let Some(definition) = definition else { return -1 };
    // SAFETY: the game's direction table, direction_count of them
    let directions = if call.directions.is_null() {
        &[][..]
    } else {
        unsafe { std::slice::from_raw_parts(call.directions, call.direction_count as usize) }
    };
    let inputs = ContrailInputs { dt: 0.0, point_dt: 0.0, directions };
    let mut state = call.state;
    let mut points = Vec::new();
    call.difference_event = -1;
    call.difference_reason = 0;
    let (definition_index, object_index, attachment_index) =
        (call.definition_index, call.object_index, call.attachment_index as i16);
    if check {
        // SAFETY: the game's recording, as described
        let events = if call.events.is_null() || call.event_count == 0 {
            &[][..]
        } else {
            unsafe { std::slice::from_raw_parts(call.events, call.event_count as usize) }
        };
        let checker = CheckHost { events, next: Cell::new(0), difference: Cell::new(None) };
        call.result = contrail_new(
            &definition,
            definition_index,
            object_index,
            attachment_index,
            &inputs,
            &checker,
            &mut state,
            &mut points,
        );
        if checker.difference.get().is_none() && checker.next.get() < events.len() {
            checker.differ(checker.next.get(), 5);
        }
        (call.difference_event, call.difference_reason) = checker.difference.get().map_or((-1, 0), |(e, r)| (e as i32, r));
    } else {
        let live = LiveHost { callbacks: host.contrails };
        call.result = contrail_new(
            &definition,
            definition_index,
            object_index,
            attachment_index,
            &inputs,
            &live,
            &mut state,
            &mut points,
        );
    }
    let ordered = if call.result != -1 { points_in_order(&state, &points) } else { Vec::new() };
    let count = ordered.len().min(call.point_capacity as usize);
    // SAFETY: the buffer holds point_capacity
    unsafe { std::ptr::copy_nonoverlapping(ordered.as_ptr(), call.points, count) };
    call.point_count = count as u32;
    call.state = state;
    0
}

/// `contrail_new` on the running game, in place of the game's. Returns 0
/// (`result` the new contrail; it, its points and the seed in `call`), or -1
/// if it can't run (the game then runs its own; nothing was done).
///
/// # Safety
/// `call` must be valid, its points buffer holding `point_capacity`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn halo_sim_contrail_new(call: *mut ContrailNewC) -> i32 {
    if call.is_null() {
        return -1;
    }
    // SAFETY: the caller's call
    run_new(unsafe { &mut *call }, false)
}

/// The check of a contrail the game made: Rust's replay of it. Returns 0,
/// or -1.
///
/// # Safety
/// As [`halo_sim_contrail_new`]; the events hold their count.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn halo_sim_contrail_new_check(call: *mut ContrailNewC) -> i32 {
    if call.is_null() {
        return -1;
    }
    // SAFETY: the caller's call
    run_new(unsafe { &mut *call }, true)
}
