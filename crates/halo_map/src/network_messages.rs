//! The netcode's messages handled (OpenCE's port/linux/game/
//! network_distributed.c's `network_distributed_handle_message`): the front
//! door every message of the distributed netcode comes through.
//!
//! - **Voice chat's** go to voice chat (in the lobby too).
//! - **None between games.**
//! - **A batch** (a tick's unreliable messages to one machine in one
//!   datagram) is taken apart, each message handled as if it came alone, no
//!   batch in a batch, none longer than the datagram.
//! - **Each message** must be of a kind there is, with as many bytes as its
//!   count of entries needs (and one entry for the kinds whose handlers read
//!   one). It must come the way its kind goes: from a client to the host
//!   (from a machine the host has), or from the host to a client, which
//!   notes the host's latest tick. A message of a kind sent every tick older
//!   than the sender's latest of it is dropped (a killing blow sent reliably
//!   is not, unless it came in a batch). The host notes a client's clock by
//!   its messages' ticks, and takes no player's or vehicle's prediction from
//!   a client whose game runs fast.
//! - Then it goes to its handler.
//!
//! The message and the kinds' tables are a struct the game passes
//! ([`Arguments`]); the senders' latest ticks are the game's memory. Voice
//! chat, the client's clock and the handlers are the game's, through
//! [`AiHost::call`] (network_distributed.c's `network_message_call`).

use crate::ai_actor::{AiCall, AiHost, AiMemory, AiMemoryExt, NONE, WorldContext};

/// network_distributed.c's message calls (halo_sim.h's
/// `_network_message_call_*`): `b` names the message (0 the one handled, 1
/// the batch's in the buffer), `actor` its size.
pub mod call {
    /// `network_voice_handles_message`, and if so handled: the result.
    pub const VOICE: i32 = 1;
    /// `distributed_note_client_clock(a, b)`: whether its game runs fast.
    pub const CLOCK: i32 = 2;
    /// The message to its handler, from the machine `a`.
    pub const DISPATCH: i32 = 3;
}

/// halo_sim.h's `struct halo_sim_network_message`.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct Arguments {
    pub machine_index: i32,
    pub message: u32,
    pub size: i32,
    pub in_progress: i32,
    pub connection: i32,
    pub server_connection: i32,
    pub client_connection: i32,
    pub host_time: u32,
    pub received: u32,
    pub handling_batch: u32,
    pub received_times: u32,
    pub batch_type: i16,
    pub message_types: i16,
    pub damage_events_type: i16,
    pub player_prediction_type: i16,
    pub vehicle_prediction_type: i16,
    pub message_header_size: i16,
    pub senders: i16,
    pub host_sender: i16,
    pub buffer_size: i16,
    pub pad: i16,
    pub entry_sizes: [i16; 96],
    pub kinds: [u8; 96],
    pub buffer: [u8; 1204],
}

const _: () = assert!(std::mem::size_of::<Arguments>() == 11 * 4 + 10 * 2 + 96 * 2 + 96 + 1204);

/// `_network_message_kind_*`.
const KIND_FROM_CLIENT: u8 = 1 << 0;
const KIND_NEEDS_ENTRY: u8 = 1 << 1;
const KIND_STALE: u8 = 1 << 2;

/// `struct distributed_message_header`'s size and its game time's offset.
const HEADER_SIZE: u32 = 8;
const GAME_TIME: u32 = 4;

struct Messages<'a, H: AiHost> {
    host: &'a H,
    m: &'a dyn AiMemory,
    address: u32,
    a: Arguments,
}

