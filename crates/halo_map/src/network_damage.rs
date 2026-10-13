//! The host checking a client's report of its player's hit (OpenCE's
//! port/linux/game/network_damage.c's `distributed_report_valid` and
//! `network_damage_handle_reports`, port/linux/NETCODE.md stage 5).
//!
//! A client deals no damage itself: it reports what its players hit, and
//! the host deals it once it has checked it. The report must be of that
//! machine's player, its damage one the player's weapons (now or lately),
//! a grenade the host saw them throw, their unarmed blow or the vehicle they
//! drove deal (found by walking the tags: weapons' triggers, projectiles'
//! impacts and detonations, effects' parts), of the shape the game gives it
//! (at a point, a melee blow, a collision), paid for at the weapon's rate,
//! from where the host had the player, at the target about where the host
//! had it when the shooter saw it. Then it becomes the damage the host
//! deals (an explosion's direction and fall-off the game's own).
//!
//! The host's records (each player's weapons and hits left, the explosions
//! of the tick, where players were) are the game's memory
//! (network_damage_records.h), the report's arguments a struct the game
//! passes ([`Arguments`]). Whether the machine has the player, its round
//! trip, objects' origins, velocities, maximum vitality and nodes, a point's
//! location and dealing the damage are the game's, through [`AiHost::call`]
//! (network_damage.c's `network_damage_call`).

use crate::ai_actor::{AiCall, AiHost, AiMemory, AiMemoryExt, NONE, WorldContext, datum_get, tag_block_count, tag_block_element};
use crate::layout::{
    damage_data as dd, damage_explosion as ex, damage_explosion_record as er, damage_history_object as ho,
    damage_history_tick as ht, damage_player_record as pr, damage_trail_point as tp, net_values as nv, player_datum as pd,
    unit_datum as ud,
};

/// network_damage.c's calls (halo_sim.h's `_network_damage_call_*`).
pub mod call {
    /// `distributed_machine_has_player(a, b)`, the result.
    pub const MACHINE_HAS_PLAYER: i32 = 1;
    /// `object_get_maximum_body_vitality(a, TRUE)`, `output[0]`.
    pub const MAXIMUM_BODY_VITALITY: i32 = 2;
    /// `object_has_node(a, b)`, the result.
    pub const HAS_NODE: i32 = 3;
    /// `distributed_machine_round_trip_ticks(a)`, `output[0]`.
    pub const ROUND_TRIP: i32 = 4;
    /// `object_get_origin(a)`, `output[0..3]`.
    pub const ORIGIN: i32 = 5;
    /// `object_get_velocities(a)`'s translational, `output[0..3]`.
    pub const VELOCITY: i32 = 6;
    /// `scenario_location_from_point` into the location at `b` of the point
    /// `input[0..3]`.
    pub const LOCATION: i32 = 7;
    /// `object_cause_damage` of the damage at `b` to `a` (its node, region
    /// and material `input[3..6]`, its normal `input[0..3]` if `input[6]`),
    /// as the host deals a client's report.
    pub const DEAL: i32 = 8;
}

/// halo_sim.h's `struct halo_sim_network_report`: a report's arguments.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct Arguments {
    /// network_damage.c's `struct distributed_hit_report`.
    pub report: [u8; 0x6c],
    pub machine_index: i32,
    /// The report's player's records (0 for a player not tracked): its
    /// weapons and hits left, its explosions this tick, and its trail.
    pub player_record: u32,
    pub explosions: u32,
    pub trail: u32,
    /// `damage_history`, `scenario_get_game_globals()`, `global_gravity`.
    pub history: u32,
    pub game_globals: u32,
    pub gravity: f32,
    pub tag_instances: u32,
    pub tag_count: i32,
    /// The damage the host deals (`struct damage_data`), made here.
    pub damage: [u8; 0x54],
    /// The counts of reports rejected and dealt.
    pub rejected: u32,
    pub dealt: u32,
}

const _: () = assert!(std::mem::size_of::<Arguments>() == 0x6c + 4 * 9 + 0x54 + 8);

/// The report's offsets in [`Arguments`] and its own (network_damage.c's
/// `struct distributed_hit_report`, its damage `struct distributed_damage`).
mod report {
    pub const OBJECT_INDEX: u32 = 0;
    pub const DEFINITION_INDEX: u32 = 4;
    pub const FLAGS: u32 = 8;
    pub const OWNER_PLAYER_INDEX: u32 = 12;
    pub const OWNER_TEAM_INDEX: u32 = 14;
    pub const OWNER_OBJECT_INDEX: u32 = 16;
    pub const ORIGIN: u32 = 20;
    pub const EPICENTER: u32 = 32;
    pub const DIRECTION: u32 = 44;
    pub const SCALE: u32 = 56;
    pub const MULTIPLIER: u32 = 60;
    pub const MATERIAL_EFFECT_SCALE: u32 = 64;
    pub const MATERIAL_TYPE: u32 = 68;
    pub const TARGET_POSITION: u32 = 72;
    pub const OBJECT_NORMAL: u32 = 84;
    pub const NODE_INDEX: u32 = 96;
    pub const REGION_INDEX: u32 = 98;
    pub const MATERIAL_INDEX: u32 = 100;
    pub const HAS_NORMAL: u32 = 102;
    pub const HOST_TIME: u32 = 104;
    pub const ARGUMENTS_DAMAGE: u32 = 0x6c + 4 * 9;
    pub const ARGUMENTS_REJECTED: u32 = ARGUMENTS_DAMAGE + 0x54;
}

/// network_damage.c's `_damage_source` flags: how a player's damage is dealt.
mod source {
    pub const IMPACT: u8 = 1 << 0;
    pub const AREA: u8 = 1 << 1;
    pub const MELEE: u8 = 1 << 2;
    pub const COLLISION: u8 = 1 << 3;
}

/// network_damage.c's limits (network_damage_records.h's among them).
const TICKS_PER_SECOND: i32 = 30;
const MAXIMUM_RECENT_WEAPONS: u32 = 8;
const RECENT_WEAPON_TICKS: i32 = 10 * TICKS_PER_SECOND;
const MAXIMUM_GRENADE_THROWS: u32 = 32;
const HIT_REPORT_BURST_SECONDS: f32 = 3.0;
const HIT_REPORT_RATE_MARGIN: f32 = 2.0;
const MELEE_HITS_PER_SECOND: f32 = 4.0;
const GRENADE_HITS_PER_SECOND: f32 = 4.0;
const VEHICLE_HITS_PER_SECOND: f32 = 15.0;
const EFFECT_HITS_PER_SECOND: f32 = 4.0;
const MAXIMUM_EXPLOSIONS_PER_TICK: i16 = 64;
const MAXIMUM_TAG_DEPTH: i16 = 4;
const TARGET_HISTORY_TICKS: i32 = 32;
const TARGET_HISTORY_SLACK_TICKS: i32 = 3;
const REPORT_MAXIMUM_AGE_TICKS: i32 = 3 * TICKS_PER_SECOND;
const PLAYER_TRAIL_INTERVAL_TICKS: i32 = 3;
const PLAYER_TRAIL_REPORT_SLACK_TICKS: i32 = TICKS_PER_SECOND;
const PLAYER_TRAIL_POINTS: u32 = 128;
const PLAYER_TRAIL_TICKS: i32 = PLAYER_TRAIL_INTERVAL_TICKS * PLAYER_TRAIL_POINTS as i32;
const REPORT_TARGET_TOLERANCE: f32 = 3.0;
const REPORT_TARGET_LEAD_TICKS: f32 = 15.0;
const REPORT_IMPACT_TOLERANCE: f32 = 2.0;
const REPORT_HISTORY_TOLERANCE: f32 = 2.0;
const REPORT_HISTORY_LEAD_TICKS: f32 = 3.0;
const REPORT_VEHICLE_TOLERANCE: f32 = 4.0;
const REPORT_COLLISION_LEAD_TICKS: f32 = 3.0;
const REPORT_RANGE_TOLERANCE: f32 = 6.0;
const REPORT_POINT_TOLERANCE: f32 = 0.01;
const REPORT_WORLD_BOUND: f32 = 32768.0;
const REPORT_MAXIMUM_MELEE_SCALE: f32 = 1.5;
const NO_PLAYER: u8 = 0xff;
const MAXIMUM_TRACKED_PLAYERS: i16 = nv::MAXIMUM_TRACKED_PLAYERS as i16;
/// `_projectile_detonation_timer_starts_immediately`.
const TIMER_STARTS_IMMEDIATELY: i16 = 0;
/// `_damage_effect_dont_scale_damage_by_distance_bit`.
const DONT_SCALE_DAMAGE_BY_DISTANCE: u32 = 1 << 0;

const fn group(code: &[u8; 4]) -> u32 {
    u32::from_be_bytes(*code)
}
const EFFECT_TAG: u32 = group(b"effe");
const PROJECTILE_TAG: u32 = group(b"proj");
const WEAPON_TAG: u32 = group(b"weap");
const DAMAGE_EFFECT_TAG: u32 = group(b"jpt!");
const OBJECT_TAG: u32 = group(b"obje");
const COLLISION_MODEL_TAG: u32 = group(b"coll");

/// network_damage_records.h's `struct damage_reach`.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Reach {
    pub distance: f32,
    pub ticks: f32,
    pub carried_ticks: f32,
}

/// C's `MAX`, `MIN`: as written (a NaN falls as they fall).
fn max(a: f32, b: f32) -> f32 {
    if a > b { a } else { b }
}
fn min(a: f32, b: f32) -> f32 {
    if a > b { b } else { a }
}

fn distance_squared(a: [f32; 3], b: [f32; 3]) -> f32 {
    let (dx, dy, dz) = (a[0] - b[0], a[1] - b[1], a[2] - b[2]);
    dx * dx + dy * dy + dz * dz
}

fn real_valid(value: f32) -> bool {
    value - value == 0.0
}

fn point_valid(point: [f32; 3], bound: f32) -> bool {
    point.iter().all(|part| part.abs() <= bound)
}

/// `distributed_reach_combine`: the farther of them (unbounded if either is).
fn reach_combine(reach: &mut Reach, other: &Reach) {
    if !(reach.distance >= 0.0) || !(other.distance >= 0.0) {
        reach.distance = -1.0;
        return;
    }
    reach.distance = max(reach.distance, other.distance);
    reach.ticks = max(reach.ticks, other.ticks);
    reach.carried_ticks = max(reach.carried_ticks, other.carried_ticks);
}

struct Damage<'a, H: AiHost> {
    context: &'a WorldContext,
    host: &'a H,
    m: &'a dyn AiMemory,
    arguments: &'a Arguments,
    /// The arguments where the game holds them.
    address: u32,
}

