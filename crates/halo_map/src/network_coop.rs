//! Network co-op (OpenCE's port/linux/game/network_coop.c): a campaign
//! level played together, its world run on the host and shown on its
//! clients.
//!
//! The host sends each tick what changed of the world its scripts and
//! machines move (`host_send_object_transforms`, `host_send_device_states`,
//! `host_send_object_looks`):
//! - **Scenery and machines** that moved since last sent (by a hundredth of
//!   a world unit, or turned), where they are and how they face.
//! - **Devices** whose position or power changed: doors, elevators, a light
//!   bridge.
//! - **Objects** whose look changed: their regions' permutations (a broken
//!   part) or their scale.
//!
//! An object first seen is where and how the map placed it, on every
//! machine, so nothing goes. With the resent state (a machine joined, or
//! every two seconds) everything that ever changed goes again.
//!
//! The host's records are the game's memory (network_coop_records.h), the
//! step's arguments a struct the game passes ([`WorldArguments`]); sending is
//! the game's, through [`AiHost::call`] (network_coop.c's
//! `network_coop_world_call`).

use crate::ai_actor::{AiCall, AiHost, AiMemory, AiMemoryExt, NONE, WorldContext};
use crate::data_array::{DATA_COUNT, DATA_DATA, DATA_SIZE};
use crate::distributed_wire::{self as wire, UNIT_SCALE};
use crate::layout::{
    coop_sent_device as sd, coop_sent_look as sl, coop_sent_transform as st, device_datum as dd,
    distributed_coop_device_state as ds, distributed_coop_object_look as ol, distributed_coop_object_transform as ot,
    net_values as nv, object_header_datum as oh, unit_datum as ud,
};

/// network_coop.c's world calls (halo_sim.h's `_network_coop_world_call_*`).
pub mod call {
    /// The message, of the type `a`, `b` entries of `actor` bytes, to every
    /// client.
    pub const SEND: i32 = 1;
}

/// halo_sim.h's `struct halo_sim_network_coop_world`.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct WorldArguments {
    pub refresh: i32,
    pub sent_transforms: u32,
    pub sent_devices: u32,
    pub sent_looks: u32,
    pub message: u32,
    pub transforms_type: i16,
    pub device_states_type: i16,
    pub looks_type: i16,
    pub pad: i16,
}

/// network_coop.c's limits: the entries in a message, and the objects a map
/// has.
const MAXIMUM_ENTRIES_PER_MESSAGE: i16 = 64;
const MAXIMUM_OBJECTS_PER_MAP: i32 = nv::MAXIMUM_TRACKED_OBJECTS as i32;
/// network_coop.c's `COOP_MOVED_OBJECTS`.
const COOP_MOVED_OBJECTS: u32 = (nv::_OBJECT_MASK_SCENERY | nv::_OBJECT_MASK_MACHINE) as u32;
/// `struct distributed_message_header`'s size.
const HEADER: u32 = 8;

struct World<'a, H: AiHost> {
    context: &'a WorldContext,
    host: &'a H,
    m: &'a dyn AiMemory,
    a: WorldArguments,
}

