//! The game's tick in Rust: the order of its steps and the questions that
//! choose them are `halo_map::game_tick`'s; each step is the game's (or a
//! system handed over behind it), through game.c's `game_tick_call`
//! ([`GameTickCallbacks`]).
//!
//! In check mode the game's own tick runs, recording each step and answer;
//! Rust's replay must make the same steps in the same order.

use std::cell::Cell;

use halo_map::game_tick::{GameTickHost, game_tick};

/// The game's side of the tick (halo_sim.c).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct GameTickCallbacks {
    /// A step: a question's answer.
    pub call: unsafe extern "C" fn(kind: i32, f: f32) -> i32,
}

/// A step of the game's tick, recorded in check mode.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct GameTickEventC {
    pub kind: i32,
    pub f: f32,
    pub result: i32,
}

/// The tick: in check mode the game's steps, and the first that differs
/// (-1 for none) and why (1 its kind, 2 its time, 4 more steps than the
/// game's, 5 fewer).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct GameTickC {
    pub events: *const GameTickEventC,
    pub event_count: u32,
    pub difference_event: i32,
    pub difference_reason: i32,
}

struct LiveHost {
    callbacks: GameTickCallbacks,
}

impl GameTickHost for LiveHost {
    fn call(&self, kind: i32, f: f32) -> i32 {
        // SAFETY: the game's callback
        unsafe { (self.callbacks.call)(kind, f) }
    }
}

struct CheckHost<'a> {
    events: &'a [GameTickEventC],
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

impl GameTickHost for CheckHost<'_> {
    fn call(&self, kind: i32, f: f32) -> i32 {
        let index = self.next.get();
        let Some(event) = self.events.get(index) else {
            self.differ(index, 4);
            return 0;
        };
        self.next.set(index + 1);
        if event.kind != kind {
            self.differ(index, 1);
        } else if event.f.to_bits() != f.to_bits() {
            self.differ(index, 2);
        }
        event.result
    }
}

/// The game's tick, in place of the game's: its steps through the game's
/// callback. Returns 0, or -1 if it can't run (the game then runs its own;
/// nothing was done).
///
/// # Safety
/// `call` must be valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn halo_sim_game_tick(call: *mut GameTickC) -> i32 {
    if call.is_null() {
        return -1;
    }
    let Some(host) = crate::world::host() else { return -1 };
    game_tick(&LiveHost { callbacks: host.game_tick });
    0
}

/// The check of a tick the game ran: Rust's replay of its steps. Returns 0,
/// or -1.
///
/// # Safety
/// `call` must be valid; the events hold their count.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn halo_sim_game_tick_check(call: *mut GameTickC) -> i32 {
    if call.is_null() {
        return -1;
    }
    // SAFETY: the caller's call
    let call = unsafe { &mut *call };
    // SAFETY: the game's recording, as described
    let events = if call.events.is_null() || call.event_count == 0 {
        &[][..]
    } else {
        unsafe { std::slice::from_raw_parts(call.events, call.event_count as usize) }
    };
    let checker = CheckHost { events, next: Cell::new(0), difference: Cell::new(None) };
    game_tick(&checker);
    if checker.difference.get().is_none() && checker.next.get() < events.len() {
        checker.differ(checker.next.get(), 5);
    }
    (call.difference_event, call.difference_reason) = checker.difference.get().map_or((-1, 0), |(e, r)| (e as i32, r));
    0
}
