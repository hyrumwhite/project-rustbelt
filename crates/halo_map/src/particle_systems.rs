//! Particle system updates (`particle_systems.c`'s `particle_system_update`,
//! each system's every frame: following its object, its system physics, each
//! type's states and their interpolated variables, new particles from the
//! type's creation physics, and each particle's states and physics, with
//! `particle_system_new_particles`, `particle_system_update_particle_default`,
//! the creation functions, `randomize_particle_variables` and the state
//! steppers).
//!
//! A system's particles are datums of their own, linked from each type; the
//! update takes the system with all of them and leaves them, the game writing
//! them back. What reads the world or allocates goes through
//! [`ParticleSystemHost::call`] (particle_systems.c's `particle_system_call`,
//! shared with the C).

// (the C's shape: its operand order, PIN's comparisons, per-component loops)
#![allow(clippy::assign_op_pattern, clippy::manual_clamp, clippy::needless_range_loop)]

use crate::cache::CacheFile;
use crate::{Result, layout};

const NONE: i32 = -1;

/// `MAXIMUM_PARTICLE_SYSTEM_TYPES_PER_SYSTEM`.
pub const MAXIMUM_TYPES: usize = 4;

/// `enum` of a system's flags.
const SYSTEM_ACTIVE: u32 = 1 << 0;
const SYSTEM_INITIALIZING: u32 = 1 << 1;

/// `enum particle_system_type_flags`.
const TYPE_STATES_LOOP: u32 = 1 << 0;
const TYPE_STATES_LOOP_FORWARD_BACKWARD: u32 = 1 << 1;
const PARTICLE_STATES_LOOP: u32 = 1 << 2;
const PARTICLE_STATES_LOOP_FORWARD_BACKWARD: u32 = 1 << 3;
const DIES_IN_WATER: u32 = 1 << 4;
const DIES_IN_AIR: u32 = 1 << 5;
const DIES_ON_GROUND: u32 = 1 << 6;
const DISABLED: u32 = 1 << 8;
const TINT_BY_EFFECT_COLOR: u32 = 1 << 9;
const INITIAL_COUNT_SCALES: u32 = 1 << 10;
const MINIMUM_COUNT_SCALES: u32 = 1 << 11;
const CREATION_RATE_SCALES: u32 = 1 << 12;
const SCALE_SCALES: u32 = 1 << 13;
const ANIMATION_RATE_SCALES: u32 = 1 << 14;
const ROTATION_RATE_SCALES: u32 = 1 << 15;

/// `point_physics_update`'s result flags.
const PHYSICS_IN_AIR: i32 = 1 << 0;
const PHYSICS_IN_WATER: i32 = 1 << 1;
const PHYSICS_COLLIDED_WITH_STRUCTURE: i32 = 1 << 2;

/// `TICKS_PER_SECOND`.
const TICKS_PER_SECOND: f32 = 30.0;

/// The type state's interpolated variables (`struct
/// particle_system_type_state_interpolated_variables`), as floats.
const VARIABLE_SCALE: usize = 0;
const VARIABLE_ANIMATION_RATE: usize = 1;
const VARIABLE_ROTATION_RATE: usize = 2;
const VARIABLE_ALPHA: usize = 3;
const VARIABLE_RADIUS: usize = 7;
const VARIABLE_MINIMUM_PARTICLE_COUNT: usize = 8;
const VARIABLE_PARTICLE_CREATION_RATE: usize = 9;

/// A particle system's calls of the rest of the game (halo_sim.h's
/// `_particle_system_call_*`).
pub mod call {
    /// The object `a` it follows, with its function `b` from the scale
    /// `f[0]`: the result whether it is active (connected to the map and the
    /// function has a value), `out` the scale, its origin and velocity.
    pub const OBJECT: i32 = 1;
    /// The system's point physics `a` from the location in `c`, `d` (leaf;
    /// cluster and bonus), the position `f[0..3]`, velocity `f[3..6]`, radius
    /// `f[6]` and time `f[7]`: `out` the position and velocity,
    /// `out_location_*` the location.
    pub const SYSTEM_PHYSICS: i32 = 2;
    /// The markers of the object `a`'s attachment `b`: the result their
    /// count, marker `c`'s position and forward in `out`.
    pub const MARKERS: i32 = 3;
    /// `object_get_location(a)`: in `out_location_*`.
    pub const OBJECT_LOCATION: i32 = 4;
    /// A particle's datum: the result its index (NONE for none).
    pub const PARTICLE_NEW: i32 = 5;
    /// `scenario_location_from_point` at `f`, from the location in `c`, `d`.
    pub const LOCATION_FROM_POINT: i32 = 6;
    /// Deleting the particle `a`'s datum.
    pub const PARTICLE_DELETE: i32 = 7;
    /// A particle's point physics `a` (interpolated with `b` by `f[0]`, or
    /// alone when `f[0]` is negative: an interpolation's is pinned), from the location in `c`, `d`, the position `f[1..4]`,
    /// velocity `f[4..7]`, radius `f[7]` and time `f[8]`: the result its
    /// flags, `out` the position and velocity, `out_location_*` the location.
    pub const PARTICLE_PHYSICS: i32 = 8;
    /// `particle_system_delete` (the system written to the game first).
    pub const DELETE: i32 = 9;
}

