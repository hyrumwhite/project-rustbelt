//! A client applying the host's state to its copies (OpenCE's
//! port/linux/game/network_distributed.c's `distributed_apply_state`,
//! network_objects.c's `network_objects_correct`, `network_objects_reconcile`
//! and `distributed_object_move`): a unit moved toward where the host has
//! it (its velocity held to how fast it may go), and an object put where
//! the host has it, or halfway there when near, its drawing smoothed over
//! the move.
//!
//! The objects are the game's memory ([`WorldContext`]), and each step's
//! arguments and result a struct the game passes ([`Arguments`]). Setting
//! an object's position (`object_set_position`, which moves it among the
//! clusters), smoothing its drawing (`render_interpolation_correct_object`)
//! and whether co-op and a cutscene are on are the game's, through
//! [`AiHost::call`] (network_objects.c's `network_apply_call`).

use crate::ai_actor::{AiCall, AiHost, AiMemory, AiMemoryExt, NONE, WorldContext, datum_get};
use crate::distributed_wire::{ObjectState, UNIT_SCALE, UnitState, VELOCITY_SCALE, axes_make_valid, transform_valid, vector_unpack};
use crate::layout::{net_values as nv, player_datum as pd, unit_datum as ud, weapon_datum as wd, weapon_magazine as wm};

/// network_objects.c's calls (halo_sim.h's `_network_apply_call_*`).
pub mod call {
    /// `object_set_position(a, input[0..3], input[3..6], input[6..9])`.
    pub const SET_POSITION: i32 = 1;
    /// `render_interpolation_correct_object(a, input[0..3])`.
    pub const INTERPOLATION: i32 = 2;
    /// Whether co-op is on (bit 0) and, if so, a cutscene (bit 1), the
    /// result.
    pub const COOP_STATE: i32 = 3;
    /// `scenario_leaf_index_from_point(input[0..3])`, the result.
    pub const LEAF: i32 = 4;
    /// `network_objects_client_has(a)`, the result.
    pub const CLIENT_HAS: i32 = 5;
    /// `network_player_detach_unit(a)`.
    pub const DETACH_UNIT: i32 = 6;
    /// `network_player_attach_unit(a, b)`.
    pub const ATTACH_UNIT: i32 = 7;
    /// `unit_kill(a)`.
    pub const UNIT_KILL: i32 = 8;
    /// `unit_kill_no_statistics(a)`.
    pub const UNIT_KILL_NO_STATISTICS: i32 = 9;
    /// `network_objects_set_seat(a, b, input[0])`.
    pub const SET_SEAT: i32 = 10;
    /// `damage_set_network_state(a, ...)`: the shields depleted, charging
    /// and over charging (`b`'s bits 0 to 2), the body and shield
    /// vitalities and the damage they show (`input[0..6]`).
    pub const SET_DAMAGE: i32 = 11;
    /// `find_netgame_flag(input[0..3], 1, 0, teleporter source, NONE)`, the
    /// result.
    pub const TELEPORTER: i32 = 12;
    /// `unit_kill_silent(a)`.
    pub const UNIT_KILL_SILENT: i32 = 13;
    /// `object_damage_update(a)`.
    pub const DAMAGE_UPDATE: i32 = 14;
    /// `object_reconnect_to_map(a, NULL)`.
    pub const RECONNECT: i32 = 15;
    /// `object_set_visibility(a, b)`.
    pub const SET_VISIBILITY: i32 = 16;
    /// `unit_network_drop_weapon(a, b)`.
    pub const DROP_WEAPON: i32 = 17;
    /// network_objects.c's `distributed_client_release_item(a, FALSE)`
    /// (any unit carrying the item drops it).
    pub const RELEASE_ITEM: i32 = 18;
    /// `unit_network_add_weapon(a, b, input[0])`.
    pub const ADD_WEAPON: i32 = 19;
}

/// halo_sim.h's `struct halo_sim_network_apply`: a step's arguments (taken
/// by value) and its result.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct Arguments {
    pub object_index: i32,
    pub position: [f32; 3],
    pub forward: [f32; 3],
    pub up: [f32; 3],
    pub velocity: [f32; 3],
    pub angular_velocity: [f32; 3],
    pub has_velocity: i32,
    pub has_angular_velocity: i32,
    pub tolerance: f32,
    pub blend_distance: f32,
    pub maximum_speed: f32,
    pub maximum_fall_speed: f32,
    pub state: UnitState,
    /// network_distributed.c's corrections count (0 for none).
    pub corrections: u32,
    /// The local players' noted positions (`distributed_own_positions`, 0
    /// for none), the smoothed round trip (`distributed_own_round_trip`),
    /// and (`distributed_correct_own_unit`'s) the local player's index.
    pub own_positions: u32,
    pub round_trip: u32,
    pub local_player_index: i32,
    /// `distributed_handle_unit_state`'s: the player's death
    /// (`distributed_deaths`) and seat disagreement
    /// (`distributed_seat_disagreements`), 0 for one not tracked, and its
    /// datum (the game's region alone).
    pub death: u32,
    pub seat_disagreement: u32,
    pub player: u32,
    /// The local players' noted vehicles (network_objects.c's
    /// `objects_client_own_vehicles`, 0 for none).
    pub own_vehicles: u32,
    /// The object's entry of network_objects.c's `objects_client_dead` (0
    /// for none).
    pub dead: u32,
    /// The local players' own inventories (network_objects.c's
    /// `objects_client_own_inventories`, 0 for none), and the loaded tags'
    /// table and count (`cache_files_tag_instances`).
    pub own_inventories: u32,
    pub tag_instances: u32,
    pub tag_count: i32,
    pub result: i32,
}

const _: () = assert!(std::mem::size_of::<Arguments>() == 4 + 15 * 4 + 8 + 16 + 64 + 8 + 44);

/// The arguments' result's offset.
const RESULT: u32 = (std::mem::size_of::<Arguments>() - 4) as u32;

impl Arguments {
    /// The arguments where the game holds them.
    pub fn read(m: &dyn AiMemory, address: u32) -> Self {
        let mut bytes = [0u8; std::mem::size_of::<Arguments>()];
        for (index, byte) in bytes.iter_mut().enumerate() {
            *byte = m.r8(address + index as u32);
        }
        // SAFETY: plain data of its size
        unsafe { std::mem::transmute(bytes) }
    }
}

/// network_distributed.h's `UNIT_WORLD_BOUND`.
const UNIT_WORLD_BOUND: f32 = 32768.0;

struct Apply<'a, H: AiHost> {
    context: &'a WorldContext,
    host: &'a H,
    m: &'a dyn AiMemory,
}

