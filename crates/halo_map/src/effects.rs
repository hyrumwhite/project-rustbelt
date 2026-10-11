//! Effect updates (`effects.c`'s `effect_update`, with `effect_generate_parts`,
//! `effect_generate_part` and `effect_generate_particles`): an effect's
//! attachment to its object, its visibility, its events' delays and
//! durations (from the game's random seeds), and the parts and particles each
//! event makes.
//!
//! What it does to the rest of the game goes through [`EffectHost::call`]
//! (effects.c's `effect_call`, shared with the C): reading the object it is
//! attached to, its function values and change colours, visibility, water,
//! node matrices, and making particle systems, sounds, objects, decals,
//! damage, lights and particles, and deleting the effect.

use crate::cache::CacheFile;
use crate::collision::{transform_point, transform_vector};
use crate::model::Matrix4x3;
use crate::{Result, layout};

const NONE: i32 = -1;

/// `MAXIMUM_EFFECT_LOCATIONS`.
pub const MAXIMUM_EFFECT_LOCATIONS: usize = 32;
/// `MAXIMUM_EFFECT_EVENTS`: the size of an effect's particle counts.
pub const MAXIMUM_EFFECT_EVENTS: usize = 32;
const MAXIMUM_EFFECT_EVENTS_PER_UPDATE: i16 = 8;
const MAXIMUM_SPLIT_SCREEN_PARTICLE_COUNT: i32 = 6;

/// `enum effect_flags`.
const EFFECT_IN_DURATION: u16 = 1 << 0;
const EFFECT_LOOP: u16 = 1 << 1;
const EFFECT_STOPPING: u16 = 1 << 2;
const EFFECT_STOPPED: u16 = 1 << 3;
const EFFECT_INVISIBLE: u16 = 1 << 4;
const EFFECT_DELETE_ON_STOP: u16 = 1 << 5;
const EFFECT_NONVIOLENT: u16 = 1 << 6;

/// `_effect_location_first_person_bit`.
const LOCATION_FIRST_PERSON: i16 = 1 << 15;
const LOCATION_NODE_MASK: i16 = LOCATION_FIRST_PERSON.wrapping_sub(1);

/// `enum effect_camera_mode`.
const CAMERA_MODE_INDEPENDENT: i16 = 0;
const CAMERA_MODE_FIRST_PERSON_ONLY: i16 = 1;
const CAMERA_MODE_THIRD_PERSON_ONLY: i16 = 2;
const CAMERA_MODE_BOTH: i16 = 3;

/// `_effect_part_definition_world_down_bit`.
const PART_WORLD_DOWN: u16 = 1 << 0;
/// `_effect_part_type_scale_bit`.
const PART_SCALE_BIT: u32 = 5;

/// `enum effect_particle_definition_flags`.
const PARTICLE_ATTACHED: u32 = 1 << 0;
const PARTICLE_RANDOM_ORIENTATION: u32 = 1 << 1;
const PARTICLE_TINT_FROM_CHANGE_COLOR: u32 = 1 << 2;
const PARTICLE_TINT_INTERPOLATE_SHIFT: u32 = 3;

/// `enum effect_particle_scale_flags`.
const PARTICLE_COUNT_BIT: u32 = 5;
const PARTICLE_DISTRIBUTION_RADIUS_BIT: u32 = 7;
const PARTICLE_RADIUS_BIT: u32 = 9;
const PARTICLE_TINT_BIT: u32 = 11;

/// `enum effect_velocity_scaleable_values_flags`.
const VELOCITY_BIT: u32 = 0;
const VELOCITY_CONE_BIT: u32 = 2;
const ANGULAR_VELOCITY_BIT: u32 = 3;

/// `enum effect_environment`.
const ENVIRONMENT_ANYWHERE: i16 = 0;
const ENVIRONMENT_AIR: i16 = 1;
const ENVIRONMENT_WATER: i16 = 2;
const ENVIRONMENT_VACUUM: i16 = 3;

/// `enum effect_disposition`.
const DISPOSITION_VIOLENT: i16 = 1;
const DISPOSITION_NONVIOLENT: i16 = 2;

/// `enum effect_definition_flags`.
const DEFINITION_DELETED_WHEN_INACTIVE: u32 = 1 << 0;
const DEFINITION_MUST_BE_DETERMINISTIC: u32 = 1 << 1;

/// The parts' base classes (`runtime_base_class_tag`).
const PARTICLE_SYSTEM_TAG: u32 = crate::fourcc(b"pctl");
const SOUND_TAG: u32 = crate::fourcc(b"snd!");
const OBJECT_TAG: u32 = crate::fourcc(b"obje");
const DECAL_TAG: u32 = crate::fourcc(b"deca");
const DAMAGE_EFFECT_TAG: u32 = crate::fourcc(b"jpt!");
const LIGHT_TAG: u32 = crate::fourcc(b"ligh");

/// An effect's calls of the rest of the game (halo_sim.h's `_effect_call_*`).
pub mod call {
    /// The object it is attached to: `a` the object; the result 0 if it is
    /// gone, 1 if its ultimate parent isn't connected to the map, 2 if it is
    /// (`out` its velocity, `out_location_*` its location).
    pub const OBJECT: i32 = 1;
    /// `effect_delete`.
    pub const DELETE: i32 = 2;
    /// Taking it out of its object's attachments (`a`), and deleting it.
    pub const DETACH_DELETE: i32 = 3;
    /// `object_get_function_value(a, b)`, from `f[0]`: the result whether it
    /// has one, `out[0]` the value.
    pub const FUNCTION_VALUE: i32 = 4;
    /// The object's (`a`) outgoing change colour `b`, in `out`.
    pub const CHANGE_COLOR: i32 = 5;
    /// Whether its location is potentially visible (`a` 1 for the
    /// deterministic test).
    pub const VISIBLE: i32 = 6;
    /// Whether the point `f` is under water, from its location.
    pub const UNDERWATER: i32 = 7;
    /// `effect_get_node_matrix(a)`, in `out` (scale, forward, left, up,
    /// position).
    pub const NODE_MATRIX: i32 = 8;
    /// Its impulse field's translational function at the point `f`, in
    /// `out`.
    pub const IMPULSE: i32 = 9;
    /// `particle_system_new_unattached`: `a` the definition; `f` the point,
    /// velocity, tint and scale.
    pub const PARTICLE_SYSTEM: i32 = 10;
    /// An impulse sound: `a` the object (NONE unattached), `b` the
    /// definition, `c` the node; `f` the point, forward and scale.
    pub const SOUND: i32 = 11;
    /// `object_new`: `a` the definition, `b` the owner; `f` the point,
    /// forward, up, velocity and angular velocity.
    pub const OBJECT_NEW: i32 = 12;
    /// `decal_new`: `a` the definition; `f` the point, velocity and radius.
    pub const DECAL: i32 = 13;
    /// `area_of_effect_cause_damage`: `a` the damage effect; `f` the point,
    /// direction and scale.
    pub const DAMAGE: i32 = 14;
    /// `light_new_unattached`: `a` the definition, `b` the object, `c` the
    /// node; `f` the point, forward and scale.
    pub const LIGHT: i32 = 15;
    /// `particle_new`: `a` the definition, `b` the object, `c` the node (and
    /// above it 1 << 16 attached to the local player, 1 << 17 not drawn first
    /// person, 1 << 18 not drawn third person), `d` the local player; `f`
    /// the position, direction, velocity, initial impulse, rotation, angular
    /// velocity, radius and colour (alpha first).
    pub const PARTICLE: i32 = 16;
}