impl<H: AiHost> Damage<'_, H> {
    fn call(&self, kind: i32, a: i32, b: i32, input: &[f32]) -> AiCall {
        let mut call = AiCall { kind, actor: NONE, a, b, ..AiCall::default() };
        call.input[..input.len()].copy_from_slice(input);
        self.host.call(&mut call);
        call
    }

    fn now(&self) -> i32 {
        self.context.game_time
    }

    /// The report's field (in the arguments where the game holds them).
    fn report(&self, offset: u32) -> u32 {
        self.address + offset
    }

    fn tags(&self) -> crate::ai_actor::AiStepContext {
        crate::ai_actor::AiStepContext {
            tag_instances: self.arguments.tag_instances,
            tag_count: self.arguments.tag_count,
            ..crate::ai_actor::AiStepContext::default()
        }
    }

    /// cache_files.c's `tag_index_is_group`.
    fn is_group(&self, tag_index: i32, group_tag: u32) -> bool {
        let m = self.m;
        let absolute_index = tag_index as i16;
        if tag_index == NONE
            || self.arguments.tag_instances == 0
            || absolute_index < 0
            || absolute_index as i32 >= self.arguments.tag_count
        {
            return false;
        }
        let instance = self.arguments.tag_instances + absolute_index as u32 * 0x20;
        m.i32(instance + 0xc) == tag_index
            && m.r32(instance + 0x14) != 0
            && (m.r32(instance) == group_tag || m.r32(instance + 4) == group_tag || m.r32(instance + 8) == group_tag)
    }

    /// `tag_get` (0 for the empty tag's data, which reads as zeros).
    fn tag(&self, group_tag: u32, tag_index: i32) -> u32 {
        crate::sensing::tag_get(self.m, &self.tags(), group_tag, tag_index).unwrap_or(0)
    }

    /// A tag reference's index (0 for the empty tag's: no tag).
    fn reference(&self, address: u32) -> i32 {
        if address < 0x100 { 0 } else { self.m.i32(address + 0xc) }
    }

    fn object(&self, object_index: i32) -> Option<u32> {
        datum_get(self.m, self.context.object_header_data, object_index)
            .map(|header| self.m.r32(header + crate::layout::object_header_datum::DATUM as u32))
    }

    fn of_types(&self, object_index: i32, mask: u32) -> Option<u32> {
        self.object(object_index).filter(|&object| mask & (1 << self.m.r8(object + ud::OBJECT_TYPE as u32)) != 0)
    }

    fn player(&self, player_index: i16) -> Option<u32> {
        use crate::data_array::{DATA_DATA, DATA_MAXIMUM_COUNT, DATA_SIZE};
        let m = self.m;
        let data = self.context.player_data;
        if player_index < 0 || player_index >= m.r16(data + DATA_MAXIMUM_COUNT) as i16 {
            return None;
        }
        let player = m.r32(data + DATA_DATA) + player_index as u32 * m.r16(data + DATA_SIZE) as i16 as u32;
        (m.i16(player + pd::IDENTIFIER as u32) != 0).then_some(player)
    }

    fn living_unit(&self, player: u32) -> i32 {
        let m = self.m;
        let unit_index = m.i32(player + pd::UNIT_INDEX as u32);
        match self.object(unit_index) {
            Some(unit) if unit_index != NONE && m.r16(unit + ud::OBJECT_DAMAGE_FLAGS as u32) as u32 & (1 << nv::_OBJECT_DEAD_BIT) == 0 => {
                unit_index
            }
            _ => NONE,
        }
    }

    // ---------- what the player could deal (the tags)

    /// `distributed_projectile_flight`.
    fn projectile_flight(&self, projectile: u32) -> Reach {
        use crate::layout::{_projectile_definition as p, projectile_material_response_definition as r};
        let m = self.m;
        let initial = m.f32(projectile + p::INITIAL_VELOCITY as u32);
        let r#final = m.f32(projectile + p::FINAL_VELOCITY as u32);
        let fastest = max(initial, r#final);
        let slowest = min(initial, r#final);
        let timer_ticks = if m.i16(projectile + p::DETONATION_TIMER_STARTS as u32) == TIMER_STARTS_IMMEDIATELY {
            m.f32(projectile + p::TIMER_UPPER_BOUND as u32) * TICKS_PER_SECOND as f32
        } else {
            0.0
        };
        let mut flight = Reach { distance: -1.0, ticks: PLAYER_TRAIL_TICKS as f32, carried_ticks: 0.0 };
        let responses = projectile + p::MATERIAL_RESPONSES as u32;
        for index in 0..tag_block_count(m, responses) {
            let response = tag_block_element(m, responses, index, r::SIZEOF);
            let attach = nv::_PROJECTILE_MATERIAL_RESPONSE_ATTACH as i16;
            if m.i16(response + r::DEFAULT_RESPONSE as u32) == attach || m.i16(response + r::POTENTIAL_RESPONSE as u32) == attach {
                return flight;
            }
        }
        let maximum_range = m.f32(projectile + p::MAXIMUM_RANGE as u32);
        if maximum_range > 0.0 {
            flight.distance = maximum_range;
            if slowest > 0.0 && 2.0 * maximum_range / slowest < flight.ticks {
                flight.ticks = 2.0 * maximum_range / slowest;
            }
            if timer_ticks > 0.0 && timer_ticks < flight.ticks {
                flight.ticks = timer_ticks;
            }
        } else if timer_ticks > 0.0 && fastest >= 0.0 {
            let gravity = self.arguments.gravity
                * max(0.0, max(m.f32(projectile + p::AIR_GRAVITY_SCALE as u32), m.f32(projectile + p::WATER_GRAVITY_SCALE as u32)));
            flight.distance = fastest * timer_ticks + 0.5 * gravity * timer_ticks * timer_ticks;
            flight.ticks = min(timer_ticks, PLAYER_TRAIL_TICKS as f32);
            flight.carried_ticks = timer_ticks;
            if !(flight.distance >= 0.0) {
                flight.distance = -1.0;
            }
        }
        flight
    }

    /// `distributed_effect_deals`: an effect's own damage as an area's, its
    /// projectiles' as theirs.
    fn effect_deals(&self, effect_index: i32, damage_index: i32, depth: i16, reach: &mut Reach) -> u8 {
        use crate::layout::{effect_definition as e, effect_event_definition as ev, effect_part_definition as part};
        let m = self.m;
        if depth > MAXIMUM_TAG_DEPTH || !self.is_group(effect_index, EFFECT_TAG) {
            return 0;
        }
        let effect = self.tag(EFFECT_TAG, effect_index);
        let mut kinds = 0;
        let events = effect + e::EVENTS as u32;
        for event_index in 0..tag_block_count(m, events) {
            let event = tag_block_element(m, events, event_index, ev::SIZEOF);
            let parts = event + ev::PARTS as u32;
            for part_index in 0..tag_block_count(m, parts) {
                let reference = tag_block_element(m, parts, part_index, part::SIZEOF) + part::REFERENCE as u32;
                let index = m.i32(reference + 0xc);
                if index == NONE {
                    continue;
                }
                let group_tag = m.r32(reference);
                if index == damage_index {
                    kinds |= source::AREA;
                } else if group_tag == PROJECTILE_TAG {
                    kinds |= self.projectile_deals(index, damage_index, depth + 1, reach);
                } else if group_tag == EFFECT_TAG {
                    kinds |= self.effect_deals(index, damage_index, depth + 1, reach);
                }
            }
        }
        kinds
    }

    /// `distributed_projectile_deals`: a projectile's impact and detonation
    /// at a point, its effects', and how far from its firer.
    fn projectile_deals(&self, projectile_index: i32, damage_index: i32, depth: i16, reach: &mut Reach) -> u8 {
        use crate::layout::{_projectile_definition as p, projectile_definition, projectile_material_response_definition as r};
        let m = self.m;
        if depth > MAXIMUM_TAG_DEPTH || !self.is_group(projectile_index, PROJECTILE_TAG) {
            return 0;
        }
        let projectile = self.tag(PROJECTILE_TAG, projectile_index) + projectile_definition::PROJECTILE as u32;
        let mut from_here = Reach::default();
        let mut kinds = 0;
        if self.reference(projectile + p::IMPACT_DAMAGE as u32) == damage_index
            || self.reference(projectile + p::ATTACHED_DETONATION_DAMAGE as u32) == damage_index
        {
            kinds |= source::IMPACT;
        }
        for offset in [p::EFFECT, p::SUPER_DETONATION, p::DETONATION_STARTED] {
            kinds |= self.effect_deals(self.reference(projectile + offset as u32), damage_index, depth + 1, &mut from_here);
        }
        let responses = projectile + p::MATERIAL_RESPONSES as u32;
        for index in 0..tag_block_count(m, responses) {
            let response = tag_block_element(m, responses, index, r::SIZEOF);
            for offset in [r::DEFAULT_EFFECT, r::POTENTIAL_EFFECT, r::DETONATION_EFFECT] {
                kinds |= self.effect_deals(self.reference(response + offset as u32), damage_index, depth + 1, &mut from_here);
            }
        }
        if kinds != 0 {
            let flight = self.projectile_flight(projectile);
            if !(flight.distance >= 0.0) || !(from_here.distance >= 0.0) {
                from_here.distance = -1.0;
            } else {
                from_here.distance += flight.distance;
                from_here.ticks += flight.ticks;
                from_here.carried_ticks += flight.carried_ticks;
            }
            reach_combine(reach, &from_here);
        }
        kinds
    }

    /// `distributed_weapon_deals`: how many hits a second the weapon deals
    /// of the damage (its melee blow, its own detonation, its triggers'
    /// projectiles at their fastest rate), and how.
    fn weapon_deals(&self, weapon_index: i32, damage_index: i32, kinds: &mut u8, reach: &mut Reach) -> f32 {
        use crate::layout::{_weapon_definition as w, weapon_definition, weapon_trigger_definition as t};
        let m = self.m;
        *kinds = 0;
        let mut rate = 0.0;
        if !self.is_group(weapon_index, WEAPON_TAG) {
            return 0.0;
        }
        let weapon = self.tag(WEAPON_TAG, weapon_index) + weapon_definition::WEAPON as u32;
        if self.reference(weapon + w::MELEE_ATTACK_DAMAGE as u32) == damage_index {
            rate = MELEE_HITS_PER_SECOND;
            *kinds |= source::MELEE;
        }
        let mut weapon_reach = Reach::default();
        let effect_kinds = self.effect_deals(self.reference(weapon + w::DETONATION_EFFECT as u32), damage_index, 0, &mut weapon_reach)
            | self.effect_deals(self.reference(weapon + w::OVERHEATED_EFFECT as u32), damage_index, 0, &mut weapon_reach);
        if effect_kinds != 0 {
            rate = max(rate, EFFECT_HITS_PER_SECOND);
            *kinds |= effect_kinds;
            reach.distance = -1.0;
        }
        let triggers = weapon + w::TRIGGERS as u32;
        for index in 0..tag_block_count(m, triggers) {
            let trigger = tag_block_element(m, triggers, index, t::SIZEOF);
            let projectile_kinds = self.projectile_deals(self.reference(trigger + t::PROJECTILE as u32), damage_index, 0, reach);
            if projectile_kinds != 0 {
                let mut rate_of_fire = max(m.f32(trigger + t::INITIAL_RATE_OF_FIRE as u32), m.f32(trigger + t::FINAL_RATE_OF_FIRE as u32));
                if rate_of_fire <= 0.0001 {
                    rate_of_fire = if m.r32(trigger + t::FLAGS as u32) & (1 << nv::_WEAPON_TRIGGER_LATCHED_BIT) != 0 {
                        TICKS_PER_SECOND as f32 / 2.0
                    } else {
                        TICKS_PER_SECOND as f32
                    };
                }
                let projectiles = m.i16(trigger + t::PROJECTILES_PER_SHOT as u32);
                let projectiles = if projectiles > 1 { projectiles } else { 1 };
                rate = max(rate, max(rate_of_fire, 1.0) * projectiles as f32);
                *kinds |= projectile_kinds;
            }
        }
        rate
    }

    /// `distributed_source_deals` (its tags walked: the game's cache of
    /// them gives what this does).
    fn source_deals(&self, source_index: i32, damage_index: i32, grenade: bool, kinds: &mut u8, reach: &mut Reach) -> f32 {
        *reach = Reach::default();
        if grenade {
            *kinds = self.projectile_deals(source_index, damage_index, 0, reach);
            return if *kinds != 0 { GRENADE_HITS_PER_SECOND } else { 0.0 };
        }
        self.weapon_deals(source_index, damage_index, kinds, reach)
    }

    /// `distributed_grenade_throw`: the oldest grenade of the type the host
    /// saw the player throw lately that has not gone off.
    fn grenade_throw(&self, grenade_type: i16) -> i16 {
        let m = self.m;
        let record = self.arguments.player_record;
        let mut oldest: i16 = NONE as i16;
        for index in 0..MAXIMUM_GRENADE_THROWS {
            let time = m.i32(record + pr::THROW_TIMES as u32 + 4 * index);
            if time != NONE
                && m.i16(record + pr::THROW_TYPES as u32 + 2 * index) == grenade_type
                && self.now().wrapping_sub(time) <= RECENT_WEAPON_TICKS
                && (oldest == NONE as i16 || time < m.i32(record + pr::THROW_TIMES as u32 + 4 * oldest as u32))
            {
                oldest = index as i16;
            }
        }
        oldest
    }

    fn grenade_projectile(&self, grenade_type: i16) -> i32 {
        use crate::layout::{game_globals, game_globals_grenade as g};
        let grenades = self.arguments.game_globals + game_globals::GRENADES as u32;
        self.reference(tag_block_element(self.m, grenades, grenade_type as i32, g::SIZEOF) + g::PROJECTILE as u32)
    }

    /// `distributed_explosion_this_tick`: an explosion of the grenade's type
    /// among the player's this tick (at the epicenter, or anywhere).
    fn explosion_this_tick(&self, grenade_type: i16, epicenter: Option<[f32; 3]>) -> bool {
        let m = self.m;
        let explosions = self.arguments.explosions;
        let projectile = self.grenade_projectile(grenade_type);
        if m.i32(explosions + er::TIME as u32) != self.now() || projectile == NONE {
            return false;
        }
        for index in 0..m.i16(explosions + er::COUNT as u32) as i32 {
            let explosion = explosions + er::EXPLOSIONS as u32 + index as u32 * ex::SIZEOF as u32;
            let mut kinds = 0;
            let mut reach = Reach::default();
            if self.source_deals(projectile, m.i32(explosion + ex::DEFINITION_INDEX as u32), true, &mut kinds, &mut reach) > 0.0
                && epicenter.is_none_or(|epicenter| m.vector(explosion + ex::EPICENTER as u32) == epicenter)
            {
                return true;
            }
        }
        false
    }

    /// `distributed_player_deals`: how many hits a second the player could
    /// deal of the damage (their weapons lately, their unarmed blow, the
    /// vehicle they drove, a grenade they threw), how, how far from them,
    /// and the grenade's type when nothing else deals it.
    fn player_deals(&self, damage_index: i32, kinds: &mut u8, reach: &mut Reach, grenade_type: &mut i16) -> f32 {
        use crate::layout::{distributed_falling_damage as fd, game_globals};
        let m = self.m;
        let record = self.arguments.player_record;
        let now = self.now();
        let mut rate = 0.0;
        let mut grenade_rate = 0.0;
        *kinds = 0;
        *grenade_type = NONE as i16;
        *reach = Reach::default();
        for index in 0..MAXIMUM_RECENT_WEAPONS {
            let definition_index = m.i32(record + pr::DEFINITION_INDICES as u32 + 4 * index);
            if definition_index != NONE && now.wrapping_sub(m.i32(record + pr::TIMES as u32 + 4 * index)) <= RECENT_WEAPON_TICKS {
                let mut weapon_kinds = 0;
                let mut weapon_reach = Reach::default();
                let weapon_rate = self.source_deals(definition_index, damage_index, false, &mut weapon_kinds, &mut weapon_reach);
                if weapon_rate > 0.0 {
                    reach_combine(reach, &weapon_reach);
                }
                rate = max(rate, weapon_rate);
                *kinds |= weapon_kinds;
            }
        }
        if m.i32(record + pr::UNARMED_MELEE_DAMAGE_INDEX as u32) == damage_index
            && damage_index != NONE
            && now.wrapping_sub(m.i32(record + pr::UNARMED_TIME as u32)) <= RECENT_WEAPON_TICKS
        {
            rate = max(rate, MELEE_HITS_PER_SECOND);
            *kinds |= source::MELEE;
        }
        let globals = self.arguments.game_globals;
        let falling_damage = globals + game_globals::FALLING_DAMAGE as u32;
        if now.wrapping_sub(m.i32(record + pr::DRIVEN_TIME as u32)) <= RECENT_WEAPON_TICKS && tag_block_count(m, falling_damage) > 0 {
            let element = tag_block_element(m, falling_damage, 0, 0);
            if self.reference(element + fd::VEHICLE_COLLISION_DAMAGE as u32) == damage_index {
                rate = max(rate, VEHICLE_HITS_PER_SECOND);
                *kinds |= source::COLLISION;
            }
        }
        let grenade_count = tag_block_count(m, globals + game_globals::GRENADES as u32);
        for index in 0..grenade_count.min(nv::NUMBER_OF_UNIT_GRENADE_TYPES as i32) as i16 {
            let thrown = self.grenade_throw(index) != NONE as i16;
            let projectile = self.grenade_projectile(index);
            if (thrown || self.explosion_this_tick(index, None)) && projectile != NONE {
                let mut this_kinds = 0;
                let mut this_reach = Reach::default();
                let this_rate = self.source_deals(projectile, damage_index, true, &mut this_kinds, &mut this_reach);
                if this_rate > 0.0 {
                    reach_combine(reach, &this_reach);
                    if *grenade_type == NONE as i16 || thrown {
                        *grenade_type = index;
                    }
                }
                grenade_rate = max(grenade_rate, this_rate);
                *kinds |= this_kinds;
            }
        }
        if rate > 0.0 {
            *grenade_type = NONE as i16;
        }
        rate = max(rate, grenade_rate);
        rate * HIT_REPORT_RATE_MARGIN
    }

    // ---------- where the host had them

    fn history_object(&self, time: i32, player_index: i16, slot: u32) -> u32 {
        let tick = self.arguments.history + (time & (TARGET_HISTORY_TICKS - 1)) as u32 * ht::SIZEOF as u32;
        tick + ht::OBJECTS as u32 + (player_index as u32 * 2 + slot) * ho::SIZEOF as u32
    }

    fn history_time(&self, time: i32) -> i32 {
        self.m.i32(self.arguments.history + (time & (TARGET_HISTORY_TICKS - 1)) as u32 * ht::SIZEOF as u32 + ht::TIME as u32)
    }

    /// `distributed_history_near`.
    fn history_near(&self, object: u32, position: [f32; 3], tolerance: f32) -> bool {
        let reach = tolerance + REPORT_HISTORY_LEAD_TICKS * self.m.f32(object + ho::SPEED as u32);
        distance_squared(position, self.m.vector(object + ho::POSITION as u32)) <= reach * reach
    }

    /// `distributed_target_seen`: TRUE (1), FALSE (0), or NONE for an
    /// object no player has now.
    fn target_seen(&self, object_index: i32, position: [f32; 3], ticks: i32) -> i16 {
        let m = self.m;
        let mut latest = false;
        let mut column: Option<(i16, u32)> = None;
        let mut back = 0;
        while back <= ticks && back < TARGET_HISTORY_TICKS {
            let time = self.now().wrapping_sub(back);
            back += 1;
            if self.history_time(time) != time {
                break;
            }
            let stale = match column {
                None => true,
                Some((player_index, slot)) => m.i32(self.history_object(time, player_index, slot) + ho::OBJECT_INDEX as u32) != object_index,
            };
            if stale {
                column = None;
                'find: for player_index in 0..MAXIMUM_TRACKED_PLAYERS {
                    for slot in 0..2 {
                        if m.i32(self.history_object(time, player_index, slot) + ho::OBJECT_INDEX as u32) == object_index {
                            column = Some((player_index, slot));
                            break 'find;
                        }
                    }
                }
            }
            let Some((player_index, slot)) = column else { continue };
            if back - 1 == 0 {
                latest = true;
            }
            if self.history_near(self.history_object(time, player_index, slot), position, REPORT_HISTORY_TOLERANCE) {
                return 1;
            }
        }
        if latest { 0 } else { NONE as i16 }
    }

    /// `distributed_player_was_near`.
    fn player_was_near(&self, player_index: i16, position: [f32; 3], ticks: i32) -> bool {
        let mut back = 0;
        while back <= ticks && back < TARGET_HISTORY_TICKS {
            let time = self.now().wrapping_sub(back);
            back += 1;
            if self.history_time(time) != time {
                break;
            }
            let object = self.history_object(time, player_index, 0);
            if self.m.i32(object + ho::OBJECT_INDEX as u32) != NONE && self.history_near(object, position, REPORT_HISTORY_TOLERANCE) {
                return true;
            }
        }
        false
    }

    /// `distributed_player_drove_into`.
    fn player_drove_into(&self, player_index: i16, epicenter: [f32; 3], target_position: [f32; 3], target_radius: f32, ticks: i32) -> bool {
        let m = self.m;
        let mut back = 0;
        while back <= ticks && back < TARGET_HISTORY_TICKS {
            let time = self.now().wrapping_sub(back);
            back += 1;
            if self.history_time(time) != time {
                break;
            }
            let object = self.history_object(time, player_index, 1);
            let vehicle_index = m.i32(object + ho::OBJECT_INDEX as u32);
            if vehicle_index == NONE || m.r8(object + ho::DRIVING as u32) == 0 {
                continue;
            }
            let Some(vehicle) = self.object(vehicle_index) else { continue };
            let radius = m.f32(vehicle + ud::OBJECT_BOUNDING_SPHERE_RADIUS as u32);
            let speed = m.f32(object + ho::SPEED as u32);
            let reach = target_radius + radius + REPORT_IMPACT_TOLERANCE + REPORT_COLLISION_LEAD_TICKS * speed;
            if self.history_near(object, epicenter, REPORT_HISTORY_TOLERANCE + radius + REPORT_VEHICLE_TOLERANCE)
                && distance_squared(epicenter, target_position) <= reach * reach
            {
                return true;
            }
        }
        false
    }

    /// `distributed_player_could_fire`.
    fn player_could_fire(&self, point: [f32; 3], reach: &Reach, host_time: i32) -> bool {
        let m = self.m;
        let now = self.now();
        let mut ticks = reach.ticks;
        let round_trip = self.call(call::ROUND_TRIP, self.arguments.machine_index, 0, &[]).output[0];
        let latest_made = now.wrapping_sub(round_trip as i32).wrapping_sub(PLAYER_TRAIL_REPORT_SLACK_TICKS);
        let host_time = if host_time < latest_made { latest_made } else { host_time };
        if !(ticks <= PLAYER_TRAIL_TICKS as f32) {
            ticks = PLAYER_TRAIL_TICKS as f32;
        }
        let earliest = host_time
            .wrapping_sub(ticks as i32)
            .wrapping_sub(TARGET_HISTORY_SLACK_TICKS)
            .wrapping_sub(PLAYER_TRAIL_INTERVAL_TICKS);
        for index in 0..PLAYER_TRAIL_POINTS {
            let point_address = self.arguments.trail + index * tp::SIZEOF as u32;
            let time = m.i32(point_address + tp::TIME as u32);
            if time == NONE || time < earliest || time > now {
                continue;
            }
            let distance = reach.distance
                + REPORT_RANGE_TOLERANCE
                + m.f32(point_address + tp::RADIUS as u32)
                + m.f32(point_address + tp::SPEED as u32)
                    * (reach.carried_ticks + PLAYER_TRAIL_INTERVAL_TICKS as f32 + TARGET_HISTORY_SLACK_TICKS as f32);
            if distance_squared(m.vector(point_address + tp::POSITION as u32), point) <= distance * distance {
                return true;
            }
        }
        false
    }

    // ---------- the report

    /// `distributed_report_paid`: whether the player has hits left for the
    /// report (an explosion's other objects that tick come free, each once).
    fn report_paid(&self, rate: f32, explosion: bool, reach: bool) -> bool {
        let m = self.m;
        let record = self.arguments.player_record;
        let explosions = self.arguments.explosions;
        let now = self.now();
        let hit_seconds = record + pr::HIT_SECONDS as u32;
        let elapsed = now.wrapping_sub(m.i32(record + pr::HIT_SECONDS_TIME as u32));
        let mut paid = false;
        if m.i32(explosions + er::TIME as u32) != now {
            m.w32(explosions + er::TIME as u32, now as u32);
            m.set_i16(explosions + er::COUNT as u32, 0);
        }
        let definition_index = m.i32(self.report(report::DEFINITION_INDEX));
        let epicenter = m.vector(self.report(report::EPICENTER));
        let object_index = m.i32(self.report(report::OBJECT_INDEX));
        let count = || m.i16(explosions + er::COUNT as u32);
        let entry = |index: i16| explosions + er::EXPLOSIONS as u32 + index as u32 * ex::SIZEOF as u32;
        if explosion {
            for index in 0..count() {
                let explosion = entry(index);
                if m.i32(explosion + ex::DEFINITION_INDEX as u32) == definition_index && m.vector(explosion + ex::EPICENTER as u32) == epicenter {
                    if m.i32(explosion + ex::OBJECT_INDEX as u32) == object_index {
                        return false;
                    }
                    paid = reach;
                }
            }
            if count() >= MAXIMUM_EXPLOSIONS_PER_TICK {
                paid = false;
            }
        }
        if !paid {
            let seconds = min(HIT_REPORT_BURST_SECONDS, m.f32(hit_seconds) + elapsed as f32 / TICKS_PER_SECOND as f32);
            m.set_f32(hit_seconds, seconds);
            m.w32(record + pr::HIT_SECONDS_TIME as u32, now as u32);
            if seconds < 1.0 / rate {
                return false;
            }
            m.set_f32(hit_seconds, seconds - 1.0 / rate);
        }
        if explosion && count() < MAXIMUM_EXPLOSIONS_PER_TICK {
            let index = count();
            m.set_i16(explosions + er::COUNT as u32, index + 1);
            let explosion = entry(index);
            m.w32(explosion + ex::DEFINITION_INDEX as u32, definition_index as u32);
            for (part, value) in epicenter.into_iter().enumerate() {
                m.set_f32(explosion + ex::EPICENTER as u32 + 4 * part as u32, value);
            }
            m.w32(explosion + ex::OBJECT_INDEX as u32, object_index as u32);
        }
        true
    }

    /// `static_target`: scenery or a device damage can destroy.
    fn static_target(&self, object_index: i32) -> bool {
        self.of_types(object_index, (nv::_OBJECT_MASK_SCENERY | nv::_OBJECT_MASK_DEVICE) as u32).is_some()
            && self.call(call::MAXIMUM_BODY_VITALITY, object_index, 0, &[]).output[0] > 0.0
    }

    /// `distributed_damage_indices_valid`.
    fn indices_valid(&self, object: u32, object_index: i32, node_index: i16, region_index: i16, material_index: i16) -> bool {
        use crate::layout::{collision_model, damage_resistance, object_definition};
        let m = self.m;
        let definition = self.tag(OBJECT_TAG, m.i32(object + ud::DEFINITION_INDEX as u32));
        let collision_model_index = self.reference(definition + object_definition::OBJECT_COLLISION_MODEL as u32);
        // (the empty tag's for one not loaded: no regions, no materials)
        let collision_model = if collision_model_index != NONE {
            Some(crate::sensing::tag_get(m, &self.tags(), COLLISION_MODEL_TAG, collision_model_index))
        } else {
            None
        };
        let resistance = |offset: usize| match collision_model {
            None => None,
            Some(None) => Some(None),
            Some(Some(model)) => Some(Some(model + collision_model::RESISTANCE as u32 + offset as u32)),
        };
        let count = |block: Option<u32>| block.map_or(0, |block| tag_block_count(m, block));
        if node_index != NONE as i16
            && (node_index < 0 || self.call(call::HAS_NODE, object_index, node_index as i32, &[]).result == 0)
        {
            return false;
        }
        if region_index != NONE as i16
            && (region_index < 0
                || region_index as usize >= nv::MAXIMUM_REGIONS_PER_OBJECT
                || resistance(damage_resistance::REGIONS).is_none_or(|block| region_index as i32 >= count(block)))
        {
            return false;
        }
        if material_index != NONE as i16
            && (material_index < 0
                || resistance(damage_resistance::MATERIALS).is_none_or(|block| material_index as i32 >= count(block)))
        {
            return false;
        }
        true
    }

    /// `distributed_object_is_players`.
    fn object_is_players(&self, player_index: i16, unit_index: i32, object_index: i32) -> bool {
        let m = self.m;
        if object_index == NONE {
            return false;
        }
        if object_index == unit_index
            || (unit_index != NONE
                && object_index == m.i32(self.object(unit_index).unwrap_or(0) + ud::OBJECT_PARENT_OBJECT_INDEX as u32))
        {
            return true;
        }
        self.player(player_index).is_some_and(|player| {
            let dead_unit_index = m.i32(player + pd::DEAD_UNIT_INDEX as u32);
            dead_unit_index != NONE && object_index == dead_unit_index
        })
    }

    /// `distributed_damage_to_data` into the arguments' damage: FALSE if it
    /// cannot be.
    fn damage_to_data(&self) -> bool {
        self.damage_to_data_at(self.report(report::DEFINITION_INDEX), self.address + report::ARGUMENTS_DAMAGE)
    }

    /// `distributed_damage_to_data`: the wire's damage at `source` as this
    /// machine has it at `damage`, FALSE if it cannot be.
    fn damage_to_data_at(&self, source: u32, damage: u32) -> bool {
        let m = self.m;
        let field = |offset: usize| damage + offset as u32;
        let wire = |offset: u32| source + offset - report::DEFINITION_INDEX;
        let definition_index = m.i32(wire(report::DEFINITION_INDEX));
        if !self.is_group(definition_index, DAMAGE_EFFECT_TAG) {
            return false;
        }
        // (damage_data_new)
        for offset in 0..dd::SIZEOF as u32 {
            m.w8(damage + offset, 0);
        }
        m.w32(field(dd::DEFINITION_INDEX), definition_index as u32);
        m.set_i16(field(dd::MATERIAL_TYPE), NONE as i16);
        m.w32(field(dd::OWNER_PLAYER_INDEX), NONE as u32);
        m.w32(field(dd::OWNER_OBJECT_INDEX), NONE as u32);
        m.set_i16(field(dd::OWNER_TEAM_INDEX), NONE as i16);
        m.set_i16(field(dd::LOCATION) + 4, NONE as i16);
        m.set_f32(field(dd::SCALE), 1.0);
        m.set_f32(field(dd::MULTIPLIER), 1.0);
        // (the flags the game has)
        m.w32(field(dd::FLAGS), m.r32(wire(report::FLAGS)) & ((1 << nv::NUMBER_OF_DAMAGE_DATA_FLAGS) - 1));
        let owner_player = m.r8(wire(report::OWNER_PLAYER_INDEX));
        let owner_player_index = if owner_player != NO_PLAYER {
            self.player(owner_player as i16).map_or(NONE, |player| (m.i16(player + pd::IDENTIFIER as u32) as i32) << 16 | owner_player as i32)
        } else {
            NONE
        };
        m.w32(field(dd::OWNER_PLAYER_INDEX), owner_player_index as u32);
        m.set_i16(field(dd::OWNER_TEAM_INDEX), m.i16(wire(report::OWNER_TEAM_INDEX)));
        let owner_object_index = m.i32(wire(report::OWNER_OBJECT_INDEX));
        let owner_valid = owner_object_index != NONE && (owner_object_index >> 16) as i16 != 0 && self.object(owner_object_index).is_some();
        m.w32(field(dd::OWNER_OBJECT_INDEX), if owner_valid { owner_object_index as u32 } else { NONE as u32 });
        for (from, to) in [(report::ORIGIN, dd::ORIGIN), (report::EPICENTER, dd::EPICENTER), (report::DIRECTION, dd::DIRECTION)] {
            for part in 0..3 {
                m.w32(field(to) + 4 * part, m.r32(wire(from) + 4 * part));
            }
        }
        m.w32(field(dd::SCALE), m.r32(wire(report::SCALE)));
        m.w32(field(dd::MULTIPLIER), m.r32(wire(report::MULTIPLIER)));
        m.w32(field(dd::MATERIAL_EFFECT_SCALE), m.r32(wire(report::MATERIAL_EFFECT_SCALE)));
        let material_type = m.i16(wire(report::MATERIAL_TYPE));
        if material_type >= 0 && (material_type as usize) < nv::NUMBER_OF_MATERIAL_TYPES {
            m.set_i16(field(dd::MATERIAL_TYPE), material_type);
        }
        self.call(call::LOCATION, 0, field(dd::LOCATION) as i32, &m.vector(field(dd::EPICENTER)));
        true
    }

    /// `distributed_report_valid`: whether the host takes the report, which
    /// it makes the damage as the host deals it (into the arguments').
    fn report_valid(&self) -> bool {
        use crate::layout::damage_effect_definition as de;
        let m = self.m;
        let now = self.now();
        let player_index = m.r8(self.report(report::OWNER_PLAYER_INDEX)) as i16;
        if player_index == NO_PLAYER as i16 || player_index >= MAXIMUM_TRACKED_PLAYERS {
            return false;
        }
        let Some(player) = self.player(player_index) else { return false };
        if self.call(call::MACHINE_HAS_PLAYER, self.arguments.machine_index, player_index as i32, &[]).result == 0 {
            return false;
        }
        let object_index = m.i32(self.report(report::OBJECT_INDEX));
        if object_index == NONE || (object_index >> 16) as i16 == 0 {
            return false;
        }
        let networked = (nv::_OBJECT_MASK_BIPED | nv::_OBJECT_MASK_VEHICLE | nv::_OBJECT_MASK_WEAPON | nv::_OBJECT_MASK_EQUIPMENT) as u32;
        let mut target = self.of_types(object_index, networked);
        if target.is_none() && self.static_target(object_index) {
            target = self.object(object_index);
        }
        let definition_index = m.i32(self.report(report::DEFINITION_INDEX));
        let Some(target) = target else { return false };
        if !self.is_group(definition_index, DAMAGE_EFFECT_TAG) {
            return false;
        }
        let origin = m.vector(self.report(report::ORIGIN));
        let epicenter = m.vector(self.report(report::EPICENTER));
        let direction = m.vector(self.report(report::DIRECTION));
        let scale = m.f32(self.report(report::SCALE));
        let target_position = m.vector(self.report(report::TARGET_POSITION));
        let has_normal = m.r8(self.report(report::HAS_NORMAL)) != 0;
        let numbers_valid = point_valid(origin, REPORT_WORLD_BOUND)
            && point_valid(epicenter, REPORT_WORLD_BOUND)
            && point_valid(direction, 2.0)
            && real_valid(scale)
            && real_valid(m.f32(self.report(report::MULTIPLIER)))
            && real_valid(m.f32(self.report(report::MATERIAL_EFFECT_SCALE)));
        if !numbers_valid
            || !point_valid(target_position, REPORT_WORLD_BOUND)
            || (has_normal && !point_valid(m.vector(self.report(report::OBJECT_NORMAL)), 2.0))
            || !self.indices_valid(
                target,
                object_index,
                m.i16(self.report(report::NODE_INDEX)),
                m.i16(self.report(report::REGION_INDEX)),
                m.i16(self.report(report::MATERIAL_INDEX)),
            )
        {
            return false;
        }
        let host_time = m.i32(self.report(report::HOST_TIME));
        if host_time == NONE || host_time > now || now.wrapping_sub(host_time) > REPORT_MAXIMUM_AGE_TICKS {
            return false;
        }
        let ticks = {
            let ticks = now.wrapping_sub(host_time) + TARGET_HISTORY_SLACK_TICKS;
            if ticks > TARGET_HISTORY_TICKS - 1 { TARGET_HISTORY_TICKS - 1 } else { ticks }
        };
        let mut kinds = 0;
        let mut fired_reach = Reach::default();
        let mut grenade_type = NONE as i16;
        let rate = self.player_deals(definition_index, &mut kinds, &mut fired_reach, &mut grenade_type);
        if rate <= 0.0 {
            return false;
        }
        let mut melee = false;
        let mut collision = false;
        let area;
        if (kinds & source::IMPACT != 0 || kinds & source::AREA != 0)
            && distance_squared(origin, epicenter) <= REPORT_POINT_TOLERANCE * REPORT_POINT_TOLERANCE
        {
            area = if kinds & source::IMPACT == 0 {
                true
            } else if kinds & source::AREA == 0 {
                false
            } else {
                m.r32(self.report(report::FLAGS)) & (1 << nv::_DAMAGE_AREA_OF_EFFECT_BIT) != 0
            };
        } else if kinds & source::MELEE != 0 {
            melee = true;
            area = true;
        } else if kinds & source::COLLISION != 0 {
            collision = true;
            area = true;
        } else {
            return false;
        }
        if !(scale >= 0.0 && scale <= if melee { REPORT_MAXIMUM_MELEE_SCALE } else { 1.0 }) {
            return false;
        }
        let definition = self.tag(DAMAGE_EFFECT_TAG, definition_index);
        let cutoff_radius = m.f32(definition + de::CUTOFF_RADIUS as u32);
        let radius = m.f32(target + ud::OBJECT_BOUNDING_SPHERE_RADIUS as u32);
        let mut reach = radius + REPORT_IMPACT_TOLERANCE;
        if area {
            reach += cutoff_radius;
        }
        if !(distance_squared(origin, target_position) <= reach * reach) {
            return false;
        }
        let mut throw_index = NONE as i16;
        if grenade_type != NONE as i16 && !self.explosion_this_tick(grenade_type, Some(epicenter)) {
            throw_index = self.grenade_throw(grenade_type);
            if throw_index == NONE as i16 {
                return false;
            }
        }
        let explosion = area && !melee && !collision;
        if !self.report_paid(rate, explosion, cutoff_radius > 0.0) {
            return false;
        }
        if throw_index != NONE as i16 && explosion {
            m.w32(self.arguments.player_record + pr::THROW_TIMES as u32 + 4 * throw_index as u32, NONE as u32);
        }
        if melee && (!self.player_was_near(player_index, epicenter, ticks) || !self.player_was_near(player_index, origin, ticks)) {
            return false;
        }
        if collision && !self.player_drove_into(player_index, epicenter, target_position, radius, ticks) {
            return false;
        }
        if !melee && !collision && fired_reach.distance >= 0.0 && !self.player_could_fire(origin, &fired_reach, host_time) {
            return false;
        }
        let seen = self.target_seen(object_index, target_position, ticks);
        if seen == 0 {
            return false;
        }
        if seen == NONE as i16 {
            let object_origin = self.call(call::ORIGIN, object_index, 0, &[]).output;
            let velocity = self.call(call::VELOCITY, object_index, 0, &[]).output;
            let speed = ((velocity[0] * velocity[0] + velocity[1] * velocity[1] + velocity[2] * velocity[2]) as f64).sqrt() as f32;
            let tolerance = REPORT_TARGET_TOLERANCE + REPORT_TARGET_LEAD_TICKS * speed;
            if !(distance_squared(target_position, [object_origin[0], object_origin[1], object_origin[2]]) <= tolerance * tolerance) {
                return false;
            }
        }
        if !self.damage_to_data() {
            return false;
        }
        let damage = self.address + report::ARGUMENTS_DAMAGE;
        let field = |offset: usize| damage + offset as u32;
        let report_flags = (1 << nv::_DAMAGE_AREA_OF_EFFECT_BIT)
            | (1 << nv::_DAMAGE_CREATE_LOCALIZED_EFFECT_BIT)
            | (1 << nv::_DAMAGE_FROM_WEAPON_BIT)
            | (1 << nv::_DAMAGE_DAMAGED_ONE_OBJECT_BIT);
        let mut flags = m.r32(field(dd::FLAGS)) & report_flags;
        if area {
            flags |= 1 << nv::_DAMAGE_AREA_OF_EFFECT_BIT;
        } else {
            flags &= !(1 << nv::_DAMAGE_AREA_OF_EFFECT_BIT);
        }
        m.w32(field(dd::FLAGS), flags);
        let mut damage_direction = m.vector(field(dd::DIRECTION));
        if explosion {
            let object_origin = self.call(call::ORIGIN, object_index, 0, &[]).output;
            let center = m.vector(target + ud::OBJECT_BOUNDING_SPHERE_CENTER as u32);
            let damage_epicenter = m.vector(field(dd::EPICENTER));
            for part in 0..3 {
                damage_direction[part] = target_position[part] + center[part] - object_origin[part] - damage_epicenter[part];
            }
            let distance = crate::actor_transitions::normalize3(&mut damage_direction);
            if m.r32(definition + de::FLAGS as u32) & DONT_SCALE_DAMAGE_BY_DISTANCE == 0 {
                let falloff_radius = m.f32(definition + de::FALLOFF_RADIUS as u32);
                let radius_delta = cutoff_radius - falloff_radius;
                let scale = if radius_delta > 0.0 {
                    let value = 1.0 - (distance - falloff_radius) / radius_delta;
                    if value < 0.0 { 0.0 } else { min(value, 1.0) }
                } else {
                    1.0
                };
                m.set_f32(field(dd::SCALE), min(m.f32(field(dd::SCALE)), scale));
            }
        } else {
            crate::actor_transitions::normalize3(&mut damage_direction);
        }
        for (part, value) in damage_direction.into_iter().enumerate() {
            m.set_f32(field(dd::DIRECTION) + 4 * part as u32, value);
        }
        m.set_f32(field(dd::MULTIPLIER), 1.0);
        m.w32(field(dd::OWNER_PLAYER_INDEX), ((m.i16(player + pd::IDENTIFIER as u32) as i32) << 16 | player_index as i32) as u32);
        m.set_i16(field(dd::OWNER_TEAM_INDEX), m.i32(player + pd::TEAM_INDEX as u32) as i16);
        let unit_index = self.living_unit(player);
        if !self.object_is_players(player_index, unit_index, m.i32(field(dd::OWNER_OBJECT_INDEX))) {
            m.w32(field(dd::OWNER_OBJECT_INDEX), unit_index as u32);
        }
        true
    }

    /// `network_damage_handle_reports`, of one report: dealt once checked,
    /// counted either way.
    fn handle_report(&self) {
        let m = self.m;
        if !self.report_valid() {
            if self.arguments.rejected != 0 {
                m.w32(self.arguments.rejected, m.r32(self.arguments.rejected).wrapping_add(1));
            }
            return;
        }
        if self.arguments.dealt != 0 {
            m.w32(self.arguments.dealt, m.r32(self.arguments.dealt).wrapping_add(1));
        }
        let mut input = [0.0f32; 9];
        input[0..3].copy_from_slice(&m.vector(self.report(report::OBJECT_NORMAL)));
        input[3] = m.i16(self.report(report::NODE_INDEX)) as f32;
        input[4] = m.i16(self.report(report::REGION_INDEX)) as f32;
        input[5] = m.i16(self.report(report::MATERIAL_INDEX)) as f32;
        input[6] = (m.r8(self.report(report::HAS_NORMAL)) != 0) as i32 as f32;
        self.call(call::DEAL, m.i32(self.report(report::OBJECT_INDEX)), (self.address + report::ARGUMENTS_DAMAGE) as i32, &input);
    }
}

