//! AI sync (OpenCE's port/linux/game/network_actors.c,
//! port/linux/NETCODE.md): how a client shows the host's AI.
//!
//! Only the host runs the AI. An actor drives its unit the way a player
//! does, through `unit_control`; the host notes the control each actor gave
//! its unit, and what it started once (an animation impulse, a melee attack,
//! a leap, speech, a custom animation, each numbered so a client plays it
//! once), and after its tick sends each client its units' controls and
//! states: near ones every tick, far ones less often, ones starting
//! something to everyone. A client keeps the latest of each and drives the
//! unit with it every tick, as a remote player's is; the state (seat,
//! shields and health, camouflage, running blindly, position) corrects any
//! drift.
//!
//! The steps: the host noting ([`note`]), its tick ([`host_tick`]), and a
//! client handling the damage ([`client_damage`]) and the states
//! ([`client_states`]) and driving the units ([`client_drive`]). The tables
//! are the game's memory (network_actors_records.h), each step's arguments a
//! struct the game passes; what they end in (sending, seating, the damage
//! state, moving a unit, `unit_control`, the actions, speech) is the game's,
//! through [`AiHost::call`] (network_actors.c's `network_actors_call`).

use crate::ai_actor::{datum_get, AiCall, AiHost, AiMemory, AiMemoryExt, NONE, WorldContext};
use crate::distributed_wire as wire;
use crate::layout::{
    client_actor as ca, distributed_actor_damage as dd, distributed_actor_damage_message as dm,
    distributed_actor_state as ds, distributed_actor_state_message as sm, host_actor as ha, net_values as nv,
    network_actors_client as nc, network_actors_host as nh, network_actors_host_inputs as hi,
    network_actors_host_sends as hs, object_header_datum as oh, unit_control_data as uc, unit_datum as ud,
};

/// network_actors.c's calls (halo_sim.h's `_network_actors_call_*`).
pub mod call {
    /// How often, in ticks, a unit at `input[0..3]` goes to the machine
    /// `a`: the result.
    pub const SEND_PERIOD: i32 = 1;
    /// The damage message's `a` entries, to every client.
    pub const SEND_DAMAGE: i32 = 2;
    /// The state message's `b` entries, to the machine `a`.
    pub const SEND_STATES: i32 = 3;
    /// The unit `a` got back up (`unit_port_resurrect`).
    pub const RESURRECT: i32 = 4;
    /// The unit `a` seated in the vehicle `b`'s seat `actor`, or out of it
    /// (NONE).
    pub const SET_SEAT: i32 = 5;
    /// `damage_set_network_state(a)`: the shields depleted, charging and
    /// over charging (`b`'s bits 0 to 2), the body and shield vitalities,
    /// the current and recent body damage and the current and recent
    /// shield damage (`input[0..6]`).
    pub const SET_DAMAGE: i32 = 6;
    /// The unit `a` moved toward where the client's table says
    /// (`network_objects_reconcile`): the result.
    pub const RECONCILE: i32 = 7;
    /// `distributed_count_correction`.
    pub const COUNT_CORRECTION: i32 = 8;
    /// `unit_set_actively_controlled(a, b)`.
    pub const SET_ACTIVELY_CONTROLLED: i32 = 9;
    /// `unit_control(a, the control at the address b)`.
    pub const UNIT_CONTROL: i32 = 10;
    /// The action `actor` (an animation impulse, a melee attack or a leap)
    /// started on the unit `a`, turned to `input[0..2]` if `b`.
    pub const ACTION: i32 = 11;
    /// The sound `b` played at the unit `a`'s head.
    pub const SPEECH: i32 = 12;
    /// The unit `a`'s custom animation `actor` of the graph `b`,
    /// interpolated if `input[0]` isn't 0.
    pub const USER_ANIMATION: i32 = 13;
}

/// halo_sim.h's `_network_actors_note_*`.
const NOTE_CONTROL: i32 = 0;
const NOTE_ACTION: i32 = 1;
const NOTE_SPEECH: i32 = 2;
const NOTE_USER_ANIMATION: i32 = 3;

/// network_actors.c's `CONTROL_HELD_TICKS`: a client keeps applying a
/// control for this long after the host last sent one.
const CONTROL_HELD_TICKS: i32 = 2 * 30;
/// network_actors.c's `IMPULSE_REPEAT_TICKS`: an impulse is repeated in
/// this many entries, in case one is lost.
const IMPULSE_REPEAT_TICKS: i16 = 3;
/// The number of `unit_start_animation_impulse` impulses (private to
/// units.c); past them, a melee attack and a leap (network_actors.c's
/// `_actor_action_melee`, `_actor_action_leap`).
const NUMBER_OF_UNIT_ANIMATION_IMPULSES: i32 = 14;
const NUMBER_OF_ACTOR_ACTIONS: u8 = (NUMBER_OF_UNIT_ANIMATION_IMPULSES + 2) as u8;
const NO_IMPULSE: u8 = 0xff;
const NO_TEAM: u8 = 0xff;
/// The game's 10 teams (game_allegiance.c).
const NUMBER_OF_TEAMS: u8 = 10;
/// network_actors.c's `ONE_SHOT_CONTROL_FLAGS`: control flags a unit acts
/// on in one tick.
const ONE_SHOT_CONTROL_FLAGS: u16 = (1 << nv::_UNIT_CONTROL_JUMP_BIT)
    | (1 << nv::_UNIT_CONTROL_ACTION_BIT)
    | (1 << nv::_UNIT_CONTROL_WEAPON_RELOAD_BIT)
    | (1 << nv::_UNIT_CONTROL_THROW_GRENADE_BIT)
    | (1 << nv::_UNIT_CONTROL_SWAP_WEAPONS_BIT);
const MAXIMUM_NETWORK_ACTORS: i16 = nv::MAXIMUM_NETWORK_ACTORS as i16;
const MAXIMUM_ENTRIES_PER_MESSAGE: i16 = nv::MAXIMUM_ACTOR_ENTRIES_PER_MESSAGE as i16;
const DATAGRAM_ENTRIES: i16 = nv::ACTOR_STATE_DATAGRAM_ENTRIES as i16;
/// network_objects.c's `MAXIMUM_TRACKED_OBJECTS`.
const TRACKED_OBJECTS: i32 = nv::MAXIMUM_TRACKED_OBJECTS as i32;
/// network_distributed.h's scales and bounds.
const VELOCITY_SCALE: f32 = 1024.0;
const UNIT_WORLD_BOUND: f32 = 32768.0;
const REMOTE_CORRECTION_TOLERANCE: f32 = 0.05;

