//! Halo's device updates (ROADMAP phase 7, objects and physics): devices.c's
//! `device_update` and device_machines.c's `machine_update`, ported as they
//! are, in the same single-precision arithmetic (controls' and light
//! fixtures' own updates do nothing).
//!
//! A device's power and position move toward their device groups' values
//! (`accelerate_to_position`), the position waiting out the definition's
//! delay from closed and making its start and stop effects. A machine
//! then: a platform turns round at its velocity; a door opens for the
//! bipeds near it (every fourth tick; a one-sided door only from its back
//! for the player's side) and closes once it has stood open long enough; an
//! elevator carries the bipeds riding it by its elevator node's motion; and
//! a machine that moved is translated.
//!
//! What a device asks of the game goes through a [`DeviceHost`] ([`call`]'s
//! kinds and the bipeds in a sphere).

use crate::{CacheFile, Result, layout};

const NONE: i32 = -1;
const EPSILON: f32 = 0.0001;

// device flags (the datum's)
const DEVICE_POSITION_CHANGED: u32 = 1 << 2;
// device definition flags
const DEVICE_POSITION_LOOPS: u32 = 1 << 0;
// machine flags (the datum's)
const MACHINE_DOES_NOT_OPERATE_AUTOMATICALLY: u32 = 1 << 0;
const MACHINE_ONE_SIDED: u32 = 1 << 1;
// machine definition flags
const MACHINE_IS_ELEVATOR: u16 = 1 << 2;
// machine types
const MACHINE_TYPE_DOOR: i16 = 0;
const MACHINE_TYPE_PLATFORM: i16 = 2;
const GAME_TEAM_PLAYER: i32 = 1;

/// A device as its update reads and changes it (the game's fields, laid
/// out for the game's side, `halo_sim.c`); a machine's part is zero for
/// another device.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct DeviceState {
    pub object_index: u32,
    pub definition_index: u32,
    pub location_cluster: i16,
    pub power_group_index: i16,
    pub device_flags: u32,
    pub power: f32,
    pub power_velocity: f32,
    pub position_group_index: i16,
    pub delay_ticks: i16,
    pub position: f32,
    pub position_velocity: f32,
    pub machine_flags: u32,
    pub door_open_ticks: i32,
    pub elevator_position: [f32; 3],
    pub object_position: [f32; 3],
    pub forward: [f32; 3],
    pub bounding_center: [f32; 3],
    pub bounding_radius: f32,
}

const _: () = assert!(std::mem::size_of::<DeviceState>() == 96);

/// A biped near a machine, as a door or an elevator reads it.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct DeviceBiped {
    pub object_index: u32,
    pub owner_team_index: i16,
    /// Whether it may not open doors (its second object function active, or
    /// its definition's unit can't open doors automatically).
    pub cannot_open_doors: u16,
    pub bounding_center: [f32; 3],
    pub position: [f32; 3],
    pub elevator_object_index: i32,
}

const _: () = assert!(std::mem::size_of::<DeviceBiped>() == 36);

/// A device definition's motion and effects (`_device_definition`), and a
/// machine's (`_machine_definition`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DeviceDefinition {
    pub flags: u32,
    pub positive_start_effect: i32,
    pub negative_start_effect: i32,
    pub positive_stop_effect: i32,
    pub negative_stop_effect: i32,
    pub delay_effect: i32,
    pub automatic_activation_radius: f32,
    pub maximum_power_acceleration: f32,
    pub maximum_power_velocity: f32,
    pub maximum_depowered_position_acceleration: f32,
    pub maximum_depowered_position_velocity: f32,
    pub maximum_powered_position_acceleration: f32,
    pub maximum_powered_position_velocity: f32,
    pub delay_ticks: f32,
    pub machine: Option<MachineDefinition>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct MachineDefinition {
    pub kind: i16,
    pub flags: u16,
    pub elevator_node_index: i16,
    pub door_open_ticks: i32,
}

