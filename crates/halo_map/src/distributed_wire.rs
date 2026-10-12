//! The distributed netcode's wire format (OpenCE's
//! port/linux/game/network_distributed.c, port/linux/NETCODE.md): vectors
//! in 16 bits a part, vitalities and angles in 16 bits, a player's unit's
//! state with only the parts that are not their defaults, and a player's
//! input as the host ran it with each earlier tick's buttons only where
//! they changed. Little-endian, as the machines lay their structs out.

/// network_distributed.h's `DISTRIBUTED_UNIT_SCALE`.
pub const UNIT_SCALE: f32 = 32767.0;
/// network_distributed.c's `VITALITY_SCALE`.
pub const VITALITY_SCALE: f32 = 16384.0;
/// player_queues_new.h's `DISTRIBUTED_INPUT_HISTORY`.
pub const INPUT_HISTORY: usize = 4;
/// players.h's `NUMBER_OF_PLAYER_POWERUPS`.
pub const PLAYER_POWERUPS: usize = 2;
/// `NO_PLAYER` (a byte player index).
pub const NO_PLAYER: u8 = 0xff;
const NONE: i32 = -1;
/// real_math.h's `_pi`, as a float.
const PI: f32 = std::f32::consts::PI;
/// `2.0f * _pi`.
const TWO_PI: f32 = 2.0 * PI;

/// `distributed_vector_pack`: each part scaled, held to 16 bits and
/// rounded.
pub fn vector_pack(vector: [f32; 3], scale: f32) -> [i16; 3] {
    vector.map(|part| {
        let value = part * scale;
        let value = if value > 32767.0 {
            32767.0
        } else if value < -32767.0 {
            -32767.0
        } else {
            value
        };
        ((value + 0.5) as f64).floor() as f32 as i16
    })
}

/// `distributed_vector_unpack`.
pub fn vector_unpack(vector: [i16; 3], scale: f32) -> [f32; 3] {
    vector.map(|part| part as f32 / scale)
}

/// `distributed_unit_vector_unpack`: made unit length (but none).
pub fn unit_vector_unpack(vector: [i16; 3]) -> [f32; 3] {
    let mut result = vector_unpack(vector, UNIT_SCALE);
    let length = ((result[0] * result[0] + result[1] * result[1] + result[2] * result[2]) as f64).sqrt() as f32;
    if length > 0.0 {
        result = result.map(|part| part / length);
    }
    result
}

/// `distributed_vitality_pack`.
pub fn vitality_pack(value: f32) -> u16 {
    let value = value * VITALITY_SCALE;
    let value = if value > 65535.0 {
        65535.0
    } else if value < 0.0 {
        0.0
    } else {
        value
    };
    ((value + 0.5) as f64).floor() as i32 as u16
}

/// `distributed_vitality_unpack`.
pub fn vitality_unpack(value: u16) -> f32 {
    value as f32 / VITALITY_SCALE
}

/// `distributed_angle_pack`: the angle's turn in 16 bits.
pub fn angle_pack(angle: f32) -> i16 {
    let mut turns = angle / TWO_PI;
    turns -= (turns as f64).floor() as f32;
    (((turns * 65536.0 + 0.5) as f64).floor() as i32 & 0xffff) as u16 as i16
}

/// `distributed_angle_unpack`: in [0, 2π), or [-π, π) signed.
pub fn angle_unpack(value: i16, signed_angle: bool) -> f32 {
    let mut angle = value as u16 as f32 * TWO_PI / 65536.0;
    if signed_angle && angle >= PI {
        angle -= TWO_PI;
    }
    angle
}

/// network_distributed.c's `struct distributed_unit_state`.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct UnitState {
    pub player_index: u8,
    pub flags: u8,
    pub killing_player_index: u8,
    pub unit_flags: u8,
    pub unit_index: i32,
    pub vehicle_index: i32,
    pub seat_index: i16,
    pub active_camouflage: u8,
    pub pad: u8,
    pub position: [u32; 3],
    pub velocity: [i16; 3],
    pub forward: [i16; 3],
    pub up: [i16; 3],
    pub predicted_time: i16,
    pub body_vitality: u16,
    pub shield_vitality: u16,
    pub current_body_damage: u16,
    pub recent_body_damage: u16,
    pub current_shield_damage: u16,
    pub recent_shield_damage: u16,
    pub powerup_durations: [i16; PLAYER_POWERUPS],
}

const _: () = assert!(std::mem::size_of::<UnitState>() == 64);

/// The unit state's flags.
pub mod unit_flag {
    pub const ALIVE: u8 = 1 << 0;
    pub const PLACED: u8 = 1 << 1;
    pub const PREDICTED: u8 = 1 << 7;
}

