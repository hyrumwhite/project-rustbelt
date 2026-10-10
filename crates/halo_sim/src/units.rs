//! Units' animation (`unit_update_animation`) in Rust, on the running game:
//! the stance from the AI's state, the base, soft ping, action and overlay
//! animations advanced, and the state its update asked for entered when it
//! may be, permutations drawn from the game's random numbers. What it sets
//! off is the game's, called back as it happens ([`UnitAnimationCallbacks`]):
//! a grenade's release, melee damage, destroying a dead unit, a limp body,
//! leaving and entering a seat.
//!
//! A player's melee step (`biped_update`'s, [`halo_sim_player_melee`]) is
//! here too: when one starts (timed by the weapon's first-person melee
//! animation) with the unit's melee action, and the tick its damage lands;
//! the weapon's reload stopping, the first-person weapon's message and the
//! damage are the game's ([`MeleeCallbacks`]).
//!
//! The unit update itself (`unit_update`: controls, timers, weapons,
//! grenades, aiming and looking, seats, lights) runs in Rust too
//! ([`halo_sim_unit_update`]), its calls of the game through units.c's
//! `unit_update_call` ([`UnitUpdateCallbacks`]).
//!
//! In check mode the game's own update runs first; Rust then replays it
//! from the state and random seed it began with, as the probe replays a
//! recorded one.

use std::ffi::{CStr, c_char};
use std::sync::Arc;

use halo_map::unit_animation::{AnimationGraph, UnitAnimationHost, UnitAnimationInputs, UnitAnimationState, unit_update_animation};
use halo_map::unit_update::{
    Controller, UnitGlobals, UnitUpdateDefinition, UnitUpdateHost, UnitUpdateInputs, UnitUpdateState, unit_update, weapon_camo_regrowth_rate,
};

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

/// The game's side of a player's melee (halo_sim.c).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct MeleeCallbacks {
    /// A melee started (its action already playing, in `state`): the
    /// weapon's reload stopped and the first-person weapon told.
    pub started: unsafe extern "C" fn(biped: u32, weapon: i32, state: *mut UnitAnimationState),
    /// Its damage lands (`unit_cause_player_melee_damage`).
    pub damage: unsafe extern "C" fn(biped: u32),
}

/// What a player's melee step reads, and where it leaves it.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct MeleeCall {
    pub biped_index: u32,
    pub unit_graph_index: i32,
    pub enhanced_animation: u32,
    /// A player asking to melee, in a seat, zoomed.
    pub use_equipment: u32,
    pub in_seat: u32,
    pub zoomed: u32,
    /// Its weapon (NONE for none), the weapon's definition, and whether the
    /// weapon prevents a melee (`weapon_prevents_melee_attack`).
    pub weapon_index: i32,
    pub weapon_definition_index: u32,
    pub weapon_prevents: u32,
    /// Its melee counters, and its unit's animation (an action may start).
    pub ticks: i8,
    pub attack_tick: i8,
    pub pad: i16,
    pub animation: UnitAnimationState,
    /// In check mode, the random seed the game's step began with.
    pub random_seed: u32,
    /// Written: whether a melee started, and whether its damage landed.
    pub started: u32,
    pub damaged: u32,
}

impl crate::Tags {
    fn melee_weapon(&mut self, definition: u32) -> Option<halo_map::biped_update::MeleeWeapon> {
        let cache = &self.cache;
        *self.melee_weapons.entry(definition).or_insert_with(|| halo_map::biped_update::MeleeWeapon::load(cache, definition).ok())
    }
}

fn run_melee(call: &mut MeleeCall, check: bool) -> i32 {
    let Some(host) = crate::world::host() else { return -1 };
    let (graph, weapon) = {
        let mut tags = TAGS.lock().unwrap_or_else(|e| e.into_inner());
        let Some(tags) = tags.as_mut() else { return -1 };
        let graph = (call.unit_graph_index != -1).then(|| tags.animation_graph(call.unit_graph_index as u32)).flatten();
        let weapon = if call.weapon_index == -1 {
            None
        } else {
            let Some(weapon) = tags.melee_weapon(call.weapon_definition_index) else { return -1 };
            Some((weapon, call.weapon_prevents != 0))
        };
        (graph, weapon)
    };
    let inputs = halo_map::biped_update::PlayerMeleeInputs {
        use_equipment: call.use_equipment != 0,
        in_seat: call.in_seat != 0,
        zoomed: call.zoomed != 0,
        weapon,
    };
    let (mut ticks, mut attack_tick) = (call.ticks, call.attack_tick);
    let outcome = halo_map::biped_update::player_melee(&inputs, &mut ticks, &mut attack_tick);
    let mut state = call.animation;
    if outcome.started {
        // (unit_animation_start_action: its melee, a permutation drawn from
        // the game's random numbers)
        let mut seed = if check { call.random_seed } else { unsafe { (host.unit_animation.random_seed)() } };
        if let Some(graph) = &graph {
            halo_map::unit_animation::unit_animation_start_action(
                graph,
                call.enhanced_animation != 0,
                &mut seed,
                &mut state,
                halo_map::unit_animation::action::MELEE,
            );
        }
        if !check {
            // SAFETY: the game's callbacks
            unsafe {
                (host.unit_animation.set_random_seed)(seed);
                (host.melee.started)(call.biped_index, call.weapon_index, &mut state);
            }
        }
    }
    if outcome.damaged && !check {
        // SAFETY: as above
        unsafe { (host.melee.damage)(call.biped_index) };
    }
    call.ticks = ticks;
    call.attack_tick = attack_tick;
    call.animation = state;
    call.started = outcome.started as u32;
    call.damaged = outcome.damaged as u32;
    0
}

