//! The AI's update in Rust: what runs each tick, an actor's steps in order
//! and the walk over the active actors are `halo_map::ai_update`'s; each
//! step, and each question the walk asks of the AI's state, is the game's
//! (or a system handed over behind it), through ai.c's `ai_update_call`
//! ([`AiUpdateCallbacks`]).
//!
//! In check mode the game's own update runs, recording each call and its
//! answers; Rust's replay must make the same calls, with the same
//! arguments, in the same order.

use std::cell::Cell;

use halo_map::ai_update::{AiUpdateCall, AiUpdateHost, ai_update};

/// The game's side of the update (halo_sim.c).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct AiUpdateCallbacks {
    pub call: unsafe extern "C" fn(call: *mut AiUpdateCall),
}

/// The update: in check mode the game's calls (with their answers), and the
/// first that differs (-1 for none) and why (1 its kind, 2 its argument, 4
/// more calls than the game's, 5 fewer).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct AiUpdateC {
    pub events: *const AiUpdateCall,
    pub event_count: u32,
    pub difference_event: i32,
    pub difference_reason: i32,
}

struct LiveHost {
    callbacks: AiUpdateCallbacks,
}

impl AiUpdateHost for LiveHost {
    fn call(&self, call: &mut AiUpdateCall) {
        // SAFETY: the game's callback
        unsafe { (self.callbacks.call)(call) }
    }
}

struct CheckHost<'a> {
    events: &'a [AiUpdateCall],
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

impl AiUpdateHost for CheckHost<'_> {
    fn call(&self, call: &mut AiUpdateCall) {
        let index = self.next.get();
        let Some(event) = self.events.get(index) else {
            self.differ(index, 4);
            return;
        };
        self.next.set(index + 1);
        if event.kind != call.kind {
            self.differ(index, 1);
        } else if event.a != call.a {
            self.differ(index, 2);
        }
        call.result = event.result;
        call.result2 = event.result2;
        call.result3 = event.result3;
        call.result4 = event.result4;
    }
}

/// The AI's update, in place of the game's: its steps through the game's
/// callback. Returns 0, or -1 if it can't run (the game then runs its own;
/// nothing was done).
///
/// # Safety
/// `update` must be valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn halo_sim_ai_update(update: *mut AiUpdateC) -> i32 {
    if update.is_null() {
        return -1;
    }
    let Some(host) = crate::world::host() else { return -1 };
    ai_update(&LiveHost { callbacks: host.ai_update });
    0
}

/// The check of an update the game ran: Rust's replay of its calls. Returns
/// 0, or -1.
///
/// # Safety
/// `update` must be valid; the events hold their count.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn halo_sim_ai_update_check(update: *mut AiUpdateC) -> i32 {
    if update.is_null() {
        return -1;
    }
    // SAFETY: the caller's update
    let update = unsafe { &mut *update };
    // SAFETY: the game's recording, as described
    let events = if update.events.is_null() || update.event_count == 0 {
        &[][..]
    } else {
        unsafe { std::slice::from_raw_parts(update.events, update.event_count as usize) }
    };
    let checker = CheckHost { events, next: Cell::new(0), difference: Cell::new(None) };
    ai_update(&checker);
    if checker.difference.get().is_none() && checker.next.get() < events.len() {
        checker.differ(checker.next.get(), 5);
    }
    (update.difference_event, update.difference_reason) =
        checker.difference.get().map_or((-1, 0), |(e, r)| (e as i32, r));
    0
}
