//! The host sending its objects to its clients each tick (OpenCE's
//! port/linux/game/network_objects.c's `network_objects_host_tick`:
//! `distributed_host_update_objects`, `_find_viewers`, `_send_states`,
//! `_send_inventories` and `_send_damage_animations`).
//!
//! - **Objects made and deleted:** each networked object the host has not
//!   told its clients of goes to all of them, reliably, with what it is,
//!   where and how it looks; each it told of that is gone (or another in its
//!   place) is deleted.
//! - **Viewers:** each client machine's players' living units, where they
//!   are, and the vehicles they drive; and where every player's unit is.
//! - **States:** each moving object goes to each client as often as it is
//!   near the client's players (every tick within 25 world units, then every
//!   second, third and fourth), the vehicle the client drives every third
//!   tick with which of its ticks the host has it at. One come to rest goes
//!   to every client three times, eight ticks apart, and four at rest each
//!   tick, round them all.
//! - **Inventories** (every third tick): a unit's weapons or grenades
//!   changed, or not sent for a second, to every client; its ammunition alone
//!   changed to its own client at once and to the others as often as its
//!   place goes to them.
//! - **Damage animations:** the flinches and deaths the host picked, each in
//!   three ticks' messages.
//!
//! Before that tick the host takes its clients' word on the vehicles their
//! players drive ([`vehicle_predictions`], `network_objects_apply_vehicle_predictions`):
//! each within a tolerance of its copy, no further from the last it took
//! than the vehicle goes in the client's ticks since, no faster than twice
//! its tag's top speed (or as fast as the host's own ticks sent it), and no
//! higher in the air than it was thrown up leaving the ground.
//!
//! The host's records are the game's memory (network_objects_records.h),
//! the tick's arguments a struct the game passes ([`Arguments`]): the
//! records' addresses, the message being sent, the messages' types and
//! sizes, the client machines and their players as the tick began, and the
//! camera of a co-op cutscene. Sending a message and an object's origin are
//! the game's, through [`AiHost::call`] (network_objects.c's
//! `network_host_call`).

use crate::ai_actor::{AiCall, AiHost, AiMemory, AiMemoryExt, NONE, WorldContext, datum_get};
use crate::data_array::{DATA_COUNT, DATA_DATA, DATA_SIZE};
use crate::distributed_wire::{self as wire, NO_PLAYER};
use crate::layout::{
    distributed_object_change as ch, distributed_object_state as st, net_values as nv, object_header_datum as oh,
    objects_host_damage_animation as da, objects_host_inventory as inv, objects_host_vehicle_prediction as vp,
    objects_host_viewer_machine as vm, objects_host_viewers as vw, objects_statistics as stats, player_datum as pd,
    unit_datum as ud,
};

/// network_objects.c's host calls (halo_sim.h's `_network_host_call_*`).
pub mod call {
    /// The message being sent (`objects_host_message`), its type
    /// `b & 0xff`, count `(b >> 8) & 0xff` and size `b >> 16`: to the client
    /// machine `a` (`distributed_send_to_machine`), or with `a` NONE where
    /// `actor` says (`distributed_send`).
    pub const SEND: i32 = 1;
    /// `object_get_origin(a)`, `output[0..3]`.
    pub const ORIGIN: i32 = 2;
    /// `distributed_machine_has_player(a, b)`, the result.
    pub const MACHINE_HAS_PLAYER: i32 = 3;
    /// `network_coop_player_has_structure_bsp(a)`, the result.
    pub const COOP_HAS_STRUCTURE_BSP: i32 = 4;
    /// `distributed_machine_round_trip_ticks(a)`, `output[0]`.
    pub const ROUND_TRIP: i32 = 5;
    /// The vehicle tag `a`'s type (the result) and its top speeds forward
    /// and back (`output[0..2]`).
    pub const VEHICLE_DEFINITION: i32 = 6;
    /// `network_objects_reconcile` of the object `a` to the prediction in
    /// [`VehicleArguments`](super::VehicleArguments), the result.
    pub const RECONCILE: i32 = 7;
}

/// halo_sim.h's `struct halo_sim_network_host`: the tick's arguments.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct Arguments {
    pub told: u32,
    pub told_count: u32,
    pub resting_cursor: u32,
    pub state_moving: u32,
    pub rest_times: u32,
    pub inventories: u32,
    pub viewers: u32,
    pub vehicle_predictions: u32,
    pub statistics: u32,
    pub damage_animations: u32,
    pub damage_animation_count: u32,
    pub message: u32,
    pub change_type: i16,
    pub state_type: i16,
    pub inventory_type: i16,
    pub damage_animation_type: i16,
    pub change_limit: i16,
    pub state_limit: i16,
    pub inventory_limit: i16,
    pub to_clients: i16,
    pub to_clients_reliably: i16,
    pub pad: i16,
    pub machine_count: i32,
    pub machine_indices: [i32; MAXIMUM_MACHINES],
    pub player_lists: [[i32; MAXIMUM_LOCAL_PLAYERS]; MAXIMUM_MACHINES],
    pub coop_cinematic: i32,
    pub camera: [f32; 3],
}

const _: () = assert!(std::mem::size_of::<Arguments>() == 12 * 4 + 10 * 2 + 4 + 128 * 4 + 128 * 16 + 16);

/// halo_sim.h's `struct halo_sim_network_vehicle_predictions`: the
/// vehicle predictions' arguments, and the prediction being taken.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct VehicleArguments {
    pub predictions: u32,
    pub gravity: f32,
    pub position: [f32; 3],
    pub forward: [f32; 3],
    pub up: [f32; 3],
    pub velocity: [f32; 3],
    pub angular_velocity: [f32; 3],
}

const _: () = assert!(std::mem::size_of::<VehicleArguments>() == 8 + 15 * 4);

/// halo_port_limits.h's `HALO_PORT_MAXIMUM_NETWORK_MACHINES`, players.h's
/// `MAXIMUM_LOCAL_PLAYERS`.
const MAXIMUM_MACHINES: usize = 128;
const MAXIMUM_LOCAL_PLAYERS: usize = 4;
const MAXIMUM_TRACKED_OBJECTS: i32 = nv::MAXIMUM_TRACKED_OBJECTS as i32;
const MAXIMUM_TRACKED_PLAYERS: i16 = nv::MAXIMUM_TRACKED_PLAYERS as i16;