/// halo_sim.h's `struct halo_sim_effect_call`: a call of the game, and its
/// answer.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EffectCall {
    pub kind: i32,
    pub a: i32,
    pub b: i32,
    pub c: i32,
    pub d: i32,
    pub f: [f32; 20],
    pub result: i32,
    pub out: [f32; 16],
    pub out_location_leaf: i32,
    pub out_location_cluster: i16,
    pub out_location_bonus: u16,
}

impl EffectCall {
    pub fn new(kind: i32) -> Self {
        EffectCall {
            kind,
            a: 0,
            b: 0,
            c: 0,
            d: 0,
            f: [0.0; 20],
            result: 0,
            out: [0.0; 16],
            out_location_leaf: 0,
            out_location_cluster: 0,
            out_location_bonus: 0,
        }
    }
}

/// What an effect's update changes of the effect (`struct effect_datum`),
/// and the game's two random seeds.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct EffectState {
    pub effect_index: u32,
    pub definition_index: u32,
    pub flags: u16,
    pub scale_a_function_index: i16,
    pub scale_b_function_index: i16,
    pub change_color_index: i16,
    pub location_leaf: i32,
    pub location_cluster: i16,
    pub location_bonus: u16,
    pub color: [f32; 3],
    pub velocity: [f32; 3],
    pub object_index: i32,
    pub owner_object_index: i32,
    pub scale_a: f32,
    pub scale_b: f32,
    pub local_player_index: i16,
    pub event_index: i16,
    pub event_time: f32,
    pub event_duration: f32,
    pub last_event_fraction: f32,
    pub particle_counts: [u8; MAXIMUM_EFFECT_EVENTS],
    /// Whether its impulse field has a translational function.
    pub has_translational_impulse: u32,
    /// `get_global_random_seed_address`'s.
    pub random_seed: u32,
    /// `get_global_local_random_seed_address`'s.
    pub local_random_seed: u32,
}

const _: () = assert!(std::mem::size_of::<EffectState>() == 124);
const _: () = assert!(std::mem::size_of::<EffectCall>() == 176);

/// One of an effect's location instances (`struct effect_location_datum`),
/// as the game holds them: `next` is the next instance's index in the same
/// list (NONE for none).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct EffectInstance {
    pub node_designator: i16,
    pub pad: i16,
    pub next: i32,
    /// scale, forward, left, up, position.
    pub matrix: [f32; 13],
}

impl EffectInstance {
    fn matrix(&self) -> Matrix4x3 {
        matrix_from(&self.matrix)
    }
    fn position(&self) -> [f32; 3] {
        [self.matrix[10], self.matrix[11], self.matrix[12]]
    }
    fn forward(&self) -> [f32; 3] {
        [self.matrix[1], self.matrix[2], self.matrix[3]]
    }
    fn up(&self) -> [f32; 3] {
        [self.matrix[7], self.matrix[8], self.matrix[9]]
    }
}

fn matrix_from(m: &[f32]) -> Matrix4x3 {
    Matrix4x3 {
        scale: m[0],
        forward: [m[1], m[2], m[3]],
        left: [m[4], m[5], m[6]],
        up: [m[7], m[8], m[9]],
        position: [m[10], m[11], m[12]],
    }
}

/// An effect's locations: each location's first instance (NONE for none),
/// and the instances.
pub struct EffectLocations<'a> {
    pub first: [i32; MAXIMUM_EFFECT_LOCATIONS],
    pub instances: &'a [EffectInstance],
}

/// What an effect's update reads of the game besides the effect.
pub struct EffectInputs<'a> {
    /// `local_player_count()`.
    pub local_player_count: i16,
    /// `random_direction_table_get`'s table.
    pub directions: &'a [[f32; 3]],
}

/// The game's side of an effect's update.
pub trait EffectHost {
    /// A call: the effect (and the seeds) written to the game first and read
    /// back after.
    fn call(&self, state: &mut EffectState, call: &mut EffectCall);
}

#[derive(Debug, Clone, Default)]
pub struct PartDefinition {
    pub environment: i16,
    pub disposition: i16,
    pub location_index: i16,
    pub flags: u16,
    pub base_class: u32,
    pub reference: i32,
    pub velocity_lower_bound: f32,
    pub velocity_upper_bound: f32,
    pub velocity_cone_angle: f32,
    pub angular_velocity_lower_bound: f32,
    pub angular_velocity_upper_bound: f32,
    pub radius_modifier_lower_bound: f32,
    pub radius_modifier_upper_bound: f32,
    pub scale_a_flags: u32,
    pub scale_b_flags: u32,
}

#[derive(Debug, Clone, Default)]
pub struct ParticlesDefinition {
    pub environment: i16,
    pub disposition: i16,
    pub camera_mode: i16,
    pub location_index: i16,
    pub offset: [f32; 3],
    pub runtime_direction: [f32; 3],
    pub particle: i32,
    pub flags: u32,
    pub distribution_function: i16,
    pub count_lower_bound: i16,
    pub count_upper_bound: i16,
    pub distribution_radius_lower_bound: f32,
    pub distribution_radius_upper_bound: f32,
    pub velocity_lower_bound: f32,
    pub velocity_upper_bound: f32,
    pub velocity_cone_angle: f32,
    pub angular_velocity_lower_bound: f32,
    pub angular_velocity_upper_bound: f32,
    pub radius_lower_bound: f32,
    pub radius_upper_bound: f32,
    /// alpha, red, green, blue.
    pub tint_lower_bound: [f32; 4],
    pub tint_upper_bound: [f32; 4],
    pub scale_a_flags: u32,
    pub scale_b_flags: u32,
}

