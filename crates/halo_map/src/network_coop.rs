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
    coop_events as ce, coop_host_attachment as at, coop_host_looping_sound as ls, coop_host_nav_point as np,
    coop_host_queue as hq, coop_sent_device as sd, coop_sent_look as sl, coop_sent_transform as st, device_datum as dd,
    distributed_coop_device_group as dg,
    distributed_coop_device_state as ds, distributed_coop_event as ev, distributed_coop_object_look as ol, distributed_coop_object_transform as ot,
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

// ---------- the device groups

/// network_coop.c's device group calls (halo_sim.h's
/// `_network_coop_world_call_send_device_groups`): the groups' message, of
/// `b` entries, to every client.
const SEND_DEVICE_GROUPS: i32 = 2;

/// halo_sim.h's `struct halo_sim_network_coop_device_groups`.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct DeviceGroupsArguments {
    pub joined: i32,
    pub host_devices: u32,
    pub device_groups: u32,
    pub scenario_group_count: i32,
    pub network_flags: i32,
    pub runtime_bit: i32,
}

/// network_coop.c's group limits: the groups told apart, those a change
/// goes in, and those each rotation adds a tick.
const MAXIMUM_DEVICE_GROUPS: i32 = 1024;
const MAXIMUM_DEVICE_GROUPS_PER_MESSAGE: i16 = 64;
const DEVICE_GROUP_SENDS: u8 = 3;
const DEVICE_GROUP_REFRESHES_PER_TICK: i32 = 8;
/// devices.c's `struct device_group_datum` (devices.c asserts it): its
/// flags and value.
const GROUP_FLAGS: u32 = 2;
const GROUP_VALUE: u32 = 4;
/// network_coop.c's `_device_group_role_*`.
const ROLE_POWER: u8 = 0;

struct DeviceGroups<'a, H: AiHost> {
    context: &'a WorldContext,
    host: &'a H,
    m: &'a dyn AiMemory,
    a: DeviceGroupsArguments,
}

