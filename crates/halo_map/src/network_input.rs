//! The netcode's input handled (OpenCE's port/linux/game/
//! network_distributed.c's `distributed_handle_inputs` and
//! `distributed_handle_actions`).
//!
//! - **The host** takes a client's players' input, each tick's with the
//!   buttons of the ticks before it: only of that machine's players, the
//!   sticks and the trigger no further than a controller's. In co-op a
//!   player's input counts in the scripts' action tests as a local player's
//!   does, its turn the short way round. Once a message it measures how long
//!   a message takes the client and back (by the latest host tick the client
//!   had heard of), smoothed as TCP smooths its own: an eighth for the
//!   average, a quarter for the jitter.
//! - **A client** takes the host's relayed input of the players it drives:
//!   the host's update, then each player's buttons, facing, sticks, trigger
//!   and choices, unpacked from the wire.
//!
//! The input is a struct the game passes ([`Arguments`]); the facings and
//! round trips are the game's memory. Handing the input on (to the update
//! server or client), the action tests and the co-op BSP note are the
//! game's, through [`AiHost::call`] (network_distributed.c's
//! `network_input_call`).

use crate::ai_actor::{AiCall, AiHost, AiMemory, AiMemoryExt, NONE, WorldContext};
use crate::distributed_wire::{self as wire, PlayerAction, RelayedAction};
use crate::layout::{distributed_input_facing as fc, distributed_round_trip as rt, player_datum as pd};

/// network_distributed.c's input calls (halo_sim.h's `_network_input_call_*`).
pub mod call {
    /// `distributed_machine_has_player(a, b)`.
    pub const MACHINE_HAS_PLAYER: i32 = 1;
    /// `player_control_action_test_note(a, the turn input[0..2], the
    /// throttle input[2..4], the trigger input[4])`.
    pub const NOTE_ACTIONS: i32 = 2;
    /// `network_coop_note_player_structure_bsp(a, b)`.
    pub const NOTE_STRUCTURE_BSP: i32 = 3;
    /// `update_server_handle_distributed_input` of the player `a`, the input
    /// `b`'s tick and buttons, the arguments' action.
    pub const HAND_ON: i32 = 4;
    /// `update_client_handle_relayed_action` of the player `a`, the update
    /// `b`, the arguments' action and buttons.
    pub const RELAYED: i32 = 5;
}

/// halo_sim.h's `struct halo_sim_network_input`.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct Arguments {
    pub machine_index: i32,
    pub data: u32,
    pub end: u32,
    pub count: i32,
    pub facings: u32,
    pub round_trips: u32,
    pub coop_active: i32,
    pub action: PlayerAction,
    pub control_flags: [u16; 4],
}

const _: () = assert!(std::mem::size_of::<Arguments>() == 7 * 4 + 0x20 + 8);

/// network_distributed.c's limits.
const MAXIMUM_TRACKED_PLAYERS: u8 = crate::layout::net_values::MAXIMUM_TRACKED_PLAYERS as u8;
const MAXIMUM_ROUND_TRIP_TICKS: i32 = 60;
const MAXIMUM_MACHINES: i32 = 128;
/// `struct distributed_player_input`: its size, the player's structure BSP,
/// the client's tick, the host's latest tick it had, the action and the
/// ticks' buttons.
mod input {
    pub const SIZEOF: u32 = 52;
    pub const STRUCTURE_BSP_INDEX: u32 = 1;
    pub const HOST_TIME: u32 = 8;
    pub const ACTION: u32 = 12;
}
/// real_math.h's `_pi`.
const PI: f32 = 3.14159265358979323846;

/// cseries.h's `PIN`.
fn pin(n: f32, floor: f32, ceiling: f32) -> f32 {
    if n < floor {
        floor
    } else if n > ceiling {
        ceiling
    } else {
        n
    }
}

struct Input<'a, H: AiHost> {
    context: &'a WorldContext,
    host: &'a H,
    m: &'a dyn AiMemory,
    address: u32,
    a: Arguments,
}

