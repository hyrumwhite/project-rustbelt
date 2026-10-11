//! The object update loop (`objects_update`, `object_update`) in Rust, on
//! the running game, with garbage's and scenery's updates: the order of
//! each tick's object updates, and each object's, are
//! `halo_map::objects_update`'s; every step of an object's update is the
//! game's (or a system already handed over, behind it), through objects.c's
//! `objects_update_call` ([`ObjectsCallbacks`]).
//!
//! In check mode the game's own loop runs first, recording each call (the
//! headers and objects it read among them) and what it read; Rust's replay
//! must make the same calls in the same order.

use std::cell::Cell;

use halo_map::objects_update::{ObjectsCall, ObjectsHost, garbage_update, objects_update, scenery_update};

use crate::TAGS;

/// The game's side of the loop (halo_sim.c).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct ObjectsCallbacks {
    /// A call, and what it read.
    pub call: unsafe extern "C" fn(call: *mut ObjectsCall) -> i32,
}

/// What a tick's loop reads.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct ObjectsUpdateC {
    pub dont_update_object: u32,
    /// In check mode: the game's calls, and what they read.
    pub events: *const ObjectsCall,
    pub event_count: u32,
    /// In check mode: the first call that differs (-1 for none) and why
    /// (1 its kind, 2 what it was given, 4 more calls than the game's, 5
    /// fewer).
    pub difference_event: i32,
    pub difference_reason: i32,
}

/// A scenery's update: in and out.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct SceneryCallC {
    pub animation_graph_index: u32,
    pub flags: u32,
    pub random_seed: u32,
    pub animation_index: i16,
    pub animation_frame_index: i16,
}

/// The running game.
struct LiveHost {
    callbacks: ObjectsCallbacks,
}

impl ObjectsHost for LiveHost {
    fn call(&self, call: &mut ObjectsCall) {
        // SAFETY: the game's callback
        unsafe { (self.callbacks.call)(call) };
    }
}

/// The game's own loop, as it recorded it.
struct CheckHost<'a> {
    events: &'a [ObjectsCall],
    next: Cell<usize>,
    difference: Cell<Option<(usize, i32)>>,
}

impl ObjectsHost for CheckHost<'_> {
    fn call(&self, call: &mut ObjectsCall) {
        let index = self.next.get();
        let Some(event) = self.events.get(index) else {
            if self.difference.get().is_none() {
                self.difference.set(Some((index, 4)));
            }
            // (a header past the end, to stop the loops)
            call.header.count = 0;
            return;
        };
        self.next.set(index + 1);
        if self.difference.get().is_none() {
            if event.kind != call.kind {
                self.difference.set(Some((index, 1)));
            } else if event.object_index != call.object_index || event.a != call.a || event.b != call.b {
                self.difference.set(Some((index, 2)));
            }
        }
        call.header = event.header;
        call.state = event.state;
    }
}

/// A tick's object updates on the running game, in place of the game's
/// loop. Returns 0, or -1 if it can't run (the game then runs its own;
/// nothing was done).
///
/// # Safety
/// `call` must be valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn halo_sim_objects_update(call: *mut ObjectsUpdateC) -> i32 {
    if call.is_null() {
        return -1;
    }
    // SAFETY: the caller's call
    let call = unsafe { &mut *call };
    let Some(host) = crate::world::host() else { return -1 };
    objects_update(&LiveHost { callbacks: host.objects }, call.dont_update_object != 0);
    0
}

/// The check of a tick's loop the game ran: Rust's replay of it. Returns 0,
/// or -1.
///
/// # Safety
/// As [`halo_sim_objects_update`]; the events hold their count.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn halo_sim_objects_update_check(call: *mut ObjectsUpdateC) -> i32 {
    if call.is_null() {
        return -1;
    }
    // SAFETY: the caller's call
    let call = unsafe { &mut *call };
    // SAFETY: the game's recording, as described
    let events =
        if call.events.is_null() || call.event_count == 0 { &[][..] } else { unsafe { std::slice::from_raw_parts(call.events, call.event_count as usize) } };
    let checker = CheckHost { events, next: Cell::new(0), difference: Cell::new(None) };
    objects_update(&checker, call.dont_update_object != 0);
    if checker.difference.get().is_none() && checker.next.get() < events.len() {
        checker.difference.set(Some((checker.next.get(), 5)));
    }
    (call.difference_event, call.difference_reason) = checker.difference.get().map_or((-1, 0), |(e, r)| (e as i32, r));
    0
}

/// `garbage_update`: its lifetime (in and out); 1 if it is still active
/// (the game deletes it if not).
///
/// # Safety
/// `lifetime_ticks` must be valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn halo_sim_garbage_update(lifetime_ticks: *mut i16) -> i32 {
    if lifetime_ticks.is_null() {
        return -1;
    }
    // SAFETY: the caller's
    garbage_update(unsafe { &mut *lifetime_ticks }) as i32
}

/// `scenery_update`: its animation and the random seed, in and out. Returns
/// 0, or -1 if the tags aren't there.
///
/// # Safety
/// `call` must be valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn halo_sim_scenery_update(call: *mut SceneryCallC) -> i32 {
    if call.is_null() {
        return -1;
    }
    // SAFETY: the caller's call
    let call = unsafe { &mut *call };
    let graph = if call.flags & 1 != 0 {
        let mut tags = TAGS.lock().unwrap_or_else(|e| e.into_inner());
        let Some(tags) = tags.as_mut() else { return -1 };
        tags.animation_graph(call.animation_graph_index)
    } else {
        None
    };
    scenery_update(graph.as_deref(), call.flags, &mut call.random_seed, &mut call.animation_index, &mut call.animation_frame_index);
    0
}