impl<H: AiHost> Apply<'_, H> {
    fn call(&self, kind: i32, a: i32, input: &[f32]) -> AiCall {
        let mut call = AiCall { kind, actor: NONE, a, ..AiCall::default() };
        call.input[..input.len()].copy_from_slice(input);
        self.host.call(&mut call);
        call
    }

    /// `object_get`.
    fn object(&self, object_index: i32) -> u32 {
        datum_get(self.m, self.context.object_header_data, object_index)
            .map_or(0, |header| self.m.r32(header + crate::layout::object_header_datum::DATUM as u32))
    }

    fn coop_state(&self) -> i32 {
        self.call(call::COOP_STATE, 0, &[]).result
    }

    /// network_objects.c's `distributed_vehicle_unsteered` (co-op's state
    /// given): a vehicle no player drives in co-op.
    fn vehicle_unsteered_with(&self, object_index: i32, coop_state: i32) -> bool {
        let m = self.m;
        let object = self.object(object_index);
        if coop_state & 1 == 0 || m.i16(object + ud::OBJECT_TYPE as u32) != crate::layout::ai_values::_OBJECT_TYPE_VEHICLE as i16 {
            return false;
        }
        let driver = m.i32(object + ud::UNIT_DRIVER_OBJECT_INDEX as u32);
        driver == NONE || m.i32(self.object(driver) + ud::UNIT_PLAYER_INDEX as u32) == NONE
    }

    fn vehicle_unsteered(&self, object_index: i32) -> bool {
        let coop_state = self.coop_state();
        self.vehicle_unsteered_with(object_index, coop_state)
    }

    /// network_objects.c's `distributed_object_follows_host`: an unsteered
    /// vehicle, and anything in a co-op cutscene.
    fn follows_host(&self, object_index: i32) -> bool {
        let coop_state = self.coop_state();
        (coop_state & 1 != 0 && coop_state & 2 != 0) || self.vehicle_unsteered_with(object_index, coop_state)
    }

    fn set_position(&self, object_index: i32, position: [f32; 3], forward: [f32; 3], up: [f32; 3]) {
        let mut input = [0.0; 9];
        input[0..3].copy_from_slice(&position);
        input[3..6].copy_from_slice(&forward);
        input[6..9].copy_from_slice(&up);
        self.call(call::SET_POSITION, object_index, &input);
    }

    fn velocities(&self, object: u32, velocity: Option<[f32; 3]>, angular_velocity: Option<[f32; 3]>) {
        let write = |offset: usize, v: [f32; 3]| {
            for (index, part) in v.into_iter().enumerate() {
                self.m.set_f32(object + offset as u32 + 4 * index as u32, part);
            }
        };
        if let Some(v) = velocity {
            write(ud::OBJECT_TRANSLATIONAL_VELOCITY, v);
        }
        if let Some(v) = angular_velocity {
            write(ud::OBJECT_ANGULAR_VELOCITY, v);
        }
    }

    fn rest_if_unsteered(&self, object_index: i32, object: u32) {
        if self.vehicle_unsteered(object_index) {
            let flags = object + ud::OBJECT_FLAGS as u32;
            self.m.w32(flags, self.m.r32(flags) | 1 << nv::_OBJECT_AT_REST_BIT);
        }
    }

    /// network_objects.c's `distributed_object_move` (the transform
    /// checked): put where the host has it, drawn from where it was.
    fn object_move(
        &self,
        object_index: i32,
        position: [f32; 3],
        forward: [f32; 3],
        up: [f32; 3],
        velocity: Option<[f32; 3]>,
        angular_velocity: Option<[f32; 3]>,
    ) {
        let object = self.object(object_index);
        let was = self.m.vector(object + ud::OBJECT_POSITION as u32);
        let offset = [was[0] - position[0], was[1] - position[1], was[2] - position[2]];
        self.set_position(object_index, position, forward, up);
        self.velocities(object, velocity, angular_velocity);
        if !self.follows_host(object_index) {
            self.call(call::INTERPOLATION, object_index, &offset);
        }
        self.rest_if_unsteered(object_index, object);
    }

    /// `network_objects_correct`: put where the host has it, if that can be.
    fn correct(
        &self,
        object_index: i32,
        position: [f32; 3],
        forward: [f32; 3],
        up: [f32; 3],
        velocity: Option<[f32; 3]>,
        angular_velocity: Option<[f32; 3]>,
    ) {
        let mut valid = [0.0; 6];
        if transform_valid(position, forward, up, velocity, angular_velocity, &mut valid) {
            let (forward, up) = split(valid);
            self.object_move(object_index, position, forward, up, velocity, angular_velocity);
        }
    }

    /// `network_objects_reconcile`: put where the host has it when further
    /// than the blend distance (TRUE), else moved halfway there (or all of
    /// the way, following the host) and drawn as a glide.
    #[allow(clippy::too_many_arguments)]
    fn reconcile(
        &self,
        object_index: i32,
        position: [f32; 3],
        forward: [f32; 3],
        up: [f32; 3],
        velocity: Option<[f32; 3]>,
        angular_velocity: Option<[f32; 3]>,
        blend_distance: f32,
    ) -> bool {
        let m = self.m;
        let object = self.object(object_index);
        let mut valid = [0.0; 6];
        if !transform_valid(position, forward, up, velocity, angular_velocity, &mut valid) {
            return false;
        }
        let (valid_forward, valid_up) = split(valid);
        let here = m.vector(object + ud::OBJECT_POSITION as u32);
        let d = [position[0] - here[0], position[1] - here[1], position[2] - here[2]];
        // (so written that its own position gone wrong, not a number, is put
        // right)
        if !(d[0] * d[0] + d[1] * d[1] + d[2] * d[2] <= blend_distance * blend_distance) {
            self.object_move(object_index, position, valid_forward, valid_up, velocity, angular_velocity);
            return true;
        }
        let blended = if self.follows_host(object_index) {
            position
        } else {
            let here = m.vector(object + ud::OBJECT_POSITION as u32);
            [here[0] + d[0] * 0.5, here[1] + d[1] * 0.5, here[2] + d[2] * 0.5]
        };
        let here = m.vector(object + ud::OBJECT_POSITION as u32);
        let offset = [here[0] - blended[0], here[1] - blended[1], here[2] - blended[2]];
        self.set_position(object_index, blended, valid_forward, valid_up);
        if !self.follows_host(object_index) {
            self.call(call::INTERPOLATION, object_index, &offset);
        }
        self.velocities(object, velocity, angular_velocity);
        self.rest_if_unsteered(object_index, object);
        false
    }

    /// network_distributed.c's `distributed_apply_state`: the unit moved
    /// toward the state (at the position) when further than the tolerance
    /// from it, its velocity held to how fast it may go across and up and
    /// fall; FALSE for a state that cannot be.
    fn apply_state(&self, arguments: &Arguments) -> bool {
        let m = self.m;
        let state = &arguments.state;
        let mut velocity = vector_unpack(state.velocity, VELOCITY_SCALE);
        let forward = vector_unpack(state.forward, UNIT_SCALE);
        let up = vector_unpack(state.up, UNIT_SCALE);
        let position = arguments.position;
        let mut axes = [forward[0], forward[1], forward[2], up[0], up[1], up[2]];
        if !position.iter().all(|part| part.abs() <= UNIT_WORLD_BOUND) || !axes_make_valid(&mut axes) {
            return false;
        }
        let (forward, up) = split(axes);
        let maximum_speed = arguments.maximum_speed;
        if maximum_speed > 0.0 {
            let rise = if velocity[2] > 0.0 { velocity[2] } else { 0.0 };
            let speed = ((velocity[0] * velocity[0] + velocity[1] * velocity[1] + rise * rise) as f64).sqrt() as f32;
            if speed > maximum_speed {
                velocity[0] *= maximum_speed / speed;
                velocity[1] *= maximum_speed / speed;
                if velocity[2] > 0.0 {
                    velocity[2] *= maximum_speed / speed;
                }
            }
            if arguments.maximum_fall_speed > 0.0 && velocity[2] < -arguments.maximum_fall_speed {
                velocity[2] = -arguments.maximum_fall_speed;
            }
        }
        let here = m.vector(self.object(arguments.object_index) + ud::OBJECT_POSITION as u32);
        let error = [position[0] - here[0], position[1] - here[1], position[2] - here[2]];
        let tolerance = arguments.tolerance;
        if error[0] * error[0] + error[1] * error[1] + error[2] * error[2] <= tolerance * tolerance {
            return true;
        }
        if self.reconcile(arguments.object_index, position, forward, up, Some(velocity), None, arguments.blend_distance)
            && arguments.corrections != 0
        {
            m.w32(arguments.corrections, m.r32(arguments.corrections).wrapping_add(1));
        }
        true
    }
}