/// halo_sim.h's `struct halo_sim_particle_system_call`.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ParticleSystemCall {
    pub kind: i32,
    pub a: i32,
    pub b: i32,
    pub c: i32,
    pub d: i32,
    pub f: [f32; 9],
    pub result: i32,
    pub out: [f32; 9],
    pub out_location_leaf: i32,
    pub out_location_cluster: i16,
    pub out_location_bonus: u16,
}

impl ParticleSystemCall {
    pub fn new(kind: i32) -> Self {
        ParticleSystemCall {
            kind,
            a: 0,
            b: 0,
            c: 0,
            d: 0,
            f: [0.0; 9],
            result: 0,
            out: [0.0; 9],
            out_location_leaf: 0,
            out_location_cluster: 0,
            out_location_bonus: 0,
        }
    }

    fn set_location(&mut self, leaf: i32, cluster: i16, bonus: u16) {
        self.c = leaf;
        self.d = (cluster as u16 as i32) | ((bonus as i32) << 16);
    }
}

/// One of a system's types (`struct particle_type`), as the game holds it.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ParticleType {
    pub state_index: i16,
    pub transition_state_index: i16,
    pub time_left_in_state: f32,
    pub state_length: f32,
    pub variables: [f32; 10],
    pub fractional_particle_count: f32,
    pub states_moving_forward: u8,
    pub reserved: u8,
    pub particle_count: i16,
    pub first_particle_index: i32,
}

/// What a system's update changes of it (`struct particle_system_datum`),
/// and the game's local random seed.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ParticleSystemState {
    pub system_index: u32,
    pub definition_index: u32,
    pub flags: u32,
    pub object_index: i32,
    pub attachment_index: i16,
    pub function_index: i16,
    pub scale: f32,
    pub location_leaf: i32,
    pub location_cluster: i16,
    pub location_bonus: u16,
    pub position: [f32; 3],
    pub velocity: [f32; 3],
    /// alpha, red, green, blue.
    pub color: [f32; 4],
    /// `physics_state.explosion.grounded`.
    pub grounded: u8,
    pub pad: [u8; 3],
    pub types: [ParticleType; MAXIMUM_TYPES],
    /// `get_global_local_random_seed_address`'s.
    pub local_random_seed: u32,
}

/// One of a system's particles (`struct ps_particle_datum`), with its datum
/// index; `next` is a datum index too.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct SystemParticle {
    pub index: i32,
    pub states_moving_forward: u8,
    pub valid: u8,
    pub pad: u16,
    pub next: i32,
    pub state_index: i16,
    pub transition_state_index: i16,
    pub time_left_in_state: f32,
    pub state_length: f32,
    pub location_leaf: i32,
    pub location_cluster: i16,
    pub location_bonus: u16,
    pub position: [f32; 3],
    pub velocity: [f32; 3],
    pub axis: [f32; 3],
    pub rotation: f32,
    pub sprite_index: f32,
    /// scale, animation rate, rotation rate, alpha, red, green, blue.
    pub randomized_variables: [f32; 7],
    pub transition_randomized_variables: [f32; 7],
}

const _: () = assert!(std::mem::size_of::<ParticleType>() == 64);
const _: () = assert!(std::mem::size_of::<ParticleSystemState>() == 336);
const _: () = assert!(std::mem::size_of::<SystemParticle>() == 132);
const _: () = assert!(std::mem::size_of::<ParticleSystemCall>() == 104);

/// What a frame's particle system updates read besides the system.
pub struct ParticleSystemInputs<'a> {
    pub dt: f32,
    /// `random_direction_table_get`'s table.
    pub directions: &'a [[f32; 3]],
}

/// The game's side of a particle system's update.
pub trait ParticleSystemHost {
    /// A call: the seed (and for `DELETE` the system and its particles)
    /// written to the game first, the seed read back after.
    fn call(&self, state: &mut ParticleSystemState, particles: &[SystemParticle], call: &mut ParticleSystemCall);
}

/// `struct particle_system_type_state`, as an update reads it.
#[derive(Debug, Clone, Default)]
pub struct TypeStateDefinition {
    pub duration_lower_bound: f32,
    pub duration_upper_bound: f32,
    pub transition_time_lower_bound: f32,
    pub transition_time_upper_bound: f32,
    pub variables: [f32; 10],
    pub particle_creation_physics: i16,
    pub particle_update_physics: i16,
}

/// `struct particle_system_type_particle_state`, as an update reads it.
#[derive(Debug, Clone, Default)]
pub struct ParticleStateDefinition {
    pub duration_lower_bound: f32,
    pub duration_upper_bound: f32,
    pub transition_time_lower_bound: f32,
    pub transition_time_upper_bound: f32,
    pub scale_lower_bound: f32,
    pub scale_upper_bound: f32,
    pub animation_rate_lower_bound: f32,
    pub animation_rate_upper_bound: f32,
    pub rotation_rate_lower_bound: f32,
    pub rotation_rate_upper_bound: f32,
    pub color_lower_bound: [f32; 4],
    pub color_upper_bound: [f32; 4],
    pub radius: f32,
    pub point_physics: i32,
}

/// `struct particle_system_type`, as an update reads it.
#[derive(Debug, Clone, Default)]
pub struct TypeDefinition {
    pub flags: u32,
    pub initial_particle_count: i16,
    pub radius: f32,
    pub initial_particle_creation_physics: i16,
    pub physics_constants: Vec<f32>,
    pub type_states: Vec<TypeStateDefinition>,
    pub particle_states: Vec<ParticleStateDefinition>,
}