/// The unit state's parts on the wire.
mod part {
    pub const RIDING: u8 = 1 << 0;
    pub const UP: u8 = 1 << 1;
    pub const DAMAGE: u8 = 1 << 2;
    pub const POWERUPS: u8 = 1 << 3;
    pub const CAMOUFLAGE: u8 = 1 << 4;
    pub const KILLER: u8 = 1 << 5;
}

/// network_distributed.c's `DISTRIBUTED_UNIT_STATE_MINIMUM_SIZE`.
const UNIT_STATE_MINIMUM_SIZE: usize = 3;

/// A cursor that writes the values' bytes in turn.
struct Put<'a> {
    buffer: &'a mut [u8],
    at: usize,
}

impl Put<'_> {
    fn bytes(&mut self, bytes: &[u8]) {
        self.buffer[self.at..self.at + bytes.len()].copy_from_slice(bytes);
        self.at += bytes.len();
    }
    fn u8(&mut self, value: u8) {
        self.bytes(&[value]);
    }
    fn i16(&mut self, value: i16) {
        self.bytes(&value.to_le_bytes());
    }
    fn u16(&mut self, value: u16) {
        self.bytes(&value.to_le_bytes());
    }
    fn i32(&mut self, value: i32) {
        self.bytes(&value.to_le_bytes());
    }
    fn vector(&mut self, value: [i16; 3]) {
        value.iter().for_each(|&part| self.i16(part));
    }
}

/// A cursor that takes the values' bytes in turn, none past the end.
struct Take<'a> {
    buffer: &'a [u8],
    at: usize,
}

impl Take<'_> {
    fn bytes<const N: usize>(&mut self) -> Option<[u8; N]> {
        let bytes = self.buffer.get(self.at..self.at + N)?.try_into().ok()?;
        self.at += N;
        Some(bytes)
    }
    fn u8(&mut self) -> Option<u8> {
        self.bytes::<1>().map(|b| b[0])
    }
    fn i8(&mut self) -> Option<i8> {
        self.u8().map(|b| b as i8)
    }
    fn i16(&mut self) -> Option<i16> {
        self.bytes().map(i16::from_le_bytes)
    }
    fn u16(&mut self) -> Option<u16> {
        self.bytes().map(u16::from_le_bytes)
    }
    fn i32(&mut self) -> Option<i32> {
        self.bytes().map(i32::from_le_bytes)
    }
    fn u32(&mut self) -> Option<u32> {
        self.bytes().map(u32::from_le_bytes)
    }
}

/// `distributed_unit_state_write`: the state's bytes on the wire, and how
/// many (the predicted tick only to its own client).
pub fn unit_state_write(state: &UnitState, own: bool, buffer: &mut [u8]) -> u16 {
    let mut flags = state.flags;
    let mut parts = 0u8;
    let mut put = Put { buffer, at: UNIT_STATE_MINIMUM_SIZE };
    if !own {
        flags &= !unit_flag::PREDICTED;
    }
    if flags & unit_flag::ALIVE != 0 {
        put.i32(state.unit_index);
        if state.vehicle_index != NONE {
            parts |= part::RIDING;
            put.i32(state.vehicle_index);
            put.i16(state.seat_index);
        }
        if flags & unit_flag::PLACED != 0 {
            state.position.iter().for_each(|&part| put.bytes(&part.to_le_bytes()));
            put.vector(state.velocity);
            put.vector(state.forward);
            if state.up[0] != 0 || state.up[1] != 0 || state.up[2] != UNIT_SCALE as i16 {
                parts |= part::UP;
                put.vector(state.up);
            }
        }
        if flags & unit_flag::PREDICTED != 0 {
            put.i16(state.predicted_time);
        }
        put.u16(state.body_vitality);
        put.u16(state.shield_vitality);
        if state.current_body_damage != 0
            || state.recent_body_damage != 0
            || state.current_shield_damage != 0
            || state.recent_shield_damage != 0
        {
            parts |= part::DAMAGE;
            put.u16(state.current_body_damage);
            put.u16(state.recent_body_damage);
            put.u16(state.current_shield_damage);
            put.u16(state.recent_shield_damage);
        }
        if state.powerup_durations.iter().any(|&duration| duration != 0) {
            parts |= part::POWERUPS;
            state.powerup_durations.iter().for_each(|&duration| put.i16(duration));
        }
        if state.unit_flags != 0 || state.active_camouflage != 0 {
            parts |= part::CAMOUFLAGE;
            put.u8(state.unit_flags);
            put.u8(state.active_camouflage);
        }
    } else if state.killing_player_index != NO_PLAYER {
        parts |= part::KILLER;
        put.u8(state.killing_player_index);
    }
    let size = put.at as u16;
    buffer[0] = state.player_index;
    buffer[1] = flags;
    buffer[2] = parts;
    size
}

