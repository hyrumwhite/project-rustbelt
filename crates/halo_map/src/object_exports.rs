//! Each object type's export of its function values (ROADMAP phase 7):
//! object_types.c's `object_type_export_function_values` over its parts,
//! `object_export_function_values`, `unit_`, `biped_`, `vehicle_`,
//! `weapon_`, `projectile_` and `device_export_function_values`, ported as
//! they are, in the same arithmetic (single precision, and double where the
//! C's `fabs` makes it so).
//!
//! An object's definition and its type's say what each of its four
//! incoming function values reads: its vitality and damage, a random
//! constant, whether it is alive, a compass, its regions' damage; a unit's
//! seats' power, aiming, mouth, light, blinking and shield sapping; a
//! biped's flying speed; a vehicle's speed, slide, turn, velocity, treads,
//! wheels, sideslip, hover, thrust and boost; a weapon's heat, ammunition,
//! triggers, illumination (which it keeps), age and light, into its first
//! visible ancestor's values; a projectile's range and time remaining; and a
//! device's power, position, lock and delay.

// (the C's clamps, as it writes them)
#![allow(clippy::manual_clamp)]

use crate::unit_animation::AnimationGraph;
use crate::{CacheFile, Result, layout};

const NONE: i32 = -1;
/// `_pi`, the same single-precision value.
const PI: f32 = std::f32::consts::PI;

// object types
const OBJECT_TYPE_BIPED: i16 = 0;
const OBJECT_TYPE_VEHICLE: i16 = 1;
const OBJECT_TYPE_WEAPON: i16 = 2;
const OBJECT_TYPE_PROJECTILE: i16 = 5;
const OBJECT_TYPE_MACHINE: i16 = 7;
const OBJECT_MASK_DEVICE: u32 = (1 << 7) | (1 << 8) | (1 << 9);
// object damage flags
const OBJECT_DEAD: u16 = 1 << 2;
// unit flags
const UNIT_CANNOT_BLINK: u32 = 1 << 22;
// weapon flags
const WEAPON_OVERHEATED: u32 = 1 << 0;
// machine flags
const MACHINE_DOES_NOT_OPERATE_AUTOMATICALLY: u32 = 1 << 0;
const MACHINE_ONE_SIDED: u32 = 1 << 1;
const MACHINE_NEVER_APPEARS_LOCKED: u32 = 1 << 2;
// device group flags
const GROUP_CAN_CHANGE_ONLY_ONCE: u16 = 1 << 0;
const GROUP_CHANGED_ONCE: u16 = 1 << 1;
// projectile flags
const PROJECTILE_TRACER: u32 = 1 << 1;
// trigger states
const TRIGGER_CHARGING: i8 = 2;
const TRIGGER_CHARGED: i8 = 3;

/// A weapon trigger, as its exports read (and keep its illumination).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ExportTrigger {
    pub state: i8,
    pub pad: i8,
    pub state_timer: i16,
    pub illumination: f32,
    pub ejection_port_position: f32,
    pub rate_of_fire: f32,
}

/// An object, as its type's exports read and change it (the game's fields,
/// laid out for the game's side, `halo_sim.c`; a type's part zero for
/// another type).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ExportState {
    pub object_index: u32,
    pub definition_index: u32,
    pub object_type: i16,
    pub pad0: i16,
    pub random_seed: u32,
    pub incoming_function_values: [f32; 4],
    // the object's
    pub current_body_damage: f32,
    pub current_shield_damage: f32,
    pub body_vitality: f32,
    pub shield_vitality: f32,
    pub damage_flags: u16,
    pub pad1: u16,
    pub region_damage: [u8; 8],
    pub node_forward: [f32; 3],
    pub local_north: f32,
    pub object_flags: u32,
    pub translational_velocity: [f32; 3],
    pub forward: [f32; 3],
    pub up: [f32; 3],
    // a unit's
    pub seat_power: [f32; 2],
    pub aiming_change: u8,
    pub pad2: u8,
    pub shield_sap_timeout: i8,
    pub pad3: i8,
    pub mouth_aperture: f32,
    pub unit_integrated_light_power: f32,
    pub unit_flags: u32,
    pub animation_graph_index: u32,
    pub animation_index: i16,
    pub pad4: i16,
    // a vehicle's
    pub speed: f32,
    pub slide: f32,
    pub turn: f32,
    pub wheel: f32,
    pub left_tread: f32,
    pub right_tread: f32,
    pub hover: f32,
    pub thrust: f32,
    pub vehicle_flags: u16,
    pub airborne_ticks: u8,
    pub pad5: u8,
    // a weapon's
    pub heat: f32,
    pub overcharged: f32,
    pub weapon_integrated_light_power: f32,
    pub age: f32,
    pub weapon_flags: u32,
    pub game_time_last_fired: i32,
    pub game_time: i32,
    pub triggers: [ExportTrigger; 2],
    pub rounds_loaded: [i16; 2],
    /// Its first visible ancestor (itself if it is visible), whose values it
    /// exports into.
    pub target_object_index: u32,
    pub target_incoming_function_values: [f32; 4],
    // a device's
    pub power: f32,
    pub power_velocity: f32,
    pub position: f32,
    pub position_velocity: f32,
    pub delay_ticks: i16,
    pub position_group_index: i16,
    pub machine_flags: u32,
    pub group_flags: u16,
    pub pad6: u16,
    // a projectile's
    pub odometer: f32,
    pub detonation_timer: f32,
    pub projectile_flags: u32,
}