/// struct distributed_actor_state flags.
mod flag {
    pub const RIDES: u8 = 1 << 0;
    pub const SHIELD_DEPLETED: u8 = 1 << 1;
    pub const SHIELD_CHARGING: u8 = 1 << 2;
    pub const SHIELD_OVER_CHARGING: u8 = 1 << 3;
    pub const CAMOUFLAGED: u8 = 1 << 4;
    pub const SUPER_CAMOUFLAGED: u8 = 1 << 5;
    pub const IMPULSE_ALIGNED: u8 = 1 << 6;
    pub const RUNNING_BLINDLY: u8 = 1 << 7;
}

/// halo_sim.h's `struct halo_sim_network_actors_note`.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct NoteArguments {
    pub kind: i32,
    pub unit_index: i32,
    pub host: u32,
    pub action: i32,
    pub aligned: i32,
    pub alignment_i: f32,
    pub alignment_j: f32,
    pub sound: i32,
    pub graph: i32,
    pub animation: i32,
    pub interpolate: i32,
    pub control: [u8; uc::SIZEOF],
}

/// halo_sim.h's `struct halo_sim_network_actors_host_tick`.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct HostTickArguments {
    pub host: u32,
    pub sends: u32,
    pub inputs: u32,
}

/// halo_sim.h's `struct halo_sim_network_actors_client`.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct ClientArguments {
    pub count: i32,
    pub entries: u32,
    pub client: u32,
    pub client_has: u32,
    pub tag_instances: u32,
    pub tag_count: i32,
    pub now: i32,
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

/// `PIN`: held between the bounds (a NaN passes through, as the macro's
/// comparisons leave it).
fn pin(value: f32, low: f32, high: f32) -> f32 {
    if value < low {
        low
    } else if value > high {
        high
    } else {
        value
    }
}

/// `(long)floor(x)` of a float, as the C converts it.
fn floor_long(value: f32) -> i32 {
    (value as f64).floor() as i32
}

/// network_actors.c's `angles_pack`: a vector as its yaw and pitch.
fn angles_pack(vector: [f32; 3]) -> [i16; 2] {
    [
        wire::angle_pack((vector[1] as f64).atan2(vector[0] as f64) as f32),
        wire::angle_pack((pin(vector[2], -1.0, 1.0) as f64).asin() as f32),
    ]
}

/// network_actors.c's `angles_unpack`.
fn angles_unpack(angles: [i16; 2]) -> [f32; 3] {
    let yaw = wire::angle_unpack(angles[0], false) as f64;
    let pitch = wire::angle_unpack(angles[1], true) as f64;
    [(pitch.cos() * yaw.cos()) as f32, (pitch.cos() * yaw.sin()) as f32, pitch.sin() as f32]
}

/// What the steps share: the game's memory, its calls and the objects.
struct Actors<'a, H: AiHost> {
    context: &'a WorldContext,
    host: &'a H,
    m: &'a dyn AiMemory,
}

impl<H: AiHost> Actors<'_, H> {
    fn call(&self, kind: i32, actor: i32, a: i32, b: i32, input: &[f32]) -> i32 {
        let mut call = AiCall { kind, actor, a, b, ..AiCall::default() };
        call.input[..input.len()].copy_from_slice(input);
        self.host.call(&mut call);
        call.result
    }

    /// `object_try_and_get_and_verify_type`: the object's datum, if it is
    /// of a type in the mask.
    fn typed(&self, object_index: i32, mask: u32) -> Option<u32> {
        let header = datum_get(self.m, self.context.object_header_data, object_index)?;
        (mask & (1 << self.m.r8(header + oh::TYPE as u32)) != 0).then(|| self.m.r32(header + oh::DATUM as u32))
    }

    fn object_type(&self, object_index: i32) -> u8 {
        datum_get(self.m, self.context.object_header_data, object_index).map_or(0xff, |header| self.m.r8(header + oh::TYPE as u32))
    }

    fn unit(&self, unit_index: i32) -> Option<u32> {
        self.typed(unit_index, nv::_OBJECT_MASK_UNIT as u32)
    }

    fn damage_flag(&self, object: u32, bit: usize) -> bool {
        self.m.r16(object + ud::OBJECT_DAMAGE_FLAGS as u32) as u32 & (1 << bit) != 0
    }

    fn unit_flag(&self, unit: u32, bit: usize) -> bool {
        self.m.r32(unit + ud::UNIT_FLAGS as u32) & (1 << bit) != 0
    }

    fn set_unit_flag(&self, unit: u32, bit: usize, value: bool) {
        let flags = self.m.r32(unit + ud::UNIT_FLAGS as u32);
        self.m.w32(unit + ud::UNIT_FLAGS as u32, if value { flags | (1 << bit) } else { flags & !(1 << bit) });
    }

    /// network_actors.c's `actor_unit_valid`: whether the unit exists, is
    /// alive, and isn't a player's.
    fn actor_unit_valid(&self, unit_index: i32) -> bool {
        self.unit(unit_index).is_some_and(|unit| {
            !self.damage_flag(unit, nv::_OBJECT_DEAD_BIT) && self.m.i16(unit + ud::UNIT_PLAYER_INDEX as u32) == NONE as i16
        })
    }

    /// `damage_get_network_state`: the shields' flags (depleted, charging,
    /// over charging as bits 0 to 2) and the body and shield vitalities,
    /// the current and recent body damage and the current and recent shield
    /// damage.
    fn damage_state(&self, object: u32) -> (i32, [f32; 6]) {
        let flags = self.damage_flag(object, nv::_OBJECT_SHIELD_DEPLETED_BIT) as i32
            | (self.damage_flag(object, nv::_OBJECT_SHIELD_CHARGING_BIT) as i32) << 1
            | (self.damage_flag(object, nv::_OBJECT_SHIELD_OVER_CHARGING_BIT) as i32) << 2;
        let real = |field: usize| self.m.f32(object + field as u32);
        (
            flags,
            [
                real(ud::OBJECT_BODY_VITALITY),
                real(ud::OBJECT_SHIELD_VITALITY),
                real(ud::OBJECT_CURRENT_BODY_DAMAGE),
                real(ud::OBJECT_RECENT_BODY_DAMAGE),
                real(ud::OBJECT_CURRENT_SHIELD_DAMAGE),
                real(ud::OBJECT_RECENT_SHIELD_DAMAGE),
            ],
        )
    }
}