/// `distributed_unit_state_read`: a state from its bytes (as far as they
/// make one: its defaults otherwise), and the bytes it took, 0 for none
/// that make one.
pub fn unit_state_read(buffer: &[u8], state: &mut UnitState) -> u16 {
    *state = UnitState {
        unit_index: NONE,
        vehicle_index: NONE,
        seat_index: NONE as i16,
        killing_player_index: NO_PLAYER,
        up: [0, 0, UNIT_SCALE as i16],
        ..UnitState::default()
    };
    let mut take = Take { buffer, at: 0 };
    let read = |take: &mut Take<'_>, state: &mut UnitState| -> Option<()> {
        state.player_index = take.u8()?;
        state.flags = take.u8()?;
        let parts = take.u8()?;
        if state.flags & unit_flag::ALIVE != 0 {
            state.unit_index = take.i32()?;
            if parts & part::RIDING != 0 {
                state.vehicle_index = take.i32()?;
                state.seat_index = take.i16()?;
            }
            if state.flags & unit_flag::PLACED != 0 {
                for index in 0..3 {
                    state.position[index] = take.u32()?;
                }
                for index in 0..3 {
                    state.velocity[index] = take.i16()?;
                }
                for index in 0..3 {
                    state.forward[index] = take.i16()?;
                }
                if parts & part::UP != 0 {
                    for index in 0..3 {
                        state.up[index] = take.i16()?;
                    }
                }
            }
            if state.flags & unit_flag::PREDICTED != 0 {
                state.predicted_time = take.i16()?;
            }
            state.body_vitality = take.u16()?;
            state.shield_vitality = take.u16()?;
            if parts & part::DAMAGE != 0 {
                state.current_body_damage = take.u16()?;
                state.recent_body_damage = take.u16()?;
                state.current_shield_damage = take.u16()?;
                state.recent_shield_damage = take.u16()?;
            }
            if parts & part::POWERUPS != 0 {
                for index in 0..PLAYER_POWERUPS {
                    state.powerup_durations[index] = take.i16()?;
                }
            }
            if parts & part::CAMOUFLAGE != 0 {
                state.unit_flags = take.u8()?;
                state.active_camouflage = take.u8()?;
            }
        } else if parts & part::KILLER != 0 {
            state.killing_player_index = take.u8()?;
        }
        Some(())
    };
    match read(&mut take, state) {
        Some(()) => take.at as u16,
        None => 0,
    }
}

/// network_distributed.c's `struct distributed_relayed_action`.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RelayedAction {
    pub player_index: u8,
    pub desired_weapon_index: i8,
    pub desired_grenade_index: i8,
    pub desired_zoom_level: i8,
    pub control_flags: [u16; INPUT_HISTORY],
    pub yaw: i16,
    pub pitch: i16,
    pub throttle_i: i8,
    pub throttle_j: i8,
    pub primary_trigger: u8,
    pub pad: u8,
}

const _: () = assert!(std::mem::size_of::<RelayedAction>() == 20);

/// players.h's `struct player_action`.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct PlayerAction {
    pub control_flags: u32,
    pub yaw: f32,
    pub pitch: f32,
    pub throttle: [f32; 2],
    pub primary_trigger: f32,
    pub desired_weapon_index: i16,
    pub desired_grenade_index: i16,
    pub desired_zoom_level: i16,
    pub pad: i16,
}

const _: () = assert!(std::mem::size_of::<PlayerAction>() == 0x20);

