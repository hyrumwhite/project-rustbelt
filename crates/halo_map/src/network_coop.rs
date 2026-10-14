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
    coop_host_queue as hq, coop_host_resend as hr, coop_players_vitality as pv, coop_presentation_inputs as pi,
    coop_player_unit as cpu, coop_resend_inputs as ri, player_datum as pd, coop_sent_device as sd, coop_sent_look as sl,
    coop_client_devices as cd, coop_client_presentation as cp, coop_client_script_sounds as ss,
    coop_client_tick_inputs as ti,
    rasterizer_screen_effect_port_state as se_state, coop_skip_vote as sv, distributed_coop_presentation as dp, distributed_coop_presentation_message as pm, coop_sent_transform as st, device_datum as dd,
    distributed_coop_device_group as dg,
    distributed_coop_device_state as ds, distributed_coop_event as ev, distributed_coop_object_names as on,
    distributed_coop_object_names_message as cn, distributed_coop_screen_effect_message as se, distributed_coop_object_look as ol, distributed_coop_object_transform as ot,
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
const ROLE_POSITION: u8 = 1;

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
        device_group_get(self.m, self.a.device_groups, group_index, self.a.network_flags, self.a.runtime_bit)
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

// ---------- the object names and the screen effect

/// network_coop.c's object names' and screen effect's calls (halo_sim.h's
/// `_network_coop_world_call_*`): their messages to every client.
const SEND_OBJECT_NAMES: i32 = 7;
const SEND_SCREEN_EFFECT: i32 = 8;

/// halo_sim.h's `struct halo_sim_network_coop_object_names`.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct ObjectNamesArguments {
    pub object_name_list: u32,
    pub name_count: i32,
    pub structure_bsp_index: i32,
    pub message: u32,
}

/// halo_sim.h's `struct halo_sim_network_coop_screen_effect`.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct ScreenEffectArguments {
    pub refresh: i32,
    pub state: u32,
    pub sent: u32,
    pub message: u32,
    pub state_size: i32,
}

/// The step's arguments, read from the game's memory.
fn arguments<T: Copy>(context: &WorldContext, m: &dyn AiMemory) -> T {
    let mut bytes = vec![0u8; std::mem::size_of::<T>()];
    for (index, byte) in bytes.iter_mut().enumerate() {
        *byte = m.r8(context.arguments + index as u32);
    }
    // SAFETY: plain data of its size
    unsafe { std::ptr::read_unaligned(bytes.as_ptr() as *const T) }
}

/// The world step of co-op's host sending which scenario object names it
/// has an object of (network_coop.c's `host_send_object_names`): one bit
/// each, for a client to delete those it has and the host hasn't.
pub fn host_object_names(context: &WorldContext, host: &impl AiHost) {
    let m = host.memory();
    let a: ObjectNamesArguments = arguments(context, m);
    let names = a.message + cn::NAMES as u32;
    for offset in 0..on::SIZEOF as u32 {
        m.w8(names + offset, 0);
    }
    m.set_i16(names + on::STRUCTURE_BSP_INDEX as u32, a.structure_bsp_index as i16);
    let count = a.name_count.min(nv::MAXIMUM_OBJECT_NAMES_PER_SCENARIO as i32) as i16;
    m.set_i16(names + on::NAME_COUNT as u32, count);
    for name_index in 0..count.max(0) as u32 {
        // (objects.c's object_index_from_name_index)
        if m.i32(a.object_name_list + 4 * name_index) != NONE {
            let byte = names + on::PRESENT as u32 + name_index / 8;
            m.w8(byte, m.r8(byte) | 1 << (name_index % 8));
        }
    }
    let mut call = AiCall { kind: SEND_OBJECT_NAMES, actor: NONE, ..AiCall::default() };
    host.call(&mut call);
}

/// The world step of co-op's host sending the scripts' screen effect
/// (network_coop.c's `host_send_screen_effect`): when it changed, and with
/// the resent state.
pub fn host_screen_effect(context: &WorldContext, host: &impl AiHost) {
    let m = host.memory();
    let a: ScreenEffectArguments = arguments(context, m);
    let size = a.state_size as u32;
    let state = a.message + se::STATE as u32;
    for offset in 0..size {
        m.w8(state + offset, m.r8(a.state + offset));
    }
    if a.refresh == 0 && (0..size).all(|offset| m.r8(state + offset) == m.r8(a.sent + offset)) {
        return;
    }
    for offset in 0..size {
        m.w8(a.sent + offset, m.r8(state + offset));
    }
    let mut call = AiCall { kind: SEND_SCREEN_EFFECT, actor: NONE, ..AiCall::default() };
    host.call(&mut call);
}

// ---------- the skip vote and the presentation

/// network_coop.c's presentation calls (halo_sim.h's
/// `_network_coop_world_call_*`).
mod presentation_call {
    /// Logs that the cutscene can be skipped.
    pub const LOG_SKIPPABLE: i32 = 9;
    /// Skips the cinematic, `a` of `b` machines voting.
    pub const SKIP_CINEMATIC: i32 = 10;
    /// The presentation's message to every client.
    pub const SEND_PRESENTATION: i32 = 11;
}

/// halo_sim.h's `struct halo_sim_network_coop_presentation`.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct PresentationArguments {
    pub inputs: u32,
    pub skip_vote: u32,
    pub message: u32,
    pub now: i32,
}

/// network_coop.c's `SKIP_VOTE_HELD_TICKS`: the host forgets a client's
/// skip vote it hasn't heard again for this long (a second).
const SKIP_VOTE_HELD_TICKS: i32 = 30;
/// network_coop.c's `FIELD_OF_VIEW_SCALE`: the field of view (radians) is
/// sent as a word, scaled by this.
const FIELD_OF_VIEW_SCALE: f32 = 10000.0;

struct Presentation<'a, H: AiHost> {
    context: &'a WorldContext,
    host: &'a H,
    m: &'a dyn AiMemory,
    a: PresentationArguments,
}