impl<H: AiHost> DeviceGroups<'_, H> {
    /// devices.c's `device_group_network_get`: the group's value, its flags
    /// a client takes, and whether it is a device's own (made at run time).
    fn group(&self, group_index: i16) -> Option<(f32, u16, bool)> {
        if group_index == NONE as i16 {
            return None;
        }
        let group = crate::ai_actor::datum_get(self.m, self.a.device_groups, group_index as i32)?;
        let flags = self.m.r16(group + GROUP_FLAGS);
        Some((
            self.m.f32(group + GROUP_VALUE),
            flags & self.a.network_flags as u16,
            flags as u32 & (1 << self.a.runtime_bit) != 0,
        ))
    }

    fn entry(&self, index: i16) -> u32 {
        use crate::layout::{coop_host_devices as hd, distributed_coop_device_groups_message as gm};
        self.a.host_devices + hd::MESSAGE as u32 + gm::GROUPS as u32 + index as u32 * dg::SIZEOF as u32
    }

    /// network_coop.c's `device_group_state`: the group's value, flags and
    /// snap count into the entry (none for a group there isn't).
    fn state(&self, group_index: i16, entry: u32) -> bool {
        use crate::layout::coop_host_devices as hd;
        let m = self.m;
        if !(0..MAXIMUM_DEVICE_GROUPS).contains(&(group_index as i32)) {
            return false;
        }
        let Some((value, flags, _)) = self.group(group_index) else { return false };
        m.set_f32(entry + dg::VALUE as u32, value);
        m.w8(entry + dg::FLAGS as u32, flags as u8);
        m.w8(entry + dg::SNAPS as u32, m.r8(self.a.host_devices + hd::SNAP_COUNTS as u32 + group_index as u32));
        m.w8(entry + dg::CHANGED as u32, 0);
        true
    }

    /// network_coop.c's `device_group_entries`: every device group, the
    /// scenario's first, then each device's own; their group indices.
    fn entries(&self) -> Vec<i16> {
        let m = self.m;
        let mut group_indices = Vec::new();
        let mut group_index: i16 = 0;
        while (group_index as i32) < self.a.scenario_group_count && (group_indices.len() as i32) < MAXIMUM_DEVICE_GROUPS {
            let entry = self.entry(group_indices.len() as i16);
            m.set_i16(entry + dg::GROUP_INDEX as u32, group_index);
            m.set_i16(entry + dg::NAME_INDEX as u32, NONE as i16);
            m.w32(entry + dg::OBJECT_INDEX as u32, NONE as u32);
            m.w32(entry + dg::DEFINITION_INDEX as u32, NONE as u32);
            m.w8(entry + dg::ROLE as u32, ROLE_POWER);
            if self.state(group_index, entry) {
                group_indices.push(group_index);
            }
            group_index += 1;
        }
        let data = self.context.object_header_data;
        let size = m.r16(data + DATA_SIZE) as i16 as u32;
        for absolute_index in 0..m.r16(data + DATA_COUNT) as i16 as u32 {
            let header = m.r32(data + DATA_DATA) + absolute_index * size;
            let identifier = m.i16(header);
            if identifier == 0 || (nv::_OBJECT_MASK_DEVICE as u32) & (1 << m.r8(header + oh::TYPE as u32)) == 0 {
                continue;
            }
            let object_index = (identifier as i32) << 16 | absolute_index as i32;
            let device = m.r32(header + oh::DATUM as u32);
            let groups = [m.i16(device + dd::DEVICE_POWER_GROUP_INDEX as u32), m.i16(device + dd::DEVICE_POSITION_GROUP_INDEX as u32)];
            for (role, &group) in groups.iter().enumerate() {
                if group_indices.len() as i32 >= MAXIMUM_DEVICE_GROUPS {
                    break;
                }
                // (scenario groups were listed above)
                if !self.group(group).is_some_and(|(_, _, runtime)| runtime) {
                    continue;
                }
                let entry = self.entry(group_indices.len() as i16);
                m.set_i16(entry + dg::GROUP_INDEX as u32, NONE as i16);
                m.w16(entry + dg::NAME_INDEX as u32, m.r16(device + ud::OBJECT_NAME_INDEX as u32));
                m.w32(entry + dg::OBJECT_INDEX as u32, object_index as u32);
                m.w32(entry + dg::DEFINITION_INDEX as u32, m.r32(device + ud::DEFINITION_INDEX as u32));
                m.w8(entry + dg::ROLE as u32, role as u8);
                if self.state(group, entry) {
                    group_indices.push(group);
                }
            }
        }
        group_indices
    }

    /// network_coop.c's `host_send_device_groups`: the groups that changed
    /// (three times each), and the next few in each rotation: one through
    /// every group, and a quicker one through those moved since the map
    /// loaded (what a late joiner lacks).
    fn send(&self) {
        use crate::layout::coop_host_devices as hd;
        let m = self.m;
        let devices = self.a.host_devices;
        let started = devices + hd::STARTED as u32;
        let values = |group: i16| devices + hd::VALUES as u32 + 4 * group as u32;
        let byte = |offset: usize, group: i16| devices + offset as u32 + group as u32;
        let refresh_next = devices + hd::REFRESH_NEXT as u32;
        let moved_refresh_next = devices + hd::MOVED_REFRESH_NEXT as u32;
        let group_indices = self.entries();
        let count = group_indices.len() as i32;
        let mut moved_count: i32 = 0;
        for (index, &group) in group_indices.iter().enumerate() {
            let entry = self.entry(index as i16);
            let value = m.f32(entry + dg::VALUE as u32);
            let flags = m.r8(entry + dg::FLAGS as u32);
            let snaps = m.r8(entry + dg::SNAPS as u32);
            if m.r8(started) == 0
                || value != m.f32(values(group))
                || flags != m.r8(byte(hd::FLAGS, group))
                || snaps != m.r8(byte(hd::SNAPS, group))
            {
                m.set_f32(values(group), value);
                m.w8(byte(hd::FLAGS, group), flags);
                m.w8(byte(hd::SNAPS, group), snaps);
                // (clients loaded the same map: the starting state isn't sent)
                if m.r8(started) != 0 {
                    m.w8(byte(hd::SENDS, group), DEVICE_GROUP_SENDS);
                    m.w8(byte(hd::MOVED, group), 1);
                }
            }
            if m.r8(byte(hd::MOVED, group)) != 0 {
                moved_count += 1;
            }
        }
        if m.i16(refresh_next) as i32 >= count {
            m.set_i16(refresh_next, 0);
        }
        if m.i16(moved_refresh_next) as i32 >= moved_count {
            m.set_i16(moved_refresh_next, 0);
        }
        let (next, moved_next) = (m.i16(refresh_next) as i32, m.i16(moved_refresh_next) as i32);
        let mut moved_index: i32 = 0;
        let mut sent: i16 = 0;
        for (index, &group) in group_indices.iter().enumerate() {
            let entry = self.entry(index as i16);
            let mut refresh = (index as i32 - next + count) % count < DEVICE_GROUP_REFRESHES_PER_TICK;
            if m.r8(byte(hd::MOVED, group)) != 0 {
                refresh = refresh
                    || self.a.joined != 0
                    || (moved_index - moved_next + moved_count) % moved_count < DEVICE_GROUP_REFRESHES_PER_TICK;
                moved_index += 1;
            }
            let sends = m.r8(byte(hd::SENDS, group));
            let changed = sends != 0;
            m.w8(entry + dg::CHANGED as u32, changed as u8);
            if changed {
                m.w8(byte(hd::SENDS, group), sends - 1);
            } else if !refresh {
                continue;
            }
            let to = self.entry(sent);
            if to != entry {
                for offset in 0..dg::SIZEOF as u32 {
                    m.w8(to + offset, m.r8(entry + offset));
                }
            }
            sent += 1;
        }
        m.w8(started, 1);
        m.set_i16(refresh_next, if count != 0 { ((next + DEVICE_GROUP_REFRESHES_PER_TICK) % count) as i16 } else { 0 });
        m.set_i16(
            moved_refresh_next,
            if moved_count != 0 { ((moved_next + DEVICE_GROUP_REFRESHES_PER_TICK) % moved_count) as i16 } else { 0 },
        );
        // (in messages of at most 64, each moved to the front over those
        // already sent)
        let mut index: i16 = 0;
        while index < sent {
            let message_count = (sent - index).min(MAXIMUM_DEVICE_GROUPS_PER_MESSAGE);
            if index > 0 {
                for offset in 0..message_count as u32 * dg::SIZEOF as u32 {
                    m.w8(self.entry(0) + offset, m.r8(self.entry(index) + offset));
                }
            }
            let mut call = AiCall { kind: SEND_DEVICE_GROUPS, actor: NONE, b: message_count as i32, ..AiCall::default() };
            self.host.call(&mut call);
            index += MAXIMUM_DEVICE_GROUPS_PER_MESSAGE;
        }
    }
}

