//! Halo's weapon update (ROADMAP phase 7, objects and physics): weapons.c's
//! `weapon_update` and its helpers, ported as they are, in the same
//! single-precision arithmetic.
//!
//! Each tick, for each weapon: its animation (whose key frames and ends
//! move its state on), detonating when dropped if it does, its integrated
//! light dimming, its heat (overheating, cooling by its age, recovering),
//! which triggers are down (the controls, the secondary trigger's mode,
//! analog, sticky and toggling triggers), reloading, each magazine's
//! recharge and reload and chamber timers, and each trigger's state
//! machine, rate of fire (with its blurred permutation) and error.
//!
//! The helpers: a trigger's firing (`weapon_trigger_begin_firing`,
//! `weapon_trigger_fire`: charging, overloading, its magazine's rounds, its
//! firing effects, misfires, heat and age, recoil, and its projectiles,
//! `trigger_create_projectiles`: aimed by the unit, the player or the actor,
//! spread in the error cone and the distribution), its charge's release,
//! overcharge, recovery, locking and tracking; a magazine's reload and
//! chamber; the weapon's state (`weapon_set_state`, its animation from the
//! graph's weapon animations) and its key frames; ejection ports; and
//! detonating.
//!
//! What the weapon does to the rest of the game goes through a
//! [`WeaponHost`] ([`call`]'s kinds) at the moments the C does it.

// (the C's shape: its switch arms' tests, its index loops over the
// triggers, and its counters)
#![allow(clippy::collapsible_match, clippy::needless_range_loop, clippy::manual_saturating_arithmetic, clippy::implicit_saturating_add)]

use std::sync::Arc;

use crate::markers::{ModelMarkers, normalize3};
use crate::model::Matrix4x3;
use crate::projectile::Random;
use crate::unit_animation::{AnimationGraph, animation_choose_random_permutation, animation_update, update_result};
use crate::{CacheFile, Result, layout};

const NONE: i32 = -1;
const TICKS_PER_SECOND: i32 = 30;

// weapon definition flags
const DETONATES_WHEN_DROPPED: u32 = 1 << 10;
const CANNOT_FIRE_AT_MAXIMUM_AGE: u32 = 1 << 11;
const SECONDARY_TRIGGER_OVERRIDES_GRENADES: u32 = 1 << 12;
// trigger definition flags
const TRIGGER_TRACKS_PROJECTILE: u32 = 1 << 0;
const TRIGGER_RANDOM_FIRING_EFFECTS: u32 = 1 << 1;
const TRIGGER_CAN_FIRE_WITH_PARTIAL_AMMUNITION: u32 = 1 << 2;
const TRIGGER_LATCHED: u32 = 1 << 3;
const TRIGGER_TOGGLES: u32 = 1 << 4;
const TRIGGER_USES_WEAPON_ORIGIN: u32 = 1 << 5;
const TRIGGER_STICKS_WHEN_DROPPED: u32 = 1 << 6;
const TRIGGER_EJECTION_PORT_DURING_CHAMBER_ANIMATION: u32 = 1 << 7;
const TRIGGER_ANALOG_RATE_OF_FIRE: u32 = 1 << 9;
const TRIGGER_USE_ERROR_WHEN_UNZOOMED: u32 = 1 << 10;
const TRIGGER_PROJECTILES_CANNOT_BE_AIMED: u32 = 1 << 11;
const TRIGGER_PROJECTILES_HAVE_IDENTICAL_ERROR: u32 = 1 << 12;
// magazine definition flags
const MAGAZINE_WASTES_ROUNDS_WHEN_RELOADED: u32 = 1 << 0;
const MAGAZINE_MUST_BE_CHAMBERED_EVERY_SHOT: u32 = 1 << 1;
// weapon datum flags
const OVERHEATED: u32 = 1 << 0;
const OVERHEATED_EXIT: u32 = 1 << 1;
const OVERHEAT_RECOIL: u32 = 1 << 2;
const NEEDS_TO_RELOAD: u32 = 1 << 3;
// control flags
const CONTROL_PRIMARY_TRIGGER: u16 = 1 << 1;
const CONTROL_SECONDARY_TRIGGER: u16 = 1 << 2;
const CONTROL_RELOAD: u16 = 1 << 3;
const CONTROL_USER_BUSY: u16 = 1 << 4;
const CONTROL_ZOOMED: u16 = 1 << 6;
// trigger datum flags
const RELEASED_SINCE_LAST_SHOT: u32 = 1 << 0;
const WAS_DOWN: u32 = 1 << 1;
const TOGGLED: u32 = 1 << 2;
const BLURRED: u32 = 1 << 4;
const FIRED_BEFORE_CHARGING: u32 = 1 << 5;
// item flags
const ITEM_BELONGS_TO_PLAYER: u32 = 1 << 1;
// object flags
const OBJECT_INVISIBLE: u32 = 1 << 0;
// unit definition flags
const UNIT_FIRES_FROM_CAMERA: u32 = 1 << 3;
// secondary trigger modes
const SECONDARY_SLAVED_TO_PRIMARY: i16 = 1;
const SECONDARY_INHIBITS_PRIMARY: i16 = 2;
const SECONDARY_LOADS_ALTERNATE_AMMUNITION: i16 = 3;
const SECONDARY_LOADS_MULTIPLE_PRIMARY_AMMUNITION: i16 = 4;
// weapon types
const WEAPON_TYPE_SHOTGUN: i16 = 1;
const WEAPON_TYPE_PLASMA_PISTOL: i16 = 3;
// first person weapon messages
const MESSAGE_PRIMARY_FIRE: i32 = 0;
const MESSAGE_SECONDARY_FIRE: i32 = 1;
const MESSAGE_PRIMARY_MISFIRE: i32 = 2;
const MESSAGE_SECONDARY_MISFIRE: i32 = 3;
const MESSAGE_RELOAD_WHILE_EMPTY: i32 = 9;
const MESSAGE_CHARGED: i32 = 14;
const MESSAGE_OVERHEATING: i32 = 15;
const MESSAGE_OVERHEATING_SUPER_RECOIL: i32 = 16;
// trigger states
const TRIGGER_IDLE: i8 = 0;
const TRIGGER_OVERLOADING: i8 = 1;
const TRIGGER_CHARGING: i8 = 2;
const TRIGGER_CHARGED: i8 = 3;
const TRIGGER_RECOVERING: i8 = 4;
const TRIGGER_TRACKING: i8 = 5;
const TRIGGER_SPEWING: i8 = 6;
const TRIGGER_LOCKED: i8 = 7;
const TRIGGER_UNINITIALIZED: i8 = 8;
// magazine states
const MAGAZINE_IDLE: i16 = 0;
const MAGAZINE_RELOADING: i16 = 1;
const MAGAZINE_UNCHAMBERED: i16 = 2;
const MAGAZINE_CHAMBERING: i16 = 3;
// weapon states
const STATE_IDLE: i8 = 0;
const STATE_PRIMARY_RECOIL: i8 = 1;
const STATE_SECONDARY_RECOIL: i8 = 2;
const STATE_PRIMARY_CHAMBER: i8 = 3;
const STATE_SECONDARY_CHAMBER: i8 = 4;
const STATE_PRIMARY_RELOAD: i8 = 5;
const STATE_SECONDARY_RELOAD: i8 = 6;
const STATE_PRIMARY_CHARGED: i8 = 7;
const STATE_SECONDARY_CHARGED: i8 = 8;
const STATE_READY: i8 = 9;
const STATE_PUT_AWAY: i8 = 10;
// overcharged actions
const OVERCHARGED_EXPLODES: i16 = 1;
const OVERCHARGED_FIRE: i16 = 2;
// trigger firing effect types
const FIRING_EFFECT: usize = 0;
const OVERHEATED_EFFECT: usize = 1;
const EMPTY_EFFECT: usize = 2;
// projectile distributions
const DISTRIBUTION_HORIZONTAL_FAN: i16 = 1;

/// `struct weapon_trigger`, as the game lays it out.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct WeaponTrigger {
    pub idle_ticks: i8,
    pub state: i8,
    pub state_timer: i16,
    pub flags: u32,
    pub firing_effects_used_flags: u16,
    pub firing_effect_index: i16,
    pub firing_effect_shots_remaining: i16,
    pub sequential_non_tracer_rounds: i16,
    pub rate_of_fire: f32,
    pub ejection_port_position: f32,
    pub illumination: f32,
    pub error: f32,
    pub charging_effect_index: i32,
}

const _: () = assert!(std::mem::size_of::<WeaponTrigger>() == 36);

/// `struct weapon_magazine`, as the game lays it out.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct WeaponMagazine {
    pub state: i16,
    pub state_timer: i16,
    pub original_time: i16,
    pub rounds_total: i16,
    pub rounds_loaded: i16,
    pub rounds_fractional_recharged: i16,
}

const _: () = assert!(std::mem::size_of::<WeaponMagazine>() == 12);

/// A weapon as its update reads and changes it (the game's fields, laid
/// out for the game's side, `halo_sim.c`).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct WeaponState {
    pub object_index: u32,
    pub definition_index: u32,
    pub parent_object_index: i32,
    pub item_flags: u32,
    pub flags: u32,
    pub control_flags: u16,
    pub pad0: u16,
    pub primary_trigger: f32,
    pub state: i8,
    pub pad1: i8,
    pub state_timer: i16,
    pub heat: f32,
    pub age: f32,
    pub overcharged: f32,
    pub integrated_light_power: f32,
    pub tracked_object_index: i32,
    pub alternate_shots_loaded: i16,
    pub animation_index: i16,
    pub animation_frame_index: i16,
    pub pad2: i16,
    pub triggers: [WeaponTrigger; 2],
    pub magazines: [WeaponMagazine; 2],
    pub overheated_effect_index: i32,
    pub object_flags: u32,
    pub game_time_last_fired: i32,
}