impl<H: AiHost> Presentation<'_, H> {
    fn call(&self, kind: i32, a: i32, b: i32) {
        let mut call = AiCall { kind, actor: NONE, a, b, ..AiCall::default() };
        self.host.call(&mut call);
    }

    fn input(&self, offset: usize) -> u32 {
        self.a.inputs + offset as u32
    }

    fn vote(&self, offset: usize) -> u32 {
        self.a.skip_vote + offset as u32
    }

    /// network_coop.c's `skip_vote_clear`.
    fn skip_vote_clear(&self) {
        let m = self.m;
        m.w8(self.vote(sv::VOTED), 0);
        m.set_i16(self.vote(sv::VOTES), 0);
        for index in 0..nv::HALO_PORT_MAXIMUM_NETWORK_MACHINES as u32 {
            m.w32(self.vote(sv::CLIENT_HEARD_TIMES) + 4 * index, NONE as u32);
            m.w32(self.vote(sv::CLIENT_VOTE_TIMES) + 4 * index, NONE as u32);
        }
    }

    /// network_coop.c's `host_count_skip_votes`: skips the cinematic when
    /// more than half the machines showing it have voted (one still loading,
    /// or joined since it began, has no say until it shows it).
    fn count_skip_votes(&self) {
        let m = self.m;
        let now = self.a.now;
        let skippable = m.r8(self.input(pi::SKIPPABLE)) != 0;
        let was_offered = m.r8(self.vote(sv::OFFERED)) != 0;
        // (the save the script makes as the cutscene becomes skippable is
        // waited for: reverting earlier would go back to the one before)
        if !skippable {
            m.w8(self.vote(sv::SKIP_SAVE_WRITTEN), 0);
        } else if m.r8(self.input(pi::SAVING_MAP)) == 0 {
            m.w8(self.vote(sv::SKIP_SAVE_WRITTEN), 1);
        }
        let offered = skippable && m.r8(self.vote(sv::SKIP_SAVE_WRITTEN)) != 0 && now >= m.i32(self.vote(sv::COOLDOWN_UNTIL));
        m.w8(self.vote(sv::OFFERED), offered as u8);
        if offered && !was_offered {
            self.call(presentation_call::LOG_SKIPPABLE, 0, 0);
        }
        if !offered {
            self.skip_vote_clear();
            m.set_i16(self.vote(sv::VOTERS), 0);
            m.w8(self.vote(sv::REQUESTED), 0);
            return;
        }
        // (already requested: main.c skips at the end of this frame)
        if m.r8(self.vote(sv::REQUESTED)) != 0 {
            return;
        }
        // (the host votes too, unless it is a dedicated server with no player)
        let mut voters = (m.r8(self.input(pi::LOCAL_PLAYER)) != 0) as i16;
        let mut votes = (m.r8(self.vote(sv::VOTED)) != 0) as i16;
        for index in 0..m.i16(self.input(pi::MACHINE_COUNT)).max(0) as u32 {
            let machine_index = m.i32(self.input(pi::MACHINE_INDICES) + 4 * index) as u32;
            let heard_time = m.i32(self.vote(sv::CLIENT_HEARD_TIMES) + 4 * machine_index);
            let vote_time = m.i32(self.vote(sv::CLIENT_VOTE_TIMES) + 4 * machine_index);
            if heard_time == NONE || now - heard_time > SKIP_VOTE_HELD_TICKS {
                continue;
            }
            voters += 1;
            if vote_time != NONE && now - vote_time <= SKIP_VOTE_HELD_TICKS {
                votes += 1;
            }
        }
        m.set_i16(self.vote(sv::VOTERS), voters);
        m.set_i16(self.vote(sv::VOTES), votes);
        if votes as i32 * 2 > voters as i32 {
            m.w8(self.vote(sv::REQUESTED), 1);
            self.call(presentation_call::SKIP_CINEMATIC, votes as i32, voters as i32);
        }
    }

    fn vector(&self, offset: usize) -> [f32; 3] {
        let at = self.input(offset);
        [self.m.f32(at), self.m.f32(at + 4), self.m.f32(at + 8)]
    }

    /// network_coop.c's `host_presentation`: what the host shows, from what
    /// was gathered of its game.
    fn presentation(&self) {
        let m = self.m;
        let p = self.a.message + pm::PRESENTATION as u32;
        let flag = |bit: usize, on: bool| {
            if on {
                m.w8(p + dp::FLAGS as u32, m.r8(p + dp::FLAGS as u32) | 1 << bit);
            }
        };
        let input = |offset: usize| m.r8(self.input(offset)) != 0;
        for offset in 0..dp::SIZEOF as u32 {
            m.w8(p + offset, 0);
        }
        flag(nv::_PRESENTATION_CINEMATIC_BIT, input(pi::CINEMATIC));
        flag(nv::_PRESENTATION_LETTERBOX_BIT, input(pi::LETTERBOX));
        m.w16(p + dp::FADE_TICKS as u32, m.r16(self.input(pi::FADE_TICKS)));
        let fade_start_time = m.i32(self.input(pi::FADE_START_TIME));
        m.w32(p + dp::FADE_START_TIME as u32, fade_start_time as u32);
        flag(nv::_PRESENTATION_FADING_OUT_BIT, input(pi::FADING_OUT));
        for (index, part) in self.vector(pi::FADE_COLOR).into_iter().enumerate() {
            m.w8(p + dp::FADE_COLOR as u32 + index as u32, (part.clamp(0.0, 1.0) * 255.0 + 0.5) as u8);
        }
        let elapsed = self.a.now.wrapping_sub(fade_start_time);
        m.set_i16(p + dp::FADE_ELAPSED as u32, elapsed.clamp(0, i16::MAX as i32) as i16);
        if input(pi::CAMERA_VALID) {
            let position = self.vector(pi::CAMERA_POSITION);
            for (index, part) in position.iter().enumerate() {
                m.set_f32(p + dp::CAMERA_POSITION as u32 + 4 * index as u32, *part);
            }
            for (from, to) in [(pi::CAMERA_FORWARD, dp::CAMERA_FORWARD), (pi::CAMERA_UP, dp::CAMERA_UP)] {
                for (index, part) in wire::vector_pack(self.vector(from), UNIT_SCALE).into_iter().enumerate() {
                    m.set_i16(p + to as u32 + 2 * index as u32, part);
                }
            }
            let field_of_view = m.f32(self.input(pi::CAMERA_FIELD_OF_VIEW)) * FIELD_OF_VIEW_SCALE + 0.5;
            m.w16(p + dp::CAMERA_FIELD_OF_VIEW as u32, field_of_view.clamp(1.0, u16::MAX as f32) as u16);
            let object_index = m.i32(self.input(pi::CAMERA_OBJECT_INDEX));
            m.w32(p + dp::CAMERA_OBJECT_INDEX as u32, object_index as u32);
            if object_index != NONE {
                if let Some(header) = crate::ai_actor::datum_get(m, self.context.object_header_data, object_index) {
                    let object = m.r32(header + oh::DATUM as u32) + ud::OBJECT_POSITION as u32;
                    for (index, part) in position.iter().enumerate() {
                        m.set_f32(p + dp::CAMERA_OBJECT_OFFSET as u32 + 4 * index as u32, part - m.f32(object + 4 * index as u32));
                    }
                }
            }
        } else {
            m.w32(p + dp::CAMERA_OBJECT_INDEX as u32, NONE as u32);
        }
        m.w8(p + dp::CAMERA_SCRIPTED as u32, input(pi::CAMERA_SCRIPTED) as u8);
        m.w8(p + dp::INPUT_DISABLED as u32, input(pi::INPUT_DISABLED) as u8);
        m.w8(p + dp::SCRIPTED_SHAKE as u32, input(pi::SCRIPTED_SHAKE) as u8);
        m.w8(p + dp::PLAYERS_VITALITY_SET as u32, input(pi::PLAYERS_VITALITY_SET) as u8);
        m.set_i16(p + dp::ACTIVATING_CLUSTER as u32, m.i16(self.input(pi::ACTIVATING_CLUSTER)).wrapping_add(1));
        for (from, to) in [
            (pi::PLAYERS_MAXIMUM_BODY_VITALITY, dp::PLAYERS_MAXIMUM_BODY_VITALITY),
            (pi::PLAYERS_MAXIMUM_SHIELD_VITALITY, dp::PLAYERS_MAXIMUM_SHIELD_VITALITY),
            (pi::UNIT_HUD_FLAGS, dp::UNIT_HUD_FLAGS),
            (pi::WEAPON_HUD_FLAGS, dp::WEAPON_HUD_FLAGS),
            (pi::TIMER_REFERENCE_TIME, dp::TIMER_REFERENCE_TIME),
        ] {
            m.w32(p + to as u32, m.r32(self.input(from)));
        }
        for index in 0..nv::GAME_ALLEGIANCE_BITVECTOR_LONGS as u32 {
            m.w32(p + dp::ALLY_TEAMS as u32 + 4 * index, m.r32(self.input(pi::ALLY_TEAMS) + 4 * index));
            m.w32(p + dp::FRIENDLY_TEAMS as u32 + 4 * index, m.r32(self.input(pi::FRIENDLY_TEAMS) + 4 * index));
        }
        flag(nv::_PRESENTATION_SKIPPABLE_BIT, m.r8(self.vote(sv::OFFERED)) != 0);
        m.w8(p + dp::SKIP_VOTES as u32, m.i16(self.vote(sv::VOTES)).min(255) as u8);
        m.w8(p + dp::SKIP_VOTERS as u32, m.i16(self.vote(sv::VOTERS)).min(255) as u8);
        flag(nv::_PRESENTATION_SHOW_HUD_BIT, input(pi::SHOW_HUD));
        flag(nv::_PRESENTATION_SHOW_HELP_TEXT_BIT, input(pi::SHOW_HELP_TEXT));
        flag(nv::_PRESENTATION_TIMER_PAUSED_BIT, input(pi::TIMER_PAUSED));
        flag(nv::_PRESENTATION_TIMER_ENABLED_BIT, input(pi::TIMER_ENABLED));
        for (from, to) in [
            (pi::TIMER_TICKS, dp::TIMER_TICKS),
            (pi::TIMER_FLASH_CUTOFF, dp::TIMER_FLASH_CUTOFF),
            (pi::TIMER_X, dp::TIMER_X),
            (pi::TIMER_Y, dp::TIMER_Y),
            (pi::TIMER_CORNER, dp::TIMER_CORNER),
        ] {
            m.w16(p + to as u32, m.r16(self.input(from)));
        }
    }
}

/// The world step of co-op's host counting the cutscene skip vote and
/// sending its presentation each tick.
pub fn host_presentation(context: &WorldContext, host: &impl AiHost) {
    let m = host.memory();
    let a: PresentationArguments = arguments(context, m);
    let presentation = Presentation { context, host, m, a };
    presentation.count_skip_votes();
    presentation.presentation();
    presentation.call(presentation_call::SEND_PRESENTATION, 0, 0);
}

// ---------- the resend and the players' vitality

/// network_coop.c's resend and vitality calls (halo_sim.h's
/// `_network_coop_world_call_*`).
mod resend_call {
    /// Logs that a machine joined.
    pub const LOG_JOINED: i32 = 12;
    /// Sets the unit's (`a`) maximum vitality to the players'.
    pub const INITIALIZE_VITALITY: i32 = 13;
}

/// halo_sim.h's `struct halo_sim_network_coop_resend`.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct ResendArguments {
    pub inputs: u32,
    pub resend: u32,
    pub vitality: u32,
}

/// network_coop.c's `OBJECT_REFRESH_TICKS`: how often the host sends again
/// what a client that joined since, or missed a message, would lack.
const OBJECT_REFRESH_TICKS: i32 = 60;

/// The world step of co-op's host noticing a machine joining and keeping
/// its players' units at the scripts' vitality, each tick.
pub fn host_resend(context: &WorldContext, host: &impl AiHost) {
    let m = host.memory();
    let a: ResendArguments = arguments(context, m);
    let call = |kind: i32, a: i32| {
        let mut call = AiCall { kind, actor: NONE, a, ..AiCall::default() };
        host.call(&mut call);
    };

    // network_coop.c's host_resend_update: whether a client machine joined
    // since the last tick, and so whether to resend this tick
    let count = m.i16(a.inputs + ri::MACHINE_COUNT as u32).max(0) as u32;
    let known_count = m.i16(a.resend + hr::MACHINE_COUNT as u32).max(0) as u32;
    let machine = |base: u32, index: u32| m.i32(base + 4 * index);
    let mut joined = false;
    for index in 0..count {
        if joined {
            break;
        }
        let id = machine(a.inputs + ri::MACHINES as u32, index);
        joined = !(0..known_count).any(|known| machine(a.resend + hr::MACHINES as u32, known) == id);
    }
    for index in 0..count {
        m.w32(a.resend + hr::MACHINES as u32 + 4 * index, machine(a.inputs + ri::MACHINES as u32, index) as u32);
    }
    m.set_i16(a.resend + hr::MACHINE_COUNT as u32, count as i16);
    m.w8(a.resend + hr::JOINED as u32, joined as u8);
    m.w8(a.resend + hr::REFRESH as u32, (joined || context.game_time % OBJECT_REFRESH_TICKS == 0) as u8);
    if joined {
        call(resend_call::LOG_JOINED, 0);
    }

    // network_coop.c's players_vitality_keep: every player's unit at the
    // maximum vitality the scripts set (the units as gathered before the
    // step: its calls change them)
    if m.r8(a.vitality + pv::SET as u32) == 0 {
        return;
    }
    let body = m.f32(a.vitality + pv::MAXIMUM_BODY as u32);
    let shield = m.f32(a.vitality + pv::MAXIMUM_SHIELD as u32);
    for player_index in 0..m.i16(a.inputs + ri::PLAYER_COUNT as u32).max(0) as u32 {
        let entry = a.inputs + ri::PLAYER_UNITS as u32 + player_index * cpu::SIZEOF as u32;
        let unit_index = m.i32(entry + cpu::UNIT_INDEX as u32);
        if unit_index == NONE
            || (m.f32(entry + cpu::MAXIMUM_BODY as u32) == body && m.f32(entry + cpu::MAXIMUM_SHIELD as u32) == shield)
        {
            continue;
        }
        call(resend_call::INITIALIZE_VITALITY, unit_index);
    }
}

// ---------- a client: the host's world changes

/// network_coop.c's client world calls (halo_sim.h's
/// `_network_coop_client_call_*`).
pub mod client_call {
    /// The device (`actor`) set to the power `input[0]`, and moved to the
    /// position `input[1]` if `a`.
    pub const DEVICE_STATE: i32 = 1;
    /// The object (`actor`) placed at `input[0..3]` facing `input[3..6]`
    /// with up `input[6..9]`.
    pub const SET_POSITION: i32 = 2;
    /// The object's (`actor`) region `a`'s permutation set to `b`.
    pub const SET_PERMUTATION: i32 = 3;
    /// The object (`actor`) scaled to `input[0]`.
    pub const SET_SCALE: i32 = 4;
}

/// halo_sim.h's `_coop_client_world_*`: which of the host's world changes.
const CLIENT_DEVICE_STATES: i16 = 0;
const CLIENT_OBJECT_TRANSFORMS: i16 = 1;
const CLIENT_OBJECT_LOOKS: i16 = 2;