/// `distributed_relayed_action_from`: the player's input as the host's
/// tick ran it (the ticks' actions, latest first, none where a tick has
/// none of the player), and whether its buttons or choices changed in the
/// ticks it carries.
pub fn relayed_action_from(player_index: i16, recent: &[Option<&PlayerAction>; INPUT_HISTORY], relayed: &mut RelayedAction) -> bool {
    let clamp = |value: f32, low: f32| {
        if value > 1.0 {
            1.0
        } else if value < low {
            low
        } else {
            value
        }
    };
    let action = recent[0].copied().unwrap_or_default();
    let throttle_i = clamp(action.throttle[0], -1.0);
    let throttle_j = clamp(action.throttle[1], -1.0);
    let trigger = clamp(action.primary_trigger, 0.0);
    let mut changed = false;
    *relayed = RelayedAction {
        player_index: player_index as u8,
        desired_weapon_index: action.desired_weapon_index as i8,
        desired_grenade_index: action.desired_grenade_index as i8,
        desired_zoom_level: (if action.desired_zoom_level > 127 { 127 } else { action.desired_zoom_level }) as i8,
        ..RelayedAction::default()
    };
    for history in 0..INPUT_HISTORY {
        let past = recent[history];
        relayed.control_flags[history] = past.map_or(0, |past| past.control_flags as u16);
        if history > 0
            && past.is_none_or(|past| {
                relayed.control_flags[history] != relayed.control_flags[history - 1]
                    || past.desired_weapon_index != action.desired_weapon_index
                    || past.desired_grenade_index != action.desired_grenade_index
                    || past.desired_zoom_level != action.desired_zoom_level
            })
        {
            changed = true;
        }
    }
    relayed.yaw = angle_pack(action.yaw);
    relayed.pitch = angle_pack(action.pitch);
    relayed.throttle_i = ((throttle_i * 127.0 + 0.5) as f64).floor() as i32 as i8;
    relayed.throttle_j = ((throttle_j * 127.0 + 0.5) as f64).floor() as i32 as i8;
    relayed.primary_trigger = ((trigger * 255.0 + 0.5) as f64).floor() as i32 as u8;
    changed
}

/// `distributed_relayed_action_write`: the action's bytes on the wire, and
/// how many.
pub fn relayed_action_write(relayed: &RelayedAction, buffer: &mut [u8]) -> u16 {
    let mut parts = 0u8;
    let mut put = Put { buffer, at: 2 };
    put.u8(relayed.desired_weapon_index as u8);
    put.u8(relayed.desired_grenade_index as u8);
    put.u8(relayed.desired_zoom_level as u8);
    put.i16(relayed.yaw);
    put.i16(relayed.pitch);
    put.u8(relayed.throttle_i as u8);
    put.u8(relayed.throttle_j as u8);
    put.u8(relayed.primary_trigger);
    put.u16(relayed.control_flags[0]);
    for history in 1..INPUT_HISTORY {
        if relayed.control_flags[history] != relayed.control_flags[history - 1] {
            parts |= 1 << (history - 1);
            put.u16(relayed.control_flags[history]);
        }
    }
    let size = put.at as u16;
    buffer[0] = relayed.player_index;
    buffer[1] = parts;
    size
}

/// `distributed_relayed_action_read`: an action from its bytes (as far as
/// they make one), and the bytes it took, 0 for none that make one.
pub fn relayed_action_read(buffer: &[u8], relayed: &mut RelayedAction) -> u16 {
    *relayed = RelayedAction::default();
    let mut take = Take { buffer, at: 0 };
    let read = |take: &mut Take<'_>, relayed: &mut RelayedAction| -> Option<()> {
        relayed.player_index = take.u8()?;
        let parts = take.u8()?;
        relayed.desired_weapon_index = take.i8()?;
        relayed.desired_grenade_index = take.i8()?;
        relayed.desired_zoom_level = take.i8()?;
        relayed.yaw = take.i16()?;
        relayed.pitch = take.i16()?;
        relayed.throttle_i = take.i8()?;
        relayed.throttle_j = take.i8()?;
        relayed.primary_trigger = take.u8()?;
        relayed.control_flags[0] = take.u16()?;
        for history in 1..INPUT_HISTORY {
            relayed.control_flags[history] =
                if parts & (1 << (history - 1)) == 0 { relayed.control_flags[history - 1] } else { take.u16()? };
        }
        Some(())
    };
    match read(&mut take, relayed) {
        Some(()) => take.at as u16,
        None => 0,
    }
}

// ---------- the receiver's checks

/// `distributed_real_valid`: finite.
fn real_valid(value: f32) -> bool {
    value - value == 0.0
}

/// `distributed_point_valid`: each part within the bound.
fn point_valid(point: [f32; 3], bound: f32) -> bool {
    point.iter().all(|part| part.abs() <= bound)
}

/// `distributed_axes_make_valid` of the axes (forward then up) in place:
/// whether they make a rotation (forward made unit length first, then up
/// square to it and unit length).
pub fn axes_make_valid(axes: &mut [f32; 6]) -> bool {
    let length = |v: &[f32]| ((v[0] * v[0] + v[1] * v[1] + v[2] * v[2]) as f64).sqrt() as f32;
    let forward_length = length(&axes[0..3]);
    let up_length = length(&axes[3..6]);
    if !(forward_length > 0.5 && forward_length < 1.5 && up_length > 0.5 && up_length < 1.5) {
        return false;
    }
    for index in 0..3 {
        axes[index] /= forward_length;
    }
    let dot = (axes[0] * axes[3] + axes[1] * axes[4] + axes[2] * axes[5]) / up_length;
    if !(dot.abs() < 0.1) {
        return false;
    }
    for index in 0..3 {
        axes[3 + index] -= dot * up_length * axes[index];
    }
    let length = length(&axes[3..6]);
    for index in 3..6 {
        axes[index] /= length;
    }
    true
}

