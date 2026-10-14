//! Network co-op's extra enemies (OpenCE's port/linux/game/coop_enemies.c):
//! the host's update, which places the riders a script loaded into a
//! dropship past its seats.
//!
//! A squad a script loads into a dropship can be larger than its seats
//! (Server Setup's EXTRA ENEMIES grows it). The riders left without a seat
//! are erased and kept; once the dropship's own riders get out, they are
//! placed beside those riders, a few ticks apart, and drop and fight as they
//! do. A dropship gone first takes them with it.
//!
//! The state is the game's memory (coop_enemies_records.h), the step's
//! arguments a struct the game passes ([`Arguments`]); finding free ground
//! (collision tests against the level and its objects), a seat's marker and
//! placing an actor are the game's, through [`AiHost::call`] (coop_enemies.c's
//! `coop_enemies_world_call`).

use crate::ai_actor::{datum_get, AiCall, AiHost, AiMemory, AiMemoryExt, NONE, WorldContext};
use crate::layout::{
    coop_enemies_state as es, coop_kept_rider as kr, coop_riding_vehicle as rv, net_values as nv,
    object_header_datum as oh, unit_datum as ud,
};

/// coop_enemies.c's calls (halo_sim.h's `_coop_enemies_call_*`).
pub mod call {
    /// Free ground about `input[0..3]` for the `a`'th enemy: `result`, and
    /// `output[0..3]`.
    pub const SPREAD_POSITION: i32 = 1;
    /// The vehicle's (`a`) seat's (`b`) marker: `result`, and `output[0..3]`.
    pub const SEAT_MARKER: i32 = 2;
    /// Places a kept rider of the variant `a`, the encounter `b` and the
    /// squad `actor` at `input[0..3]` facing `input[3]`.
    pub const PLACE_RIDER: i32 = 3;
}

/// halo_sim.h's `struct halo_sim_coop_enemies`.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct Arguments {
    pub state: u32,
}

/// coop_enemies.c's `RIDER_RELEASE_TICKS`: a kept rider placed every this
/// many ticks, as the seated ones jumped out.
const RIDER_RELEASE_TICKS: i32 = 4;
/// coop_enemies.c's `SPREAD_PLACES_PER_RING` and `RIDER_SPACING`: a kept
/// rider's place beside a rider who got out, each a little apart.
const SPREAD_PLACES_PER_RING: i16 = 6;
const RIDER_SPACING: f32 = 0.5;
/// real_math.h's `_pi`.
const PI: f32 = std::f32::consts::PI;

struct Update<'a, H: AiHost> {
    context: &'a WorldContext,
    host: &'a H,
    m: &'a dyn AiMemory,
    state: u32,
}

