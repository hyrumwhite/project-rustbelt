//! Halo's weapon update (ROADMAP phase 7, objects and physics): weapons.c's
//! `weapon_update` with `weapon_trigger_can_fire_again`, ported as they
//! are, in the same single-precision arithmetic.
//!
//! Each tick, for each weapon: its animation (whose key frames and ends
//! move its state on), detonating when dropped if it does, its integrated
//! light dimming, its heat (overheating, cooling by its age, recovering),
//! which triggers are down (the controls, the secondary trigger's mode,
//! analog, sticky and toggling triggers), reloading, each magazine's
//! recharge and reload and chamber timers, and each trigger's state
//! machine (firing, spewing, overloading, charging, recovering, tracking,
//! latching), rate of fire (with its blurred permutation) and error.
//!
//! What a weapon's helpers do (firing, reloading, chambering, charging,
//! effects, messages) is the game's, through a [`WeaponHost`] ([`call`]'s
//! kinds), at the moments the C calls them.

// (the C's shape: its switch arms' tests, its index loops over the
// triggers, and its counters)
#![allow(clippy::collapsible_match, clippy::needless_range_loop, clippy::manual_saturating_arithmetic, clippy::implicit_saturating_add)]

use std::sync::Arc;

use crate::unit_animation::{AnimationGraph, animation_update, update_result};
use crate::{CacheFile, Result, layout};

const NONE: i32 = -1;
const TICKS_PER_SECOND: i32 = 30;

// weapon definition flags
const DETONATES_WHEN_DROPPED: u32 = 1 << 10;
const SECONDARY_TRIGGER_OVERRIDES_GRENADES: u32 = 1 << 12;
// trigger definition flags
const TRIGGER_CAN_FIRE_WITH_PARTIAL_AMMUNITION: u32 = 1 << 2;
const TRIGGER_LATCHED: u32 = 1 << 3;
const TRIGGER_TOGGLES: u32 = 1 << 4;
const TRIGGER_STICKS_WHEN_DROPPED: u32 = 1 << 6;
const TRIGGER_ANALOG_RATE_OF_FIRE: u32 = 1 << 9;
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
// trigger datum flags
const RELEASED_SINCE_LAST_SHOT: u32 = 1 << 0;
const WAS_DOWN: u32 = 1 << 1;
const TOGGLED: u32 = 1 << 2;
const BLURRED: u32 = 1 << 4;
const FIRED_BEFORE_CHARGING: u32 = 1 << 5;
// item flags
const ITEM_BELONGS_TO_PLAYER: u32 = 1 << 1;
// secondary trigger modes
const SECONDARY_SLAVED_TO_PRIMARY: i16 = 1;
const SECONDARY_INHIBITS_PRIMARY: i16 = 2;
// weapon types
const WEAPON_TYPE_PLASMA_PISTOL: i16 = 3;
// first person weapon messages
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
const MAGAZINE_RELOADING: i16 = 1;
const MAGAZINE_UNCHAMBERED: i16 = 2;
const MAGAZINE_CHAMBERING: i16 = 3;

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
}