impl DeviceDefinition {
    /// A device's definition (a machine's part if `machine`).
    pub fn load(cache: &CacheFile, datum: u32, machine: bool) -> Result<Self> {
        use layout::_device_definition as d;
        let root = cache.tag_root(datum, crate::groups::OBJECT)?;
        let device = root.add(layout::device_definition::DEVICE);
        let effect = |rel: usize| -> Result<i32> { Ok(device.reference(rel)?.datum.map_or(NONE, |datum| datum as i32)) };
        let machine = if machine {
            use layout::_machine_definition as m;
            let part = root.add(layout::machine_definition::MACHINE);
            Some(MachineDefinition {
                kind: part.i16(m::TYPE)?,
                flags: part.u16(m::FLAGS)?,
                elevator_node_index: part.i16(m::ELEVATOR_NODE_INDEX)?,
                door_open_ticks: part.i32(m::RUNTIME_DOOR_OPEN_TICKS)?,
            })
        } else {
            None
        };
        Ok(DeviceDefinition {
            flags: device.u32(d::FLAGS)?,
            positive_start_effect: effect(d::POSITIVE_START_EFFECT)?,
            negative_start_effect: effect(d::NEGATIVE_START_EFFECT)?,
            positive_stop_effect: effect(d::POSITIVE_STOP_EFFECT)?,
            negative_stop_effect: effect(d::NEGATIVE_STOP_EFFECT)?,
            delay_effect: effect(d::DELAY_EFFECT)?,
            automatic_activation_radius: device.f32(d::AUTOMATIC_ACTIVATION_RADIUS)?,
            maximum_power_acceleration: device.f32(d::RUNTIME_MAXIMUM_POWER_ACCELERATION)?,
            maximum_power_velocity: device.f32(d::RUNTIME_MAXIMUM_POWER_VELOCITY)?,
            maximum_depowered_position_acceleration: device.f32(d::RUNTIME_MAXIMUM_DEPOWERED_POSITION_ACCELERATION)?,
            maximum_depowered_position_velocity: device.f32(d::RUNTIME_MAXIMUM_DEPOWERED_POSITION_VELOCITY)?,
            maximum_powered_position_acceleration: device.f32(d::RUNTIME_MAXIMUM_POWERED_POSITION_ACCELERATION)?,
            maximum_powered_position_velocity: device.f32(d::RUNTIME_MAXIMUM_POWERED_POSITION_VELOCITY)?,
            delay_ticks: device.f32(d::RUNTIME_DELAY_TICKS)?,
            machine,
        })
    }
}

/// What a device's update asks of the game (`DeviceCall::kind`).
pub mod call {
    /// `device_effect_new(device, a)` (an effect or a sound; none for NONE).
    pub const EFFECT: i32 = 1;
    /// A device group's actual value: group `a`, `f[0]` the value.
    pub const SET_GROUP_ACTUAL: i32 = 2;
    /// `game_team_is_enemy(a, b)`: 1 if so.
    pub const TEAM_IS_ENEMY: i32 = 3;
    /// `device_group_set_desired_value(a, f[0])`.
    pub const SET_GROUP_DESIRED: i32 = 4;
    /// `object_translate(object, f, NULL)`: a biped an elevator carries, or
    /// the machine itself.
    pub const TRANSLATE: i32 = 5;
}

/// One call of the game.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct DeviceCall {
    pub kind: i32,
    pub object_index: i32,
    pub a: i32,
    pub b: i32,
    pub f: [f32; 3],
}

/// What a device's update reads of the game besides the device.
#[derive(Debug, Clone, Copy, Default)]
pub struct DeviceInputs {
    /// The power and position groups' actual values (for a group the
    /// device has).
    pub power_group_value: f32,
    pub position_group_value: f32,
    pub game_time: i32,
    /// The machine's elevator node's position, if it is an elevator with
    /// one.
    pub elevator_node_position: Option<[f32; 3]>,
}

/// A running game, as a device's update asks it. The device's state is the
/// update's as it stands (the host writes it to the game first, and reads
/// back what the call changed).
pub trait DeviceHost {
    fn call(&self, state: &mut DeviceState, call: &DeviceCall) -> i32;
    /// `objects_in_sphere(_object_mask_biped, _object_mask_biped, ...)`: the
    /// bipeds whose bounding spheres the sphere touches (at most `maximum`).
    fn bipeds_in_sphere(&self, cluster: i16, center: [f32; 3], radius: f32, maximum: usize) -> Vec<DeviceBiped>;
}