// ---------- the host

/// network_actors.c's `host_actor_for`: the unit's entry, adding one if
/// needed (None if the table is full).
fn host_actor_for(m: &dyn AiMemory, table: u32, unit_index: i32) -> Option<u32> {
    let count = m.i16(table + nh::COUNT as u32);
    let entry = |index: i16| table + nh::ACTORS as u32 + index as u32 * ha::SIZEOF as u32;
    if let Some(index) = (0..count.max(0)).find(|&index| m.i32(entry(index) + ha::UNIT_INDEX as u32) == unit_index) {
        return Some(entry(index));
    }
    if count >= MAXIMUM_NETWORK_ACTORS {
        return None;
    }
    let actor = entry(count);
    for offset in 0..ha::SIZEOF as u32 {
        m.w8(actor + offset, 0);
    }
    m.w32(actor + ha::UNIT_INDEX as u32, unit_index as u32);
    m.set_i16(actor + ha::IMPULSE as u32, NONE as i16);
    m.w32(actor + ha::SPEECH_SOUND as u32, NONE as u32);
    m.w32(actor + ha::USER_ANIMATION_GRAPH as u32, NONE as u32);
    m.set_i16(table + nh::COUNT as u32, count + 1);
    Some(actor)
}

/// network_actors.c's `next_event_number`: never 0, which a client's new
/// entry has as played.
fn next_event_number(m: &dyn AiMemory, table: u32) -> u8 {
    let mut number = m.r8(table + nh::EVENT_NUMBER as u32).wrapping_add(1);
    if number == 0 {
        number = 1;
    }
    m.w8(table + nh::EVENT_NUMBER as u32, number);
    number
}

/// The world step of the host noting what an actor gave its unit or
/// started it on: network_actors.c's `network_actors_note_control`,
/// `_note_impulse`, `_note_melee`, `_note_leap`, `_note_speech` and
/// `_note_user_animation`, each kept to be sent in the next entries.
pub fn note(context: &WorldContext, host: &impl AiHost) {
    let m = host.memory();
    let a: NoteArguments = arguments(context, m);
    let Some(actor) = host_actor_for(m, a.host, a.unit_index) else { return };
    match a.kind {
        NOTE_CONTROL => {
            m.w8(actor + ha::NOTED as u32, 1);
            for (offset, byte) in a.control.iter().enumerate() {
                m.w8(actor + ha::CONTROL as u32 + offset as u32, *byte);
            }
        }
        NOTE_ACTION => {
            m.set_i16(actor + ha::IMPULSE as u32, a.action as i16);
            m.set_i16(actor + ha::IMPULSE_SENDS as u32, IMPULSE_REPEAT_TICKS);
            m.w8(actor + ha::IMPULSE_NUMBER as u32, next_event_number(m, a.host));
            m.w8(actor + ha::IMPULSE_ALIGNED as u32, (a.aligned != 0) as u8);
            if a.aligned != 0 {
                m.set_f32(actor + ha::IMPULSE_ALIGNMENT as u32, a.alignment_i);
                m.set_f32(actor + ha::IMPULSE_ALIGNMENT as u32 + 4, a.alignment_j);
            }
        }
        NOTE_SPEECH => {
            m.w32(actor + ha::SPEECH_SOUND as u32, a.sound as u32);
            m.set_i16(actor + ha::SPEECH_SENDS as u32, IMPULSE_REPEAT_TICKS);
            m.w8(actor + ha::SPEECH_NUMBER as u32, next_event_number(m, a.host));
        }
        NOTE_USER_ANIMATION => {
            m.w32(actor + ha::USER_ANIMATION_GRAPH as u32, a.graph as u32);
            m.set_i16(actor + ha::USER_ANIMATION as u32, a.animation as i16);
            m.w8(actor + ha::USER_ANIMATION_INTERPOLATE as u32, (a.interpolate != 0) as u8);
            m.set_i16(actor + ha::USER_ANIMATION_SENDS as u32, IMPULSE_REPEAT_TICKS);
            m.w8(actor + ha::USER_ANIMATION_NUMBER as u32, next_event_number(m, a.host));
        }
        _ => {}
    }
}

