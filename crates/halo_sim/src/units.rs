//! Units' animation (`unit_update_animation`) in Rust, on the running game:
//! the stance from the AI's state, the base, soft ping, action and overlay
//! animations advanced, and the state its update asked for entered when it
//! may be, permutations drawn from the game's random numbers. What it sets
//! off is the game's, called back as it happens ([`UnitAnimationCallbacks`]):
//! a grenade's release, melee damage, destroying a dead unit, a limp body,
//! leaving and entering a seat.
//!
//! In check mode the game's own update runs first; Rust then replays it
//! from the state and random seed it began with, as the probe replays a
//! recorded one.

use std::ffi::{CStr, c_char};
use std::sync::Arc;

use halo_map::unit_animation::{AnimationGraph, UnitAnimationHost, UnitAnimationInputs, UnitAnimationState, unit_update_animation};

use crate::TAGS;

/// The game's side of a unit's animation (halo_sim.c, which writes the
/// state to the unit first and reads it back after).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct UnitAnimationCallbacks {
    pub random_seed: unsafe extern "C" fn() -> u32,
    pub set_random_seed: unsafe extern "C" fn(seed: u32),
    /// A side effect (`unit_animation::side_effects`' bit): whether the
    /// unit is still there.
    pub effect: unsafe extern "C" fn(unit: u32, state: *mut UnitAnimationState, effect: u32) -> i32,
}

/// What a unit's animation update reads, and where it leaves it.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct UnitAnimationCall {
    pub unit_index: u32,
    /// The unit definition's graph (NONE for none).
    pub unit_graph_index: i32,
    /// The state it begins with, and leaves.
    pub state: UnitAnimationState,
    pub state_desired: i8,
    pub crouching: i8,
    pub desired_state: i8,
    pub magic_seat_index: i8,
    pub dead: u32,
    pub parented: u32,
    pub player: u32,
    pub force_alert: u32,
    pub flaming: u32,
    pub magic_base_seat_index: i16,
    pub pad: i16,
    pub at_rest: u32,
    pub biped: u32,
    pub airborne: u32,
    pub destroyed_after_dying: u32,
    pub has_no_dying_airborne: u32,
    /// The current weapon's label (NUL-terminated).
    pub weapon_label: *const c_char,
    /// In check mode, the random seed the game's update began with.
    pub random_seed: u32,
    /// Written: what it returned (1: a leap's start done, the unit jumps),
    /// and its side effects.
    pub result: i16,
    pub pad2: i16,
    pub effects: u32,
}

impl crate::Tags {
    fn animation_graph(&mut self, datum: u32) -> Option<Arc<AnimationGraph>> {
        let cache = &self.cache;
        self.unit_graphs.entry(datum).or_insert_with(|| AnimationGraph::load(cache, datum).ok().map(Arc::new)).clone()
    }
}

/// The running game, doing what a unit's animation sets off.
struct LiveHost {
    callbacks: UnitAnimationCallbacks,
    unit_index: u32,
}

impl UnitAnimationHost for LiveHost {
    fn random_seed(&self) -> u32 {
        // SAFETY: the game's callback
        unsafe { (self.callbacks.random_seed)() }
    }

    fn set_random_seed(&self, seed: u32) {
        // SAFETY: as above
        unsafe { (self.callbacks.set_random_seed)(seed) };
    }

    fn effect(&self, state: &mut UnitAnimationState, effect: u32) -> bool {
        // SAFETY: as above, with the update's state
        unsafe { (self.callbacks.effect)(self.unit_index, state, effect) != 0 }
    }
}

fn run(call: &mut UnitAnimationCall, check: bool) -> i32 {
    let Some(host) = crate::world::host() else { return -1 };
    // (the graphs it may play from: the unit's, and its animation's)
    let (unit_graph, object_graph) = {
        let mut tags = TAGS.lock().unwrap_or_else(|e| e.into_inner());
        let Some(tags) = tags.as_mut() else { return -1 };
        let unit_graph = (call.unit_graph_index != -1).then(|| tags.animation_graph(call.unit_graph_index as u32)).flatten();
        let object_graph = (call.state.object_animation_graph_index != u32::MAX)
            .then(|| tags.animation_graph(call.state.object_animation_graph_index))
            .flatten();
        (unit_graph, object_graph)
    };
    let unit_graph_index = call.unit_graph_index as u32;
    let object_graph_index = call.state.object_animation_graph_index;
    let graph = |index: u32| -> Option<&AnimationGraph> {
        if index == unit_graph_index {
            unit_graph.as_deref()
        } else if index == object_graph_index {
            object_graph.as_deref()
        } else {
            None
        }
    };
    let weapon_label = if call.weapon_label.is_null() {
        "unarmed".to_string()
    } else {
        // SAFETY: the game's NUL-terminated label
        unsafe { CStr::from_ptr(call.weapon_label) }.to_string_lossy().into_owned()
    };
    let inputs = UnitAnimationInputs {
        state_desired: call.state_desired,
        crouching: call.crouching != 0,
        desired_state: call.desired_state,
        dead: call.dead != 0,
        parented: call.parented != 0,
        player: call.player != 0,
        force_alert: call.force_alert != 0,
        flaming: call.flaming != 0,
        magic_base_seat_index: call.magic_base_seat_index,
        magic_seat_index: call.magic_seat_index,
        at_rest: call.at_rest != 0,
        biped: call.biped != 0,
        airborne: call.airborne != 0,
        weapon_label,
        random_seed: call.random_seed,
        destroyed_after_dying: call.destroyed_after_dying != 0,
        has_no_dying_airborne: call.has_no_dying_airborne != 0,
    };
    let live = LiveHost { callbacks: host.unit_animation, unit_index: call.unit_index };
    let mut state = call.state;
    let (result, effects) =
        unit_update_animation(unit_graph_index, &graph, &inputs, &mut state, if check { None } else { Some(&live) });
    call.state = state;
    call.result = result;
    call.effects = effects;
    0
}

/// A unit's animation update on the running game, in place of the game's.
/// Returns 0 (the state, result and effects written), or -1 if it can't run
/// (the game then runs its own; nothing was done).
///
/// # Safety
/// `call` must be valid, its pointers the game's as described.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn halo_sim_unit_animation_update(call: *mut UnitAnimationCall) -> i32 {
    if call.is_null() {
        return -1;
    }
    // SAFETY: the caller's call
    run(unsafe { &mut *call }, false)
}

/// The check of a unit's animation update the game ran: Rust's replay of
/// it from the state and random seed it began with. Returns 0, or -1.
///
/// # Safety
/// As [`halo_sim_unit_animation_update`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn halo_sim_unit_animation_check(call: *mut UnitAnimationCall) -> i32 {
    if call.is_null() {
        return -1;
    }
    // SAFETY: the caller's call
    run(unsafe { &mut *call }, true)
}