/// `accelerate_to_position`: moves `position` toward `target` under the
/// bounds, TRUE when it arrives.
#[allow(clippy::too_many_arguments)]
pub fn accelerate_to_position(
    position: &mut f32,
    velocity: &mut f32,
    target_position: f32,
    maximum_velocity: f32,
    acceleration: f32,
    minimum_position: f32,
    maximum_position: f32,
    periodic: bool,
) -> bool {
    let current_position = *position;
    let mut current_velocity = *velocity;
    let acceleration_squared = acceleration * acceleration;
    let mut result = false;
    let mut delta = target_position - current_position;

    if periodic {
        let half_range = (maximum_position - minimum_position) * 0.5;
        if delta > half_range {
            delta -= half_range + half_range;
        } else if delta < -half_range {
            delta += half_range + half_range;
        }
    }

    let limit = if maximum_velocity < acceleration { maximum_velocity } else { acceleration };
    let output_position;
    if (delta - current_velocity).abs() <= limit {
        current_velocity = 0.0;
        output_position = if target_position < minimum_position {
            minimum_position
        } else if target_position > maximum_position {
            maximum_position
        } else {
            target_position
        };
        result = true;
    } else {
        let braking_distance = (maximum_velocity + maximum_velocity) * delta.abs();
        let mut speed = if braking_distance >= acceleration_squared { acceleration } else { braking_distance.sqrt() };
        if delta < 0.0 {
            speed = -speed;
        }
        let mut step_clamped = speed - current_velocity;
        if step_clamped.abs() > maximum_velocity {
            step_clamped = if step_clamped < 0.0 { -maximum_velocity } else { maximum_velocity };
        }
        current_velocity += step_clamped;
        let mut new_position = step_clamped * 0.5 + current_velocity + current_position;
        if periodic {
            if new_position < minimum_position {
                // (in the C's order)
                #[allow(clippy::assign_op_pattern)]
                {
                    new_position = maximum_position - minimum_position + new_position;
                }
            } else if new_position > maximum_position {
                new_position -= maximum_position - minimum_position;
            }
        }
        if new_position < minimum_position {
            new_position = minimum_position;
        } else if new_position > maximum_position {
            new_position = maximum_position;
        }
        output_position = new_position;
    }

    *velocity = current_velocity;
    *position = output_position;
    result
}

fn effect(host: &dyn DeviceHost, device: &mut DeviceState, effect_index: i32) {
    let call = DeviceCall { kind: call::EFFECT, object_index: device.object_index as i32, a: effect_index, ..Default::default() };
    host.call(device, &call);
}

/// `device_update`: whether the device's power or position moved.
pub fn device_update(inputs: &DeviceInputs, definition: &DeviceDefinition, host: &dyn DeviceHost, device: &mut DeviceState) -> bool {
    let mut changed = false;

    if device.power_group_index as i32 != NONE {
        let group_value = inputs.power_group_value;
        if group_value != device.power || device.power_velocity != 0.0 {
            let old_power = device.power;
            if !accelerate_to_position(
                &mut device.power,
                &mut device.power_velocity,
                group_value,
                definition.maximum_power_acceleration,
                definition.maximum_power_velocity,
                0.0,
                1.0,
                false,
            ) {
                changed = true;
            }
            if old_power != device.power {
                device.device_flags |= DEVICE_POSITION_CHANGED;
            }
        }
    }

    if device.position_group_index as i32 != NONE {
        let group_value = inputs.position_group_value;
        if group_value != device.position || device.position_velocity != 0.0 {
            let maximum_acceleration = (1.0 - device.power) * definition.maximum_depowered_position_acceleration
                + device.power * definition.maximum_powered_position_acceleration;
            let maximum_velocity = (1.0 - device.power) * definition.maximum_depowered_position_velocity
                + device.power * definition.maximum_powered_position_velocity;
            let moving_forward = device.position_velocity > 0.0;

            #[allow(clippy::neg_cmp_op_on_partial_ord)]
            if !(device.delay_ticks as f32 >= definition.delay_ticks) && device.position == 0.0 && !(group_value < device.position) {
                device.delay_ticks += 1;
                if device.delay_ticks == 1 {
                    effect(host, device, definition.delay_effect);
                }
            } else {
                let old_velocity = device.position_velocity;
                let old_position = device.position;

                if device.position_velocity.abs() > maximum_velocity {
                    device.position_velocity = if moving_forward { maximum_velocity } else { -maximum_velocity };
                }

                if accelerate_to_position(
                    &mut device.position,
                    &mut device.position_velocity,
                    group_value,
                    maximum_acceleration,
                    maximum_velocity,
                    0.0,
                    1.0,
                    definition.flags & DEVICE_POSITION_LOOPS != 0,
                ) {
                    let effect_index = if moving_forward { definition.positive_stop_effect } else { definition.negative_stop_effect };
                    effect(host, device, effect_index);
                } else {
                    if device.position_velocity != 0.0 && old_velocity * device.position_velocity <= 0.0 {
                        let effect_index = if device.position_velocity > old_velocity {
                            definition.positive_start_effect
                        } else {
                            definition.negative_start_effect
                        };
                        effect(host, device, effect_index);
                    }
                    changed = true;
                }

                if old_position != device.position {
                    device.device_flags |= DEVICE_POSITION_CHANGED;
                }
            }
        } else {
            device.delay_ticks = 0;
        }
    }

    changed
}