/// halo_sim.h's `struct halo_sim_network_coop_client_world`.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct ClientWorldArguments {
    pub kind: i16,
    pub count: i16,
    pub entries: u32,
    pub object_name_list: u32,
    pub tag_instances: u32,
    pub tag_count: i32,
}

/// network_coop.c's `DEVICE_POSITION_TOLERANCE`: a client's devices are
/// moved to the host's position when this far from it (or still and off it
/// at all).
const DEVICE_POSITION_TOLERANCE: f32 = 0.03;
/// network_distributed.h's `UNIT_WORLD_BOUND`.
const UNIT_WORLD_BOUND: f32 = 32768.0;
/// objects.h's `MAXIMUM_OBJECT_NAMES_PER_SCENARIO` (scenario.h).
const MAXIMUM_OBJECT_NAMES: i16 = nv::MAXIMUM_OBJECT_NAMES_PER_SCENARIO as i16;

struct ClientWorld<'a, H: AiHost> {
    context: &'a WorldContext,
    host: &'a H,
    m: &'a dyn AiMemory,
    a: ClientWorldArguments,
}

impl<H: AiHost> ClientWorld<'_, H> {
    fn call(&self, kind: i32, actor: i32, a: i32, b: i32, input: &[f32]) {
        let mut call = AiCall { kind, actor, a, b, ..AiCall::default() };
        call.input[..input.len()].copy_from_slice(input);
        self.host.call(&mut call);
    }

    /// `object_try_and_get_and_verify_type`: the object's datum, if it is of
    /// a type in the mask.
    fn typed(&self, object_index: i32, mask: u32) -> Option<u32> {
        let header = crate::ai_actor::datum_get(self.m, self.context.object_header_data, object_index)?;
        (mask & (1 << self.m.r8(header + oh::TYPE as u32)) != 0).then(|| self.m.r32(header + oh::DATUM as u32))
    }

    /// network_coop.c's `object_find`: the local copy of an object the host
    /// named, of a type in the mask (NONE for none). An unnamed object sits
    /// at the same object index on every machine; if its salt differs here,
    /// matching the absolute index is enough, as long as the tag matches.
    fn object_find(&self, name_index: i16, object_index: i32, definition_index: i32, mask: u32) -> Option<(i32, u32)> {
        let m = self.m;
        let mut object_index = object_index;
        if name_index != NONE as i16 {
            // (objects.c's object_index_from_name_index)
            object_index = if (0..MAXIMUM_OBJECT_NAMES).contains(&name_index) {
                m.i32(self.a.object_name_list + 4 * name_index as u32)
            } else {
                NONE
            };
        }
        if object_index == NONE {
            return None;
        }
        let mut object = self.typed(object_index, mask);
        if object.is_none() && name_index == NONE as i16 {
            // (the object of the types at that absolute index, whatever its
            // salt: object_iterator_next's)
            let data = self.context.object_header_data;
            let absolute_index = object_index as i16;
            if absolute_index >= 0 && absolute_index < m.r16(data + DATA_COUNT) as i16 {
                let size = m.r16(data + DATA_SIZE) as i16 as u32;
                let header = m.r32(data + DATA_DATA) + absolute_index as u32 * size;
                let identifier = m.i16(header);
                if identifier != 0 && mask & (1 << m.r8(header + oh::TYPE as u32)) != 0 {
                    object = Some(m.r32(header + oh::DATUM as u32));
                    object_index = (identifier as i32) << 16 | absolute_index as i32;
                }
            }
        }
        let object = object?;
        if m.i32(object + ud::DEFINITION_INDEX as u32) != definition_index
            || (name_index == NONE as i16 && m.i16(object + ud::OBJECT_NAME_INDEX as u32) != NONE as i16)
        {
            return None;
        }
        Some((object_index, object))
    }

    fn entry(&self, index: i16, size: usize) -> u32 {
        self.a.entries + index as u32 * size as u32
    }

    /// network_coop.c's `network_coop_handle_device_states`: each device
    /// where the host's is (moved there if off it), at its power.
    fn device_states(&self) {
        let m = self.m;
        for index in 0..self.a.count {
            let state = self.entry(index, ds::SIZEOF);
            let found = self.object_find(
                m.i16(state + ds::NAME_INDEX as u32),
                m.i32(state + ds::OBJECT_INDEX as u32),
                m.i32(state + ds::DEFINITION_INDEX as u32),
                nv::_OBJECT_MASK_DEVICE as u32,
            );
            let position = m.f32(state + ds::POSITION as u32);
            let power = m.f32(state + ds::POWER as u32);
            let Some((device_index, device)) = found else { continue };
            if !wire::real_valid(position) || !wire::real_valid(power) {
                continue;
            }
            let difference = (m.f32(device + dd::DEVICE_POSITION as u32) - position).abs();
            let moving = m.r8(state + ds::MOVING as u32) != 0;
            let snap = (!moving && difference > 0.001) || difference > DEVICE_POSITION_TOLERANCE;
            self.call(client_call::DEVICE_STATE, device_index, snap as i32, 0, &[power, position]);
        }
    }

    /// network_coop.c's `network_coop_handle_object_transforms`: each
    /// scenery and machine where the host's is.
    fn object_transforms(&self) {
        let m = self.m;
        for index in 0..self.a.count {
            let transform = self.entry(index, ot::SIZEOF);
            let Some((object_index, object)) = self.object_find(
                m.i16(transform + ot::NAME_INDEX as u32),
                m.i32(transform + ot::OBJECT_INDEX as u32),
                m.i32(transform + ot::DEFINITION_INDEX as u32),
                COOP_MOVED_OBJECTS,
            ) else {
                continue;
            };
            if m.i32(object + ud::OBJECT_PARENT_OBJECT_INDEX as u32) != NONE {
                continue;
            }
            let vector = |at: u32| [m.i16(at), m.i16(at + 2), m.i16(at + 4)];
            let forward = wire::vector_unpack(vector(transform + ot::FORWARD as u32), UNIT_SCALE);
            let up = wire::vector_unpack(vector(transform + ot::UP as u32), UNIT_SCALE);
            let at = transform + ot::POSITION as u32;
            let position = [m.f32(at), m.f32(at + 4), m.f32(at + 8)];
            let mut axes = [forward[0], forward[1], forward[2], up[0], up[1], up[2]];
            if wire::point_valid(position, UNIT_WORLD_BOUND) && wire::axes_make_valid(&mut axes) {
                let mut input = [0.0; 9];
                input[..3].copy_from_slice(&position);
                input[3..].copy_from_slice(&axes);
                self.call(client_call::SET_POSITION, object_index, 0, 0, &input);
            }
        }
    }

    /// network_coop.c's `network_coop_handle_object_looks`: each object's
    /// permutations (only ones its model has, or none) and scale as the
    /// host's.
    fn object_looks(&self) {
        let m = self.m;
        let tags = crate::ai_actor::AiStepContext {
            tag_instances: self.a.tag_instances,
            tag_count: self.a.tag_count,
            ..crate::ai_actor::AiStepContext::default()
        };
        for index in 0..self.a.count {
            let look = self.entry(index, ol::SIZEOF);
            let Some((object_index, object)) = self.object_find(
                m.i16(look + ol::NAME_INDEX as u32),
                m.i32(look + ol::OBJECT_INDEX as u32),
                m.i32(look + ol::DEFINITION_INDEX as u32),
                u32::MAX,
            ) else {
                continue;
            };
            let definition_index = m.i32(object + ud::DEFINITION_INDEX as u32);
            let model_index = crate::sensing::tag_get(m, &tags, crate::fourcc(b"obje"), definition_index)
                .map_or(0, |definition| m.i32(definition + crate::layout::object_definition::OBJECT_MODEL as u32 + 0xc));
            let model = if model_index != NONE { crate::sensing::tag_get(m, &tags, crate::fourcc(b"mode"), model_index) } else { None };
            if let Some(model) = model {
                let regions = model + crate::layout::model::REGIONS as u32;
                let count = crate::ai_actor::tag_block_count(m, regions).min(nv::MAXIMUM_REGIONS_PER_OBJECT as i32);
                for region_index in 0..count {
                    let region = crate::ai_actor::tag_block_element(m, regions, region_index, crate::layout::model_region::SIZEOF);
                    let permutation_index = m.r8(look + ol::REGION_PERMUTATIONS as u32 + region_index as u32);
                    if permutation_index == 0xff
                        || (permutation_index as i32) < crate::ai_actor::tag_block_count(m, region + crate::layout::model_region::PERMUTATIONS as u32)
                    {
                        self.call(client_call::SET_PERMUTATION, object_index, region_index, permutation_index as i32, &[]);
                    }
                }
            }
            let scale = m.f32(look + ol::SCALE as u32);
            if wire::real_valid(scale) && scale > 0.0 && scale < 100.0 && scale != m.f32(object + ud::OBJECT_SCALE as u32) {
                self.call(client_call::SET_SCALE, object_index, 0, 0, &[scale]);
            }
        }
    }
}

/// The world step of a co-op client applying its host's world changes: a
/// message of device states, scenery transforms or looks.
pub fn client_world(context: &WorldContext, host: &impl AiHost) {
    let m = host.memory();
    let a: ClientWorldArguments = arguments(context, m);
    let world = ClientWorld { context, host, m, a };
    match a.kind {
        CLIENT_DEVICE_STATES => world.device_states(),
        CLIENT_OBJECT_TRANSFORMS => world.object_transforms(),
        CLIENT_OBJECT_LOOKS => world.object_looks(),
        _ => {}
    }
}

// ---------- a client: the host's device groups

/// halo_sim.h's `_network_coop_client_call_set_device_group`: the device
/// group (`actor`) set to `input[0]` with the flags `a`, jumping there if
/// `b`.
const SET_DEVICE_GROUP: i32 = 5;

/// halo_sim.h's `struct halo_sim_network_coop_client_devices`.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct ClientDevicesArguments {
    pub count: i32,
    pub entries: u32,
    pub client_devices: u32,
    pub device_groups: u32,
    pub scenario_group_count: i32,
    pub network_flags: i32,
    pub runtime_bit: i32,
    pub object_name_list: u32,
}

