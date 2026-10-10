//! Item updates (`item_update`) in Rust, on the running game: an item's fall,
//! bounces, rest, spin and detonation timer are `halo_map::items`', its ray
//! and point tests Rust's on the live world, its markers from the game's
//! tags; impacts, garbage, translating, accelerating and detonating are the
//! game's, through items.c's `item_call` ([`ItemCallbacks`]).
//!
//! In check mode the game's own update runs first, recording each call with
//! the item before and after; Rust replays it from the item as it was (its
//! tests on the live world, which only the item's own move has changed).

use std::cell::Cell;
use std::sync::Arc;

use halo_map::collision::{CollisionResult, World};
use halo_map::items::{ItemCall, ItemDefinition, ItemHost, ItemInputs, ItemObject, ItemState, item_update};
use halo_map::model::Matrix4x3;

use crate::TAGS;
use crate::world::{GameObjects, purpose};

/// The game's side of item updates (halo_sim.c).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct ItemCallbacks {
    /// A call: the item written to the game first, read back after; 1 if it
    /// is still there.
    pub call: unsafe extern "C" fn(state: *mut ItemState, call: *const ItemCall) -> i32,
}

/// A call of the game's update, recorded in check mode.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct ItemEventC {
    pub call: ItemCall,
    pub result: i32,
    pub before: ItemState,
    pub after: ItemState,
}

/// What an item's update reads, and where it leaves the item.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct ItemCallC {
    pub state: ItemState,
    pub structure_bsp_index: i32,
    /// Which breakable surfaces stand (32 bytes; null for every one).
    pub breakable_surfaces: *const u8,
    pub global_gravity: f32,
    pub game_engine_running: u32,
    pub game_time: i32,
    /// The item's node matrices (13 floats each), in the game's memory.
    pub node_matrices: *const f32,
    pub node_count: u32,
    /// In check mode: the game's calls.
    pub events: *const ItemEventC,
    pub event_count: u32,
    /// In check mode: the first call that differs (-1 for none) and why
    /// (1 its kind, 2 what it was given, 3 the item before it, 4 more calls
    /// than the game's, 5 fewer); the item Rust leaves is compared by the
    /// game.
    pub difference_event: i32,
    pub difference_reason: i32,
    /// Whether the item is still there after.
    pub still_there: u32,
}

impl crate::Tags {
    fn item_definition(&mut self, datum: u32) -> Option<Arc<ItemDefinition>> {
        let cache = &self.cache;
        self.item_definitions.entry(datum).or_insert_with(|| ItemDefinition::load(cache, datum).ok().map(Arc::new)).clone()
    }
}

/// The world both hosts test, and the other objects they read.
struct Common {
    source: GameObjects,
    structure: Arc<crate::Structure>,
}

impl Common {
    fn world(&self) -> World<'_> {
        World { collision: &self.structure.collision, clusters: &self.structure.clusters, objects: &[], source: Some(&self.source) }
    }

    fn object(&self, index: u32) -> Option<ItemObject> {
        let o = self.source.host_object(index, purpose::OTHER)?;
        let node_matrices = (0..o.node_count as usize)
            .map(|n| {
                let mut f = [0.0f32; 13];
                // SAFETY: the host's matrices, node_count of them
                unsafe { std::ptr::copy_nonoverlapping(o.matrices.add(n * 13), f.as_mut_ptr(), 13) };
                matrix(&f)
            })
            .collect();
        Some(ItemObject { object_type: o.object_type, node_matrices })
    }

    fn surface(&self, index: i16) -> Option<(u8, bool)> {
        let surface = self.structure.collision.surfaces.get(usize::try_from(index).ok()?)?;
        Some((surface.flags, self.structure.collision.breakable_surfaces.standing(surface.breakable_surface as usize)))
    }
}

fn matrix(f: &[f32; 13]) -> Matrix4x3 {
    Matrix4x3 { scale: f[0], forward: [f[1], f[2], f[3]], left: [f[4], f[5], f[6]], up: [f[7], f[8], f[9]], position: [f[10], f[11], f[12]] }
}

/// The running game.
struct LiveHost {
    common: Common,
    callbacks: ItemCallbacks,
    still_there: Cell<bool>,
}