/// The arguments where the game holds them.
fn arguments(m: &dyn AiMemory, address: u32) -> Arguments {
    let mut bytes = [0u8; std::mem::size_of::<Arguments>()];
    for (index, byte) in bytes.iter_mut().enumerate() {
        *byte = m.r8(address + index as u32);
    }
    // SAFETY: plain data of its size
    unsafe { std::mem::transmute(bytes) }
}

/// The world step of network_damage.c's handling of a client's hit report.
pub fn handle_report(context: &WorldContext, host: &impl AiHost) {
    let m = host.memory();
    let arguments = arguments(m, context.arguments);
    let damage = Damage { context, host, m, arguments: &arguments, address: context.arguments };
    damage.handle_report();
    let _ = report::ARGUMENTS_REJECTED;
}

// ---------- the damage's hooks, the host's bookkeeping and a client's replay

/// halo_sim.h's `struct halo_sim_network_damage_hook`: the arguments of
/// network_damage.c's other steps (its bookkeeping, a client's replay of an
/// event, and damage.c's hooks: whether this machine deals damage, and the
/// host's events of what it dealt).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct HookArguments {
    /// A client's replay's event (network_damage.c's `struct
    /// distributed_damage_event`).
    pub entry: [u8; 0x6c],
    /// The damage a replay makes (`struct damage_data`), and a recoil's
    /// copy.
    pub damage: [u8; 0x54],
    /// The caller's damage (`struct damage_data`): damage.c's.
    pub damage_address: u32,
    pub object_index: i32,
    pub node_index: i16,
    pub region_index: i16,
    pub material_index: i16,
    pub has_normal: i16,
    pub normal: [f32; 3],
    pub authorized: i32,
    pub being_damaged_flags: u32,
    pub shield_damage: f32,
    pub body_damage: f32,
    pub body_damage_multiplier: f32,
    pub total_damage: f32,
    pub body_part: i16,
    pub pad: i16,
    /// The player effect's player, the aftermath's victim's.
    pub player_index: i32,
    /// `game_connection()`, `distributed_latest_host_time()`, and whether
    /// the host is dealing a client's report.
    pub connection: i32,
    pub host_time: i32,
    pub dealing_report: i32,
    /// network_damage.c's records: `damage_players`, `damage_history`,
    /// `damage_trails`, `damage_events` and their count,
    /// `damage_event_destinations`, `damage_reports` and their count, and
    /// the count of events replayed.
    pub players: u32,
    pub history: u32,
    pub trails: u32,
    pub events: u32,
    pub event_count: u32,
    pub destinations: u32,
    pub reports: u32,
    pub report_count: u32,
    pub replayed: u32,
    pub game_globals: u32,
    pub tag_instances: u32,
    pub tag_count: i32,
    pub result: i32,
}

