//! The AI's updates over the game's arrays rather than one actor (the
//! encounters' and the conversations' updates), on the running game: each works on the arrays in
//! the game's memory, in place, and asks the rest of the game through its
//! call function in the C.
//!
//! In check mode the game's own update runs first, recording each call with
//! a checksum of the arrays before it and the bytes it changed; the game
//! then puts the arrays back as the update began, Rust replays the update
//! on them (each call's changes applied as the game's made them), and the
//! game compares the arrays Rust leaves with its own.

use std::cell::Cell;

use halo_map::ai_actor::{AiCall, AiHost, AiMemory, AiStepContext, WorldContext};
use halo_map::communication::ai_communication_event;
use halo_map::conversations::ai_conversation_update;
use halo_map::encounters::encounters_update;

use crate::ai_steps::GameMemory;

/// halo_sim.h's `struct halo_sim_world_region`: memory a world step works on.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct WorldRegionC {
    pub address: u32,
    pub size: u32,
}

/// A call of the game's update, recorded in check mode: the arrays'
/// checksum before it ([`world_checksum`]) and its runs of changed bytes.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct WorldEventC {
    pub call: AiCall,
    pub checksum: u32,
    pub run_first: u32,
    pub run_count: u32,
}

/// Bytes a call changed: at `offset` into the regions laid end to end,
/// their new values at `data` in the pool.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct WorldRunC {
    pub offset: u32,
    pub size: u32,
    pub data: u32,
}

/// A world step: where it works, its call function, and in check mode the
/// game's calls and the first that differs (-1 for none) and why (1 its
/// kind, 2 its arguments, 3 the arrays before it, 4 more calls than the
/// game's, 5 fewer).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct WorldStepC {
    pub context: WorldContext,
    pub call: Option<unsafe extern "C" fn(call: *mut AiCall)>,
    pub regions: *const WorldRegionC,
    pub region_count: u32,
    pub events: *const WorldEventC,
    pub event_count: u32,
    pub runs: *const WorldRunC,
    pub run_count: u32,
    pub pool: *const u8,
    pub difference_event: i32,
    pub difference_reason: i32,
}

/// halo_sim.c's `halo_sim_world_checksum`: FNV-1a over the regions end to
/// end.
pub(crate) fn world_checksum(regions: &[WorldRegionC]) -> u32 {
    let mut hash = 2166136261u32;
    for region in regions {
        // SAFETY: the game's memory the region names
        let bytes = unsafe { std::slice::from_raw_parts(region.address as usize as *const u8, region.size as usize) };
        for &byte in bytes {
            hash ^= byte as u32;
            hash = hash.wrapping_mul(16777619);
        }
    }
    hash
}

struct LiveHost {
    call: unsafe extern "C" fn(call: *mut AiCall),
}

impl AiHost for LiveHost {
    fn memory(&self) -> &dyn AiMemory {
        &GameMemory
    }
    fn call(&self, call: &mut AiCall) {
        // SAFETY: the game's callback
        unsafe { (self.call)(call) };
    }
    fn structure_test_vector(&self, _: &AiStepContext, _: bool, _: u32, _: [f32; 3], _: [f32; 3], _: f32) -> Option<(f32, i32)> {
        None
    }
}

struct CheckHost<'a> {
    regions: &'a [WorldRegionC],
    events: &'a [WorldEventC],
    runs: &'a [WorldRunC],
    pool: *const u8,
    next: Cell<usize>,
    difference: Cell<Option<(usize, i32)>>,
}

impl CheckHost<'_> {
    fn differ(&self, event: usize, reason: i32) {
        if self.difference.get().is_none() {
            self.difference.set(Some((event, reason)));
        }
    }

    /// Writes bytes at an offset into the regions laid end to end.
    fn write(&self, offset: u32, bytes: &[u8]) {
        world_write(self.regions, offset, bytes);
    }
}

/// Writes bytes at an offset into the regions laid end to end.
pub(crate) fn world_write(regions: &[WorldRegionC], mut offset: u32, bytes: &[u8]) {
    for region in regions {
        if offset < region.size {
            // SAFETY: the game's memory the region names, within it
            unsafe {
                std::ptr::copy_nonoverlapping(
                    bytes.as_ptr(),
                    (region.address + offset) as usize as *mut u8,
                    bytes.len().min((region.size - offset) as usize),
                )
            };
            return;
        }
        offset -= region.size;
    }
}

impl AiHost for CheckHost<'_> {
    fn memory(&self) -> &dyn AiMemory {
        &GameMemory
    }
    fn structure_test_vector(&self, _: &AiStepContext, _: bool, _: u32, _: [f32; 3], _: [f32; 3], _: f32) -> Option<(f32, i32)> {
        None
    }
    fn call(&self, call: &mut AiCall) {
        let index = self.next.get();
        let Some(event) = self.events.get(index) else {
            self.differ(index, 4);
            return;
        };
        self.next.set(index + 1);
        let e = &event.call;
        if e.kind != call.kind {
            self.differ(index, 1);
        } else if e.actor != call.actor
            || e.a != call.a
            || e.b != call.b
            || e.input.iter().zip(call.input.iter()).any(|(x, y)| x.to_bits() != y.to_bits())
        {
            self.differ(index, 2);
        }
        if world_checksum(self.regions) != event.checksum {
            self.differ(index, 3);
        }
        let first = event.run_first as usize;
        for run in self.runs.get(first..first + event.run_count as usize).unwrap_or(&[]) {
            // SAFETY: the run's bytes in the game's pool
            let bytes = unsafe { std::slice::from_raw_parts(self.pool.add(run.data as usize), run.size as usize) };
            self.write(run.offset, bytes);
        }
        call.output = e.output;
        call.result = e.result;
        call.result2 = e.result2;
    }
}