/// network_distributed.c's `OWN_POSITION_TICKS`, its noted positions'
/// size, and `LOCAL_CORRECTION_TOLERANCE`.
const OWN_POSITION_TICKS: i32 = 64;
const OWN_POSITION_SIZE: u32 = 20;
const LOCAL_CORRECTION_TOLERANCE: f32 = 3.0;
/// players.h's `MAXIMUM_LOCAL_PLAYERS`.
const MAXIMUM_LOCAL_PLAYERS: i32 = 4;
/// network_distributed.h's remote players' tolerance and blend.
const REMOTE_CORRECTION_TOLERANCE: f32 = 0.05;
const REMOTE_BLEND_DISTANCE: f32 = 1.0;
/// network_distributed.c's waits.
const DEATH_BLOW_WAIT_TICKS: i32 = 15;
const SEAT_DISAGREEMENT_TICKS: i16 = 15;
const SEAT_DISAGREEMENT_SLACK_TICKS: i16 = 6;

impl<H: AiHost> Apply<'_, H> {
    fn leaf(&self, point: [f32; 3]) -> i32 {
        self.call(call::LEAF, 0, &point).result
    }

    /// network_distributed.c's `distributed_correct_own_unit`: the client's
    /// own player's unit where the host has it, when further than the
    /// tolerance from where this machine had it at the tick the host has it
    /// at (its prediction come back): moved by the difference, keeping what
    /// it has done since (the ticks noted since moved with it); without that
    /// tick, from where it is. In co-op, not into a BSP not yet loaded.
    fn correct_own_unit(&self, arguments: &Arguments) {
        let m = self.m;
        let state = &arguments.state;
        let unit_index = arguments.object_index;
        let host_position = state.position.map(f32::from_bits);
        let object = self.object(unit_index);
        let apply = |position: [f32; 3]| {
            let applied = Arguments {
                object_index: unit_index,
                position,
                state: *state,
                tolerance: LOCAL_CORRECTION_TOLERANCE,
                corrections: arguments.corrections,
                ..Arguments::default()
            };
            self.apply_state(&applied);
        };
        if self.coop_state() & 1 != 0
            && self.leaf(host_position) == NONE
            && self.leaf(m.vector(object + ud::OBJECT_POSITION as u32)) != NONE
        {
            return;
        }
        let local = arguments.local_player_index;
        if state.flags & crate::distributed_wire::unit_flag::PREDICTED != 0
            && arguments.own_positions != 0
            && (0..MAXIMUM_LOCAL_PLAYERS).contains(&local)
        {
            let table = arguments.own_positions + local as u32 * OWN_POSITION_TICKS as u32 * OWN_POSITION_SIZE;
            let now = self.context.game_time;
            let time = now.wrapping_sub((now as u16).wrapping_sub(state.predicted_time as u16) as i32);
            let own = table + (time & (OWN_POSITION_TICKS - 1)) as u32 * OWN_POSITION_SIZE;
            if m.i32(own) == time && m.i32(own + 4) == unit_index {
                let elapsed = now.wrapping_sub(time) as f32;
                let round_trip = m.f32(arguments.round_trip);
                m.set_f32(
                    arguments.round_trip,
                    if round_trip <= 0.0 { elapsed } else { round_trip + (elapsed - round_trip) / 8.0 },
                );
                let noted = m.vector(own + 8);
                let mut error = [host_position[0] - noted[0], host_position[1] - noted[1], host_position[2] - noted[2]];
                if !(error[0] * error[0] + error[1] * error[1] + error[2] * error[2]
                    > LOCAL_CORRECTION_TOLERANCE * LOCAL_CORRECTION_TOLERANCE)
                {
                    return;
                }
                let before = m.vector(object + ud::OBJECT_POSITION as u32);
                apply([before[0] + error[0], before[1] + error[1], before[2] + error[2]]);
                let after = m.vector(object + ud::OBJECT_POSITION as u32);
                error = [after[0] - before[0], after[1] - before[1], after[2] - before[2]];
                for index in 0..OWN_POSITION_TICKS as u32 {
                    let entry = table + index * OWN_POSITION_SIZE;
                    if m.i32(entry + 4) == unit_index {
                        for part in 0..3 {
                            let address = entry + 8 + 4 * part;
                            m.set_f32(address, m.f32(address) + error[part as usize]);
                        }
                    }
                }
                return;
            }
        }
        apply(host_position);
    }
}