/// devices.c's `device_group_network_get`: the group's value, its flags a
/// client takes, and whether it is a device's own (made at run time).
fn device_group_get(m: &dyn AiMemory, device_groups: u32, group_index: i16, network_flags: i32, runtime_bit: i32) -> Option<(f32, u16, bool)> {
    if group_index == NONE as i16 {
        return None;
    }
    let group = crate::ai_actor::datum_get(m, device_groups, group_index as i32)?;
    let flags = m.r16(group + GROUP_FLAGS);
    Some((m.f32(group + GROUP_VALUE), flags & network_flags as u16, flags as u32 & (1 << runtime_bit) != 0))
}

/// The world step of a co-op client applying its host's device groups:
/// network_coop.c's `network_coop_handle_device_groups`, with
/// `device_group_find`.
pub fn client_devices(context: &WorldContext, host: &impl AiHost) {
    let m = host.memory();
    let a: ClientDevicesArguments = arguments(context, m);
    let get = |group_index: i16| device_group_get(m, a.device_groups, group_index, a.network_flags, a.runtime_bit);
    let world = ClientWorld {
        context,
        host,
        m,
        a: ClientWorldArguments { kind: NONE as i16, count: 0, entries: 0, object_name_list: a.object_name_list, tag_instances: 0, tag_count: 0 },
    };
    let in_range = |group_index: i16| (0..MAXIMUM_DEVICE_GROUPS).contains(&(group_index as i32));
    for index in 0..a.count.max(0) as u32 {
        let entry = a.entries + index * dg::SIZEOF as u32;
        // (network_coop.c's device_group_find: the local group the host's
        // entry is, a scenario's by its index, a device's own by the device
        // and its role)
        let entry_group = m.i16(entry + dg::GROUP_INDEX as u32);
        let group_index = if entry_group != NONE as i16 {
            (entry_group >= 0
                && (entry_group as i32) < a.scenario_group_count
                && in_range(entry_group)
                && get(entry_group).is_some_and(|(_, _, runtime)| !runtime))
            .then_some(entry_group)
        } else {
            world
                .object_find(
                    m.i16(entry + dg::NAME_INDEX as u32),
                    m.i32(entry + dg::OBJECT_INDEX as u32),
                    m.i32(entry + dg::DEFINITION_INDEX as u32),
                    nv::_OBJECT_MASK_DEVICE as u32,
                )
                .and_then(|(_, device)| match m.r8(entry + dg::ROLE as u32) {
                    ROLE_POWER => Some(m.i16(device + dd::DEVICE_POWER_GROUP_INDEX as u32)),
                    ROLE_POSITION => Some(m.i16(device + dd::DEVICE_POSITION_GROUP_INDEX as u32)),
                    _ => None,
                })
                .filter(|&group| in_range(group) && get(group).is_some_and(|(_, _, runtime)| runtime))
        };
        let Some(group_index) = group_index else { continue };
        let Some((here, flags, _)) = get(group_index) else { continue };
        let value = m.f32(entry + dg::VALUE as u32);
        if !wire::real_valid(value) {
            continue;
        }
        let value = value.clamp(0.0, 1.0);
        let snaps_at = a.client_devices + cd::SNAPS as u32 + group_index as u32;
        let seen_at = a.client_devices + cd::SEEN as u32 + group_index as u32;
        let entry_snaps = m.r8(entry + dg::SNAPS as u32);
        let entry_flags = m.r8(entry + dg::FLAGS as u32);
        // (jumping straight to the value when the host did, or when a
        // rotation's entry disagrees: a change missed, or a late join; a
        // change's entry animates, as it did on the host)
        let snap = (m.r8(seen_at) != 0 && entry_snaps != m.r8(snaps_at)) || (m.r8(entry + dg::CHANGED as u32) == 0 && value != here);
        if snap || value != here || entry_flags != flags as u8 {
            let mut call = AiCall { kind: SET_DEVICE_GROUP, actor: group_index as i32, a: entry_flags as i32, b: snap as i32, ..AiCall::default() };
            call.input[0] = value;
            host.call(&mut call);
        }
        m.w8(snaps_at, entry_snaps);
        m.w8(seen_at, 1);
    }
}

// ---------- a client: the host's object names

/// halo_sim.h's `_network_coop_client_call_create_named` and `_delete`:
/// the object of the scenario's name `actor` made, the object `actor`
/// deleted.
const CREATE_NAMED: i32 = 6;
const DELETE: i32 = 7;

/// halo_sim.h's `struct halo_sim_network_coop_client_names`.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct ClientNamesArguments {
    pub names: u32,
    pub differing: u32,
    pub object_name_list: u32,
    pub scenario_names: u32,
    pub scenario_name_count: i32,
    pub structure_bsp_index: i32,
}

/// scenario_definitions.h's `struct scenario_object_name`: its size and
/// its runtime object type.
const SCENARIO_OBJECT_NAME_SIZE: u32 = 36;
const SCENARIO_OBJECT_NAME_TYPE: u32 = 32;
/// network_coop.c's `NETWORKED_OBJECT_TYPES`: the types network_objects.c
/// syncs, which the names leave alone.
const NETWORKED_OBJECT_TYPES: u32 =
    (nv::_OBJECT_MASK_BIPED | nv::_OBJECT_MASK_VEHICLE | nv::_OBJECT_MASK_WEAPON | nv::_OBJECT_MASK_EQUIPMENT) as u32;

/// The world step of a co-op client creating or deleting its named scenery
/// and devices to match its host's: network_coop.c's
/// `network_coop_handle_object_names`. A difference must show up twice in a
/// row, on the same BSP, so a message crossing a BSP switch or a script's
/// create can't cause a false one.
pub fn client_names(context: &WorldContext, host: &impl AiHost) {
    let m = host.memory();
    let a: ClientNamesArguments = arguments(context, m);
    let structure_bsp_index = m.i16(a.names + on::STRUCTURE_BSP_INDEX as u32);
    let name_count = m.i16(a.names + on::NAME_COUNT as u32);
    let bytes = nv::COOP_OBJECT_NAME_BYTES as u32;
    if structure_bsp_index as i32 != a.structure_bsp_index
        || name_count as i32 != a.scenario_name_count.min(nv::MAXIMUM_OBJECT_NAMES_PER_SCENARIO as i32)
    {
        for index in 0..bytes {
            m.w8(a.differing + index, 0);
        }
        return;
    }
    // network_coop.c's object_holds_player: a player's unit, or carrying one
    // (scripts never delete those)
    fn holds_player(m: &dyn AiMemory, context: &WorldContext, object_index: i32) -> bool {
        // (players.c's player_index_from_unit_index)
        let data = context.player_data;
        let size = m.r16(data + DATA_SIZE) as i16 as u32;
        for player_index in 0..m.r16(data + DATA_COUNT) as i16 as u32 {
            let player = m.r32(data + DATA_DATA) + player_index * size;
            if m.i16(player + pd::IDENTIFIER as u32) != 0 && m.i32(player + pd::UNIT_INDEX as u32) == object_index {
                return true;
            }
        }
        let object = |index: i32| {
            crate::ai_actor::datum_get(m, context.object_header_data, index).map(|header| m.r32(header + oh::DATUM as u32))
        };
        let mut child_index = object(object_index).map_or(NONE, |o| m.i32(o + ud::OBJECT_FIRST_CHILD_OBJECT_INDEX as u32));
        while child_index != NONE {
            if holds_player(m, context, child_index) {
                return true;
            }
            child_index = object(child_index).map_or(NONE, |o| m.i32(o + ud::OBJECT_NEXT_OBJECT_INDEX as u32));
        }
        false
    }
    for name_index in 0..name_count.max(0) as u32 {
        let differing = a.differing + name_index / 8;
        let bit = 1u8 << (name_index % 8);
        let host_has = m.r8(a.names + on::PRESENT as u32 + name_index / 8) & bit != 0;
        // (objects.c's object_index_from_name_index; network_coop.c's
        // object_name_type)
        let object_index = m.i32(a.object_name_list + 4 * name_index);
        let kind = if (name_index as i32) < a.scenario_name_count {
            m.i16(a.scenario_names + name_index * SCENARIO_OBJECT_NAME_SIZE + SCENARIO_OBJECT_NAME_TYPE)
        } else {
            NONE as i16
        };
        if !(0..nv::NUMBER_OF_OBJECT_TYPES as i16).contains(&kind)
            || NETWORKED_OBJECT_TYPES & (1 << kind) != 0
            || host_has == (object_index != NONE)
        {
            m.w8(differing, m.r8(differing) & !bit);
            continue;
        }
        if m.r8(differing) & bit == 0 {
            m.w8(differing, m.r8(differing) | bit);
            continue;
        }
        m.w8(differing, m.r8(differing) & !bit);
        if host_has {
            let mut call = AiCall { kind: CREATE_NAMED, actor: name_index as i32, ..AiCall::default() };
            host.call(&mut call);
        } else if !holds_player(m, context, object_index) {
            let mut call = AiCall { kind: DELETE, actor: object_index, ..AiCall::default() };
            host.call(&mut call);
        }
    }
}

// ---------- a client: the host's events

/// network_coop.c's client event calls (halo_sim.h's
/// `_network_coop_client_call_*`, after the device groups' and names').
mod event_call {
    pub const HUD_HELP_TEXT: i32 = 8;
    pub const HUD_STATE_MESSAGE: i32 = 9;
    pub const HUD_OBJECTIVE: i32 = 10;
    pub const HUD_FLASH: i32 = 11;
    pub const HUD_CLEAR: i32 = 12;
    pub const HUD_CHECKPOINT: i32 = 13;
    pub const PLAYER_EFFECT: i32 = 14;
    pub const NAV_POINT: i32 = 15;
    pub const UNIT_STOP_ANIMATION: i32 = 16;
    pub const UNIT_PLAY_ANIMATION: i32 = 17;
    pub const SCENERY_ANIMATION: i32 = 18;
    pub const SOUND_IMPULSE: i32 = 19;
    pub const LOOPING_SOUND_START: i32 = 20;
    pub const LOOPING_SOUND_STOP: i32 = 21;
    pub const EFFECT_AT_FLAG: i32 = 22;
    pub const EFFECT_ON_MARKER: i32 = 23;
    pub const DETACH: i32 = 24;
    pub const ATTACH: i32 = 25;
    pub const STOP_IMPULSE: i32 = 26;
    pub const TITLE: i32 = 27;
    pub const BREAK_SURFACE: i32 = 28;
    pub const UNIT_OPEN: i32 = 29;
}

/// halo_sim.h's `struct halo_sim_network_coop_client_events`.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct ClientEventsArguments {
    pub count: i32,
    pub entries: u32,
    pub events: u32,
    pub script_sounds: u32,
    pub client_has: u32,
    pub object_name_list: u32,
    pub tag_instances: u32,
    pub tag_count: i32,
    pub hud_message_count: i32,
    pub arrow_count: i32,
    pub cutscene_flag_count: i32,
    pub chapter_title_count: i32,
    pub structure_bsp_index: i32,
    pub looping_playing: u32,
}