/// `struct particle_system_definition`, as an update reads it.
#[derive(Debug, Clone, Default)]
pub struct ParticleSystemDefinition {
    pub system_update_point_physics: i32,
    pub system_update_physics: i16,
    /// No more than a system holds (the game cuts a tag's to that).
    pub types: Vec<TypeDefinition>,
}

fn floats<const N: usize>(p: crate::cache::Ptr, rel: usize) -> Result<[f32; N]> {
    let mut v = [0.0; N];
    for (i, x) in v.iter_mut().enumerate() {
        *x = p.f32(rel + 4 * i)?;
    }
    Ok(v)
}

impl ParticleSystemDefinition {
    pub fn load(cache: &CacheFile, datum: u32) -> Result<Self> {
        use layout::{
            particle_system_definition as d, particle_system_physics_constant as k, particle_system_type as t,
            particle_system_type_particle_state as p, particle_system_type_state as s,
        };
        let root = cache.tag_root(datum, crate::fourcc(b"pctl"))?;
        let mut types = Vec::new();
        for ty in root.block(d::TYPES, t::SIZEOF)?.iter().take(MAXIMUM_TYPES) {
            let physics_constants = ty.block(t::PHYSICS_CONSTANTS, k::SIZEOF)?.iter().map(|c| c.f32(0)).collect::<Result<_>>()?;
            let mut type_states = Vec::new();
            for state in ty.block(t::TYPE_STATES, s::SIZEOF)?.iter() {
                type_states.push(TypeStateDefinition {
                    duration_lower_bound: state.f32(s::DURATION_LOWER_BOUND)?,
                    duration_upper_bound: state.f32(s::DURATION_UPPER_BOUND)?,
                    transition_time_lower_bound: state.f32(s::TRANSITION_TIME_LOWER_BOUND)?,
                    transition_time_upper_bound: state.f32(s::TRANSITION_TIME_UPPER_BOUND)?,
                    variables: floats(state, s::VARIABLES)?,
                    particle_creation_physics: state.i16(s::PARTICLE_CREATION_PHYSICS)?,
                    particle_update_physics: state.i16(s::PARTICLE_UPDATE_PHYSICS)?,
                });
            }
            let mut particle_states = Vec::new();
            for state in ty.block(t::PARTICLE_STATES, p::SIZEOF)?.iter() {
                particle_states.push(ParticleStateDefinition {
                    duration_lower_bound: state.f32(p::DURATION_LOWER_BOUND)?,
                    duration_upper_bound: state.f32(p::DURATION_UPPER_BOUND)?,
                    transition_time_lower_bound: state.f32(p::TRANSITION_TIME_LOWER_BOUND)?,
                    transition_time_upper_bound: state.f32(p::TRANSITION_TIME_UPPER_BOUND)?,
                    scale_lower_bound: state.f32(p::SCALE_LOWER_BOUND)?,
                    scale_upper_bound: state.f32(p::SCALE_UPPER_BOUND)?,
                    animation_rate_lower_bound: state.f32(p::ANIMATION_RATE_LOWER_BOUND)?,
                    animation_rate_upper_bound: state.f32(p::ANIMATION_RATE_UPPER_BOUND)?,
                    rotation_rate_lower_bound: state.f32(p::ROTATION_RATE_LOWER_BOUND)?,
                    rotation_rate_upper_bound: state.f32(p::ROTATION_RATE_UPPER_BOUND)?,
                    color_lower_bound: floats(state, p::COLOR_LOWER_BOUND)?,
                    color_upper_bound: floats(state, p::COLOR_UPPER_BOUND)?,
                    radius: state.f32(p::VARIABLES)?,
                    point_physics: state.reference(p::POINT_PHYSICS)?.datum.map_or(NONE, |d| d as i32),
                });
            }
            types.push(TypeDefinition {
                flags: ty.u32(t::FLAGS)?,
                initial_particle_count: ty.i16(t::INITIAL_PARTICLE_COUNT)?,
                radius: ty.f32(t::VARIABLES)?,
                initial_particle_creation_physics: ty.i16(t::INITIAL_PARTICLE_CREATION_PHYSICS)?,
                physics_constants,
                type_states,
                particle_states,
            });
        }
        Ok(ParticleSystemDefinition {
            system_update_point_physics: root.reference(d::SYSTEM_UPDATE_POINT_PHYSICS)?.datum.map_or(NONE, |d| d as i32),
            system_update_physics: root.i16(d::SYSTEM_UPDATE_PHYSICS)?,
            types,
        })
    }
}

/// `seed_random` on the local seed.
fn local_random(seed: &mut u32) -> u16 {
    *seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
    (*seed >> 16) as u16
}

/// `real_local_random_range` (`real_seed_random_range` on the local seed).
fn real_local_random_range(seed: &mut u32, lower_bound: f32, upper_bound: f32) -> f32 {
    *seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
    let random = (*seed >> 16) as f32 / 65535.0;
    lower_bound + (upper_bound - lower_bound) * random
}

/// `seed_random_range` on the local seed.
fn local_random_range(seed: &mut u32, lower_bound: i16, upper_bound: i16) -> i16 {
    let random = local_random(seed) as u32;
    let span = (upper_bound as i32 - lower_bound as i32) as u32;
    lower_bound.wrapping_add((span.wrapping_mul(random) >> 16) as i16)
}

/// `local_random_direction3d`.
fn local_random_direction3d(seed: &mut u32, directions: &[[f32; 3]]) -> [f32; 3] {
    let index = local_random_range(seed, 0, directions.len() as i16);
    directions.get(index as usize).copied().unwrap_or([0.0; 3])
}