impl<H: AiHost> Actors<'_, H> {
    /// network_actors.c's `actor_state_from_unit`: the entry to send for an
    /// AI unit, written at `state`.
    fn state_from_unit(&self, actor: u32, state: u32) {
        let m = self.m;
        let unit_index = m.i32(actor + ha::UNIT_INDEX as u32);
        let Some(unit) = self.unit(unit_index) else { return };
        let control = actor + ha::CONTROL as u32;
        let camouflage = pin(m.f32(unit + ud::UNIT_ACTIVE_CAMOUFLAGE as u32), 0.0, 1.0);
        let at = |field: usize| state + field as u32;
        for offset in 0..ds::SIZEOF as u32 {
            m.w8(state + offset, 0);
        }
        let mut flags = 0u8;
        m.w32(at(ds::UNIT_INDEX), unit_index as u32);
        m.w32(at(ds::VEHICLE_INDEX), NONE as u32);
        m.set_i16(at(ds::SEAT_INDEX), NONE as i16);
        let parent = m.i32(unit + ud::OBJECT_PARENT_OBJECT_INDEX as u32);
        let seat = m.i16(unit + ud::UNIT_PARENT_SEAT_INDEX as u32);
        if parent != NONE && seat != NONE as i16 {
            flags |= flag::RIDES;
            m.w32(at(ds::VEHICLE_INDEX), parent as u32);
            m.set_i16(at(ds::SEAT_INDEX), seat);
        }
        m.w8(at(ds::ANIMATION_STATE), m.r8(control + uc::ANIMATION_STATE as u32));
        m.w8(at(ds::AIMING_SPEED), m.r8(control + uc::AIMING_SPEED as u32));
        let trigger = |value: f32| floor_long(pin(value, 0.0, 1.0) * 255.0 + 0.5) as u8;
        m.w8(at(ds::PRIMARY_TRIGGER), trigger(m.f32(control + uc::PRIMARY_TRIGGER as u32)));
        m.w16(at(ds::CONTROL_FLAGS), m.r16(control + uc::CONTROL_FLAGS as u32));
        // (for a burst the AI holds the trigger for, the flags and trigger
        // the unit's update produced this tick)
        if m.i32(unit + ud::UNIT_PERSISTENT_CONTROL_TIMER as u32) > 0 {
            let mask = (1u32 << nv::NUMBER_OF_UNIT_CONTROL_FLAGS) - 1;
            m.w16(at(ds::CONTROL_FLAGS), (m.r32(unit + ud::UNIT_CONTROL_FLAGS as u32) & mask) as u16);
            m.w8(at(ds::PRIMARY_TRIGGER), trigger(m.f32(unit + ud::UNIT_PRIMARY_TRIGGER as u32)));
        }
        let throttle = m.vector(control + uc::THROTTLE as u32);
        for (part, value) in throttle.iter().enumerate() {
            m.w8(at(ds::THROTTLE) + part as u32, floor_long(pin(*value, -1.0, 1.0) * 127.0 + 0.5) as i8 as u8);
        }
        for (field, vector) in [(ds::FACING, uc::FACING_VECTOR), (ds::AIMING, uc::AIMING_VECTOR), (ds::LOOKING, uc::LOOKING_VECTOR)] {
            let angles = angles_pack(m.vector(control + vector as u32));
            m.set_i16(at(field), angles[0]);
            m.set_i16(at(field) + 2, angles[1]);
        }
        m.w8(at(ds::IMPULSE), NO_IMPULSE);
        if m.i16(actor + ha::IMPULSE as u32) != NONE as i16 && m.i16(actor + ha::IMPULSE_SENDS as u32) > 0 {
            m.w8(at(ds::IMPULSE), m.i16(actor + ha::IMPULSE as u32) as u8);
            m.w8(at(ds::IMPULSE_NUMBER), m.r8(actor + ha::IMPULSE_NUMBER as u32));
            if m.r8(actor + ha::IMPULSE_ALIGNED as u32) != 0 {
                flags |= flag::IMPULSE_ALIGNED;
                let alignment = actor + ha::IMPULSE_ALIGNMENT as u32;
                let yaw = (m.f32(alignment + 4) as f64).atan2(m.f32(alignment) as f64) as f32;
                m.set_i16(at(ds::IMPULSE_ALIGNMENT), wire::angle_pack(yaw));
            }
        }
        m.w32(at(ds::SPEECH_SOUND), NONE as u32);
        if m.i32(actor + ha::SPEECH_SOUND as u32) != NONE && m.i16(actor + ha::SPEECH_SENDS as u32) > 0 {
            m.w32(at(ds::SPEECH_SOUND), m.r32(actor + ha::SPEECH_SOUND as u32));
            m.w8(at(ds::SPEECH_NUMBER), m.r8(actor + ha::SPEECH_NUMBER as u32));
        }
        m.w32(at(ds::USER_ANIMATION_GRAPH), NONE as u32);
        if m.i32(actor + ha::USER_ANIMATION_GRAPH as u32) != NONE && m.i16(actor + ha::USER_ANIMATION_SENDS as u32) > 0 {
            m.w32(at(ds::USER_ANIMATION_GRAPH), m.r32(actor + ha::USER_ANIMATION_GRAPH as u32));
            m.w16(at(ds::USER_ANIMATION), m.r16(actor + ha::USER_ANIMATION as u32));
            m.w8(at(ds::USER_ANIMATION_INTERPOLATE), m.r8(actor + ha::USER_ANIMATION_INTERPOLATE as u32));
            m.w8(at(ds::USER_ANIMATION_NUMBER), m.r8(actor + ha::USER_ANIMATION_NUMBER as u32));
        }
        if self.unit_flag(unit, nv::_UNIT_RUNNING_BLINDLY_BIT) {
            flags |= flag::RUNNING_BLINDLY;
            m.set_i16(at(ds::RUN_BLINDLY_ANGLE), wire::angle_pack(m.f32(unit + ud::UNIT_RUN_BLINDLY_ANGLE as u32)));
        }
        let (damage_flags, damage) = self.damage_state(unit);
        if damage_flags & 1 != 0 {
            flags |= flag::SHIELD_DEPLETED;
        }
        if damage_flags & 2 != 0 {
            flags |= flag::SHIELD_CHARGING;
        }
        if damage_flags & 4 != 0 {
            flags |= flag::SHIELD_OVER_CHARGING;
        }
        m.w16(at(ds::BODY_VITALITY), wire::vitality_pack(damage[0]));
        m.w16(at(ds::SHIELD_VITALITY), wire::vitality_pack(damage[1]));
        if self.unit_flag(unit, nv::_UNIT_ACTIVE_CAMOUFLAGED_BIT) {
            flags |= flag::CAMOUFLAGED;
        }
        if self.unit_flag(unit, nv::_UNIT_SUPER_CAMOUFLAGED_BIT) {
            flags |= flag::SUPER_CAMOUFLAGED;
        }
        m.w8(at(ds::FLAGS), flags);
        m.w8(at(ds::ACTIVE_CAMOUFLAGE), floor_long(camouflage * 255.0 + 0.5) as u8);
        let team = m.i16(unit + ud::OBJECT_OWNER_TEAM_INDEX as u32);
        m.w8(at(ds::TEAM), if team >= 0 && team < NO_TEAM as i16 { team as u8 } else { NO_TEAM });
        for offset in (0..12).step_by(4) {
            m.w32(at(ds::POSITION) + offset, m.r32(unit + ud::OBJECT_POSITION as u32 + offset));
        }
        for (field, vector, scale) in [
            (ds::VELOCITY, ud::OBJECT_TRANSLATIONAL_VELOCITY, VELOCITY_SCALE),
            (ds::FORWARD, ud::OBJECT_FORWARD, wire::UNIT_SCALE),
            (ds::UP, ud::OBJECT_UP, wire::UNIT_SCALE),
        ] {
            let packed = wire::vector_pack(m.vector(unit + vector as u32), scale);
            for (part, value) in packed.iter().enumerate() {
                m.set_i16(at(field) + 2 * part as u32, *value);
            }
        }
    }

    /// network_actors.c's `host_send_damage`: the damage of the AI units
    /// taking some, to every client (and once more when it ends, so a flare
    /// goes).
    fn send_damage(&self, table: u32, sends: u32) {
        let m = self.m;
        let message = sends + hs::DAMAGE_MESSAGE as u32 + dm::UNITS as u32;
        let mut count: i16 = 0;
        for index in 0..m.i16(table + nh::COUNT as u32).max(0) as u32 {
            let actor = table + nh::ACTORS as u32 + index * ha::SIZEOF as u32;
            let unit_index = m.i32(actor + ha::UNIT_INDEX as u32);
            if !self.actor_unit_valid(unit_index) {
                continue;
            }
            let Some(unit) = self.unit(unit_index) else { continue };
            let (_, damage) = self.damage_state(unit);
            let damaged = damage[4] > 0.0 || damage[5] > 0.0 || damage[2] > 0.0 || damage[3] > 0.0;
            if !damaged && m.r8(actor + ha::DAMAGED as u32) == 0 {
                continue;
            }
            m.w8(actor + ha::DAMAGED as u32, damaged as u8);
            let entry = message + count as u32 * dd::SIZEOF as u32;
            m.w32(entry + dd::UNIT_INDEX as u32, unit_index as u32);
            m.w16(entry + dd::CURRENT_SHIELD_DAMAGE as u32, wire::vitality_pack(damage[4]));
            m.w16(entry + dd::RECENT_SHIELD_DAMAGE as u32, wire::vitality_pack(damage[5]));
            m.w16(entry + dd::CURRENT_BODY_DAMAGE as u32, wire::vitality_pack(damage[2]));
            m.w16(entry + dd::RECENT_BODY_DAMAGE as u32, wire::vitality_pack(damage[3]));
            count += 1;
            if count == MAXIMUM_ENTRIES_PER_MESSAGE {
                self.call(call::SEND_DAMAGE, NONE, count as i32, 0, &[]);
                count = 0;
            }
        }
        if count != 0 {
            self.call(call::SEND_DAMAGE, NONE, count as i32, 0, &[]);
        }
    }
}