/// network_coop.c's `_coop_effect_*`: an effect at a cutscene flag, or on
/// an object's marker.
const EFFECT_AT_FLAG: u8 = 0;
/// network_coop.c's `CAMERA_WORLD_BOUND`: a scripted camera shake moves the
/// camera no further than an observer accepts.
const CAMERA_WORLD_BOUND: f32 = 5000.0;
/// game.h's `NUMBER_OF_SOLO_CAMPAIGN_TEAMS`.
const SOLO_CAMPAIGN_TEAMS: i32 = 10;
/// network_distributed.h's `MAXIMUM_TRACKED_OBJECTS`.
const TRACKED_OBJECTS: i32 = nv::MAXIMUM_TRACKED_OBJECTS as i32;

/// `PIN` of a real (finite here: the events' reals are checked first).
fn pin(value: f32, low: f32, high: f32) -> f32 {
    if value < low {
        low
    } else if value > high {
        high
    } else {
        value
    }
}

/// A long passed in a call's reals, as its bits.
fn bits(value: i32) -> f32 {
    f32::from_bits(value as u32)
}

struct ClientEvents<'a, H: AiHost> {
    world: ClientWorld<'a, H>,
    a: ClientEventsArguments,
    tags: crate::ai_actor::AiStepContext,
    /// The looping sounds this step started (true) or stopped: their tags
    /// say so after the calls, as the entries' playing (gathered as the step
    /// began) no longer does.
    looping: std::cell::RefCell<Vec<(i32, bool)>>,
}

impl<H: AiHost> ClientEvents<'_, H> {
    fn m(&self) -> &dyn AiMemory {
        self.world.m
    }

    fn call(&self, kind: i32, actor: i32, a: i32, b: i32, input: &[f32]) {
        self.world.call(kind, actor, a, b, input);
    }

    fn of_group(&self, tag_index: i32, group: &[u8; 4]) -> bool {
        crate::sensing::tag_of_group(self.m(), &self.tags, crate::fourcc(group), tag_index)
    }

    /// network_objects.c's `network_objects_client_has`.
    fn client_has(&self, object_index: i32) -> bool {
        let absolute_index = object_index as i16 as i32;
        object_index != NONE
            && (0..TRACKED_OBJECTS).contains(&absolute_index)
            && self.m().i32(self.a.client_has + 4 * absolute_index as u32) == object_index
            && self.world.typed(object_index, u32::MAX).is_some()
    }

    /// `distributed_object_index_valid`, the client has it and it is a unit.
    fn client_unit(&self, object_index: i32) -> bool {
        object_index != NONE
            && object_index >> 16 != 0
            && self.client_has(object_index)
            && self.world.typed(object_index, nv::_OBJECT_MASK_UNIT as u32).is_some()
    }

    /// network_distributed.c's `distributed_graph_animation`: whether the
    /// graph is an animation graph with that animation.
    fn graph_animation(&self, graph_index: i32, animation_index: i16) -> bool {
        if !self.of_group(graph_index, b"antr") {
            return false;
        }
        let Some(graph) = crate::sensing::tag_get(self.m(), &self.tags, crate::fourcc(b"antr"), graph_index) else { return false };
        let count = crate::ai_actor::tag_block_count(self.m(), graph + crate::layout::animation_graph::ANIMATIONS as u32);
        (0..count).contains(&(animation_index as i32))
    }

    /// The model of an object's definition, if it has one.
    fn model(&self, object: u32) -> Option<u32> {
        let m = self.m();
        let definition_index = m.i32(object + ud::DEFINITION_INDEX as u32);
        let model_index = crate::sensing::tag_get(m, &self.tags, crate::fourcc(b"obje"), definition_index)
            .map_or(0, |definition| m.i32(definition + crate::layout::object_definition::OBJECT_MODEL as u32 + 0xc));
        if model_index == NONE {
            return None;
        }
        crate::sensing::tag_get(m, &self.tags, crate::fourcc(b"mode"), model_index)
    }

    /// network_coop.c's `client_apply_hud`.
    fn hud(&self, event: u32) {
        let m = self.m();
        let value = m.i16(event + ev::VALUE as u32);
        let message = (0..self.a.hud_message_count).contains(&(value as i32));
        match m.r8(event + ev::TYPE as u32) as usize {
            nv::_COOP_HUD_HELP_TEXT => {
                self.call(event_call::HUD_HELP_TEXT, NONE, (m.i16(event + ev::FRAME as u32) != 0) as i32, 0, &[]);
                if message {
                    self.call(event_call::HUD_STATE_MESSAGE, NONE, value as i32, 0, &[]);
                }
            }
            nv::_COOP_HUD_OBJECTIVE if message => self.call(event_call::HUD_OBJECTIVE, NONE, value as i32, 0, &[]),
            nv::_COOP_HUD_HELP_FLASH => self.call(event_call::HUD_FLASH, NONE, (value != 0) as i32, 0, &[]),
            nv::_COOP_HUD_MESSAGES_CLEAR => self.call(event_call::HUD_CLEAR, NONE, 0, 0, &[]),
            nv::_COOP_HUD_CHECKPOINT => self.call(event_call::HUD_CHECKPOINT, NONE, (value != 0) as i32, 0, &[]),
            _ => {}
        }
    }

    /// network_coop.c's `client_apply_player_effect`.
    fn player_effect(&self, event: u32) {
        let m = self.m();
        let reals = [0, 1, 2].map(|index| m.f32(event + ev::REALS as u32 + 4 * index));
        let kind = m.r8(event + ev::TYPE as u32);
        let values = match kind as usize {
            nv::_COOP_PLAYER_EFFECT_TRANSLATION => reals.map(|r| pin(r, -CAMERA_WORLD_BOUND, CAMERA_WORLD_BOUND)),
            nv::_COOP_PLAYER_EFFECT_ROTATION => reals,
            nv::_COOP_PLAYER_EFFECT_START => [pin(reals[0], 0.0, 1.0), pin(reals[1], 0.0, 60.0), 0.0],
            nv::_COOP_PLAYER_EFFECT_STOP => [pin(reals[0], 0.0, 60.0), 0.0, 0.0],
            _ => return,
        };
        self.call(event_call::PLAYER_EFFECT, NONE, kind as i32, 0, &values);
    }

    /// network_coop.c's `client_apply_nav_point`.
    fn nav_point(&self, event: u32) {
        let m = self.m();
        let kind = m.r8(event + ev::TYPE as u32) as usize;
        let value = m.i16(event + ev::VALUE as u32);
        let target = m.i32(event + ev::TARGET as u32);
        let object_index = m.i32(event + ev::OBJECT_INDEX as u32);
        let activate = value != NONE as i16;
        let flag = kind == nv::_COOP_NAV_POINT_TEAM_FLAG || kind == nv::_COOP_NAV_POINT_UNIT_FLAG;
        let team = kind == nv::_COOP_NAV_POINT_TEAM_FLAG || kind == nv::_COOP_NAV_POINT_TEAM_OBJECT;
        if (activate && !(0..self.a.arrow_count).contains(&(value as i32)))
            || (flag && !(0..self.a.cutscene_flag_count).contains(&object_index))
            || (team && !(0..SOLO_CAMPAIGN_TEAMS).contains(&target))
            || (!team && self.world.typed(target, nv::_OBJECT_MASK_UNIT as u32).is_none())
            || kind > nv::_COOP_NAV_POINT_UNIT_OBJECT
        {
            return;
        }
        let offset = m.f32(event + ev::REALS as u32);
        self.call(event_call::NAV_POINT, target, kind as i32, value as i32, &[offset, bits(object_index)]);
    }

    /// network_coop.c's `client_apply_sound`.
    fn sound(&self, event: u32, index: u32) {
        let m = self.m();
        let tag_index = m.i32(event + ev::TAG_INDEX as u32);
        let object_index = m.i32(event + ev::OBJECT_INDEX as u32);
        let object_index = if object_index != NONE && self.client_has(object_index) { object_index } else { NONE };
        let scale = pin(m.f32(event + ev::REALS as u32), 0.0, 1.0);
        match m.r8(event + ev::TYPE as u32) as usize {
            nv::_COOP_SOUND_IMPULSE if self.of_group(tag_index, b"snd!") => {
                self.call(event_call::SOUND_IMPULSE, object_index, tag_index, 0, &[scale]);
                let sounds = self.a.script_sounds;
                let next = m.i16(sounds + ss::NEXT as u32);
                m.w32(sounds + ss::DEFINITION_INDICES as u32 + 4 * next as u32, tag_index as u32);
                m.set_i16(sounds + ss::NEXT as u32, ((next as i32 + 1) % nv::COOP_CLIENT_SCRIPT_SOUND_COUNT as i32) as i16);
            }
            nv::_COOP_SOUND_LOOPING_START if self.of_group(tag_index, b"lsnd") => {
                // (a resent sound already playing here is left alone)
                let started = self.looping.borrow().iter().rev().find(|(tag, _)| *tag == tag_index).map(|&(_, playing)| playing);
                let playing = started.unwrap_or_else(|| m.r8(self.a.looping_playing + index) != 0);
                if !playing {
                    self.call(event_call::LOOPING_SOUND_START, object_index, tag_index, 0, &[scale]);
                    self.looping.borrow_mut().push((tag_index, true));
                }
            }
            nv::_COOP_SOUND_LOOPING_STOP if self.of_group(tag_index, b"lsnd") => {
                self.call(event_call::LOOPING_SOUND_STOP, NONE, tag_index, 0, &[]);
                self.looping.borrow_mut().push((tag_index, false));
            }
            _ => {}
        }
    }

    /// network_coop.c's `client_apply_effect`.
    fn effect(&self, event: u32) {
        let m = self.m();
        let tag_index = m.i32(event + ev::TAG_INDEX as u32);
        let value = m.i16(event + ev::VALUE as u32);
        if !self.of_group(tag_index, b"effe") {
            return;
        }
        if m.r8(event + ev::TYPE as u32) == EFFECT_AT_FLAG {
            if (0..self.a.cutscene_flag_count).contains(&(value as i32)) {
                self.call(event_call::EFFECT_AT_FLAG, NONE, tag_index, value as i32, &[]);
            }
            return;
        }
        let Some((object_index, object)) = self.find(event, u32::MAX) else { return };
        let Some(model) = self.model(object) else { return };
        let markers = crate::ai_actor::tag_block_count(m, model + crate::layout::model::MARKERS as u32);
        if (0..markers).contains(&(value as i32)) {
            self.call(event_call::EFFECT_ON_MARKER, object_index, tag_index, value as i32, &[]);
        }
    }

    /// The event's object (`object_find` of its name, object and
    /// definition).
    fn find(&self, event: u32, mask: u32) -> Option<(i32, u32)> {
        let m = self.m();
        self.world.object_find(
            m.i16(event + ev::NAME_INDEX as u32),
            m.i32(event + ev::OBJECT_INDEX as u32),
            m.i32(event + ev::DEFINITION_INDEX as u32),
            mask,
        )
    }

    /// network_coop.c's `client_apply_attach`: the parent is the event's
    /// object; the child its target, with its definition in its tag and its
    /// name index in its first real; its value and frame the markers.
    fn attach(&self, event: u32) {
        let m = self.m();
        let parent = self.find(event, u32::MAX).map(|(index, _)| index).unwrap_or(NONE);
        let child_name = pin(m.f32(event + ev::REALS as u32), -1.0, 32767.0) as i16;
        let child = self
            .world
            .object_find(child_name, m.i32(event + ev::TARGET as u32), m.i32(event + ev::TAG_INDEX as u32), u32::MAX)
            .map(|(index, _)| index)
            .unwrap_or(NONE);
        if parent == NONE || child == NONE || parent == child {
            return;
        }
        let parent_of = |index: i32| self.world.typed(index, u32::MAX).map_or(NONE, |o| m.i32(o + ud::OBJECT_PARENT_OBJECT_INDEX as u32));
        let attaching = m.r8(event + ev::TYPE as u32) == nv::_COOP_ATTACH as u8;
        // (a resent attach this machine already has)
        if attaching && parent_of(child) == parent {
            return;
        }
        // (not to one of its own children, which objects.c cannot do)
        if attaching {
            let mut ancestor = parent_of(parent);
            while ancestor != NONE {
                if ancestor == child {
                    return;
                }
                ancestor = parent_of(ancestor);
            }
        }
        if !attaching {
            self.call(event_call::DETACH, parent, child, 0, &[]);
        } else {
            let frame = m.i16(event + ev::FRAME as u32) as i32;
            self.call(event_call::ATTACH, parent, child, m.i16(event + ev::VALUE as u32) as i32, &[bits(frame)]);
        }
    }

    /// network_coop.c's `client_stop_script_sounds`: a skipped cutscene's
    /// dialogue stopped.
    fn stop_script_sounds(&self) {
        let m = self.m();
        for index in 0..nv::COOP_CLIENT_SCRIPT_SOUND_COUNT as u32 {
            let at = self.a.script_sounds + ss::DEFINITION_INDICES as u32 + 4 * index;
            let definition_index = m.i32(at);
            if definition_index != NONE && self.of_group(definition_index, b"snd!") {
                self.call(event_call::STOP_IMPULSE, NONE, definition_index, 0, &[]);
            }
            m.w32(at, NONE as u32);
        }
    }

    /// network_coop.c's `client_apply_event` (the message's `index`th).
    fn apply(&self, event: u32, index: u32) {
        let m = self.m();
        let value = m.i16(event + ev::VALUE as u32);
        let object_index = m.i32(event + ev::OBJECT_INDEX as u32);
        match m.r8(event + ev::KIND as u32) as usize {
            nv::_COOP_EVENT_SOUND => self.sound(event, index),
            nv::_COOP_EVENT_TITLE => {
                if (0..self.a.chapter_title_count).contains(&(value as i32)) {
                    let delay = pin(m.f32(event + ev::REALS as u32), 0.0, 60.0);
                    self.call(event_call::TITLE, NONE, value as i32, 0, &[delay]);
                }
            }
            nv::_COOP_EVENT_HUD => self.hud(event),
            nv::_COOP_EVENT_PLAYER_EFFECT => self.player_effect(event),
            nv::_COOP_EVENT_NAV_POINT => self.nav_point(event),
            nv::_COOP_EVENT_UNIT_ANIMATION => {
                if self.client_unit(object_index) {
                    let graph = m.i32(event + ev::TAG_INDEX as u32);
                    if graph == NONE {
                        self.call(event_call::UNIT_STOP_ANIMATION, object_index, 0, 0, &[]);
                    } else if self.graph_animation(graph, value) {
                        let interpolate = m.r8(event + ev::INTERPOLATE as u32) as i32;
                        let frame = m.i16(event + ev::FRAME as u32) as i32;
                        self.call(event_call::UNIT_PLAY_ANIMATION, object_index, graph, value as i32, &[bits(interpolate), bits(frame)]);
                    }
                }
            }
            nv::_COOP_EVENT_SCENERY_ANIMATION => {
                let scenery = self.find(event, nv::_OBJECT_MASK_SCENERY as u32);
                let graph = m.i32(event + ev::TAG_INDEX as u32);
                if let Some((scenery_index, _)) = scenery
                    && self.graph_animation(graph, value)
                {
                    let frame = m.i16(event + ev::FRAME as u32) as i32;
                    self.call(event_call::SCENERY_ANIMATION, scenery_index, graph, value as i32, &[bits(frame)]);
                }
            }
            nv::_COOP_EVENT_EFFECT => self.effect(event),
            nv::_COOP_EVENT_ATTACH => self.attach(event),
            nv::_COOP_EVENT_REVERTED => self.stop_script_sounds(),
            nv::_COOP_EVENT_SURFACE_BROKEN => {
                if m.i16(event + ev::FRAME as u32) as i32 == self.a.structure_bsp_index {
                    let reals = [0, 1, 2].map(|index| m.f32(event + ev::REALS as u32 + 4 * index));
                    self.call(event_call::BREAK_SURFACE, NONE, value as i32, 0, &reals);
                }
            }
            nv::_COOP_EVENT_UNIT_OPEN => {
                if self.client_unit(object_index) {
                    self.call(event_call::UNIT_OPEN, object_index, (value != 0) as i32, 0, &[]);
                }
            }
            _ => {}
        }
    }
}