/// `PIN(n, a, b)` on shorts.
fn pin(n: i32, a: i32, b: i32) -> i32 {
    if n < a {
        a
    } else if n > b {
        b
    } else {
        n
    }
}

/// `PIN(t, 0, 1)` as the C writes it.
fn pin01(t: f32) -> f32 {
    if t < 0.0 {
        0.0
    } else if t > 1.0 {
        1.0
    } else {
        t
    }
}

/// `cross_product3d`.
fn cross3(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

/// `rotate_vector_about_axis`.
fn rotate_vector_about_axis(v: [f32; 3], n: [f32; 3], sine: f32, cosine: f32) -> [f32; 3] {
    let one_minus_cosine_times_v_dot_n = (n[0] * v[0] + n[1] * v[1] + n[2] * v[2]) * (1.0 - cosine);
    let v_cross_n = [n[2] * v[1] - v[2] * n[1], n[0] * v[2] - v[0] * n[2], n[1] * v[0] - v[1] * n[0]];
    [
        cosine * v[0] + one_minus_cosine_times_v_dot_n * n[0] - sine * v_cross_n[0],
        cosine * v[1] + one_minus_cosine_times_v_dot_n * n[1] - sine * v_cross_n[1],
        cosine * v[2] + one_minus_cosine_times_v_dot_n * n[2] - sine * v_cross_n[2],
    ]
}

/// `(long)value`, as x86 truncates it (0x80000000 out of range).
fn c_long(value: f32) -> i32 {
    if (-2_147_483_648.0..2_147_483_648.0).contains(&value) { value as i32 } else { i32::MIN }
}

/// A marker as particle creation reads it.
#[derive(Clone, Copy, Default)]
struct Marker {
    position: [f32; 3],
    forward: [f32; 3],
}

/// A system's update.
struct Update<'a, H: ParticleSystemHost> {
    definition: &'a ParticleSystemDefinition,
    inputs: &'a ParticleSystemInputs<'a>,
    host: &'a H,
}

/// The particle at a datum index the update follows (the game truncates
/// some to shorts: the absolute index is what finds it).
fn find(particles: &[SystemParticle], index: i32) -> Option<usize> {
    particles.iter().position(|p| p.index & 0xffff == index & 0xffff)
}