/// The world step of the host's tick: network_actors.c's
/// `network_actors_host_tick`. Each AI unit driven this tick (or fleeing,
/// which moves without its actor) goes to each client: near ones every
/// tick, far ones less often, one with a fresh impulse, action, speech or
/// animation, or starting a one-tick action, to everyone. A unit no actor
/// drove is dropped from the table. The damage the units are taking goes
/// to every client.
pub fn host_tick(context: &WorldContext, host: &impl AiHost) {
    let m = host.memory();
    let a: HostTickArguments = arguments(context, m);
    let actors = Actors { context, host, m };
    let (table, sends) = (a.host, a.sends);
    let now = m.i32(a.inputs + hi::NOW as u32);
    let entry = |index: i16| table + nh::ACTORS as u32 + index as u32 * ha::SIZEOF as u32;
    let state_at = |index: i16| sends + hs::STATES as u32 + index as u32 * ds::SIZEOF as u32;
    let mut state_count: i16 = 0;
    let mut index: i16 = 0;
    while index < m.i16(table + nh::COUNT as u32) {
        let actor = entry(index);
        let unit_index = m.i32(actor + ha::UNIT_INDEX as u32);
        let running_blindly = |unit_index: i32| {
            actors.unit(unit_index).is_some_and(|unit| actors.unit_flag(unit, nv::_UNIT_RUNNING_BLINDLY_BIT))
        };
        if !actors.actor_unit_valid(unit_index) || (m.r8(actor + ha::NOTED as u32) == 0 && !running_blindly(unit_index)) {
            let count = m.i16(table + nh::COUNT as u32) - 1;
            m.set_i16(table + nh::COUNT as u32, count);
            let last = entry(count);
            for offset in 0..ha::SIZEOF as u32 {
                m.w8(actor + offset, m.r8(last + offset));
            }
            continue;
        }
        let state = state_at(state_count);
        actors.state_from_unit(actor, state);
        let control_flags = m.r16(state + ds::CONTROL_FLAGS as u32);
        let urgent = m.r8(state + ds::IMPULSE as u32) != NO_IMPULSE
            || m.i32(state + ds::SPEECH_SOUND as u32) != NONE
            || m.i32(state + ds::USER_ANIMATION_GRAPH as u32) != NONE
            || control_flags & !m.r16(actor + ha::SENT_CONTROL_FLAGS as u32) & ONE_SHOT_CONTROL_FLAGS != 0;
        m.w8(sends + hs::URGENT as u32 + state_count as u32, urgent as u8);
        m.w16(actor + ha::SENT_CONTROL_FLAGS as u32, control_flags);
        state_count += 1;
        m.w8(actor + ha::NOTED as u32, 0);
        for field in [ha::IMPULSE_SENDS, ha::SPEECH_SENDS, ha::USER_ANIMATION_SENDS] {
            let sends = m.i16(actor + field as u32);
            if sends > 0 {
                m.set_i16(actor + field as u32, sends - 1);
            }
        }
        index += 1;
    }
    actors.send_damage(table, sends);
    let message = sends + hs::STATE_MESSAGE as u32 + sm::STATES as u32;
    for machine_number in 0..m.i16(a.inputs + hi::MACHINE_COUNT as u32).max(0) as u32 {
        let machine_index = m.i32(a.inputs + hi::MACHINE_INDICES as u32 + 4 * machine_number);
        let mut count: i16 = 0;
        for index in 0..state_count {
            let state = state_at(index);
            let position = m.vector(state + ds::POSITION as u32);
            let period = actors.call(call::SEND_PERIOD, NONE, machine_index, 0, &position) as i16;
            let absolute_index = m.i32(state + ds::UNIT_INDEX as u32) & 0xffff;
            if m.r8(sends + hs::URGENT as u32 + index as u32) == 0 && now.wrapping_add(absolute_index) % period as i32 != 0 {
                continue;
            }
            let entry = message + count as u32 * ds::SIZEOF as u32;
            for offset in 0..ds::SIZEOF as u32 {
                m.w8(entry + offset, m.r8(state + offset));
            }
            count += 1;
            if count == MAXIMUM_ENTRIES_PER_MESSAGE || count == DATAGRAM_ENTRIES {
                actors.call(call::SEND_STATES, NONE, machine_index, count as i32, &[]);
                count = 0;
            }
        }
        if count != 0 {
            actors.call(call::SEND_STATES, NONE, machine_index, count as i32, &[]);
        }
    }
}

// ---------- a client