impl<H: AiHost> Update<'_, H> {
    fn vehicle(&self, number: i16) -> u32 {
        self.state + es::VEHICLES as u32 + number as u32 * rv::SIZEOF as u32
    }

    fn rider(&self, index: i16) -> u32 {
        self.state + es::RIDERS as u32 + index as u32 * kr::SIZEOF as u32
    }

    fn copy(&self, to: u32, from: u32, size: usize) {
        for offset in 0..size as u32 {
            self.m.w8(to + offset, self.m.r8(from + offset));
        }
    }

    /// `object_try_and_get_and_verify_type(.., _object_mask_unit)`: the
    /// unit's datum.
    fn unit(&self, object_index: i32) -> Option<u32> {
        let header = datum_get(self.m, self.context.object_header_data, object_index)?;
        (nv::_OBJECT_MASK_UNIT as u32 & (1 << self.m.r8(header + oh::TYPE as u32)) != 0)
            .then(|| self.m.r32(header + oh::DATUM as u32))
    }

    fn dead(&self, object: u32) -> bool {
        self.m.r16(object + ud::OBJECT_DAMAGE_FLAGS as u32) & (1 << nv::_OBJECT_DEAD_BIT) != 0
    }

    fn point(&self, at: u32) -> [f32; 3] {
        [self.m.f32(at), self.m.f32(at + 4), self.m.f32(at + 8)]
    }

    /// The vehicle's seated riders that are alive and out of it.
    fn riders_out(&self, vehicle: u32) -> Vec<u32> {
        let m = self.m;
        let vehicle_index = m.i32(vehicle + rv::VEHICLE_INDEX as u32);
        (0..m.i16(vehicle + rv::SEATED_COUNT as u32).max(0) as u32)
            .filter_map(|index| self.unit(m.i32(vehicle + rv::SEATED_UNIT_INDICES as u32 + 4 * index)))
            .filter(|&unit| !self.dead(unit) && m.i32(unit + ud::OBJECT_PARENT_OBJECT_INDEX as u32) != vehicle_index)
            .collect()
    }

    /// coop_enemies.c's `coop_enemies_forget_vehicle`: the riders kept for
    /// a vehicle forgotten, and the vehicle with them.
    fn forget_vehicle(&self, number: i16) {
        let m = self.m;
        let vehicle_index = m.i32(self.vehicle(number) + rv::VEHICLE_INDEX as u32);
        let mut kept: i16 = 0;
        for index in 0..m.i16(self.state + es::RIDER_COUNT as u32) {
            if m.i32(self.rider(index) + kr::VEHICLE_INDEX as u32) != vehicle_index {
                if kept != index {
                    self.copy(self.rider(kept), self.rider(index), kr::SIZEOF);
                }
                kept += 1;
            }
        }
        m.set_i16(self.state + es::RIDER_COUNT as u32, kept);
        let count = m.i16(self.state + es::VEHICLE_COUNT as u32) - 1;
        m.set_i16(self.state + es::VEHICLE_COUNT as u32, count);
        if count != number {
            self.copy(self.vehicle(number), self.vehicle(count), rv::SIZEOF);
        }
    }

    /// coop_enemies.c's `coop_enemies_rider_place`: beside one of the riders
    /// who got out (a ring further about them each round), else at a seat's
    /// marker, else the vehicle's place; free ground about it if there is
    /// some, else a ring about it.
    fn rider_place(&self, vehicle: u32, number: i16) -> ([f32; 3], f32) {
        let m = self.m;
        let vehicle_index = m.i32(vehicle + rv::VEHICLE_INDEX as u32);
        let object = self.unit(vehicle_index).expect("a vehicle the update found");
        let mut position = self.point(object + ud::OBJECT_POSITION as u32);
        let forward = self.point(object + ud::OBJECT_FORWARD as u32);
        let facing = (forward[1] as f64).atan2(forward[0] as f64) as f32;
        let out = self.riders_out(vehicle);
        let round;
        if !out.is_empty() {
            position = self.point(out[number as usize % out.len()] + ud::OBJECT_POSITION as u32);
            round = (number as i32 / out.len() as i32) as i16;
        } else {
            round = number;
            if m.i16(vehicle + rv::SEATED_COUNT as u32) > 0 {
                let mut call = AiCall {
                    kind: call::SEAT_MARKER,
                    actor: NONE,
                    a: vehicle_index,
                    b: m.i16(vehicle + rv::SEAT_INDICES as u32) as i32,
                    ..AiCall::default()
                };
                self.host.call(&mut call);
                if call.result != 0 {
                    position = call.vector();
                }
            }
        }
        let mut call = AiCall { kind: call::SPREAD_POSITION, actor: NONE, a: number as i32 + 1, ..AiCall::default() };
        call.input[..3].copy_from_slice(&position);
        self.host.call(&mut call);
        if call.result != 0 {
            return (call.vector(), facing);
        }
        let angle = number as f32 * (PI * 2.0 / SPREAD_PLACES_PER_RING as f32);
        let distance = RIDER_SPACING * (round / SPREAD_PLACES_PER_RING + 1) as f32;
        position[0] += (angle as f64).cos() as f32 * distance;
        position[1] += (angle as f64).sin() as f32 * distance;
        (position, facing)
    }

    /// coop_enemies.c's `coop_enemies_update`.
    fn run(&self) {
        let m = self.m;
        let now = self.context.game_time;
        for number in (0..m.i16(self.state + es::VEHICLE_COUNT as u32)).rev() {
            let vehicle = self.vehicle(number);
            let vehicle_index = m.i32(vehicle + rv::VEHICLE_INDEX as u32);
            let rider_count = m.i16(self.state + es::RIDER_COUNT as u32);
            // (the vehicle's next kept rider)
            let index = (0..rider_count)
                .find(|&index| m.i32(self.rider(index) + kr::VEHICLE_INDEX as u32) == vehicle_index)
                .unwrap_or(rider_count);
            // (a vehicle gone, or destroyed, takes its kept riders with it;
            // one with none kept is done with)
            if self.unit(vehicle_index).is_none_or(|object| self.dead(object)) || index >= rider_count {
                self.forget_vehicle(number);
                continue;
            }
            if m.r8(vehicle + rv::RELEASING as u32) == 0 {
                if self.riders_out(vehicle).is_empty() {
                    continue;
                }
                m.w8(vehicle + rv::RELEASING as u32, 1);
                m.set_i16(vehicle + rv::RELEASED_COUNT as u32, 0);
                m.w32(vehicle + rv::NEXT_RELEASE_TIME as u32, now as u32);
            }
            if now < m.i32(vehicle + rv::NEXT_RELEASE_TIME as u32) {
                continue;
            }
            let rider = self.rider(index);
            let (variant, encounter, squad) =
                (m.i32(rider + kr::VARIANT_DEFINITION_INDEX as u32), m.i32(rider + kr::ENCOUNTER_INDEX as u32), m.i16(rider + kr::SQUAD_INDEX as u32));
            let last = rider_count - 1;
            m.set_i16(self.state + es::RIDER_COUNT as u32, last);
            if last != index {
                self.copy(rider, self.rider(last), kr::SIZEOF);
            }
            let released = m.i16(vehicle + rv::RELEASED_COUNT as u32);
            m.set_i16(vehicle + rv::RELEASED_COUNT as u32, released + 1);
            let (position, facing) = self.rider_place(vehicle, released);
            let mut call = AiCall { kind: call::PLACE_RIDER, actor: squad as i32, a: variant, b: encounter, ..AiCall::default() };
            call.input[..3].copy_from_slice(&position);
            call.input[3] = facing;
            self.host.call(&mut call);
            m.w32(vehicle + rv::NEXT_RELEASE_TIME as u32, (now + RIDER_RELEASE_TICKS) as u32);
        }
    }
}

/// The world step of co-op's host placing the riders kept for want of a
/// dropship's seat, each tick.
pub fn update(context: &WorldContext, host: &impl AiHost) {
    let m = host.memory();
    let state = m.r32(context.arguments);
    Update { context, host, m, state }.run();
}