impl<H: ParticleSystemHost> Update<'_, H> {
    fn call(&self, state: &mut ParticleSystemState, particles: &[SystemParticle], call: &mut ParticleSystemCall) {
        self.host.call(state, particles, call);
    }

    /// `particle_system_next_type_state_index`.
    fn next_type_state_index(&self, state: &ParticleSystemState, type_index: usize, ty: &mut ParticleType) {
        let type_definition = &self.definition.types[type_index];
        let states_moving_forward = ty.states_moving_forward != 0;
        let step: i16 = if states_moving_forward { 1 } else { -1 };
        let state_index = ty.state_index;
        let next_state_index = state_index.wrapping_add(step);
        ty.transition_state_index = next_state_index;
        let count = type_definition.type_states.len() as i32;
        if next_state_index < 0 || next_state_index as i32 >= count {
            let flags = type_definition.flags;
            if flags & TYPE_STATES_LOOP == 0 || state.object_index == NONE || count <= 0 {
                ty.state_index = NONE as i16;
                ty.transition_state_index = NONE as i16;
            } else if flags & TYPE_STATES_LOOP_FORWARD_BACKWARD != 0 {
                ty.transition_state_index = pin(state_index as i32 - step as i32, 0, count - 1) as i16;
                ty.states_moving_forward = (!states_moving_forward) as u8;
            } else {
                ty.transition_state_index = 0;
            }
        }
    }

    /// `particle_system_next_particle_state_index`.
    fn next_particle_state_index(&self, type_index: usize, particle: &mut SystemParticle) {
        let type_definition = &self.definition.types[type_index];
        let states_moving_forward = particle.states_moving_forward != 0;
        let step: i16 = if states_moving_forward { 1 } else { -1 };
        let state_index = particle.state_index;
        let next_state_index = state_index.wrapping_add(step);
        particle.transition_state_index = next_state_index;
        let count = type_definition.particle_states.len() as i32;
        if next_state_index < 0 || next_state_index as i32 >= count {
            let flags = type_definition.flags;
            if flags & PARTICLE_STATES_LOOP != 0 && count > 0 {
                if flags & PARTICLE_STATES_LOOP_FORWARD_BACKWARD != 0 {
                    particle.transition_state_index = pin(state_index as i32 - step as i32, 0, count - 1) as i16;
                    particle.states_moving_forward = (!states_moving_forward) as u8;
                } else {
                    particle.transition_state_index = 0;
                }
            } else {
                particle.state_index = NONE as i16;
                particle.transition_state_index = NONE as i16;
            }
        }
    }

    /// `randomize_particle_variables`.
    fn randomize_particle_variables(&self, seed: &mut u32, type_index: usize, state_index: i16) -> [f32; 7] {
        let state_definition = self.definition.types[type_index]
            .particle_states
            .get(state_index as usize)
            .cloned()
            .unwrap_or_default();
        let color_fraction = real_local_random_range(seed, 0.0, 1.0);
        let mut v = [0.0; 7];
        v[1] = real_local_random_range(
            seed,
            state_definition.animation_rate_lower_bound,
            state_definition.animation_rate_upper_bound,
        );
        v[2] = real_local_random_range(
            seed,
            state_definition.rotation_rate_lower_bound,
            state_definition.rotation_rate_upper_bound,
        );
        v[0] = real_local_random_range(seed, state_definition.scale_lower_bound, state_definition.scale_upper_bound);
        v[3] = real_local_random_range(seed, state_definition.color_lower_bound[0], state_definition.color_upper_bound[0]);
        for c in 1..4 {
            v[3 + c] = (state_definition.color_upper_bound[c] - state_definition.color_lower_bound[c]) * color_fraction
                + state_definition.color_lower_bound[c];
        }
        v
    }

    /// The system physics (`particle_system_update_default`, and the
    /// explosion's, which is the same).
    fn update_system_physics(&self, state: &mut ParticleSystemState, particles: &[SystemParticle], dt: f32) {
        if state.object_index == NONE && self.definition.system_update_point_physics != NONE {
            let mut call = ParticleSystemCall::new(call::SYSTEM_PHYSICS);
            call.a = self.definition.system_update_point_physics;
            call.set_location(state.location_leaf, state.location_cluster, state.location_bonus);
            call.f[..3].copy_from_slice(&state.position);
            call.f[3..6].copy_from_slice(&state.velocity);
            call.f[6] = 1.0;
            call.f[7] = dt;
            self.call(state, particles, &mut call);
            state.position = [call.out[0], call.out[1], call.out[2]];
            state.velocity = [call.out[3], call.out[4], call.out[5]];
            (state.location_leaf, state.location_cluster, state.location_bonus) =
                (call.out_location_leaf, call.out_location_cluster, call.out_location_bonus);
        }
    }

    /// A particle's creation physics (`particle_creation_functions`).
    fn create_particle(
        &self,
        state: &mut ParticleSystemState,
        type_index: usize,
        function_index: i16,
        particle: &mut SystemParticle,
        marker: &Marker,
    ) {
        let constants = &self.definition.types[type_index].physics_constants;
        let constant = |i: usize| constants.get(i).copied().unwrap_or(0.0);
        match function_index {
            1 => {
                // particle_system_new_particle_explosion
                let xy_spread = constant(0);
                let z_spread = constant(1);
                let intensity = constant(2);
                let mut velocity = local_random_direction3d(&mut state.local_random_seed, self.inputs.directions);
                velocity[0] *= xy_spread;
                velocity[1] *= xy_spread;
                velocity[2] *= z_spread;
                if state.grounded != 0 {
                    velocity[2] = velocity[2].abs();
                }
                for c in 0..3 {
                    particle.position[c] = marker.position[c] + velocity[c];
                }
                particle.axis = [velocity[0], velocity[1], 0.0];
                for c in 0..3 {
                    particle.velocity[c] = velocity[c] * intensity + state.velocity[c];
                }
                particle.axis = rotate_vector_about_axis(particle.axis, [0.0, 0.0, 1.0], 1.0, 0.0);
            }
            2 => {
                // particle_system_new_particle_jet
                let mut velocity = constant(0);
                let spread_fraction = constant(1);
                let rotates_up = constant(2);
                velocity *= 1.0 / TICKS_PER_SECOND;
                let spread_scale = velocity * spread_fraction;
                let directed_scale = (1.0 - spread_fraction) * velocity;
                let spread = local_random_direction3d(&mut state.local_random_seed, self.inputs.directions);
                for c in 0..3 {
                    particle.velocity[c] =
                        directed_scale * marker.forward[c] + spread[c] * spread_scale + state.velocity[c];
                }
                particle.position = marker.position;
                particle.axis = if rotates_up != 0.0 {
                    cross3(particle.velocity, [0.0, 0.0, 1.0])
                } else {
                    cross3(marker.forward, particle.velocity)
                };
            }
            _ => {
                // particle_system_new_particle_default
                particle.position = marker.position;
                particle.velocity = state.velocity;
            }
        }
    }

    /// `particle_system_new_particles`.
    fn new_particles(
        &self,
        state: &mut ParticleSystemState,
        particles: &mut Vec<SystemParticle>,
        type_index: usize,
        dt: f32,
    ) {
        let type_definition = &self.definition.types[type_index];
        let initializing = state.flags & SYSTEM_INITIALIZING != 0;
        let state_definition = if initializing {
            None
        } else {
            type_definition.type_states.get(state.types[type_index].state_index as usize).cloned()
        };

        let target_particle_count: i16 = if initializing {
            if type_definition.flags & INITIAL_COUNT_SCALES != 0 {
                c_long(type_definition.initial_particle_count as f32 * state.scale + 0.5) as i16
            } else {
                type_definition.initial_particle_count
            }
        } else {
            let ty = &mut state.types[type_index];
            let particle_count = dt * ty.variables[VARIABLE_PARTICLE_CREATION_RATE];
            let whole_particle_count = c_long(particle_count) as i16;
            let mut target = ty.particle_count.wrapping_add(whole_particle_count);
            ty.fractional_particle_count += particle_count - whole_particle_count as f32;
            if ty.fractional_particle_count > 1.0 {
                target = target.wrapping_add(1);
                ty.fractional_particle_count -= 1.0;
            }
            target
        };

        if state.types[type_index].particle_count < target_particle_count {
            let mut markers = Vec::new();
            if state.object_index != NONE {
                let mut count = 1;
                let mut k = 0;
                while k < count && k < 8 {
                    let mut call = ParticleSystemCall::new(call::MARKERS);
                    call.a = state.object_index;
                    call.b = state.attachment_index as i32;
                    call.c = k;
                    self.call(state, particles, &mut call);
                    count = call.result;
                    if k < count {
                        markers.push(Marker {
                            position: [call.out[0], call.out[1], call.out[2]],
                            forward: [call.out[3], call.out[4], call.out[5]],
                        });
                    }
                    k += 1;
                }
                let mut call = ParticleSystemCall::new(call::OBJECT_LOCATION);
                call.a = state.object_index;
                self.call(state, particles, &mut call);
                (state.location_leaf, state.location_cluster, state.location_bonus) =
                    (call.out_location_leaf, call.out_location_cluster, call.out_location_bonus);
            } else {
                markers.push(Marker { position: state.position, forward: [0.0; 3] });
            }

            if state.location_cluster != NONE as i16 {
                let mut particles_created: i16 = 0;
                while state.types[type_index].particle_count < target_particle_count {
                    if markers.is_empty() || particles_created >= 128 {
                        break;
                    }
                    let mut call = ParticleSystemCall::new(call::PARTICLE_NEW);
                    self.call(state, particles, &mut call);
                    let particle_index = call.result;
                    if particle_index == NONE {
                        break;
                    }
                    let mut particle = SystemParticle {
                        index: particle_index,
                        valid: 1,
                        state_index: NONE as i16,
                        transition_state_index: NONE as i16,
                        states_moving_forward: 1,
                        sprite_index: -1.0,
                        ..Default::default()
                    };
                    particle.rotation =
                        real_local_random_range(&mut state.local_random_seed, 0.0, std::f32::consts::PI * 2.0);
                    let mut creation_function_index = if initializing {
                        type_definition.initial_particle_creation_physics
                    } else {
                        state_definition.as_ref().map_or(0, |s| s.particle_creation_physics)
                    };
                    // (the game makes a bad one the default in the tag)
                    if !(0..3).contains(&creation_function_index) {
                        creation_function_index = 0;
                    }
                    let marker_index =
                        local_random_range(&mut state.local_random_seed, 0, markers.len() as i16) as usize;
                    let marker = markers.get(marker_index).copied().unwrap_or_default();
                    self.create_particle(state, type_index, creation_function_index, &mut particle, &marker);

                    let mut call = ParticleSystemCall::new(call::LOCATION_FROM_POINT);
                    call.set_location(particle.location_leaf, particle.location_cluster, particle.location_bonus);
                    call.f[..3].copy_from_slice(&particle.position);
                    self.call(state, particles, &mut call);
                    (particle.location_leaf, particle.location_cluster, particle.location_bonus) =
                        (call.out_location_leaf, call.out_location_cluster, call.out_location_bonus);

                    if particle.location_cluster != NONE as i16 {
                        let ty = &mut state.types[type_index];
                        ty.particle_count = ty.particle_count.wrapping_add(1);
                        particle.next = ty.first_particle_index;
                        ty.first_particle_index = particle_index;
                        particles.push(particle);
                    } else {
                        let mut call = ParticleSystemCall::new(call::PARTICLE_DELETE);
                        call.a = particle_index;
                        self.call(state, particles, &mut call);
                    }
                    particles_created += 1;
                }
            }
        }

        let ty = &mut state.types[type_index];
        if (ty.particle_count as f32) < ty.variables[VARIABLE_MINIMUM_PARTICLE_COUNT] {
            // (real)pow(0.30000001f, delta_time * TICKS_PER_SECOND): 0.30000001f is
            // the float 0.3f
            let factor = (0.3f32 as f64).powf((dt * TICKS_PER_SECOND) as f64) as f32;
            ty.time_left_in_state *= factor;
        }
    }

    /// `particle_system_update_particle_default`.
    fn update_particle(
        &self,
        state: &mut ParticleSystemState,
        particles: &[SystemParticle],
        type_index: usize,
        dt: f32,
        particle: &mut SystemParticle,
    ) {
        let type_definition = &self.definition.types[type_index];
        let ty = &state.types[type_index];
        let state_definition = type_definition.particle_states.get(particle.state_index as usize).cloned().unwrap_or_default();
        let mut call = ParticleSystemCall::new(call::PARTICLE_PHYSICS);
        let radius;
        if particle.transition_state_index == NONE as i16 {
            radius = type_definition.radius * ty.variables[VARIABLE_RADIUS] * state_definition.radius;
            call.a = state_definition.point_physics;
            call.b = NONE;
            call.f[0] = -1.0;
        } else {
            let transition = type_definition
                .particle_states
                .get(particle.transition_state_index as usize)
                .cloned()
                .unwrap_or_default();
            let t = pin01(particle.time_left_in_state / particle.state_length);
            let inverse_t = 1.0 - t;
            radius = type_definition.radius
                * ty.variables[VARIABLE_RADIUS]
                * (state_definition.radius * t + transition.radius * inverse_t);
            call.a = state_definition.point_physics;
            call.b = transition.point_physics;
            call.f[0] = t;
        }
        call.set_location(particle.location_leaf, particle.location_cluster, particle.location_bonus);
        call.f[1..4].copy_from_slice(&particle.position);
        call.f[4..7].copy_from_slice(&particle.velocity);
        call.f[7] = radius;
        call.f[8] = dt;
        self.call(state, particles, &mut call);
        particle.position = [call.out[0], call.out[1], call.out[2]];
        particle.velocity = [call.out[3], call.out[4], call.out[5]];
        (particle.location_leaf, particle.location_cluster, particle.location_bonus) =
            (call.out_location_leaf, call.out_location_cluster, call.out_location_bonus);
        let collision_flags = call.result;
        let flags = type_definition.flags;
        if (collision_flags & PHYSICS_IN_AIR != 0 && flags & DIES_IN_AIR != 0)
            || (collision_flags & PHYSICS_IN_WATER != 0 && flags & DIES_IN_WATER != 0)
            || (collision_flags & PHYSICS_COLLIDED_WITH_STRUCTURE != 0 && flags & DIES_ON_GROUND != 0)
        {
            particle.valid = 0;
        }
    }

    /// One type's states (the first half of `particle_system_update`'s type
    /// loop).
    fn update_type_states(&self, state: &mut ParticleSystemState, type_index: usize) {
        let type_definition = &self.definition.types[type_index];
        let mut ty = state.types[type_index];
        while ty.state_index != NONE as i16 {
            let state_definition = type_definition.type_states.get(ty.state_index as usize).cloned().unwrap_or_default();
            if ty.time_left_in_state < 0.0 {
                let duration;
                if ty.transition_state_index == NONE as i16 {
                    self.next_type_state_index(state, type_index, &mut ty);
                    duration = real_local_random_range(
                        &mut state.local_random_seed,
                        state_definition.transition_time_lower_bound,
                        state_definition.transition_time_upper_bound,
                    );
                } else {
                    ty.state_index = ty.transition_state_index;
                    ty.transition_state_index = NONE as i16;
                    let transition = type_definition.type_states.get(ty.state_index as usize).cloned().unwrap_or_default();
                    duration = real_local_random_range(
                        &mut state.local_random_seed,
                        transition.duration_lower_bound,
                        transition.duration_upper_bound,
                    );
                }
                ty.state_length = duration;
                ty.time_left_in_state += duration;
            } else {
                if ty.transition_state_index == NONE as i16 {
                    ty.variables = state_definition.variables;
                } else {
                    let transition =
                        type_definition.type_states.get(ty.transition_state_index as usize).cloned().unwrap_or_default();
                    let t = pin01(ty.time_left_in_state / ty.state_length);
                    for i in 0..10 {
                        ty.variables[i] = state_definition.variables[i] * t + transition.variables[i] * (1.0 - t);
                    }
                }
                let flags = type_definition.flags;
                if flags & TINT_BY_EFFECT_COLOR != 0 {
                    for c in 0..4 {
                        ty.variables[VARIABLE_ALPHA + c] *= state.color[c];
                    }
                }
                if flags & MINIMUM_COUNT_SCALES != 0 {
                    ty.variables[VARIABLE_MINIMUM_PARTICLE_COUNT] *= state.scale;
                }
                if flags & CREATION_RATE_SCALES != 0 {
                    ty.variables[VARIABLE_PARTICLE_CREATION_RATE] *= state.scale;
                }
                if flags & SCALE_SCALES != 0 {
                    ty.variables[VARIABLE_SCALE] *= state.scale;
                }
                if flags & ANIMATION_RATE_SCALES != 0 {
                    ty.variables[VARIABLE_ANIMATION_RATE] *= state.scale;
                }
                if flags & ROTATION_RATE_SCALES != 0 {
                    ty.variables[VARIABLE_ROTATION_RATE] *= state.scale;
                }
                break;
            }
        }
        state.types[type_index] = ty;
    }

    /// One type's particles (the second half of `particle_system_update`'s
    /// type loop).
    fn update_type_particles(
        &self,
        state: &mut ParticleSystemState,
        particles: &mut Vec<SystemParticle>,
        type_index: usize,
        dt: f32,
    ) {
        let type_definition = &self.definition.types[type_index];
        let mut previous: Option<i32> = None;
        // (the game follows the list through shorts)
        let mut particle_index = state.types[type_index].first_particle_index as i16 as i32;
        while particle_index != NONE {
            let Some(slot) = find(particles, particle_index) else { break };
            let mut particle = particles[slot];
            particle.time_left_in_state -= dt;

            if particle.state_index == NONE as i16 && !type_definition.particle_states.is_empty() {
                particle.state_index = 0;
                let state_definition = &type_definition.particle_states[0];
                let duration = real_local_random_range(
                    &mut state.local_random_seed,
                    state_definition.duration_lower_bound,
                    state_definition.duration_upper_bound,
                );
                particle.time_left_in_state = duration;
                particle.state_length = duration;
                particle.randomized_variables =
                    self.randomize_particle_variables(&mut state.local_random_seed, type_index, particle.state_index);
            }

            if particle.valid == 0 {
                particle.state_index = NONE as i16;
            }

            while particle.state_index != NONE as i16 {
                let state_definition =
                    type_definition.particle_states.get(particle.state_index as usize).cloned().unwrap_or_default();
                if particle.time_left_in_state < 0.0 {
                    let duration;
                    if particle.transition_state_index == NONE as i16 {
                        self.next_particle_state_index(type_index, &mut particle);
                        duration = real_local_random_range(
                            &mut state.local_random_seed,
                            state_definition.transition_time_lower_bound,
                            state_definition.transition_time_upper_bound,
                        );
                    } else {
                        particle.state_index = particle.transition_state_index;
                        particle.transition_state_index = NONE as i16;
                        let transition =
                            type_definition.particle_states.get(particle.state_index as usize).cloned().unwrap_or_default();
                        duration = real_local_random_range(
                            &mut state.local_random_seed,
                            transition.duration_lower_bound,
                            transition.duration_upper_bound,
                        );
                    }
                    particle.state_length = duration;
                    particle.time_left_in_state += duration;
                    if particle.transition_state_index != NONE as i16 {
                        particle.transition_randomized_variables = self.randomize_particle_variables(
                            &mut state.local_random_seed,
                            type_index,
                            particle.transition_state_index,
                        );
                    } else {
                        particle.randomized_variables = particle.transition_randomized_variables;
                    }
                } else {
                    break;
                }
            }

            if particle.state_index == NONE as i16 {
                let next = particle.next;
                match previous.and_then(|p| find(particles, p)) {
                    Some(p) => particles[p].next = next,
                    None => state.types[type_index].first_particle_index = next,
                }
                particles[slot] = particle;
                let mut call = ParticleSystemCall::new(call::PARTICLE_DELETE);
                call.a = particle_index;
                self.call(state, particles, &mut call);
                particles.retain(|p| p.index & 0xffff != particle_index & 0xffff);
                particle_index = next as i16 as i32;
                let ty = &mut state.types[type_index];
                ty.particle_count = ty.particle_count.wrapping_sub(1);
            } else {
                let multipliers = state.types[type_index].variables;
                if particle.transition_state_index == NONE as i16 {
                    particle.rotation +=
                        particle.randomized_variables[2] * multipliers[VARIABLE_ROTATION_RATE] * dt;
                    particle.sprite_index +=
                        particle.randomized_variables[1] * multipliers[VARIABLE_ANIMATION_RATE] * dt;
                } else {
                    let t = pin01(particle.time_left_in_state / particle.state_length);
                    particle.rotation += (particle.transition_randomized_variables[2] * (1.0 - t)
                        + particle.randomized_variables[2] * t)
                        * multipliers[VARIABLE_ROTATION_RATE]
                        * dt;
                    particle.sprite_index += (particle.transition_randomized_variables[1] * (1.0 - t)
                        + particle.randomized_variables[1] * t)
                        * multipliers[VARIABLE_ANIMATION_RATE]
                        * dt;
                }
                // (the game's particle update physics are all the default)
                self.update_particle(state, particles, type_index, dt, &mut particle);
                particles[slot] = particle;
                previous = Some(particle_index);
                particle_index = particle.next as i16 as i32;
            }
        }
    }

    /// `particle_system_update`: false if it deleted the system.
    fn update(&self, state: &mut ParticleSystemState, particles: &mut Vec<SystemParticle>) -> bool {
        let dt = self.inputs.dt;
        let definition = self.definition;
        let mut live_type_count = 0;

        if state.object_index != NONE {
            let mut call = ParticleSystemCall::new(call::OBJECT);
            call.a = state.object_index;
            call.b = state.function_index as i32;
            call.f[0] = state.scale;
            self.call(state, particles, &mut call);
            state.scale = call.out[0];
            if call.result != 0 {
                state.flags |= SYSTEM_ACTIVE;
            } else {
                state.flags &= !SYSTEM_ACTIVE;
            }
            state.position = [call.out[1], call.out[2], call.out[3]];
            state.velocity = [call.out[4], call.out[5], call.out[6]];
            for c in 0..3 {
                state.velocity[c] *= TICKS_PER_SECOND;
            }
        }

        self.update_system_physics(state, particles, dt);

        for type_index in 0..definition.types.len().min(MAXIMUM_TYPES) {
            if definition.types[type_index].flags & DISABLED != 0 {
                continue;
            }
            state.types[type_index].time_left_in_state -= dt;
            self.update_type_states(state, type_index);
            if state.types[type_index].state_index != NONE as i16 {
                if state.flags & SYSTEM_ACTIVE != 0 {
                    self.new_particles(state, particles, type_index, dt);
                }
                self.update_type_particles(state, particles, type_index, dt);
                live_type_count += 1;
            }
        }

        state.flags &= !SYSTEM_INITIALIZING;

        if live_type_count == 0 && state.object_index == NONE {
            let mut call = ParticleSystemCall::new(call::DELETE);
            self.call(state, particles, &mut call);
            return false;
        }
        true
    }
}

/// A particle system's update over the frame (`particle_system_update`),
/// from `state` and its `particles`, where they are left (the particles in
/// no order). False if it deleted the system.
pub fn particle_system_update(
    definition: &ParticleSystemDefinition,
    inputs: &ParticleSystemInputs,
    host: &impl ParticleSystemHost,
    state: &mut ParticleSystemState,
    particles: &mut Vec<SystemParticle>,
) -> bool {
    Update { definition, inputs, host }.update(state, particles)
}

/// A system's particles in the order the game walks them: each type's list
/// from its first particle (by the full datum indices the lists hold).
pub fn particles_in_order(state: &ParticleSystemState, type_count: usize, particles: &[SystemParticle]) -> Vec<SystemParticle> {
    let mut ordered = Vec::with_capacity(particles.len());
    for ty in state.types.iter().take(type_count.min(MAXIMUM_TYPES)) {
        let mut index = ty.first_particle_index;
        while index != NONE {
            let Some(particle) = particles.iter().find(|p| p.index == index) else { break };
            ordered.push(*particle);
            index = particle.next;
            if ordered.len() > particles.len() {
                break;
            }
        }
    }
    ordered
}