impl<H: AiHost> World<'_, H> {
    fn send(&self, kind: i16, count: i16, entry_size: u32) {
        let mut call = AiCall { kind: call::SEND, actor: entry_size as i32, a: kind as i32, b: count as i32, ..AiCall::default() };
        self.host.call(&mut call);
    }

    /// The objects of the types (`object_iterator_next`, no header flags
    /// asked): their datum indices and datums.
    fn objects(&self, mask: u32) -> Vec<(i32, u32)> {
        let m = self.m;
        let data = self.context.object_header_data;
        let size = m.r16(data + DATA_SIZE) as i16 as u32;
        let base = m.r32(data + DATA_DATA);
        let mut objects = Vec::new();
        for absolute_index in 0..m.r16(data + DATA_COUNT) as i16 as u32 {
            let header = base + absolute_index * size;
            let identifier = m.i16(header);
            if identifier != 0 && mask & (1 << m.r8(header + oh::TYPE as u32)) != 0 {
                objects.push(((identifier as i32) << 16 | absolute_index as i32, m.r32(header + oh::DATUM as u32)));
            }
        }
        objects
    }

    fn copy(&self, to: u32, from: u32, size: u32) {
        for offset in 0..size {
            self.m.w8(to + offset, self.m.r8(from + offset));
        }
    }

    /// The entry's common head: the object's name, a pad, the object and its
    /// tag.
    fn entry_head(&self, entry: u32, object_index: i32, object: u32) {
        let m = self.m;
        m.w16(entry, m.r16(object + ud::OBJECT_NAME_INDEX as u32));
        m.w16(entry + 2, 0);
        m.w32(entry + 4, object_index as u32);
        m.w32(entry + 8, m.r32(object + ud::DEFINITION_INDEX as u32));
    }

    /// network_coop.c's `host_send_object_transforms`.
    fn send_transforms(&self) {
        let m = self.m;
        let a = &self.a;
        let mut count: i16 = 0;
        for (object_index, object) in self.objects(COOP_MOVED_OBJECTS) {
            let absolute_index = object_index as i16 as i32;
            if m.i32(object + ud::OBJECT_PARENT_OBJECT_INDEX as u32) != NONE || !(0..MAXIMUM_OBJECTS_PER_MAP).contains(&absolute_index) {
                continue;
            }
            let record = a.sent_transforms + absolute_index as u32 * st::SIZEOF as u32;
            let position = object + ud::OBJECT_POSITION as u32;
            let forward = object + ud::OBJECT_FORWARD as u32;
            if m.i32(record + st::OBJECT_INDEX as u32) != object_index {
                // (an object first seen is where the map placed it, on every
                // machine)
                m.w32(record + st::OBJECT_INDEX as u32, object_index as u32);
                m.w8(record + st::MOVED as u32, 0);
                self.copy(record + st::POSITION as u32, position, 12);
                self.copy(record + st::FORWARD as u32, forward, 12);
                continue;
            }
            let (sent_position, now_position) = (m.vector(record + st::POSITION as u32), m.vector(position));
            let (sent_forward, now_forward) = (m.vector(record + st::FORWARD as u32), m.vector(forward));
            let d = [now_position[0] - sent_position[0], now_position[1] - sent_position[1], now_position[2] - sent_position[2]];
            let moving = d[0] * d[0] + d[1] * d[1] + d[2] * d[2] >= 0.0001
                || sent_forward[0] * now_forward[0] + sent_forward[1] * now_forward[1] + sent_forward[2] * now_forward[2] <= 0.9999;
            if !moving && !(a.refresh != 0 && m.r8(record + st::MOVED as u32) != 0) {
                continue;
            }
            m.w8(record + st::MOVED as u32, 1);
            self.copy(record + st::POSITION as u32, position, 12);
            self.copy(record + st::FORWARD as u32, forward, 12);
            let entry = a.message + HEADER + count as u32 * ot::SIZEOF as u32;
            self.entry_head(entry, object_index, object);
            self.copy(entry + ot::POSITION as u32, position, 12);
            let packed = [
                wire::vector_pack(now_forward, UNIT_SCALE),
                wire::vector_pack(m.vector(object + ud::OBJECT_UP as u32), UNIT_SCALE),
            ];
            for (at, vector) in [(ot::FORWARD, packed[0]), (ot::UP, packed[1])] {
                for (part, value) in vector.iter().enumerate() {
                    m.w16(entry + at as u32 + 2 * part as u32, *value as u16);
                }
            }
            count += 1;
            if count == MAXIMUM_ENTRIES_PER_MESSAGE {
                self.send(a.transforms_type, count, ot::SIZEOF as u32);
                count = 0;
            }
        }
        if count > 0 {
            self.send(a.transforms_type, count, ot::SIZEOF as u32);
        }
    }

    /// network_coop.c's `host_send_device_states`.
    fn send_device_states(&self) {
        let m = self.m;
        let a = &self.a;
        let mut count: i16 = 0;
        for (object_index, device) in self.objects(nv::_OBJECT_MASK_DEVICE as u32) {
            let absolute_index = object_index as i16 as i32;
            if !(0..MAXIMUM_OBJECTS_PER_MAP).contains(&absolute_index) {
                continue;
            }
            let record = a.sent_devices + absolute_index as u32 * sd::SIZEOF as u32;
            let position = m.f32(device + dd::DEVICE_POSITION as u32);
            let power = m.f32(device + dd::DEVICE_POWER as u32);
            if m.i32(record + sd::OBJECT_INDEX as u32) != object_index {
                // (as the map placed it, on every machine)
                m.w32(record + sd::OBJECT_INDEX as u32, object_index as u32);
                m.w8(record + sd::MOVED as u32, 0);
                m.set_f32(record + sd::POSITION as u32, position);
                m.set_f32(record + sd::POWER as u32, power);
                continue;
            }
            let changed = m.f32(record + sd::POSITION as u32) != position || m.f32(record + sd::POWER as u32) != power;
            if !changed && !(a.refresh != 0 && m.r8(record + sd::MOVED as u32) != 0) {
                continue;
            }
            m.w8(record + sd::MOVED as u32, 1);
            m.set_f32(record + sd::POSITION as u32, position);
            m.set_f32(record + sd::POWER as u32, power);
            let entry = a.message + HEADER + count as u32 * ds::SIZEOF as u32;
            m.w16(entry + ds::NAME_INDEX as u32, m.r16(device + ud::OBJECT_NAME_INDEX as u32));
            m.w8(entry + ds::MOVING as u32, (m.f32(device + dd::DEVICE_POSITION_VELOCITY as u32) != 0.0) as u8);
            m.w8(entry + ds::MOVING as u32 + 1, 0);
            m.w32(entry + ds::OBJECT_INDEX as u32, object_index as u32);
            m.w32(entry + ds::DEFINITION_INDEX as u32, m.r32(device + ud::DEFINITION_INDEX as u32));
            m.set_f32(entry + ds::POSITION as u32, position);
            m.set_f32(entry + ds::POWER as u32, power);
            count += 1;
            if count == MAXIMUM_ENTRIES_PER_MESSAGE {
                self.send(a.device_states_type, count, ds::SIZEOF as u32);
                count = 0;
            }
        }
        if count > 0 {
            self.send(a.device_states_type, count, ds::SIZEOF as u32);
        }
    }

    /// network_coop.c's `host_send_object_looks`.
    fn send_looks(&self) {
        let m = self.m;
        let a = &self.a;
        let regions = nv::MAXIMUM_REGIONS_PER_OBJECT as u32;
        let mut count: i16 = 0;
        for (object_index, object) in self.objects(u32::MAX) {
            let absolute_index = object_index as i16 as i32;
            if !(0..MAXIMUM_OBJECTS_PER_MAP).contains(&absolute_index) {
                continue;
            }
            let record = a.sent_looks + absolute_index as u32 * sl::SIZEOF as u32;
            let permutations = object + ud::OBJECT_REGION_PERMUTATIONS as u32;
            let scale = m.f32(object + ud::OBJECT_SCALE as u32);
            if m.i32(record + sl::OBJECT_INDEX as u32) != object_index {
                // (an object first seen looks the same on every machine)
                m.w32(record + sl::OBJECT_INDEX as u32, object_index as u32);
                m.w8(record + sl::CHANGED as u32, 0);
                m.set_f32(record + sl::SCALE as u32, scale);
                self.copy(record + sl::REGION_PERMUTATIONS as u32, permutations, regions);
                continue;
            }
            let changing = m.f32(record + sl::SCALE as u32) != scale
                || (0..regions).any(|index| m.r8(record + sl::REGION_PERMUTATIONS as u32 + index) != m.r8(permutations + index));
            if !changing && !(a.refresh != 0 && m.r8(record + sl::CHANGED as u32) != 0) {
                continue;
            }
            m.w8(record + sl::CHANGED as u32, 1);
            m.set_f32(record + sl::SCALE as u32, scale);
            self.copy(record + sl::REGION_PERMUTATIONS as u32, permutations, regions);
            let entry = a.message + HEADER + count as u32 * ol::SIZEOF as u32;
            self.entry_head(entry, object_index, object);
            self.copy(entry + ol::REGION_PERMUTATIONS as u32, permutations, regions);
            m.set_f32(entry + ol::SCALE as u32, scale);
            count += 1;
            if count == MAXIMUM_ENTRIES_PER_MESSAGE {
                self.send(a.looks_type, count, ol::SIZEOF as u32);
                count = 0;
            }
        }
        if count > 0 {
            self.send(a.looks_type, count, ol::SIZEOF as u32);
        }
    }
}

/// The world step of co-op's host sending the world's changes each tick.
pub fn host_world(context: &WorldContext, host: &impl AiHost) {
    let m = host.memory();
    let mut bytes = [0u8; std::mem::size_of::<WorldArguments>()];
    for (index, byte) in bytes.iter_mut().enumerate() {
        *byte = m.r8(context.arguments + index as u32);
    }
    // SAFETY: plain data of its size
    let a: WorldArguments = unsafe { std::mem::transmute(bytes) };
    let world = World { context, host, m, a };
    world.send_transforms();
    world.send_device_states();
    world.send_looks();
}