#[derive(Debug, Clone, Default)]
pub struct EventDefinition {
    pub skip_fraction: f32,
    pub delay_lower_bound: f32,
    pub delay_upper_bound: f32,
    pub duration_lower_bound: f32,
    pub duration_upper_bound: f32,
    pub parts: Vec<PartDefinition>,
    /// No more than the effect counts (the game cuts a tag's to that).
    pub particles: Vec<ParticlesDefinition>,
}

/// `struct effect_definition`, as an update reads it.
#[derive(Debug, Clone, Default)]
pub struct EffectDefinition {
    pub flags: u32,
    pub loop_start_index: i16,
    pub loop_stop_index: i16,
    /// No more than the effect holds (the game cuts a tag's to that).
    pub location_count: i32,
    pub events: Vec<EventDefinition>,
}

fn reference(p: crate::cache::Ptr, rel: usize) -> Result<i32> {
    Ok(p.reference(rel)?.datum.map_or(NONE, |d| d as i32))
}

fn vector(p: crate::cache::Ptr, rel: usize) -> Result<[f32; 3]> {
    Ok([p.f32(rel)?, p.f32(rel + 4)?, p.f32(rel + 8)?])
}

fn argb(p: crate::cache::Ptr, rel: usize) -> Result<[f32; 4]> {
    Ok([p.f32(rel)?, p.f32(rel + 4)?, p.f32(rel + 8)?, p.f32(rel + 12)?])
}

impl EffectDefinition {
    pub fn load(cache: &CacheFile, datum: u32) -> Result<Self> {
        use layout::{
            effect_definition as d, effect_event_definition as e, effect_location_definition as l,
            effect_part_definition as p, effect_particles_definition as q,
        };
        let root = cache.tag_root(datum, crate::fourcc(b"effe"))?;
        let locations = root.block(d::LOCATIONS, l::SIZEOF)?;
        let mut events = Vec::new();
        for event in root.block(d::EVENTS, e::SIZEOF)?.iter() {
            let mut parts = Vec::new();
            for part in event.block(e::PARTS, p::SIZEOF)?.iter() {
                parts.push(PartDefinition {
                    environment: part.i16(p::ENVIRONMENT)?,
                    disposition: part.i16(p::DISPOSITION)?,
                    location_index: part.i16(p::LOCATION_INDEX)?,
                    flags: part.u16(p::FLAGS)?,
                    base_class: part.u32(p::RUNTIME_BASE_CLASS_TAG)?,
                    reference: reference(part, p::REFERENCE)?,
                    velocity_lower_bound: part.f32(p::VELOCITY_LOWER_BOUND)?,
                    velocity_upper_bound: part.f32(p::VELOCITY_UPPER_BOUND)?,
                    velocity_cone_angle: part.f32(p::VELOCITY_CONE_ANGLE)?,
                    angular_velocity_lower_bound: part.f32(p::ANGULAR_VELOCITY_LOWER_BOUND)?,
                    angular_velocity_upper_bound: part.f32(p::ANGULAR_VELOCITY_UPPER_BOUND)?,
                    radius_modifier_lower_bound: part.f32(p::RADIUS_MODIFIER_LOWER_BOUND)?,
                    radius_modifier_upper_bound: part.f32(p::RADIUS_MODIFIER_UPPER_BOUND)?,
                    scale_a_flags: part.u32(p::SCALE_A_FLAGS)?,
                    scale_b_flags: part.u32(p::SCALE_B_FLAGS)?,
                });
            }
            let mut particles = Vec::new();
            for particle in event.block(e::PARTICLES, q::SIZEOF)?.iter().take(MAXIMUM_EFFECT_EVENTS) {
                particles.push(ParticlesDefinition {
                    environment: particle.i16(q::ENVIRONMENT)?,
                    disposition: particle.i16(q::DISPOSITION)?,
                    camera_mode: particle.i16(q::CAMERA_MODE)?,
                    location_index: particle.i16(q::LOCATION_INDEX)?,
                    offset: vector(particle, q::OFFSET)?,
                    runtime_direction: vector(particle, q::RUNTIME_DIRECTION)?,
                    particle: reference(particle, q::PARTICLE)?,
                    flags: particle.u32(q::FLAGS)?,
                    distribution_function: particle.i16(q::DISTRIBUTION_FUNCTION)?,
                    count_lower_bound: particle.i16(q::COUNT_LOWER_BOUND)?,
                    count_upper_bound: particle.i16(q::COUNT_UPPER_BOUND)?,
                    distribution_radius_lower_bound: particle.f32(q::DISTRIBUTION_RADIUS_LOWER_BOUND)?,
                    distribution_radius_upper_bound: particle.f32(q::DISTRIBUTION_RADIUS_UPPER_BOUND)?,
                    velocity_lower_bound: particle.f32(q::VELOCITY_LOWER_BOUND)?,
                    velocity_upper_bound: particle.f32(q::VELOCITY_UPPER_BOUND)?,
                    velocity_cone_angle: particle.f32(q::VELOCITY_CONE_ANGLE)?,
                    angular_velocity_lower_bound: particle.f32(q::ANGULAR_VELOCITY_LOWER_BOUND)?,
                    angular_velocity_upper_bound: particle.f32(q::ANGULAR_VELOCITY_UPPER_BOUND)?,
                    radius_lower_bound: particle.f32(q::RADIUS_LOWER_BOUND)?,
                    radius_upper_bound: particle.f32(q::RADIUS_UPPER_BOUND)?,
                    tint_lower_bound: argb(particle, q::TINT_LOWER_BOUND)?,
                    tint_upper_bound: argb(particle, q::TINT_UPPER_BOUND)?,
                    scale_a_flags: particle.u32(q::SCALE_A_FLAGS)?,
                    scale_b_flags: particle.u32(q::SCALE_B_FLAGS)?,
                });
            }
            events.push(EventDefinition {
                skip_fraction: event.f32(e::SKIP_FRACTION)?,
                delay_lower_bound: event.f32(e::DELAY_LOWER_BOUND)?,
                delay_upper_bound: event.f32(e::DELAY_UPPER_BOUND)?,
                duration_lower_bound: event.f32(e::DURATION_LOWER_BOUND)?,
                duration_upper_bound: event.f32(e::DURATION_UPPER_BOUND)?,
                parts,
                particles,
            });
        }
        Ok(EffectDefinition {
            flags: root.u32(d::FLAGS)?,
            loop_start_index: root.i16(d::LOOP_START_INDEX)?,
            loop_stop_index: root.i16(d::LOOP_STOP_INDEX)?,
            location_count: (locations.count as i32).min(MAXIMUM_EFFECT_LOCATIONS as i32),
            events,
        })
    }
}