/// A player's melee step on the running game, in place of the game's.
/// Returns 0 (the counters and animation written), or -1 if it can't run
/// (the game then runs its own; nothing was done).
///
/// # Safety
/// `call` must be valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn halo_sim_player_melee(call: *mut MeleeCall) -> i32 {
    if call.is_null() {
        return -1;
    }
    // SAFETY: the caller's call
    run_melee(unsafe { &mut *call }, false)
}

/// The check of a player's melee step the game ran: Rust's replay of it
/// from the counters, animation and random seed it began with.
///
/// # Safety
/// As [`halo_sim_player_melee`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn halo_sim_player_melee_check(call: *mut MeleeCall) -> i32 {
    if call.is_null() {
        return -1;
    }
    // SAFETY: the caller's call
    run_melee(unsafe { &mut *call }, true)
}

/// The game's side of a unit's update (halo_sim.c, which writes the state
/// to the unit first and reads it back after; units.c's
/// `unit_update_call`).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct UnitUpdateCallbacks {
    pub call: unsafe extern "C" fn(unit: u32, state: *mut UnitUpdateState, kind: i32, a: i32, b: f32) -> i32,
}

/// A call of the game's update, recorded in check mode: its kind and what it
/// was given and gave back, the random seed after it, and the unit as it
/// stood before and after.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct UnitUpdateEventC {
    pub kind: i32,
    pub a: i32,
    pub b: f32,
    pub result: i32,
    pub seed: u32,
    pub before: UnitUpdateState,
    pub after: UnitUpdateState,
}

/// A driver's or gunner's controls, as the game gives them.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct ControllerC {
    pub owner_team_index: i16,
    pub pad: i16,
    pub controls: u32,
    pub control_flags: u32,
    pub desired_facing_vector: [f32; 3],
    pub desired_aiming_vector: [f32; 3],
    pub throttle: [f32; 3],
    pub primary_trigger: f32,
}

impl ControllerC {
    fn controller(&self) -> Controller {
        Controller {
            owner_team_index: self.owner_team_index,
            controls: self.controls != 0,
            control_flags: self.control_flags,
            desired_facing_vector: self.desired_facing_vector,
            desired_aiming_vector: self.desired_aiming_vector,
            throttle: self.throttle,
            primary_trigger: self.primary_trigger,
        }
    }
}

/// What a unit's update reads, and where it leaves it.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct UnitUpdateCall {
    pub unit_index: u32,
    pub definition_index: u32,
    /// The state it begins with, and leaves; the game's unit timer.
    pub state: UnitUpdateState,
    pub globals: UnitGlobals,
    pub allow_integrated_lights: u32,
    pub game_engine_running: u32,
    pub cheat_jetpack: u32,
    pub cheat_infinite_ammo: u32,
    /// Its driver's and gunner's controls (`has_*` 0 for none).
    pub has_driver: u32,
    pub driver: ControllerC,
    pub has_gunner: u32,
    pub gunner: ControllerC,
    /// Its current weapon's definition (NONE for none).
    pub weapon_definition_index: u32,
    /// In check mode, the game's calls.
    pub events: *const UnitUpdateEventC,
    pub event_count: u32,
    /// In check mode: the first call that differs (-1 for none) and why
    /// (1 its kind, 2 what it was given, 3 the unit before it, 4 more calls
    /// than the game's, 5 fewer).
    pub difference_event: i32,
    pub difference_reason: i32,
}

impl crate::Tags {
    fn unit_update_definition(&mut self, definition: u32) -> Option<Arc<UnitUpdateDefinition>> {
        let cache = &self.cache;
        self.unit_update_definitions
            .entry(definition)
            .or_insert_with(|| UnitUpdateDefinition::load(cache, definition).ok().map(Arc::new))
            .clone()
    }

    fn weapon_camo_regrowth_rate(&mut self, definition: u32) -> f32 {
        let cache = &self.cache;
        *self.weapon_camo_rates.entry(definition).or_insert_with(|| weapon_camo_regrowth_rate(cache, definition).unwrap_or(0.0))
    }
}

/// The running game, answering a unit's update.
struct LiveUpdateHost {
    callbacks: UnitUpdateCallbacks,
    random: UnitAnimationCallbacks,
    unit_index: u32,
}

impl UnitUpdateHost for LiveUpdateHost {
    fn random_seed(&self) -> u32 {
        // SAFETY: the game's callback
        unsafe { (self.random.random_seed)() }
    }