/// The world step of co-op's host sending its device groups.
pub fn host_device_groups(context: &WorldContext, host: &impl AiHost) {
    let m = host.memory();
    let mut bytes = [0u8; std::mem::size_of::<DeviceGroupsArguments>()];
    for (index, byte) in bytes.iter_mut().enumerate() {
        *byte = m.r8(context.arguments + index as u32);
    }
    // SAFETY: plain data of its size
    let a: DeviceGroupsArguments = unsafe { std::mem::transmute(bytes) };
    DeviceGroups { context, host, m, a }.send();
}

// ---------- the events, and the scripts' state resent

/// network_coop.c's queue calls (halo_sim.h's `_network_coop_world_call_*`).
mod queue_call {
    /// The events' message, of the first `b` queued, to every client.
    pub const SEND_EVENTS: i32 = 3;
    /// Whether the looping sound (`a`, a tag) is no longer playing:
    /// `result`.
    pub const LOOPING_SOUND_STOPPED: i32 = 4;
    /// Whether the structure's breakable surface `a` is broken: `result`,
    /// and its centroid: `output[0..3]`.
    pub const SURFACE_BROKEN: i32 = 5;
    /// Logs that the queue is full (an event of kind `a` not sent).
    pub const LOG_EVENTS_FULL: i32 = 6;
}