impl ItemHost for LiveHost {
    fn call(&self, state: &mut ItemState, call: &ItemCall) -> bool {
        // SAFETY: the game's callback
        let there = unsafe { (self.callbacks.call)(state, call) } != 0;
        if !there {
            self.still_there.set(false);
        }
        there
    }
    fn test_vector(&self, flags: u32, point: [f32; 3], vector: [f32; 3], ignore_object: u32) -> CollisionResult {
        self.common.world().test(flags, point, vector, ignore_object)
    }
    fn test_point(&self, flags: u32, point: [f32; 3], ignore_object: u32) -> bool {
        self.common.world().test_point(flags, point, ignore_object)
    }
    fn object(&self, index: u32) -> Option<ItemObject> {
        self.common.object(index)
    }
    fn surface(&self, index: i16) -> Option<(u8, bool)> {
        self.common.surface(index)
    }
}

/// The game's own update, as it recorded it.
struct CheckHost<'a> {
    common: Common,
    events: &'a [ItemEventC],
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

impl ItemHost for CheckHost<'_> {
    fn call(&self, state: &mut ItemState, call: &ItemCall) -> bool {
        let index = self.next.get();
        let Some(event) = self.events.get(index) else {
            self.differ(index, 4);
            return true;
        };
        self.next.set(index + 1);
        let e = &event.call;
        if e.kind != call.kind {
            self.differ(index, 1);
        } else if e.a != call.a || e.b != call.b || e.c != call.c || e.f.map(f32::to_bits) != call.f.map(f32::to_bits) {
            self.differ(index, 2);
        } else if event.before != *state {
            self.differ(index, 3);
        }
        *state = event.after;
        event.result != 0
    }
    fn test_vector(&self, flags: u32, point: [f32; 3], vector: [f32; 3], ignore_object: u32) -> CollisionResult {
        self.common.world().test(flags, point, vector, ignore_object)
    }
    fn test_point(&self, flags: u32, point: [f32; 3], ignore_object: u32) -> bool {
        self.common.world().test_point(flags, point, ignore_object)
    }
    fn object(&self, index: u32) -> Option<ItemObject> {
        self.common.object(index)
    }
    fn surface(&self, index: i16) -> Option<(u8, bool)> {
        self.common.surface(index)
    }
}

fn run(call: &mut ItemCallC, check: bool) -> i32 {
    let Some(host) = crate::world::host() else { return -1 };
    let Some(structure) = crate::structure(call.structure_bsp_index, call.breakable_surfaces) else { return -1 };
    let definition = {
        let mut tags = TAGS.lock().unwrap_or_else(|e| e.into_inner());
        let Some(tags) = tags.as_mut() else { return -1 };
        tags.item_definition(call.state.definition_index)
    };
    let Some(definition) = definition else { return -1 };
    let node_matrices = (0..call.node_count as usize)
        .map(|n| {
            let mut f = [0.0f32; 13];
            // SAFETY: the game's matrices, node_count of them
            unsafe { std::ptr::copy_nonoverlapping(call.node_matrices.add(n * 13), f.as_mut_ptr(), 13) };
            matrix(&f)
        })
        .collect();
    let inputs = ItemInputs {
        global_gravity: call.global_gravity,
        game_engine_running: call.game_engine_running != 0,
        game_time: call.game_time,
        structure_bsp_index: call.structure_bsp_index as i16,
        node_matrices,
    };
    let common = Common { source: GameObjects { host }, structure };
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
        let checker = CheckHost { common, events, next: Cell::new(0), difference: Cell::new(None) };
        item_update(&inputs, &definition, &checker, &mut state);
        if checker.difference.get().is_none() && checker.next.get() < events.len() {
            checker.differ(checker.next.get(), 5);
        }
        (call.difference_event, call.difference_reason) = checker.difference.get().map_or((-1, 0), |(e, r)| (e as i32, r));
    } else {
        let live = LiveHost { common, callbacks: host.items, still_there: Cell::new(true) };
        item_update(&inputs, &definition, &live, &mut state);
        call.still_there = live.still_there.get() as u32;
    }
    call.state = state;
    0
}

/// An item's update on the running game, in place of the game's. Returns 0
/// (the item's state written), or -1 if it can't run (the game then runs its
/// own; nothing was done).
///
/// # Safety
/// `call` must be valid; its node matrices hold their count.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn halo_sim_item_update(call: *mut ItemCallC) -> i32 {
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
/// As [`halo_sim_item_update`]; the events hold their count.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn halo_sim_item_update_check(call: *mut ItemCallC) -> i32 {
    if call.is_null() {
        return -1;
    }
    // SAFETY: the caller's call
    run(unsafe { &mut *call }, true)
}