/// network_objects.c's `OBJECT_WORLD_BOUND`.
const OBJECT_WORLD_BOUND: f32 = 32768.0;

/// network_objects.c's `distributed_transform_valid`: whether a message's
/// transform can be (the position within the world, the velocities, if
/// any, finite), and its axes made a rotation into `valid` (forward then
/// up; as far as made where they cannot be).
pub fn transform_valid(
    position: [f32; 3],
    forward: [f32; 3],
    up: [f32; 3],
    velocity: Option<[f32; 3]>,
    angular_velocity: Option<[f32; 3]>,
    valid: &mut [f32; 6],
) -> bool {
    let vector_valid = |v: [f32; 3]| v.iter().all(|&part| real_valid(part));
    if !point_valid(position, OBJECT_WORLD_BOUND)
        || velocity.is_some_and(|v| !vector_valid(v))
        || angular_velocity.is_some_and(|v| !vector_valid(v))
    {
        return false;
    }
    valid[0..3].copy_from_slice(&forward);
    valid[3..6].copy_from_slice(&up);
    axes_make_valid(valid)
}

/// network_objects.c's `distributed_vector_clamp`: no longer than maximum.
pub fn vector_clamp(vector: &mut [f32; 3], maximum: f32) {
    let length = ((vector[0] * vector[0] + vector[1] * vector[1] + vector[2] * vector[2]) as f64).sqrt() as f32;
    if length > maximum {
        for part in vector.iter_mut() {
            *part *= maximum / length;
        }
    }
}

/// network_objects.c's `distributed_checksum` (FNV-1a).
pub fn checksum(bytes: &[u8]) -> u32 {
    bytes.iter().fold(2_166_136_261u32, |checksum, &byte| (checksum ^ byte as u32).wrapping_mul(16_777_619))
}

// ---------- objects

/// network_distributed.h's velocity scales.
pub const VELOCITY_SCALE: f32 = 1024.0;
pub const ANGULAR_VELOCITY_SCALE: f32 = 4096.0;

/// network_objects.c's `struct distributed_object_state`.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ObjectState {
    pub object_index: i32,
    pub flags: u8,
    pub pad: u8,
    pub time: i16,
    pub position: [u32; 3],
    pub forward: [i16; 3],
    pub up: [i16; 3],
    pub translational_velocity: [i16; 3],
    pub angular_velocity: [i16; 3],
}

const _: () = assert!(std::mem::size_of::<ObjectState>() == 44);

/// The object state's flags.
pub mod object_flag {
    pub const AT_REST: u8 = 1 << 1;
    pub const DEAD: u8 = 1 << 2;
}

/// network_objects.c's `distributed_object_state_unpack`: forward, up,
/// velocity and angular velocity, as they came.
pub fn object_state_unpack(state: &ObjectState) -> [[f32; 3]; 4] {
    [
        vector_unpack(state.forward, UNIT_SCALE),
        vector_unpack(state.up, UNIT_SCALE),
        vector_unpack(state.translational_velocity, VELOCITY_SCALE),
        vector_unpack(state.angular_velocity, ANGULAR_VELOCITY_SCALE),
    ]
}

/// network_objects.c's `struct distributed_inventory`.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Inventory {
    pub unit_index: i32,
    pub grenade_counts: [i8; 2],
    pub current_weapon_index: i8,
    pub pad: u8,
    pub weapon_indices: [i32; 4],
    pub rounds_total: [[i16; 2]; 4],
    pub rounds_loaded: [[i16; 2]; 4],
    pub age: [u16; 4],
}

const _: () = assert!(std::mem::size_of::<Inventory>() == 64);

/// network_objects.c's `AGE_SCALE`.
const AGE_SCALE: f32 = 65535.0;

// ---------- sampling the game

use crate::ai_actor::AiMemory;
use crate::layout::{net_values as nv, player_datum as pd, unit_datum as ud, weapon_datum as wd, weapon_magazine as wm};

/// Where the samplers find the game's objects and players.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct Game {
    pub object_header_data: u32,
    pub player_data: u32,
}

fn vector(m: &dyn AiMemory, address: u32) -> [f32; 3] {
    [0, 4, 8].map(|offset| f32::from_bits(m.r32(address + offset)))
}