const _: () = assert!(std::mem::size_of::<WeaponState>() == 168);

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct MagazineDefinition {
    pub flags: u32,
    pub rounds_recharged_per_second: i16,
    pub rounds_loaded_maximum: i16,
    pub rounds_reloaded: i16,
    pub chamber_time: f32,
    pub reloading_effect: i32,
    pub chambering_effect: i32,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct FiringEffect {
    pub shots_lower_bound: i16,
    pub shots_upper_bound: i16,
    pub effects: [i32; 3],
    pub damage_effects: [i32; 3],
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct TriggerDefinition {
    pub flags: u32,
    pub initial_rate_of_fire: f32,
    pub final_rate_of_fire: f32,
    pub blurred_rate_of_fire: f32,
    pub magazine_index: i16,
    pub rounds_per_shot: i16,
    pub minimum_rounds_loaded_per_shot: i16,
    pub rounds_between_tracers: i16,
    pub firing_noise: i16,
    pub charging_time: f32,
    pub charged_time: f32,
    pub overcharged_action: i16,
    pub spew_time: f32,
    pub charging_effect: i32,
    pub projectile_distribution_function: i16,
    pub projectiles_per_shot: i16,
    pub projectile_distribution_angle: f32,
    pub projectile_error_inner_cone_angle: f32,
    pub projectile_error_angle_lower_bound: f32,
    pub projectile_error_angle_upper_bound: f32,
    pub first_person_weapon_offset: [f32; 3],
    pub projectile: i32,
    pub raw_ejection_port_recovery_time: f32,
    pub raw_illumination_recovery_time: f32,
    pub heat_generated_per_round: f32,
    pub age_generated_per_round: f32,
    pub overloading_time: f32,
    pub illumination_recovery_time: f32,
    pub ejection_port_recovery_time: f32,
    pub rate_of_fire_acceleration_time: f32,
    pub rate_of_fire_deceleration_time: f32,
    pub error_acceleration_time: f32,
    pub error_deceleration_time: f32,
    pub firing_effect_count: i32,
    pub firing_effects: Vec<FiringEffect>,
}

/// A weapon definition's (`_weapon_definition`'s) heat, age, triggers and
/// magazines, and its animation graph and model's markers.
#[derive(Debug, Clone, Default)]
pub struct WeaponDefinition {
    pub flags: u32,
    pub secondary_trigger_mode: i16,
    pub maximum_alternate_shots_loaded: i16,
    pub heat_recovery_threshold: f32,
    pub heat_overheated_threshold: f32,
    pub heat_detonation_threshold: f32,
    pub overheated_explosion_fraction: f32,
    pub heat_loss_per_second: f32,
    pub overheated_effect: i32,
    pub detonation_effect: i32,
    pub age_heat_recovery_penalty: f32,
    pub age_rate_of_fire_penalty: f32,
    pub age_misfire_start: f32,
    pub age_misfire_chance: f32,
    pub weapon_type: i16,
    /// The blocks' counts as the tag has them.
    pub magazine_count: i32,
    pub trigger_count: i32,
    pub magazines: Vec<MagazineDefinition>,
    pub triggers: Vec<TriggerDefinition>,
    pub animation_graph: Option<Arc<AnimationGraph>>,
    pub has_animation_graph: bool,
}

fn reference(p: crate::cache::Ptr, rel: usize) -> Result<i32> {
    Ok(p.reference(rel)?.datum.map_or(NONE, |d| d as i32))
}

impl WeaponDefinition {
    pub fn load(cache: &CacheFile, datum: u32, graph: Option<Arc<AnimationGraph>>) -> Result<Self> {
        use layout::{_weapon_definition as w, trigger_firing_effect as e, weapon_magazine_definition as m, weapon_trigger_definition as t};
        let root = cache.tag_root(datum, crate::groups::OBJECT)?;
        let weapon = root.add(layout::weapon_definition::WEAPON);
        let magazine_block = weapon.block(w::MAGAZINES, m::SIZEOF)?;
        let trigger_block = weapon.block(w::TRIGGERS, t::SIZEOF)?;
        let mut magazines = Vec::new();
        for magazine in magazine_block.iter() {
            magazines.push(MagazineDefinition {
                flags: magazine.u32(m::FLAGS)?,
                rounds_recharged_per_second: magazine.i16(m::ROUNDS_RECHARGED_PER_SECOND)?,
                rounds_loaded_maximum: magazine.i16(m::ROUNDS_LOADED_MAXIMUM)?,
                rounds_reloaded: magazine.i16(m::ROUNDS_RELOADED)?,
                chamber_time: magazine.f32(m::CHAMBER_TIME)?,
                reloading_effect: reference(magazine, m::RELOADING_EFFECT)?,
                chambering_effect: reference(magazine, m::CHAMBERING_EFFECT)?,
            });
        }
        let mut triggers = Vec::new();
        for trigger in trigger_block.iter() {
            let effect_block = trigger.block(t::FIRING_EFFECTS, e::SIZEOF)?;
            let mut firing_effects = Vec::new();
            for effect in effect_block.iter() {
                let refs = |rel: usize| -> Result<[i32; 3]> {
                    Ok([reference(effect, rel)?, reference(effect, rel + 16)?, reference(effect, rel + 32)?])
                };
                firing_effects.push(FiringEffect {
                    shots_lower_bound: effect.i16(e::SHOTS_LOWER_BOUND)?,
                    shots_upper_bound: effect.i16(e::SHOTS_UPPER_BOUND)?,
                    effects: refs(e::EFFECTS)?,
                    damage_effects: refs(e::DAMAGE_EFFECTS)?,
                });
            }
            triggers.push(TriggerDefinition {
                flags: trigger.u32(t::FLAGS)?,
                initial_rate_of_fire: trigger.f32(t::INITIAL_RATE_OF_FIRE)?,
                final_rate_of_fire: trigger.f32(t::FINAL_RATE_OF_FIRE)?,
                blurred_rate_of_fire: trigger.f32(t::BLURRED_RATE_OF_FIRE)?,
                magazine_index: trigger.i16(t::MAGAZINE_INDEX)?,
                rounds_per_shot: trigger.i16(t::ROUNDS_PER_SHOT)?,
                minimum_rounds_loaded_per_shot: trigger.i16(t::MINIMUM_ROUNDS_LOADED_PER_SHOT)?,
                rounds_between_tracers: trigger.i16(t::ROUNDS_BETWEEN_TRACERS)?,
                firing_noise: trigger.i16(t::FIRING_NOISE)?,
                charging_time: trigger.f32(t::CHARGING_TIME)?,
                charged_time: trigger.f32(t::CHARGED_TIME)?,
                overcharged_action: trigger.i16(t::OVERCHARGED_ACTION)?,
                spew_time: trigger.f32(t::SPEW_TIME)?,
                charging_effect: reference(trigger, t::CHARGING_EFFECT)?,
                projectile_distribution_function: trigger.i16(t::PROJECTILE_DISTRIBUTION_FUNCTION)?,
                projectiles_per_shot: trigger.i16(t::PROJECTILES_PER_SHOT)?,
                projectile_distribution_angle: trigger.f32(t::PROJECTILE_DISTRIBUTION_ANGLE)?,
                projectile_error_inner_cone_angle: trigger.f32(t::PROJECTILE_ERROR_INNER_CONE_ANGLE)?,
                projectile_error_angle_lower_bound: trigger.f32(t::PROJECTILE_ERROR_ANGLE_LOWER_BOUND)?,
                projectile_error_angle_upper_bound: trigger.f32(t::PROJECTILE_ERROR_ANGLE_UPPER_BOUND)?,
                first_person_weapon_offset: trigger.vec3(t::FIRST_PERSON_WEAPON_OFFSET)?,
                projectile: reference(trigger, t::PROJECTILE)?,
                raw_ejection_port_recovery_time: trigger.f32(t::EJECTION_PORT_RECOVERY_TIME)?,
                raw_illumination_recovery_time: trigger.f32(t::ILLUMINATION_RECOVERY_TIME)?,
                heat_generated_per_round: trigger.f32(t::HEAT_GENERATED_PER_ROUND)?,
                age_generated_per_round: trigger.f32(t::AGE_GENERATED_PER_ROUND)?,
                overloading_time: trigger.f32(t::OVERLOADING_TIME)?,
                illumination_recovery_time: trigger.f32(t::RUNTIME_ILLUMINATION_RECOVERY_TIME)?,
                ejection_port_recovery_time: trigger.f32(t::RUNTIME_EJECTION_PORT_RECOVERY_TIME)?,
                rate_of_fire_acceleration_time: trigger.f32(t::RUNTIME_RATE_OF_FIRE_ACCELERATION_TIME)?,
                rate_of_fire_deceleration_time: trigger.f32(t::RUNTIME_RATE_OF_FIRE_DECELERATION_TIME)?,
                error_acceleration_time: trigger.f32(t::RUNTIME_ERROR_ACCELERATION_TIME)?,
                error_deceleration_time: trigger.f32(t::RUNTIME_ERROR_DECELERATION_TIME)?,
                firing_effect_count: effect_block.iter().count() as i32,
                firing_effects,
            });
        }
        let has_animation_graph = root.reference(layout::_object_definition::ANIMATION_GRAPH)?.datum.is_some();
        Ok(WeaponDefinition {
            flags: weapon.u32(w::FLAGS)?,
            secondary_trigger_mode: weapon.i16(w::SECONDARY_TRIGGER_MODE)?,
            maximum_alternate_shots_loaded: weapon.i16(w::MAXIMUM_ALTERNATE_SHOTS_LOADED)?,
            heat_recovery_threshold: weapon.f32(w::HEAT_RECOVERY_THRESHOLD)?,
            heat_overheated_threshold: weapon.f32(w::HEAT_OVERHEATED_THRESHOLD)?,
            heat_detonation_threshold: weapon.f32(w::HEAT_DETONATION_THRESHOLD)?,
            overheated_explosion_fraction: weapon.f32(w::OVERHEATED_EXPLOSION_FRACTION)?,
            heat_loss_per_second: weapon.f32(w::HEAT_LOSS_PER_SECOND)?,
            overheated_effect: reference(weapon, w::OVERHEATED_EFFECT)?,
            detonation_effect: reference(weapon, w::DETONATION_EFFECT)?,
            age_heat_recovery_penalty: weapon.f32(w::AGE_HEAT_RECOVERY_PENALTY)?,
            age_rate_of_fire_penalty: weapon.f32(w::AGE_RATE_OF_FIRE_PENALTY)?,
            age_misfire_start: weapon.f32(w::AGE_MISFIRE_START)?,
            age_misfire_chance: weapon.f32(w::AGE_MISFIRE_CHANCE)?,
            weapon_type: weapon.i16(w::WEAPON_TYPE)?,
            magazine_count: magazine_block.iter().count() as i32,
            trigger_count: trigger_block.iter().count() as i32,
            magazines,
            triggers,
            animation_graph: graph,
            has_animation_graph,
        })
    }

    /// `weapon_magazine_count`.
    fn magazine_count(&self) -> usize {
        self.magazine_count.clamp(0, 2) as usize
    }

    /// `weapon_trigger_count`.
    fn trigger_count(&self) -> usize {
        self.trigger_count.clamp(0, 2) as usize
    }

    /// `weapon_trigger_magazine_valid`.
    fn trigger_magazine_valid(&self, trigger: &TriggerDefinition) -> bool {
        trigger.magazine_index as i32 == NONE || (trigger.magazine_index >= 0 && (trigger.magazine_index as usize) < self.magazine_count())
    }
}

/// What the weapon asks of the rest of the game (`WeaponCall::kind`).
pub mod call {
    /// `item_detonate`.
    pub const DETONATE_ITEM: i32 = 1;
    /// `first_person_weapon_message_from_weapon(weapon, a)`.
    pub const MESSAGE: i32 = 2;
    /// `weapon_effect_looping_new(weapon, a)`: its index out.
    pub const LOOPING_EFFECT: i32 = 3;
    /// `effect_stop(a, TRUE)`.
    pub const EFFECT_STOP: i32 = 4;
    /// The trigger `a`'s blurred permutation on (`b` 1) or off.
    pub const PERMUTE_BLURRED: i32 = 5;
    /// `weapon_effect_new(weapon, a, f[0], f[1])` (an effect or a sound): its
    /// index out.
    pub const EFFECT: i32 = 6;
    /// `scenario_location_underwater` of the weapon: 1 if so.
    pub const UNDERWATER: i32 = 7;
    /// `weapon_get_first_person_animation_time(weapon, 0, reload while
    /// empty, a)`.
    pub const ANIMATION_TIME: i32 = 8;
    /// `unit_handle_weapon_state_change(owner, a)`.
    pub const WEAPON_STATE_CHANGE: i32 = 9;
    /// `game_engine_weapon_fired` for the owner's player, if it has one.
    pub const WEAPON_FIRED: i32 = 10;
    /// `ai_handle_unit_effect(owner, shooting, a)`.
    pub const AI_SHOOTING: i32 = 11;
    /// The damage effect `a` dealt to its owner (a firing effect's).
    pub const SELF_DAMAGE: i32 = 12;
    /// `object_delete(weapon)`.
    pub const DELETE: i32 = 13;
    /// `actor_firing_blindly(a)`: 1 if so.
    pub const FIRING_BLINDLY: i32 = 14;
    /// `unit_adjust_projectile_ray(owner, origin, forward, velocity, a, b)`:
    /// `f` the origin and forward in, out with the velocity (`out[6]`).
    pub const PROJECTILE_RAY: i32 = 15;
    /// `player_aim_projectile(a, origin, forward)`: the target, and the
    /// forward (`out[3..6]`).
    pub const PLAYER_AIM: i32 = 16;
    /// `actor_aim_projectile(a, origin, forward, error)`: the target, the
    /// forward and the error (`out[6]`).
    pub const ACTOR_AIM: i32 = 17;
    /// A projectile (definition `a`, owner `b`, target `c`; `d` 1 a tracer, 2
    /// to be kept inside the structure from the owner's camera): `f` its
    /// position, forward, up and velocity. Its index out.
    pub const NEW_PROJECTILE: i32 = 18;
}

/// One call of the game, and what it answered.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct WeaponCall {
    pub kind: i32,
    pub a: i32,
    pub b: i32,
    pub c: i32,
    pub d: i32,
    pub f: [f32; 12],
    pub result: i32,
    pub out: [f32; 8],
}

/// The unit that owns a weapon, as firing reads it.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct OwnerUnit {
    pub definition_index: u32,
    pub definition_flags: u32,
    pub dead: bool,
    pub player_index: i32,
    pub actor_index: i32,
    pub gunner_object_index: i32,
    /// The gunner's (when it has one).
    pub gunner_player_index: i32,
    pub gunner_actor_index: i32,
}