const _: () = assert!(std::mem::size_of::<HookArguments>() == 0x6c + 0x54 + 4 * 2 + 2 * 4 + 12 + 4 * 6 + 4 + 4 * 4 + 4 * 12 + 4);

/// The result's offset.
const HOOK_RESULT: u32 = (std::mem::size_of::<HookArguments>() - 4) as u32;
/// The damage's offset.
const HOOK_DAMAGE: u32 = 0x6c;

/// More of network_damage.c's calls (halo_sim.h's `_network_damage_call_*`).
pub mod hook_call {
    /// `unit_unarmed_melee_damage(a)`, the result.
    pub const UNARMED_MELEE_DAMAGE: i32 = 9;
    /// `distributed_player_machine(a)`, the result.
    pub const PLAYER_MACHINE: i32 = 10;
    /// `damage_replay_player_effect(a, the damage at b, input[0])`.
    pub const REPLAY_PLAYER_EFFECT: i32 = 11;
    /// `damage_replay_aftermath(a, the damage at b, being damaged flags
    /// input[0]'s bits, shield input[1], body input[2], multiplier input[3],
    /// body part input[4])`.
    pub const REPLAY_AFTERMATH: i32 = 12;
    /// `distributed_set_death(a, killer input[0], friendly fire input[1],
    /// by vehicle input[2])`.
    pub const SET_DEATH: i32 = 13;
    /// The host's killing blow replayed (`damage_replay_kill(a, the damage
    /// at b, node input[0], region input[1], material input[2])`, its killer
    /// input[3]'s bits and their score input[4]'s).
    pub const REPLAY_KILL: i32 = 14;
    /// `players_show_telefragged(a)`.
    pub const TELEFRAGGED: i32 = 15;
    /// `damage_replay_static(a, the damage at b, node input[0], region
    /// input[1], material input[2])`.
    pub const REPLAY_STATIC: i32 = 16;
    /// `distributed_get_death(a)`: whether it has one the result, its killer
    /// `output[0]`, friendly fire `output[1]`, by vehicle `output[2]`.
    pub const GET_DEATH: i32 = 17;
    /// `game_engine_network_player_score(a)`, the result.
    pub const SCORE: i32 = 18;
    /// `network_objects_client_has(a)`, the result.
    pub const CLIENT_HAS: i32 = 19;
}