fn i16_at(m: &dyn AiMemory, address: u32) -> i16 {
    m.r16(address) as i16
}

fn i32_at(m: &dyn AiMemory, address: u32) -> i32 {
    m.r32(address) as i32
}

fn clamp_unit(value: f32) -> f32 {
    if value > 1.0 {
        1.0
    } else if value < 0.0 {
        0.0
    } else {
        value
    }
}

impl Game {
    /// `object_try_and_get`.
    fn object(&self, m: &dyn AiMemory, object_index: i32) -> Option<u32> {
        crate::ai_actor::datum_get(m, self.object_header_data, object_index)
            .map(|header| m.r32(header + crate::layout::object_header_datum::DATUM as u32))
    }

    /// `object_try_and_get_and_verify_type`.
    fn object_of_types(&self, m: &dyn AiMemory, object_index: i32, mask: u32) -> Option<u32> {
        self.object(m, object_index).filter(|&object| mask & (1 << m.r8(object + ud::OBJECT_TYPE as u32)) != 0)
    }

    /// network_distributed.c's `distributed_player`: the player at the
    /// absolute index, if any.
    fn player(&self, m: &dyn AiMemory, player_index: i16) -> Option<u32> {
        use crate::data_array::{DATA_DATA, DATA_MAXIMUM_COUNT, DATA_SIZE};
        let maximum_count = m.r16(self.player_data + DATA_MAXIMUM_COUNT) as i16;
        if player_index < 0 || player_index >= maximum_count {
            return None;
        }
        let size = m.r16(self.player_data + DATA_SIZE) as i16 as u32;
        let player = m.r32(self.player_data + DATA_DATA) + player_index as u32 * size;
        (i16_at(m, player + pd::IDENTIFIER as u32) != 0).then_some(player)
    }

    /// network_distributed.c's `distributed_living_unit`.
    fn living_unit(&self, m: &dyn AiMemory, player: Option<u32>) -> i32 {
        let Some(player) = player else { return NONE };
        let unit_index = i32_at(m, player + pd::UNIT_INDEX as u32);
        if unit_index == NONE {
            return NONE;
        }
        match self.object(m, unit_index) {
            Some(unit) if m.r16(unit + ud::OBJECT_DAMAGE_FLAGS as u32) as u32 & (1 << nv::_OBJECT_DEAD_BIT) == 0 => unit_index,
            _ => NONE,
        }
    }
}

/// What a player's state needs of the netcode's own tables: the tick,
/// whether the host, whether the player is tracked, its prediction's ticks
/// and its death.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct PlayerExtras {
    pub game_time: i32,
    pub server: i32,
    pub tracked: i32,
    pub prediction_taken_time: i32,
    pub prediction_taken_host_time: i32,
    pub death_valid: i32,
    pub death_killing_player_index: i32,
    pub death_friendly_fire: i32,
    pub death_killed_by_vehicle: i32,
}

/// network_distributed.c's `PREDICTION_ECHO_TICKS`.
const PREDICTION_ECHO_TICKS: i32 = 3;