    fn set_random_seed(&self, seed: u32) {
        // SAFETY: as above
        unsafe { (self.random.set_random_seed)(seed) };
    }

    fn call(&self, state: &mut UnitUpdateState, kind: i32, a: i32, b: f32) -> i32 {
        // SAFETY: as above, with the update's state
        unsafe { (self.callbacks.call)(self.unit_index, state, kind, a, b) }
    }
}

/// The game's own update, as it recorded its calls: each answered from the
/// recording, and the unit before it compared with the game's.
struct CheckUpdateHost<'a> {
    events: &'a [UnitUpdateEventC],
    next: std::cell::Cell<usize>,
    seed: std::cell::Cell<u32>,
    difference: std::cell::Cell<Option<(usize, i32)>>,
}

impl CheckUpdateHost<'_> {
    fn differ(&self, event: usize, reason: i32) {
        if self.difference.get().is_none() {
            self.difference.set(Some((event, reason)));
        }
    }
}

impl UnitUpdateHost for CheckUpdateHost<'_> {
    fn random_seed(&self) -> u32 {
        self.seed.get()
    }

    fn set_random_seed(&self, _seed: u32) {}

    fn call(&self, state: &mut UnitUpdateState, kind: i32, a: i32, b: f32) -> i32 {
        let index = self.next.get();
        let Some(event) = self.events.get(index) else {
            self.differ(index, 4);
            return 0;
        };
        self.next.set(index + 1);
        if event.kind != kind {
            self.differ(index, 1);
        } else if event.a != a || event.b.to_bits() != b.to_bits() {
            self.differ(index, 2);
        } else if event.before != *state {
            self.differ(index, 3);
        }
        *state = event.after;
        self.seed.set(event.seed);
        event.result
    }
}

fn run_unit_update(call: &mut UnitUpdateCall, check: bool) -> i32 {
    let Some(host) = crate::world::host() else { return -1 };
    let (definition, camo_rate) = {
        let mut tags = TAGS.lock().unwrap_or_else(|e| e.into_inner());
        let Some(tags) = tags.as_mut() else { return -1 };
        let Some(definition) = tags.unit_update_definition(call.definition_index) else { return -1 };
        let camo_rate =
            if call.weapon_definition_index == u32::MAX { 0.0 } else { tags.weapon_camo_regrowth_rate(call.weapon_definition_index) };
        (definition, camo_rate)
    };
    let inputs = UnitUpdateInputs {
        allow_integrated_lights: call.allow_integrated_lights != 0,
        game_engine_running: call.game_engine_running != 0,
        cheat_jetpack: call.cheat_jetpack != 0,
        cheat_infinite_ammo: call.cheat_infinite_ammo != 0,
        driver: (call.has_driver != 0).then(|| call.driver.controller()),
        gunner: (call.has_gunner != 0).then(|| call.gunner.controller()),
        weapon_camo_regrowth_rate: camo_rate,
    };
    let mut state = call.state;
    let mut globals = call.globals;
    call.difference_event = -1;
    call.difference_reason = 0;
    if check {
        let events: &[UnitUpdateEventC] = if call.events.is_null() {
            &[]
        } else {
            // SAFETY: the game's recording, `event_count` of them
            unsafe { std::slice::from_raw_parts(call.events, call.event_count as usize) }
        };
        let checker = CheckUpdateHost {
            events,
            next: std::cell::Cell::new(0),
            seed: std::cell::Cell::new(0),
            difference: std::cell::Cell::new(None),
        };
        unit_update(&definition, &inputs, &mut globals, &mut state, Some(&checker));
        if checker.difference.get().is_none() && checker.next.get() < events.len() {
            checker.differ(checker.next.get(), 5);
        }
        (call.difference_event, call.difference_reason) = checker.difference.get().map_or((-1, 0), |(e, r)| (e as i32, r));
    } else {
        let live = LiveUpdateHost { callbacks: host.unit_update, random: host.unit_animation, unit_index: call.unit_index };
        unit_update(&definition, &inputs, &mut globals, &mut state, Some(&live));
    }
    call.state = state;
    call.globals = globals;
    0
}

/// A unit's update on the running game, in place of the game's. Returns 0
/// (the state and unit timer written), or -1 if it can't run (the game then
/// runs its own; nothing was done).
///
/// # Safety
/// `call` must be valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn halo_sim_unit_update(call: *mut UnitUpdateCall) -> i32 {
    if call.is_null() {
        return -1;
    }
    // SAFETY: the caller's call
    run_unit_update(unsafe { &mut *call }, false)
}

/// The check of a unit's update the game ran: Rust's replay of it from the
/// state it began with, each of the game's calls answered from its
/// recording (`call.events`). Returns 0, or -1.
///
/// # Safety
/// As [`halo_sim_unit_update`]; `call.events` holds `call.event_count`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn halo_sim_unit_update_check(call: *mut UnitUpdateCall) -> i32 {
    if call.is_null() {
        return -1;
    }
    // SAFETY: the caller's call
    run_unit_update(unsafe { &mut *call }, true)
}