/// halo_sim.h's world steps.
const STEP_ENCOUNTERS: i32 = 0;
const STEP_CONVERSATIONS: i32 = 1;
const STEP_COMMUNICATION: i32 = 2;
const STEP_NETWORK_APPLY_STATE: i32 = 3;
const STEP_NETWORK_OBJECT_CORRECT: i32 = 4;
const STEP_NETWORK_OBJECT_RECONCILE: i32 = 5;
const STEP_NETWORK_CORRECT_OWN_UNIT: i32 = 6;
const STEP_NETWORK_UNIT_STATE: i32 = 7;
const STEP_NETWORK_OBJECT_STATE: i32 = 8;
const STEP_NETWORK_INVENTORY: i32 = 9;
const STEP_NETWORK_OBJECT_CHANGE: i32 = 10;
const STEP_NETWORK_HIT_REPORT: i32 = 11;
const STEP_NETWORK_DAMAGE_BOOKKEEPING: i32 = 12;
const STEP_NETWORK_DAMAGE_REPLAY: i32 = 13;
const STEP_NETWORK_DAMAGE_DEALS: i32 = 14;
const STEP_NETWORK_DAMAGE_PLAYER_EFFECT: i32 = 15;
const STEP_NETWORK_DAMAGE_AFTERMATH: i32 = 16;
const STEP_NETWORK_HOST_TICK: i32 = 17;

fn run(step: i32, context: &WorldContext, host: &impl AiHost) -> Option<()> {
    match step {
        STEP_ENCOUNTERS => encounters_update(context, host),
        STEP_CONVERSATIONS => ai_conversation_update(context, host),
        STEP_COMMUNICATION => ai_communication_event(context, host),
        STEP_NETWORK_APPLY_STATE => halo_map::network_apply::apply_state(context, host),
        STEP_NETWORK_OBJECT_CORRECT => halo_map::network_apply::object_correct(context, host),
        STEP_NETWORK_OBJECT_RECONCILE => halo_map::network_apply::object_reconcile(context, host),
        STEP_NETWORK_CORRECT_OWN_UNIT => halo_map::network_apply::correct_own_unit(context, host),
        STEP_NETWORK_UNIT_STATE => halo_map::network_apply::handle_unit_state(context, host),
        STEP_NETWORK_OBJECT_STATE => halo_map::network_apply::handle_object_state(context, host),
        STEP_NETWORK_INVENTORY => halo_map::network_apply::apply_inventory(context, host),
        STEP_NETWORK_OBJECT_CHANGE => halo_map::network_apply::handle_change(context, host),
        STEP_NETWORK_HIT_REPORT => halo_map::network_damage::handle_report(context, host),
        STEP_NETWORK_DAMAGE_BOOKKEEPING => halo_map::network_damage::bookkeeping(context, host),
        STEP_NETWORK_DAMAGE_REPLAY => halo_map::network_damage::replay(context, host),
        STEP_NETWORK_DAMAGE_DEALS => halo_map::network_damage::deals(context, host),
        STEP_NETWORK_DAMAGE_PLAYER_EFFECT => halo_map::network_damage::player_effect(context, host),
        STEP_NETWORK_DAMAGE_AFTERMATH => halo_map::network_damage::aftermath(context, host),
        STEP_NETWORK_HOST_TICK => halo_map::network_host::host_tick(context, host),
        _ => return None,
    }
    Some(())
}

/// An array of `count` elements at `pointer` (none when it is null).
///
/// # Safety
/// `pointer` must hold `count` elements for the slice's life.
pub(crate) unsafe fn slice_of<'a, T>(pointer: *const T, count: u32) -> &'a [T] {
    if pointer.is_null() || count == 0 {
        &[]
    } else {
        // SAFETY: as the caller promises
        unsafe { std::slice::from_raw_parts(pointer, count as usize) }
    }
}

/// A world step on the game. Returns 0, or -1 for a step it doesn't know.
///
/// # Safety
/// `step_call` must be valid, its context and call function the game's.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn halo_sim_world_step(step: i32, step_call: *mut WorldStepC) -> i32 {
    if step_call.is_null() {
        return -1;
    }
    // SAFETY: the caller's step
    let step_call = unsafe { &mut *step_call };
    let Some(call) = step_call.call else { return -1 };
    let host = LiveHost { call };
    match run(step, &step_call.context, &host) {
        Some(()) => 0,
        None => -1,
    }
}

/// A world step replayed on the game's recorded calls (check mode).
/// Returns 0, or -1 for a step it doesn't know.
///
/// # Safety
/// `step_call` must be valid: its regions the game's memory, its events,
/// runs and pool the game's recording.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn halo_sim_world_step_check(step: i32, step_call: *mut WorldStepC) -> i32 {
    if step_call.is_null() {
        return -1;
    }
    // SAFETY: the caller's step
    let step_call = unsafe { &mut *step_call };
    // SAFETY: the game's recording: arrays of the counts it gives
    let (regions, events, runs) = unsafe {
        (
            slice_of(step_call.regions, step_call.region_count),
            slice_of(step_call.events, step_call.event_count),
            slice_of(step_call.runs, step_call.run_count),
        )
    };
    let host = CheckHost {
        regions,
        events,
        runs,
        pool: step_call.pool,
        next: Cell::new(0),
        difference: Cell::new(None),
    };
    if run(step, &step_call.context, &host).is_none() {
        return -1;
    }
    if host.difference.get().is_none() && host.next.get() < events.len() {
        host.differ(host.next.get(), 5);
    }
    let (event, reason) = host.difference.get().map_or((-1, 0), |(event, reason)| (event as i32, reason));
    step_call.difference_event = event;
    step_call.difference_reason = reason;
    0
}
