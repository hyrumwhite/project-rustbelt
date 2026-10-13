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

// ---------- a client's sends each tick

/// network_distributed.c's client sends' calls (halo_sim.h's
/// `_network_client_sends_call_*`).
pub mod sends_call {
    /// `update_client_distributed_input(a)` into the arguments' tick, action
    /// and buttons: whether there is one.
    pub const LOCAL_INPUT: i32 = 1;
    /// The arguments' message, its type `b & 0xff`, count `(b >> 8) & 0xff`
    /// and size `b >> 16`, where `actor` says.
    pub const SEND: i32 = 2;
    /// `distributed_state_from_player(a)` into the arguments' state.
    pub const STATE: i32 = 3;
    /// The packer begun for the sender `a`'s messages of type `b`.
    pub const PACKER_BEGIN: i32 = 4;
    /// The arguments' entry, of `b` bytes, into the packer.
    pub const PACKER_ADD: i32 = 5;
    /// The packer's message so far into the batch.
    pub const PACKER_FLUSH: i32 = 6;
}

/// halo_sim.h's `struct halo_sim_network_client_sends`.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct ClientSendsArguments {
    pub own_positions: u32,
    pub host_time: i32,
    pub structure_bsp_index: i32,
    pub lie_ticks: i32,
    pub local_players: [i32; 4],
    pub inputs_type: i16,
    pub predictions_type: i16,
    pub to_host: i16,
    pub host_sender: i16,
    pub tick: i32,
    pub action: PlayerAction,
    pub control_flags: [u16; 4],
    pub state: [u8; 64],
    pub entry: [u8; 64],
    pub message: [u8; 8 + 4 * 52],
}

const _: () = assert!(std::mem::size_of::<ClientSendsArguments>() == 4 * 4 + 16 + 8 + 4 + 0x20 + 8 + 64 + 64 + 216);

/// network_distributed.c's `OWN_POSITION_TICKS` and `struct
/// distributed_own_position`'s size.
const OWN_POSITION_TICKS: i32 = 64;
const OWN_POSITION_SIZE: u32 = 20;
/// The tests' lie: a step aside, up, down, a dash (network_distributed.c's).
const LIE_OFFSETS: [[f32; 3]; 4] = [[2.5, 0.0, 0.0], [0.0, 0.0, 2.5], [0.0, 0.0, -2.5], [1.0, 1.0, 0.0]];

struct ClientSends<'a, H: AiHost> {
    context: &'a WorldContext,
    host: &'a H,
    m: &'a dyn AiMemory,
    address: u32,
    a: ClientSendsArguments,
}

