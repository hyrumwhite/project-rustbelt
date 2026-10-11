//! Weapon updates (`weapon_update` and its helpers) in Rust, on the running
//! game: a weapon's animation, heat, triggers, magazines, firing (its
//! projectiles aimed and spread), reloading, chambering, charging and state
//! are `halo_map::weapons`'; what they do to the rest of the game (effects,
//! messages, aiming, creating projectiles, the unit, the gametype, the AI,
//! damage) is the game's, through weapons.c's `weapon_game_call`
//! ([`WeaponCallbacks`]).
//!
//! In check mode the game's own update runs first, recording each call with
//! the weapon and random seed before and after; Rust replays it from the
//! weapon and seed as they were.

use std::cell::Cell;
use std::sync::Arc;

use halo_map::weapons::{EffectObject, OwnerUnit, WeaponCall, WeaponDefinition, WeaponHost, WeaponInputs, WeaponState, weapon_update};

use crate::TAGS;
use crate::world::{GameObjects, purpose};

/// `_unit_integrated_light_controls_weapon_directly_bit`.
const UNIT_INTEGRATED_LIGHT_CONTROLS_WEAPON_DIRECTLY: u32 = 1 << 23;

/// A unit as firing reads it (the game's side).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct OwnerUnitC {
    pub definition_index: u32,
    pub dead: u32,
    pub player_index: i32,
    pub actor_index: i32,
    pub gunner_object_index: i32,
    pub gunner_player_index: i32,
    pub gunner_actor_index: i32,
}

/// The game's side of weapon updates (halo_sim.c).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct WeaponCallbacks {
    /// A call: the weapon written to the game first, read back after; 1 if
    /// it is still there.
    pub call: unsafe extern "C" fn(state: *mut WeaponState, call: *mut WeaponCall) -> i32,
    /// An object, if it is a unit: 1.
    pub owner_unit: unsafe extern "C" fn(index: u32, out: *mut OwnerUnitC) -> i32,
}

/// A call of the game's update, recorded in check mode.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct WeaponEventC {
    pub call: WeaponCall,
    pub seed: u32,
    pub before: WeaponState,
    pub after: WeaponState,
}

/// What a weapon's update reads, and where it leaves the weapon.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct WeaponCallC {
    pub state: WeaponState,
    pub game_time: i32,
    pub game_engine_running: u32,
    pub bottomless_clip: u32,
    pub infinite_ammo: u32,
    /// `random_vector_in_cone3d`'s table.
    pub directions: *const [f32; 3],
    pub direction_count: u32,
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
    callbacks: WeaponCallbacks,
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

    fn owner_unit(&self, index: i32) -> Option<OwnerUnit> {
        let mut unit = OwnerUnitC::default();
        // SAFETY: the game's callback
        if unsafe { (self.callbacks.owner_unit)(index as u32, &mut unit) } == 0 {
            return None;
        }
        let definition_flags = {
            let mut tags = TAGS.lock().unwrap_or_else(|e| e.into_inner());
            tags.as_mut().map_or(0, |t| t.unit_flags(unit.definition_index))
        };
        Some(OwnerUnit {
            definition_index: unit.definition_index,
            definition_flags,
            dead: unit.dead != 0,
            player_index: unit.player_index,
            actor_index: unit.actor_index,
            gunner_object_index: unit.gunner_object_index,
            gunner_player_index: unit.gunner_player_index,
            gunner_actor_index: unit.gunner_actor_index,
        })
    }

    fn effect_object(&self, index: i32) -> Option<EffectObject> {
        let grip = crate::unit_ik::read_grip(&self.source, index)?;
        Some(EffectObject {
            markers: grip.markers,
            node_matrices: grip.node_matrices,
            region_permutations: grip.region_permutations,
            mirrored: grip.mirrored,
        })
    }
}

/// The running game.
struct LiveHost {
    common: Common,
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
    fn call(&self, state: &mut WeaponState, call: &mut WeaponCall) -> i32 {
        // SAFETY: as above
        if unsafe { (self.common.callbacks.call)(state, call) } == 0 {
            self.still_there.set(false);
        }
        call.result
    }
    fn object_exists(&self, index: i32) -> bool {
        self.common.object_exists(index)
    }
    fn unit_light_controls_weapon(&self, index: i32) -> bool {
        self.common.unit_light_controls_weapon(index)
    }
    fn owner_unit(&self, index: i32) -> Option<OwnerUnit> {
        self.common.owner_unit(index)
    }
    fn effect_object(&self, index: i32) -> Option<EffectObject> {
        self.common.effect_object(index)
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
    fn call(&self, state: &mut WeaponState, call: &mut WeaponCall) -> i32 {
        let index = self.next.get();
        let Some(event) = self.events.get(index) else {
            self.differ(index, 4);
            return 0;
        };
        self.next.set(index + 1);
        let e = &event.call;
        if e.kind != call.kind {
            self.differ(index, 1);
        } else if e.a != call.a || e.b != call.b || e.c != call.c || e.d != call.d || e.f.map(f32::to_bits) != call.f.map(f32::to_bits) {
            self.differ(index, 2);
        } else if event.before != *state {
            self.differ(index, 3);
        }
        *state = event.after;
        self.seed.set(event.seed);
        call.result = e.result;
        call.out = e.out;
        e.result
    }
    fn object_exists(&self, index: i32) -> bool {
        self.common.object_exists(index)
    }
    fn unit_light_controls_weapon(&self, index: i32) -> bool {
        self.common.unit_light_controls_weapon(index)
    }
    fn owner_unit(&self, index: i32) -> Option<OwnerUnit> {
        self.common.owner_unit(index)
    }
    fn effect_object(&self, index: i32) -> Option<EffectObject> {
        self.common.effect_object(index)
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
    // SAFETY: the game's direction table, direction_count of them
    let directions = if call.directions.is_null() {
        &[][..]
    } else {
        unsafe { std::slice::from_raw_parts(call.directions, call.direction_count as usize) }
    };
    let inputs = WeaponInputs {
        game_time: call.game_time,
        game_engine_running: call.game_engine_running != 0,
        bottomless_clip: call.bottomless_clip != 0,
        infinite_ammo: call.infinite_ammo != 0,
        directions,
    };
    let common = Common { source: GameObjects { host }, callbacks: host.weapons };
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
        weapon_update(&definition, &inputs, &checker, &mut state);
        if checker.difference.get().is_none() && checker.next.get() < events.len() {
            checker.differ(checker.next.get(), 5);
        }
        (call.difference_event, call.difference_reason) = checker.difference.get().map_or((-1, 0), |(e, r)| (e as i32, r));
    } else {
        let live = LiveHost { common, random: host.unit_animation, still_there: Cell::new(true) };
        weapon_update(&definition, &inputs, &live, &mut state);
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