/// Which of the game's random seeds a draw takes.
#[derive(Clone, Copy)]
enum Seed {
    /// `get_global_random_seed_address`.
    Global,
    /// `get_global_local_random_seed_address`.
    Local,
}

/// An effect's update.
struct Update<'a, H: EffectHost> {
    definition: &'a EffectDefinition,
    inputs: &'a EffectInputs<'a>,
    locations: &'a EffectLocations<'a>,
    host: &'a H,
}

/// `effect_scale`'s scales.
#[derive(Clone, Copy)]
struct Scales {
    a: f32,
    b: f32,
}

impl Scales {
    /// `effect_scale`.
    fn scale(self, value: f32, scale_a_flags: u32, scale_b_flags: u32, bit: u32) -> f32 {
        let mut scaled_value = value;
        if scale_a_flags & (1 << bit) != 0 {
            scaled_value *= self.a;
        }
        if scale_b_flags & (1 << bit) != 0 {
            scaled_value *= self.b;
        }
        scaled_value
    }
}

fn seed_of(state: &mut EffectState, seed: Seed) -> &mut u32 {
    match seed {
        Seed::Global => &mut state.random_seed,
        Seed::Local => &mut state.local_random_seed,
    }
}

/// `seed_random`.
fn seed_random(seed: &mut u32) -> u16 {
    *seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
    (*seed >> 16) as u16
}

/// `real_seed_random`.
fn real_seed_random(seed: &mut u32) -> f32 {
    *seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
    (*seed >> 16) as f32 / 65535.0
}

/// `real_seed_random_range`.
fn real_seed_random_range(seed: &mut u32, lower_bound: f32, upper_bound: f32) -> f32 {
    let random = real_seed_random(seed);
    lower_bound + (upper_bound - lower_bound) * random
}

/// `seed_random_direction3d`.
fn seed_random_direction3d(seed: &mut u32, directions: &[[f32; 3]]) -> [f32; 3] {
    let span = directions.len() as i16 as i32 as u32;
    let index = ((span.wrapping_mul(seed_random(seed) as u32)) >> 16) as i16;
    directions.get(index as usize).copied().unwrap_or([0.0; 3])
}

/// `effect_real_random_range`.
#[allow(clippy::too_many_arguments)]
fn effect_real_random_range(
    seed: &mut u32,
    scales: Scales,
    lower_bound: f32,
    upper_bound: f32,
    scale_a_flags: u32,
    scale_b_flags: u32,
    first_bit: u32,
) -> f32 {
    let base = scales.scale(lower_bound, scale_a_flags, scale_b_flags, first_bit);
    let range = scales.scale(upper_bound - lower_bound, scale_a_flags, scale_b_flags, first_bit + 1);
    real_seed_random_range(seed, 0.0, range) + base
}

/// `effect_evaluate_function_integral`.
fn effect_evaluate_function_integral(function_index: i16, fraction: f32) -> f32 {
    if fraction == -1.0 {
        return 0.0;
    }
    match function_index {
        0 => 1.0,
        1 => {
            if fraction < 1.0 {
                0.0
            } else {
                1.0
            }
        }
        2 => fraction,
        3 => fraction * fraction,
        4 => (2.0 - fraction) * fraction,
        5 => (3.0 - (fraction + fraction)) * fraction * fraction,
        _ => fraction,
    }
}

/// `effect_part_allowed_by_disposition`.
fn effect_part_allowed_by_disposition(nonviolent: bool, disposition: i16) -> bool {
    if nonviolent {
        return disposition != DISPOSITION_VIOLENT;
    }
    disposition != DISPOSITION_NONVIOLENT
}

/// `matrix4x3_transform_normal`.
fn transform_normal(m: &Matrix4x3, n: [f32; 3]) -> [f32; 3] {
    let [i, j, k] = n;
    [
        i * m.forward[0] + j * m.left[0] + k * m.up[0],
        i * m.forward[1] + j * m.left[1] + k * m.up[1],
        i * m.forward[2] + j * m.left[2] + k * m.up[2],
    ]
}