impl<H: AiHost> Messages<'_, H> {
    fn call(&self, kind: i32, a: i32, b: i32, actor: i32) -> AiCall {
        let mut call = AiCall { kind, actor, a, b, ..AiCall::default() };
        self.host.call(&mut call);
        call
    }

    fn buffer(&self) -> u32 {
        self.address + std::mem::offset_of!(Arguments, buffer) as u32
    }

    /// network_distributed.c's `distributed_message_stale`: a kind sent
    /// every tick, older than the sender's latest of it (which it becomes
    /// otherwise).
    fn stale(&self, machine_index: i32, kind: u8, game_time: i32) -> bool {
        let m = self.m;
        let a = &self.a;
        if a.kinds[kind as usize] & KIND_STALE == 0 {
            return false;
        }
        let sender = if machine_index == NONE { a.host_sender } else { machine_index as i16 };
        if sender < 0 || sender >= a.senders {
            return true;
        }
        let latest = a.received_times + 4 * (sender as u32 * a.message_types as u32 + kind as u32);
        if m.i32(latest) != NONE && game_time < m.i32(latest) {
            return true;
        }
        m.w32(latest, game_time as u32);
        false
    }

    /// network_distributed.c's `network_distributed_handle_message` of the
    /// message `which` names, of its size.
    fn handle(&self, machine_index: i32, which: i32, size: u16) {
        let m = self.m;
        let a = &self.a;
        let message = if which != 0 { self.buffer() } else { a.message };
        // (voice chat's, in the lobby too)
        if self.call(call::VOICE, machine_index, which, size as i32).result != 0 {
            return;
        }
        // (none between games: loading, or in the menus)
        if (size as u32) < HEADER_SIZE || a.in_progress == 0 {
            return;
        }
        let header_size = a.message_header_size as u32;
        let kind = m.r8(message + header_size);
        let count = m.r8(message + header_size + 1);
        let game_time = m.i32(message + GAME_TIME);
        // a tick's messages in one: each as if it came alone
        if kind as i16 == a.batch_type {
            let mut offset = HEADER_SIZE as u16;
            while offset as u32 + 2 <= size as u32 {
                let length = m.r16(message + offset as u32);
                offset += 2;
                if length > size - offset
                    || (length as u32) < HEADER_SIZE - header_size
                    || header_size + length as u32 > a.buffer_size as u16 as u32
                {
                    break;
                }
                let buffer = self.buffer();
                for index in 0..header_size {
                    m.w8(buffer + index, m.r8(message + index));
                }
                for index in 0..length as u32 {
                    m.w8(buffer + header_size + index, m.r8(message + offset as u32 + index));
                }
                offset += length;
                // (no batch in a batch)
                if m.r8(buffer + header_size) as i16 != a.batch_type {
                    m.w8(a.handling_batch, 1);
                    self.handle(machine_index, 1, (header_size + length as u32) as u16);
                    m.w8(a.handling_batch, 0);
                }
            }
            return;
        }
        let types = a.message_types as u8;
        let entry_size = if kind < types { a.entry_sizes[kind as usize] as u16 as u32 } else { 0 };
        if kind == 0 || kind >= types || (size as u32) < HEADER_SIZE + count as u32 * entry_size {
            return;
        }
        if a.kinds[kind as usize] & KIND_NEEDS_ENTRY != 0 && count < 1 {
            return;
        }
        m.w32(a.received, m.r32(a.received).wrapping_add(1));
        // (each kind from the host, or from a client)
        if a.kinds[kind as usize] & KIND_FROM_CLIENT != 0 {
            if machine_index == NONE || a.connection != a.server_connection {
                return;
            }
        } else {
            if a.connection != a.client_connection {
                return;
            }
            // (the host's latest tick, which this client's input tells it back)
            let host_time = m.i32(a.host_time);
            if host_time == NONE || game_time > host_time {
                m.w32(a.host_time, game_time as u32);
            }
        }
        if (m.r8(a.handling_batch) != 0 || kind as i16 != a.damage_events_type) && self.stale(machine_index, kind, game_time) {
            return;
        }
        // (the host: a client's clock, by its messages' ticks; its players'
        // predictions not taken while its game runs fast)
        if machine_index != NONE {
            let fast = self.call(call::CLOCK, machine_index, game_time, NONE).result as u8 != 0;
            if (kind as i16 == a.player_prediction_type || kind as i16 == a.vehicle_prediction_type) && fast {
                return;
            }
        }
        self.call(call::DISPATCH, machine_index, which, size as i32);
    }
}

/// The world step of a netcode message handled.
pub fn handle_message(context: &WorldContext, host: &impl AiHost) {
    let m = host.memory();
    let mut bytes = [0u8; std::mem::size_of::<Arguments>()];
    for (index, byte) in bytes.iter_mut().enumerate() {
        *byte = m.r8(context.arguments + index as u32);
    }
    // SAFETY: plain data of its size
    let a: Arguments = unsafe { std::mem::transmute(bytes) };
    let messages = Messages { host, m, address: context.arguments, a };
    messages.handle(a.machine_index, 0, a.size as u16);
}