/// network_objects.c's limits.
const TICKS_PER_SECOND: i32 = 30;
const INVENTORY_INTERVAL_TICKS: i32 = 3;
const INVENTORY_REFRESH_TICKS: i32 = TICKS_PER_SECOND;
const EMPTY_INVENTORY_TICKS: i32 = 3 * INVENTORY_REFRESH_TICKS;
const RESTING_STATES_PER_TICK: i16 = 4;
const REST_STATE_REPEATS: i32 = 3;
const REST_STATE_REPEAT_TICKS: i32 = 8;
const MAXIMUM_OBJECT_PERIOD_TICKS: i16 = 4;
const OWN_VEHICLE_PERIOD_TICKS: i16 = 3;
const VEHICLE_PREDICTION_REFERENCE_TICKS: i32 = 2 * TICKS_PER_SECOND;
const VEHICLE_PREDICTION_JITTER_TICKS: i32 = 6;
const VEHICLE_PREDICTION_RISE_TOLERANCE: f32 = 2.0;
const VEHICLE_PREDICTION_CEILING_LEAD_TICKS: f32 = 15.0;
const VEHICLE_PREDICTION_ANCHOR_TICKS: i32 = TICKS_PER_SECOND;
const VEHICLE_PREDICTION_SPEED_TICKS: i32 = VEHICLE_PREDICTION_ANCHOR_TICKS + 2 * TICKS_PER_SECOND;
const VEHICLE_PREDICTION_STILL_DISTANCE: f32 = 0.01;
const HOST_VEHICLE_ACCEPT_TOLERANCE: f32 = 5.0;
const MAXIMUM_PREDICTED_VEHICLE_SPEED: f32 = 3.0;
const MAXIMUM_PREDICTED_VEHICLE_ANGULAR_SPEED: f32 = 1.0;
const PREDICTED_VEHICLE_SPEED_MARGIN: f32 = 0.1;
/// network_distributed.h's.
const HOST_VEHICLE_BLEND_DISTANCE: f32 = 0.5;
/// vehicles.c's vehicle types that float, fly or stay.
const VEHICLE_TYPE_HUMAN_BOAT: i16 = 2;
const VEHICLE_TYPE_HUMAN_PLANE: i16 = 3;
const VEHICLE_TYPE_ALIEN_FIGHTER: i16 = 5;
const VEHICLE_TYPE_TURRET: i16 = 6;
const NEAR_OBJECT_DISTANCE: f32 = 25.0;
const MIDDLE_OBJECT_DISTANCE: f32 = 60.0;
const FAR_OBJECT_DISTANCE: f32 = 120.0;

/// network_objects.c's `NETWORKED_OBJECT_TYPES`.
const NETWORKED_OBJECT_TYPES: u32 =
    (nv::_OBJECT_MASK_BIPED | nv::_OBJECT_MASK_VEHICLE | nv::_OBJECT_MASK_WEAPON | nv::_OBJECT_MASK_EQUIPMENT) as u32;

/// `_object_change_create`, `_object_change_delete`.
const CHANGE_CREATE: u8 = 0;
const CHANGE_DELETE: u8 = 1;

/// The change's and state's flags (`_distributed_object_carried_bit` and
/// the rest).
const FLAG_CARRIED: u8 = 1 << 0;
const FLAG_AT_REST: u8 = 1 << 1;
const FLAG_DEAD: u8 = 1 << 2;
const FLAG_PREDICTED: u8 = 1 << 3;

/// `struct distributed_message_header`'s size; the entries follow it.
const HEADER: u32 = crate::layout::distributed_message_header::SIZEOF as u32;

/// `struct distributed_inventory`'s size, and its bytes up to the
/// ammunition (its weapons, the one in hand and its grenades).
const INVENTORY_SIZE: usize = std::mem::size_of::<wire::Inventory>();
const INVENTORY_WEAPONS_SIZE: usize = 24;
const _: () = assert!(INVENTORY_SIZE == 64);

/// `struct distributed_damage_animation`'s size.
const DAMAGE_ANIMATION_SIZE: u32 = crate::layout::distributed_damage_animation::SIZEOF as u32;

/// The kinds of states sent this tick: moving (to each client as often as
/// it is near its players) or to every client.
#[derive(Clone, Copy, PartialEq)]
enum StateKind {
    Moving,
    ToAll,
}

/// The kinds of inventories sent this time.
#[derive(Clone, Copy, PartialEq)]
enum InventoryKind {
    /// its weapons or grenades changed, or not sent for a while: to every
    /// client
    ToAll,
    /// its ammunition alone changed: to its own client, and to the others
    /// as often as they are sent where it is
    Ammunition,
    /// ... not now, but lately: to the others, in their turn
    AmmunitionLately,
}

/// The tick's arguments where the game holds them.
fn arguments(m: &dyn AiMemory, address: u32) -> Arguments {
    let mut bytes = [0u8; std::mem::size_of::<Arguments>()];
    for (index, byte) in bytes.iter_mut().enumerate() {
        *byte = m.r8(address + index as u32);
    }
    // SAFETY: plain data of its size
    unsafe { std::mem::transmute(bytes) }
}

fn write_bytes(m: &dyn AiMemory, address: u32, bytes: &[u8]) {
    for (index, &byte) in bytes.iter().enumerate() {
        m.w8(address + index as u32, byte);
    }
}

/// A plain struct's bytes.
fn bytes_of<T: Copy, const N: usize>(value: &T) -> [u8; N] {
    assert_eq!(std::mem::size_of::<T>(), N);
    // SAFETY: plain data of N bytes (checked)
    unsafe { std::mem::transmute_copy(value) }
}

struct Host<'a, H: AiHost> {
    context: &'a WorldContext,
    host: &'a H,
    m: &'a dyn AiMemory,
    arguments: Arguments,
}