/// The world step of a co-op client applying its host's events, each once
/// and in order: network_coop.c's `network_coop_handle_events`.
pub fn client_events(context: &WorldContext, host: &impl AiHost) {
    let m = host.memory();
    let a: ClientEventsArguments = arguments(context, m);
    let world = ClientWorld {
        context,
        host,
        m,
        a: ClientWorldArguments { kind: NONE as i16, count: 0, entries: 0, object_name_list: a.object_name_list, tag_instances: 0, tag_count: 0 },
    };
    let tags = crate::ai_actor::AiStepContext { tag_instances: a.tag_instances, tag_count: a.tag_count, ..crate::ai_actor::AiStepContext::default() };
    let events = ClientEvents { world, a, tags, looping: Default::default() };
    for index in 0..a.count.max(0) as u32 {
        let event = a.entries + index * ev::SIZEOF as u32;
        let number = m.r16(event + ev::NUMBER as u32);
        // (already applied: the comparison handles the number wrapping)
        let applied_number = m.r16(a.events + ce::APPLIED_NUMBER as u32);
        if m.r8(a.events + ce::APPLIED_ANY as u32) != 0 && (number.wrapping_sub(applied_number) as i16) <= 0 {
            continue;
        }
        m.w16(a.events + ce::APPLIED_NUMBER as u32, number);
        m.w8(a.events + ce::APPLIED_ANY as u32, 1);
        // (not with a number that isn't one)
        if (0..3).any(|part| !wire::real_valid(m.f32(event + ev::REALS as u32 + 4 * part))) {
            continue;
        }
        events.apply(event, index);
    }
}

// ---------- a client: the host's presentation messages

/// halo_sim.h's `_network_coop_client_call_set_screen_effect`: the screen
/// effect set as the step left it.
const SET_SCREEN_EFFECT: i32 = 30;
/// halo_sim.h's `_coop_client_message_*`.
const CLIENT_MESSAGE_PRESENTATION: i16 = 0;

/// halo_sim.h's `struct halo_sim_network_coop_client_messages`.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct ClientMessagesArguments {
    pub kind: i16,
    pub count: i16,
    pub entries: u32,
    pub host_time: i32,
    pub now: i32,
    pub window_count: i32,
    pub presentation: u32,
    pub screen_effect: u32,
    pub screen_effect_size: i32,
}

/// network_coop.c's screen effect bounds: the rasterizer's convolution
/// types and overbright modes, and the extra passes a script gets.
const SCREEN_EFFECT_CONVOLUTION_TYPES: i16 = 3;
const SCREEN_EFFECT_OVERBRIGHT_MODES: i16 = 3;
const SCREEN_EFFECT_MAXIMUM_EXTRA_PASSES: i16 = 8;
/// network_coop_records.h's `COOP_PRESENTATION_BUFFER_COUNT`.
const PRESENTATION_BUFFER_COUNT: i32 = nv::COOP_PRESENTATION_BUFFER_COUNT as i32;