const _: () = assert!(std::mem::size_of::<WeaponState>() == 160);

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct MagazineDefinition {
    pub rounds_recharged_per_second: i16,
    pub rounds_loaded_maximum: i16,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct TriggerDefinition {
    pub flags: u32,
    pub initial_rate_of_fire: f32,
    pub final_rate_of_fire: f32,
    pub blurred_rate_of_fire: f32,
    pub magazine_index: i16,
    pub rounds_per_shot: i16,
    pub minimum_rounds_loaded_per_shot: i16,
    pub charged_time: f32,
    pub illumination_recovery_time: f32,
    pub ejection_port_recovery_time: f32,
    pub rate_of_fire_acceleration_time: f32,
    pub rate_of_fire_deceleration_time: f32,
    pub error_acceleration_time: f32,
    pub error_deceleration_time: f32,
}

/// A weapon definition's (`_weapon_definition`'s) heat, age, triggers and
/// magazines, and its animation graph.
#[derive(Debug, Clone, Default)]
pub struct WeaponDefinition {
    pub flags: u32,
    pub secondary_trigger_mode: i16,
    pub maximum_alternate_shots_loaded: i16,
    pub heat_recovery_threshold: f32,
    pub heat_overheated_threshold: f32,
    pub heat_loss_per_second: f32,
    pub age_heat_recovery_penalty: f32,
    pub age_rate_of_fire_penalty: f32,
    pub weapon_type: i16,
    /// The blocks' counts as the tag has them.
    pub magazine_count: i32,
    pub trigger_count: i32,
    pub magazines: Vec<MagazineDefinition>,
    pub triggers: Vec<TriggerDefinition>,
    pub animation_graph: Option<Arc<AnimationGraph>>,
    pub has_animation_graph: bool,
}

impl WeaponDefinition {
    pub fn load(cache: &CacheFile, datum: u32, graph: Option<Arc<AnimationGraph>>) -> Result<Self> {
        use layout::{_weapon_definition as w, weapon_magazine_definition as m, weapon_trigger_definition as t};
        let root = cache.tag_root(datum, crate::groups::OBJECT)?;
        let weapon = root.add(layout::weapon_definition::WEAPON);
        let magazine_block = weapon.block(w::MAGAZINES, m::SIZEOF)?;
        let trigger_block = weapon.block(w::TRIGGERS, t::SIZEOF)?;
        let mut magazines = Vec::new();
        for magazine in magazine_block.iter() {
            magazines.push(MagazineDefinition {
                rounds_recharged_per_second: magazine.i16(m::ROUNDS_RECHARGED_PER_SECOND)?,
                rounds_loaded_maximum: magazine.i16(m::ROUNDS_LOADED_MAXIMUM)?,
            });
        }
        let mut triggers = Vec::new();
        for trigger in trigger_block.iter() {
            triggers.push(TriggerDefinition {
                flags: trigger.u32(t::FLAGS)?,
                initial_rate_of_fire: trigger.f32(t::INITIAL_RATE_OF_FIRE)?,
                final_rate_of_fire: trigger.f32(t::FINAL_RATE_OF_FIRE)?,
                blurred_rate_of_fire: trigger.f32(t::BLURRED_RATE_OF_FIRE)?,
                magazine_index: trigger.i16(t::MAGAZINE_INDEX)?,
                rounds_per_shot: trigger.i16(t::ROUNDS_PER_SHOT)?,
                minimum_rounds_loaded_per_shot: trigger.i16(t::MINIMUM_ROUNDS_LOADED_PER_SHOT)?,
                charged_time: trigger.f32(t::CHARGED_TIME)?,
                illumination_recovery_time: trigger.f32(t::RUNTIME_ILLUMINATION_RECOVERY_TIME)?,
                ejection_port_recovery_time: trigger.f32(t::RUNTIME_EJECTION_PORT_RECOVERY_TIME)?,
                rate_of_fire_acceleration_time: trigger.f32(t::RUNTIME_RATE_OF_FIRE_ACCELERATION_TIME)?,
                rate_of_fire_deceleration_time: trigger.f32(t::RUNTIME_RATE_OF_FIRE_DECELERATION_TIME)?,
                error_acceleration_time: trigger.f32(t::RUNTIME_ERROR_ACCELERATION_TIME)?,
                error_deceleration_time: trigger.f32(t::RUNTIME_ERROR_DECELERATION_TIME)?,
            });
        }
        let has_animation_graph = root.reference(layout::_object_definition::ANIMATION_GRAPH)?.datum.is_some();
        Ok(WeaponDefinition {
            flags: weapon.u32(w::FLAGS)?,
            secondary_trigger_mode: weapon.i16(w::SECONDARY_TRIGGER_MODE)?,
            maximum_alternate_shots_loaded: weapon.i16(w::MAXIMUM_ALTERNATE_SHOTS_LOADED)?,
            heat_recovery_threshold: weapon.f32(w::HEAT_RECOVERY_THRESHOLD)?,
            heat_overheated_threshold: weapon.f32(w::HEAT_OVERHEATED_THRESHOLD)?,
            heat_loss_per_second: weapon.f32(w::HEAT_LOSS_PER_SECOND)?,
            age_heat_recovery_penalty: weapon.f32(w::AGE_HEAT_RECOVERY_PENALTY)?,
            age_rate_of_fire_penalty: weapon.f32(w::AGE_RATE_OF_FIRE_PENALTY)?,
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

/// What a weapon's update asks of the game (its helpers: `a` a trigger or
/// magazine index, or a message; `b` a flag).
pub mod call {
    /// `weapon_state_key_frame`.
    pub const STATE_KEY_FRAME: i32 = 1;
    /// `weapon_state_next`.
    pub const STATE_NEXT: i32 = 2;
    /// `item_detonate`.
    pub const DETONATE: i32 = 3;
    /// Overheating: the first person weapon's message `a`, and the
    /// definition's looping overheated effect (its index the weapon's).
    pub const OVERHEATED: i32 = 4;
    /// `effect_stop(a, TRUE)`.
    pub const EFFECT_STOP: i32 = 5;
    /// `weapon_magazine_start_reload(weapon, a, TRUE)`.
    pub const MAGAZINE_START_RELOAD: i32 = 6;
    pub const MAGAZINE_FINISH_RELOAD: i32 = 7;
    pub const MAGAZINE_START_CHAMBER: i32 = 8;
    pub const MAGAZINE_FINISH_CHAMBER: i32 = 9;
    /// `weapon_trigger_begin_firing(weapon, a, b)`.
    pub const TRIGGER_BEGIN_FIRING: i32 = 10;
    pub const TRIGGER_RECOVER: i32 = 11;
    pub const TRIGGER_OVERLOAD: i32 = 12;
    pub const TRIGGER_IDLE: i32 = 13;
    pub const TRIGGER_FULLY_CHARGED: i32 = 14;
    pub const TRIGGER_RELEASE_CHARGE: i32 = 15;
    pub const TRIGGER_OVERCHARGED: i32 = 16;
    pub const TRIGGER_LOCKED: i32 = 17;
    pub const TRIGGER_FINISH_TRACKING: i32 = 18;
    /// The trigger `a`'s blurred permutation on (`b` 1) or off.
    pub const PERMUTE_BLURRED: i32 = 19;
}

/// A running game, as a weapon's update asks it. The weapon's state is the
/// update's as it stands (the host writes it to the game first, and reads
/// back what the call changed).
pub trait WeaponHost {
    fn random_seed(&self) -> u32;
    fn set_random_seed(&self, seed: u32);
    fn call(&self, state: &mut WeaponState, kind: i32, a: i32, b: i32);
    /// Whether an object is there.
    fn object_exists(&self, index: i32) -> bool;
    /// Whether an object is a unit whose integrated light controls its
    /// weapon's directly.
    fn unit_light_controls_weapon(&self, index: i32) -> bool;
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
pub fn weapon_update(definition: &WeaponDefinition, host: &dyn WeaponHost, weapon: &mut WeaponState) {
    let call = |weapon: &mut WeaponState, kind: i32, a: i32, b: i32| host.call(weapon, kind, a, b);

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
            update_result::KEY_FRAME => call(weapon, call::STATE_KEY_FRAME, 0, 0),
            update_result::WILL_RESTART_ON_NEXT_FRAME => call(weapon, call::STATE_NEXT, 0, 0),
            _ => {}
        }
    }

    if definition.flags & DETONATES_WHEN_DROPPED != 0 && weapon.parent_object_index == NONE {
        call(weapon, call::DETONATE, 0, 0);
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
            call(weapon, call::OVERHEATED, message, 0);
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
                call(weapon, call::EFFECT_STOP, effect, 0);
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
        call(weapon, call::MAGAZINE_START_RELOAD, 0, 1);
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
                    call(weapon, call::MAGAZINE_FINISH_RELOAD, magazine_index as i32, 0);
                }
            }
            MAGAZINE_UNCHAMBERED => call(weapon, call::MAGAZINE_START_CHAMBER, magazine_index as i32, 0),
            MAGAZINE_CHAMBERING => {
                if magazine.state_timer == 0 {
                    call(weapon, call::MAGAZINE_FINISH_CHAMBER, magazine_index as i32, 0);
                }
            }
            _ => {}
        }
    }

    for trigger_index in 0..definition.trigger_count() {
        let trigger_definition = definition.triggers[trigger_index];
        let t = trigger_index as i32;

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
                    && definition.trigger_magazine_valid(&trigger_definition)
                {
                    let magazine_index = trigger_definition.magazine_index as usize;
                    if let Some(magazine) = weapon.magazines.get(magazine_index)
                        && ((magazine.rounds_loaded < trigger_definition.rounds_per_shot
                            && trigger_definition.flags & TRIGGER_CAN_FIRE_WITH_PARTIAL_AMMUNITION == 0)
                            || magazine.rounds_loaded < trigger_definition.minimum_rounds_loaded_per_shot
                            || magazine.rounds_loaded == 0)
                    {
                        call(weapon, call::MAGAZINE_START_RELOAD, magazine_index as i32, 1);
                    }
                }
                if down && trigger_can_fire_again(definition, weapon, trigger_index) {
                    call(weapon, call::TRIGGER_BEGIN_FIRING, t, 0);
                } else if weapon.triggers[trigger_index].idle_ticks < 127 {
                    weapon.triggers[trigger_index].idle_ticks += 1;
                }
            }
            TRIGGER_SPEWING => {
                if weapon.triggers[trigger_index].state_timer != 0 {
                    call(weapon, call::TRIGGER_BEGIN_FIRING, t, 1);
                } else {
                    call(weapon, call::TRIGGER_RECOVER, t, 0);
                }
            }
            TRIGGER_OVERLOADING => {
                if !down {
                    call(weapon, call::TRIGGER_BEGIN_FIRING, t, 1);
                } else if weapon.triggers[trigger_index].state_timer == 0
                    && weapon.alternate_shots_loaded < definition.maximum_alternate_shots_loaded
                {
                    call(weapon, call::TRIGGER_OVERLOAD, t, 0);
                }
            }
            TRIGGER_CHARGING => {
                if weapon.triggers[trigger_index].state_timer != 0 {
                    if !down {
                        if trigger_index == 0
                            && definition.trigger_count > 1
                            && weapon.triggers[trigger_index].flags & FIRED_BEFORE_CHARGING == 0
                        {
                            call(weapon, call::TRIGGER_BEGIN_FIRING, t, 1);
                        } else {
                            call(weapon, call::TRIGGER_IDLE, t, 0);
                        }
                        let effect = weapon.triggers[trigger_index].charging_effect_index;
                        if effect != NONE {
                            call(weapon, call::EFFECT_STOP, effect, 0);
                            weapon.triggers[trigger_index].charging_effect_index = NONE;
                        }
                    }
                } else {
                    call(weapon, call::TRIGGER_FULLY_CHARGED, t, 0);
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
                        } else if definition.trigger_magazine_valid(&trigger_definition) {
                            weapon.magazines.get(trigger_definition.magazine_index as usize).map(|m| m.rounds_loaded)
                        } else {
                            None
                        };
                        if let Some(rounds_loaded) = rounds_loaded
                            && rounds_loaded < trigger_definition.rounds_per_shot
                            && trigger_definition.flags & TRIGGER_CAN_FIRE_WITH_PARTIAL_AMMUNITION == 0
                        {
                            call(weapon, call::TRIGGER_RELEASE_CHARGE, t, 0);
                        }
                    } else {
                        call(weapon, call::TRIGGER_OVERCHARGED, t, 0);
                    }
                } else {
                    call(weapon, call::TRIGGER_RELEASE_CHARGE, t, 0);
                }
            }
            TRIGGER_RECOVERING => {
                if weapon.triggers[trigger_index].state_timer == 0 {
                    if trigger_definition.flags & TRIGGER_LATCHED != 0
                        && weapon.item_flags & ITEM_BELONGS_TO_PLAYER != 0
                        && weapon.triggers[trigger_index].flags & RELEASED_SINCE_LAST_SHOT == 0
                    {
                        call(weapon, call::TRIGGER_LOCKED, t, 0);
                    } else {
                        call(weapon, call::TRIGGER_IDLE, t, 0);
                    }
                }
            }
            TRIGGER_TRACKING => {
                if !down || weapon.tracked_object_index == NONE {
                    call(weapon, call::TRIGGER_FINISH_TRACKING, t, 0);
                }
            }
            TRIGGER_LOCKED => {
                if !down {
                    call(weapon, call::TRIGGER_IDLE, t, 0);
                }
            }
            TRIGGER_UNINITIALIZED => {
                if weapon.triggers[trigger_index].state_timer == 0 {
                    call(weapon, call::TRIGGER_IDLE, t, 0);
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
                call(weapon, call::PERMUTE_BLURRED, t, 1);
                weapon.triggers[trigger_index].flags |= BLURRED;
            }
        } else {
            let trigger = &mut weapon.triggers[trigger_index];
            trigger.rate_of_fire -= trigger_definition.rate_of_fire_deceleration_time;
            if trigger.rate_of_fire < 0.0 {
                trigger.rate_of_fire = 0.0;
            }
            if trigger.flags & BLURRED != 0 && trigger.rate_of_fire < trigger_definition.blurred_rate_of_fire {
                call(weapon, call::PERMUTE_BLURRED, t, 0);
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