impl<H: AiHost> Actors<'_, H> {
    /// `distributed_object_index_valid` and `network_objects_client_has`:
    /// an object the client has from the host.
    fn client_has(&self, a: &ClientArguments, object_index: i32) -> bool {
        let absolute_index = object_index & 0xffff;
        object_index != NONE
            && object_index >> 16 != 0
            && absolute_index < TRACKED_OBJECTS
            && self.m.i32(a.client_has + 4 * absolute_index as u32) == object_index
            && self.typed(object_index, u32::MAX).is_some()
    }

    /// The damage state set: the shields' flags as the entry says, the
    /// vitalities and damage given.
    fn set_damage(&self, unit_index: i32, flags: i32, damage: &[f32; 6]) {
        self.call(call::SET_DAMAGE, NONE, unit_index, flags, damage);
    }
}

/// network_actors.c's `client_actor_for`.
fn client_actor_for(m: &dyn AiMemory, table: u32, unit_index: i32) -> Option<u32> {
    let count = m.i16(table + nc::COUNT as u32);
    let entry = |index: i16| table + nc::ACTORS as u32 + index as u32 * ca::SIZEOF as u32;
    if let Some(index) = (0..count.max(0)).find(|&index| m.i32(entry(index) + ca::UNIT_INDEX as u32) == unit_index) {
        return Some(entry(index));
    }
    if count >= MAXIMUM_NETWORK_ACTORS {
        return None;
    }
    let actor = entry(count);
    for offset in 0..ca::SIZEOF as u32 {
        m.w8(actor + offset, 0);
    }
    m.w32(actor + ca::UNIT_INDEX as u32, unit_index as u32);
    m.set_i16(actor + ca::IMPULSE as u32, NONE as i16);
    m.w32(actor + ca::SPEECH_SOUND as u32, NONE as u32);
    m.w32(actor + ca::USER_ANIMATION_GRAPH as u32, NONE as u32);
    m.set_i16(table + nc::COUNT as u32, count + 1);
    Some(actor)
}

/// The world step of a client handling the damage the host's AI units are
/// taking: network_actors.c's `network_actors_handle_damage` (the shields'
/// flare and the body's wounds they draw).
pub fn client_damage(context: &WorldContext, host: &impl AiHost) {
    let m = host.memory();
    let a: ClientArguments = arguments(context, m);
    let actors = Actors { context, host, m };
    for index in 0..a.count.max(0) as u32 {
        let entry = a.entries + index * dd::SIZEOF as u32;
        let unit_index = m.i32(entry + dd::UNIT_INDEX as u32);
        if !actors.client_has(&a, unit_index) || !actors.actor_unit_valid(unit_index) {
            continue;
        }
        let Some(unit) = actors.unit(unit_index) else { continue };
        let (flags, mut damage) = actors.damage_state(unit);
        damage[4] = wire::vitality_unpack(m.r16(entry + dd::CURRENT_SHIELD_DAMAGE as u32));
        damage[5] = wire::vitality_unpack(m.r16(entry + dd::RECENT_SHIELD_DAMAGE as u32));
        damage[2] = wire::vitality_unpack(m.r16(entry + dd::CURRENT_BODY_DAMAGE as u32));
        damage[3] = wire::vitality_unpack(m.r16(entry + dd::RECENT_BODY_DAMAGE as u32));
        actors.set_damage(unit_index, flags, &damage);
    }
}

