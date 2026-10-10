//! Weapon updates (`weapon_update`) in Rust, on the running game: a weapon's
//! animation, heat, triggers, magazines and their timers are
//! `halo_map::weapons`'; its helpers (firing, reloading, chambering,
//! charging, effects, messages) are the game's, through weapons.c's
//! `weapon_update_call` ([`WeaponCallbacks`]).
//!
//! In check mode the game's own update runs first, recording each call with
//! the weapon and random seed before and after; Rust replays it from the
//! weapon and seed as they were.

use std::cell::Cell;
use std::sync::Arc;

use halo_map::weapons::{WeaponDefinition, WeaponHost, WeaponState, weapon_update};

use crate::TAGS;
use crate::world::{GameObjects, purpose};

/// `_unit_integrated_light_controls_weapon_directly_bit`.
const UNIT_INTEGRATED_LIGHT_CONTROLS_WEAPON_DIRECTLY: u32 = 1 << 23;

/// The game's side of weapon updates (halo_sim.c).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct WeaponCallbacks {
    /// A call: the weapon written to the game first, read back after; 1 if
    /// it is still there.
    pub call: unsafe extern "C" fn(state: *mut WeaponState, kind: i32, a: i32, b: i32) -> i32,
}

/// A call of the game's update, recorded in check mode.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct WeaponEventC {
    pub kind: i32,
    pub a: i32,
    pub b: i32,
    pub seed: u32,
    pub before: WeaponState,
    pub after: WeaponState,
}

/// What a weapon's update reads, and where it leaves the weapon.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct WeaponCallC {
    pub state: WeaponState,
    /// In check mode: the seed it began with, and the game's calls.
    pub random_seed: u32,
    pub events: *const WeaponEventC,
    pub event_count: u32,
    /// In check mode: the first call that differs (-1 for none) and why
    /// (1 its kind, 2 what it was given, 3 the weapon before it, 4 more calls
    /// than the game's, 5 fewer); the weapon Rust leaves is compared by the
    /// game.
    pub difference_event: i32,
    pub difference_reason: i32,
    /// Whether the weapon is still there after.
    pub still_there: u32,
}

impl crate::Tags {
    fn weapon_definition(&mut self, datum: u32) -> Option<Arc<WeaponDefinition>> {
        if let Some(definition) = self.weapon_definitions.get(&datum) {
            return definition.clone();
        }
        let graph = self
            .cache
            .tag_root(datum, halo_map::groups::OBJECT)
            .and_then(|root| root.reference(halo_map::layout::_object_definition::ANIMATION_GRAPH))
            .ok()
            .and_then(|r| r.datum)
            .and_then(|graph| self.animation_graph(graph));
        let definition = WeaponDefinition::load(&self.cache, datum, graph).ok().map(Arc::new);
        self.weapon_definitions.insert(datum, definition.clone());
        definition
    }
}

/// The objects both hosts read.
struct Common {
    source: GameObjects,
}

impl Common {
    fn object_exists(&self, index: i32) -> bool {
        self.source.host_object(index as u32, purpose::OTHER).is_some()
    }

    fn unit_light_controls_weapon(&self, index: i32) -> bool {
        const OBJECT_MASK_UNIT: u32 = (1 << 0) | (1 << 1);
        let Some(o) = self.source.host_object(index as u32, purpose::OTHER) else { return false };
        if o.object_type < 0 || OBJECT_MASK_UNIT & (1 << o.object_type as u32) == 0 {
            return false;
        }
        let flags = {
            let mut tags = TAGS.lock().unwrap_or_else(|e| e.into_inner());
            tags.as_mut().map_or(0, |t| t.unit_flags(o.definition_index))
        };
        flags & UNIT_INTEGRATED_LIGHT_CONTROLS_WEAPON_DIRECTLY != 0
    }
}

/// The running game.
struct LiveHost {
    common: Common,
    callbacks: WeaponCallbacks,
    random: crate::units::UnitAnimationCallbacks,
    still_there: Cell<bool>,
}