impl<H: AiHost> Apply<'_, H> {
    /// network_distributed.c's `distributed_player`.
    fn player(&self, player_index: i16) -> Option<u32> {
        use crate::data_array::{DATA_DATA, DATA_MAXIMUM_COUNT, DATA_SIZE};
        let m = self.m;
        let data = self.context.player_data;
        let maximum_count = m.r16(data + DATA_MAXIMUM_COUNT) as i16;
        if player_index < 0 || player_index >= maximum_count {
            return None;
        }
        let player = m.r32(data + DATA_DATA) + player_index as u32 * m.r16(data + DATA_SIZE) as i16 as u32;
        (m.i16(player + pd::IDENTIFIER as u32) != 0).then_some(player)
    }

    /// `object_try_and_get`.
    fn try_object(&self, object_index: i32) -> Option<u32> {
        datum_get(self.m, self.context.object_header_data, object_index)
            .map(|header| self.m.r32(header + crate::layout::object_header_datum::DATUM as u32))
    }

    fn dead(&self, object: u32) -> bool {
        self.m.r16(object + ud::OBJECT_DAMAGE_FLAGS as u32) as u32 & (1 << nv::_OBJECT_DEAD_BIT) != 0
    }

    /// network_distributed.c's `distributed_living_unit`.
    fn living_unit(&self, player: u32) -> i32 {
        let unit_index = self.m.i32(player + pd::UNIT_INDEX as u32);
        match self.try_object(unit_index) {
            Some(unit) if unit_index != NONE && !self.dead(unit) => unit_index,
            _ => NONE,
        }
    }

    /// network_distributed.c's `distributed_handle_unit_state`: the host's
    /// word on a player's unit. How it died noted; dead, its copy killed
    /// once its killing blow has had a while to come; alive, its unit (once
    /// this machine has it) the player's, its seat the host's (a local
    /// player's once it has disagreed longer than a round trip), its
    /// shields, powerups and camouflage the host's, and (on foot) moved
    /// toward where the host has it, onto the teleporter it lands on.
    fn handle_unit_state(&self, arguments: &Arguments) {
        use crate::distributed_wire::{NO_PLAYER, unit_flag, vitality_unpack};
        let m = self.m;
        let state = &arguments.state;
        let Some(player) = self.player(state.player_index as i16) else { return };
        let player_index = (m.i16(player + pd::IDENTIFIER as u32) as i32) << 16 | state.player_index as i32;
        let local = m.i16(player + pd::LOCAL_PLAYER_INDEX as u32) != NONE as i16;
        let alive = state.flags & unit_flag::ALIVE != 0;
        let death = arguments.death;
        if death != 0 {
            m.w8(death, !alive as u8);
            m.set_i16(
                death + 2,
                if state.killing_player_index != NO_PLAYER { state.killing_player_index as i16 } else { NONE as i16 },
            );
            m.w8(death + 4, (state.flags & (1 << 2) != 0) as u8);
            m.w8(death + 5, (state.flags & (1 << 3) != 0) as u8);
        }
        let mut unit_index = self.living_unit(player);
        if !alive {
            if unit_index != NONE && death != 0 {
                let dead_since = m.i32(death + 8);
                let now = self.context.game_time;
                if dead_since == NONE {
                    m.w32(death + 8, now as u32);
                } else if now.wrapping_sub(dead_since) >= DEATH_BLOW_WAIT_TICKS {
                    self.call(call::UNIT_KILL_NO_STATISTICS, unit_index, &[]);
                }
            }
            return;
        }
        if death != 0 {
            m.w32(death + 8, NONE as u32);
        }
        let state_unit = state.unit_index;
        if state_unit == NONE || self.call(call::CLIENT_HAS, state_unit, &[]).result == 0 {
            return;
        }
        match self.try_object(state_unit) {
            Some(unit) if (nv::_OBJECT_MASK_UNIT as u32) & (1 << m.r8(unit + ud::OBJECT_TYPE as u32)) != 0 && !self.dead(unit) => {}
            _ => return,
        }
        let player_unit = m.i32(player + pd::UNIT_INDEX as u32);
        if player_unit != state_unit {
            if player_unit != NONE {
                self.call(call::DETACH_UNIT, player_index, &[]);
            }
            let mut attach = AiCall { kind: call::ATTACH_UNIT, actor: NONE, a: player_index, b: state_unit, ..AiCall::default() };
            self.host.call(&mut attach);
            if unit_index != NONE && unit_index != state_unit {
                self.call(call::UNIT_KILL, unit_index, &[]);
            }
        }
        unit_index = state_unit;
        let unit = self.object(unit_index);
        let at = |offset: usize| unit + offset as u32;
        if arguments.seat_disagreement != 0 {
            let parent = m.i32(at(ud::OBJECT_PARENT_OBJECT_INDEX));
            let seat = m.i16(at(ud::UNIT_PARENT_SEAT_INDEX));
            let vehicle_index = if parent != NONE && seat != NONE as i16 { parent } else { NONE };
            let same = vehicle_index == state.vehicle_index && (vehicle_index == NONE || seat == state.seat_index);
            let disagreement = arguments.seat_disagreement;
            if same {
                m.set_i16(disagreement, 0);
            } else {
                let mut take = !local;
                if !take {
                    let count = m.i16(disagreement).wrapping_add(1);
                    m.set_i16(disagreement, count);
                    let round_trip = (m.f32(arguments.round_trip) as f64).ceil() as i16;
                    let limit = round_trip.wrapping_add(SEAT_DISAGREEMENT_SLACK_TICKS);
                    take = count > if SEAT_DISAGREEMENT_TICKS > limit { SEAT_DISAGREEMENT_TICKS } else { limit };
                }
                if take {
                    let mut seat_call =
                        AiCall { kind: call::SET_SEAT, actor: NONE, a: unit_index, b: state.vehicle_index, ..AiCall::default() };
                    seat_call.input[0] = state.seat_index as f32;
                    self.host.call(&mut seat_call);
                    m.set_i16(disagreement, 0);
                }
            }
        }
        {
            let bits = (state.flags >> 4) as i32 & 7;
            let damage = [
                vitality_unpack(state.body_vitality),
                vitality_unpack(state.shield_vitality),
                vitality_unpack(state.current_body_damage),
                vitality_unpack(state.recent_body_damage),
                vitality_unpack(state.current_shield_damage),
                vitality_unpack(state.recent_shield_damage),
            ];
            let mut damage_call = AiCall { kind: call::SET_DAMAGE, actor: NONE, a: unit_index, b: bits, ..AiCall::default() };
            damage_call.input[..6].copy_from_slice(&damage);
            self.host.call(&mut damage_call);
        }
        for index in 0..crate::distributed_wire::PLAYER_POWERUPS {
            m.set_i16(player + pd::POWERUP_DURATIONS as u32 + 2 * index as u32, state.powerup_durations[index]);
        }
        let flags = m.r32(at(ud::UNIT_FLAGS));
        let set = |flags: u32, bit: usize, on: bool| if on { flags | 1 << bit } else { flags & !(1 << bit) };
        let flags = set(flags, nv::_UNIT_ACTIVE_CAMOUFLAGED_BIT, state.unit_flags & 1 != 0);
        let flags = set(flags, nv::_UNIT_SUPER_CAMOUFLAGED_BIT, state.unit_flags & 2 != 0);
        m.w32(at(ud::UNIT_FLAGS), flags);
        m.set_f32(at(ud::UNIT_ACTIVE_CAMOUFLAGE), state.active_camouflage as f32 / 255.0);
        if state.flags & unit_flag::PLACED != 0 && m.i32(at(ud::OBJECT_PARENT_OBJECT_INDEX)) == NONE {
            let before = m.vector(at(ud::OBJECT_POSITION));
            if local {
                self.correct_own_unit(&Arguments {
                    object_index: unit_index,
                    state: *state,
                    corrections: arguments.corrections,
                    own_positions: arguments.own_positions,
                    round_trip: arguments.round_trip,
                    local_player_index: m.i16(player + pd::LOCAL_PLAYER_INDEX as u32) as i32,
                    ..Arguments::default()
                });
            } else {
                self.apply_state(&Arguments {
                    object_index: unit_index,
                    position: state.position.map(f32::from_bits),
                    state: *state,
                    tolerance: REMOTE_CORRECTION_TOLERANCE,
                    blend_distance: REMOTE_BLEND_DISTANCE,
                    corrections: arguments.corrections,
                    ..Arguments::default()
                });
            }
            let after = m.vector(at(ud::OBJECT_POSITION));
            let d = [after[0] - before[0], after[1] - before[1], after[2] - before[2]];
            if d[0] * d[0] + d[1] * d[1] + d[2] * d[2] > 1.0 {
                let teleporter = self.call(call::TELEPORTER, 0, &after).result;
                m.w32(player + pd::TELEPORTER_INDEX as u32, teleporter as u32);
            }
        }
    }
}

