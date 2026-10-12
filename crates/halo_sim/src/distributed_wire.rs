//! The distributed netcode's wire format in Rust
//! ([`halo_map::distributed_wire`]): one entry for each of
//! network_distributed.c's packers, writers and readers, on the game's
//! structs and buffers.

use halo_map::distributed_wire::{
    Game, INPUT_HISTORY, ObjectState, PlayerAction, PlayerExtras, RelayedAction, UnitState, angle_pack, angle_unpack,
    axes_make_valid, checksum, inventory_from_unit, object_state_unpack, relayed_action_from, relayed_action_read,
    relayed_action_write, state_from_object, state_from_player, transform_valid, unit_state_read, unit_state_write,
    unit_vector_unpack, vector_clamp, vector_pack, vector_unpack, vitality_pack, vitality_unpack,
};

use crate::ai_steps::GameMemory;

/// halo_sim.h's `struct halo_sim_wire_call`.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct WireCall {
    pub kind: i32,
    pub input: *const u8,
    pub end: *const u8,
    pub output: *mut u8,
    pub a: i32,
    pub b: i32,
    pub value: f32,
    pub result: i32,
    pub real_result: f32,
}

/// halo_sim.h's `_halo_sim_wire_*`.
pub mod kind {
    pub const VECTOR_PACK: i32 = 1;
    pub const VECTOR_UNPACK: i32 = 2;
    pub const UNIT_VECTOR_UNPACK: i32 = 3;
    pub const VITALITY_PACK: i32 = 4;
    pub const VITALITY_UNPACK: i32 = 5;
    pub const ANGLE_PACK: i32 = 6;
    pub const ANGLE_UNPACK: i32 = 7;
    pub const UNIT_STATE_WRITE: i32 = 8;
    pub const UNIT_STATE_READ: i32 = 9;
    pub const RELAYED_ACTION_WRITE: i32 = 10;
    pub const RELAYED_ACTION_READ: i32 = 11;
    pub const RELAYED_ACTION_FROM: i32 = 12;
    pub const AXES_MAKE_VALID: i32 = 13;
    pub const TRANSFORM_VALID: i32 = 14;
    pub const VECTOR_CLAMP: i32 = 15;
    pub const CHECKSUM: i32 = 16;
    pub const OBJECT_STATE_UNPACK: i32 = 17;
    pub const STATE_FROM_PLAYER: i32 = 18;
    pub const STATE_FROM_OBJECT: i32 = 19;
    pub const INVENTORY_FROM_UNIT: i32 = 20;
}

/// halo_sim.h's `struct halo_sim_wire_transform`: a transform's parts,
/// the velocities' null for none.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct WireTransform {
    pub position: *const [f32; 3],
    pub forward: *const [f32; 3],
    pub up: *const [f32; 3],
    pub velocity: *const [f32; 3],
    pub angular_velocity: *const [f32; 3],
}

/// The most bytes a unit state and a relayed action take on the wire.
const UNIT_STATE_MAXIMUM_SIZE: usize = 64;
const RELAYED_ACTION_MAXIMUM_SIZE: usize = 20;

/// # Safety
/// `pointer` must hold a `T`.
unsafe fn read<T: Copy>(pointer: *const u8) -> T {
    // SAFETY: as the caller promises
    unsafe { (pointer as *const T).read_unaligned() }
}

/// # Safety
/// `pointer` must have room for a `T`.
unsafe fn write<T>(pointer: *mut u8, value: T) {
    // SAFETY: as the caller promises
    unsafe { (pointer as *mut T).write_unaligned(value) }
}

/// The bytes from `input` to `end`.
///
/// # Safety
/// They must be readable.
unsafe fn span<'a>(input: *const u8, end: *const u8) -> &'a [u8] {
    let length = (end as usize).saturating_sub(input as usize);
    // SAFETY: as the caller promises
    unsafe { std::slice::from_raw_parts(input, length) }
}