/// network_damage.c's event kinds and kill flags, its events' limits, and
/// game.h's connections.
const EVENT_PLAYER_EFFECT: u8 = 0;
const EVENT_AFTERMATH: u8 = 1;
const EVENT_KILL: u8 = 2;
const EVENT_STATIC: u8 = 3;
const KILL_FRIENDLY_FIRE: u8 = 1 << 0;
const KILL_BY_VEHICLE: u8 = 1 << 1;
const KILL_TELEFRAGGED: u8 = 1 << 2;
const MAXIMUM_DAMAGE_EVENTS_PER_TICK: i16 = 256;
const RESERVED_KILL_EVENTS: i16 = 64;
const MAXIMUM_HIT_REPORTS_PER_TICK: i16 = 256;
const MAXIMUM_EVENT_PLAYERS: i16 = 8;
const CONNECTION_CLIENT: i32 = 1;
const CONNECTION_SERVER: i32 = 2;

/// network_damage.c's `struct distributed_damage_event`.
mod event {
    pub const SIZEOF: u32 = 0x6c;
    pub const KIND: u32 = 0;
    pub const PLAYER_INDEX: u32 = 1;
    pub const KILL_FLAGS: u32 = 2;
    pub const OBJECT_INDEX: u32 = 4;
    pub const DAMAGE: u32 = 8;
    pub const BEING_DAMAGED_FLAGS: u32 = 76;
    pub const SHIELD_DAMAGE: u32 = 80;
    pub const BODY_DAMAGE: u32 = 84;
    pub const BODY_DAMAGE_MULTIPLIER: u32 = 88;
    pub const TOTAL_DAMAGE: u32 = 92;
    pub const BODY_PART: u32 = 96;
    pub const NODE_INDEX: u32 = 98;
    pub const REGION_INDEX: u32 = 100;
    pub const MATERIAL_INDEX: u32 = 102;
    pub const KILLER_SCORE: u32 = 104;
}

/// `struct distributed_damage`'s offsets (from its start).
mod wire {
    pub const DEFINITION_INDEX: u32 = 0;
    pub const FLAGS: u32 = 4;
    pub const OWNER_PLAYER_INDEX: u32 = 8;
    pub const OWNER_TEAM_INDEX: u32 = 10;
    pub const OWNER_OBJECT_INDEX: u32 = 12;
    pub const ORIGIN: u32 = 16;
    pub const EPICENTER: u32 = 28;
    pub const DIRECTION: u32 = 40;
    pub const SCALE: u32 = 52;
    pub const MULTIPLIER: u32 = 56;
    pub const MATERIAL_EFFECT_SCALE: u32 = 60;
    pub const MATERIAL_TYPE: u32 = 64;
    pub const SIZEOF: u32 = 0x44;
}