/// halo_sim.h's `struct halo_sim_network_coop_queue`.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct QueueArguments {
    pub refresh: i32,
    pub joined: i32,
    pub events: u32,
    pub queue: u32,
    pub structure_bsp_index: i32,
    pub surface_count: i32,
}

/// network_coop.c's `host_send_broken_surfaces`: the broken surfaces
/// resent each refresh.
const SURFACES_PER_REFRESH: i16 = 8;

struct Queue<'a, H: AiHost> {
    context: &'a WorldContext,
    host: &'a H,
    m: &'a dyn AiMemory,
    a: QueueArguments,
}

impl<H: AiHost> Queue<'_, H> {
    fn call(&self, kind: i32, a: i32, b: i32) -> AiCall {
        let mut call = AiCall { kind, actor: NONE, a, b, ..AiCall::default() };
        self.host.call(&mut call);
        call
    }

    /// `object_try_and_get`: the object's datum.
    fn object(&self, object_index: i32) -> Option<u32> {
        crate::ai_actor::datum_get(self.m, self.context.object_header_data, object_index)
            .map(|header| self.m.r32(header + oh::DATUM as u32))
    }

    fn count(&self) -> u32 {
        self.a.events + ce::COUNT as u32
    }

    fn event(&self, index: i16) -> u32 {
        self.a.events + ce::EVENTS as u32 + index as u32 * ev::SIZEOF as u32
    }

    fn send(&self, index: i16) -> u32 {
        self.a.events + ce::SENDS as u32 + 2 * index as u32
    }

    /// network_coop.c's `event_new`: a new event of the kind, to be filled
    /// in (none with the queue full).
    fn event_new(&self, kind: usize) -> Option<u32> {
        let m = self.m;
        let count = m.i16(self.count());
        if count as usize == nv::COOP_MAXIMUM_QUEUED_EVENTS {
            self.call(queue_call::LOG_EVENTS_FULL, kind as i32, 0);
            return None;
        }
        let event = self.event(count);
        for offset in 0..ev::SIZEOF as u32 {
            m.w8(event + offset, 0);
        }
        m.w8(event + ev::KIND as u32, kind as u8);
        let number = m.r16(self.a.events + ce::NEXT_NUMBER as u32).wrapping_add(1);
        m.w16(self.a.events + ce::NEXT_NUMBER as u32, number);
        m.w16(event + ev::NUMBER as u32, number);
        for field in [ev::OBJECT_INDEX, ev::TARGET, ev::TAG_INDEX, ev::DEFINITION_INDEX] {
            m.w32(event + field as u32, NONE as u32);
        }
        m.set_i16(event + ev::NAME_INDEX as u32, NONE as i16);
        m.set_i16(self.send(count), 0);
        m.set_i16(self.count(), count + 1);
        Some(event)
    }

    /// network_coop.c's `send_attach`: an attach, with the markers' indices.
    fn send_attach(&self, kind: usize, parent_index: i32, parent_marker_index: i16, child_index: i32, child_marker_index: i16) {
        let m = self.m;
        let (Some(parent), Some(child)) = (self.object(parent_index), self.object(child_index)) else { return };
        let Some(event) = self.event_new(nv::_COOP_EVENT_ATTACH) else { return };
        m.w8(event + ev::TYPE as u32, kind as u8);
        m.w32(event + ev::OBJECT_INDEX as u32, parent_index as u32);
        m.w16(event + ev::NAME_INDEX as u32, m.r16(parent + ud::OBJECT_NAME_INDEX as u32));
        m.w32(event + ev::DEFINITION_INDEX as u32, m.r32(parent + ud::DEFINITION_INDEX as u32));
        m.w32(event + ev::TARGET as u32, child_index as u32);
        m.w32(event + ev::TAG_INDEX as u32, m.r32(child + ud::DEFINITION_INDEX as u32));
        m.set_f32(event + ev::REALS as u32, m.i16(child + ud::OBJECT_NAME_INDEX as u32) as f32);
        m.set_i16(event + ev::VALUE as u32, parent_marker_index);
        m.set_i16(event + ev::FRAME as u32, child_marker_index);
    }

    /// Removes a record from a list of them, its last put in its place.
    fn remove(&self, records: u32, size: u32, count_at: u32, index: i16) {
        let count = self.m.i16(count_at) - 1;
        self.m.set_i16(count_at, count);
        for offset in 0..size {
            self.m.w8(records + index as u32 * size + offset, self.m.r8(records + count as u32 * size + offset));
        }
    }

    /// network_coop.c's `host_send_attachments`.
    fn attachments(&self) {
        let m = self.m;
        if self.a.refresh == 0 {
            return;
        }
        let records = self.a.queue + hq::ATTACHMENTS as u32;
        let count_at = self.a.queue + hq::ATTACHMENT_COUNT as u32;
        let mut index: i16 = 0;
        while index < m.i16(count_at) {
            let attachment = records + index as u32 * at::SIZEOF as u32;
            let parent_index = m.i32(attachment + at::PARENT_INDEX as u32);
            let child_index = m.i32(attachment + at::CHILD_INDEX as u32);
            // (the child is gone or has a new parent: forget it)
            let child = self.object(child_index);
            if !child.is_some_and(|child| m.i32(child + ud::OBJECT_PARENT_OBJECT_INDEX as u32) == parent_index) {
                self.remove(records, at::SIZEOF as u32, count_at, index);
                continue;
            }
            self.send_attach(
                nv::_COOP_ATTACH,
                parent_index,
                m.i16(attachment + at::PARENT_MARKER_INDEX as u32),
                child_index,
                m.i16(attachment + at::CHILD_MARKER_INDEX as u32),
            );
            index += 1;
        }
    }

    /// network_coop.c's `host_send_looping_sounds`: a looping sound that
    /// stopped without the scripts stopping it is stopped on the clients;
    /// the rest are started again with the resent state.
    fn looping_sounds(&self) {
        let m = self.m;
        let records = self.a.queue + hq::LOOPING_SOUNDS as u32;
        let count_at = self.a.queue + hq::LOOPING_SOUND_COUNT as u32;
        let mut index: i16 = 0;
        while index < m.i16(count_at) {
            let sound = records + index as u32 * ls::SIZEOF as u32;
            let definition_index = m.i32(sound + ls::DEFINITION_INDEX as u32);
            if self.call(queue_call::LOOPING_SOUND_STOPPED, definition_index, 0).result != 0 {
                if let Some(event) = self.event_new(nv::_COOP_EVENT_SOUND) {
                    m.w8(event + ev::TYPE as u32, nv::_COOP_SOUND_LOOPING_STOP as u8);
                    m.w32(event + ev::TAG_INDEX as u32, definition_index as u32);
                }
                self.remove(records, ls::SIZEOF as u32, count_at, index);
                continue;
            }
            if self.a.refresh != 0 {
                if let Some(event) = self.event_new(nv::_COOP_EVENT_SOUND) {
                    m.w8(event + ev::TYPE as u32, nv::_COOP_SOUND_LOOPING_START as u8);
                    m.w32(event + ev::TAG_INDEX as u32, definition_index as u32);
                    m.w32(event + ev::OBJECT_INDEX as u32, m.r32(sound + ls::OBJECT_INDEX as u32));
                    m.set_f32(event + ev::REALS as u32, m.f32(sound + ls::SCALE as u32));
                }
            }
            index += 1;
        }
    }

    /// network_coop.c's `host_send_hud_state`: the nav points active again,
    /// and the objective to a machine that joined.
    fn hud_state(&self) {
        let m = self.m;
        if self.a.refresh == 0 {
            return;
        }
        let objective = m.i16(self.a.queue + hq::HUD_OBJECTIVE as u32);
        if self.a.joined != 0 && objective != NONE as i16 {
            if let Some(event) = self.event_new(nv::_COOP_EVENT_HUD) {
                m.w8(event + ev::TYPE as u32, nv::_COOP_HUD_OBJECTIVE as u8);
                m.set_i16(event + ev::VALUE as u32, objective);
            }
        }
        for index in 0..m.i16(self.a.queue + hq::NAV_POINT_COUNT as u32) {
            let nav_point = self.a.queue + hq::NAV_POINTS as u32 + index as u32 * np::SIZEOF as u32;
            // (network_coop.c's send_nav_point)
            if let Some(event) = self.event_new(nv::_COOP_EVENT_NAV_POINT) {
                m.w8(event + ev::TYPE as u32, m.i16(nav_point + np::KIND as u32) as u8);
                m.w16(event + ev::VALUE as u32, m.r16(nav_point + np::NAV_INDEX as u32));
                m.w32(event + ev::TARGET as u32, m.r32(nav_point + np::TARGET as u32));
                m.w32(event + ev::OBJECT_INDEX as u32, m.r32(nav_point + np::MARKER as u32));
                m.set_f32(event + ev::REALS as u32, m.f32(nav_point + np::VERTICAL_OFFSET as u32));
            }
        }
    }

    /// network_coop.c's `host_send_broken_surfaces`: a few broken surfaces
    /// again each refresh, round them all (`network_coop_note_surface_broken`'s
    /// events, broken from their middle).
    fn broken_surfaces(&self) {
        let m = self.m;
        let count = self.a.surface_count as i16;
        if self.a.refresh == 0 || count <= 0 {
            return;
        }
        let cursor_at = self.a.queue + hq::BROKEN_SURFACE_CURSOR as u32;
        let cursor = m.i16(cursor_at) as i32;
        let mut sent: i16 = 0;
        let mut step: i16 = 0;
        while step < count && sent < SURFACES_PER_REFRESH {
            let index = ((cursor + step as i32) % count as i32) as i16;
            let call = self.call(queue_call::SURFACE_BROKEN, index as i32, 0);
            if call.result != 0 {
                if let Some(event) = self.event_new(nv::_COOP_EVENT_SURFACE_BROKEN) {
                    m.set_i16(event + ev::VALUE as u32, index);
                    m.set_i16(event + ev::FRAME as u32, self.a.structure_bsp_index as i16);
                    for (axis, &value) in call.vector().iter().enumerate() {
                        m.set_f32(event + ev::REALS as u32 + 4 * axis as u32, value);
                    }
                }
                sent += 1;
            }
            step += 1;
        }
        m.set_i16(cursor_at, ((cursor + step as i32) % count as i32) as i16);
    }

    /// network_coop.c's `host_send_events`: the events queued, each kept
    /// for `EVENT_SENDS` ticks' messages.
    fn events(&self) {
        let m = self.m;
        let mut count = m.i16(self.count());
        if count == 0 {
            return;
        }
        self.call(queue_call::SEND_EVENTS, 0, count as i32);
        // (dropping the ones sent their times, the rest kept in order)
        let mut index: i16 = 0;
        while index < count {
            let sends = m.i16(self.send(index)) + 1;
            m.set_i16(self.send(index), sends);
            if (sends as usize) < nv::COOP_EVENT_SENDS {
                index += 1;
                continue;
            }
            count -= 1;
            m.set_i16(self.count(), count);
            for offset in 0..(count - index) as u32 * ev::SIZEOF as u32 {
                m.w8(self.event(index) + offset, m.r8(self.event(index + 1) + offset));
            }
            for offset in 0..(count - index) as u32 * 2 {
                m.w8(self.send(index) + offset, m.r8(self.send(index + 1) + offset));
            }
        }
    }
}

/// The world step of co-op's host resending the scripts' state (what they
/// attached, the looping sounds they started, the HUD state they set, a few
/// broken surfaces) and sending its events each tick.
pub fn host_queue(context: &WorldContext, host: &impl AiHost) {
    let m = host.memory();
    let mut bytes = [0u8; std::mem::size_of::<QueueArguments>()];
    for (index, byte) in bytes.iter_mut().enumerate() {
        *byte = m.r8(context.arguments + index as u32);
    }
    // SAFETY: plain data of its size
    let a: QueueArguments = unsafe { std::mem::transmute(bytes) };
    let queue = Queue { context, host, m, a };
    queue.attachments();
    queue.looping_sounds();
    queue.hud_state();
    queue.broken_surfaces();
    queue.events();
}