/// The object a weapon's effects and projectiles come from, as its
/// trigger's markers read it.
#[derive(Debug, Clone, Default)]
pub struct EffectObject {
    pub markers: Option<Arc<ModelMarkers>>,
    pub node_matrices: Vec<Matrix4x3>,
    pub region_permutations: [u8; 8],
    pub mirrored: bool,
}

/// What a weapon's update reads of the game besides the weapon.
#[derive(Debug, Clone, Copy)]
pub struct WeaponInputs<'a> {
    pub game_time: i32,
    pub game_engine_running: bool,
    pub bottomless_clip: bool,
    pub infinite_ammo: bool,
    /// `random_vector_in_cone3d`'s table.
    pub directions: &'a [[f32; 3]],
}

/// A running game, as a weapon's update asks it. The weapon's state is the
/// update's as it stands (the host writes it to the game first, and reads
/// back what the call changed).
pub trait WeaponHost {
    fn random_seed(&self) -> u32;
    fn set_random_seed(&self, seed: u32);
    fn call(&self, state: &mut WeaponState, call: &mut WeaponCall) -> i32;
    /// Whether an object is there.
    fn object_exists(&self, index: i32) -> bool;
    /// Whether an object is a unit whose integrated light controls its
    /// weapon's directly.
    fn unit_light_controls_weapon(&self, index: i32) -> bool;
    /// An object, if it is a unit.
    fn owner_unit(&self, index: i32) -> Option<OwnerUnit>;
    fn effect_object(&self, index: i32) -> Option<EffectObject>;
}

/// The update and its helpers' shared reads.
struct Weapon<'a> {
    definition: &'a WeaponDefinition,
    host: &'a dyn WeaponHost,
    inputs: &'a WeaponInputs<'a>,
}