const _: () = assert!(std::mem::size_of::<ExportState>() == 308);

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ExportTriggerDefinition {
    pub charging_time: f32,
    pub charged_illumination: f32,
}

/// What an object's definition and its type's say its exports read.
#[derive(Debug, Clone, Default)]
pub struct ExportDefinition {
    pub object_function_modes: [i16; 4],
    pub unit_function_modes: [i16; 4],
    pub biped_function_modes: [i16; 4],
    pub flying_velocity: f32,
    pub vehicle_function_modes: [i16; 4],
    /// The vehicle's speed block (`unknown2f8`, `unknown2fc`), turn block
    /// (`unknown308`, `unknown30c`), slides (`unknown330`, `unknown334`) and
    /// wheel circumference.
    pub vehicle_speeds: [f32; 2],
    pub vehicle_turns: [f32; 2],
    pub vehicle_slides: [f32; 2],
    pub wheel_circumference: f32,
    pub weapon_function_modes: [i16; 4],
    pub heat_recovery_threshold: f32,
    pub heat_illumination: f32,
    pub magazine_count: i32,
    pub rounds_loaded_maximum: [i16; 2],
    pub trigger_count: i32,
    pub triggers: [ExportTriggerDefinition; 2],
    pub projectile_function_inputs: [i16; 4],
    pub maximum_range: f32,
    pub device_function_modes: [i16; 4],
    pub maximum_power_velocity: f32,
    pub maximum_powered_position_velocity: f32,
    pub delay_ticks: f32,
}

fn modes(p: crate::cache::Ptr, rel: usize) -> Result<[i16; 4]> {
    Ok([p.i16(rel)?, p.i16(rel + 2)?, p.i16(rel + 4)?, p.i16(rel + 6)?])
}