impl<H: AiHost> ClientSends<'_, H> {
    fn call(&self, kind: i32, a: i32, b: i32) -> AiCall {
        let mut call = AiCall { kind, actor: NONE, a, b, ..AiCall::default() };
        self.host.call(&mut call);
        call
    }

    fn field(&self, offset: usize) -> u32 {
        self.address + offset as u32
    }

    fn object(&self, object_index: i32) -> Option<u32> {
        crate::ai_actor::datum_get(self.m, self.context.object_header_data, object_index)
            .map(|header| self.m.r32(header + crate::layout::object_header_datum::DATUM as u32))
    }

    /// network_distributed.c's `distributed_living_unit`.
    fn living_unit(&self, player: Option<u32>) -> i32 {
        let Some(player) = player else { return NONE };
        let unit_index = self.m.i32(player + pd::UNIT_INDEX as u32);
        match self.object(unit_index) {
            Some(unit)
                if unit_index != NONE
                    && self.m.r16(unit + crate::layout::unit_datum::OBJECT_DAMAGE_FLAGS as u32) as u32
                        & (1 << crate::layout::net_values::_OBJECT_DEAD_BIT)
                        == 0 =>
            {
                unit_index
            }
            _ => NONE,
        }
    }

    fn parent(&self, unit_index: i32) -> i32 {
        self.m.i32(self.object(unit_index).unwrap_or(0) + crate::layout::unit_datum::OBJECT_PARENT_OBJECT_INDEX as u32)
    }

    /// network_distributed.c's `distributed_note_own_positions`: where its
    /// own players' units are this tick (not riding: a rider's is its
    /// seat's).
    fn note_own_positions(&self) {
        let m = self.m;
        let now = self.context.game_time;
        for (local_player_index, &player_index) in self.a.local_players.iter().enumerate() {
            let player = if player_index != NONE {
                crate::ai_actor::datum_get(m, self.context.player_data, player_index)
            } else {
                None
            };
            let mut unit_index = self.living_unit(player);
            let own = self.a.own_positions
                + (local_player_index as u32 * OWN_POSITION_TICKS as u32 + (now & (OWN_POSITION_TICKS - 1)) as u32) * OWN_POSITION_SIZE;
            if unit_index != NONE && self.parent(unit_index) != NONE {
                unit_index = NONE;
            }
            m.w32(own, now as u32);
            m.w32(own + 4, unit_index as u32);
            if unit_index != NONE {
                let object = self.object(unit_index).unwrap_or(0);
                for part in 0..3u32 {
                    m.w32(own + 8 + 4 * part, m.r32(object + crate::layout::unit_datum::OBJECT_POSITION as u32 + 4 * part));
                }
            }
        }
    }

    /// network_distributed.c's `distributed_client_send_inputs`: its own
    /// players' input of the tick just run, with the buttons of the ticks
    /// before it, to the host.
    fn send_inputs(&self) {
        let m = self.m;
        let a = &self.a;
        let message = self.field(std::mem::offset_of!(ClientSendsArguments, message));
        let mut count: u32 = 0;
        for (local_player_index, &player_index) in a.local_players.iter().enumerate() {
            let entry = message + 8 + count * 52;
            if player_index == NONE {
                continue;
            }
            for offset in 0..52 {
                m.w8(entry + offset, 0);
            }
            if self.call(sends_call::LOCAL_INPUT, local_player_index as i32, 0).result as u8 == 0 {
                continue;
            }
            m.w32(entry + 4, m.r32(self.field(std::mem::offset_of!(ClientSendsArguments, tick))));
            for offset in 0..0x20 {
                m.w8(entry + 12 + offset, m.r8(self.field(std::mem::offset_of!(ClientSendsArguments, action)) + offset));
            }
            for offset in 0..8 {
                m.w8(entry + 44 + offset, m.r8(self.field(std::mem::offset_of!(ClientSendsArguments, control_flags)) + offset));
            }
            m.w8(entry, player_index as u8);
            m.w8(entry + 1, a.structure_bsp_index as u8);
            m.w32(entry + 8, a.host_time as u32);
            count += 1;
        }
        if count != 0 {
            let b = (a.inputs_type as i32 & 0xff) | ((count as i32) << 8) | (((8 + count * 52) as i32) << 16);
            let mut call = AiCall { kind: sends_call::SEND, actor: a.to_host as i32, a: 0, b, ..AiCall::default() };
            self.host.call(&mut call);
        }
    }

    /// network_distributed.c's `distributed_client_send_predictions`: its
    /// own players' units on foot, where they are, to the host.
    fn send_predictions(&self) {
        use crate::data_array::{DATA_COUNT, DATA_DATA, DATA_SIZE};
        let m = self.m;
        let a = &self.a;
        let now = self.context.game_time;
        self.call(sends_call::PACKER_BEGIN, a.host_sender as i32, a.predictions_type as i32);
        let data = self.context.player_data;
        let size = m.r16(data + DATA_SIZE) as i16 as u32;
        for player_index in 0..m.r16(data + DATA_COUNT) as i16 as u32 {
            let player = m.r32(data + DATA_DATA) + player_index * size;
            if m.i16(player + pd::IDENTIFIER as u32) == 0 {
                continue;
            }
            let unit_index = self.living_unit(Some(player));
            // a client speaks for its own players only, where they are
            if m.i16(player + pd::LOCAL_PLAYER_INDEX as u32) == NONE as i16 || unit_index == NONE || self.parent(unit_index) != NONE {
                continue;
            }
            self.call(sends_call::STATE, player_index as i32, 0);
            let state_address = self.field(std::mem::offset_of!(ClientSendsArguments, state));
            let mut bytes = [0u8; 64];
            for (offset, byte) in bytes.iter_mut().enumerate() {
                *byte = m.r8(state_address + offset as u32);
            }
            // SAFETY: plain data of its size
            let mut state: wire::UnitState = unsafe { std::mem::transmute(bytes) };
            // (the tests' lie, every so often for three ticks)
            if a.lie_ticks > 0 && now % a.lie_ticks < 3 {
                let offset = LIE_OFFSETS[((now / a.lie_ticks) % 4) as usize];
                for part in 0..3 {
                    state.position[part] = (f32::from_bits(state.position[part]) + offset[part]).to_bits();
                }
            }
            let mut entry = [0u8; 64];
            let written = wire::unit_state_write(&state, true, &mut entry) as usize;
            let entry_address = self.field(std::mem::offset_of!(ClientSendsArguments, entry));
            for (offset, &byte) in entry[..written].iter().enumerate() {
                m.w8(entry_address + offset as u32, byte);
            }
            self.call(sends_call::PACKER_ADD, 0, written as i32);
        }
        self.call(sends_call::PACKER_FLUSH, 0, 0);
    }
}

/// The world step of a client's sends each tick (network_distributed.c's
/// `distributed_note_own_positions`, `distributed_client_send_inputs` and
/// `distributed_client_send_predictions`).
pub fn client_sends(context: &WorldContext, host: &impl AiHost) {
    let m = host.memory();
    let mut bytes = [0u8; std::mem::size_of::<ClientSendsArguments>()];
    for (index, byte) in bytes.iter_mut().enumerate() {
        *byte = m.r8(context.arguments + index as u32);
    }
    // SAFETY: plain data of its size
    let a: ClientSendsArguments = unsafe { std::mem::transmute(bytes) };
    let sends = ClientSends { context, host, m, address: context.arguments, a };
    sends.note_own_positions();
    sends.send_inputs();
    sends.send_predictions();
}