impl Weapon<'_> {
    fn call(&self, state: &mut WeaponState, kind: i32, a: i32, b: i32) -> WeaponCall {
        let mut c = WeaponCall { kind, a, b, ..Default::default() };
        c.result = self.host.call(state, &mut c);
        c
    }

    fn with_random<R>(&self, f: impl FnOnce(&mut Random) -> R) -> R {
        let mut random = Random { seed: self.host.random_seed(), directions: self.inputs.directions };
        let result = f(&mut random);
        self.host.set_random_seed(random.seed);
        result
    }

    /// `weapon_get_owner_object_index`: its parent, if a unit.
    fn owner(&self, weapon: &WeaponState) -> (i32, Option<OwnerUnit>) {
        if weapon.parent_object_index == NONE {
            return (NONE, None);
        }
        match self.host.owner_unit(weapon.parent_object_index) {
            Some(unit) => (weapon.parent_object_index, Some(unit)),
            None => (NONE, None),
        }
    }

    /// `weapon_effect_new` (nothing for none): its index.
    fn effect_new(&self, weapon: &mut WeaponState, effect_index: i32, scale: f32, error: f32) -> i32 {
        if effect_index == NONE {
            return NONE;
        }
        let mut c = WeaponCall { kind: call::EFFECT, a: effect_index, ..Default::default() };
        c.f[0] = scale;
        c.f[1] = error;
        self.host.call(weapon, &mut c)
    }

    /// `weapon_trigger_change_state`.
    fn trigger_change_state(&self, weapon: &mut WeaponState, trigger_index: usize, state: i8, timer: i16) {
        if let Some(trigger) = weapon.triggers.get_mut(trigger_index) {
            trigger.state = state;
            trigger.state_timer = timer;
        }
    }

    /// `weapon_state_interruptable`.
    fn state_interruptable(old: i8, new: i8) -> bool {
        if old != STATE_IDLE { old > STATE_IDLE && old <= STATE_SECONDARY_RECOIL && new >= old } else { true }
    }

    /// `weapon_set_state`: whether it changed (the weapon's state, its
    /// animation, and its unit told).
    fn set_state(&self, weapon: &mut WeaponState, new_state: i8, immediate: bool) -> bool {
        if !(immediate || Self::state_interruptable(weapon.state, new_state)) {
            return false;
        }
        if self.definition.has_animation_graph
            && let Some(graph) = self.definition.animation_graph.as_deref()
            && let Some(animations) = &graph.weapon_animations
        {
            let slot = match new_state {
                STATE_IDLE => Some(0),
                STATE_PRIMARY_RECOIL => Some(9),
                STATE_SECONDARY_RECOIL => Some(10),
                STATE_PRIMARY_CHAMBER => Some(5),
                STATE_SECONDARY_CHAMBER => Some(6),
                STATE_PRIMARY_RELOAD | STATE_SECONDARY_RELOAD => Some(3),
                STATE_PRIMARY_CHARGED | STATE_SECONDARY_CHARGED => Some(8),
                STATE_READY => Some(1),
                STATE_PUT_AWAY => Some(2),
                _ => None,
            };
            if let Some(slot) = slot {
                let animation_index = animations.get(slot).copied().unwrap_or(NONE as i16);
                if animation_index as i32 != NONE || new_state == STATE_IDLE {
                    let mut seed = self.host.random_seed();
                    weapon.animation_index = animation_choose_random_permutation(graph, &mut seed, animation_index);
                    self.host.set_random_seed(seed);
                    weapon.animation_frame_index = 0;
                    weapon.state = new_state;
                }
            }
        }
        if self.owner(weapon).1.is_some() {
            self.call(weapon, call::WEAPON_STATE_CHANGE, new_state as i32, 0);
        }
        true
    }

    /// `weapon_magazine_state_change_ok`.
    fn magazine_state_change_ok(weapon: &WeaponState) -> bool {
        weapon.triggers[0].state == TRIGGER_IDLE && weapon.triggers[1].state == TRIGGER_IDLE && weapon.state == STATE_IDLE
    }

    /// `weapon_trigger_start_ejection_port`.
    fn trigger_start_ejection_port(&self, weapon: &mut WeaponState, trigger_index: usize, chamber: bool) {
        let Some(trigger_definition) = self.definition.triggers.get(trigger_index) else { return };
        if trigger_index < 2 && trigger_definition.raw_ejection_port_recovery_time > 0.0 {
            let during_chamber = trigger_definition.flags & TRIGGER_EJECTION_PORT_DURING_CHAMBER_ANIMATION != 0;
            if (during_chamber && chamber) || (!during_chamber && !chamber) {
                weapon.triggers[trigger_index].ejection_port_position = 1.0;
            }
        }
    }

    /// `weapon_state_key_frame`.
    fn state_key_frame(&self, weapon: &mut WeaponState) {
        match weapon.state {
            STATE_PRIMARY_CHAMBER => self.trigger_start_ejection_port(weapon, 0, true),
            STATE_SECONDARY_CHAMBER => self.trigger_start_ejection_port(weapon, 1, true),
            _ => {}
        }
    }

    /// `weapon_state_next`.
    fn state_next(&self, weapon: &mut WeaponState) {
        if weapon.state < STATE_PRIMARY_CHARGED || (weapon.state > STATE_SECONDARY_CHARGED && weapon.state != STATE_PUT_AWAY) {
            self.set_state(weapon, STATE_IDLE, true);
        }
    }

    /// `weapon_detonate`.
    fn detonate(&self, weapon: &mut WeaponState) {
        self.effect_new(weapon, self.definition.detonation_effect, 0.0, 0.0);
        self.call(weapon, call::DELETE, 0, 0);
    }

    /// `weapon_magazine_start_reload`.
    fn magazine_start_reload(&self, weapon: &mut WeaponState, magazine_index: usize, unknown: bool) {
        if magazine_index >= 2 {
            return;
        }
        let Some(&magazine_definition) = self.definition.magazines.get(magazine_index) else { return };
        let state = weapon.magazines[magazine_index].state;
        if (state == MAGAZINE_IDLE || state == MAGAZINE_UNCHAMBERED) && Self::magazine_state_change_ok(weapon) {
            let magazine = weapon.magazines[magazine_index];
            if magazine.rounds_total > 0 && magazine.rounds_loaded < magazine_definition.rounds_loaded_maximum {
                let mut reload_type = NONE;
                self.set_state(weapon, STATE_PRIMARY_RELOAD + magazine_index as i8, false);
                self.effect_new(weapon, magazine_definition.reloading_effect, 0.0, 0.0);
                let loaded = weapon.magazines[magazine_index].rounds_loaded;
                self.call(weapon, call::MESSAGE, MESSAGE_RELOAD_WHILE_EMPTY + (loaded != 0) as i32, 0);
                if self.definition.weapon_type == WEAPON_TYPE_SHOTGUN {
                    let one_left = magazine_definition.rounds_loaded_maximum as i32 - weapon.magazines[magazine_index].rounds_loaded as i32 == 1;
                    reload_type = if unknown {
                        if one_left { 2 } else { 0 }
                    } else if one_left {
                        1
                    } else {
                        NONE
                    };
                }
                weapon.magazines[magazine_index].state = MAGAZINE_RELOADING;
                let time = self.call(weapon, call::ANIMATION_TIME, reload_type, 0).result as i16;
                weapon.magazines[magazine_index].state_timer = time;
                weapon.magazines[magazine_index].original_time = time;
            }
            weapon.flags &= !NEEDS_TO_RELOAD;
        }
    }

    /// `weapon_magazine_finish_reload`.
    fn magazine_finish_reload(&self, weapon: &mut WeaponState, magazine_index: usize) {
        if magazine_index >= 2 {
            return;
        }
        let Some(&magazine_definition) = self.definition.magazines.get(magazine_index) else { return };
        let magazine = &mut weapon.magazines[magazine_index];
        if magazine_definition.flags & MAGAZINE_WASTES_ROUNDS_WHEN_RELOADED != 0 {
            magazine.rounds_loaded = 0;
        }
        let rounds_to_load =
            if magazine_definition.rounds_reloaded > magazine.rounds_total { magazine.rounds_total } else { magazine_definition.rounds_reloaded } as i32;
        let mut rounds_loaded = (magazine.rounds_loaded as i32 + rounds_to_load) as i16;
        if rounds_loaded > magazine_definition.rounds_loaded_maximum {
            rounds_loaded = magazine_definition.rounds_loaded_maximum;
        }
        if !self.inputs.infinite_ammo && weapon.item_flags & ITEM_BELONGS_TO_PLAYER != 0 {
            magazine.rounds_total = (magazine.rounds_total as i32 - rounds_loaded as i32 + magazine.rounds_loaded as i32) as i16;
        }
        magazine.rounds_loaded = rounds_loaded;
        magazine.state = MAGAZINE_UNCHAMBERED;
        magazine.state_timer = 0;
        let magazine = *magazine;
        if magazine.rounds_total > 0
            && magazine.rounds_loaded < magazine_definition.rounds_loaded_maximum
            && magazine_definition.flags & MAGAZINE_WASTES_ROUNDS_WHEN_RELOADED == 0
            && weapon.control_flags & 0x26 == 0
        {
            self.magazine_start_reload(weapon, magazine_index, false);
        }
    }

    /// `weapon_magazine_start_chamber`.
    fn magazine_start_chamber(&self, weapon: &mut WeaponState, magazine_index: usize) {
        if magazine_index >= 2 {
            return;
        }
        let state = weapon.magazines[magazine_index].state;
        if (state == MAGAZINE_IDLE || state == MAGAZINE_UNCHAMBERED) && Self::magazine_state_change_ok(weapon) {
            let Some(&magazine_definition) = self.definition.magazines.get(magazine_index) else { return };
            self.set_state(weapon, STATE_PRIMARY_CHAMBER + magazine_index as i8, false);
            self.effect_new(weapon, magazine_definition.chambering_effect, 0.0, 0.0);
            weapon.magazines[magazine_index].state = MAGAZINE_CHAMBERING;
            weapon.magazines[magazine_index].state_timer = (magazine_definition.chamber_time * TICKS_PER_SECOND as f32) as i16;
        }
    }

    /// `weapon_magazine_finish_chamber` (`weapon_magazine_idle`).
    fn magazine_finish_chamber(&self, weapon: &mut WeaponState, magazine_index: usize) {
        if let Some(magazine) = weapon.magazines.get_mut(magazine_index) {
            magazine.state = MAGAZINE_IDLE;
            magazine.state_timer = 0;
        }
    }

    /// `weapon_trigger_idle`.
    fn trigger_idle(&self, weapon: &mut WeaponState, trigger_index: usize) {
        self.trigger_change_state(weapon, trigger_index, TRIGGER_IDLE, 0);
    }

    /// `weapon_trigger_locked`.
    fn trigger_locked(&self, weapon: &mut WeaponState, trigger_index: usize) {
        self.trigger_change_state(weapon, trigger_index, TRIGGER_LOCKED, NONE as i16);
    }

    /// `weapon_trigger_recover`.
    fn trigger_recover(&self, weapon: &mut WeaponState, trigger_index: usize) {
        if trigger_index >= 2 {
            return;
        }
        weapon.triggers[trigger_index].idle_ticks = 0;
        self.trigger_idle(weapon, trigger_index);
    }

    /// `weapon_trigger_finish_tracking`.
    fn trigger_finish_tracking(&self, weapon: &mut WeaponState, trigger_index: usize) {
        weapon.tracked_object_index = NONE;
        self.trigger_recover(weapon, trigger_index);
    }

    /// `weapon_trigger_fully_charged`.
    fn trigger_fully_charged(&self, weapon: &mut WeaponState, trigger_index: usize) {
        let charged_time = self.definition.triggers[trigger_index].charged_time;
        self.trigger_change_state(weapon, trigger_index, TRIGGER_CHARGED, (charged_time * TICKS_PER_SECOND as f32) as i16);
        self.set_state(weapon, STATE_PRIMARY_CHARGED + trigger_index as i8, true);
        self.call(weapon, call::MESSAGE, MESSAGE_CHARGED, 0);
    }

    /// `weapon_trigger_overload`.
    fn trigger_overload(&self, weapon: &mut WeaponState, trigger_index: usize) {
        if (trigger_index as i32 + 1) < self.definition.trigger_count {
            self.trigger_fire(weapon, trigger_index + 1);
        }
        let overloading_time = self.definition.triggers[trigger_index].overloading_time;
        self.trigger_change_state(weapon, trigger_index, TRIGGER_OVERLOADING, (overloading_time * TICKS_PER_SECOND as f32) as i16);
    }

    /// `weapon_trigger_release_charge`.
    fn trigger_release_charge(&self, weapon: &mut WeaponState, trigger_index: usize) {
        let spew_time = self.definition.triggers[trigger_index].spew_time;
        if spew_time > 0.0 {
            self.trigger_change_state(weapon, trigger_index, TRIGGER_SPEWING, (spew_time * TICKS_PER_SECOND as f32) as i16);
        } else {
            if self.definition.trigger_count > 1 {
                self.trigger_fire(weapon, 1);
            }
            self.trigger_recover(weapon, trigger_index);
        }
        if let Some(trigger) = weapon.triggers.get_mut(trigger_index) {
            trigger.rate_of_fire = 0.0;
        }
    }

    /// `weapon_trigger_overcharged`.
    fn trigger_overcharged(&self, weapon: &mut WeaponState, trigger_index: usize) {
        match self.definition.triggers[trigger_index].overcharged_action {
            OVERCHARGED_EXPLODES => self.detonate(weapon),
            OVERCHARGED_FIRE => self.trigger_release_charge(weapon, trigger_index),
            _ => {}
        }
    }

    /// `weapon_trigger_begin_firing`.
    fn trigger_begin_firing(&self, weapon: &mut WeaponState, trigger_index: usize, force: bool) {
        if trigger_index >= 2 {
            return;
        }
        let trigger_definition = &self.definition.triggers[trigger_index];
        let mut can_fire = true;
        if trigger_definition.magazine_index as i32 != NONE {
            let magazine = if self.definition.trigger_magazine_valid(trigger_definition) {
                weapon.magazines.get(trigger_definition.magazine_index as usize)
            } else {
                None
            };
            if magazine.is_none_or(|m| m.state != MAGAZINE_IDLE) {
                can_fire = false;
            }
        }
        if weapon.flags & OVERHEATED != 0 {
            can_fire = false;
        }
        if self.call(weapon, call::UNDERWATER, 0, 0).result != 0 {
            can_fire = false;
        }
        if !can_fire {
            return;
        }
        if !force && trigger_definition.charging_time > 0.0 {
            if self.definition.flags & CANNOT_FIRE_AT_MAXIMUM_AGE != 0 && weapon.age >= 1.0 {
                self.trigger_fire(weapon, trigger_index);
            } else {
                if self.definition.trigger_count > 1 {
                    let effect = self.effect_new(weapon, trigger_definition.charging_effect, 0.0, 0.0);
                    weapon.triggers[trigger_index].charging_effect_index = effect;
                } else if weapon.triggers[trigger_index].rate_of_fire > 0.0 {
                    weapon.triggers[trigger_index].flags |= FIRED_BEFORE_CHARGING;
                    self.trigger_fire(weapon, trigger_index);
                } else {
                    weapon.triggers[trigger_index].flags &= !FIRED_BEFORE_CHARGING;
                }
                let timer = (trigger_definition.charging_time * TICKS_PER_SECOND as f32) as i16;
                self.trigger_change_state(weapon, trigger_index, TRIGGER_CHARGING, timer);
            }
        } else if !force && trigger_definition.overloading_time > 0.0 {
            let timer = (trigger_definition.overloading_time * TICKS_PER_SECOND as f32) as i16;
            self.trigger_change_state(weapon, trigger_index, TRIGGER_OVERLOADING, timer);
        } else {
            self.trigger_fire(weapon, trigger_index);
        }
    }

    /// `weapon_trigger_fire`.
    fn trigger_fire(&self, weapon: &mut WeaponState, trigger_index: usize) {
        if trigger_index >= 2 {
            return;
        }
        let definition = self.definition;
        let trigger_definition = &definition.triggers[trigger_index];
        let (owner_object_index, _) = self.owner(weapon);
        let mut damage_effect_index = NONE;
        let mut effect_index = NONE;
        let mut effect_error = 0.0f32;
        let mut effect_scale = 0.0f32;
        let mut fired = false;
        let mut misfired = false;
        let loads_alternate_ammunition = trigger_index == 1
            && (definition.secondary_trigger_mode == SECONDARY_LOADS_ALTERNATE_AMMUNITION
                || definition.secondary_trigger_mode == SECONDARY_LOADS_MULTIPLE_PRIMARY_AMMUNITION);

        if trigger_definition.magazine_index as i32 != NONE {
            let magazine_index = trigger_definition.magazine_index as usize;
            let valid = definition.trigger_magazine_valid(trigger_definition) && magazine_index < 2;
            if valid && (!loads_alternate_ammunition || weapon.alternate_shots_loaded < definition.maximum_alternate_shots_loaded) {
                let magazine_definition = definition.magazines[magazine_index];
                let released = weapon.triggers[trigger_index].flags & RELEASED_SINCE_LAST_SHOT != 0;
                let magazine = &mut weapon.magazines[magazine_index];
                if (magazine.rounds_loaded >= trigger_definition.rounds_per_shot
                    || trigger_definition.flags & TRIGGER_CAN_FIRE_WITH_PARTIAL_AMMUNITION != 0)
                    && !(definition.flags & CANNOT_FIRE_AT_MAXIMUM_AGE != 0 && weapon.age >= 1.0)
                    && (magazine.rounds_loaded >= trigger_definition.minimum_rounds_loaded_per_shot || !released)
                {
                    let emptied = !self.inputs.bottomless_clip && {
                        magazine.rounds_loaded = (magazine.rounds_loaded as i32 - trigger_definition.rounds_per_shot as i32) as i16;
                        magazine.rounds_loaded <= 0
                    };
                    if emptied {
                        magazine.rounds_loaded = 0;
                    } else if magazine_definition.flags & MAGAZINE_MUST_BE_CHAMBERED_EVERY_SHOT != 0 {
                        magazine.state = MAGAZINE_UNCHAMBERED;
                        magazine.state_timer = 0;
                    }
                    fired = true;
                }
            }
        } else {
            fired = true;
        }
        if self.inputs.bottomless_clip {
            fired = true;
        }

        if trigger_definition.firing_effect_count > 0 {
            let count = trigger_definition.firing_effect_count;
            if weapon.triggers[trigger_index].firing_effect_shots_remaining <= 0 {
                let starting_firing_effect_index = weapon.triggers[trigger_index].firing_effect_index;
                let mut firing_effect_index = if trigger_definition.flags & TRIGGER_RANDOM_FIRING_EFFECTS != 0 {
                    self.with_random(|r| (r.next() as i32 % count) as i16)
                } else {
                    starting_firing_effect_index
                };
                loop {
                    let trigger = &mut weapon.triggers[trigger_index];
                    if trigger.firing_effects_used_flags as i32 == (1i32 << count) - 1 {
                        trigger.firing_effects_used_flags = 0;
                    }
                    loop {
                        firing_effect_index += 1;
                        if firing_effect_index as i32 >= count {
                            firing_effect_index = 0;
                        }
                        if (trigger.firing_effects_used_flags as u32 >> firing_effect_index as u32) & 1 == 0 {
                            break;
                        }
                    }
                    let next_firing_effect = trigger_definition.firing_effects[firing_effect_index as usize];
                    trigger.firing_effect_index = firing_effect_index;
                    trigger.firing_effects_used_flags |= 1 << firing_effect_index;
                    let shots = self.with_random(|r| r.index(next_firing_effect.shots_lower_bound, next_firing_effect.shots_upper_bound));
                    let trigger = &mut weapon.triggers[trigger_index];
                    trigger.firing_effect_shots_remaining = shots;
                    if !(trigger.firing_effect_shots_remaining <= 0 && firing_effect_index != starting_firing_effect_index) {
                        break;
                    }
                }
            }
            let trigger = &mut weapon.triggers[trigger_index];
            trigger.firing_effect_shots_remaining = trigger.firing_effect_shots_remaining.wrapping_sub(1);
            let firing_effect = trigger_definition
                .firing_effects
                .get(trigger.firing_effect_index as usize)
                .copied()
                .unwrap_or(FiringEffect { effects: [NONE; 3], damage_effects: [NONE; 3], ..Default::default() });

            if definition.age_misfire_start > 0.0 && definition.age_misfire_start < 1.0 && weapon.age > definition.age_misfire_start {
                let mut misfire_chance =
                    ((weapon.age - definition.age_misfire_start) * definition.age_misfire_chance) / (1.0 - definition.age_misfire_start);
                if weapon.triggers[trigger_index].state == TRIGGER_SPEWING {
                    misfire_chance *= 2.0;
                }
                if self.with_random(|r| r.real()) < misfire_chance {
                    misfired = true;
                }
            }

            let effect_type;
            if !fired {
                effect_type = EMPTY_EFFECT;
                effect_scale = 1.0;
                effect_error = 0.0;
            } else if misfired {
                effect_type = OVERHEATED_EFFECT;
                effect_scale = weapon.triggers[trigger_index].rate_of_fire;
                effect_error = 0.0;
            } else {
                effect_type = FIRING_EFFECT;
                effect_scale = weapon.triggers[trigger_index].rate_of_fire;
                effect_error = if definition.heat_overheated_threshold == 0.0 { 0.0 } else { weapon.heat / definition.heat_overheated_threshold };
            }
            effect_index = firing_effect.effects[effect_type];
            damage_effect_index = firing_effect.damage_effects[effect_type];
        }

        if fired {
            if weapon.item_flags & ITEM_BELONGS_TO_PLAYER != 0 && self.inputs.game_engine_running {
                self.call(weapon, call::WEAPON_FIRED, owner_object_index, 0);
            }
            weapon.game_time_last_fired = self.inputs.game_time;
            let message = if misfired {
                if trigger_index != 0 { MESSAGE_SECONDARY_MISFIRE } else { MESSAGE_PRIMARY_MISFIRE }
            } else if trigger_index != 0 {
                MESSAGE_SECONDARY_FIRE
            } else {
                MESSAGE_PRIMARY_FIRE
            };
            self.call(weapon, call::MESSAGE, message, 0);
            self.trigger_start_ejection_port(weapon, trigger_index, false);
            if trigger_definition.raw_illumination_recovery_time > 0.0 {
                weapon.triggers[trigger_index].illumination = 1.0;
            }
            if !self.inputs.bottomless_clip {
                weapon.heat += trigger_definition.heat_generated_per_round;
            }
            if weapon.item_flags & ITEM_BELONGS_TO_PLAYER == 0 && weapon.heat > definition.heat_overheated_threshold {
                weapon.heat = definition.heat_overheated_threshold;
            } else if weapon.heat > 1.0 {
                weapon.heat = 1.0;
            }
            if weapon.item_flags & ITEM_BELONGS_TO_PLAYER != 0 && !self.inputs.infinite_ammo {
                weapon.age += trigger_definition.age_generated_per_round;
                if weapon.age > 1.0 {
                    weapon.age = 1.0;
                }
            }
            self.set_state(weapon, if trigger_index != 0 { STATE_SECONDARY_RECOIL } else { STATE_PRIMARY_RECOIL }, false);
            if !misfired {
                if loads_alternate_ammunition {
                    weapon.alternate_shots_loaded += 1;
                } else {
                    self.trigger_create_projectiles(weapon, trigger_index);
                    self.call(weapon, call::AI_SHOOTING, trigger_definition.firing_noise as i32, 0);
                }
            }
            if owner_object_index != NONE && damage_effect_index != NONE {
                self.call(weapon, call::SELF_DAMAGE, damage_effect_index, 0);
            }
            if definition.weapon_type == WEAPON_TYPE_PLASMA_PISTOL && trigger_index == 1 {
                weapon.flags |= OVERHEAT_RECOIL;
            }
        }

        if weapon.heat > definition.heat_detonation_threshold && self.with_random(|r| r.real()) < definition.overheated_explosion_fraction {
            self.detonate(weapon);
        }

        if !fired {
            self.trigger_change_state(weapon, trigger_index, TRIGGER_LOCKED, NONE as i16);
        } else if weapon.triggers[trigger_index].state != TRIGGER_SPEWING || misfired {
            if trigger_definition.flags & TRIGGER_TRACKS_PROJECTILE != 0 {
                self.trigger_change_state(weapon, trigger_index, TRIGGER_TRACKING, NONE as i16);
            } else {
                self.trigger_recover(weapon, trigger_index);
            }
        }
        weapon.triggers[trigger_index].flags &= !RELEASED_SINCE_LAST_SHOT;
        self.effect_new(weapon, effect_index, effect_scale, effect_error);
    }

    /// `trigger_create_projectiles`.
    fn trigger_create_projectiles(&self, weapon: &mut WeaponState, trigger_index: usize) {
        if trigger_index >= 2 {
            return;
        }
        let definition = self.definition;
        let trigger_definition = &definition.triggers[trigger_index];
        let (owner_object_index, unit) = self.owner(weapon);
        let marker_name = if trigger_index == 0 { "primary trigger" } else { "secondary trigger" };
        // (weapon_get_effect_object_index: its parent, while it is invisible)
        let effect_object_index =
            if weapon.object_flags & OBJECT_INVISIBLE != 0 && weapon.parent_object_index != NONE { weapon.parent_object_index } else { weapon.object_index as i32 };
        let effect_object = self.host.effect_object(effect_object_index).unwrap_or_default();
        let mut markers: Vec<Matrix4x3> = effect_object
            .markers
            .as_deref()
            .map(|m| m.get_by_name(marker_name, &effect_object.region_permutations, &effect_object.node_matrices, effect_object.mirrored, 64))
            .unwrap_or_default()
            .into_iter()
            .map(|marker| marker.matrix)
            .collect();
        if markers.is_empty() {
            // (object_get_marker_by_name's stand-in: the root node)
            let mut root = effect_object.node_matrices.first().copied().unwrap_or_default();
            if effect_object.mirrored {
                root.left = [-root.left[0], -root.left[1], -root.left[2]];
            }
            markers.push(root);
        }
        let marker_count = if trigger_definition.flags & TRIGGER_USES_WEAPON_ORIGIN == 0 { 1 } else { markers.len() };

        for marker in markers.iter().take(marker_count) {
            let mut origin = marker.position;
            let mut forward = marker.forward;
            let mut velocity = 0.0f32;
            let mut error = 0.0f32;
            let mut target_object_index = NONE;

            if trigger_definition.flags & TRIGGER_PROJECTILES_CANNOT_BE_AIMED == 0
                && let Some(unit) = unit
                && !unit.dead
            {
                let (mut player_index, mut actor_index) = (unit.player_index, unit.actor_index);
                if unit.gunner_object_index != NONE {
                    player_index = unit.gunner_player_index;
                    actor_index = unit.gunner_actor_index;
                }
                let adjust_origin = unit.definition_flags & UNIT_FIRES_FROM_CAMERA != 0;
                let mut use_aiming_vector = true;
                if actor_index != NONE && self.call(weapon, call::FIRING_BLINDLY, actor_index, 0).result != 0 {
                    use_aiming_vector = false;
                }
                if unit.gunner_object_index != NONE {
                    use_aiming_vector = false;
                }
                let mut c = WeaponCall { kind: call::PROJECTILE_RAY, a: adjust_origin as i32, b: use_aiming_vector as i32, ..Default::default() };
                c.f[..3].copy_from_slice(&origin);
                c.f[3..6].copy_from_slice(&forward);
                c.result = self.host.call(weapon, &mut c);
                origin = [c.out[0], c.out[1], c.out[2]];
                forward = [c.out[3], c.out[4], c.out[5]];
                velocity = c.out[6];

                if player_index != NONE {
                    let mut right = cross3([0.0, 0.0, 1.0], forward);
                    if normalize3(&mut right) == 0.0 {
                        right = [0.0, 1.0, 0.0];
                    }
                    let mut up = cross3(forward, right);
                    normalize3(&mut up);
                    let offset = trigger_definition.first_person_weapon_offset;
                    origin = point_from_line3(origin, forward, offset[0]);
                    origin = point_from_line3(origin, right, offset[1]);
                    origin = point_from_line3(origin, up, offset[2]);
                    let mut c = WeaponCall { kind: call::PLAYER_AIM, a: player_index, ..Default::default() };
                    c.f[..3].copy_from_slice(&origin);
                    c.f[3..6].copy_from_slice(&forward);
                    target_object_index = self.host.call(weapon, &mut c);
                    forward = [c.out[3], c.out[4], c.out[5]];
                } else if actor_index != NONE {
                    let mut c = WeaponCall { kind: call::ACTOR_AIM, a: actor_index, ..Default::default() };
                    c.f[..3].copy_from_slice(&origin);
                    c.f[3..6].copy_from_slice(&forward);
                    c.f[6] = error;
                    target_object_index = self.host.call(weapon, &mut c);
                    forward = [c.out[3], c.out[4], c.out[5]];
                    error = c.out[6];
                }
            }

            if trigger_definition.flags & TRIGGER_USES_WEAPON_ORIGIN != 0 {
                origin = marker.position;
            }

            let (projectile_definition_index, projectile_count) = if trigger_index == 0 && weapon.alternate_shots_loaded > 0 {
                let secondary = definition.triggers.get(1).map_or(NONE, |t| t.projectile);
                let mut alternate_shots_loaded = weapon.alternate_shots_loaded;
                if definition.secondary_trigger_mode == SECONDARY_LOADS_MULTIPLE_PRIMARY_AMMUNITION {
                    alternate_shots_loaded += 1;
                }
                weapon.alternate_shots_loaded = 0;
                (secondary, (trigger_definition.projectiles_per_shot as i32 * alternate_shots_loaded as i32) as i16)
            } else {
                (trigger_definition.projectile, trigger_definition.projectiles_per_shot)
            };

            if projectile_definition_index != NONE {
                // (weapon_get_projectile_owner_object_index: its unit, or the
                // unit's gunner)
                let projectile_owner = match unit {
                    Some(unit) if unit.gunner_object_index != NONE => unit.gunner_object_index,
                    Some(_) => owner_object_index,
                    None => NONE,
                };
                let mut first_projectile_forward = [0.0f32; 3];
                for projectile_index in 0..projectile_count.max(0) {
                    let mut data_forward = forward;
                    let mut tracer = false;
                    {
                        let trigger = &mut weapon.triggers[trigger_index];
                        if trigger.rate_of_fire == 0.0 || {
                            let rounds = trigger.sequential_non_tracer_rounds;
                            trigger.sequential_non_tracer_rounds = rounds.wrapping_add(1);
                            rounds >= trigger_definition.rounds_between_tracers
                        } {
                            tracer = true;
                            trigger.sequential_non_tracer_rounds = 0;
                        }
                    }
                    if error == 0.0 {
                        let fraction = if trigger_definition.flags & TRIGGER_ANALOG_RATE_OF_FIRE != 0 {
                            weapon.primary_trigger
                        } else {
                            weapon.triggers[trigger_index].error
                        };
                        error = (1.0 - fraction) * trigger_definition.projectile_error_angle_lower_bound
                            + fraction * trigger_definition.projectile_error_angle_upper_bound;
                    }
                    if trigger_definition.flags & TRIGGER_USE_ERROR_WHEN_UNZOOMED == 0 || weapon.control_flags & CONTROL_ZOOMED == 0 {
                        data_forward = self.with_random(|r| r.vector_in_cone(data_forward, trigger_definition.projectile_error_inner_cone_angle, error));
                    }
                    if projectile_index == 0 {
                        first_projectile_forward = data_forward;
                    }
                    if trigger_definition.flags & TRIGGER_PROJECTILES_HAVE_IDENTICAL_ERROR != 0 {
                        data_forward = first_projectile_forward;
                    }
                    let mut up = perpendicular3(data_forward);
                    normalize3(&mut up);
                    projectile_distribute(
                        &mut data_forward,
                        up,
                        trigger_definition.projectile_distribution_function,
                        trigger_definition.projectile_distribution_angle,
                        projectile_index,
                        projectile_count,
                    );
                    let translational_velocity = [velocity * data_forward[0], velocity * data_forward[1], velocity * data_forward[2]];
                    let inside_bsp = unit.is_some_and(|u| u.player_index != NONE);
                    let mut c = WeaponCall {
                        kind: call::NEW_PROJECTILE,
                        a: trigger_definition.projectile,
                        b: projectile_owner,
                        c: target_object_index,
                        d: (tracer as i32) | ((inside_bsp as i32) << 1),
                        ..Default::default()
                    };
                    c.f[..3].copy_from_slice(&origin);
                    c.f[3..6].copy_from_slice(&data_forward);
                    c.f[6..9].copy_from_slice(&up);
                    c.f[9..12].copy_from_slice(&translational_velocity);
                    self.host.call(weapon, &mut c);
                }
            }
        }
    }
}

