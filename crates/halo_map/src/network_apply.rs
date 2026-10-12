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
use crate::distributed_wire::{UNIT_SCALE, UnitState, VELOCITY_SCALE, axes_make_valid, transform_valid, vector_unpack};
use crate::layout::{net_values as nv, unit_datum as ud};

/// network_objects.c's calls (halo_sim.h's `_network_apply_call_*`).
pub mod call {
    /// `object_set_position(a, input[0..3], input[3..6], input[6..9])`.
    pub const SET_POSITION: i32 = 1;
    /// `render_interpolation_correct_object(a, input[0..3])`.
    pub const INTERPOLATION: i32 = 2;
    /// Whether co-op is on (bit 0) and, if so, a cutscene (bit 1), the
    /// result.
    pub const COOP_STATE: i32 = 3;
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
    pub result: i32,
}

const _: () = assert!(std::mem::size_of::<Arguments>() == 4 + 15 * 4 + 8 + 16 + 64 + 8);

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
    fn correct(&self, arguments: &Arguments) {
        let (velocity, angular_velocity) = optional_velocities(arguments);
        let mut valid = [0.0; 6];
        if transform_valid(arguments.position, arguments.forward, arguments.up, velocity, angular_velocity, &mut valid) {
            let (forward, up) = split(valid);
            self.object_move(arguments.object_index, arguments.position, forward, up, velocity, angular_velocity);
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

/// The world step of `network_objects_correct`.
pub fn object_correct(context: &WorldContext, host: &impl AiHost) {
    let (apply, arguments) = begin(context, host);
    apply.correct(&arguments);
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