impl<H: AiHost> Actors<'_, H> {
    /// network_distributed.c's `distributed_graph_animation`: whether the
    /// graph is an animation graph with that animation.
    fn graph_animation(&self, tags: &crate::ai_actor::AiStepContext, graph_index: i32, animation_index: i16) -> bool {
        if !crate::sensing::tag_of_group(self.m, tags, crate::fourcc(b"antr"), graph_index) {
            return false;
        }
        let Some(graph) = crate::sensing::tag_get(self.m, tags, crate::fourcc(b"antr"), graph_index) else { return false };
        let count = crate::ai_actor::tag_block_count(self.m, graph + crate::layout::animation_graph::ANIMATIONS as u32);
        (0..count).contains(&(animation_index as i32))
    }

    /// network_actors.c's `actor_state_apply`: the entry's control kept (to
    /// drive the unit with every tick until the next), what it starts once
    /// kept to play if not played, and its state applied to the unit.
    /// False for an entry with invalid values.
    fn state_apply(&self, a: &ClientArguments, tags: &crate::ai_actor::AiStepContext, state: u32) -> bool {
        let m = self.m;
        let at = |field: usize| state + field as u32;
        let unit_index = m.i32(at(ds::UNIT_INDEX));
        if !self.client_has(a, unit_index) {
            return false;
        }
        // (a body the host's unit got back up from: a Flood combat form that
        // feigned death, at rest; the host sends only living units)
        if let Some(unit) = self.unit(unit_index) {
            if self.damage_flag(unit, nv::_OBJECT_DEAD_BIT)
                && m.r32(unit + ud::OBJECT_FLAGS as u32) & (1 << nv::_OBJECT_AT_REST_BIT) != 0
                && m.i16(unit + ud::UNIT_PLAYER_INDEX as u32) == NONE as i16
                && m.r16(at(ds::BODY_VITALITY)) > 0
            {
                self.call(call::RESURRECT, NONE, unit_index, 0, &[]);
            }
        }
        let impulse = m.r8(at(ds::IMPULSE));
        let speech_sound = m.i32(at(ds::SPEECH_SOUND));
        let graph = m.i32(at(ds::USER_ANIMATION_GRAPH));
        let control_flags = m.r16(at(ds::CONTROL_FLAGS)) as u32;
        let position = m.vector(at(ds::POSITION));
        if !self.actor_unit_valid(unit_index)
            || m.r8(at(ds::ANIMATION_STATE)) as usize >= nv::NUMBER_OF_UNIT_ANIMATION_STATES
            || m.r8(at(ds::AIMING_SPEED)) as usize >= nv::NUMBER_OF_UNIT_AIMING_SPEEDS
            || control_flags & !((1u32 << nv::NUMBER_OF_UNIT_CONTROL_FLAGS) - 1) != 0
            || (impulse != NO_IMPULSE && impulse >= NUMBER_OF_ACTOR_ACTIONS)
            || (speech_sound != NONE && !crate::sensing::tag_of_group(m, tags, crate::fourcc(b"snd!"), speech_sound))
            || (graph != NONE && !self.graph_animation(tags, graph, m.i16(at(ds::USER_ANIMATION))))
            || !wire::point_valid(position, UNIT_WORLD_BOUND)
        {
            return false;
        }
        let packed = |field: usize| [0, 2, 4].map(|offset| m.i16(at(field) + offset));
        let velocity = wire::vector_unpack(packed(ds::VELOCITY), VELOCITY_SCALE);
        let forward = wire::vector_unpack(packed(ds::FORWARD), wire::UNIT_SCALE);
        let up = wire::vector_unpack(packed(ds::UP), wire::UNIT_SCALE);
        let mut axes = [forward[0], forward[1], forward[2], up[0], up[1], up[2]];
        if !wire::axes_make_valid(&mut axes) {
            return false;
        }
        let Some(actor) = client_actor_for(m, a.client, unit_index) else { return true };

        // the control, applied every tick until the next entry
        let control = actor + ca::CONTROL as u32;
        m.w32(actor + ca::RECEIVED_TIME as u32, a.now as u32);
        m.w8(control + uc::ANIMATION_STATE as u32, m.r8(at(ds::ANIMATION_STATE)));
        m.w8(control + uc::AIMING_SPEED as u32, m.r8(at(ds::AIMING_SPEED)));
        m.w16(control + uc::CONTROL_FLAGS as u32, control_flags as u16);
        m.set_i16(control + uc::WEAPON_INDEX as u32, NONE as i16);
        m.set_i16(control + uc::GRENADE_INDEX as u32, NONE as i16);
        m.set_i16(control + uc::ZOOM_LEVEL as u32, NONE as i16);
        for part in 0..3u32 {
            m.set_f32(control + uc::THROTTLE as u32 + 4 * part, m.i8(at(ds::THROTTLE) + part) as f32 / 127.0);
        }
        m.set_f32(control + uc::PRIMARY_TRIGGER as u32, m.r8(at(ds::PRIMARY_TRIGGER)) as f32 / 255.0);
        for (field, vector) in [(ds::FACING, uc::FACING_VECTOR), (ds::AIMING, uc::AIMING_VECTOR), (ds::LOOKING, uc::LOOKING_VECTOR)] {
            let unpacked = angles_unpack([m.i16(at(field)), m.i16(at(field) + 2)]);
            for (part, value) in unpacked.iter().enumerate() {
                m.set_f32(control + vector as u32 + 4 * part as u32, *value);
            }
        }
        let flags = m.r8(at(ds::FLAGS));
        if impulse != NO_IMPULSE && m.r8(at(ds::IMPULSE_NUMBER)) != m.r8(actor + ca::PLAYED_IMPULSE_NUMBER as u32) {
            let yaw = wire::angle_unpack(m.i16(at(ds::IMPULSE_ALIGNMENT)), false) as f64;
            m.set_i16(actor + ca::IMPULSE as u32, impulse as i16);
            m.w8(actor + ca::IMPULSE_NUMBER as u32, m.r8(at(ds::IMPULSE_NUMBER)));
            m.w8(actor + ca::IMPULSE_ALIGNED as u32, (flags & flag::IMPULSE_ALIGNED != 0) as u8);
            m.set_f32(actor + ca::IMPULSE_ALIGNMENT as u32, yaw.cos() as f32);
            m.set_f32(actor + ca::IMPULSE_ALIGNMENT as u32 + 4, yaw.sin() as f32);
        }
        if speech_sound != NONE && m.r8(at(ds::SPEECH_NUMBER)) != m.r8(actor + ca::PLAYED_SPEECH_NUMBER as u32) {
            m.w32(actor + ca::SPEECH_SOUND as u32, speech_sound as u32);
            m.w8(actor + ca::SPEECH_NUMBER as u32, m.r8(at(ds::SPEECH_NUMBER)));
        }
        if graph != NONE && m.r8(at(ds::USER_ANIMATION_NUMBER)) != m.r8(actor + ca::PLAYED_USER_ANIMATION_NUMBER as u32) {
            m.w32(actor + ca::USER_ANIMATION_GRAPH as u32, graph as u32);
            m.w16(actor + ca::USER_ANIMATION as u32, m.r16(at(ds::USER_ANIMATION)));
            m.w8(actor + ca::USER_ANIMATION_INTERPOLATE as u32, (m.r8(at(ds::USER_ANIMATION_INTERPOLATE)) != 0) as u8);
            m.w8(actor + ca::USER_ANIMATION_NUMBER as u32, m.r8(at(ds::USER_ANIMATION_NUMBER)));
        }

        // the state: seat, shields and health, camouflage, running blindly,
        // and position (a unit in a vehicle goes where the vehicle goes)
        let Some(unit) = self.unit(unit_index) else { return true };
        let parent = |unit: u32| m.i32(unit + ud::OBJECT_PARENT_OBJECT_INDEX as u32);
        if flags & flag::RIDES != 0 {
            let vehicle_index = m.i32(at(ds::VEHICLE_INDEX));
            let seat_index = m.i16(at(ds::SEAT_INDEX));
            if parent(unit) != vehicle_index || m.i16(unit + ud::UNIT_PARENT_SEAT_INDEX as u32) != seat_index {
                self.call(call::SET_SEAT, seat_index as i32, unit_index, vehicle_index, &[]);
            }
        } else if parent(unit) != NONE {
            self.call(call::SET_SEAT, NONE, unit_index, NONE, &[]);
        }
        let (_, mut damage) = self.damage_state(unit);
        let damage_flags = (flags & flag::SHIELD_DEPLETED != 0) as i32
            | ((flags & flag::SHIELD_CHARGING != 0) as i32) << 1
            | ((flags & flag::SHIELD_OVER_CHARGING != 0) as i32) << 2;
        damage[0] = wire::vitality_unpack(m.r16(at(ds::BODY_VITALITY)));
        damage[1] = wire::vitality_unpack(m.r16(at(ds::SHIELD_VITALITY)));
        self.set_damage(unit_index, damage_flags, &damage);
        self.set_unit_flag(unit, nv::_UNIT_ACTIVE_CAMOUFLAGED_BIT, flags & flag::CAMOUFLAGED != 0);
        self.set_unit_flag(unit, nv::_UNIT_SUPER_CAMOUFLAGED_BIT, flags & flag::SUPER_CAMOUFLAGED != 0);
        m.set_f32(unit + ud::UNIT_ACTIVE_CAMOUFLAGE as u32, m.r8(at(ds::ACTIVE_CAMOUFLAGE)) as f32 / 255.0);
        let team = m.r8(at(ds::TEAM));
        if team != NO_TEAM && team < NUMBER_OF_TEAMS {
            m.set_i16(unit + ud::OBJECT_OWNER_TEAM_INDEX as u32, team as i16);
        }
        self.set_unit_flag(unit, nv::_UNIT_RUNNING_BLINDLY_BIT, flags & flag::RUNNING_BLINDLY != 0);
        if flags & flag::RUNNING_BLINDLY != 0 {
            m.set_f32(unit + ud::UNIT_RUN_BLINDLY_ANGLE as u32, wire::angle_unpack(m.i16(at(ds::RUN_BLINDLY_ANGLE)), false));
        }
        // (vehicles are placed only by the host's object states)
        if parent(unit) == NONE && self.object_type(unit_index) as usize != nv::_OBJECT_TYPE_VEHICLE {
            let here = m.vector(unit + ud::OBJECT_POSITION as u32);
            let dx = position[0] - here[0];
            let dy = position[1] - here[1];
            let dz = position[2] - here[2];
            if dx * dx + dy * dy + dz * dz > REMOTE_CORRECTION_TOLERANCE * REMOTE_CORRECTION_TOLERANCE {
                let table = a.client;
                for (field, vector) in [
                    (nc::RECONCILE_POSITION, position),
                    (nc::RECONCILE_FORWARD, [axes[0], axes[1], axes[2]]),
                    (nc::RECONCILE_UP, [axes[3], axes[4], axes[5]]),
                    (nc::RECONCILE_VELOCITY, velocity),
                ] {
                    for (part, value) in vector.iter().enumerate() {
                        m.set_f32(table + field as u32 + 4 * part as u32, *value);
                    }
                }
                if self.call(call::RECONCILE, NONE, unit_index, 0, &[]) != 0 {
                    self.call(call::COUNT_CORRECTION, NONE, NONE, 0, &[]);
                }
            }
        }
        true
    }
}