/// `machine_update`.
pub fn machine_update(inputs: &DeviceInputs, definition: &DeviceDefinition, host: &dyn DeviceHost, machine: &mut DeviceState) {
    let Some(machine_definition) = &definition.machine else { return };
    let machine_index = machine.object_index as i32;

    if machine_definition.kind == MACHINE_TYPE_PLATFORM {
        machine.position += (1.0 - machine.power) * definition.maximum_depowered_position_velocity
            + machine.power * definition.maximum_powered_position_velocity;
        if machine.position >= 1.0 {
            machine.position -= 1.0;
        }
        machine.position_velocity = 0.0;
        machine.device_flags |= DEVICE_POSITION_CHANGED;
        if machine.position_group_index as i32 != NONE {
            let call = DeviceCall {
                kind: call::SET_GROUP_ACTUAL,
                object_index: machine_index,
                a: machine.position_group_index as i32,
                f: [machine.position, 0.0, 0.0],
                ..Default::default()
            };
            host.call(machine, &call);
        }
    }

    if machine.machine_flags & MACHINE_DOES_NOT_OPERATE_AUTOMATICALLY == 0
        && machine_definition.kind == MACHINE_TYPE_DOOR
        && (inputs.game_time.wrapping_add(machine_index) & 3) == 0
    {
        let mut activate = false;
        let automatic_activation_radius = if definition.automatic_activation_radius < EPSILON {
            machine.bounding_radius
        } else {
            definition.automatic_activation_radius
        };
        let bipeds = host.bipeds_in_sphere(machine.location_cluster, machine.bounding_center, automatic_activation_radius, 16);
        for unit in &bipeds {
            let can_open = unit.cannot_open_doors == 0;
            // (a one-sided door, closed, opens only from behind for the
            // player's side)
            let blocked = machine.machine_flags & MACHINE_ONE_SIDED != 0 && machine.position == 0.0 && {
                let call = DeviceCall {
                    kind: call::TEAM_IS_ENEMY,
                    object_index: NONE,
                    a: GAME_TEAM_PLAYER,
                    b: unit.owner_team_index as i32,
                    ..Default::default()
                };
                host.call(machine, &call) == 0
            } && {
                let to_unit = [
                    unit.bounding_center[0] - machine.bounding_center[0],
                    unit.bounding_center[1] - machine.bounding_center[1],
                    unit.bounding_center[2] - machine.bounding_center[2],
                ];
                to_unit[0] * machine.forward[0] + to_unit[1] * machine.forward[1] + to_unit[2] * machine.forward[2] > 0.0
            };
            if !blocked && can_open {
                activate = true;
            }
        }

        if activate {
            if machine.position_group_index as i32 != NONE {
                let call = DeviceCall {
                    kind: call::SET_GROUP_DESIRED,
                    object_index: machine_index,
                    a: machine.position_group_index as i32,
                    f: [1.0, 0.0, 0.0],
                    ..Default::default()
                };
                host.call(machine, &call);
            }
            machine.door_open_ticks = -3;
        }
    }

    if machine_definition.kind == MACHINE_TYPE_DOOR {
        if machine.position == 1.0 {
            machine.door_open_ticks += 1;
            if machine.door_open_ticks > machine_definition.door_open_ticks && machine.position_group_index as i32 != NONE {
                let call = DeviceCall {
                    kind: call::SET_GROUP_DESIRED,
                    object_index: machine_index,
                    a: machine.position_group_index as i32,
                    f: [0.0, 0.0, 0.0],
                    ..Default::default()
                };
                host.call(machine, &call);
            }
        } else {
            machine.door_open_ticks = 0;
        }
    }

    if machine_definition.flags & MACHINE_IS_ELEVATOR != 0
        && machine_definition.elevator_node_index as i32 != NONE
        && let Some(node_position) = inputs.elevator_node_position
    {
        {
            let elevator_offset = [
                node_position[0] - machine.elevator_position[0],
                node_position[1] - machine.elevator_position[1],
                node_position[2] - machine.elevator_position[2],
            ];
            if elevator_offset[0] != 0.0 || elevator_offset[1] != 0.0 || elevator_offset[2] != 0.0 {
                let bipeds = host.bipeds_in_sphere(machine.location_cluster, machine.bounding_center, machine.bounding_radius, 2048);
                for biped in &bipeds {
                    if biped.elevator_object_index == machine_index {
                        let new_position = [
                            elevator_offset[0] + biped.position[0],
                            elevator_offset[1] + biped.position[1],
                            elevator_offset[2] + biped.position[2],
                        ];
                        let call = DeviceCall {
                            kind: call::TRANSLATE,
                            object_index: biped.object_index as i32,
                            f: new_position,
                            ..Default::default()
                        };
                        host.call(machine, &call);
                    }
                }
            }
            machine.elevator_position = node_position;
        }
    }

    if machine.device_flags & DEVICE_POSITION_CHANGED != 0 {
        let call = DeviceCall { kind: call::TRANSLATE, object_index: machine_index, f: machine.object_position, ..Default::default() };
        host.call(machine, &call);
        machine.device_flags &= !DEVICE_POSITION_CHANGED;
    }
}