/// network_objects.c's tolerances, its noted vehicles' size, and the wait
/// before a biped the host says is dead dies here without its blow.
const REMOTE_OBJECT_TOLERANCE: f32 = 0.05;
const REMOTE_OBJECT_ANGLE_TOLERANCE: f32 = 0.98;
const REMOTE_VEHICLE_BLEND_DISTANCE: f32 = 2.0;
const LOCAL_VEHICLE_TOLERANCE: f32 = 4.0;
const OWN_VEHICLE_POSITION_TICKS: i32 = 64;
const DEAD_BIPED_FALLBACK_TICKS: i32 = 15;

impl<H: AiHost> Apply<'_, H> {
    fn of_types(&self, object_index: i32, mask: u32) -> Option<u32> {
        self.try_object(object_index).filter(|&object| mask & (1 << self.m.r8(object + ud::OBJECT_TYPE as u32)) != 0)
    }

    /// network_distributed.c's `distributed_player_is_local` (the player's
    /// datum index).
    fn player_is_local(&self, player_index: i32) -> bool {
        player_index != NONE
            && datum_get(self.m, self.context.player_data, player_index)
                .is_some_and(|player| self.m.i16(player + pd::LOCAL_PLAYER_INDEX as u32) != NONE as i16)
    }

    /// network_objects.c's `distributed_client_dead_biped`: a biped no
    /// player's, the host says is dead, killed with nothing to show once
    /// the host has said so for a while (its entry in `dead`).
    fn dead_biped(&self, object_index: i32, dead: u32) {
        let m = self.m;
        let Some(unit) = self.of_types(object_index, nv::_OBJECT_MASK_BIPED as u32) else { return };
        if self.dead(unit) || m.i32(unit + ud::UNIT_PLAYER_INDEX as u32) != NONE || dead == 0 {
            return;
        }
        let now = self.context.game_time;
        if m.i32(dead) != object_index {
            m.w32(dead, object_index as u32);
            m.w32(dead + 4, now as u32);
        } else if now.wrapping_sub(m.i32(dead + 4)) >= DEAD_BIPED_FALLBACK_TICKS {
            self.call(call::UNIT_KILL_SILENT, object_index, &[]);
            self.call(call::UNIT_KILL_NO_STATISTICS, object_index, &[]);
            self.call(call::DAMAGE_UPDATE, object_index, &[]);
        }
    }

    /// network_objects.c's `distributed_client_dead_vehicle`.
    fn dead_vehicle(&self, object_index: i32) {
        let Some(vehicle) = self.of_types(object_index, nv::_OBJECT_MASK_VEHICLE as u32) else { return };
        if self.dead(vehicle) {
            return;
        }
        self.call(call::UNIT_KILL_NO_STATISTICS, object_index, &[]);
        self.call(call::DAMAGE_UPDATE, object_index, &[]);
    }

    /// network_objects.c's `distributed_client_correct_own_vehicle`: the
    /// vehicle this machine's player drives, where the host has it, if
    /// further than a tolerance from where this machine had it at the
    /// tick the host has it at: moved by the difference (turned and going
    /// as it drives it), the ticks noted since moved with it. FALSE
    /// without that tick.
    fn correct_own_vehicle(&self, vehicle_index: i32, player_index: i32, state: &ObjectState, arguments: &Arguments) -> bool {
        let m = self.m;
        let local = match datum_get(m, self.context.player_data, player_index) {
            Some(player) => m.i16(player + pd::LOCAL_PLAYER_INDEX as u32) as i32,
            None => NONE,
        };
        let now = self.context.game_time;
        let time = now.wrapping_sub((now as u16).wrapping_sub(state.time as u16) as i32);
        if state.flags & OBJECT_PREDICTED == 0 || !(0..MAXIMUM_LOCAL_PLAYERS).contains(&local) || arguments.own_vehicles == 0 {
            return false;
        }
        let table = arguments.own_vehicles + local as u32 * OWN_VEHICLE_POSITION_TICKS as u32 * OWN_POSITION_SIZE;
        let own = table + (time & (OWN_VEHICLE_POSITION_TICKS - 1)) as u32 * OWN_POSITION_SIZE;
        if m.i32(own) != time || m.i32(own + 4) != vehicle_index {
            return false;
        }
        let position = state.position.map(f32::from_bits);
        let noted = m.vector(own + 8);
        let mut error = [position[0] - noted[0], position[1] - noted[1], position[2] - noted[2]];
        if error[0] * error[0] + error[1] * error[1] + error[2] * error[2] <= LOCAL_VEHICLE_TOLERANCE * LOCAL_VEHICLE_TOLERANCE {
            return true;
        }
        let object = self.object(vehicle_index);
        let before = m.vector(object + ud::OBJECT_POSITION as u32);
        let forward = m.vector(object + ud::OBJECT_FORWARD as u32);
        let up = m.vector(object + ud::OBJECT_UP as u32);
        self.correct(vehicle_index, [before[0] + error[0], before[1] + error[1], before[2] + error[2]], forward, up, None, None);
        if arguments.corrections != 0 {
            m.w32(arguments.corrections, m.r32(arguments.corrections).wrapping_add(1));
        }
        let after = m.vector(object + ud::OBJECT_POSITION as u32);
        error = [after[0] - before[0], after[1] - before[1], after[2] - before[2]];
        for index in 0..OWN_VEHICLE_POSITION_TICKS as u32 {
            let entry = table + index * OWN_POSITION_SIZE;
            if m.i32(entry + 4) == vehicle_index {
                for part in 0..3 {
                    let address = entry + 8 + 4 * part;
                    m.set_f32(address, m.f32(address) + error[part as usize]);
                }
            }
        }
        true
    }

    fn set_at_rest(&self, object: u32, at_rest: bool) {
        let flags = object + ud::OBJECT_FLAGS as u32;
        let bit = 1 << nv::_OBJECT_AT_REST_BIT;
        self.m.w32(flags, if at_rest { self.m.r32(flags) | bit } else { self.m.r32(flags) & !bit });
    }

    /// network_objects.c's `network_objects_handle_states`, of one state: a
    /// copy the host says is dead killed (a biped's, a vehicle's); an item
    /// made carried that never was here put in the world; one in the world
    /// moved toward where the host has it (a vehicle this machine drives
    /// by its own prediction come back), at rest as the host has it.
    fn handle_object_state(&self, arguments: &Arguments) {
        use crate::distributed_wire::{object_flag, object_state_unpack};
        let m = self.m;
        let state = object_state(arguments);
        let object_index = state.object_index;
        if object_index == NONE || (object_index >> 16) as i16 == 0 || self.call(call::CLIENT_HAS, object_index, &[]).result == 0 {
            return;
        }
        let object = self.object(object_index);
        let at = |offset: usize| object + offset as u32;
        if state.flags & object_flag::DEAD != 0 {
            self.dead_biped(object_index, arguments.dead);
            self.dead_vehicle(object_index);
            if self.try_object(object_index).is_none() {
                return;
            }
        }
        let connected = |m: &dyn AiMemory| m.r32(at(ud::OBJECT_FLAGS)) & (1 << nv::_OBJECT_CONNECTED_TO_MAP_BIT) != 0;
        if (nv::_OBJECT_MASK_ITEM as u32) & (1 << m.r8(at(ud::OBJECT_TYPE))) != 0
            && m.i32(at(ud::OBJECT_PARENT_OBJECT_INDEX)) == NONE
            && !connected(m)
            && m.r32(object + crate::layout::item_datum::ITEM_FLAGS as u32) & (1 << nv::_ITEM_ATTACHED_TO_UNIT_BIT) == 0
        {
            self.call(call::RECONNECT, object_index, &[]);
            let mut visible = AiCall { kind: call::SET_VISIBILITY, actor: NONE, a: object_index, b: 1, ..AiCall::default() };
            self.host.call(&mut visible);
        }
        if m.i32(at(ud::OBJECT_PARENT_OBJECT_INDEX)) != NONE || !connected(m) {
            return;
        }
        let [forward, up, velocity, angular_velocity] = object_state_unpack(&state);
        let position = state.position.map(f32::from_bits);
        let mut valid = [0.0; 6];
        if !transform_valid(position, forward, up, None, None, &mut valid) {
            return;
        }
        let (forward, up) = split(valid);
        let mut tolerance = REMOTE_OBJECT_TOLERANCE;
        let mut angle_tolerance = REMOTE_OBJECT_ANGLE_TOLERANCE;
        let mut blend_distance = REMOTE_BLEND_DISTANCE;
        let mut own_vehicle = false;
        if state.flags & object_flag::AT_REST != 0 {
            blend_distance = 0.0;
        }
        if m.i16(at(ud::OBJECT_TYPE)) == crate::layout::ai_values::_OBJECT_TYPE_VEHICLE as i16 {
            if blend_distance > 0.0 {
                blend_distance = REMOTE_VEHICLE_BLEND_DISTANCE;
            }
            if self.vehicle_unsteered(object_index) {
                tolerance = 0.0;
                angle_tolerance = 2.0;
            }
            let driver = m.i32(at(ud::UNIT_DRIVER_OBJECT_INDEX));
            if driver != NONE {
                let player_index = m.i32(self.object(driver) + ud::UNIT_PLAYER_INDEX as u32);
                if self.player_is_local(player_index) {
                    if self.correct_own_vehicle(object_index, player_index, &state, arguments) {
                        return;
                    }
                    tolerance = LOCAL_VEHICLE_TOLERANCE;
                    angle_tolerance = -1.0;
                    blend_distance = 0.0;
                    own_vehicle = true;
                }
            }
        }
        let at_rest = state.flags & object_flag::AT_REST != 0 || self.vehicle_unsteered(object_index);
        if !own_vehicle {
            self.set_at_rest(object, at_rest);
        }
        let here = m.vector(at(ud::OBJECT_POSITION));
        let d = [position[0] - here[0], position[1] - here[1], position[2] - here[2]];
        let object_forward = m.vector(at(ud::OBJECT_FORWARD));
        let object_up = m.vector(at(ud::OBJECT_UP));
        if d[0] * d[0] + d[1] * d[1] + d[2] * d[2] <= tolerance * tolerance
            && forward[0] * object_forward[0] + forward[1] * object_forward[1] + forward[2] * object_forward[2] >= angle_tolerance
            && up[0] * object_up[0] + up[1] * object_up[1] + up[2] * object_up[2] >= angle_tolerance
        {
            return;
        }
        if self.reconcile(object_index, position, forward, up, Some(velocity), Some(angular_velocity), blend_distance)
            && arguments.corrections != 0
        {
            m.w32(arguments.corrections, m.r32(arguments.corrections).wrapping_add(1));
        }
        if own_vehicle {
            self.set_at_rest(object, at_rest);
        }
    }
}