/// The hooks' arguments where the game holds them.
fn hook_arguments(m: &dyn AiMemory, address: u32) -> HookArguments {
    let mut bytes = [0u8; std::mem::size_of::<HookArguments>()];
    for (index, byte) in bytes.iter_mut().enumerate() {
        *byte = m.r8(address + index as u32);
    }
    // SAFETY: plain data of its size
    unsafe { std::mem::transmute(bytes) }
}

struct Hooks<'a, H: AiHost> {
    damage: Damage<'a, H>,
    hook: &'a HookArguments,
    address: u32,
}

impl<H: AiHost> Hooks<'_, H> {
    fn m(&self) -> &dyn AiMemory {
        self.damage.m
    }

    fn call(&self, kind: i32, a: i32, b: i32, input: &[f32]) -> AiCall {
        self.damage.call(kind, a, b, input)
    }

    /// `distributed_player_is_local` (a player's datum index).
    fn is_local(&self, player_index: i32) -> bool {
        player_index != NONE
            && datum_get(self.m(), self.damage.context.player_data, player_index)
                .is_some_and(|player| self.m().i16(player + pd::LOCAL_PLAYER_INDEX as u32) != NONE as i16)
    }

    /// `distributed_player_from_byte`.
    fn player_from_byte(&self, player_index: u8) -> i32 {
        if player_index == NO_PLAYER {
            return NONE;
        }
        self.damage
            .player(player_index as i16)
            .map_or(NONE, |player| (self.m().i16(player + pd::IDENTIFIER as u32) as i32) << 16 | player_index as i32)
    }

    /// `distributed_player_to_byte`.
    fn player_to_byte(player_index: i32) -> u8 {
        if player_index != NONE { player_index as u8 } else { NO_PLAYER }
    }

    /// `distributed_living_unit` of a player's datum index.
    fn living_unit_of(&self, player_index: i32) -> i32 {
        match datum_get(self.m(), self.damage.context.player_data, player_index) {
            Some(player) if player_index != NONE => self.damage.living_unit(player),
            _ => NONE,
        }
    }

    /// `distributed_damage_from_data`: the caller's damage at `damage` as
    /// the wire has it at `result`.
    fn damage_from_data(&self, damage: u32, result: u32) {
        let m = self.m();
        for offset in 0..wire::SIZEOF {
            m.w8(result + offset, 0);
        }
        m.w32(result + wire::DEFINITION_INDEX, m.r32(damage + dd::DEFINITION_INDEX as u32));
        m.w32(result + wire::FLAGS, m.r32(damage + dd::FLAGS as u32));
        m.w8(result + wire::OWNER_PLAYER_INDEX, Self::player_to_byte(m.i32(damage + dd::OWNER_PLAYER_INDEX as u32)));
        m.set_i16(result + wire::OWNER_TEAM_INDEX, m.i16(damage + dd::OWNER_TEAM_INDEX as u32));
        m.w32(result + wire::OWNER_OBJECT_INDEX, m.r32(damage + dd::OWNER_OBJECT_INDEX as u32));
        for (from, to) in [(dd::ORIGIN, wire::ORIGIN), (dd::EPICENTER, wire::EPICENTER), (dd::DIRECTION, wire::DIRECTION)] {
            for part in 0..3 {
                m.w32(result + to + 4 * part, m.r32(damage + from as u32 + 4 * part));
            }
        }
        m.w32(result + wire::SCALE, m.r32(damage + dd::SCALE as u32));
        m.w32(result + wire::MULTIPLIER, m.r32(damage + dd::MULTIPLIER as u32));
        m.w32(result + wire::MATERIAL_EFFECT_SCALE, m.r32(damage + dd::MATERIAL_EFFECT_SCALE as u32));
        m.set_i16(result + wire::MATERIAL_TYPE, m.i16(damage + dd::MATERIAL_TYPE as u32));
    }

    /// `distributed_damage_is_recoil`: a weapon's own at the unit firing it.
    fn is_recoil(&self, damage: u32, unit_index: i32) -> bool {
        let m = self.m();
        let unit = if unit_index != NONE { self.damage.of_types(unit_index, nv::_OBJECT_MASK_UNIT as u32) } else { None };
        let Some(unit) = unit else { return false };
        let flags = m.r32(damage + dd::FLAGS as u32);
        let origin = m.vector(damage + dd::ORIGIN as u32);
        let epicenter = m.vector(damage + dd::EPICENTER as u32);
        m.i32(damage + dd::OWNER_PLAYER_INDEX as u32) == NONE
            && m.i32(damage + dd::OWNER_OBJECT_INDEX as u32) == NONE
            && flags & (1 << nv::_DAMAGE_FROM_WEAPON_BIT) != 0
            && flags & (1 << nv::_DAMAGE_AREA_OF_EFFECT_BIT) == 0
            && origin == epicenter
            && epicenter == m.vector(unit + ud::OBJECT_BOUNDING_SPHERE_CENTER as u32)
    }

    /// `distributed_damage_material`: what the damage would show where this
    /// machine doesn't deal it (the material it strikes, or the shield's,
    /// and how much is left of that), at the first of the object and what it
    /// rides that takes it.
    fn damage_material(&self, damage: u32, object_index: i32, material_index: i16) {
        use crate::layout::{collision_model, damage_definition as dfn, damage_effect_definition as de, damage_resistance as dr, damage_resistance_material as rm, object_definition};
        let m = self.m();
        let d = &self.damage;
        // (the empty tag's data, all zero, for one not loaded)
        let effect = d.tag(DAMAGE_EFFECT_TAG, m.i32(damage + dd::DEFINITION_INDEX as u32));
        let definition = effect + de::DAMAGE as u32;
        let definition_flags = if effect == 0 { 0 } else { m.r32(definition + dfn::FLAGS as u32) };
        let read_f32 = |address: u32| if effect == 0 { 0.0 } else { m.f32(address) };
        let scale = m.f32(damage + dd::SCALE as u32);
        let most = ((1.0 - scale) * read_f32(definition + dfn::DAMAGE_MINIMUM as u32) + read_f32(definition + dfn::DAMAGE_UPPER_BOUND as u32) * scale)
            * m.f32(damage + dd::MULTIPLIER as u32);
        let flags = m.r32(damage + dd::FLAGS as u32);
        let mut objects = Vec::new();
        let mut index = object_index;
        while index != NONE && objects.len() < 16 {
            objects.push(index);
            if flags & (1 << nv::_DAMAGE_AREA_OF_EFFECT_BIT) != 0 || flags & (1 << nv::_DAMAGE_KILL_INSTANTLY_BIT) != 0 {
                break;
            }
            index = m.i32(d.object(index).unwrap_or(0) + ud::OBJECT_PARENT_OBJECT_INDEX as u32);
        }
        if objects.is_empty() {
            return;
        }
        // (an object's collision model's resistance, none without one)
        let resistance_of = |object: u32| -> Option<u32> {
            let definition = d.tag(OBJECT_TAG, m.i32(object + ud::DEFINITION_INDEX as u32));
            let collision_model_index = d.reference(definition + object_definition::OBJECT_COLLISION_MODEL as u32);
            if collision_model_index == NONE {
                return None;
            }
            let model = d.tag(COLLISION_MODEL_TAG, collision_model_index);
            (model != 0).then_some(model + collision_model::RESISTANCE as u32)
        };
        let read_r32 = |address: u32| m.r32(address);
        let read_i16 = |address: u32| m.i16(address);
        let read_count = |address: u32| tag_block_count(m, address);
        let mut parent_takes_body_damage = true;
        if let Some(resistance) = resistance_of(d.object(objects[0]).unwrap_or(0)) {
            parent_takes_body_damage = read_r32(resistance + dr::FLAGS as u32) & (1 << 4) == 0;
        }
        for index in (0..objects.len()).rev() {
            let object = d.object(objects[index]).unwrap_or(0);
            let Some(resistance) = resistance_of(object) else { continue };
            let resistance_flags = read_r32(resistance + dr::FLAGS as u32);
            let materials = resistance + dr::MATERIALS as u32;
            let material_count = read_count(materials);
            let indirect = read_i16(resistance + dr::INDIRECT_DAMAGE_MATERIAL_INDEX as u32);
            let material = if index == 0 && material_index >= 0 && (material_index as i32) < material_count {
                Some(tag_block_element(m, materials, material_index as i32, rm::SIZEOF))
            } else if indirect >= 0 && (indirect as i32) < material_count {
                Some(tag_block_element(m, materials, indirect as i32, rm::SIZEOF))
            } else {
                None
            };
            m.set_i16(damage + dd::MATERIAL_TYPE as u32, material.map_or(0, |material| m.i16(material + rm::MATERIAL_TYPE as u32)));
            let shield = flags & (1 << nv::_DAMAGE_BYPASSES_SHIELDS_BIT) == 0
                && definition_flags & (1 << 9) == 0
                && m.f32(object + ud::OBJECT_MAXIMUM_SHIELD_VITALITY as u32) > 0.0
                && m.f32(object + ud::OBJECT_SHIELD_VITALITY as u32) > 0.0
                && material.is_none_or(|material| m.f32(material + rm::SHIELD_LEAK_FRACTION as u32) < 1.0)
                && (index == 0 || resistance_flags & (1 << 0) != 0);
            let body = (index == 0 || (parent_takes_body_damage && resistance_flags & (1 << 1) != 0)) && definition_flags & (1 << 6) == 0;
            if most > 0.0 && (shield || (body && (resistance_flags & (1 << 5) == 0 || definition_flags & (1 << 5) != 0))) {
                if shield {
                    m.set_i16(damage + dd::MATERIAL_TYPE as u32, read_i16(resistance + dr::SHIELD_MATERIAL_TYPE as u32));
                    m.set_f32(damage + dd::MATERIAL_EFFECT_SCALE as u32, m.f32(object + ud::OBJECT_SHIELD_VITALITY as u32));
                } else {
                    let body_vitality = m.f32(object + ud::OBJECT_BODY_VITALITY as u32);
                    let pinned = if body_vitality < 0.0 { 0.0 } else { min(body_vitality, 1.0) };
                    m.set_f32(damage + dd::MATERIAL_EFFECT_SCALE as u32, pinned);
                }
                return;
            }
            if body {
                return;
            }
        }
    }

    /// `network_damage_deals`: whether this machine deals the damage. A
    /// client none (its own player's hit on the host's object reported, a
    /// weapon's own shake of its own player shown) but carrying out the
    /// host's word; the host all but a client's player's (whose machine
    /// reports it) unless dealing that report, or the machine has left.
    /// Where it doesn't, what the damage would show is filled in.
    fn deals(&self) -> bool {
        let m = self.m();
        let hook = self.hook;
        let damage = hook.damage_address;
        let object_index = hook.object_index;
        let owner_player_index = m.i32(damage + dd::OWNER_PLAYER_INDEX as u32);
        if hook.connection == CONNECTION_CLIENT {
            if hook.authorized != 0 {
                return true;
            }
            if let Some(unit) = self.damage.of_types(object_index, nv::_OBJECT_MASK_UNIT as u32) {
                let player_index = m.i32(unit + ud::UNIT_PLAYER_INDEX as u32);
                if player_index != NONE
                    && self.is_local(player_index)
                    && self.living_unit_of(player_index) == object_index
                    && self.is_recoil(damage, object_index)
                {
                    let copy = self.address + HOOK_DAMAGE;
                    for offset in 0..dd::SIZEOF as u32 {
                        m.w8(copy + offset, m.r8(damage + offset));
                    }
                    self.call(hook_call::REPLAY_PLAYER_EFFECT, player_index, copy as i32, &[0.0]);
                }
            }
            let count = m.i16(hook.report_count);
            if self.is_local(owner_player_index)
                && (self.call(hook_call::CLIENT_HAS, object_index, 0, &[]).result != 0
                    || self.damage.static_target(object_index))
                && count < MAXIMUM_HIT_REPORTS_PER_TICK
            {
                m.set_i16(hook.report_count, count + 1);
                let report = hook.reports + count as u32 * 0x6c;
                for offset in 0..0x6c {
                    m.w8(report + offset, 0);
                }
                m.w32(report + report::OBJECT_INDEX, object_index as u32);
                self.damage_from_data(damage, report + report::DEFINITION_INDEX);
                let origin = self.call(call::ORIGIN, object_index, 0, &[]).output;
                for part in 0..3 {
                    m.set_f32(report + report::TARGET_POSITION + 4 * part, origin[part as usize]);
                }
                m.set_i16(report + report::NODE_INDEX, hook.node_index);
                m.set_i16(report + report::REGION_INDEX, hook.region_index);
                m.set_i16(report + report::MATERIAL_INDEX, hook.material_index);
                m.w32(report + report::HOST_TIME, hook.host_time as u32);
                if hook.has_normal != 0 {
                    for part in 0..3 {
                        m.set_f32(report + report::OBJECT_NORMAL + 4 * part, hook.normal[part as usize]);
                    }
                    m.w8(report + report::HAS_NORMAL, 1);
                }
            }
            self.damage_material(damage, object_index, hook.material_index);
            return false;
        }
        if hook.connection == CONNECTION_SERVER && hook.dealing_report == 0 && owner_player_index != NONE && !self.is_local(owner_player_index) {
            let owner = datum_get(m, self.damage.context.player_data, owner_player_index);
            if let Some(owner) = owner {
                if m.r8(owner + pd::QUIT_OUT_OF_GAME as u32) == 0
                    && self.call(hook_call::PLAYER_MACHINE, owner_player_index & 0xffff, 0, &[]).result != NONE
                {
                    self.damage_material(damage, object_index, hook.material_index);
                    return false;
                }
            }
        }
        true
    }

    /// network_damage.c's `damage_event_new`: an event of this tick (the
    /// last RESERVED_KILL_EVENTS kept for killing blows), or none.
    fn event_new(&self, kind: u8, kill: bool) -> Option<u32> {
        let m = self.m();
        let hook = self.hook;
        let count = m.i16(hook.event_count);
        if count >= MAXIMUM_DAMAGE_EVENTS_PER_TICK - if kill { 0 } else { RESERVED_KILL_EVENTS } {
            return None;
        }
        m.set_i16(hook.event_count, count + 1);
        let event = hook.events + count as u32 * event::SIZEOF;
        for offset in 0..event::SIZEOF {
            m.w8(event + offset, 0);
        }
        m.w8(event + event::KIND, kind);
        m.w8(event + event::PLAYER_INDEX, NO_PLAYER);
        m.w32(event + event::OBJECT_INDEX, hook.object_index as u32);
        self.damage_from_data(hook.damage_address, event + event::DAMAGE);
        m.set_i16(event + event::NODE_INDEX, hook.node_index);
        m.set_i16(event + event::REGION_INDEX, hook.region_index);
        m.set_i16(event + event::MATERIAL_INDEX, hook.material_index);
        Some(event)
    }

    /// `network_damage_player_effect`: the host's event of a client's
    /// player's screen shaken and flashed (not a weapon's own shake of the
    /// player firing it).
    fn player_effect(&self) {
        let m = self.m();
        let hook = self.hook;
        let player_index = hook.player_index;
        if hook.connection != CONNECTION_SERVER
            || m.i16(hook.event_count) >= MAXIMUM_DAMAGE_EVENTS_PER_TICK - RESERVED_KILL_EVENTS
            || self.is_local(player_index)
            || self.is_recoil(hook.damage_address, self.living_unit_of(player_index))
        {
            return;
        }
        let count = m.i16(hook.event_count);
        m.set_i16(hook.event_count, count + 1);
        let event = hook.events + count as u32 * event::SIZEOF;
        for offset in 0..event::SIZEOF {
            m.w8(event + offset, 0);
        }
        m.w8(event + event::KIND, EVENT_PLAYER_EFFECT);
        m.w8(event + event::PLAYER_INDEX, Self::player_to_byte(player_index));
        m.w32(event + event::OBJECT_INDEX, NONE as u32);
        self.damage_from_data(hook.damage_address, event + event::DAMAGE);
        m.set_f32(event + event::TOTAL_DAMAGE, hook.total_damage);
    }

    /// `network_damage_aftermath`: the host's event of an object damaged
    /// (destructible scenery's; a unit's, a killing blow's with who the host
    /// says dealt it).
    fn aftermath(&self) {
        let m = self.m();
        let hook = self.hook;
        let object_index = hook.object_index;
        if hook.connection != CONNECTION_SERVER {
            return;
        }
        if self.damage.static_target(object_index) {
            self.event_new(EVENT_STATIC, false);
            return;
        }
        let Some(unit) = self.damage.of_types(object_index, nv::_OBJECT_MASK_UNIT as u32) else { return };
        let flags = hook.being_damaged_flags;
        let victim = hook.player_index;
        let biped = (nv::_OBJECT_MASK_BIPED as u32) & (1 << m.r8(unit + ud::OBJECT_TYPE as u32)) != 0;
        let kill = flags & (1 << nv::_OBJECT_BEING_DAMAGED_BODY_DEPLETED_BIT) != 0
            && (victim != NONE || (biped && flags & (1 << nv::_OBJECT_BEING_DAMAGED_BODY_DESTROYED_BIT) == 0));
        let Some(event) = self.event_new(EVENT_AFTERMATH, kill) else { return };
        m.w32(event + event::BEING_DAMAGED_FLAGS, flags);
        m.set_f32(event + event::SHIELD_DAMAGE, hook.shield_damage);
        m.set_f32(event + event::BODY_DAMAGE, hook.body_damage);
        m.set_f32(event + event::BODY_DAMAGE_MULTIPLIER, hook.body_damage_multiplier);
        m.set_i16(event + event::BODY_PART, hook.body_part);
        if !kill {
            return;
        }
        m.w8(event + event::KIND, EVENT_KILL);
        let mut killer = NO_PLAYER;
        let mut friendly_fire = false;
        let mut by_vehicle = false;
        if victim != NONE {
            let death = self.call(hook_call::GET_DEATH, victim & 0xffff, 0, &[]);
            if death.result != 0 {
                killer = death.output[0] as i32 as u8;
                friendly_fire = death.output[1] != 0.0;
                by_vehicle = death.output[2] != 0.0;
            }
        }
        m.w8(event + event::PLAYER_INDEX, killer);
        let mut kill_flags = 0;
        if friendly_fire {
            kill_flags |= KILL_FRIENDLY_FIRE;
        }
        if by_vehicle {
            kill_flags |= KILL_BY_VEHICLE;
        }
        if victim != NONE
            && datum_get(m, self.damage.context.player_data, victim).is_some_and(|player| m.i32(player + pd::TELEFRAG_TIMEOUT as u32) >= 90)
        {
            kill_flags |= KILL_TELEFRAGGED;
        }
        m.w8(event + event::KILL_FLAGS, kill_flags);
        if killer != NO_PLAYER {
            let killer_index = self.player_from_byte(killer);
            if killer_index != NONE {
                m.w32(event + event::KILLER_SCORE, self.call(hook_call::SCORE, killer_index, 0, &[]).result as u32);
            }
        }
    }

    /// network_damage.c's `network_damage_handle_events`, of one event: a
    /// client's replay of the host's damage (its local player's screen
    /// effect; a unit's flinch and the rest but its death; a biped's killing
    /// blow whole, with the host's killer; destructible scenery's).
    fn replay(&self) {
        let m = self.m();
        let entry = self.address;
        let source = entry + event::DAMAGE;
        let damage = self.address + HOOK_DAMAGE;
        let numbers_valid = point_valid(m.vector(source + wire::ORIGIN), REPORT_WORLD_BOUND)
            && point_valid(m.vector(source + wire::EPICENTER), REPORT_WORLD_BOUND)
            && point_valid(m.vector(source + wire::DIRECTION), 2.0)
            && real_valid(m.f32(source + wire::SCALE))
            && real_valid(m.f32(source + wire::MULTIPLIER))
            && real_valid(m.f32(source + wire::MATERIAL_EFFECT_SCALE));
        if !numbers_valid
            || !self.damage.damage_to_data_at(source, damage)
            || !real_valid(m.f32(entry + event::SHIELD_DAMAGE))
            || !real_valid(m.f32(entry + event::BODY_DAMAGE))
            || !real_valid(m.f32(entry + event::BODY_DAMAGE_MULTIPLIER))
            || !real_valid(m.f32(entry + event::TOTAL_DAMAGE))
        {
            return;
        }
        let replayed = self.hook.replayed;
        if replayed != 0 {
            m.w32(replayed, m.r32(replayed).wrapping_add(1));
        }
        let object_index = m.i32(entry + event::OBJECT_INDEX);
        let node_index = m.i16(entry + event::NODE_INDEX);
        let region_index = m.i16(entry + event::REGION_INDEX);
        let material_index = m.i16(entry + event::MATERIAL_INDEX);
        let client_has = |object_index: i32| self.call(hook_call::CLIENT_HAS, object_index, 0, &[]).result != 0;
        match m.r8(entry + event::KIND) {
            EVENT_PLAYER_EFFECT => {
                let player_index = self.player_from_byte(m.r8(entry + event::PLAYER_INDEX));
                if self.is_local(player_index) {
                    self.call(hook_call::REPLAY_PLAYER_EFFECT, player_index, damage as i32, &[m.f32(entry + event::TOTAL_DAMAGE)]);
                }
            }
            EVENT_AFTERMATH => {
                if client_has(object_index) && self.damage.of_types(object_index, nv::_OBJECT_MASK_UNIT as u32).is_some() {
                    let flags = m.r32(entry + event::BEING_DAMAGED_FLAGS)
                        & !((1 << nv::_OBJECT_BEING_DAMAGED_BODY_DEPLETED_BIT) | (1 << nv::_OBJECT_BEING_DAMAGED_KILLED_INSTANTLY_BIT));
                    self.call(
                        hook_call::REPLAY_AFTERMATH,
                        object_index,
                        damage as i32,
                        &[
                            f32::from_bits(flags),
                            m.f32(entry + event::SHIELD_DAMAGE),
                            m.f32(entry + event::BODY_DAMAGE),
                            m.f32(entry + event::BODY_DAMAGE_MULTIPLIER),
                            m.i16(entry + event::BODY_PART) as f32,
                        ],
                    );
                }
            }
            EVENT_KILL => {
                if !client_has(object_index) {
                    return;
                }
                let Some(unit) = self.damage.of_types(object_index, nv::_OBJECT_MASK_BIPED as u32) else { return };
                if m.r16(unit + ud::OBJECT_DAMAGE_FLAGS as u32) as u32 & (1 << nv::_OBJECT_DEAD_BIT) != 0
                    || !self.damage.indices_valid(unit, object_index, node_index, region_index, material_index)
                {
                    return;
                }
                let victim = m.i32(unit + ud::UNIT_PLAYER_INDEX as u32);
                let killer = m.r8(entry + event::PLAYER_INDEX);
                let kill_flags = m.r8(entry + event::KILL_FLAGS);
                if victim != NONE {
                    self.call(
                        hook_call::SET_DEATH,
                        victim & 0xffff,
                        0,
                        &[killer as f32, (kill_flags & KILL_FRIENDLY_FIRE != 0) as i32 as f32, (kill_flags & KILL_BY_VEHICLE != 0) as i32 as f32],
                    );
                } else {
                    m.w32(damage + dd::FLAGS as u32, m.r32(damage + dd::FLAGS as u32) | 1 << nv::_DAMAGE_NO_STATISTICS_BIT);
                }
                let killer_index = if killer != NO_PLAYER { self.player_from_byte(killer) } else { NONE };
                self.call(
                    hook_call::REPLAY_KILL,
                    object_index,
                    damage as i32,
                    &[
                        node_index as f32,
                        region_index as f32,
                        material_index as f32,
                        f32::from_bits(killer_index as u32),
                        f32::from_bits(m.r32(entry + event::KILLER_SCORE)),
                    ],
                );
                if victim != NONE && kill_flags & KILL_TELEFRAGGED != 0 {
                    self.call(hook_call::TELEFRAGGED, victim, 0, &[]);
                }
            }
            EVENT_STATIC => {
                if object_index != NONE
                    && (object_index >> 16) as i16 != 0
                    && self.damage.static_target(object_index)
                    && self.damage.indices_valid(self.damage.object(object_index).unwrap_or(0), object_index, node_index, region_index, material_index)
                {
                    self.call(
                        hook_call::REPLAY_STATIC,
                        object_index,
                        damage as i32,
                        &[node_index as f32, region_index as f32, material_index as f32],
                    );
                }
            }
            _ => {}
        }
    }

    /// network_damage.c's `distributed_note_weapons`: the weapons each
    /// player carries (and their vehicle's, driving or gunning), their
    /// unarmed blow, and whether they drive, noted.
    fn note_weapons(&self) {
        use crate::data_array::{DATA_COUNT, DATA_DATA, DATA_SIZE};
        let m = self.m();
        let d = &self.damage;
        let now = d.now();
        let data = d.context.player_data;
        let size = m.r16(data + DATA_SIZE) as i16 as u32;
        for player_index in 0..m.r16(data + DATA_COUNT) as i16 as u32 {
            let player = m.r32(data + DATA_DATA) + player_index * size;
            if m.i16(player + pd::IDENTIFIER as u32) == 0 {
                continue;
            }
            let unit_index = d.living_unit(player);
            if player_index as i16 >= MAXIMUM_TRACKED_PLAYERS || unit_index == NONE {
                continue;
            }
            let record = self.hook.players + player_index * pr::SIZEOF as u32;
            let unarmed = self.call(hook_call::UNARMED_MELEE_DAMAGE, unit_index, 0, &[]).result;
            m.w32(record + pr::UNARMED_MELEE_DAMAGE_INDEX as u32, unarmed as u32);
            m.w32(record + pr::UNARMED_TIME as u32, now as u32);
            let parent = m.i32(d.object(unit_index).unwrap_or(0) + ud::OBJECT_PARENT_OBJECT_INDEX as u32);
            let units = [unit_index, parent];
            if parent != NONE {
                if let Some(vehicle) = d.of_types(parent, nv::_OBJECT_MASK_VEHICLE as u32) {
                    if m.i32(vehicle + ud::UNIT_DRIVER_OBJECT_INDEX as u32) == unit_index {
                        m.w32(record + pr::DRIVEN_TIME as u32, now as u32);
                    }
                }
            }
            for (unit_number, &index) in units.iter().enumerate() {
                let unit = if index != NONE { d.of_types(index, nv::_OBJECT_MASK_UNIT as u32) } else { None };
                let Some(unit) = unit else { continue };
                if unit_number == 1
                    && m.i32(unit + ud::UNIT_DRIVER_OBJECT_INDEX as u32) != unit_index
                    && m.i32(unit + ud::UNIT_GUNNER_OBJECT_INDEX as u32) != unit_index
                {
                    continue;
                }
                for slot in 0..4u32 {
                    let weapon_index = m.i32(unit + ud::UNIT_WEAPON_OBJECT_INDICES as u32 + 4 * slot);
                    if weapon_index == NONE {
                        continue;
                    }
                    let Some(weapon) = d.object(weapon_index) else { continue };
                    let definition_index = m.i32(weapon + ud::DEFINITION_INDEX as u32);
                    let mut oldest = 0u32;
                    let mut found = MAXIMUM_RECENT_WEAPONS;
                    for index in 0..MAXIMUM_RECENT_WEAPONS {
                        if m.i32(record + pr::DEFINITION_INDICES as u32 + 4 * index) == definition_index {
                            found = index;
                            break;
                        }
                        if m.i32(record + pr::TIMES as u32 + 4 * index) < m.i32(record + pr::TIMES as u32 + 4 * oldest) {
                            oldest = index;
                        }
                    }
                    if found == MAXIMUM_RECENT_WEAPONS {
                        found = oldest;
                        m.w32(record + pr::DEFINITION_INDICES as u32 + 4 * found, definition_index as u32);
                    }
                    m.w32(record + pr::TIMES as u32 + 4 * found, now as u32);
                }
            }
        }
    }

    /// network_damage.c's `distributed_note_targets`: where each player's
    /// unit and vehicle are this tick (and whether they drive it), and every
    /// few ticks where their unit is, how fast, and the size of its vehicle.
    fn note_targets(&self) {
        let m = self.m();
        let d = &self.damage;
        let now = d.now();
        let tick = self.hook.history + (now & (TARGET_HISTORY_TICKS - 1)) as u32 * ht::SIZEOF as u32;
        m.w32(tick + ht::TIME as u32, now as u32);
        for player_index in 0..MAXIMUM_TRACKED_PLAYERS {
            let unit_index = d.player(player_index).map_or(NONE, |player| d.living_unit(player));
            let object = |slot: u32| tick + ht::OBJECTS as u32 + (player_index as u32 * 2 + slot) * ho::SIZEOF as u32;
            m.w32(object(0) + ho::OBJECT_INDEX as u32, unit_index as u32);
            let parent = if unit_index != NONE { m.i32(d.object(unit_index).unwrap_or(0) + ud::OBJECT_PARENT_OBJECT_INDEX as u32) } else { NONE };
            m.w32(object(1) + ho::OBJECT_INDEX as u32, parent as u32);
            m.w8(object(0) + ho::DRIVING as u32, 0);
            m.w8(object(1) + ho::DRIVING as u32, 0);
            for slot in 0..2 {
                let object_index = m.i32(object(slot) + ho::OBJECT_INDEX as u32);
                if object_index == NONE {
                    continue;
                }
                let origin = self.call(call::ORIGIN, object_index, 0, &[]).output;
                for part in 0..3 {
                    m.set_f32(object(slot) + ho::POSITION as u32 + 4 * part, origin[part as usize]);
                }
                let v = self.call(call::VELOCITY, object_index, 0, &[]).output;
                m.set_f32(object(slot) + ho::SPEED as u32, ((v[0] * v[0] + v[1] * v[1] + v[2] * v[2]) as f64).sqrt() as f32);
            }
            if parent != NONE {
                let driving = d
                    .of_types(parent, nv::_OBJECT_MASK_VEHICLE as u32)
                    .is_some_and(|vehicle| m.i32(vehicle + ud::UNIT_DRIVER_OBJECT_INDEX as u32) == unit_index);
                m.w8(object(1) + ho::DRIVING as u32, driving as u8);
            }
            if now % PLAYER_TRAIL_INTERVAL_TICKS == 0 {
                let point = self.hook.trails
                    + (player_index as u32 * PLAYER_TRAIL_POINTS + ((now / PLAYER_TRAIL_INTERVAL_TICKS) & (PLAYER_TRAIL_POINTS as i32 - 1)) as u32)
                        * tp::SIZEOF as u32;
                m.w32(point + tp::TIME as u32, if unit_index != NONE { now as u32 } else { NONE as u32 });
                if unit_index != NONE {
                    for part in 0..3 {
                        m.w32(point + tp::POSITION as u32 + 4 * part, m.r32(object(0) + ho::POSITION as u32 + 4 * part));
                    }
                    let mut speed = m.f32(object(0) + ho::SPEED as u32);
                    let mut radius = 0.0;
                    if parent != NONE {
                        speed = max(speed, m.f32(object(1) + ho::SPEED as u32));
                        radius = m.f32(d.object(parent).unwrap_or(0) + ud::OBJECT_BOUNDING_SPHERE_RADIUS as u32);
                    }
                    m.set_f32(point + tp::SPEED as u32, speed);
                    m.set_f32(point + tp::RADIUS as u32, radius);
                }
            }
        }
    }

    /// network_damage.c's `distributed_note_event_destinations`: where each
    /// damage event of this tick goes (a player effect to its player's
    /// machine; damage to a unit to its player's, its riders', those sent
    /// them, and its owner's; anything else to all).
    fn note_event_destinations(&self) {
        use crate::layout::damage_event_destination as dst;
        use crate::data_array::{DATA_COUNT, DATA_DATA, DATA_SIZE};
        let m = self.m();
        let d = &self.damage;
        let now = d.now();
        let hook = self.hook;
        let tick = hook.history + (now & (TARGET_HISTORY_TICKS - 1)) as u32 * ht::SIZEOF as u32;
        for index in 0..m.i16(hook.event_count).max(0) as u32 {
            let event = hook.events + index * event::SIZEOF;
            let destination = hook.destinations + index * dst::SIZEOF as u32;
            let count_address = destination + dst::PLAYER_COUNT as u32;
            let indices = destination + dst::PLAYER_INDICES as u32;
            m.w32(destination + dst::OWNER_MACHINE_INDEX as u32, NONE as u32);
            let mut count: i16 = 0;
            m.set_i16(count_address, count);
            let kind = m.r8(event + event::KIND);
            if kind == EVENT_PLAYER_EFFECT {
                m.set_i16(indices, m.r8(event + event::PLAYER_INDEX) as i16);
                m.set_i16(count_address, 1);
                continue;
            }
            if kind != EVENT_AFTERMATH {
                m.set_i16(count_address, NONE as i16);
                continue;
            }
            let owner = m.r8(event + event::DAMAGE + wire::OWNER_PLAYER_INDEX);
            if owner != NO_PLAYER {
                let machine = self.call(hook_call::PLAYER_MACHINE, owner as i32, 0, &[]).result;
                m.w32(destination + dst::OWNER_MACHINE_INDEX as u32, machine as u32);
            }
            let object_index = m.i32(event + event::OBJECT_INDEX);
            if let Some(unit) = d.of_types(object_index, nv::_OBJECT_MASK_UNIT as u32) {
                let mut unit_player_index = m.i32(unit + ud::UNIT_PLAYER_INDEX as u32);
                if unit_player_index == NONE {
                    let data = d.context.player_data;
                    let size = m.r16(data + DATA_SIZE) as i16 as u32;
                    for player_index in 0..m.r16(data + DATA_COUNT) as i16 as u32 {
                        let player = m.r32(data + DATA_DATA) + player_index * size;
                        if m.i16(player + pd::IDENTIFIER as u32) != 0 && m.i32(player + pd::DEAD_UNIT_INDEX as u32) == object_index {
                            unit_player_index = (m.i16(player + pd::IDENTIFIER as u32) as i32) << 16 | player_index as i32;
                            break;
                        }
                    }
                }
                if unit_player_index != NONE {
                    m.set_i16(indices + 2 * count as u32, unit_player_index as i16);
                    count += 1;
                }
            }
            m.set_i16(count_address, count);
            if m.i32(tick + ht::TIME as u32) != now {
                continue;
            }
            for player_index in 0..MAXIMUM_TRACKED_PLAYERS {
                let object = |slot: u32| tick + ht::OBJECTS as u32 + (player_index as u32 * 2 + slot) * ho::SIZEOF as u32;
                if (m.i32(object(0) + ho::OBJECT_INDEX as u32) == object_index || m.i32(object(1) + ho::OBJECT_INDEX as u32) == object_index)
                    && (count == 0 || m.i16(indices) != player_index)
                {
                    if count >= MAXIMUM_EVENT_PLAYERS {
                        count = NONE as i16;
                        m.set_i16(count_address, count);
                        break;
                    }
                    m.set_i16(indices + 2 * count as u32, player_index);
                    count += 1;
                    m.set_i16(count_address, count);
                }
            }
        }
    }
}