/// network_distributed.c's `distributed_state_from_player`: the player's
/// unit as the host has it (alive: where, riding what, its shields and
/// health, powerups and camouflage, and where its client predicted it;
/// dead: who killed it).
pub fn state_from_player(m: &dyn AiMemory, game: &Game, player_index: i16, extras: &PlayerExtras) -> UnitState {
    let player = game.player(m, player_index);
    let unit_index = game.living_unit(m, player);
    let mut state = UnitState {
        player_index: player_index as u8,
        unit_index: NONE,
        vehicle_index: NONE,
        seat_index: NONE as i16,
        ..UnitState::default()
    };
    if unit_index != NONE {
        let unit = game.object(m, unit_index).unwrap_or(0);
        let at = |offset: usize| unit + offset as u32;
        let parent = i32_at(m, at(ud::OBJECT_PARENT_OBJECT_INDEX));
        let seat = i16_at(m, at(ud::UNIT_PARENT_SEAT_INDEX));
        state.unit_index = unit_index;
        state.flags |= unit_flag::ALIVE;
        if parent == NONE {
            state.flags |= unit_flag::PLACED;
        }
        if parent != NONE && seat != NONE as i16 {
            state.vehicle_index = parent;
            state.seat_index = seat;
        }
        state.position = [0, 4, 8].map(|offset| m.r32(at(ud::OBJECT_POSITION) + offset));
        state.velocity = vector_pack(vector(m, at(ud::OBJECT_TRANSLATIONAL_VELOCITY)), VELOCITY_SCALE);
        state.forward = vector_pack(vector(m, at(ud::OBJECT_FORWARD)), UNIT_SCALE);
        state.up = vector_pack(vector(m, at(ud::OBJECT_UP)), UNIT_SCALE);
        // (damage_get_network_state)
        let damage_flags = m.r16(at(ud::OBJECT_DAMAGE_FLAGS)) as u32;
        let flag = |bit: usize| damage_flags & (1 << bit) != 0;
        if flag(nv::_OBJECT_SHIELD_DEPLETED_BIT) {
            state.flags |= 1 << 4;
        }
        if flag(nv::_OBJECT_SHIELD_CHARGING_BIT) {
            state.flags |= 1 << 5;
        }
        if flag(nv::_OBJECT_SHIELD_OVER_CHARGING_BIT) {
            state.flags |= 1 << 6;
        }
        let real = |offset: usize| f32::from_bits(m.r32(at(offset)));
        state.body_vitality = vitality_pack(real(ud::OBJECT_BODY_VITALITY));
        state.shield_vitality = vitality_pack(real(ud::OBJECT_SHIELD_VITALITY));
        state.current_body_damage = vitality_pack(real(ud::OBJECT_CURRENT_BODY_DAMAGE));
        state.recent_body_damage = vitality_pack(real(ud::OBJECT_RECENT_BODY_DAMAGE));
        state.current_shield_damage = vitality_pack(real(ud::OBJECT_CURRENT_SHIELD_DAMAGE));
        state.recent_shield_damage = vitality_pack(real(ud::OBJECT_RECENT_SHIELD_DAMAGE));
        let player = player.unwrap_or(0);
        for index in 0..PLAYER_POWERUPS {
            state.powerup_durations[index] = i16_at(m, player + pd::POWERUP_DURATIONS as u32 + 2 * index as u32);
        }
        let unit_flags = m.r32(at(ud::UNIT_FLAGS));
        if unit_flags & (1 << nv::_UNIT_ACTIVE_CAMOUFLAGED_BIT) != 0 {
            state.unit_flags |= 1 << 0;
        }
        if unit_flags & (1 << nv::_UNIT_SUPER_CAMOUFLAGED_BIT) != 0 {
            state.unit_flags |= 1 << 1;
        }
        let camouflage = clamp_unit(real(ud::UNIT_ACTIVE_CAMOUFLAGE));
        state.active_camouflage = ((camouflage * 255.0 + 0.5) as f64).floor() as i32 as u8;
        if extras.server != 0
            && extras.tracked != 0
            && extras.prediction_taken_host_time != NONE
            && extras.game_time.wrapping_sub(extras.prediction_taken_host_time) < PREDICTION_ECHO_TICKS
        {
            state.flags |= unit_flag::PREDICTED;
            state.predicted_time = extras
                .prediction_taken_time
                .wrapping_add(extras.game_time)
                .wrapping_sub(extras.prediction_taken_host_time) as u16 as i16;
        }
    }
    state.killing_player_index = NO_PLAYER;
    if unit_index == NONE && extras.tracked != 0 && extras.death_valid != 0 {
        if extras.death_killing_player_index as i16 != NONE as i16 {
            state.killing_player_index = extras.death_killing_player_index as u8;
        }
        if extras.death_friendly_fire != 0 {
            state.flags |= 1 << 2;
        }
        if extras.death_killed_by_vehicle != 0 {
            state.flags |= 1 << 3;
        }
    }
    state
}

/// network_objects.c's `distributed_state_from_object`.
pub fn state_from_object(m: &dyn AiMemory, game: &Game, object_index: i32) -> ObjectState {
    let object = game.object(m, object_index).unwrap_or(0);
    let at = |offset: usize| object + offset as u32;
    let mut state = ObjectState { object_index, ..ObjectState::default() };
    if m.r32(at(ud::OBJECT_FLAGS)) & (1 << nv::_OBJECT_AT_REST_BIT) != 0 {
        state.flags |= object_flag::AT_REST;
    }
    if (nv::_OBJECT_MASK_UNIT as u32) & (1 << m.r8(at(ud::OBJECT_TYPE))) != 0
        && m.r16(at(ud::OBJECT_DAMAGE_FLAGS)) as u32 & (1 << nv::_OBJECT_DEAD_BIT) != 0
    {
        state.flags |= object_flag::DEAD;
    }
    state.position = [0, 4, 8].map(|offset| m.r32(at(ud::OBJECT_POSITION) + offset));
    state.forward = vector_pack(vector(m, at(ud::OBJECT_FORWARD)), UNIT_SCALE);
    state.up = vector_pack(vector(m, at(ud::OBJECT_UP)), UNIT_SCALE);
    state.translational_velocity = vector_pack(vector(m, at(ud::OBJECT_TRANSLATIONAL_VELOCITY)), VELOCITY_SCALE);
    state.angular_velocity = vector_pack(vector(m, at(ud::OBJECT_ANGULAR_VELOCITY)), ANGULAR_VELOCITY_SCALE);
    state
}