/// network_objects.c's `struct distributed_own_inventory`: its unit, its
/// grenades and when it last threw each, its weapons, their rounds, when
/// each last fired and when this machine last did anything with it.
mod own {
    pub const SIZEOF: u32 = 96;
    pub const UNIT_INDEX: u32 = 0;
    pub const GRENADE_COUNTS: u32 = 4;
    pub const GRENADE_TIMES: u32 = 8;
    pub const WEAPON_INDICES: u32 = 16;
    pub const ROUNDS: u32 = 32;
    pub const FIRED_TIMES: u32 = 64;
    pub const WEAPON_TIMES: u32 = 80;
}

/// network_objects.c's inventory limits and waits.
const INVENTORY_INTERVAL_TICKS: i32 = 3;
const OWN_INVENTORY_MARGIN_TICKS: i32 = 2;
const DEFAULT_OWN_ROUND_TRIP_TICKS: f32 = 6.0;
const MAXIMUM_OWN_ROUND_TRIP_TICKS: f32 = 60.0;
const MAXIMUM_INVENTORY_GRENADES: i8 = 16;
const AGE_SCALE: f32 = 65535.0;
const WEAPON_TAG: u32 = crate::fourcc(b"weap");

impl<H: AiHost> Apply<'_, H> {
    /// `weapon_try_and_get`.
    fn weapon(&self, weapon_index: i32) -> Option<u32> {
        if weapon_index == NONE { None } else { self.of_types(weapon_index, nv::_OBJECT_MASK_WEAPON as u32) }
    }

    /// network_objects.c's `distributed_client_own_settled`: whether the
    /// host has had a round trip (and its inventories' interval) since this
    /// machine last did that with its own player's unit (NONE: never).
    fn own_settled(&self, arguments: &Arguments, time: i32) -> bool {
        if time == NONE {
            return true;
        }
        let mut round_trip = self.m.f32(arguments.round_trip);
        if !(round_trip > 0.0) {
            round_trip = DEFAULT_OWN_ROUND_TRIP_TICKS;
        }
        if round_trip > MAXIMUM_OWN_ROUND_TRIP_TICKS {
            round_trip = MAXIMUM_OWN_ROUND_TRIP_TICKS;
        }
        self.context.game_time.wrapping_sub(time)
            > (round_trip as f64).ceil() as i32 + INVENTORY_INTERVAL_TICKS + OWN_INVENTORY_MARGIN_TICKS
    }

    /// network_objects.c's `distributed_client_note_own_inventory`, not
    /// compared (what the host changed is not this machine's doing).
    fn note_own_inventory(&self, own_inventory: u32, unit_index: i32) {
        let m = self.m;
        let unit = self.object(unit_index);
        if m.i32(own_inventory + own::UNIT_INDEX) != unit_index {
            m.w32(own_inventory + own::UNIT_INDEX, unit_index as u32);
            for grenade_type in 0..2 {
                m.w32(own_inventory + own::GRENADE_TIMES + 4 * grenade_type, NONE as u32);
            }
            for slot in 0..4 {
                m.w32(own_inventory + own::WEAPON_INDICES + 4 * slot, NONE as u32);
                m.w32(own_inventory + own::WEAPON_TIMES + 4 * slot, NONE as u32);
            }
        }
        for grenade_type in 0..2 {
            m.w8(own_inventory + own::GRENADE_COUNTS + grenade_type, m.r8(unit + ud::UNIT_GRENADE_COUNTS as u32 + grenade_type));
        }
        for slot in 0..4u32 {
            let weapon_index = m.i32(unit + ud::UNIT_WEAPON_OBJECT_INDICES as u32 + 4 * slot);
            let Some(weapon) = self.weapon(weapon_index) else {
                m.w32(own_inventory + own::WEAPON_INDICES + 4 * slot, NONE as u32);
                continue;
            };
            if m.i32(own_inventory + own::WEAPON_INDICES + 4 * slot) != weapon_index {
                m.w32(own_inventory + own::WEAPON_INDICES + 4 * slot, weapon_index as u32);
                m.w32(own_inventory + own::WEAPON_TIMES + 4 * slot, NONE as u32);
            }
            m.w32(own_inventory + own::FIRED_TIMES + 4 * slot, m.r32(weapon + wd::WEAPON_GAME_TIME_LAST_FIRED as u32));
            for magazine in 0..2u32 {
                let state = weapon + wd::WEAPON_MAGAZINES as u32 + magazine * wm::SIZEOF as u32;
                let rounds = own_inventory + own::ROUNDS + 8 * slot + 4 * magazine;
                m.set_i16(rounds, m.i16(state + wm::ROUNDS_TOTAL as u32));
                m.set_i16(rounds + 2, m.i16(state + wm::ROUNDS_LOADED as u32));
            }
        }
    }

    /// network_objects.c's `distributed_client_apply_inventory`: the unit
    /// carries what the host's does. Its weapons slot for slot (once this
    /// machine has them); the weapon in hand (a local player's own choice,
    /// unless gone); the rounds (no more than the magazines hold; a local
    /// player's only once the host has had a round trip since it used the
    /// weapon) and age; the grenades (a local player's when fewer, or once
    /// the host has seen the last it threw).
    fn apply_inventory(&self, arguments: &Arguments) {
        let m = self.m;
        // SAFETY: the unit state's 64 bytes hold the inventory's 64
        let inventory: crate::distributed_wire::Inventory =
            unsafe { std::ptr::read_unaligned(&arguments.state as *const UnitState as *const crate::distributed_wire::Inventory) };
        let unit_index = inventory.unit_index;
        if self.call(call::CLIENT_HAS, unit_index, &[]).result == 0
            || self.of_types(unit_index, nv::_OBJECT_MASK_UNIT as u32).is_none()
        {
            return;
        }
        let unit = self.object(unit_index);
        let at = |offset: usize| unit + offset as u32;
        if self.dead(unit) {
            return;
        }
        let player_index = m.i32(at(ud::UNIT_PLAYER_INDEX));
        let local = self.player_is_local(player_index);
        let mut own_inventory = None;
        if local && arguments.own_inventories != 0 {
            let player = datum_get(m, self.context.player_data, player_index).unwrap_or(0);
            let local_player_index = m.i16(player + pd::LOCAL_PLAYER_INDEX as u32) as i32;
            if (0..MAXIMUM_LOCAL_PLAYERS).contains(&local_player_index) {
                let entry = arguments.own_inventories + local_player_index as u32 * own::SIZEOF;
                if m.i32(entry + own::UNIT_INDEX) == unit_index {
                    own_inventory = Some(entry);
                }
            }
        }
        let weapons = |slot: u32| at(ud::UNIT_WEAPON_OBJECT_INDICES) + 4 * slot;
        for slot in 0..4u32 {
            let wanted_index = inventory.weapon_indices[slot as usize];
            let weapon_index = m.i32(weapons(slot));
            if weapon_index == wanted_index {
                continue;
            }
            if wanted_index != NONE
                && (self.call(call::CLIENT_HAS, wanted_index, &[]).result == 0
                    || self.of_types(wanted_index, nv::_OBJECT_MASK_WEAPON as u32).is_none())
            {
                continue;
            }
            if weapon_index != NONE {
                let mut drop = AiCall { kind: call::DROP_WEAPON, actor: NONE, a: unit_index, b: slot as i32, ..AiCall::default() };
                self.host.call(&mut drop);
            }
            if wanted_index != NONE {
                self.call(call::RELEASE_ITEM, wanted_index, &[]);
                let mut add = AiCall { kind: call::ADD_WEAPON, actor: NONE, a: unit_index, b: wanted_index, ..AiCall::default() };
                add.input[0] = slot as f32;
                self.host.call(&mut add);
            }
        }
        let current = inventory.current_weapon_index as i16;
        let unit_current = m.i16(at(ud::UNIT_CURRENT_WEAPON_INDEX));
        if (0..4).contains(&current)
            && unit_current != current
            && (!local || unit_current == NONE as i16 || m.i32(weapons(unit_current as u16 as u32)) == NONE)
        {
            m.set_i16(at(ud::UNIT_DESIRED_WEAPON_INDEX), current);
        }
        let tags = crate::ai_actor::AiStepContext {
            tag_instances: arguments.tag_instances,
            tag_count: arguments.tag_count,
            ..crate::ai_actor::AiStepContext::default()
        };
        for slot in 0..4u32 {
            let weapon_index = m.i32(weapons(slot));
            let Some(weapon) = self.weapon(weapon_index) else { continue };
            if weapon_index != inventory.weapon_indices[slot as usize] {
                continue;
            }
            if let Some(own_inventory) = own_inventory {
                if m.i32(own_inventory + own::WEAPON_INDICES + 4 * slot) == weapon_index
                    && !self.own_settled(arguments, m.i32(own_inventory + own::WEAPON_TIMES + 4 * slot))
                {
                    continue;
                }
            }
            let definition_index = m.i32(weapon + ud::DEFINITION_INDEX as u32);
            let magazines = crate::sensing::tag_get(m, &tags, WEAPON_TAG, definition_index)
                .map(|definition| definition + (crate::layout::weapon_definition::WEAPON + crate::layout::_weapon_definition::MAGAZINES) as u32);
            let magazine_count = magazines.map_or(0, |block| crate::ai_actor::tag_block_count(m, block));
            for magazine in 0..2i32 {
                if magazine >= magazine_count {
                    break;
                }
                use crate::layout::weapon_magazine_definition as md;
                let definition = crate::ai_actor::tag_block_element(m, magazines.unwrap_or(0), magazine, md::SIZEOF);
                let initial = m.i16(definition + md::ROUNDS_TOTAL_INITIAL as u32);
                let maximum = m.i16(definition + md::ROUNDS_TOTAL_MAXIMUM as u32);
                let total_maximum = if maximum > initial { maximum } else { initial };
                let loaded_maximum = m.i16(definition + md::ROUNDS_LOADED_MAXIMUM as u32);
                let total = inventory.rounds_total[slot as usize][magazine as usize];
                let loaded = inventory.rounds_loaded[slot as usize][magazine as usize];
                let state = weapon + wd::WEAPON_MAGAZINES as u32 + magazine as u32 * wm::SIZEOF as u32;
                m.set_i16(state + wm::ROUNDS_TOTAL as u32, if total < 0 { 0 } else if total > total_maximum { total_maximum } else { total });
                m.set_i16(
                    state + wm::ROUNDS_LOADED as u32,
                    if loaded < 0 { 0 } else if loaded > loaded_maximum { loaded_maximum } else { loaded },
                );
            }
            m.set_f32(weapon + wd::WEAPON_AGE as u32, inventory.age[slot as usize] as f32 / AGE_SCALE);
        }
        for grenade_type in 0..2u32 {
            let wanted = inventory.grenade_counts[grenade_type as usize];
            let count = if wanted < 0 {
                0
            } else if wanted > MAXIMUM_INVENTORY_GRENADES {
                MAXIMUM_INVENTORY_GRENADES
            } else {
                wanted
            };
            let address = at(ud::UNIT_GRENADE_COUNTS) + grenade_type;
            let take = match own_inventory {
                None => true,
                Some(own_inventory) => {
                    count < m.r8(address) as i8
                        || self.own_settled(arguments, m.i32(own_inventory + own::GRENADE_TIMES + 4 * grenade_type))
                }
            };
            if take {
                m.w8(address, count as u8);
            }
        }
        if let Some(own_inventory) = own_inventory {
            self.note_own_inventory(own_inventory, unit_index);
        }
    }
}