/// The world step of a co-op client handling one of its host's
/// presentation messages: network_coop.c's
/// `network_coop_handle_presentation` (the host's presentation of a tick,
/// kept to play in its turn) or `network_coop_handle_screen_effect` (the
/// scripts' screen effect, as this machine can draw it).
pub fn client_messages(context: &WorldContext, host: &impl AiHost) {
    let m = host.memory();
    let a: ClientMessagesArguments = arguments(context, m);
    let p = a.presentation;
    if a.kind == CLIENT_MESSAGE_PRESENTATION {
        m.w32(p + cp::HEARD_TIME as u32, a.now as u32);
        if m.r8(p + cp::PLAYING as u32) != 0 && a.host_time <= m.i32(p + cp::PLAYED_TIME as u32) {
            return;
        }
        let slot = (a.host_time % PRESENTATION_BUFFER_COUNT) as u32;
        for offset in 0..dp::SIZEOF as u32 {
            m.w8(p + cp::BUFFERED as u32 + slot * dp::SIZEOF as u32 + offset, m.r8(a.entries + offset));
        }
        m.w32(p + cp::BUFFERED_TIMES as u32 + 4 * slot, a.host_time as u32);
        if m.r8(p + cp::HEARD_ANY as u32) == 0 || a.host_time > m.i32(p + cp::NEWEST_TIME as u32) {
            m.w32(p + cp::NEWEST_TIME as u32, a.host_time as u32);
        }
        m.w8(p + cp::HEARD_ANY as u32, 1);
        return;
    }
    if a.count != 1 {
        return;
    }
    // (every field from the tint onward is a real, and must be a number)
    let size = a.screen_effect_size as u32;
    let first_real = se_state::FILTER_DESATURATION_TINT as u32;
    if (first_real..size).step_by(4).any(|offset| !wire::real_valid(m.f32(a.entries + offset))) {
        return;
    }
    let state = a.screen_effect;
    for offset in 0..size {
        m.w8(state + offset, m.r8(a.entries + offset));
    }
    let short = |field: usize| state + field as u32;
    let clamp = |field: usize, high: i16| m.set_i16(short(field), m.i16(short(field)).clamp(0, high));
    // (what the rasterizer can draw, and video with no convolution, as
    // rasterizer_screen_effect_set_video leaves it)
    clamp(se_state::CONVOLUTION_TYPE, SCREEN_EFFECT_CONVOLUTION_TYPES - 1);
    clamp(se_state::CONVOLUTION_EXTRA_PASSES, SCREEN_EFFECT_MAXIMUM_EXTRA_PASSES);
    clamp(se_state::VIDEO_OVERBRIGHT_MODE, SCREEN_EFFECT_OVERBRIGHT_MODES - 1);
    let radius = state + se_state::CONVOLUTION_RADIUS as u32;
    if m.r8(state + se_state::VIDEO_ON as u32) != 0 {
        m.set_i16(short(se_state::CONVOLUTION_TYPE), 0);
        m.set_i16(short(se_state::CONVOLUTION_EXTRA_PASSES), 0);
        m.set_f32(radius, 0.0);
        m.set_f32(radius + 4, 0.0);
    }
    // (the game can only blur one window, so split screen goes without)
    if a.window_count > 1 {
        m.set_i16(short(se_state::CONVOLUTION_TYPE), 0);
        m.set_f32(radius, 0.0);
        m.set_f32(radius + 4, 0.0);
    }
    let mut call = AiCall { kind: SET_SCREEN_EFFECT, actor: NONE, ..AiCall::default() };
    host.call(&mut call);
}

// ---------- a client's tick

/// network_coop.c's client tick calls (halo_sim.h's
/// `_network_coop_client_call_*`, after the screen effect's).
mod tick_call {
    pub const INPUT_ENABLE: i32 = 31;
    pub const ALLEGIANCE: i32 = 32;
    pub const SHAKE_END: i32 = 33;
    pub const CINEMATIC_START: i32 = 34;
    pub const CINEMATIC_STOP: i32 = 35;
    pub const HOST_CAMERA: i32 = 36;
    pub const LETTERBOX: i32 = 37;
    pub const ACTIVATING_CLUSTER: i32 = 38;
    pub const CAMERA: i32 = 39;
    pub const FADE: i32 = 40;
    pub const HUD_STATE: i32 = 41;
    pub const SKIP_VOTE: i32 = 42;
}

/// halo_sim.h's `struct halo_sim_network_coop_client_tick`.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct ClientTickArguments {
    pub inputs: u32,
    pub presentation: u32,
    pub skip_vote: u32,
    pub vitality: u32,
}

/// network_coop.c's presentation timing: a client ends the cinematic after
/// this long without hearing from the host, and plays the host's
/// presentations about this many ticks behind the newest it has.
const PRESENTATION_SILENCE_TICKS: i32 = 60;
const PRESENTATION_DELAY_TICKS: i32 = 2;
/// network_coop.c's `CAMERA_OBJECT_DISTANCE`: the farthest an object the
/// cutscene camera films can be from it.
const CAMERA_OBJECT_DISTANCE: f32 = 8.0;
/// network_coop.c's field of view limits (an observer's).
const CAMERA_MINIMUM_FIELD_OF_VIEW: f32 = 0.001;
const CAMERA_MAXIMUM_FIELD_OF_VIEW: f32 = PI / 2.0;
/// network_coop.c's view behind the host for a player with nothing to
/// watch: its distance behind and above the host's eyes, and the share of
/// the way it eases there each tick.
const WATCH_HOST_DISTANCE: f32 = 3.0;
const WATCH_HOST_HEIGHT: f32 = 1.0;
const WATCH_HOST_FOLLOW: f32 = 0.2;
/// real_math.h's `_real_epsilon` and `_pi`.
const REAL_EPSILON: f32 = 0.0001;
const PI: f32 = std::f32::consts::PI;

type Vector3 = [f32; 3];

fn dot(a: Vector3, b: Vector3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// real_math.h's `normalize3d`: the magnitude, the vector made unit length
/// unless it is about zero.
fn normalize3d(v: &mut Vector3) -> f32 {
    let magnitude = dot(*v, *v).sqrt();
    if !(REAL_EPSILON > magnitude.abs()) {
        let scale = 1.0 / magnitude;
        *v = v.map(|part| part * scale);
        magnitude
    } else {
        0.0
    }
}

/// real_math.c's `points_interpolate` and `vectors_interpolate`.
fn interpolate(a: Vector3, b: Vector3, t: f32) -> Vector3 {
    let one_minus_t = 1.0 - t;
    [0, 1, 2].map(|index| one_minus_t * a[index] + t * b[index])
}

/// real_math.h's `valid_realcmp`.
fn valid_realcmp(a: f32, b: f32) -> bool {
    let y = a - b;
    y.is_finite() && y.abs() < 0.001
}

/// real_math.h's `valid_real_vector3d_axes2`.
fn valid_axes2(forward: Vector3, up: Vector3) -> bool {
    valid_realcmp(dot(forward, forward), 1.0) && valid_realcmp(dot(up, up), 1.0) && valid_realcmp(dot(forward, up), 0.0)
}

/// observer.c's `observer_up_from_forward`: up square to the look, level
/// to the side.
fn up_from_forward(forward: Vector3) -> Vector3 {
    let mut right = [forward[1], -forward[0], 0.0];
    if normalize3d(&mut right) == 0.0 {
        right = [1.0, 0.0, 0.0];
    }
    // (real_math.h's cross_product3d of right and forward)
    [
        right[1] * forward[2] - right[2] * forward[1],
        right[2] * forward[0] - right[0] * forward[2],
        right[0] * forward[1] - right[1] * forward[0],
    ]
}

struct ClientTick<'a, H: AiHost> {
    context: &'a WorldContext,
    host: &'a H,
    m: &'a dyn AiMemory,
    a: ClientTickArguments,
}

