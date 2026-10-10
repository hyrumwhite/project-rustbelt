//! An object's damage over time (`object_damage_update`) in Rust, on the
//! running game: the shield's over-charge and recharge and the damage's
//! decay are `halo_map::damage_update`'s; an act of god's death, the HUD's
//! shield tick, the difficulty and the recharging effect are the game's,
//! through damage.c's `damage_update_call` ([`DamageUpdateCallbacks`]).
//!
//! In check mode the game's own update runs first, recording each call with
//! the object before and after; Rust replays it from the object as it was.

use std::cell::Cell;

use halo_map::damage::ObjectDamageState;
use halo_map::damage_update::{DamageUpdateHost, DamageUpdateInputs, object_damage_update};

use crate::TAGS;

/// The game's side of the update (halo_sim.c).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct DamageUpdateCallbacks {
    /// A call: the object written to the game first, read back after.
    pub call: unsafe extern "C" fn(state: *mut ObjectDamageState, kind: i32, f: f32, out: *mut f32) -> i32,
}

/// A call of the game's update, recorded in check mode.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct DamageUpdateEventC {
    pub kind: i32,
    pub f: f32,
    pub result: i32,
    pub out: f32,
    pub before: ObjectDamageState,
    pub after: ObjectDamageState,
}

/// What the update reads, and where it leaves the object.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct DamageUpdateCallC {
    pub state: ObjectDamageState,
    pub update_shields: u32,
    pub game_engine_running: u32,
    /// In check mode: the game's calls.
    pub events: *const DamageUpdateEventC,
    pub event_count: u32,
    /// In check mode: the first call that differs (-1 for none) and why
    /// (1 its kind, 2 what it was given, 3 the object before it, 4 more
    /// calls than the game's, 5 fewer); the object Rust leaves is compared by
    /// the game.
    pub difference_event: i32,
    pub difference_reason: i32,
    /// Whether the object is still there after (an act of god's death can
    /// delete it).
    pub still_there: u32,
}

/// The running game.
struct LiveHost {
    callbacks: DamageUpdateCallbacks,
    still_there: Cell<bool>,
}

impl DamageUpdateHost for LiveHost {
    fn call(&self, state: &mut ObjectDamageState, kind: i32, f: f32) -> (i32, f32) {
        let mut out = 0.0;
        // SAFETY: the game's callback
        let result = unsafe { (self.callbacks.call)(state, kind, f, &mut out) };
        if result < 0 {
            self.still_there.set(false);
        }
        (result, out)
    }
}

/// The game's own update, as it recorded it.
struct CheckHost<'a> {
    events: &'a [DamageUpdateEventC],
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

impl DamageUpdateHost for CheckHost<'_> {
    fn call(&self, state: &mut ObjectDamageState, kind: i32, f: f32) -> (i32, f32) {
        let index = self.next.get();
        let Some(event) = self.events.get(index) else {
            self.differ(index, 4);
            return (0, 0.0);
        };
        self.next.set(index + 1);
        if event.kind != kind {
            self.differ(index, 1);
        } else if event.f.to_bits() != f.to_bits() {
            self.differ(index, 2);
        } else if event.before != *state {
            self.differ(index, 3);
        }
        *state = event.after;
        (event.result, event.out)
    }
}

fn run(call: &mut DamageUpdateCallC, check: bool) -> i32 {
    let Some(host) = crate::world::host() else { return -1 };
    let resistance = {
        let mut tags = TAGS.lock().unwrap_or_else(|e| e.into_inner());
        let Some(tags) = tags.as_mut() else { return -1 };
        tags.damage_resistance(call.state.definition_index)
    };
    let Some(resistance) = resistance else { return -1 };
    let inputs = DamageUpdateInputs {
        update_shields: call.update_shields != 0,
        game_engine_running: call.game_engine_running != 0,
        shield_recharge_velocity: resistance.runtime_shield_recharge_velocity,
    };
    let mut state = call.state;
    call.difference_event = -1;
    call.difference_reason = 0;
    call.still_there = 1;
    if check {
        // SAFETY: the game's recording, as described
        let events = if call.events.is_null() || call.event_count == 0 {
            &[][..]
        } else {
            unsafe { std::slice::from_raw_parts(call.events, call.event_count as usize) }
        };
        let checker = CheckHost { events, next: Cell::new(0), difference: Cell::new(None) };
        object_damage_update(&inputs, &checker, &mut state);
        if checker.difference.get().is_none() && checker.next.get() < events.len() {
            checker.differ(checker.next.get(), 5);
        }
        (call.difference_event, call.difference_reason) = checker.difference.get().map_or((-1, 0), |(e, r)| (e as i32, r));
    } else {
        let live = LiveHost { callbacks: host.damage_update, still_there: Cell::new(true) };
        object_damage_update(&inputs, &live, &mut state);
        call.still_there = live.still_there.get() as u32;
    }
    call.state = state;
    0
}

/// An object's damage update on the running game, in place of the game's.
/// Returns 0 (the object's state written), or -1 if it can't run (the game
/// then runs its own; nothing was done).
///
/// # Safety
/// `call` must be valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn halo_sim_object_damage_update(call: *mut DamageUpdateCallC) -> i32 {
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
/// As [`halo_sim_object_damage_update`]; the events hold their count.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn halo_sim_object_damage_update_check(call: *mut DamageUpdateCallC) -> i32 {
    if call.is_null() {
        return -1;
    }
    // SAFETY: the caller's call
    run(unsafe { &mut *call }, true)
}