/// A hook's step, run.
fn run_hook<H: AiHost>(context: &WorldContext, host: &H, run: impl FnOnce(&Hooks<'_, H>)) {
    let m = host.memory();
    let hook = hook_arguments(m, context.arguments);
    // (the report's arguments the shared helpers read: the tags and globals)
    // SAFETY: plain data, zeroed
    let mut arguments: Arguments = unsafe { std::mem::zeroed() };
    arguments.history = hook.history;
    arguments.game_globals = hook.game_globals;
    arguments.tag_instances = hook.tag_instances;
    arguments.tag_count = hook.tag_count;
    let hooks = Hooks { damage: Damage { context, host, m, arguments: &arguments, address: 0 }, hook: &hook, address: context.arguments };
    run(&hooks);
}

/// The world step of the host's damage bookkeeping a tick (network_damage.c's
/// `distributed_note_weapons`, `_note_targets`, `_note_event_destinations`).
pub fn bookkeeping(context: &WorldContext, host: &impl AiHost) {
    run_hook(context, host, |hooks| {
        hooks.note_weapons();
        hooks.note_targets();
        hooks.note_event_destinations();
    });
}

/// The world step of a client's replay of one of the host's damage events.
pub fn replay(context: &WorldContext, host: &impl AiHost) {
    run_hook(context, host, |hooks| hooks.replay());
}

/// The world step of `network_damage_deals`.
pub fn deals(context: &WorldContext, host: &impl AiHost) {
    run_hook(context, host, |hooks| {
        let result = hooks.deals();
        hooks.m().w32(hooks.address + HOOK_RESULT, result as u32);
    });
}

/// The world step of `network_damage_player_effect`.
pub fn player_effect(context: &WorldContext, host: &impl AiHost) {
    run_hook(context, host, |hooks| hooks.player_effect());
}

/// The world step of `network_damage_aftermath`.
pub fn aftermath(context: &WorldContext, host: &impl AiHost) {
    run_hook(context, host, |hooks| hooks.aftermath());
}