impl WeaponHost for LiveHost {
    fn random_seed(&self) -> u32 {
        // SAFETY: the game's callback
        unsafe { (self.random.random_seed)() }
    }
    fn set_random_seed(&self, seed: u32) {
        // SAFETY: as above
        unsafe { (self.random.set_random_seed)(seed) };
    }
    fn call(&self, state: &mut WeaponState, kind: i32, a: i32, b: i32) {
        // SAFETY: as above
        if unsafe { (self.callbacks.call)(state, kind, a, b) } == 0 {
            self.still_there.set(false);
        }
    }
    fn object_exists(&self, index: i32) -> bool {
        self.common.object_exists(index)
    }
    fn unit_light_controls_weapon(&self, index: i32) -> bool {
        self.common.unit_light_controls_weapon(index)
    }
}

/// The game's own update, as it recorded it.
struct CheckHost<'a> {
    common: Common,
    events: &'a [WeaponEventC],
    next: Cell<usize>,
    seed: Cell<u32>,
    difference: Cell<Option<(usize, i32)>>,
}

impl CheckHost<'_> {
    fn differ(&self, event: usize, reason: i32) {
        if self.difference.get().is_none() {
            self.difference.set(Some((event, reason)));
        }
    }
}

impl WeaponHost for CheckHost<'_> {
    fn random_seed(&self) -> u32 {
        self.seed.get()
    }
    fn set_random_seed(&self, seed: u32) {
        self.seed.set(seed);
    }
    fn call(&self, state: &mut WeaponState, kind: i32, a: i32, b: i32) {
        let index = self.next.get();
        let Some(event) = self.events.get(index) else {
            self.differ(index, 4);
            return;
        };
        self.next.set(index + 1);
        if event.kind != kind {
            self.differ(index, 1);
        } else if event.a != a || event.b != b {
            self.differ(index, 2);
        } else if event.before != *state {
            self.differ(index, 3);
        }
        *state = event.after;
        self.seed.set(event.seed);
    }
    fn object_exists(&self, index: i32) -> bool {
        self.common.object_exists(index)
    }
    fn unit_light_controls_weapon(&self, index: i32) -> bool {
        self.common.unit_light_controls_weapon(index)
    }
}

fn run(call: &mut WeaponCallC, check: bool) -> i32 {
    let Some(host) = crate::world::host() else { return -1 };
    let definition = {
        let mut tags = TAGS.lock().unwrap_or_else(|e| e.into_inner());
        let Some(tags) = tags.as_mut() else { return -1 };
        tags.weapon_definition(call.state.definition_index)
    };
    let Some(definition) = definition else { return -1 };
    let common = Common { source: GameObjects { host } };
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
        let checker = CheckHost { common, events, next: Cell::new(0), seed: Cell::new(call.random_seed), difference: Cell::new(None) };
        weapon_update(&definition, &checker, &mut state);
        if checker.difference.get().is_none() && checker.next.get() < events.len() {
            checker.differ(checker.next.get(), 5);
        }
        (call.difference_event, call.difference_reason) = checker.difference.get().map_or((-1, 0), |(e, r)| (e as i32, r));
    } else {
        let live = LiveHost { common, callbacks: host.weapons, random: host.unit_animation, still_there: Cell::new(true) };
        weapon_update(&definition, &live, &mut state);
        call.still_there = live.still_there.get() as u32;
    }
    call.state = state;
    0
}

/// A weapon's update on the running game, in place of the game's. Returns 0
/// (the weapon's state written), or -1 if it can't run (the game then runs
/// its own; nothing was done).
///
/// # Safety
/// `call` must be valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn halo_sim_weapon_update(call: *mut WeaponCallC) -> i32 {
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
/// As [`halo_sim_weapon_update`]; the events hold their count.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn halo_sim_weapon_update_check(call: *mut WeaponCallC) -> i32 {
    if call.is_null() {
        return -1;
    }
    // SAFETY: the caller's call
    run(unsafe { &mut *call }, true)
}