impl ExportDefinition {
    pub fn load(cache: &CacheFile, datum: u32, object_type: i16) -> Result<Self> {
        let root = cache.tag_root(datum, crate::groups::OBJECT)?;
        let mut d = ExportDefinition { object_function_modes: modes(root, layout::_object_definition::FUNCTION_MODES)?, ..Default::default() };
        if object_type == OBJECT_TYPE_BIPED || object_type == OBJECT_TYPE_VEHICLE {
            d.unit_function_modes = modes(root.add(layout::unit_definition::UNIT), layout::_unit_definition::FUNCTION_MODES)?;
        }
        if object_type == OBJECT_TYPE_BIPED {
            let biped = root.add(layout::biped_definition::BIPED);
            d.biped_function_modes = modes(biped, layout::_biped_definition::FUNCTION_MODES)?;
            d.flying_velocity = biped.f32(layout::_biped_definition::FLYING_VELOCITY)?;
        }
        if object_type == OBJECT_TYPE_VEHICLE {
            use layout::vehicle_definition as v;
            d.vehicle_function_modes = modes(root, v::FUNCTION_MODES)?;
            d.vehicle_speeds = [root.f32(v::UNKNOWN2F8)?, root.f32(v::UNKNOWN2FC)?];
            d.vehicle_turns = [root.f32(v::UNKNOWN308)?, root.f32(v::UNKNOWN30C)?];
            d.vehicle_slides = [root.f32(v::UNKNOWN330)?, root.f32(v::UNKNOWN334)?];
            d.wheel_circumference = root.f32(v::WHEEL_CIRCUMFERENCE)?;
        }
        if object_type == OBJECT_TYPE_WEAPON {
            use layout::{_weapon_definition as w, weapon_magazine_definition as m, weapon_trigger_definition as t};
            let weapon = root.add(layout::weapon_definition::WEAPON);
            d.weapon_function_modes = modes(weapon, w::FUNCTION_MODES)?;
            d.heat_recovery_threshold = weapon.f32(w::HEAT_RECOVERY_THRESHOLD)?;
            d.heat_illumination = weapon.f32(w::HEAT_ILLUMINATION)?;
            let magazines = weapon.block(w::MAGAZINES, m::SIZEOF)?;
            d.magazine_count = magazines.iter().count() as i32;
            for (index, magazine) in magazines.iter().take(2).enumerate() {
                d.rounds_loaded_maximum[index] = magazine.i16(m::ROUNDS_LOADED_MAXIMUM)?;
            }
            let triggers = weapon.block(w::TRIGGERS, t::SIZEOF)?;
            d.trigger_count = triggers.iter().count() as i32;
            for (index, trigger) in triggers.iter().take(2).enumerate() {
                d.triggers[index] =
                    ExportTriggerDefinition { charging_time: trigger.f32(t::CHARGING_TIME)?, charged_illumination: trigger.f32(t::CHARGED_ILLUMINATION)? };
            }
        }
        if object_type == OBJECT_TYPE_PROJECTILE {
            let projectile = root.add(layout::projectile_definition::PROJECTILE);
            d.projectile_function_inputs = modes(projectile, layout::_projectile_definition::FUNCTION_INPUTS)?;
            d.maximum_range = projectile.f32(layout::_projectile_definition::MAXIMUM_RANGE)?;
        }
        if object_type >= 0 && OBJECT_MASK_DEVICE & (1 << object_type as u32) != 0 {
            use layout::_device_definition as dv;
            let device = root.add(layout::device_definition::DEVICE);
            d.device_function_modes = modes(device, dv::FUNCTION_MODES)?;
            d.maximum_power_velocity = device.f32(dv::RUNTIME_MAXIMUM_POWER_VELOCITY)?;
            d.maximum_powered_position_velocity = device.f32(dv::RUNTIME_MAXIMUM_POWERED_POSITION_VELOCITY)?;
            d.delay_ticks = device.f32(dv::RUNTIME_DELAY_TICKS)?;
        }
        Ok(d)
    }
}

fn pin01(value: f32) -> f32 {
    if value < 0.0 {
        0.0
    } else if value > 1.0 {
        1.0
    } else {
        value
    }
}

fn max(a: f32, b: f32) -> f32 {
    if a > b { a } else { b }
}