impl<H: AiHost> Host<'_, H> {
    fn now(&self) -> i32 {
        self.context.game_time
    }

    fn call(&self, kind: i32, a: i32, b: i32, actor: i32) -> AiCall {
        let mut call = AiCall { kind, actor, a, b, ..AiCall::default() };
        self.host.call(&mut call);
        call
    }

    fn origin(&self, object_index: i32) -> [f32; 3] {
        self.call(call::ORIGIN, object_index, 0, NONE).vector()
    }

    /// The message in the buffer, of its type and count, to the machine
    /// (NONE: where `destination` says).
    fn send(&self, machine_index: i32, destination: i16, kind: i16, count: i16, size: u32) {
        let b = (kind as i32 & 0xff) | ((count as i32 & 0xff) << 8) | ((size as i32) << 16);
        self.call(call::SEND, machine_index, b, if machine_index == NONE { destination as i32 } else { 0 });
    }

    fn game(&self) -> wire::Game {
        wire::Game { object_header_data: self.context.object_header_data, player_data: self.context.player_data }
    }

    /// `object_try_and_get`.
    fn object(&self, object_index: i32) -> Option<u32> {
        datum_get(self.m, self.context.object_header_data, object_index).map(|header| self.m.r32(header + oh::DATUM as u32))
    }

    fn object_type(&self, object: u32) -> u32 {
        self.m.r8(object + ud::OBJECT_TYPE as u32) as u32
    }

    /// `object_try_and_get_and_verify_type`.
    fn object_of_types(&self, object_index: i32, mask: u32) -> Option<u32> {
        self.object(object_index).filter(|&object| mask & (1 << self.object_type(object)) != 0)
    }

    fn dead(&self, object: u32) -> bool {
        self.m.r16(object + ud::OBJECT_DAMAGE_FLAGS as u32) as u32 & (1 << nv::_OBJECT_DEAD_BIT) != 0
    }

    fn object_flag(&self, object: u32, bit: usize) -> bool {
        self.m.r32(object + ud::OBJECT_FLAGS as u32) & (1 << bit) != 0
    }

    /// The objects of the types, as `object_iterator_next` finds them (no
    /// header flags asked): their datum indices.
    fn objects_of_types(&self, mask: u32) -> Vec<i32> {
        let m = self.m;
        let data = self.context.object_header_data;
        let size = m.r16(data + DATA_SIZE) as i16 as u32;
        let base = m.r32(data + DATA_DATA);
        let mut indices = Vec::new();
        for absolute_index in 0..m.r16(data + DATA_COUNT) as i16 as u32 {
            let header = base + absolute_index * size;
            let identifier = m.i16(header);
            if identifier != 0 && mask & (1 << m.r8(header + oh::TYPE as u32)) != 0 {
                indices.push((identifier as i32) << 16 | absolute_index as i32);
            }
        }
        indices
    }

    /// network_objects.c's `distributed_object_networked`.
    fn networked(&self, object_index: i32) -> bool {
        let m = self.m;
        let Some(header) = datum_get(m, self.context.object_header_data, object_index) else { return false };
        m.r32(header + oh::DATUM as u32) != 0
            && NETWORKED_OBJECT_TYPES & (1 << m.r8(header + oh::TYPE as u32)) != 0
            && m.r8(header + oh::FLAGS as u32) & (1 << nv::_OBJECT_HEADER_BEING_DELETED_BIT) == 0
            && (object_index as i16 as i32) < MAXIMUM_TRACKED_OBJECTS
    }

    /// `player_try_and_get` of a player's datum index.
    fn player(&self, player_index: i32) -> Option<u32> {
        if player_index == NONE {
            return None;
        }
        datum_get(self.m, self.context.player_data, player_index)
    }

    /// network_distributed.c's `distributed_living_unit`.
    fn living_unit(&self, player: Option<u32>) -> i32 {
        let Some(player) = player else { return NONE };
        let unit_index = self.m.i32(player + pd::UNIT_INDEX as u32);
        match self.object(unit_index) {
            Some(unit) if unit_index != NONE && !self.dead(unit) => unit_index,
            _ => NONE,
        }
    }

    /// network_objects.c's `distributed_driven_vehicle`.
    fn driven_vehicle(&self, player: Option<u32>) -> i32 {
        let unit_index = self.living_unit(player);
        if unit_index == NONE {
            return NONE;
        }
        let parent = self.m.i32(self.object(unit_index).unwrap_or(0) + ud::OBJECT_PARENT_OBJECT_INDEX as u32);
        if parent == NONE {
            return NONE;
        }
        match self.object_of_types(parent, nv::_OBJECT_MASK_VEHICLE as u32) {
            Some(vehicle) if self.m.i32(vehicle + ud::UNIT_DRIVER_OBJECT_INDEX as u32) == unit_index => parent,
            _ => NONE,
        }
    }

    // ---------- the records

    fn told(&self, absolute_index: i32) -> u32 {
        self.arguments.told + 4 * absolute_index as u32
    }

    fn told_count(&self) -> i32 {
        self.m.i32(self.arguments.told_count)
    }

    fn inventory(&self, absolute_index: i32) -> u32 {
        self.arguments.inventories + absolute_index as u32 * inv::SIZEOF as u32
    }

    fn machine(&self, machine_number: i16) -> u32 {
        self.arguments.viewers + vw::MACHINES as u32 + machine_number as u32 * vm::SIZEOF as u32
    }

    fn viewer_count(&self) -> i16 {
        self.m.i16(self.arguments.viewers + vw::COUNT as u32)
    }

    fn viewer_index(&self, machine_number: i16) -> i32 {
        self.m.i32(self.arguments.viewers + vw::INDICES as u32 + 4 * machine_number as u32)
    }

    fn count_statistic(&self, offset: usize) {
        let address = self.arguments.statistics + offset as u32;
        self.m.w32(address, self.m.r32(address).wrapping_add(1));
    }

    // ---------- the objects made and deleted

    /// network_objects.c's `distributed_change_from_object`.
    fn change_from_object(&self, object_index: i32, change: u32) {
        let m = self.m;
        let object = self.object(object_index).unwrap_or(0);
        let at = |offset: usize| object + offset as u32;
        for offset in 0..ch::SIZEOF as u32 {
            m.w8(change + offset, 0);
        }
        m.w8(change + ch::CHANGE as u32, CHANGE_CREATE);
        m.w32(change + ch::OBJECT_INDEX as u32, object_index as u32);
        m.w32(change + ch::DEFINITION_INDEX as u32, m.r32(at(ud::DEFINITION_INDEX)));
        let owner = m.i32(at(ud::OBJECT_OWNER_PLAYER_INDEX));
        m.w8(change + ch::OWNER_PLAYER_INDEX as u32, if owner != NONE { owner as u8 } else { NO_PLAYER });
        m.w16(change + ch::OWNER_TEAM_INDEX as u32, m.r16(at(ud::OBJECT_OWNER_TEAM_INDEX)));
        m.w16(change + ch::VARIANT_NUMBER as u32, m.r16(at(ud::OBJECT_VARIANT_NUMBER)));
        let permutation = m.i16(at(ud::OBJECT_FORCED_SHADER_PERMUTATION_INDEX)).clamp(0, 0xff);
        m.w8(change + ch::FORCED_SHADER_PERMUTATION_INDEX as u32, permutation as u8);
        for (from, to) in [
            (ud::OBJECT_POSITION, ch::POSITION),
            (ud::OBJECT_FORWARD, ch::FORWARD),
            (ud::OBJECT_UP, ch::UP),
            (ud::OBJECT_TRANSLATIONAL_VELOCITY, ch::TRANSLATIONAL_VELOCITY),
            (ud::OBJECT_ANGULAR_VELOCITY, ch::ANGULAR_VELOCITY),
        ] {
            for part in 0..3 {
                m.w32(change + to as u32 + 4 * part, m.r32(at(from) + 4 * part));
            }
        }
        for offset in 0..(nv::NUMBER_OF_OBJECT_CHANGE_COLORS * 12) as u32 {
            m.w8(change + ch::CHANGE_COLORS as u32 + offset, m.r8(at(ud::OBJECT_BASE_CHANGE_COLORS) + offset));
        }
        for offset in 0..nv::MAXIMUM_REGIONS_PER_OBJECT as u32 {
            m.w8(change + ch::REGION_PERMUTATIONS as u32 + offset, m.r8(at(ud::OBJECT_REGION_PERMUTATIONS) + offset));
        }
        let mut flags = 0;
        if self.object_flag(object, nv::_OBJECT_AT_REST_BIT) {
            flags |= FLAG_AT_REST;
        }
        if (nv::_OBJECT_MASK_UNIT as u32) & (1 << self.object_type(object)) != 0 && self.dead(object) {
            flags |= FLAG_DEAD;
        }
        if (nv::_OBJECT_MASK_ITEM as u32) & (1 << self.object_type(object)) != 0 {
            let attached = m.r32(object + crate::layout::item_datum::ITEM_FLAGS as u32) & (1 << nv::_ITEM_ATTACHED_TO_UNIT_BIT) != 0;
            if attached
                || m.i32(at(ud::OBJECT_PARENT_OBJECT_INDEX)) != NONE
                || !self.object_flag(object, nv::_OBJECT_CONNECTED_TO_MAP_BIT)
            {
                flags |= FLAG_CARRIED;
            }
        }
        m.w8(change + ch::FLAGS as u32, flags);
    }

    /// The change message's entry.
    fn change_entry(&self, count: i16) -> u32 {
        self.arguments.message + HEADER + count as u32 * ch::SIZEOF as u32
    }

    /// One more change in the message: sent once it is full.
    fn change_added(&self, count: &mut i16) {
        *count += 1;
        if *count == self.arguments.change_limit {
            self.send_changes(*count);
            *count = 0;
        }
    }

    fn send_changes(&self, count: i16) {
        let a = &self.arguments;
        self.send(NONE, a.to_clients_reliably, a.change_type, count, HEADER + count as u32 * ch::SIZEOF as u32);
    }

    /// A delete of the object in the message.
    fn delete_entry(&self, count: i16, object_index: i32) {
        let entry = self.change_entry(count);
        for offset in 0..ch::SIZEOF as u32 {
            self.m.w8(entry + offset, 0);
        }
        self.m.w8(entry + ch::CHANGE as u32, CHANGE_DELETE);
        self.m.w32(entry + ch::OBJECT_INDEX as u32, object_index as u32);
    }

    /// network_objects.c's `distributed_host_update_objects`.
    fn update_objects(&self) {
        let m = self.m;
        let mut seen = vec![false; MAXIMUM_TRACKED_OBJECTS as usize];
        let mut count: i16 = 0;
        for object_index in self.objects_of_types(NETWORKED_OBJECT_TYPES) {
            if !self.networked(object_index) {
                continue;
            }
            let absolute_index = object_index as i16 as i32;
            seen[absolute_index as usize] = true;
            let told = m.i32(self.told(absolute_index));
            if told == object_index {
                continue;
            }
            // (another object in the same place: that one is gone)
            if told != NONE {
                self.delete_entry(count, told);
                self.count_statistic(stats::DELETES);
                self.change_added(&mut count);
            }
            m.w32(self.told(absolute_index), object_index as u32);
            if absolute_index >= self.told_count() {
                m.w32(self.arguments.told_count, (absolute_index + 1) as u32);
            }
            self.count_statistic(stats::CREATES);
            self.change_from_object(object_index, self.change_entry(count));
            self.change_added(&mut count);
        }
        for absolute_index in 0..self.told_count() {
            let told = m.i32(self.told(absolute_index));
            if told == NONE || seen[absolute_index as usize] {
                continue;
            }
            self.delete_entry(count, told);
            m.w32(self.told(absolute_index), NONE as u32);
            self.count_statistic(stats::DELETES);
            self.change_added(&mut count);
        }
        if count != 0 {
            self.send_changes(count);
        }
    }

    // ---------- the viewers

    /// network_objects.c's `distributed_host_find_viewers`.
    fn find_viewers(&self) {
        let m = self.m;
        let a = &self.arguments;
        let viewers = a.viewers;
        let count = a.machine_count as i16;
        m.set_i16(viewers + vw::COUNT as u32, count);
        for index in 0..count.max(0) as usize {
            m.w32(viewers + vw::INDICES as u32 + 4 * index as u32, a.machine_indices[index] as u32);
        }
        m.set_i16(viewers + vw::PLAYER_UNIT_COUNT as u32, 0);
        let data = self.context.player_data;
        let size = m.r16(data + DATA_SIZE) as i16 as u32;
        for player_index in 0..m.r16(data + DATA_COUNT) as i16 as u32 {
            let unit_count = m.i16(viewers + vw::PLAYER_UNIT_COUNT as u32);
            if unit_count >= MAXIMUM_TRACKED_PLAYERS {
                break;
            }
            let player = m.r32(data + DATA_DATA) + player_index * size;
            if m.i16(player + pd::IDENTIFIER as u32) == 0 {
                continue;
            }
            let unit_index = self.living_unit(Some(player));
            if unit_index != NONE {
                m.set_i16(viewers + vw::PLAYER_UNIT_COUNT as u32, unit_count + 1);
                let origin = self.origin(unit_index);
                self.write_point(viewers + vw::PLAYER_UNIT_ORIGINS as u32 + 12 * unit_count as u32, origin);
            }
        }
        for machine_number in 0..count.max(0) {
            let machine = self.machine(machine_number);
            let player_list = &a.player_lists[machine_number as usize];
            m.set_i16(machine + vm::UNIT_COUNT as u32, 0);
            m.set_i16(machine + vm::VEHICLE_COUNT as u32, 0);
            for &player_index in player_list {
                let player = self.player(player_index);
                let unit_index = self.living_unit(player);
                let vehicle_index = if unit_index != NONE { self.driven_vehicle(player) } else { NONE };
                if unit_index == NONE {
                    continue;
                }
                let count = m.i16(machine + vm::UNIT_COUNT as u32);
                m.set_i16(machine + vm::UNIT_COUNT as u32, count + 1);
                m.w32(machine + vm::UNIT_INDICES as u32 + 4 * count as u32, unit_index as u32);
                let origin = self.origin(unit_index);
                self.write_point(machine + vm::ORIGINS as u32 + 12 * count as u32, origin);
                if vehicle_index != NONE {
                    let count = m.i16(machine + vm::VEHICLE_COUNT as u32);
                    m.set_i16(machine + vm::VEHICLE_COUNT as u32, count + 1);
                    m.w32(machine + vm::VEHICLE_INDICES as u32 + 4 * count as u32, vehicle_index as u32);
                    m.set_i16(machine + vm::VEHICLE_PLAYER_INDICES as u32 + 2 * count as u32, player_index as i16);
                }
            }
        }
    }

    fn write_point(&self, address: u32, point: [f32; 3]) {
        for (part, value) in point.iter().enumerate() {
            self.m.set_f32(address + 4 * part as u32, *value);
        }
    }

    /// network_objects.c's `distributed_host_object_period`: how often, in
    /// ticks, a moving object goes to the machine, by how near it is to the
    /// machine's nearest player (every player's, for a machine with none).
    fn object_period(&self, machine_number: i16, position: [f32; 3]) -> i16 {
        let m = self.m;
        let a = &self.arguments;
        let machine = self.machine(machine_number);
        let (mut count, mut origins) = (m.i16(machine + vm::UNIT_COUNT as u32), machine + vm::ORIGINS as u32);
        let mut nearest = -1.0f32;
        if count == 0 {
            count = m.i16(a.viewers + vw::PLAYER_UNIT_COUNT as u32);
            origins = a.viewers + vw::PLAYER_UNIT_ORIGINS as u32;
        }
        // (in a co-op cutscene every client looks through the host's camera)
        if a.coop_cinematic != 0 {
            let dx = position[0] - a.camera[0];
            let dy = position[1] - a.camera[1];
            let dz = position[2] - a.camera[2];
            nearest = dx * dx + dy * dy + dz * dz;
        }
        if count == 0 && nearest < 0.0 {
            return MAXIMUM_OBJECT_PERIOD_TICKS;
        }
        for index in 0..count.max(0) as u32 {
            let origin = m.vector(origins + 12 * index);
            let dx = position[0] - origin[0];
            let dy = position[1] - origin[1];
            let dz = position[2] - origin[2];
            let distance_squared = dx * dx + dy * dy + dz * dz;
            if nearest < 0.0 || distance_squared < nearest {
                nearest = distance_squared;
            }
        }
        if nearest < NEAR_OBJECT_DISTANCE * NEAR_OBJECT_DISTANCE {
            1
        } else if nearest < MIDDLE_OBJECT_DISTANCE * MIDDLE_OBJECT_DISTANCE {
            2
        } else if nearest < FAR_OBJECT_DISTANCE * FAR_OBJECT_DISTANCE {
            3
        } else {
            MAXIMUM_OBJECT_PERIOD_TICKS
        }
    }

    // ---------- the states

    /// network_objects.c's `distributed_host_placed_object`: the object at
    /// an absolute index whose state goes to the clients.
    fn placed_object(&self, absolute_index: i32) -> Option<(i32, u32)> {
        let object_index = self.m.i32(self.told(absolute_index));
        if object_index == NONE {
            return None;
        }
        let object = self.object(object_index)?;
        if self.m.i32(object + ud::OBJECT_PARENT_OBJECT_INDEX as u32) != NONE
            || !self.object_flag(object, nv::_OBJECT_CONNECTED_TO_MAP_BIT)
            || self.object_flag(object, nv::_OBJECT_OUTSIDE_OF_MAP_BIT)
        {
            return None;
        }
        if (nv::_OBJECT_MASK_UNIT as u32) & (1 << self.object_type(object)) != 0
            && self.m.i32(object + ud::UNIT_PLAYER_INDEX as u32) != NONE
            && !self.dead(object)
        {
            return None;
        }
        Some((object_index, object))
    }

    /// network_objects.c's `distributed_host_rest_state_due`.
    fn rest_state_due(&self, absolute_index: i32) -> bool {
        let rest_time = self.m.i32(self.arguments.rest_times + 4 * absolute_index as u32);
        let since = self.now().wrapping_sub(rest_time);
        rest_time != NONE && since < REST_STATE_REPEATS * REST_STATE_REPEAT_TICKS && since % REST_STATE_REPEAT_TICKS == 0
    }

    /// network_objects.c's `distributed_host_send_states`.
    fn send_states(&self) {
        let m = self.m;
        let a = &self.arguments;
        let now = self.now();
        let game = self.game();
        let told_count = self.told_count();
        let mut states: Vec<(wire::ObjectState, StateKind)> = Vec::new();
        // the moving ones, and those at rest now that were not
        for absolute_index in 0..told_count {
            let moving = a.state_moving + absolute_index as u32;
            let was_moving = m.r8(moving) != 0;
            m.w8(moving, 0);
            let Some((object_index, object)) = self.placed_object(absolute_index) else { continue };
            let at_rest = self.object_flag(object, nv::_OBJECT_AT_REST_BIT);
            m.w8(moving, !at_rest as u8);
            if at_rest && was_moving {
                m.w32(a.rest_times + 4 * absolute_index as u32, now as u32);
            }
            if at_rest && !self.rest_state_due(absolute_index) {
                continue;
            }
            let state = wire::state_from_object(m, &game, object_index);
            states.push((state, if at_rest { StateKind::ToAll } else { StateKind::Moving }));
        }
        // those at rest from the cursor on, round to it
        let mut resting = 0;
        let mut step = 0;
        while step < told_count && resting < RESTING_STATES_PER_TICK {
            let absolute_index = (m.i32(a.resting_cursor) + step) % told_count;
            step += 1;
            let Some((object_index, object)) = self.placed_object(absolute_index) else { continue };
            if !self.object_flag(object, nv::_OBJECT_AT_REST_BIT) {
                continue;
            }
            resting += 1;
            m.w32(a.resting_cursor, ((absolute_index + 1) % told_count) as u32);
            states.push((wire::state_from_object(m, &game, object_index), StateKind::ToAll));
        }
        for machine_number in 0..self.viewer_count().max(0) {
            let machine_index = self.viewer_index(machine_number);
            let machine = self.machine(machine_number);
            let mut count: i16 = 0;
            for (state, kind) in &states {
                let mut own_vehicle = NONE as i16;
                for vehicle_number in 0..m.i16(machine + vm::VEHICLE_COUNT as u32).max(0) {
                    if m.i32(machine + vm::VEHICLE_INDICES as u32 + 4 * vehicle_number as u32) == state.object_index {
                        own_vehicle = vehicle_number;
                    }
                }
                if *kind == StateKind::Moving {
                    let period = if own_vehicle != NONE as i16 {
                        OWN_VEHICLE_PERIOD_TICKS
                    } else {
                        self.object_period(machine_number, state.position.map(f32::from_bits))
                    };
                    let turn = if own_vehicle != NONE as i16 { 0 } else { state.object_index as i16 as i32 };
                    if now.wrapping_add(turn) % period as i32 != 0 {
                        continue;
                    }
                }
                let entry = a.message + HEADER + count as u32 * st::SIZEOF as u32;
                write_bytes(m, entry, &bytes_of::<_, 44>(state));
                // (its own vehicle where it had it: at which of its ticks)
                if own_vehicle != NONE as i16 {
                    let player_index = m.i16(machine + vm::VEHICLE_PLAYER_INDICES as u32 + 2 * own_vehicle as u32);
                    if player_index >= 0 && player_index < MAXIMUM_TRACKED_PLAYERS {
                        let prediction = a.vehicle_predictions + player_index as u32 * vp::SIZEOF as u32;
                        let accepted_host_time = m.i32(prediction + vp::ACCEPTED_HOST_TIME as u32);
                        if m.i32(prediction + vp::ACCEPTED_VEHICLE_INDEX as u32) == state.object_index
                            && now.wrapping_sub(accepted_host_time) < VEHICLE_PREDICTION_REFERENCE_TICKS
                        {
                            m.w8(entry + st::FLAGS as u32, m.r8(entry + st::FLAGS as u32) | FLAG_PREDICTED);
                            let accepted_time = m.r16(prediction + vp::ACCEPTED_TIME as u32) as i32;
                            let time = accepted_time.wrapping_add(now).wrapping_sub(accepted_host_time);
                            m.w16(entry + st::TIME as u32, time as u16);
                        }
                    }
                }
                count += 1;
                if count == a.state_limit {
                    self.send(machine_index, 0, a.state_type, count, HEADER + count as u32 * st::SIZEOF as u32);
                    count = 0;
                }
            }
            if count != 0 {
                self.send(machine_index, 0, a.state_type, count, HEADER + count as u32 * st::SIZEOF as u32);
            }
        }
    }

    // ---------- the inventories

    /// network_objects.c's `distributed_host_send_inventories`.
    fn send_inventories(&self) {
        let m = self.m;
        let a = &self.arguments;
        let now = self.now();
        let game = self.game();
        let round = now / INVENTORY_INTERVAL_TICKS;
        let mut inventories: Vec<(wire::Inventory, InventoryKind, [f32; 3])> = Vec::new();
        for unit_index in self.objects_of_types(nv::_OBJECT_MASK_UNIT as u32) {
            if inventories.len() >= MAXIMUM_TRACKED_OBJECTS as usize {
                break;
            }
            let unit = self.object(unit_index).unwrap_or(0);
            let absolute_index = unit_index as i16 as i32;
            let record = self.inventory(absolute_index);
            let mut carries = m.r8(unit + ud::UNIT_GRENADE_COUNTS as u32) != 0 || m.r8(unit + ud::UNIT_GRENADE_COUNTS as u32 + 1) != 0;
            for slot in 0..4 {
                carries |= m.i32(unit + ud::UNIT_WEAPON_OBJECT_INDICES as u32 + 4 * slot) != NONE;
            }
            if !self.networked(unit_index) || self.dead(unit) {
                continue;
            }
            // (one carrying nothing, as it has come to for a while: its last
            // weapon out of its hands everywhere, and then no more of it)
            let carried_time = m.i32(record + inv::CARRIED_TIME as u32);
            if carries {
                m.w32(record + inv::CARRIED_TIME as u32, now as u32);
            } else if carried_time == NONE || now.wrapping_sub(carried_time) >= EMPTY_INVENTORY_TICKS {
                continue;
            }
            let inventory = wire::inventory_from_unit(m, &game, unit_index);
            let bytes: [u8; INVENTORY_SIZE] = bytes_of(&inventory);
            let checksum = wire::checksum(&bytes);
            let weapons_checksum = wire::checksum(&bytes[..INVENTORY_WEAPONS_SIZE]);
            let ammunition_time = m.i32(record + inv::AMMUNITION_TIME as u32);
            let kind = if m.r32(record + inv::WEAPONS_CHECKSUM as u32) != weapons_checksum
                || now.wrapping_sub(m.i32(record + inv::TIME as u32)) >= INVENTORY_REFRESH_TICKS
            {
                m.w32(record + inv::TIME as u32, now as u32);
                m.w32(record + inv::AMMUNITION_TIME as u32, NONE as u32);
                InventoryKind::ToAll
            } else if m.r32(record + inv::CHECKSUM as u32) != checksum {
                m.w32(record + inv::AMMUNITION_TIME as u32, now as u32);
                InventoryKind::Ammunition
            } else if ammunition_time != NONE
                && now.wrapping_sub(ammunition_time) < MAXIMUM_OBJECT_PERIOD_TICKS as i32 * INVENTORY_INTERVAL_TICKS
            {
                InventoryKind::AmmunitionLately
            } else {
                continue;
            };
            m.w32(record + inv::CHECKSUM as u32, checksum);
            m.w32(record + inv::WEAPONS_CHECKSUM as u32, weapons_checksum);
            let origin = self.origin(unit_index);
            inventories.push((inventory, kind, origin));
        }
        for machine_number in 0..self.viewer_count().max(0) {
            let machine_index = self.viewer_index(machine_number);
            let machine = self.machine(machine_number);
            let mut count: i16 = 0;
            for (inventory, kind, origin) in &inventories {
                if *kind != InventoryKind::ToAll {
                    let absolute_index = inventory.unit_index as i16 as i32;
                    let mut own = false;
                    for unit_number in 0..m.i16(machine + vm::UNIT_COUNT as u32).max(0) {
                        own |= m.i32(machine + vm::UNIT_INDICES as u32 + 4 * unit_number as u32) == inventory.unit_index;
                    }
                    if own {
                        // (its own unit's: each change at once)
                        if *kind != InventoryKind::Ammunition {
                            continue;
                        }
                    } else {
                        // (another's: every round near its players, further
                        // every second to fourth, in the unit's turn)
                        let period = self.object_period(machine_number, *origin) as i32;
                        let ammunition_time = m.i32(self.inventory(absolute_index) + inv::AMMUNITION_TIME as u32);
                        if (round + absolute_index) % period != 0
                            || now.wrapping_sub(ammunition_time) >= period * INVENTORY_INTERVAL_TICKS
                        {
                            continue;
                        }
                    }
                }
                let entry = a.message + HEADER + count as u32 * INVENTORY_SIZE as u32;
                write_bytes(m, entry, &bytes_of::<_, INVENTORY_SIZE>(inventory));
                count += 1;
                if count == a.inventory_limit {
                    self.send(machine_index, 0, a.inventory_type, count, HEADER + count as u32 * INVENTORY_SIZE as u32);
                    count = 0;
                }
            }
            if count != 0 {
                self.send(machine_index, 0, a.inventory_type, count, HEADER + count as u32 * INVENTORY_SIZE as u32);
            }
        }
    }

    // ---------- the damage animations

    /// network_objects.c's `distributed_host_send_damage_animations`.
    fn send_damage_animations(&self) {
        let m = self.m;
        let a = &self.arguments;
        let entry = |index: i16| a.damage_animations + index as u32 * da::SIZEOF as u32;
        let mut count: i16 = 0;
        let mut index: i16 = 0;
        while index < m.i16(a.damage_animation_count) {
            let animation = entry(index);
            for offset in 0..DAMAGE_ANIMATION_SIZE {
                m.w8(a.message + HEADER + count as u32 * DAMAGE_ANIMATION_SIZE + offset, m.r8(animation + da::ANIMATION as u32 + offset));
            }
            count += 1;
            let sends = m.i16(animation + da::SENDS as u32) - 1;
            m.set_i16(animation + da::SENDS as u32, sends);
            if sends <= 0 {
                let last = m.i16(a.damage_animation_count) - 1;
                m.set_i16(a.damage_animation_count, last);
                for offset in 0..da::SIZEOF as u32 {
                    m.w8(animation + offset, m.r8(entry(last) + offset));
                }
                continue;
            }
            index += 1;
        }
        if count > 0 {
            self.send(NONE, a.to_clients, a.damage_animation_type, count, HEADER + count as u32 * DAMAGE_ANIMATION_SIZE);
        }
    }
}