/// Runs the call's packer, writer or reader. Returns 0, or -1 for a kind
/// it doesn't know.
///
/// # Safety
/// `call` must be valid, its pointers as its kind takes them.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn halo_sim_distributed_wire(call: *mut WireCall) -> i32 {
    if call.is_null() {
        return -1;
    }
    // SAFETY: the caller's call, and its pointers as its kind takes them
    unsafe {
        let call = &mut *call;
        match call.kind {
            kind::VECTOR_PACK => write(call.output, vector_pack(read(call.input), call.value)),
            kind::VECTOR_UNPACK => write(call.output, vector_unpack(read(call.input), call.value)),
            kind::UNIT_VECTOR_UNPACK => write(call.output, unit_vector_unpack(read(call.input))),
            kind::VITALITY_PACK => call.result = vitality_pack(call.value) as i32,
            kind::VITALITY_UNPACK => call.real_result = vitality_unpack(call.a as u16),
            kind::ANGLE_PACK => call.result = angle_pack(call.value) as i32,
            kind::ANGLE_UNPACK => call.real_result = angle_unpack(call.a as i16, call.b != 0),
            kind::UNIT_STATE_WRITE => {
                let buffer = std::slice::from_raw_parts_mut(call.output, UNIT_STATE_MAXIMUM_SIZE);
                call.result = unit_state_write(&read::<UnitState>(call.input), call.a != 0, buffer) as i32;
            }
            kind::UNIT_STATE_READ => {
                let mut state = UnitState::default();
                call.result = unit_state_read(span(call.input, call.end), &mut state) as i32;
                write(call.output, state);
            }
            kind::RELAYED_ACTION_WRITE => {
                let buffer = std::slice::from_raw_parts_mut(call.output, RELAYED_ACTION_MAXIMUM_SIZE);
                call.result = relayed_action_write(&read::<RelayedAction>(call.input), buffer) as i32;
            }
            kind::RELAYED_ACTION_READ => {
                let mut relayed = RelayedAction::default();
                call.result = relayed_action_read(span(call.input, call.end), &mut relayed) as i32;
                write(call.output, relayed);
            }
            kind::RELAYED_ACTION_FROM => {
                // (the ticks' actions, latest first, and their counts)
                let actions = call.input as *const *const PlayerAction;
                let counts = call.end as *const i16;
                let player_index = call.a as i16;
                let recent: [Option<&PlayerAction>; INPUT_HISTORY] = std::array::from_fn(|history| {
                    let tick = actions.add(history).read_unaligned();
                    let count = counts.add(history).read_unaligned();
                    if history == 0 {
                        tick.add(player_index as usize).as_ref()
                    } else if !tick.is_null() && player_index < count {
                        tick.add(player_index as usize).as_ref()
                    } else {
                        None
                    }
                });
                let mut relayed = RelayedAction::default();
                call.result = relayed_action_from(player_index, &recent, &mut relayed) as i32;
                write(call.output, relayed);
            }
            kind::AXES_MAKE_VALID => {
                let mut axes: [f32; 6] = read(call.input);
                call.result = axes_make_valid(&mut axes) as i32;
                write(call.output, axes);
            }
            kind::TRANSFORM_VALID => {
                let transform: WireTransform = read(call.input);
                let optional = |pointer: *const [f32; 3]| (!pointer.is_null()).then(|| pointer.read_unaligned());
                // (written only as far as the game's would be)
                let mut valid: [f32; 6] = read(call.output);
                call.result = transform_valid(
                    transform.position.read_unaligned(),
                    transform.forward.read_unaligned(),
                    transform.up.read_unaligned(),
                    optional(transform.velocity),
                    optional(transform.angular_velocity),
                    &mut valid,
                ) as i32;
                write(call.output, valid);
            }
            kind::VECTOR_CLAMP => {
                let mut vector: [f32; 3] = read(call.input);
                vector_clamp(&mut vector, call.value);
                write(call.output, vector);
            }
            kind::CHECKSUM => {
                call.result = checksum(std::slice::from_raw_parts(call.input, call.a.max(0) as usize)) as i32;
            }
            kind::OBJECT_STATE_UNPACK => write(call.output, object_state_unpack(&read::<ObjectState>(call.input))),
            kind::STATE_FROM_PLAYER => {
                let game: Game = read(call.end);
                write(call.output, state_from_player(&GameMemory, &game, call.a as i16, &read::<PlayerExtras>(call.input)));
            }
            kind::STATE_FROM_OBJECT => {
                let game: Game = read(call.end);
                write(call.output, state_from_object(&GameMemory, &game, call.a));
            }
            kind::INVENTORY_FROM_UNIT => {
                let game: Game = read(call.end);
                write(call.output, inventory_from_unit(&GameMemory, &game, call.a));
            }
            _ => return -1,
        }
    }
    0
}