impl<H: AiHost> ClientTick<'_, H> {
    fn call(&self, kind: i32, actor: i32, a: i32, b: i32, input: &[f32]) {
        let mut call = AiCall { kind, actor, a, b, ..AiCall::default() };
        call.input[..input.len()].copy_from_slice(input);
        self.host.call(&mut call);
    }

    fn p(&self, field: usize) -> u32 {
        self.a.presentation + field as u32
    }

    fn input(&self, field: usize) -> bool {
        self.m.r8(self.a.inputs + field as u32) != 0
    }

    fn vector(&self, at: u32) -> Vector3 {
        [self.m.f32(at), self.m.f32(at + 4), self.m.f32(at + 8)]
    }

    fn set_vector(&self, at: u32, v: Vector3) {
        for (index, part) in v.iter().enumerate() {
            self.m.set_f32(at + 4 * index as u32, *part);
        }
    }

    /// network_coop.c's `client_cinematic_end`.
    fn cinematic_end(&self) {
        let m = self.m;
        if m.r8(self.p(cp::CINEMATIC_STARTED)) == 0 {
            return;
        }
        m.w8(self.p(cp::CINEMATIC_STARTED), 0);
        self.call(tick_call::CINEMATIC_STOP, NONE, 0, 0, &[]);
    }

    /// network_coop.c's `client_host_camera_set`.
    fn host_camera_set(&self, host_camera: bool) {
        let m = self.m;
        if (m.r8(self.p(cp::HOST_CAMERA)) != 0) == host_camera {
            return;
        }
        m.w8(self.p(cp::HOST_CAMERA), host_camera as u8);
        self.call(tick_call::HOST_CAMERA, NONE, host_camera as i32, 0, &[]);
    }

    /// network_coop.c's `client_watch_host_from_behind`: the view eased
    /// toward a spot behind and above the host's eyes, level with where it
    /// looks; whether it makes a camera.
    fn watch_host_from_behind(&self, position: &mut Vector3, forward: &mut Vector3, up: &mut Vector3) -> bool {
        let m = self.m;
        let mut level = [forward[0], forward[1], 0.0];
        if normalize3d(&mut level) == 0.0 {
            level = *forward;
        }
        // (real_math.h's point_from_line3d)
        let mut behind = [0, 1, 2].map(|index| level[index] * -WATCH_HOST_DISTANCE + position[index]);
        behind[2] += WATCH_HOST_HEIGHT;
        let at_position = self.p(cp::WATCHING_HOST_POSITION);
        let at_forward = self.p(cp::WATCHING_HOST_FORWARD);
        if m.r8(self.p(cp::WATCHING_HOST_VALID)) == 0 {
            m.w8(self.p(cp::WATCHING_HOST_VALID), 1);
            self.set_vector(at_position, behind);
            self.set_vector(at_forward, *forward);
        } else {
            self.set_vector(at_position, interpolate(self.vector(at_position), behind, WATCH_HOST_FOLLOW));
            self.set_vector(at_forward, interpolate(self.vector(at_forward), *forward, WATCH_HOST_FOLLOW));
        }
        let mut watched_forward = self.vector(at_forward);
        let magnitude = normalize3d(&mut watched_forward);
        self.set_vector(at_forward, watched_forward);
        if magnitude == 0.0 {
            return false;
        }
        *position = self.vector(at_position);
        // (world up is not square to a host looking up or down: up rebuilt
        // from the look when that pair would fail the camera's check)
        let mut axes = [watched_forward[0], watched_forward[1], watched_forward[2], 0.0, 0.0, 1.0];
        let valid = wire::axes_make_valid(&mut axes);
        *forward = [axes[0], axes[1], axes[2]];
        *up = [axes[3], axes[4], axes[5]];
        if !valid {
            *up = up_from_forward(*forward);
        }
        valid_axes2(*forward, *up)
    }

    /// network_coop.c's `client_presentation_apply`: the host's
    /// presentation kept in the slot.
    fn apply(&self, slot: u32) {
        let m = self.m;
        let pres = self.p(cp::BUFFERED) + slot * dp::SIZEOF as u32;
        let flags = m.r8(pres + dp::FLAGS as u32);
        let flag = |bit: usize| flags & (1 << bit) != 0;
        let byte = |field: usize| m.r8(pres + field as u32) != 0;
        let cinematic = flag(nv::_PRESENTATION_CINEMATIC_BIT);
        self.call(tick_call::INPUT_ENABLE, NONE, !byte(dp::INPUT_DISABLED) as i32, 0, &[]);
        let body = m.f32(pres + dp::PLAYERS_MAXIMUM_BODY_VITALITY as u32);
        let shield = m.f32(pres + dp::PLAYERS_MAXIMUM_SHIELD_VITALITY as u32);
        if byte(dp::PLAYERS_VITALITY_SET) && wire::real_valid(body) && wire::real_valid(shield) && body > 0.0 && shield >= 0.0 {
            m.w8(self.a.vitality + pv::SET as u32, 1);
            m.set_f32(self.a.vitality + pv::MAXIMUM_BODY as u32, body);
            m.set_f32(self.a.vitality + pv::MAXIMUM_SHIELD as u32, shield);
        }
        self.call(tick_call::ALLEGIANCE, NONE, slot as i32, 0, &[]);
        if !byte(dp::SCRIPTED_SHAKE) && self.input(ti::SCRIPTED_SHAKE) {
            self.call(tick_call::SHAKE_END, NONE, 0, 0, &[]);
        }
        // (a machine that joined mid-cutscene may already have started it)
        let started_at = self.p(cp::CINEMATIC_STARTED);
        if cinematic && m.r8(started_at) == 0 {
            if !self.input(ti::CINEMATIC_IN_PROGRESS) {
                self.call(tick_call::CINEMATIC_START, NONE, 0, 0, &[]);
            }
            m.w8(started_at, 1);
        } else if !cinematic {
            self.cinematic_end();
        }
        let started = m.r8(started_at) != 0;
        let camera_scripted = byte(dp::CAMERA_SCRIPTED);
        self.host_camera_set((started && camera_scripted && !self.input(ti::WATCHING_RIDER)) || self.input(ti::NOTHING_TO_WATCH));
        if started {
            self.call(tick_call::LETTERBOX, NONE, flag(nv::_PRESENTATION_LETTERBOX_BIT) as i32, 0, &[]);
        }
        let cluster = m.i16(pres + dp::ACTIVATING_CLUSTER as u32).wrapping_sub(1);
        self.call(tick_call::ACTIVATING_CLUSTER, NONE, cluster as i32, 0, &[]);
        if m.r8(self.p(cp::HOST_CAMERA)) != 0 {
            let packed = |at: u32| [m.i16(at), m.i16(at + 2), m.i16(at + 4)];
            let mut forward = wire::vector_unpack(packed(pres + dp::CAMERA_FORWARD as u32), UNIT_SCALE);
            let mut up = wire::vector_unpack(packed(pres + dp::CAMERA_UP as u32), UNIT_SCALE);
            let mut position = self.vector(pres + dp::CAMERA_POSITION as u32);
            let object_index = m.i32(pres + dp::CAMERA_OBJECT_INDEX as u32);
            let filmed = if object_index != NONE && object_index >> 16 != 0 {
                crate::ai_actor::datum_get(m, self.context.object_header_data, object_index).map(|header| m.r32(header + oh::DATUM as u32))
            } else {
                None
            };
            // (the camera placed relative to this machine's copy of the
            // filmed object, which may be a tick behind the host's)
            let offset = self.vector(pres + dp::CAMERA_OBJECT_OFFSET as u32);
            if let Some(filmed) = filmed
                && offset.iter().all(|part| wire::real_valid(*part))
                && dot(offset, offset) < CAMERA_OBJECT_DISTANCE * CAMERA_OBJECT_DISTANCE * 4.0
            {
                let origin = self.vector(filmed + ud::OBJECT_POSITION as u32);
                position = [0, 1, 2].map(|index| offset[index] * 1.0 + origin[index]);
            }
            let mut axes = [forward[0], forward[1], forward[2], up[0], up[1], up[2]];
            if wire::point_valid(position, CAMERA_WORLD_BOUND) && wire::axes_make_valid(&mut axes) {
                forward = [axes[0], axes[1], axes[2]];
                up = [axes[3], axes[4], axes[5]];
                let mut camera_ready = true;
                if !(started && camera_scripted) {
                    camera_ready = self.watch_host_from_behind(&mut position, &mut forward, &mut up);
                }
                if camera_ready && valid_axes2(forward, up) {
                    let field_of_view = pin(
                        m.r16(pres + dp::CAMERA_FIELD_OF_VIEW as u32) as f32 / FIELD_OF_VIEW_SCALE,
                        CAMERA_MINIMUM_FIELD_OF_VIEW,
                        CAMERA_MAXIMUM_FIELD_OF_VIEW,
                    );
                    let mut input = [0.0; 9];
                    input[..3].copy_from_slice(&position);
                    input[3..6].copy_from_slice(&forward);
                    input[6..].copy_from_slice(&up);
                    self.call(tick_call::CAMERA, NONE, 0, field_of_view.to_bits() as i32, &input);
                }
            }
        } else {
            m.w8(self.p(cp::WATCHING_HOST_VALID), 0);
        }
        // (each new fade started, backdated to when the host started it)
        let fade_start_time = m.i32(pres + dp::FADE_START_TIME as u32);
        if fade_start_time != m.i32(self.p(cp::FADE_START_TIME)) {
            let color = [0, 1, 2].map(|index| m.r8(pres + dp::FADE_COLOR as u32 + index) as f32 / 255.0);
            let start = m.i32(self.a.inputs + ti::NOW as u32) - m.i16(pres + dp::FADE_ELAPSED as u32) as i32;
            let ticks = m.i16(pres + dp::FADE_TICKS as u32) as i32;
            self.call(tick_call::FADE, start, ticks, flag(nv::_PRESENTATION_FADING_OUT_BIT) as i32, &color);
            m.w32(self.p(cp::FADE_START_TIME), fade_start_time as u32);
        }
        let vote = self.a.skip_vote;
        m.w8(vote + sv::OFFERED as u32, flag(nv::_PRESENTATION_SKIPPABLE_BIT) as u8);
        m.set_i16(vote + sv::VOTES as u32, m.r8(pres + dp::SKIP_VOTES as u32) as i16);
        m.set_i16(vote + sv::VOTERS as u32, m.r8(pres + dp::SKIP_VOTERS as u32) as i16);
        self.call(tick_call::HUD_STATE, NONE, slot as i32, 0, &[]);
    }

    /// network_coop.c's `client_play_presentation`: one presentation a
    /// tick, about PRESENTATION_DELAY_TICKS behind the newest, catching up
    /// two at a time when further behind.
    fn play_presentation(&self) {
        let m = self.m;
        if m.r8(self.p(cp::HEARD_ANY)) == 0 {
            return;
        }
        let newest = m.i32(self.p(cp::NEWEST_TIME));
        let played_at = self.p(cp::PLAYED_TIME);
        let lag = newest.wrapping_sub(m.i32(played_at));
        if m.r8(self.p(cp::PLAYING)) == 0 || lag > PRESENTATION_BUFFER_COUNT || lag < -PRESENTATION_BUFFER_COUNT {
            m.w32(played_at, newest.wrapping_sub(PRESENTATION_DELAY_TICKS) as u32);
        } else if lag > PRESENTATION_DELAY_TICKS + 1 {
            m.w32(played_at, m.i32(played_at).wrapping_add(2) as u32);
        } else if lag > 0 {
            m.w32(played_at, m.i32(played_at).wrapping_add(1) as u32);
        } else {
            return;
        }
        m.w8(self.p(cp::PLAYING), 1);
        let played = m.i32(played_at);
        let mut best: Option<(i32, u32)> = None;
        for index in 0..PRESENTATION_BUFFER_COUNT as u32 {
            let time = m.i32(self.p(cp::BUFFERED_TIMES) + 4 * index);
            if time <= played && time > played - PRESENTATION_BUFFER_COUNT && time > best.map_or(NONE, |(t, _)| t) {
                best = Some((time, index));
            }
        }
        if let Some((_, slot)) = best {
            self.apply(slot);
        }
    }

    /// network_coop.c's `players_vitality_keep`, a client's: every player's
    /// unit at the maximum vitality the host's scripts set.
    fn vitality_keep(&self) {
        let m = self.m;
        if m.r8(self.a.vitality + pv::SET as u32) == 0 {
            return;
        }
        let body = m.f32(self.a.vitality + pv::MAXIMUM_BODY as u32);
        let shield = m.f32(self.a.vitality + pv::MAXIMUM_SHIELD as u32);
        let data = self.context.player_data;
        let size = m.r16(data + DATA_SIZE) as i16 as u32;
        for player_index in 0..m.r16(data + DATA_COUNT) as i16 as u32 {
            let player = m.r32(data + DATA_DATA) + player_index * size;
            if m.i16(player + pd::IDENTIFIER as u32) == 0 {
                continue;
            }
            let unit_index = m.i32(player + pd::UNIT_INDEX as u32);
            let Some(header) = crate::ai_actor::datum_get(m, self.context.object_header_data, unit_index) else { continue };
            let unit = m.r32(header + oh::DATUM as u32);
            let body_at = unit + ud::OBJECT_MAXIMUM_BODY_VITALITY as u32;
            let shield_at = unit + ud::OBJECT_MAXIMUM_SHIELD_VITALITY as u32;
            if m.f32(body_at) == body && m.f32(shield_at) == shield {
                continue;
            }
            m.set_f32(body_at, body);
            m.set_f32(shield_at, shield);
        }
    }

    /// network_coop.c's `network_coop_client_tick`.
    fn run(&self) {
        let m = self.m;
        self.play_presentation();
        self.vitality_keep();
        let now = m.i32(self.a.inputs + ti::NOW as u32);
        if now.wrapping_sub(m.i32(self.p(cp::HEARD_TIME))) > PRESENTATION_SILENCE_TICKS {
            self.cinematic_end();
            self.host_camera_set(false);
            self.call(tick_call::INPUT_ENABLE, NONE, 1, 0, &[]);
        }
        // (sent every tick while a skip is offered, voted or not: the host
        // only counts machines it hears from)
        let vote = self.a.skip_vote;
        if !(m.r8(vote + sv::OFFERED as u32) != 0 && m.r8(self.p(cp::CINEMATIC_STARTED)) != 0) {
            m.w8(vote + sv::VOTED as u32, 0);
        } else {
            self.call(tick_call::SKIP_VOTE, NONE, (m.r8(vote + sv::VOTED as u32) != 0) as i32, 0, &[]);
        }
    }
}

/// The world step of a co-op client's tick: the host's presentation played
/// in its turn, the players' vitality kept, the cinematic ended when the
/// host goes quiet, and the skip vote sent while a skip is offered.
pub fn client_tick(context: &WorldContext, host: &impl AiHost) {
    let m = host.memory();
    let a: ClientTickArguments = arguments(context, m);
    ClientTick { context, host, m, a }.run();
}