/// C's `MAX` and `MIN` (the first unless the second is greater, or less).
fn max(a: f32, b: f32) -> f32 {
    if a > b { a } else { b }
}

fn min(a: f32, b: f32) -> f32 {
    if a < b { a } else { b }
}

fn distance_squared(a: [f32; 3], b: [f32; 3]) -> f32 {
    let (dx, dy, dz) = (a[0] - b[0], a[1] - b[1], a[2] - b[2]);
    dx * dx + dy * dy + dz * dz
}

/// `(real)sqrt(...)` of a vector's squared length.
fn length(v: [f32; 3]) -> f32 {
    ((v[0] * v[0] + v[1] * v[1] + v[2] * v[2]) as f64).sqrt() as f32
}

struct Vehicles<'a, H: AiHost> {
    context: &'a WorldContext,
    host: &'a H,
    m: &'a dyn AiMemory,
    arguments: VehicleArguments,
}

impl<H: AiHost> Vehicles<'_, H> {
    fn call(&self, kind: i32, a: i32, b: i32) -> AiCall {
        let mut call = AiCall { kind, actor: NONE, a, b, ..AiCall::default() };
        self.host.call(&mut call);
        call
    }

    fn object(&self, object_index: i32) -> Option<u32> {
        datum_get(self.m, self.context.object_header_data, object_index).map(|header| self.m.r32(header + oh::DATUM as u32))
    }

    fn vehicle(&self, object_index: i32) -> Option<u32> {
        if object_index == NONE {
            return None;
        }
        self.object(object_index)
            .filter(|&object| (nv::_OBJECT_MASK_VEHICLE as u32) & (1 << self.m.r8(object + ud::OBJECT_TYPE as u32)) != 0)
    }

    fn record(&self, player_index: i16) -> u32 {
        self.arguments.predictions + player_index as u32 * vp::SIZEOF as u32
    }

    fn position(&self, object: u32) -> [f32; 3] {
        self.m.vector(object + ud::OBJECT_POSITION as u32)
    }

    fn write_vector(&self, address: u32, v: [f32; 3]) {
        for (part, value) in v.iter().enumerate() {
            self.m.set_f32(address + 4 * part as u32, *value);
        }
    }

    /// The vehicle's driver's player, if the driver is a player's.
    fn driver_player(&self, vehicle: u32) -> Option<i32> {
        let driver_index = self.m.i32(vehicle + ud::UNIT_DRIVER_OBJECT_INDEX as u32);
        if driver_index == NONE {
            return None;
        }
        let driver = self.object(driver_index)?;
        let player_index = self.m.i32(driver + ud::UNIT_PLAYER_INDEX as u32);
        (player_index != NONE).then_some(player_index)
    }

    /// network_objects.c's `network_objects_note_vehicle_ground`.
    fn note_vehicle_ground(&self, player_index: i16) {
        let m = self.m;
        let record = self.record(player_index);
        let vehicle_index = m.i32(record + vp::ACCEPTED_VEHICLE_INDEX as u32);
        let vehicle = self.vehicle(vehicle_index);
        let driven = vehicle.and_then(|vehicle| self.driver_player(vehicle)).is_some_and(|player| player as i16 == player_index);
        let Some(vehicle) = vehicle.filter(|_| driven) else {
            m.w32(record + vp::GROUND_VEHICLE_INDEX as u32, NONE as u32);
            return;
        };
        // (on the ground, or one new to it: from where it is)
        if m.r8(vehicle + crate::layout::vehicle_datum::VEHICLE_AIRBORNE_TICKS as u32) == 0
            || m.i32(record + vp::GROUND_VEHICLE_INDEX as u32) != vehicle_index
        {
            m.w32(record + vp::GROUND_VEHICLE_INDEX as u32, vehicle_index as u32);
            m.w32(record + vp::GROUND_TIME as u32, self.context.game_time as u32);
            m.set_f32(record + vp::GROUND_HEIGHT as u32, self.position(vehicle)[2]);
            let velocity = m.vector(vehicle + ud::OBJECT_TRANSLATIONAL_VELOCITY as u32);
            let mut rise = max(velocity[2], 0.0);
            // (so written that a speed not a number is none)
            if !(rise <= MAXIMUM_PREDICTED_VEHICLE_SPEED) {
                rise = 0.0;
            }
            m.set_f32(record + vp::GROUND_RISE_SPEED as u32, rise);
        }
    }

    /// network_objects.c's `network_objects_vehicle_below_ceiling`.
    fn below_ceiling(&self, player_index: i16, vehicle: u32, vehicle_index: i32, position: [f32; 3]) -> bool {
        let m = self.m;
        let record = self.record(player_index);
        let kind = self.call(call::VEHICLE_DEFINITION, m.i32(vehicle + ud::DEFINITION_INDEX as u32), 0).result as i16;
        if [VEHICLE_TYPE_HUMAN_BOAT, VEHICLE_TYPE_HUMAN_PLANE, VEHICLE_TYPE_ALIEN_FIGHTER, VEHICLE_TYPE_TURRET].contains(&kind)
            || m.i32(record + vp::GROUND_VEHICLE_INDEX as u32) != vehicle_index
            || m.r8(vehicle + crate::layout::vehicle_datum::VEHICLE_AIRBORNE_TICKS as u32) == 0
        {
            return true;
        }
        let round_trip = self.call(call::ROUND_TRIP, m.i32(record + vp::MACHINE_INDEX as u32), 0).output[0];
        let ticks = self.context.game_time.wrapping_sub(m.i32(record + vp::GROUND_TIME as u32)) as f32
            - min(round_trip, VEHICLE_PREDICTION_CEILING_LEAD_TICKS)
            - VEHICLE_PREDICTION_JITTER_TICKS as f32;
        let gravity = self.arguments.gravity;
        if !(ticks > 0.0) || !(gravity > 0.0) {
            return true;
        }
        let ceiling = VEHICLE_PREDICTION_RISE_TOLERANCE
            + (m.f32(record + vp::GROUND_RISE_SPEED as u32) + PREDICTED_VEHICLE_SPEED_MARGIN) * ticks
            - 0.5 * gravity * ticks * ticks;
        // (so written that a position not a number is not below)
        position[2] - m.f32(record + vp::GROUND_HEIGHT as u32) <= ceiling
    }

    /// How fast the vehicle may go a tick: twice its tag's top speed, or as
    /// fast as the host's own ticks sent it lately (less what it took of
    /// the client beyond that top speed), the most of this span and the one
    /// before, held to the most a predicted vehicle goes.
    fn speed(&self, record: u32, vehicle: u32, object_index: i32) -> f32 {
        let m = self.m;
        let now = self.context.game_time;
        let definition = self.call(call::VEHICLE_DEFINITION, m.i32(vehicle + ud::DEFINITION_INDEX as u32), 0);
        let mut top_speed = 2.0 * max(definition.output[0], definition.output[1]);
        let mut host_speed = length(m.vector(vehicle + ud::OBJECT_TRANSLATIONAL_VELOCITY as u32));
        let speed_time = record + vp::SPEED_TIME as u32;
        let speeds = record + vp::SPEEDS as u32;
        // (so written that a tag's speed not a number is none)
        if !(top_speed >= 0.0) {
            top_speed = 0.0;
        }
        // (what the host's ticks sent it since the last taken)
        if m.i32(record + vp::ACCEPTED_VEHICLE_INDEX as u32) == object_index {
            let taken_speed = length(m.vector(record + vp::ACCEPTED_VELOCITY as u32));
            if taken_speed > top_speed {
                host_speed -= taken_speed - top_speed;
            }
        }
        // (the most of each span; so written that a speed not a number is
        // none)
        let time = m.i32(speed_time);
        if now < time || now.wrapping_sub(time) >= 2 * VEHICLE_PREDICTION_SPEED_TICKS {
            m.w32(speed_time, now as u32);
            m.set_f32(speeds, 0.0);
            m.set_f32(speeds + 4, 0.0);
        } else if now.wrapping_sub(time) >= VEHICLE_PREDICTION_SPEED_TICKS {
            m.w32(speed_time, now as u32);
            m.set_f32(speeds + 4, m.f32(speeds));
            m.set_f32(speeds, 0.0);
        }
        if host_speed > m.f32(speeds) {
            m.set_f32(speeds, host_speed);
        }
        min(max(top_speed, max(m.f32(speeds), m.f32(speeds + 4))), MAXIMUM_PREDICTED_VEHICLE_SPEED)
    }

    /// network_objects.c's `network_objects_apply_vehicle_predictions`.
    fn apply(&self) {
        let m = self.m;
        let now = self.context.game_time;
        for player_index in 0..MAXIMUM_TRACKED_PLAYERS {
            let record = self.record(player_index);
            self.note_vehicle_ground(player_index);
            if m.r8(record + vp::VALID as u32) == 0 {
                continue;
            }
            m.w8(record + vp::VALID as u32, 0);
            let mut bytes = [0u8; 44];
            for (index, byte) in bytes.iter_mut().enumerate() {
                *byte = m.r8(record + vp::STATE as u32 + index as u32);
            }
            // SAFETY: plain data of its size
            let state: wire::ObjectState = unsafe { std::mem::transmute(bytes) };
            let position = state.position.map(f32::from_bits);
            let Some(vehicle) = self.vehicle(state.object_index) else { continue };
            if m.i32(vehicle + ud::UNIT_DRIVER_OBJECT_INDEX as u32) == NONE
                || m.i32(vehicle + ud::OBJECT_PARENT_OBJECT_INDEX as u32) != NONE
            {
                continue;
            }
            let Some(driver_player) = self.driver_player(vehicle) else { continue };
            if driver_player as i16 != player_index
                || self.call(call::MACHINE_HAS_PLAYER, m.i32(record + vp::MACHINE_INDEX as u32), player_index as i32).result == 0
            {
                continue;
            }
            // (not from a co-op client still loading the host's BSP: the
            // vehicle falls there with no ground under it)
            if self.call(call::COOP_HAS_STRUCTURE_BSP, driver_player, 0).result == 0 {
                continue;
            }
            // (so written that a position not a number is not taken)
            if !(distance_squared(position, self.position(vehicle)) <= HOST_VEHICLE_ACCEPT_TOLERANCE * HOST_VEHICLE_ACCEPT_TOLERANCE) {
                continue;
            }
            // (no higher in the air than it was thrown up leaving the ground)
            if !self.below_ceiling(player_index, vehicle, state.object_index, position) {
                continue;
            }
            let speed = self.speed(record, vehicle, state.object_index);
            let mut anchored = m.i32(record + vp::ACCEPTED_VEHICLE_INDEX as u32) == state.object_index
                && now.wrapping_sub(m.i32(record + vp::ANCHOR_HOST_TIME as u32)) < VEHICLE_PREDICTION_REFERENCE_TICKS;
            if anchored {
                let host_ticks = now.wrapping_sub(m.i32(record + vp::ACCEPTED_HOST_TIME as u32));
                let reach = (speed + PREDICTED_VEHICLE_SPEED_MARGIN) * host_ticks as f32 + HOST_VEHICLE_BLEND_DISTANCE;
                let from = m.vector(record + vp::ACCEPTED_HOST_POSITION as u32);
                // (the host's copy moved further than it goes since the last
                // taken, which only the host moves it by: taken from where it
                // is, a new anchor)
                if !(distance_squared(self.position(vehicle), from) <= reach * reach) {
                    anchored = false;
                } else {
                    // (a client's clock that jumped counts no more than the
                    // host's ticks since and the jitter)
                    let anchor_time = m.r16(record + vp::ANCHOR_TIME as u32) as i32;
                    let client_ticks = ((state.time as u16 as i32 - anchor_time) as u16 as i16) as i32;
                    let ticks = client_ticks
                        .min(now.wrapping_sub(m.i32(record + vp::ANCHOR_HOST_TIME as u32)) + VEHICLE_PREDICTION_JITTER_TICKS);
                    let reach = (speed + PREDICTED_VEHICLE_SPEED_MARGIN) * ticks as f32 + HOST_VEHICLE_BLEND_DISTANCE;
                    let from = m.vector(record + vp::ANCHOR_POSITION as u32);
                    if ticks <= 0 || !(distance_squared(position, from) <= reach * reach) {
                        continue;
                    }
                }
            }
            let [forward, up, mut velocity, mut angular_velocity] = wire::object_state_unpack(&state);
            // (no faster than it goes: the margin is for where it is only)
            wire::vector_clamp(&mut velocity, speed);
            wire::vector_clamp(&mut angular_velocity, MAXIMUM_PREDICTED_VEHICLE_ANGULAR_SPEED);
            let mut valid = [0.0f32; 6];
            if !wire::transform_valid(position, forward, up, Some(velocity), Some(angular_velocity), &mut valid) {
                continue;
            }
            let previous_position = self.position(vehicle);
            // (the prediction in the arguments, for the reconcile)
            let arguments = self.context.arguments;
            self.write_vector(arguments + 8, position);
            self.write_vector(arguments + 20, [valid[0], valid[1], valid[2]]);
            self.write_vector(arguments + 32, [valid[3], valid[4], valid[5]]);
            self.write_vector(arguments + 44, velocity);
            self.write_vector(arguments + 56, angular_velocity);
            self.call(call::RECONCILE, state.object_index, 0);
            // (a copy at rest runs no physics, and so neither falls nor
            // counts its ticks in the air: one the client moved from where it
            // came to rest, or says moves, wakes, and settles again if still)
            let flags = vehicle + ud::OBJECT_FLAGS as u32;
            if m.r32(flags) & (1 << nv::_OBJECT_AT_REST_BIT) == 0 {
                m.w32(record + vp::REST_VEHICLE_INDEX as u32, NONE as u32);
            } else {
                if m.i32(record + vp::REST_VEHICLE_INDEX as u32) != state.object_index {
                    m.w32(record + vp::REST_VEHICLE_INDEX as u32, state.object_index as u32);
                    self.write_vector(record + vp::REST_POSITION as u32, previous_position);
                }
                let rest_position = m.vector(record + vp::REST_POSITION as u32);
                if !(distance_squared(self.position(vehicle), rest_position)
                    <= VEHICLE_PREDICTION_STILL_DISTANCE * VEHICLE_PREDICTION_STILL_DISTANCE)
                    || velocity[0] != 0.0
                    || velocity[1] != 0.0
                    || velocity[2] != 0.0
                {
                    m.w32(flags, m.r32(flags) & !(1 << nv::_OBJECT_AT_REST_BIT));
                    m.w32(record + vp::REST_VEHICLE_INDEX as u32, NONE as u32);
                }
            }
            m.w32(record + vp::ACCEPTED_VEHICLE_INDEX as u32, state.object_index as u32);
            m.w16(record + vp::ACCEPTED_TIME as u32, state.time as u16);
            m.w32(record + vp::ACCEPTED_HOST_TIME as u32, now as u32);
            self.write_vector(record + vp::ACCEPTED_HOST_POSITION as u32, self.position(vehicle));
            self.write_vector(record + vp::ACCEPTED_VELOCITY as u32, m.vector(vehicle + ud::OBJECT_TRANSLATIONAL_VELOCITY as u32));
            // (a new anchor: the first, one a second on, or after a jump)
            if !anchored || now.wrapping_sub(m.i32(record + vp::ANCHOR_HOST_TIME as u32)) >= VEHICLE_PREDICTION_ANCHOR_TICKS {
                m.w16(record + vp::ANCHOR_TIME as u32, state.time as u16);
                m.w32(record + vp::ANCHOR_HOST_TIME as u32, now as u32);
                self.write_vector(record + vp::ANCHOR_POSITION as u32, position);
            }
        }
    }
}

/// The world step of the host taking its clients' vehicle predictions
/// (network_objects.c's `network_objects_apply_vehicle_predictions`).
pub fn vehicle_predictions(context: &WorldContext, host: &impl AiHost) {
    let m = host.memory();
    let mut bytes = [0u8; std::mem::size_of::<VehicleArguments>()];
    for (index, byte) in bytes.iter_mut().enumerate() {
        *byte = m.r8(context.arguments + index as u32);
    }
    // SAFETY: plain data of its size
    let arguments: VehicleArguments = unsafe { std::mem::transmute(bytes) };
    Vehicles { context, host, m, arguments }.apply();
}

/// The world step of the host's tick (network_objects.c's
/// `network_objects_host_tick`).
pub fn host_tick(context: &WorldContext, host: &impl AiHost) {
    let m = host.memory();
    let host = Host { context, host, m, arguments: arguments(m, context.arguments) };
    host.update_objects();
    host.find_viewers();
    host.send_states();
    if host.now() % INVENTORY_INTERVAL_TICKS == 0 {
        host.send_inventories();
    }
    host.send_damage_animations();
}