fn cross3(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

/// `point_from_line3d`: `v * t + p`.
fn point_from_line3(p: [f32; 3], v: [f32; 3], t: f32) -> [f32; 3] {
    [v[0] * t + p[0], v[1] * t + p[1], v[2] * t + p[2]]
}

/// `perpendicular3d`.
fn perpendicular3(a: [f32; 3]) -> [f32; 3] {
    let (i, j, k) = (a[0].abs(), a[1].abs(), a[2].abs());
    if i <= j && i <= k {
        [0.0, a[2], -a[1]]
    } else if j <= k {
        [-a[2], 0.0, a[0]]
    } else {
        [a[1], -a[0], 0.0]
    }
}

/// `rotate_vector_about_axis`.
fn rotate_vector_about_axis(v: &mut [f32; 3], n: [f32; 3], sine: f32, cosine: f32) {
    let one_minus_cosine_times_v_dot_n = (n[0] * v[0] + n[1] * v[1] + n[2] * v[2]) * (1.0 - cosine);
    let v_cross_n_i = n[2] * v[1] - v[2] * n[1];
    let v_cross_n_j = n[0] * v[2] - v[0] * n[2];
    let v_cross_n_k = n[1] * v[0] - v[1] * n[0];
    v[0] = cosine * v[0] + one_minus_cosine_times_v_dot_n * n[0] - sine * v_cross_n_i;
    v[1] = cosine * v[1] + one_minus_cosine_times_v_dot_n * n[1] - sine * v_cross_n_j;
    v[2] = cosine * v[2] + one_minus_cosine_times_v_dot_n * n[2] - sine * v_cross_n_k;
}

/// `projectile_distribute`: a projectile of several turned across the fan.
fn projectile_distribute(forward: &mut [f32; 3], up: [f32; 3], function: i16, angle: f32, index: i16, count: i16) {
    let offset: f32 = if count & 1 != 0 {
        if index == 0 {
            0.0
        } else {
            let mut step = index - 1;
            if step & 1 != 0 {
                step >>= 1;
            } else {
                step = -(step >> 1);
            }
            step as f32
        }
    } else {
        let offset = (index >> 1) as f32 - 0.5;
        if index & 1 != 0 { -offset } else { offset }
    };
    let angle = offset * angle;
    if function == DISTRIBUTION_HORIZONTAL_FAN {
        rotate_vector_about_axis(forward, up, (angle as f64).sin() as f32, (angle as f64).cos() as f32);
    }
}

/// `weapon_trigger_can_fire_again`.
fn trigger_can_fire_again(definition: &WeaponDefinition, weapon: &WeaponState, trigger_index: usize) -> bool {
    let trigger = &weapon.triggers[trigger_index];
    let trigger_definition = &definition.triggers[trigger_index];
    let fraction = if trigger_definition.flags & TRIGGER_ANALOG_RATE_OF_FIRE != 0 { weapon.primary_trigger } else { trigger.rate_of_fire };
    let rate_of_fire = (trigger_definition.final_rate_of_fire - trigger_definition.initial_rate_of_fire) * fraction
        + trigger_definition.initial_rate_of_fire;
    let mut required_ticks = if rate_of_fire > 0.0001 { TICKS_PER_SECOND as f32 / rate_of_fire } else { 0.0 };
    if definition.age_rate_of_fire_penalty > 0.0 {
        required_ticks *= weapon.age * definition.age_rate_of_fire_penalty + 1.0;
    }
    let result = trigger.idle_ticks as f32 + 1.0 >= required_ticks;
    if trigger_definition.flags & TRIGGER_LATCHED != 0
        && weapon.item_flags & ITEM_BELONGS_TO_PLAYER != 0
        && trigger.flags & RELEASED_SINCE_LAST_SHOT == 0
    {
        return false;
    }
    result
}

/// `weapon_update`.
pub fn weapon_update(definition: &WeaponDefinition, inputs: &WeaponInputs, host: &dyn WeaponHost, weapon: &mut WeaponState) {
    let w = Weapon { definition, host, inputs };

    if weapon.tracked_object_index != NONE && !host.object_exists(weapon.tracked_object_index) {
        weapon.tracked_object_index = NONE;
    }

    if definition.has_animation_graph && weapon.animation_index as i32 != NONE {
        let mut seed = host.random_seed();
        let result = animation_update(
            definition.animation_graph.as_deref(),
            &mut seed,
            &mut weapon.animation_index,
            &mut weapon.animation_frame_index,
        );
        host.set_random_seed(seed);
        match result {
            update_result::KEY_FRAME => w.state_key_frame(weapon),
            update_result::WILL_RESTART_ON_NEXT_FRAME => w.state_next(weapon),
            _ => {}
        }
    }

    if definition.flags & DETONATES_WHEN_DROPPED != 0 && weapon.parent_object_index == NONE {
        w.call(weapon, call::DETONATE_ITEM, 0, 0);
    }

    if weapon.integrated_light_power > 0.0
        && !(weapon.parent_object_index != NONE && host.unit_light_controls_weapon(weapon.parent_object_index))
    {
        weapon.integrated_light_power -= 1.0 / 24.0;
        if weapon.integrated_light_power < 0.0 {
            weapon.integrated_light_power = 0.0;
        }
    }

    if weapon.heat > 0.0 {
        if weapon.heat >= definition.heat_overheated_threshold && weapon.flags & OVERHEATED == 0 {
            weapon.flags |= OVERHEATED;
            let message = if definition.weapon_type == WEAPON_TYPE_PLASMA_PISTOL && weapon.flags & OVERHEAT_RECOIL != 0 {
                weapon.flags &= !OVERHEAT_RECOIL;
                MESSAGE_OVERHEATING_SUPER_RECOIL
            } else {
                MESSAGE_OVERHEATING
            };
            w.call(weapon, call::MESSAGE, message, 0);
            weapon.overheated_effect_index = w.call(weapon, call::LOOPING_EFFECT, definition.overheated_effect, 0).result;
        }

        if weapon.overcharged == 0.0 {
            let mut heat_loss = definition.heat_loss_per_second * (1.0 / TICKS_PER_SECOND as f32);
            if definition.age_heat_recovery_penalty > 0.0 {
                heat_loss *= 1.0 - weapon.age * definition.age_heat_recovery_penalty;
            }
            weapon.heat -= heat_loss;
            if weapon.heat < 0.0 {
                weapon.heat = 0.0;
            }
            if weapon.flags & OVERHEATED != 0
                && weapon.flags & OVERHEATED_EXIT == 0
                && (weapon.heat - definition.heat_recovery_threshold) / heat_loss <= 1.0
            {
                weapon.flags |= OVERHEATED_EXIT;
            }
        }

        if weapon.flags & OVERHEATED != 0 && weapon.heat < definition.heat_recovery_threshold {
            weapon.flags &= !(OVERHEATED | OVERHEATED_EXIT);
            if weapon.overheated_effect_index != NONE {
                let effect = weapon.overheated_effect_index;
                w.call(weapon, call::EFFECT_STOP, effect, 0);
            }
        }
    }

    weapon.overcharged = 0.0;
    if weapon.state_timer > 0 {
        weapon.state_timer -= 1;
    }

    let mut triggers_down = if weapon.control_flags & CONTROL_USER_BUSY == 0 && weapon.state_timer <= 0 {
        [
            weapon.control_flags & CONTROL_PRIMARY_TRIGGER != 0,
            definition.flags & SECONDARY_TRIGGER_OVERRIDES_GRENADES != 0 && weapon.control_flags & CONTROL_SECONDARY_TRIGGER != 0,
        ]
    } else {
        [false, false]
    };

    match definition.secondary_trigger_mode {
        SECONDARY_SLAVED_TO_PRIMARY => {
            if triggers_down[1] && definition.trigger_count > 0 && weapon.triggers[0].rate_of_fire != 1.0 {
                triggers_down[1] = false;
            }
        }
        SECONDARY_INHIBITS_PRIMARY => {
            if triggers_down[1] {
                triggers_down[0] = false;
            }
        }
        _ => {}
    }

    if weapon.control_flags & CONTROL_RELOAD != 0 && definition.magazine_count > 0 {
        weapon.flags |= NEEDS_TO_RELOAD;
    }
    if weapon.flags & NEEDS_TO_RELOAD != 0 {
        w.magazine_start_reload(weapon, 0, true);
    }

    for magazine_index in 0..definition.magazine_count() {
        let magazine_definition = definition.magazines[magazine_index];
        {
            let magazine = &mut weapon.magazines[magazine_index];
            if magazine_definition.rounds_recharged_per_second > 0 && magazine.rounds_loaded < magazine_definition.rounds_loaded_maximum {
                let rounds_recharged = (magazine_definition.rounds_recharged_per_second as i32 / TICKS_PER_SECOND) as i16;
                let fractional_rounds_recharged = (magazine_definition.rounds_recharged_per_second as i32 % TICKS_PER_SECOND) as i16;
                magazine.rounds_loaded = magazine.rounds_loaded.wrapping_add(rounds_recharged);
                magazine.rounds_fractional_recharged = magazine.rounds_fractional_recharged.wrapping_add(fractional_rounds_recharged);
                if magazine.rounds_fractional_recharged as i32 >= TICKS_PER_SECOND {
                    magazine.rounds_loaded = magazine.rounds_loaded.wrapping_add(1);
                    magazine.rounds_fractional_recharged -= TICKS_PER_SECOND as i16;
                }
                if magazine.rounds_loaded > magazine_definition.rounds_loaded_maximum {
                    magazine.rounds_loaded = magazine_definition.rounds_loaded_maximum;
                }
            }
            if magazine.state_timer != 0 {
                magazine.state_timer -= 1;
            }
        }
        let magazine = weapon.magazines[magazine_index];
        match magazine.state {
            MAGAZINE_RELOADING => {
                if magazine.state_timer as i32 - 1 <= 0 {
                    w.magazine_finish_reload(weapon, magazine_index);
                }
            }
            MAGAZINE_UNCHAMBERED => w.magazine_start_chamber(weapon, magazine_index),
            MAGAZINE_CHAMBERING => {
                if magazine.state_timer == 0 {
                    w.magazine_finish_chamber(weapon, magazine_index);
                }
            }
            _ => {}
        }
    }

    for trigger_index in 0..definition.trigger_count() {
        let trigger_definition = &definition.triggers[trigger_index];

        if trigger_definition.flags & TRIGGER_ANALOG_RATE_OF_FIRE != 0 && weapon.item_flags & ITEM_BELONGS_TO_PLAYER != 0 {
            triggers_down[trigger_index] = weapon.primary_trigger > 0.05;
        }
        if trigger_definition.flags & TRIGGER_STICKS_WHEN_DROPPED != 0 && weapon.parent_object_index == NONE {
            triggers_down[trigger_index] = true;
        }
        let down = &mut triggers_down[trigger_index];
        {
            let trigger = &mut weapon.triggers[trigger_index];
            if trigger.state_timer != 0 {
                trigger.state_timer -= 1;
            }
            if trigger_definition.flags & TRIGGER_TOGGLES != 0 {
                if trigger.flags & WAS_DOWN == 0 && *down {
                    trigger.flags ^= TOGGLED;
                }
                if *down {
                    trigger.flags |= WAS_DOWN;
                } else {
                    trigger.flags &= !WAS_DOWN;
                }
                *down = trigger.flags & TOGGLED != 0;
            }
            if !*down {
                trigger.flags |= RELEASED_SINCE_LAST_SHOT;
            }
            if trigger.ejection_port_position > 0.0 {
                trigger.ejection_port_position -= trigger_definition.ejection_port_recovery_time;
                if trigger.ejection_port_position <= 0.0 {
                    trigger.ejection_port_position = 0.0;
                }
            }
            if trigger.illumination > 0.0 {
                trigger.illumination -= trigger_definition.illumination_recovery_time;
                if trigger.illumination <= 0.0 {
                    trigger.illumination = 0.0;
                }
            }
        }
        let down = *down;

        match weapon.triggers[trigger_index].state {
            TRIGGER_IDLE => {
                if weapon.control_flags & CONTROL_USER_BUSY == 0
                    && weapon.parent_object_index != NONE
                    && trigger_definition.magazine_index as i32 != NONE
                    && definition.trigger_magazine_valid(trigger_definition)
                {
                    let magazine_index = trigger_definition.magazine_index as usize;
                    if let Some(magazine) = weapon.magazines.get(magazine_index)
                        && ((magazine.rounds_loaded < trigger_definition.rounds_per_shot
                            && trigger_definition.flags & TRIGGER_CAN_FIRE_WITH_PARTIAL_AMMUNITION == 0)
                            || magazine.rounds_loaded < trigger_definition.minimum_rounds_loaded_per_shot
                            || magazine.rounds_loaded == 0)
                    {
                        w.magazine_start_reload(weapon, magazine_index, true);
                    }
                }
                if down && trigger_can_fire_again(definition, weapon, trigger_index) {
                    w.trigger_begin_firing(weapon, trigger_index, false);
                } else if weapon.triggers[trigger_index].idle_ticks < 127 {
                    weapon.triggers[trigger_index].idle_ticks += 1;
                }
            }
            TRIGGER_SPEWING => {
                if weapon.triggers[trigger_index].state_timer != 0 {
                    w.trigger_begin_firing(weapon, trigger_index, true);
                } else {
                    w.trigger_recover(weapon, trigger_index);
                }
            }
            TRIGGER_OVERLOADING => {
                if !down {
                    w.trigger_begin_firing(weapon, trigger_index, true);
                } else if weapon.triggers[trigger_index].state_timer == 0
                    && weapon.alternate_shots_loaded < definition.maximum_alternate_shots_loaded
                {
                    w.trigger_overload(weapon, trigger_index);
                }
            }
            TRIGGER_CHARGING => {
                if weapon.triggers[trigger_index].state_timer != 0 {
                    if !down {
                        if trigger_index == 0
                            && definition.trigger_count > 1
                            && weapon.triggers[trigger_index].flags & FIRED_BEFORE_CHARGING == 0
                        {
                            w.trigger_begin_firing(weapon, trigger_index, true);
                        } else {
                            w.trigger_idle(weapon, trigger_index);
                        }
                        let effect = weapon.triggers[trigger_index].charging_effect_index;
                        if effect != NONE {
                            w.call(weapon, call::EFFECT_STOP, effect, 0);
                            weapon.triggers[trigger_index].charging_effect_index = NONE;
                        }
                    }
                } else {
                    w.trigger_fully_charged(weapon, trigger_index);
                }
            }
            TRIGGER_CHARGED => {
                if down {
                    weapon.overcharged = 1.0
                        - (weapon.triggers[trigger_index].state_timer as f32 * (1.0 / TICKS_PER_SECOND as f32))
                            / trigger_definition.charged_time;
                    if weapon.triggers[trigger_index].state_timer != 0 {
                        // (no magazine reads the slot just before the
                        // magazines: the second trigger's charging effect's
                        // low half is its rounds loaded)
                        let rounds_loaded = if trigger_definition.magazine_index as i32 == NONE {
                            Some(weapon.triggers[1].charging_effect_index as u32 as u16 as i16)
                        } else if definition.trigger_magazine_valid(trigger_definition) {
                            weapon.magazines.get(trigger_definition.magazine_index as usize).map(|m| m.rounds_loaded)
                        } else {
                            None
                        };
                        if let Some(rounds_loaded) = rounds_loaded
                            && rounds_loaded < trigger_definition.rounds_per_shot
                            && trigger_definition.flags & TRIGGER_CAN_FIRE_WITH_PARTIAL_AMMUNITION == 0
                        {
                            w.trigger_release_charge(weapon, trigger_index);
                        }
                    } else {
                        w.trigger_overcharged(weapon, trigger_index);
                    }
                } else {
                    w.trigger_release_charge(weapon, trigger_index);
                }
            }
            TRIGGER_RECOVERING => {
                if weapon.triggers[trigger_index].state_timer == 0 {
                    if trigger_definition.flags & TRIGGER_LATCHED != 0
                        && weapon.item_flags & ITEM_BELONGS_TO_PLAYER != 0
                        && weapon.triggers[trigger_index].flags & RELEASED_SINCE_LAST_SHOT == 0
                    {
                        w.trigger_locked(weapon, trigger_index);
                    } else {
                        w.trigger_idle(weapon, trigger_index);
                    }
                }
            }
            TRIGGER_TRACKING => {
                if !down || weapon.tracked_object_index == NONE {
                    w.trigger_finish_tracking(weapon, trigger_index);
                }
            }
            TRIGGER_LOCKED => {
                if !down {
                    w.trigger_idle(weapon, trigger_index);
                }
            }
            TRIGGER_UNINITIALIZED => {
                if weapon.triggers[trigger_index].state_timer == 0 {
                    w.trigger_idle(weapon, trigger_index);
                }
            }
            _ => {}
        }

        if down {
            let trigger = &mut weapon.triggers[trigger_index];
            trigger.rate_of_fire += trigger_definition.rate_of_fire_acceleration_time;
            if trigger.rate_of_fire > 1.0 {
                trigger.rate_of_fire = 1.0;
            }
            if trigger_definition.blurred_rate_of_fire != 0.0
                && trigger.flags & BLURRED == 0
                && trigger.rate_of_fire > trigger_definition.blurred_rate_of_fire
            {
                w.call(weapon, call::PERMUTE_BLURRED, trigger_index as i32, 1);
                weapon.triggers[trigger_index].flags |= BLURRED;
            }
        } else {
            let trigger = &mut weapon.triggers[trigger_index];
            trigger.rate_of_fire -= trigger_definition.rate_of_fire_deceleration_time;
            if trigger.rate_of_fire < 0.0 {
                trigger.rate_of_fire = 0.0;
            }
            if trigger.flags & BLURRED != 0 && trigger.rate_of_fire < trigger_definition.blurred_rate_of_fire {
                w.call(weapon, call::PERMUTE_BLURRED, trigger_index as i32, 0);
                weapon.triggers[trigger_index].flags &= !BLURRED;
            }
        }

        let trigger = &mut weapon.triggers[trigger_index];
        if trigger.state == TRIGGER_SPEWING || trigger.state == TRIGGER_RECOVERING || down {
            trigger.error += trigger_definition.error_acceleration_time;
            if trigger.error > 1.0 {
                trigger.error = 1.0;
            }
        } else {
            trigger.error -= trigger_definition.error_deceleration_time;
            if trigger.error < 0.0 {
                trigger.error = 0.0;
            }
        }
    }
}