fn dot3(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// `rotate_vector_about_axis`.
fn rotate_vector_about_axis(v: [f32; 3], n: [f32; 3], sine: f32, cosine: f32) -> [f32; 3] {
    let one_minus_cosine_times_v_dot_n = dot3(n, v) * (1.0 - cosine);
    let v_cross_n = [n[2] * v[1] - v[2] * n[1], n[0] * v[2] - v[0] * n[2], n[1] * v[0] - v[1] * n[0]];
    [
        cosine * v[0] + one_minus_cosine_times_v_dot_n * n[0] - sine * v_cross_n[0],
        cosine * v[1] + one_minus_cosine_times_v_dot_n * n[1] - sine * v_cross_n[1],
        cosine * v[2] + one_minus_cosine_times_v_dot_n * n[2] - sine * v_cross_n[2],
    ]
}

fn put3(f: &mut [f32], at: usize, v: [f32; 3]) {
    f[at..at + 3].copy_from_slice(&v);
}

impl<H: EffectHost> Update<'_, H> {
    fn call(&self, state: &mut EffectState, call: &mut EffectCall) {
        self.host.call(state, call);
    }

    fn simple(&self, state: &mut EffectState, kind: i32) {
        let mut call = EffectCall::new(kind);
        self.call(state, &mut call);
    }

    /// `effect_get_random_seed`.
    fn definition_seed(&self) -> Seed {
        if self.definition.flags & DEFINITION_MUST_BE_DETERMINISTIC != 0 {
            Seed::Global
        } else {
            Seed::Local
        }
    }

    /// `effect_set_event`.
    fn set_event(&self, state: &mut EffectState, event_index: i16) {
        if event_index >= 0 && (event_index as usize) < self.definition.events.len() {
            let event = &self.definition.events[event_index as usize];
            state.flags &= !EFFECT_IN_DURATION;
            state.event_index = event_index;
            state.event_time = 0.0;
            state.event_duration = real_seed_random_range(
                seed_of(state, self.definition_seed()),
                event.delay_lower_bound,
                event.delay_upper_bound,
            );
        }
    }

    /// `effect_stop`: false if it deleted the effect.
    fn stop(&self, state: &mut EffectState, and_delete: bool) -> bool {
        if state.flags & EFFECT_LOOP != 0 {
            if and_delete {
                state.flags |= EFFECT_DELETE_ON_STOP;
            } else {
                state.flags &= !EFFECT_DELETE_ON_STOP;
            }
            let stop_event_index = self.definition.loop_stop_index;
            if stop_event_index < 0 || stop_event_index as i32 + 1 >= self.definition.events.len() as i32 {
                state.flags |= EFFECT_STOPPED;
            } else {
                self.set_event(state, stop_event_index + 1);
                state.flags |= EFFECT_STOPPING;
            }
            true
        } else {
            self.simple(state, call::DELETE);
            false
        }
    }

    /// `effect_location_get_next_instance`.
    fn next_instance(&self, state: &EffectState, index: &mut i32, camera_mode: i16) -> Option<&EffectInstance> {
        loop {
            if *index == NONE {
                return None;
            }
            let location = self.locations.instances.get(*index as usize)?;
            *index = location.next;
            let first_person =
                location.node_designator != NONE as i16 && location.node_designator & LOCATION_FIRST_PERSON != 0;
            if camera_mode == CAMERA_MODE_FIRST_PERSON_ONLY
                || (camera_mode == CAMERA_MODE_BOTH
                    && state.local_player_index != NONE as i16
                    && self.inputs.local_player_count == 1)
            {
                if !first_person {
                    continue;
                }
            } else if first_person {
                continue;
            }
            return Some(location);
        }
    }

    /// `effect_get_node_matrix`.
    fn node_matrix(&self, state: &mut EffectState, node_designator: i16) -> Matrix4x3 {
        let mut call = EffectCall::new(call::NODE_MATRIX);
        call.a = node_designator as i32;
        self.call(state, &mut call);
        matrix_from(&call.out[..13])
    }

    /// `effect_allowed_by_environment`.
    fn allowed_by_environment(&self, state: &mut EffectState, environment: i16, world_point: [f32; 3]) -> bool {
        let underwater = |update: &Self, state: &mut EffectState| {
            let mut call = EffectCall::new(call::UNDERWATER);
            put3(&mut call.f, 0, world_point);
            update.call(state, &mut call);
            call.result != 0
        };
        match environment {
            ENVIRONMENT_ANYWHERE => true,
            ENVIRONMENT_AIR => !underwater(self, state),
            ENVIRONMENT_WATER => underwater(self, state),
            ENVIRONMENT_VACUUM => false,
            // (the game's asserts; it would return an unset value)
            _ => false,
        }
    }

    /// `effect_random_translational_velocity`: the direction and velocity.
    #[allow(clippy::too_many_arguments)]
    fn random_translational_velocity(
        &self,
        state: &mut EffectState,
        seed: Seed,
        forward: [f32; 3],
        lower_bound: f32,
        upper_bound: f32,
        cone_angle: f32,
        scale_a_flags: u32,
        scale_b_flags: u32,
    ) -> ([f32; 3], [f32; 3]) {
        let scales = Scales { a: state.scale_a, b: state.scale_b };
        let seed = seed_of(state, seed);
        let magnitude =
            effect_real_random_range(seed, scales, lower_bound, upper_bound, scale_a_flags, scale_b_flags, VELOCITY_BIT);
        let cone_angle = scales.scale(cone_angle, scale_a_flags, scale_b_flags, VELOCITY_CONE_BIT);
        let angle = real_seed_random(seed) * cone_angle;
        let mut direction = forward;
        if angle != 0.0 {
            let axis = seed_random_direction3d(seed, self.inputs.directions);
            direction = rotate_vector_about_axis(direction, axis, sine(angle), cosine(angle));
        }
        let velocity = [magnitude * direction[0], magnitude * direction[1], magnitude * direction[2]];
        (direction, velocity)
    }

    /// `effect_random_angular_velocity`.
    #[allow(clippy::too_many_arguments)]
    fn random_angular_velocity(
        &self,
        state: &mut EffectState,
        seed: Seed,
        lower_bound: f32,
        upper_bound: f32,
        scale_a_flags: u32,
        scale_b_flags: u32,
    ) -> [f32; 3] {
        let scales = Scales { a: state.scale_a, b: state.scale_b };
        let seed = seed_of(state, seed);
        let magnitude = effect_real_random_range(
            seed,
            scales,
            lower_bound,
            upper_bound,
            scale_a_flags,
            scale_b_flags,
            ANGULAR_VELOCITY_BIT,
        );
        if magnitude != 0.0 {
            let v = seed_random_direction3d(seed, self.inputs.directions);
            [v[0] * magnitude, v[1] * magnitude, v[2] * magnitude]
        } else {
            [0.0; 3]
        }
    }

    /// `effect_generate_part`.
    #[allow(clippy::too_many_arguments)]
    fn generate_part(
        &self,
        state: &mut EffectState,
        part: &PartDefinition,
        instance: &EffectInstance,
        world_point: [f32; 3],
        world_forward: [f32; 3],
        world_up: [f32; 3],
        scale: f32,
    ) {
        match part.base_class {
            PARTICLE_SYSTEM_TAG => {
                let (_, mut velocity) = self.random_translational_velocity(
                    state,
                    Seed::Local,
                    world_forward,
                    part.velocity_lower_bound,
                    part.velocity_upper_bound,
                    part.velocity_cone_angle,
                    part.scale_a_flags,
                    part.scale_b_flags,
                );
                velocity[0] += state.velocity[0];
                velocity[1] += state.velocity[1];
                velocity[2] += state.velocity[2];
                let mut call = EffectCall::new(call::PARTICLE_SYSTEM);
                call.a = part.reference;
                put3(&mut call.f, 0, world_point);
                put3(&mut call.f, 3, velocity);
                put3(&mut call.f, 6, state.color);
                call.f[9] = scale;
                self.call(state, &mut call);
            }
            SOUND_TAG => {
                let mut call = EffectCall::new(call::SOUND);
                call.b = part.reference;
                if state.object_index != NONE {
                    let mut node_index = instance.node_designator;
                    if node_index != NONE as i16 {
                        node_index &= LOCATION_NODE_MASK;
                    }
                    call.a = state.object_index;
                    call.c = node_index as i32;
                    put3(&mut call.f, 0, instance.position());
                    put3(&mut call.f, 3, instance.forward());
                } else {
                    call.a = NONE;
                    put3(&mut call.f, 0, world_point);
                    put3(&mut call.f, 3, world_forward);
                }
                call.f[6] = scale;
                self.call(state, &mut call);
            }
            OBJECT_TAG => {
                let (_, mut velocity) = self.random_translational_velocity(
                    state,
                    Seed::Global,
                    world_forward,
                    part.velocity_lower_bound,
                    part.velocity_upper_bound,
                    part.velocity_cone_angle,
                    part.scale_a_flags,
                    part.scale_b_flags,
                );
                velocity[0] += state.velocity[0];
                velocity[1] += state.velocity[1];
                velocity[2] += state.velocity[2];
                let angular_velocity = self.random_angular_velocity(
                    state,
                    Seed::Global,
                    part.angular_velocity_lower_bound,
                    part.angular_velocity_upper_bound,
                    part.scale_a_flags,
                    part.scale_b_flags,
                );
                let mut call = EffectCall::new(call::OBJECT_NEW);
                call.a = part.reference;
                call.b = state.owner_object_index;
                put3(&mut call.f, 0, world_point);
                put3(&mut call.f, 3, world_forward);
                put3(&mut call.f, 6, world_up);
                put3(&mut call.f, 9, velocity);
                put3(&mut call.f, 12, angular_velocity);
                self.call(state, &mut call);
            }
            DECAL_TAG => {
                let (_, velocity) = self.random_translational_velocity(
                    state,
                    Seed::Local,
                    world_forward,
                    part.velocity_lower_bound,
                    part.velocity_upper_bound,
                    part.velocity_cone_angle,
                    part.scale_a_flags,
                    part.scale_b_flags,
                );
                let radius_modifier = real_seed_random_range(
                    &mut state.local_random_seed,
                    part.radius_modifier_lower_bound,
                    part.radius_modifier_upper_bound,
                );
                let mut call = EffectCall::new(call::DECAL);
                call.a = part.reference;
                put3(&mut call.f, 0, world_point);
                put3(&mut call.f, 3, velocity);
                call.f[6] = radius_modifier;
                self.call(state, &mut call);
            }
            DAMAGE_EFFECT_TAG => {
                let mut call = EffectCall::new(call::DAMAGE);
                call.a = part.reference;
                put3(&mut call.f, 0, world_point);
                put3(&mut call.f, 3, world_forward);
                call.f[6] = scale;
                self.call(state, &mut call);
            }
            LIGHT_TAG => {
                let mut node_index = instance.node_designator;
                if node_index != NONE as i16 {
                    node_index &= LOCATION_NODE_MASK;
                }
                let mut call = EffectCall::new(call::LIGHT);
                call.a = part.reference;
                call.b = state.object_index;
                call.c = node_index as i32;
                put3(&mut call.f, 0, instance.position());
                put3(&mut call.f, 3, instance.forward());
                call.f[6] = scale;
                self.call(state, &mut call);
            }
            // (a bad part: the game asserts)
            _ => {}
        }
    }

    /// `effect_generate_parts`.
    fn generate_parts(&self, state: &mut EffectState) {
        let Some(event) = self.definition.events.get(state.event_index as usize) else { return };
        for part in &event.parts {
            if part.location_index < 0
                || part.location_index as i32 >= self.definition.location_count
                || part.location_index as usize >= MAXIMUM_EFFECT_LOCATIONS
                || part.reference == NONE
            {
                continue;
            }
            if !effect_part_allowed_by_disposition(state.flags & EFFECT_NONVIOLENT != 0, part.disposition) {
                continue;
            }
            let mut index = self.locations.first[part.location_index as usize];
            while let Some(instance) = self.next_instance(state, &mut index, CAMERA_MODE_INDEPENDENT) {
                let instance = *instance;
                let (world_point, mut world_forward, mut world_up);
                if instance.node_designator != NONE as i16 {
                    let node_matrix = self.node_matrix(state, instance.node_designator);
                    world_point = transform_point(&node_matrix, instance.position());
                    world_forward = transform_normal(&node_matrix, instance.forward());
                    world_up = transform_normal(&node_matrix, instance.up());
                } else {
                    world_point = instance.position();
                    world_forward = instance.forward();
                    world_up = instance.up();
                }
                if part.flags & PART_WORLD_DOWN != 0 {
                    world_forward = [0.0, 0.0, -1.0];
                    world_up = [1.0, 0.0, 0.0];
                }
                if self.allowed_by_environment(state, part.environment, world_point) {
                    let scale = Scales { a: state.scale_a, b: state.scale_b }.scale(
                        1.0,
                        part.scale_a_flags,
                        part.scale_b_flags,
                        PART_SCALE_BIT,
                    );
                    self.generate_part(state, part, &instance, world_point, world_forward, world_up, scale);
                }
            }
        }
    }

    /// `effect_generate_particles`.
    fn generate_particles(&self, state: &mut EffectState) {
        let Some(event) = self.definition.events.get(state.event_index as usize) else { return };
        let last_event_fraction = state.last_event_fraction;
        let event_fraction = if state.event_duration > 0.0 { state.event_time / state.event_duration } else { 1.0 };

        for (particle_index, particles) in event.particles.iter().enumerate().take(MAXIMUM_EFFECT_EVENTS) {
            if particles.location_index < 0
                || particles.location_index as i32 >= self.definition.location_count
                || particles.location_index as usize >= MAXIMUM_EFFECT_LOCATIONS
            {
                continue;
            }
            if !effect_part_allowed_by_disposition(state.flags & EFFECT_NONVIOLENT != 0, particles.disposition) {
                continue;
            }
            let count = state.particle_counts[particle_index] as i32;
            let last_particle_count = c_long(
                effect_evaluate_function_integral(particles.distribution_function, last_event_fraction) * count as f32,
            );
            let count_delta = c_long(
                effect_evaluate_function_integral(particles.distribution_function, event_fraction) * count as f32,
            )
            .wrapping_sub(last_particle_count) as i16;
            if count_delta <= 0 {
                continue;
            }

            let mut index = self.locations.first[particles.location_index as usize];
            let mut instance = self.next_instance(state, &mut index, particles.camera_mode).copied();
            while let Some(location) = instance {
                for _ in 0..count_delta {
                    self.generate_particle(state, particles, &location);
                }
                instance = self.next_instance(state, &mut index, particles.camera_mode).copied();
            }
        }

        state.last_event_fraction = event_fraction;
    }

    /// One particle of `effect_generate_particles`.
    fn generate_particle(&self, state: &mut EffectState, particles: &ParticlesDefinition, instance: &EffectInstance) {
        let scales = Scales { a: state.scale_a, b: state.scale_b };
        let emission_radius = effect_real_random_range(
            &mut state.local_random_seed,
            scales,
            particles.distribution_radius_lower_bound,
            particles.distribution_radius_upper_bound,
            particles.scale_a_flags,
            particles.scale_b_flags,
            PARTICLE_DISTRIBUTION_RADIUS_BIT,
        );
        let random_offset = seed_random_direction3d(&mut state.local_random_seed, self.inputs.directions);
        let matrix = instance.matrix();
        let relative_offset = transform_vector(&matrix, particles.offset);
        let mut position = [0.0; 3];
        for c in 0..3 {
            position[c] = matrix.position[c] + random_offset[c] * emission_radius + relative_offset[c];
        }

        let (direction, velocity) = self.random_translational_velocity(
            state,
            Seed::Local,
            particles.runtime_direction,
            particles.velocity_lower_bound,
            particles.velocity_upper_bound,
            particles.velocity_cone_angle,
            particles.scale_a_flags,
            particles.scale_b_flags,
        );
        let mut direction = transform_normal(&matrix, direction);
        let mut velocity = transform_vector(&matrix, velocity);

        let (world_point, world_direction, world_velocity);
        if instance.node_designator != NONE as i16 {
            let node_matrix = self.node_matrix(state, instance.node_designator);
            world_point = transform_point(&node_matrix, position);
            world_direction = transform_normal(&node_matrix, direction);
            world_velocity = transform_vector(&node_matrix, velocity);
        } else {
            world_point = position;
            world_direction = direction;
            world_velocity = velocity;
        }

        if !self.allowed_by_environment(state, particles.environment, world_point) {
            return;
        }

        let object_index;
        let mut node_index = 0i16;
        let mut initial_impulse = [0.0; 3];
        if particles.flags & PARTICLE_ATTACHED != 0 {
            node_index = instance.node_designator;
            if node_index != NONE as i16 {
                node_index &= LOCATION_NODE_MASK;
            }
            object_index = state.object_index;
        } else {
            if state.has_translational_impulse != 0 {
                let mut call = EffectCall::new(call::IMPULSE);
                put3(&mut call.f, 0, world_point);
                self.call(state, &mut call);
                initial_impulse = [call.out[0], call.out[1], call.out[2]];
            }
            object_index = NONE;
            position = world_point;
            direction = world_direction;
            velocity = [
                world_velocity[0] + state.velocity[0] * 30.0,
                world_velocity[1] + state.velocity[1] * 30.0,
                world_velocity[2] + state.velocity[2] * 30.0,
            ];
        }

        let scales = Scales { a: state.scale_a, b: state.scale_b };
        let radius = effect_real_random_range(
            &mut state.local_random_seed,
            scales,
            particles.radius_lower_bound,
            particles.radius_upper_bound,
            particles.scale_a_flags,
            particles.scale_b_flags,
            PARTICLE_RADIUS_BIT,
        );
        let angular_velocity = effect_real_random_range(
            &mut state.local_random_seed,
            scales,
            particles.angular_velocity_lower_bound,
            particles.angular_velocity_upper_bound,
            particles.scale_a_flags,
            particles.scale_b_flags,
            ANGULAR_VELOCITY_BIT,
        );
        let rotation = if particles.flags & PARTICLE_RANDOM_ORIENTATION != 0 {
            real_seed_random_range(&mut state.local_random_seed, 0.0, 2.0 * std::f32::consts::PI)
        } else {
            0.0
        };

        let mut alpha = if (particles.scale_a_flags | particles.scale_b_flags) & (1 << PARTICLE_TINT_BIT) != 0 {
            scales.scale(1.0, particles.scale_a_flags, particles.scale_b_flags, PARTICLE_TINT_BIT)
        } else {
            real_seed_random(&mut state.local_random_seed)
        };
        let lower = &particles.tint_lower_bound;
        let upper = &particles.tint_upper_bound;
        let mut rgb = [0.0; 3];
        crate::object_functions::rgb_colors_interpolate(
            &mut rgb,
            (particles.flags >> PARTICLE_TINT_INTERPOLATE_SHIFT) & 3,
            [lower[1], lower[2], lower[3]],
            [upper[1], upper[2], upper[3]],
            alpha,
        );
        alpha = (1.0 - alpha) * lower[0] + upper[0] * alpha;
        if particles.flags & PARTICLE_TINT_FROM_CHANGE_COLOR != 0 {
            rgb[0] *= state.color[0];
            rgb[1] *= state.color[1];
            rgb[2] *= state.color[2];
        }

        let attached_to_local_player =
            instance.node_designator != NONE as i16 && instance.node_designator & LOCATION_FIRST_PERSON != 0;
        let mut call = EffectCall::new(call::PARTICLE);
        call.a = particles.particle;
        call.b = object_index;
        call.c = (node_index as u16 as i32)
            | ((attached_to_local_player as i32) << 16)
            | (((particles.camera_mode == CAMERA_MODE_THIRD_PERSON_ONLY) as i32) << 17)
            | (((particles.camera_mode == CAMERA_MODE_FIRST_PERSON_ONLY) as i32) << 18);
        call.d = state.local_player_index as i32;
        put3(&mut call.f, 0, position);
        put3(&mut call.f, 3, direction);
        put3(&mut call.f, 6, velocity);
        put3(&mut call.f, 9, initial_impulse);
        call.f[12] = rotation;
        call.f[13] = angular_velocity;
        call.f[14] = radius;
        call.f[15] = alpha;
        put3(&mut call.f, 16, rgb);
        self.call(state, &mut call);
    }

    /// The attached object's part of `effect_update`: false if it deleted the
    /// effect.
    fn update_attached(&self, state: &mut EffectState) -> bool {
        let mut call = EffectCall::new(call::OBJECT);
        call.a = state.object_index;
        self.call(state, &mut call);
        if call.result == 0 {
            self.simple(state, call::DELETE);
            return false;
        }
        if call.result == 2 {
            state.location_leaf = call.out_location_leaf;
            state.location_cluster = call.out_location_cluster;
            state.location_bonus = call.out_location_bonus;
            state.velocity = [call.out[0], call.out[1], call.out[2]];
        } else {
            state.location_cluster = NONE as i16;
        }

        if state.flags & EFFECT_LOOP != 0 {
            let mut call = EffectCall::new(call::FUNCTION_VALUE);
            call.a = state.object_index;
            call.b = state.scale_a_function_index as i32;
            call.f[0] = state.scale_a;
            self.call(state, &mut call);
            state.scale_a = call.out[0];
            if call.result == 0 {
                if self.definition.flags & DEFINITION_DELETED_WHEN_INACTIVE != 0 {
                    let mut call = EffectCall::new(call::DETACH_DELETE);
                    call.a = state.object_index;
                    self.call(state, &mut call);
                    return false;
                } else if state.flags & EFFECT_STOPPING == 0 && state.flags & EFFECT_STOPPED == 0 && !self.stop(state, false)
                {
                    return false;
                }
            } else {
                let flags = state.flags;
                if flags & EFFECT_STOPPED != 0 {
                    if flags & EFFECT_DELETE_ON_STOP != 0 {
                        self.simple(state, call::DELETE);
                        return false;
                    }
                    state.flags = flags & !EFFECT_STOPPED;
                    self.set_event(state, 0);
                }
            }

            let mut call = EffectCall::new(call::FUNCTION_VALUE);
            call.a = state.object_index;
            call.b = state.scale_b_function_index as i32;
            call.f[0] = state.scale_b;
            self.call(state, &mut call);
            state.scale_b = call.out[0];

            if state.change_color_index != NONE as i16 {
                let mut call = EffectCall::new(call::CHANGE_COLOR);
                call.a = state.object_index;
                call.b = state.change_color_index as i32;
                self.call(state, &mut call);
                state.color = [call.out[0], call.out[1], call.out[2]];
            }
        }
        true
    }

    /// `effect_update`.
    fn update(&self, state: &mut EffectState, mut dt: f32) {
        let definition = self.definition;
        if state.object_index != NONE && !self.update_attached(state) {
            return;
        }

        let mut visible = false;
        if state.location_cluster != NONE as i16 {
            let mut call = EffectCall::new(call::VISIBLE);
            call.a = (definition.flags & DEFINITION_MUST_BE_DETERMINISTIC != 0) as i32;
            self.call(state, &mut call);
            visible = call.result != 0;
        }
        if !visible {
            let flags = state.flags;
            if flags & EFFECT_INVISIBLE == 0 {
                if flags & EFFECT_LOOP != 0 {
                    state.flags = flags | EFFECT_INVISIBLE;
                } else {
                    self.simple(state, call::DELETE);
                    return;
                }
            }
        } else {
            let flags = state.flags;
            if flags & EFFECT_INVISIBLE != 0 {
                state.flags = flags & !EFFECT_INVISIBLE;
            }
        }

        let mut iteration_index: i16 = 0;
        while dt >= 0.0 {
            let flags = state.flags;
            if flags & EFFECT_STOPPED != 0 || iteration_index >= MAXIMUM_EFFECT_EVENTS_PER_UPDATE {
                break;
            }
            let event_completed;
            if state.event_duration - state.event_time <= dt {
                dt -= state.event_duration - state.event_time;
                event_completed = true;
                state.event_time = state.event_duration;
            } else {
                state.event_time += dt;
                event_completed = false;
                dt = -1.0;
            }

            if flags & EFFECT_IN_DURATION != 0 {
                if flags & EFFECT_INVISIBLE == 0 {
                    self.generate_particles(state);
                }
                if event_completed {
                    let mut next_event_index = if state.flags & EFFECT_LOOP != 0
                        && state.event_index == definition.loop_stop_index
                        && definition.loop_start_index != NONE as i16
                    {
                        definition.loop_start_index
                    } else {
                        state.event_index.wrapping_add(1)
                    };
                    while (next_event_index as i32) < definition.events.len() as i32
                        && real_seed_random(seed_of(state, self.definition_seed()))
                            < definition.events[next_event_index as usize].skip_fraction
                    {
                        next_event_index += 1;
                    }
                    if next_event_index as i32 >= definition.events.len() as i32 {
                        let end_flags = state.flags;
                        if end_flags & EFFECT_LOOP != 0 {
                            state.flags = end_flags | EFFECT_STOPPED;
                        } else {
                            self.simple(state, call::DELETE);
                        }
                        return;
                    }
                    self.set_event(state, next_event_index);
                }
            } else if event_completed {
                let Some(event) = definition.events.get(state.event_index as usize) else { return };
                state.flags |= EFFECT_IN_DURATION;
                state.event_time = 0.0;
                state.last_event_fraction = -1.0;
                state.event_duration = real_seed_random_range(
                    seed_of(state, self.definition_seed()),
                    event.duration_lower_bound,
                    event.duration_upper_bound,
                );
                let scales = Scales { a: state.scale_a, b: state.scale_b };
                for (particle_index, particles) in event.particles.iter().enumerate() {
                    let count = c_long(effect_real_random_range(
                        &mut state.local_random_seed,
                        scales,
                        particles.count_lower_bound as f32,
                        particles.count_upper_bound as f32,
                        particles.scale_a_flags,
                        particles.scale_b_flags,
                        PARTICLE_COUNT_BIT,
                    )) as u8;
                    state.particle_counts[particle_index] = count;
                    if count as i32 > MAXIMUM_SPLIT_SCREEN_PARTICLE_COUNT {
                        state.particle_counts[particle_index] = c_long(
                            (count as f32 - MAXIMUM_SPLIT_SCREEN_PARTICLE_COUNT as f32)
                                / self.inputs.local_player_count as f32
                                + MAXIMUM_SPLIT_SCREEN_PARTICLE_COUNT as f32,
                        ) as u8;
                    }
                }
                if state.flags & EFFECT_INVISIBLE == 0 {
                    self.generate_parts(state);
                }
            }
            iteration_index += 1;
        }
    }
}

/// `(long)value`, as x86 truncates it: a value out of range (or NaN) is
/// 0x80000000, where Rust's `as` would saturate.
fn c_long(value: f32) -> i32 {
    if (-2_147_483_648.0..2_147_483_648.0).contains(&value) { value as i32 } else { i32::MIN }
}

/// `sine` and `cosine`: the C library's double functions, as floats.
fn sine(a: f32) -> f32 {
    (a as f64).sin() as f32
}
fn cosine(a: f32) -> f32 {
    (a as f64).cos() as f32
}

/// `effect_update`: the effect's update over `dt` seconds, from `state`
/// (where it is left). Whether the effect is still there is the host's to
/// say (an effect deleted goes through its calls).
pub fn effect_update(
    definition: &EffectDefinition,
    inputs: &EffectInputs,
    locations: &EffectLocations,
    host: &impl EffectHost,
    state: &mut EffectState,
    dt: f32,
) {
    let update = Update { definition, inputs, locations, host };
    update.update(state, dt);
}