/// The world step of a client handling the host's entries for its AI
/// units: network_actors.c's `network_actors_handle_states`.
pub fn client_states(context: &WorldContext, host: &impl AiHost) {
    let m = host.memory();
    let a: ClientArguments = arguments(context, m);
    let actors = Actors { context, host, m };
    let tags = crate::ai_actor::AiStepContext { tag_instances: a.tag_instances, tag_count: a.tag_count, ..Default::default() };
    for index in 0..a.count.max(0) as u32 {
        actors.state_apply(&a, &tags, a.entries + index * ds::SIZEOF as u32);
    }
}

/// The world step of a client driving the host's AI units, where the host
/// runs the AI in its tick: network_actors.c's `network_actors_drive`. Each
/// is driven with the last control the host sent, and plays what it started
/// once; one the host let go of (or quiet two seconds) is let go.
pub fn client_drive(context: &WorldContext, host: &impl AiHost) {
    let m = host.memory();
    let a: ClientArguments = arguments(context, m);
    let actors = Actors { context, host, m };
    let table = a.client;
    let entry = |index: i16| table + nc::ACTORS as u32 + index as u32 * ca::SIZEOF as u32;
    let mut index: i16 = 0;
    while index < m.i16(table + nc::COUNT as u32) {
        let actor = entry(index);
        let unit_index = m.i32(actor + ca::UNIT_INDEX as u32);
        if !actors.actor_unit_valid(unit_index) || a.now.wrapping_sub(m.i32(actor + ca::RECEIVED_TIME as u32)) > CONTROL_HELD_TICKS {
            if m.r8(actor + ca::DRIVEN as u32) != 0 && actors.unit(unit_index).is_some() {
                actors.call(call::SET_ACTIVELY_CONTROLLED, NONE, unit_index, 0, &[]);
            }
            let count = m.i16(table + nc::COUNT as u32) - 1;
            m.set_i16(table + nc::COUNT as u32, count);
            let last = entry(count);
            for offset in 0..ca::SIZEOF as u32 {
                m.w8(actor + offset, m.r8(last + offset));
            }
            continue;
        }
        index += 1;
        // (vehicles follow the host's transform instead)
        if actors.object_type(unit_index) as usize == nv::_OBJECT_TYPE_VEHICLE {
            continue;
        }
        if m.r8(actor + ca::DRIVEN as u32) == 0 {
            actors.call(call::SET_ACTIVELY_CONTROLLED, NONE, unit_index, 1, &[]);
            m.w8(actor + ca::DRIVEN as u32, 1);
        }
        actors.call(call::UNIT_CONTROL, NONE, unit_index, (actor + ca::CONTROL as u32) as i32, &[]);
        let impulse = m.i16(actor + ca::IMPULSE as u32);
        if impulse != NONE as i16 && m.r8(actor + ca::IMPULSE_NUMBER as u32) != m.r8(actor + ca::PLAYED_IMPULSE_NUMBER as u32) {
            let alignment = [m.f32(actor + ca::IMPULSE_ALIGNMENT as u32), m.f32(actor + ca::IMPULSE_ALIGNMENT as u32 + 4)];
            actors.call(call::ACTION, impulse as i32, unit_index, m.r8(actor + ca::IMPULSE_ALIGNED as u32) as i32, &alignment);
            m.w8(actor + ca::PLAYED_IMPULSE_NUMBER as u32, m.r8(actor + ca::IMPULSE_NUMBER as u32));
            m.set_i16(actor + ca::IMPULSE as u32, NONE as i16);
        }
        let speech_sound = m.i32(actor + ca::SPEECH_SOUND as u32);
        if speech_sound != NONE {
            actors.call(call::SPEECH, NONE, unit_index, speech_sound, &[]);
            m.w8(actor + ca::PLAYED_SPEECH_NUMBER as u32, m.r8(actor + ca::SPEECH_NUMBER as u32));
            m.w32(actor + ca::SPEECH_SOUND as u32, NONE as u32);
        }
        let graph = m.i32(actor + ca::USER_ANIMATION_GRAPH as u32);
        if graph != NONE {
            let interpolate = if m.r8(actor + ca::USER_ANIMATION_INTERPOLATE as u32) != 0 { 1.0 } else { 0.0 };
            actors.call(call::USER_ANIMATION, m.i16(actor + ca::USER_ANIMATION as u32) as i32, unit_index, graph, &[interpolate]);
            m.w8(actor + ca::PLAYED_USER_ANIMATION_NUMBER as u32, m.r8(actor + ca::USER_ANIMATION_NUMBER as u32));
            m.w32(actor + ca::USER_ANIMATION_GRAPH as u32, NONE as u32);
        }
    }
}