/// network_objects.c's `distributed_inventory_from_unit`: its grenades,
/// its weapon, and each weapon's magazines and age.
pub fn inventory_from_unit(m: &dyn AiMemory, game: &Game, unit_index: i32) -> Inventory {
    let unit = game.object(m, unit_index).unwrap_or(0);
    let at = |offset: usize| unit + offset as u32;
    let mut inventory = Inventory { unit_index, ..Inventory::default() };
    for index in 0..2 {
        inventory.grenade_counts[index] = m.r8(at(ud::UNIT_GRENADE_COUNTS) + index as u32) as i8;
    }
    inventory.current_weapon_index = i16_at(m, at(ud::UNIT_CURRENT_WEAPON_INDEX)) as i8;
    for slot in 0..4 {
        let weapon_index = i32_at(m, at(ud::UNIT_WEAPON_OBJECT_INDICES) + 4 * slot as u32);
        let weapon = if weapon_index != NONE {
            game.object_of_types(m, weapon_index, 1 << nv::_OBJECT_TYPE_WEAPON)
        } else {
            None
        };
        inventory.weapon_indices[slot] = NONE;
        if let Some(weapon) = weapon {
            inventory.weapon_indices[slot] = weapon_index;
            for magazine in 0..2 {
                let address = weapon + wd::WEAPON_MAGAZINES as u32 + (magazine * wm::SIZEOF) as u32;
                inventory.rounds_total[slot][magazine] = i16_at(m, address + wm::ROUNDS_TOTAL as u32);
                inventory.rounds_loaded[slot][magazine] = i16_at(m, address + wm::ROUNDS_LOADED as u32);
            }
            let age = f32::from_bits(m.r32(weapon + wd::WEAPON_AGE as u32));
            // (so written that one not a number is none)
            let age = if age > 1.0 {
                1.0
            } else if age >= 0.0 {
                age
            } else {
                0.0
            };
            inventory.age[slot] = ((age * AGE_SCALE + 0.5) as f64).floor() as i32 as u16;
        }
    }
    inventory
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unit_state_round_trip() {
        let state = UnitState {
            player_index: 3,
            flags: unit_flag::ALIVE | unit_flag::PLACED | unit_flag::PREDICTED,
            killing_player_index: NO_PLAYER,
            unit_index: 0x1234_0005,
            vehicle_index: NONE,
            seat_index: -1,
            position: [1.5f32.to_bits(), (-2.0f32).to_bits(), 0.25f32.to_bits()],
            velocity: [1, -2, 3],
            forward: [32767, 0, 0],
            up: [0, 0, 32767],
            predicted_time: 77,
            body_vitality: 16384,
            shield_vitality: 8192,
            powerup_durations: [0, 30],
            ..UnitState::default()
        };
        let mut buffer = [0u8; 64];
        let size = unit_state_write(&state, true, &mut buffer);
        // 3 + unit 4 + placed (12 + 6 + 6) + predicted 2 + vitalities 4 + powerups 4
        assert_eq!(size, 41);
        let mut back = UnitState::default();
        assert_eq!(unit_state_read(&buffer[..size as usize], &mut back), size);
        assert_eq!(back, state);
        // to another client: no predicted tick
        assert_eq!(unit_state_write(&state, false, &mut buffer), 39);
        assert_eq!(unit_state_read(&buffer[..20], &mut back), 0);
    }

    #[test]
    fn relayed_round_trip() {
        let relayed = RelayedAction { player_index: 1, control_flags: [5, 5, 4, 4], yaw: -100, throttle_i: -127, ..Default::default() };
        let mut buffer = [0u8; 32];
        let size = relayed_action_write(&relayed, &mut buffer);
        assert_eq!(size, 14 + 2);
        let mut back = RelayedAction::default();
        assert_eq!(relayed_action_read(&buffer[..size as usize], &mut back), size);
        assert_eq!(back, relayed);
    }

    #[test]
    fn packing() {
        assert_eq!(vector_pack([1.0, -1.0, 0.5], UNIT_SCALE), [32767, -32767, 16384]);
        assert_eq!(vitality_pack(1.0), 16384);
        assert_eq!(angle_pack(PI), -32768);
        assert_eq!(angle_unpack(-32768, true), -PI);
    }
}