fn dot3(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn magnitude3(v: [f32; 3]) -> f32 {
    dot3(v, v).sqrt()
}

/// `object_export_function_values`.
fn object_export(definition: &ExportDefinition, state: &mut ExportState) {
    for i in 0..4 {
        let mode = definition.object_function_modes[i];
        if mode == 0 {
            continue;
        }
        let value = match mode {
            1 => state.body_vitality,
            2 => {
                let value = state.shield_vitality;
                if value < 1.0 { value } else { 1.0 }
            }
            3 => state.current_body_damage,
            4 => state.current_shield_damage,
            5 => {
                if state.incoming_function_values[i] == 1.0 {
                    // (real_random)
                    state.random_seed = state.random_seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                    (state.random_seed >> 16) as f32 / 65535.0
                } else {
                    0.0
                }
            }
            18 => {
                if state.damage_flags & OBJECT_DEAD != 0 {
                    0.0
                } else {
                    1.0
                }
            }
            19 => {
                let forward = state.node_forward;
                if (forward[2].abs() as f64) < 0.995f32 as f64 {
                    let angle = (forward[0] as f64).atan2(forward[1] as f64) as f32;
                    // (signed_angular_difference)
                    let mut value = angle - state.local_north;
                    if value >= PI {
                        value -= PI * 2.0;
                    }
                    if value <= -PI {
                        value += PI * 2.0;
                    }
                    pin01(0.15915494 * value + 0.5)
                } else {
                    state.incoming_function_values[i]
                }
            }
            _ => {
                let region_index = mode as i32 - 10;
                if (0..8).contains(&region_index) { state.region_damage[region_index as usize] as f32 / 255.0 } else { 0.0 }
            }
        };
        state.incoming_function_values[i] = value;
    }
}

/// `unit_export_function_values`.
fn unit_export(definition: &ExportDefinition, graph: Option<&AnimationGraph>, state: &mut ExportState) {
    for i in 0..4 {
        let mode = definition.unit_function_modes[i];
        if mode == 0 {
            continue;
        }
        let value = match mode {
            1 => state.seat_power[0],
            2 => state.seat_power[1],
            3 => state.aiming_change as f32 * (1.0 / 255.0),
            4 => state.mouth_aperture,
            5 => state.unit_integrated_light_power,
            6 => {
                if state.damage_flags & OBJECT_DEAD == 0 && state.unit_flags & UNIT_CANNOT_BLINK == 0 {
                    1.0
                } else {
                    0.0
                }
            }
            7 => {
                // (the January executable compares and divides the animation's
                // index, not its frame: kept)
                let loop_frame_index = graph
                    .and_then(|g| usize::try_from(state.animation_index).ok().and_then(|i| g.animations.get(i)))
                    .map_or(0, |a| a.loop_frame_index);
                if state.animation_index >= loop_frame_index {
                    1.0 - state.shield_sap_timeout as f32 * (1.0 / 90.0)
                } else {
                    state.animation_index as f32 / loop_frame_index as f32
                }
            }
            _ => 0.0,
        };
        state.incoming_function_values[i] = value;
    }
}

/// `biped_export_function_values`.
fn biped_export(definition: &ExportDefinition, state: &mut ExportState) {
    for i in 0..4 {
        let mode = definition.biped_function_modes[i];
        if mode == 0 {
            continue;
        }
        let value = if mode == 1 {
            pin01(magnitude3(state.translational_velocity) / (definition.flying_velocity / 30.0))
        } else {
            0.0
        };
        state.incoming_function_values[i] = value;
    }
}

/// `vehicle_export_function_values`.
fn vehicle_export(definition: &ExportDefinition, state: &mut ExportState) {
    let forward_speed = definition.vehicle_speeds[0].abs();
    let reverse_speed = definition.vehicle_speeds[1].abs();
    let maximum_speed = max(forward_speed, reverse_speed);
    let left_slide = definition.vehicle_slides[0].abs();
    let right_slide = definition.vehicle_slides[1].abs();
    let maximum_slide = max(left_slide, right_slide);
    let left_turn = definition.vehicle_turns[0].abs();
    let right_turn = definition.vehicle_turns[1].abs();
    let maximum_turn = max(left_turn, right_turn);
    let (speed, slide, turn) = (state.speed, state.slide, state.turn);
    let velocity = state.translational_velocity;
    for i in 0..4 {
        let mode = definition.vehicle_function_modes[i];
        if mode == 0 {
            continue;
        }
        let result = match mode {
            1 | 28..=31 => speed.abs() / maximum_speed,
            2 => (if speed < 0.0 { 0.0 } else { speed }) / forward_speed,
            3 => (if speed > 0.0 { 0.0 } else { speed }).abs() / reverse_speed,
            4 => slide.abs() / maximum_slide,
            5 => slide.abs() / left_slide,
            6 => slide.abs() / right_slide,
            7 => {
                // (fabs's doubles)
                let a = speed.abs() as f64 / maximum_speed as f64;
                let b = slide.abs() as f64 / maximum_slide as f64;
                (if a > b { a } else { b }) as f32
            }
            8 => turn.abs() / maximum_turn,
            9 => turn.abs() / left_turn,
            10 => turn.abs() / right_turn,
            11 => {
                if state.vehicle_flags & (1 << 2) != 0 {
                    1.0
                } else {
                    0.0
                }
            }
            12 => {
                if state.vehicle_flags & (1 << 3) != 0 {
                    1.0
                } else {
                    0.0
                }
            }
            14 => magnitude3(velocity) / maximum_speed,
            15 => {
                if state.object_flags & ((1 << 2) | (1 << 3) | (1 << 4)) != 0 {
                    magnitude3(velocity) / maximum_speed
                } else {
                    0.0
                }
            }
            16 => {
                if state.object_flags & (1 << 1) != 0 {
                    magnitude3(velocity) / maximum_speed
                } else {
                    0.0
                }
            }
            17 => dot3(velocity, state.forward).abs() / maximum_speed,
            18 | 19 => dot3(velocity, state.up).abs() / maximum_speed,
            20 => state.left_tread / definition.wheel_circumference,
            21 => state.right_tread / definition.wheel_circumference,
            22 => (speed - turn).abs() / maximum_speed,
            23 => (turn + speed).abs() / maximum_speed,
            24..=27 => state.wheel / definition.wheel_circumference,
            32 => {
                // (component_vectors_from_normal3d)
                let vn = dot3(velocity, state.forward);
                let parallel = [vn * state.forward[0], vn * state.forward[1], vn * state.forward[2]];
                let across = [velocity[0] - parallel[0], velocity[1] - parallel[1], velocity[2] - parallel[2]];
                let result = magnitude3(across) * (1.0 / 0.3);
                result * result
            }
            33 => state.hover,
            34 => state.thrust,
            35 => {
                let dot_speed = dot3(velocity, state.forward).abs() / maximum_speed;
                let forward_value = speed.abs() / forward_speed;
                let blend = pin01((state.airborne_ticks as f32 * 0.2 + 1.0) * 0.5);
                dot_speed * (1.0 - blend) + forward_value * blend
            }
            36 => {
                let result = magnitude3(velocity) / definition.vehicle_speeds[0];
                (result * state.thrust - 0.05) * (1.0 / (0.9 - 0.05))
            }
            _ => 0.0,
        };
        state.incoming_function_values[i] = pin01(result);
    }
}

/// `weapon_trigger_get_charged_fraction`.
fn charged_fraction(definition: &ExportDefinition, state: &ExportState, trigger_index: usize) -> f32 {
    let Some(trigger) = state.triggers.get(trigger_index) else { return 0.0 };
    match trigger.state {
        TRIGGER_CHARGING => {
            1.0 - (trigger.state_timer as f32 * (1.0 / 30.0)) / definition.triggers[trigger_index].charging_time
        }
        TRIGGER_CHARGED => 1.0,
        _ => 0.0,
    }
}

/// `weapon_export_function_values`.
fn weapon_export(definition: &ExportDefinition, state: &mut ExportState) {
    let into_itself = state.target_object_index == state.object_index;
    for i in 0..4 {
        let mode = definition.weapon_function_modes[i];
        if mode == 0 {
            continue;
        }
        let mut value = 0.0;
        match mode {
            6 => value = 1.0,
            1 => value = state.heat,
            9 => {
                if state.weapon_flags & WEAPON_OVERHEATED != 0 && definition.heat_recovery_threshold != 1.0 {
                    value = (state.heat - definition.heat_recovery_threshold) / (1.0 - definition.heat_recovery_threshold);
                }
            }
            12 => {
                for trigger_index in 0..definition.trigger_count.clamp(0, 2) as usize {
                    let trigger_definition = definition.triggers[trigger_index];
                    if trigger_definition.charging_time > 0.0 {
                        let charged_illumination = charged_fraction(definition, state, trigger_index) * trigger_definition.charged_illumination;
                        value = max(value, charged_illumination);
                    }
                    if state.triggers[trigger_index].state == TRIGGER_CHARGED {
                        value = max(
                            value,
                            (1.0 - trigger_definition.charged_illumination) * state.overcharged + trigger_definition.charged_illumination,
                        );
                    }
                    value = max(value, state.triggers[trigger_index].illumination);
                    state.triggers[trigger_index].illumination = value;
                }
                value = max(value, definition.heat_illumination * state.heat);
            }
            2 | 3 => {
                let magazine_index = (mode - 2) as usize;
                if (magazine_index as i32) < definition.magazine_count && definition.rounds_loaded_maximum[magazine_index] != 0 {
                    value = state.rounds_loaded[magazine_index] as f32 / definition.rounds_loaded_maximum[magazine_index] as f32;
                }
            }
            7 | 8 => {
                let trigger_index = (mode - 7) as usize;
                if (trigger_index as i32) < definition.trigger_count {
                    value = state.triggers[trigger_index].ejection_port_position;
                }
            }
            4 | 5 => {
                let trigger_index = (mode - 4) as usize;
                if (trigger_index as i32) < definition.trigger_count {
                    value = state.triggers[trigger_index].rate_of_fire;
                }
            }
            15 | 16 => {
                let trigger_index = (mode - 15) as usize;
                if (trigger_index as i32) < definition.trigger_count {
                    value = state.triggers[trigger_index].rate_of_fire;
                    if state.game_time.wrapping_sub(state.game_time_last_fired) > 1 {
                        value = 0.0;
                    }
                }
            }
            10 | 11 => {
                let trigger_index = (mode - 10) as usize;
                if (trigger_index as i32) < definition.trigger_count {
                    value = charged_fraction(definition, state, trigger_index);
                }
            }
            14 => value = state.weapon_integrated_light_power,
            13 => value = state.age,
            _ => {}
        }
        if into_itself {
            state.incoming_function_values[i] = value;
        } else {
            state.target_incoming_function_values[i] = value;
        }
    }
}

/// `projectile_export_function_values`.
fn projectile_export(definition: &ExportDefinition, state: &mut ExportState) {
    for i in 0..4 {
        let value = match definition.projectile_function_inputs[i] {
            0 => continue,
            1 => {
                if definition.maximum_range != 0.0 {
                    state.odometer / definition.maximum_range
                } else {
                    0.0
                }
            }
            2 => state.detonation_timer,
            3 => {
                if state.projectile_flags & PROJECTILE_TRACER != 0 {
                    1.0
                } else {
                    0.0
                }
            }
            // (the game halts on any other)
            _ => continue,
        };
        state.incoming_function_values[i] = value;
    }
}

/// `device_export_function_values`.
fn device_export(definition: &ExportDefinition, state: &mut ExportState) {
    for i in 0..4 {
        let mode = definition.device_function_modes[i];
        if mode == 0 {
            continue;
        }
        let mut value = 0.0;
        match mode {
            1 => value = state.power,
            2 => {
                if state.power_velocity != 0.0 {
                    value = (state.power_velocity.abs() as f64 / definition.maximum_power_velocity as f64) as f32;
                }
            }
            3 => value = state.position,
            4 => {
                if state.position_velocity != 0.0 {
                    value = (state.position_velocity.abs() as f64 / definition.maximum_powered_position_velocity as f64) as f32;
                }
            }
            5 => {
                if state.power == 0.0 {
                    value = 1.0;
                }
                if state.object_type == OBJECT_TYPE_MACHINE && state.position_group_index as i32 != NONE {
                    if state.machine_flags & (MACHINE_DOES_NOT_OPERATE_AUTOMATICALLY | MACHINE_ONE_SIDED) != 0 {
                        value = 1.0;
                    }
                    if state.group_flags & GROUP_CAN_CHANGE_ONLY_ONCE != 0 && state.group_flags & GROUP_CHANGED_ONCE != 0 {
                        value = 1.0;
                    }
                    if state.position == 1.0 || state.machine_flags & MACHINE_NEVER_APPEARS_LOCKED != 0 {
                        value = 0.0;
                    }
                }
            }
            6 => {
                if definition.delay_ticks <= 0.0 || state.delay_ticks as f32 == definition.delay_ticks {
                    value = 0.0;
                } else {
                    value = state.delay_ticks as f32 / definition.delay_ticks;
                }
            }
            _ => {}
        }
        state.incoming_function_values[i] = value;
    }
}

/// `object_type_export_function_values`: the object's part, then its type's
/// parts in order.
pub fn object_type_export_function_values(definition: &ExportDefinition, graph: Option<&AnimationGraph>, state: &mut ExportState) {
    object_export(definition, state);
    match state.object_type {
        OBJECT_TYPE_BIPED => {
            unit_export(definition, graph, state);
            biped_export(definition, state);
        }
        OBJECT_TYPE_VEHICLE => {
            unit_export(definition, graph, state);
            vehicle_export(definition, state);
        }
        OBJECT_TYPE_WEAPON => weapon_export(definition, state),
        OBJECT_TYPE_PROJECTILE => projectile_export(definition, state),
        t if t >= 0 && OBJECT_MASK_DEVICE & (1 << t as u32) != 0 => device_export(definition, state),
        _ => {}
    }
}