/// network_objects.c's `_distributed_object_predicted_bit`.
const OBJECT_PREDICTED: u8 = 1 << 3;

/// The object state an object's step takes (in the arguments' state's
/// bytes).
fn object_state(arguments: &Arguments) -> crate::distributed_wire::ObjectState {
    // SAFETY: the unit state's 64 bytes hold the object state's 44
    unsafe { std::ptr::read_unaligned(&arguments.state as *const UnitState as *const crate::distributed_wire::ObjectState) }
}

fn split(axes: [f32; 6]) -> ([f32; 3], [f32; 3]) {
    ([axes[0], axes[1], axes[2]], [axes[3], axes[4], axes[5]])
}

fn optional_velocities(arguments: &Arguments) -> (Option<[f32; 3]>, Option<[f32; 3]>) {
    (
        (arguments.has_velocity != 0).then_some(arguments.velocity),
        (arguments.has_angular_velocity != 0).then_some(arguments.angular_velocity),
    )
}

fn begin<'a, H: AiHost>(context: &'a WorldContext, host: &'a H) -> (Apply<'a, H>, Arguments) {
    let apply = Apply { context, host, m: host.memory() };
    let arguments = Arguments::read(apply.m, context.arguments);
    (apply, arguments)
}

/// The world step of `distributed_apply_state`.
pub fn apply_state(context: &WorldContext, host: &impl AiHost) {
    let (apply, arguments) = begin(context, host);
    let result = apply.apply_state(&arguments);
    apply.m.w32(context.arguments + RESULT, result as u32);
}