impl<H: AiHost> Input<'_, H> {
    fn call(&self, kind: i32, a: i32, b: i32, input: &[f32]) -> AiCall {
        let mut call = AiCall { kind, actor: NONE, a, b, ..AiCall::default() };
        call.input[..input.len()].copy_from_slice(input);
        self.host.call(&mut call);
        call
    }

    /// The action (and the buttons) into the arguments, to be handed on.
    fn hand(&self, action: &PlayerAction, control_flags: Option<[u16; 4]>) {
        // SAFETY: plain data of its size
        let bytes: [u8; 0x20] = unsafe { std::mem::transmute(*action) };
        let at = self.address + std::mem::offset_of!(Arguments, action) as u32;
        for (index, &byte) in bytes.iter().enumerate() {
            self.m.w8(at + index as u32, byte);
        }
        if let Some(flags) = control_flags {
            let at = self.address + std::mem::offset_of!(Arguments, control_flags) as u32;
            for (index, &flag) in flags.iter().enumerate() {
                self.m.w16(at + 2 * index as u32, flag);
            }
        }
    }

    /// network_distributed.c's `distributed_player`.
    fn player(&self, player_index: u8) -> Option<u32> {
        use crate::data_array::{DATA_DATA, DATA_MAXIMUM_COUNT, DATA_SIZE};
        let m = self.m;
        let data = self.context.player_data;
        if player_index as i16 >= m.r16(data + DATA_MAXIMUM_COUNT) as i16 {
            return None;
        }
        let player = m.r32(data + DATA_DATA) + player_index as u32 * m.r16(data + DATA_SIZE) as i16 as u32;
        (m.i16(player + pd::IDENTIFIER as u32) != 0).then_some(player)
    }

    /// network_distributed.c's `distributed_note_input_actions`: a client
    /// player's input counted in the scripts' action tests, its turn since
    /// its last input the short way round.
    fn note_input_actions(&self, player_index: u8, action: &PlayerAction) {
        let m = self.m;
        let facing = self.a.facings + player_index as u32 * fc::SIZEOF as u32;
        let mut turn = [0.0f32; 2];
        if m.r8(facing + fc::VALID as u32) != 0 {
            turn[0] = action.yaw - m.f32(facing + fc::FACING as u32);
            turn[1] = action.pitch - m.f32(facing + fc::FACING as u32 + 4);
            if turn[0] > PI {
                turn[0] -= 2.0 * PI;
            } else if turn[0] < -PI {
                turn[0] += 2.0 * PI;
            }
        }
        m.w8(facing + fc::VALID as u32, 1);
        m.set_f32(facing + fc::FACING as u32, action.yaw);
        m.set_f32(facing + fc::FACING as u32 + 4, action.pitch);
        self.call(
            call::NOTE_ACTIONS,
            action.control_flags as i32,
            0,
            &[turn[0], turn[1], action.throttle[0], action.throttle[1], action.primary_trigger],
        );
    }

    /// network_distributed.c's `distributed_handle_inputs`.
    fn handle_inputs(&self) {
        let m = self.m;
        let a = &self.a;
        let machine_index = a.machine_index;
        let mut host_time = NONE;
        for index in 0..a.count.max(0) {
            let entry = a.data + index as u32 * input::SIZEOF;
            let player_index = m.r8(entry);
            if player_index >= MAXIMUM_TRACKED_PLAYERS
                || self.call(call::MACHINE_HAS_PLAYER, machine_index, player_index as i32, &[]).result as u8 == 0
            {
                continue;
            }
            let Some(player) = self.player(player_index) else { continue };
            // (the sticks and the trigger no further than a controller's)
            let mut bytes = [0u8; 0x20];
            for (offset, byte) in bytes.iter_mut().enumerate() {
                *byte = m.r8(entry + input::ACTION + offset as u32);
            }
            // SAFETY: plain data of its size
            let mut action: PlayerAction = unsafe { std::mem::transmute(bytes) };
            action.throttle[0] = pin(action.throttle[0], -1.0, 1.0);
            action.throttle[1] = pin(action.throttle[1], -1.0, 1.0);
            action.primary_trigger = pin(action.primary_trigger, 0.0, 1.0);
            self.hand(&action, None);
            // (a spectator's buttons pick whom it watches: not counted)
            if a.coop_active != 0 && m.i32(player + pd::UNIT_INDEX as u32) != NONE {
                self.note_input_actions(player_index, &action);
            }
            self.call(call::NOTE_STRUCTURE_BSP, player_index as i32, m.r8(entry + input::STRUCTURE_BSP_INDEX) as i32, &[]);
            let player_datum_index = (m.i16(player + pd::IDENTIFIER as u32) as i32) << 16 | player_index as i32;
            self.call(call::HAND_ON, player_datum_index, index, &[]);
            let input_host_time = m.i32(entry + input::HOST_TIME);
            if input_host_time != NONE && (host_time == NONE || input_host_time > host_time) {
                host_time = input_host_time;
            }
        }
        if host_time != NONE && (0..MAXIMUM_MACHINES).contains(&machine_index) {
            let sample = self.context.game_time.wrapping_sub(host_time);
            // (a longer one as the longest taken)
            if sample >= 0 {
                let sample = sample.min(MAXIMUM_ROUND_TRIP_TICKS) as f32;
                let round_trip = a.round_trips + machine_index as u32 * rt::SIZEOF as u32;
                let average = round_trip + rt::AVERAGE as u32;
                let deviation = round_trip + rt::DEVIATION as u32;
                // (as TCP smooths its own: an eighth, a quarter)
                if m.r8(round_trip + rt::VALID as u32) == 0 {
                    m.w8(round_trip + rt::VALID as u32, 1);
                    m.set_f32(average, sample);
                    m.set_f32(deviation, sample / 2.0);
                } else {
                    let difference = sample - m.f32(average);
                    m.set_f32(average, m.f32(average) + difference / 8.0);
                    m.set_f32(deviation, m.f32(deviation) + (difference.abs() - m.f32(deviation)) / 4.0);
                }
            }
        }
    }

    /// network_distributed.c's `distributed_handle_actions`.
    fn handle_actions(&self) {
        let m = self.m;
        let a = &self.a;
        let mut data = a.data;
        if a.end < data || a.end - data < 4 {
            return;
        }
        let update_number = m.i32(data);
        data += 4;
        for _ in 0..a.count.max(0) {
            let buffer: Vec<u8> = (data..a.end).map(|address| m.r8(address)).collect();
            let mut relayed = RelayedAction::default();
            let size = wire::relayed_action_read(&buffer, &mut relayed);
            if size == 0 {
                break;
            }
            data += size as u32;
            let action = PlayerAction {
                control_flags: relayed.control_flags[0] as u32,
                yaw: wire::angle_unpack(relayed.yaw, false),
                pitch: wire::angle_unpack(relayed.pitch, true),
                throttle: [
                    pin(relayed.throttle_i as f32 / 127.0, -1.0, 1.0),
                    pin(relayed.throttle_j as f32 / 127.0, -1.0, 1.0),
                ],
                primary_trigger: relayed.primary_trigger as f32 / 255.0,
                desired_weapon_index: relayed.desired_weapon_index as i16,
                desired_grenade_index: relayed.desired_grenade_index as i16,
                desired_zoom_level: relayed.desired_zoom_level as i16,
                pad: 0,
            };
            self.hand(&action, Some(relayed.control_flags));
            self.call(call::RELAYED, relayed.player_index as i32, update_number, &[]);
        }
    }
}

fn input<'a, H: AiHost>(context: &'a WorldContext, host: &'a H) -> Input<'a, H> {
    let m = host.memory();
    let mut bytes = [0u8; std::mem::size_of::<Arguments>()];
    for (index, byte) in bytes.iter_mut().enumerate() {
        *byte = m.r8(context.arguments + index as u32);
    }
    // SAFETY: plain data of its size
    let a: Arguments = unsafe { std::mem::transmute(bytes) };
    Input { context, host, m, address: context.arguments, a }
}

/// The world step of the host taking a client's players' input.
pub fn handle_inputs(context: &WorldContext, host: &impl AiHost) {
    input(context, host).handle_inputs();
}

/// The world step of a client taking the host's relayed input.
pub fn handle_actions(context: &WorldContext, host: &impl AiHost) {
    input(context, host).handle_actions();
}