/// The world step of `distributed_handle_unit_state`.
pub fn handle_unit_state(context: &WorldContext, host: &impl AiHost) {
    let (apply, arguments) = begin(context, host);
    apply.handle_unit_state(&arguments);
}

/// The world step of `distributed_correct_own_unit`.
pub fn correct_own_unit(context: &WorldContext, host: &impl AiHost) {
    let (apply, arguments) = begin(context, host);
    apply.correct_own_unit(&arguments);
}

/// The world step of `network_objects_correct`.
pub fn object_correct(context: &WorldContext, host: &impl AiHost) {
    let (apply, arguments) = begin(context, host);
    let (velocity, angular_velocity) = optional_velocities(&arguments);
    apply.correct(arguments.object_index, arguments.position, arguments.forward, arguments.up, velocity, angular_velocity);
}

/// The world step of network_objects.c's `distributed_client_apply_inventory`.
pub fn apply_inventory(context: &WorldContext, host: &impl AiHost) {
    let (apply, arguments) = begin(context, host);
    apply.apply_inventory(&arguments);
}

/// The world step of network_objects.c's handling of an object's state.
pub fn handle_object_state(context: &WorldContext, host: &impl AiHost) {
    let (apply, arguments) = begin(context, host);
    apply.handle_object_state(&arguments);
}

/// The world step of `network_objects_reconcile`.
pub fn object_reconcile(context: &WorldContext, host: &impl AiHost) {
    let (apply, arguments) = begin(context, host);
    let (velocity, angular_velocity) = optional_velocities(&arguments);
    let result = apply.reconcile(
        arguments.object_index,
        arguments.position,
        arguments.forward,
        arguments.up,
        velocity,
        angular_velocity,
        arguments.blend_distance,
    );
    apply.m.w32(context.arguments + RESULT, result as u32);
}
