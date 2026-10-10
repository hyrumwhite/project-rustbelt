//! Halo's projectiles (ROADMAP phase 7, objects and physics):
//! `projectile_update` and `projectile_collision` in OpenCE's
//! `source/items/projectiles.c`, ported as they are, in the same
//! single-precision arithmetic (double where the C does it in double), for
//! what moves a projectile: its timers, deceleration, gravity and range,
//! its flight through the structure and water, and how it answers what it
//! hits (each material's response: disappear, detonate, reflect,
//! overpenetrate or attach, with its noise and friction).
//!
//! It flies through objects too (through [`World`]'s objects), steers
//! toward its target when guided (with the wander the game's periodic
//! table gives it), overpenetrates or sticks to what it hits, sticking in
//! the hit node's space, and counts the needles already stuck there for a
//! super-combining explosion.
//!
//! What a projectile does to the world is left out: effects, sounds,
//! damage (the material type an object's damage reports back is an input,
//! [`ProjectileContext::damage_material_types`], until damage is ported),
//! breakable surfaces, the AI's notices, detonations, and the other
//! needles a super-combining hit resets.

use crate::{
    collision::{self, CollisionKind, World},
    layout, CacheFile, Result,
};

/// A material's response (`projectile_material_response_definition`).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct MaterialResponse {
    pub default_response: i16,
    pub potential_response: i16,
    pub potential_flags: u16,
    pub potential_skip_fraction: f32,
    pub potential_angle_lower_bound: f32,
    pub potential_angle_upper_bound: f32,
    pub potential_velocity_lower_bound: f32,
    pub potential_velocity_upper_bound: f32,
    pub angular_noise: f32,
    pub velocity_noise: f32,
    pub initial_friction: f32,
    pub parallel_friction: f32,
    pub perpendicular_friction: f32,
}

/// `default_projectile_material_response`: disappear.
pub const DEFAULT_MATERIAL_RESPONSE: MaterialResponse = MaterialResponse {
    default_response: RESPONSE_DISAPPEAR,
    potential_response: RESPONSE_DISAPPEAR,
    potential_flags: 0,
    potential_skip_fraction: 0.0,
    potential_angle_lower_bound: 0.0,
    potential_angle_upper_bound: 0.0,
    potential_velocity_lower_bound: 0.0,
    potential_velocity_upper_bound: 0.0,
    angular_noise: 0.0,
    velocity_noise: 0.0,
    initial_friction: 0.0,
    parallel_friction: 0.0,
    perpendicular_friction: 0.0,
};

pub const RESPONSE_DISAPPEAR: i16 = 0;
pub const RESPONSE_DETONATE: i16 = 1;
pub const RESPONSE_REFLECT: i16 = 2;
pub const RESPONSE_OVERPENETRATE: i16 = 3;
pub const RESPONSE_ATTACH: i16 = 4;

pub const ACTION_NONE: i16 = 0;
pub const ACTION_DETONATE: i16 = 1;
pub const ACTION_DISAPPEAR: i16 = 2;

/// `_projectile_datum` flags.
pub mod flags {
    pub const COLLIDED_ONCE: u32 = 1 << 2;
    pub const ATTACHED: u32 = 1 << 3;
    pub const STOPPED_AFTER_COLLISION: u32 = 1 << 4;
    pub const COUNTING_DOWN: u32 = 1 << 5;
    pub const ALREADY_SUPER_EXPLODED: u32 = 1 << 6;
    pub const WILL_SUPER_EXPLODE: u32 = 1 << 7;
}

/// The object flags a projectile reads and sets.
pub mod object_flags {
    pub const WHOLLY_UNDER_MEDIA: u32 = 1 << 4;
    pub const AT_REST: u32 = 1 << 5;
    pub const OUTSIDE_OF_MAP: u32 = 1 << 21;
}

/// `_projectile_definition` flags.
const DETONATION_MAX_TIME_IF_ATTACHED: u32 = 1 << 2;
const SUPER_COMBINING_EXPLOSION: u32 = 1 << 3;
/// `MAXIMUM_COMBINING_PROJECTILES`.
const MAXIMUM_COMBINING_PROJECTILES: usize = 6;

/// A projectile definition (`proj` tag), what moves it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ProjectileDefinition {
    pub flags: u32,
    pub detonation_timer_starts: i16,
    pub collision_radius: f32,
    pub timer_upper_bound: f32,
    pub minimum_velocity: f32,
    pub maximum_range: f32,
    pub air_gravity_scale: f32,
    pub air_damage_range: [f32; 2],
    pub water_gravity_scale: f32,
    pub water_damage_range: [f32; 2],
    pub initial_velocity: f32,
    pub final_velocity: f32,
    pub guided_angular_velocity: f32,
    /// Whether it has impact damage (which an object it hits takes).
    pub has_impact_damage: bool,
    pub material_responses: Vec<MaterialResponse>,
}

impl ProjectileDefinition {
    pub fn load(cache: &CacheFile, datum: u32) -> Result<Self> {
        use layout::{_projectile_definition as p, projectile_material_response_definition as r};
        let root = cache.tag_root(datum, crate::groups::PROJECTILE)?.add(layout::projectile_definition::PROJECTILE);
        let mut material_responses = Vec::new();
        for response in root.block(p::MATERIAL_RESPONSES, r::SIZEOF)?.iter() {
            material_responses.push(MaterialResponse {
                default_response: response.i16(r::DEFAULT_RESPONSE)?,
                potential_response: response.i16(r::POTENTIAL_RESPONSE)?,
                potential_flags: response.i16(r::POTENTIAL_FLAGS)? as u16,
                potential_skip_fraction: response.f32(r::POTENTIAL_SKIP_FRACTION)?,
                potential_angle_lower_bound: response.f32(r::POTENTIAL_ANGLE_LOWER_BOUND)?,
                potential_angle_upper_bound: response.f32(r::POTENTIAL_ANGLE_UPPER_BOUND)?,
                potential_velocity_lower_bound: response.f32(r::POTENTIAL_VELOCITY_LOWER_BOUND)?,
                potential_velocity_upper_bound: response.f32(r::POTENTIAL_VELOCITY_UPPER_BOUND)?,
                angular_noise: response.f32(r::ANGULAR_NOISE)?,
                velocity_noise: response.f32(r::VELOCITY_NOISE)?,
                initial_friction: response.f32(r::INITIAL_FRICTION)?,
                parallel_friction: response.f32(r::PARALLEL_FRICTION)?,
                perpendicular_friction: response.f32(r::PERPENDICULAR_FRICTION)?,
            });
        }
        Ok(ProjectileDefinition {
            flags: root.u32(p::FLAGS)?,
            detonation_timer_starts: root.i16(p::DETONATION_TIMER_STARTS)?,
            collision_radius: root.f32(p::COLLISION_RADIUS)?,
            timer_upper_bound: root.f32(p::TIMER_UPPER_BOUND)?,
            minimum_velocity: root.f32(p::MINIMUM_VELOCITY)?,
            maximum_range: root.f32(p::MAXIMUM_RANGE)?,
            air_gravity_scale: root.f32(p::AIR_GRAVITY_SCALE)?,
            air_damage_range: [root.f32(p::AIR_DAMAGE_RANGE_LOWER_BOUND)?, root.f32(p::AIR_DAMAGE_RANGE_UPPER_BOUND)?],
            water_gravity_scale: root.f32(p::WATER_GRAVITY_SCALE)?,
            water_damage_range: [root.f32(p::WATER_DAMAGE_RANGE_LOWER_BOUND)?, root.f32(p::WATER_DAMAGE_RANGE_UPPER_BOUND)?],
            initial_velocity: root.f32(p::INITIAL_VELOCITY)?,
            final_velocity: root.f32(p::FINAL_VELOCITY)?,
            guided_angular_velocity: root.f32(p::GUIDED_ANGULAR_VELOCITY)?,
            has_impact_damage: root.reference(p::IMPACT_DAMAGE)?.datum.is_some(),
            material_responses,
        })
    }
}

/// What `projectile_update` reads and changes of a projectile.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ProjectileState {
    pub position: [f32; 3],
    pub velocity: [f32; 3],
    pub flags: u32,
    pub object_flags: u32,
    pub action: i16,
    pub hit_material_type: i16,
    pub ignore_object_index: i32,
    pub target_object_index: i32,
    pub parent_object_index: i32,
    pub detonation_timer: f32,
    pub detonation_timer_delta: f32,
    pub arming_time: f32,
    pub arming_time_delta: f32,
    pub odometer: f32,
    pub deceleration_timer: f32,
    pub deceleration_timer_delta: f32,
    pub deceleration: f32,
    pub maximum_damage_distance: f32,
    pub location_leaf: i32,
    pub location_cluster: i16,
}

/// The game's global random numbers (`random_math.c`): its seed, and the
/// direction table `random_vector_in_cone3d` draws from.
pub struct Random<'a> {
    pub seed: u32,
    pub directions: &'a [[f32; 3]],
}

impl Random<'_> {
    /// `seed_random`.
    fn next(&mut self) -> u16 {
        self.seed = self.seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (self.seed >> 16) as u16
    }

    /// `real_random`.
    pub fn real(&mut self) -> f32 {
        self.seed = self.seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (self.seed >> 16) as f32 / 65535.0
    }

    /// `real_random_range`.
    pub fn range(&mut self, lower: f32, upper: f32) -> f32 {
        let random = self.real();
        lower + (upper - lower) * random
    }

    /// `seed_random_range`.
    fn index(&mut self, lower: i16, upper: i16) -> i16 {
        let span = (upper as i32 - lower as i32) as u32;
        lower.wrapping_add(((span.wrapping_mul(self.next() as u32)) >> 16) as i16)
    }

    /// `random_vector_in_cone3d`.
    fn vector_in_cone(&mut self, axis: [f32; 3], inner: f32, outer: f32) -> [f32; 3] {
        let mut result = axis;
        let index = self.index(0, self.directions.len() as i16);
        let direction = self.directions.get(index as usize).copied().unwrap_or([0.0; 3]);
        let mut rotation_axis = cross3(axis, direction);
        if normalize3(&mut rotation_axis) > 0.0 {
            let angle = self.range(inner, outer);
            result = rotate_vector_about_axis(result, rotation_axis, sine(angle), cosine(angle));
        }
        result
    }
}

/// `sine` and `cosine`: the C library's double functions, as floats.
fn sine(a: f32) -> f32 {
    (a as f64).sin() as f32
}
fn cosine(a: f32) -> f32 {
    (a as f64).cos() as f32
}

fn dot3(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross3(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

/// `magnitude3d`.
fn magnitude3(v: [f32; 3]) -> f32 {
    (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt()
}

/// `normalize3d`: the magnitude, 0 (leaving the vector) below epsilon.
fn normalize3(v: &mut [f32; 3]) -> f32 {
    let magnitude = magnitude3(*v);
    if !(0.0001 > magnitude.abs()) {
        let inverse = 1.0 / magnitude;
        *v = [v[0] * inverse, v[1] * inverse, v[2] * inverse];
        magnitude
    } else {
        0.0
    }
}

fn scale3(v: [f32; 3], c: f32) -> [f32; 3] {
    [v[0] * c, v[1] * c, v[2] * c]
}

/// `PIN(n, a, b)`, as the macro compares.
fn pin(n: f32, a: f32, b: f32) -> f32 {
    if n < a {
        a
    } else if n > b {
        b
    } else {
        n
    }
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

/// `angle_between_vectors3d`.
fn angle_between_vectors3d(a: [f32; 3], b: [f32; 3]) -> f32 {
    let aa_bb = (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]) * (b[0] * b[0] + b[1] * b[1] + b[2] * b[2]);
    if aa_bb == 0.0 {
        return 0.0;
    }
    let ab = dot3(a, b);
    let c = pin((ab / aa_bb * ab) + (ab / aa_bb * ab) - 1.0, -1.0, 1.0);
    let angle = ((c as f64).acos() as f32) * 0.5;
    if ab < 0.0 { std::f32::consts::PI - angle } else { angle }
}

/// The global gravity (world units per tick squared).
const GLOBAL_GRAVITY: f32 = crate::physics::GLOBAL_GRAVITY;
pub const MAXIMUM_PROJECTILE_COLLISIONS_PER_UPDATE: usize = 10;
/// `_collision_test_for_projectiles_flags` (as the game sets them: the
/// structure, water and objects, front faces, no invisible surfaces, the
/// hit point kept in a leaf).
const COLLISION_FLAGS: u32 = 0x0010_00e9;
/// `_collision_test_for_projectiles_fat_flags`: objects only.
const FAT_COLLISION_FLAGS: u32 = 0x89;

/// What an update did beyond the state: how many collisions it had, how
/// many of them did impact damage to an object, the object it stuck to,
/// how many other needles a super-combining hit set off, and how many
/// steps it steered.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct UpdateSummary {
    pub collisions: usize,
    pub damaged_objects: usize,
    pub attached_to: Option<u32>,
    pub combining_projectiles: usize,
    pub steered: usize,
}

/// An object as a projectile's hit reads it beyond the broad phase.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ProjectileObject {
    pub index: u32,
    pub definition_index: u32,
    pub kind: i16,
    /// Its parent (`u32::MAX` for none).
    pub parent: u32,
    /// A projectile's own flags.
    pub projectile_flags: u32,
}

/// What a guided projectile's steering reads in a step: its bounding
/// sphere's centre, its target's and the target's centre of mass
/// (`unit_get_center_of_mass`), the difficulty's guidance scale (for a
/// player's unit; 1 otherwise), and the game time.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Guidance {
    pub projectile_center: [f32; 3],
    pub target_center: [f32; 3],
    pub target_center_of_mass: [f32; 3],
    pub guidance_scale: f32,
    pub game_time: i32,
}

/// What a projectile update reads of the rest of the game.
pub struct ProjectileContext<'a> {
    /// The projectile's datum index and definition.
    pub projectile_index: u32,
    pub definition_index: u32,
    /// The random seeds each collision response begins with (the effects a
    /// response creates draw from the same numbers).
    pub collision_seeds: &'a [u32],
    /// What each collision's impact damage reported (`object_cause_damage`'s
    /// material type; -2 for no damage done), and the random seed it left
    /// (the damage draws from the game's random numbers too).
    pub damage_material_types: &'a [i16],
    pub damage_seeds: &'a [u32],
    /// The objects the broad phase may hit.
    pub objects: &'a [ProjectileObject],
    /// Each guided step's inputs.
    pub guidance: &'a [Guidance],
    /// The game's wander table (`_periodic_function_wander`).
    pub wander_table: Option<&'a [u8; 1024]>,
}

/// Why an update couldn't be replayed: an input the game didn't record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MissingInput {
    Guidance,
    Damage,
    Object,
}

/// `periodic_function_evaluate` of a table (not a sliding function).
fn periodic_function_evaluate(table: &[u8; 1024], time: f32) -> f32 {
    let time = time * 25.6;
    let fraction = (time as f64 % 1.0) as f32;
    // (fast_ftol: rounded to nearest)
    let mut index = ((time - fraction) as f64).round_ties_even() as i32;
    index &= 1023;
    let first_value = table[index as usize] as f32 * (1.0 / 255.0);
    index = (index + 1) & 1023;
    let second_value = table[index as usize] as f32 * (1.0 / 255.0);
    (1.0 - fraction) * first_value + second_value * fraction
}

/// `projectile_update`'s guided steering: the velocity turned toward the
/// target's centre of mass (wandering about it until close), by the
/// guided angular velocity.
fn steer(definition: &ProjectileDefinition, state: &ProjectileState, projectile_index: u32, guidance: &Guidance, table: &[u8; 1024]) -> [f32; 3] {
    use std::f32::consts::PI;
    let mut new_velocity = state.velocity;
    let mut angular_velocity = definition.guided_angular_velocity * (1.0 / 30.0);
    angular_velocity *= guidance.guidance_scale;
    let d = [
        guidance.target_center[0] - guidance.projectile_center[0],
        guidance.target_center[1] - guidance.projectile_center[1],
        guidance.target_center[2] - guidance.projectile_center[2],
    ];
    let target_distance = magnitude3(d);
    let wander_scale = if target_distance > 10.0 {
        1.0
    } else if target_distance > 2.0 {
        pin((target_distance - 2.0) * 0.125, 0.0, 1.0)
    } else {
        0.0
    };
    let identifier = (projectile_index >> 16) as i32;
    let time = |k: i32| ((guidance.game_time.wrapping_add(k.wrapping_mul(identifier))) & 0xffff) as f32 * (1.0 / 90.0);
    // (set_real_euler_angles2d takes the pitch first)
    let pitch = PI - periodic_function_evaluate(table, time(3)) * (PI / 2.0);
    let yaw = periodic_function_evaluate(table, time(7)) * (PI * 2.0);
    // (vector3d_from_euler_angles2d)
    let cosine_pitch = cosine(pitch);
    let wander_direction = [cosine(yaw) * cosine_pitch, sine(yaw) * cosine_pitch, sine(pitch)];
    let c = guidance.target_center_of_mass;
    let target_point = [
        wander_direction[0] * wander_scale + c[0],
        wander_direction[1] * wander_scale + c[1],
        wander_direction[2] * wander_scale + c[2],
    ];
    let target_vector = [target_point[0] - state.position[0], target_point[1] - state.position[1], target_point[2] - state.position[2]];
    let mut rotation_axis = cross3(state.velocity, target_vector);
    if dot3(target_vector, state.velocity) > 0.0 && normalize3(&mut rotation_axis) > 0.0 {
        new_velocity = rotate_vector_about_axis(new_velocity, rotation_axis, sine(angular_velocity), cosine(angular_velocity));
    }
    new_velocity
}

fn set_action(state: &mut ProjectileState, action: i16) {
    if action > state.action {
        state.action = action;
    }
}

/// `projectile_calculate_deceleration_from_distances`.
fn deceleration_from_distances(definition: &ProjectileDefinition, minimum: f32, maximum: f32) -> f32 {
    let distance_delta = maximum - minimum;
    if definition.initial_velocity != definition.final_velocity && distance_delta != 0.0 {
        (definition.initial_velocity * definition.initial_velocity - definition.final_velocity * definition.final_velocity)
            / (2.0 * distance_delta)
    } else {
        0.0
    }
}

/// `projectile_calculate_deceleration` (with the game's air branch, which
/// takes the water damage range's upper bound).
fn calculate_deceleration(definition: &ProjectileDefinition, state: &mut ProjectileState) {
    let under_water = state.object_flags & object_flags::WHOLLY_UNDER_MEDIA != 0;
    let range = if under_water { definition.water_damage_range } else { definition.air_damage_range };
    state.deceleration = deceleration_from_distances(definition, range[0], range[1]);
    state.maximum_damage_distance = definition.water_damage_range[1];
    if range[0] > 0.0 {
        state.deceleration_timer_delta = range[0] / definition.initial_velocity;
        return;
    }
    state.deceleration_timer = 1.0;
    state.deceleration_timer_delta = 0.0;
}

/// One `projectile_update`; `random.seed` is the seed it begins with.
/// `Err` for an input the game didn't record.
pub fn projectile_update(
    world: &World<'_>,
    definition: &ProjectileDefinition,
    state: &mut ProjectileState,
    random: &mut Random<'_>,
    context: &ProjectileContext<'_>,
) -> std::result::Result<UpdateSummary, MissingInput> {
    let mut summary = UpdateSummary::default();
    let mut time_remaining = 1.0f32;
    let mut collision_count = 0usize;

    state.arming_time += state.arming_time_delta;
    state.deceleration_timer += state.deceleration_timer_delta;

    // (the game's after-first-bounce case falls through to when-at-rest's)
    let mut detonation_timer_running = match definition.detonation_timer_starts {
        1 | 2 => state.flags & flags::STOPPED_AFTER_COLLISION != 0,
        _ => true,
    };
    if state.flags & (flags::COUNTING_DOWN | flags::ATTACHED) != 0 {
        detonation_timer_running = true;
    }
    if detonation_timer_running {
        state.flags |= flags::COUNTING_DOWN;
        state.detonation_timer += state.detonation_timer_delta;
        if state.detonation_timer >= 1.0 {
            set_action(state, ACTION_DETONATE);
        }
    }

    while time_remaining > 0.0
        && (state.action == ACTION_NONE
            || (state.action == ACTION_DETONATE && state.arming_time_delta != 0.0 && state.arming_time < 1.0))
        && state.flags & flags::ATTACHED == 0
        && state.object_flags & object_flags::AT_REST == 0
        && state.parent_object_index == -1
    {
        let speed = magnitude3(state.velocity);
        let mut new_velocity = state.velocity;
        if state.target_object_index != -1 && definition.guided_angular_velocity > 0.0 {
            let (Some(guidance), Some(table)) = (context.guidance.get(summary.steered), context.wander_table) else {
                return Err(MissingInput::Guidance);
            };
            new_velocity = steer(definition, state, context.projectile_index, guidance, table);
            summary.steered += 1;
        }
        let mut final_speed = speed;
        let mut average_speed = speed;
        let mut average_velocity = state.velocity;

        if state.deceleration_timer >= 1.0 {
            if speed > definition.final_velocity && state.deceleration != 0.0 {
                final_speed = speed - state.deceleration * time_remaining;
                if final_speed <= definition.final_velocity {
                    let fraction = (speed - definition.final_velocity) / (state.deceleration * time_remaining);
                    final_speed = definition.final_velocity * 0.99;
                    average_speed = (final_speed + speed) * fraction * 0.5 + (1.0 - fraction) * definition.final_velocity;
                    new_velocity = scale3(new_velocity, final_speed / speed);
                    for i in 0..3 {
                        average_velocity[i] =
                            (new_velocity[i] + state.velocity[i]) * fraction * 0.5 + (1.0 - fraction) * new_velocity[i];
                    }
                } else {
                    average_speed = speed - state.deceleration * time_remaining * 0.5;
                    new_velocity = scale3(new_velocity, final_speed / speed);
                    for i in 0..3 {
                        average_velocity[i] = (new_velocity[i] + state.velocity[i]) * 0.5;
                    }
                }
            } else if definition.maximum_range == 0.0
                && definition.timer_upper_bound == 0.0
                && definition.minimum_velocity <= definition.final_velocity
                && (state.deceleration != 0.0 || state.odometer >= state.maximum_damage_distance)
            {
                set_action(state, ACTION_DISAPPEAR);
            } else if speed < definition.final_velocity && speed > 0.0 {
                new_velocity = scale3(new_velocity, definition.final_velocity / speed * 0.99);
            }
        }

        let gravity_acceleration = if state.object_flags & object_flags::WHOLLY_UNDER_MEDIA != 0 {
            GLOBAL_GRAVITY * definition.water_gravity_scale
        } else {
            GLOBAL_GRAVITY * definition.air_gravity_scale
        };
        new_velocity[2] -= gravity_acceleration * time_remaining;
        average_velocity[2] -= gravity_acceleration * time_remaining * 0.5;

        let distance_fraction = if definition.maximum_range != 0.0
            && average_speed * time_remaining + state.odometer > definition.maximum_range
        {
            let fraction = if average_speed != 0.0 {
                (definition.maximum_range - state.odometer) / average_speed * time_remaining
            } else {
                0.0
            };
            set_action(state, ACTION_DETONATE);
            fraction
        } else {
            1.0
        };

        let step = distance_fraction * time_remaining;
        let mut new_position = [
            average_velocity[0] * step + state.position[0],
            average_velocity[1] * step + state.position[1],
            average_velocity[2] * step + state.position[2],
        ];

        let mut moved = false;
        let mut location = (state.location_leaf, state.location_cluster);
        if collision_count != MAXIMUM_PROJECTILE_COLLISIONS_PER_UPDATE && state.action != ACTION_DISAPPEAR {
            moved = true;
            let collision = collision_test_line(world, definition, state, new_position);
            location = (collision.location_leaf, collision.location_cluster);
            if collision.kind.is_some() {
                time_remaining = 1.0 - collision.t;
                new_velocity[2] += gravity_acceleration * time_remaining;
                if final_speed != 0.0 {
                    let restored = time_remaining * state.deceleration + final_speed;
                    let restored = if restored < speed { restored } else { speed };
                    new_velocity = scale3(new_velocity, restored / final_speed);
                }
                if collision.plane[2] > 0.3 {
                    state.flags |= flags::COLLIDED_ONCE;
                }
                state.ignore_object_index = -1;
                if let Some(&seed) = context.collision_seeds.get(collision_count) {
                    random.seed = seed;
                }
                projectile_collision(
                    world,
                    definition,
                    state,
                    &collision,
                    &mut new_position,
                    &mut new_velocity,
                    random,
                    context,
                    collision_count,
                    &mut summary,
                )?;
                collision_count += 1;
                if state.flags & flags::ATTACHED != 0 {
                    moved = false;
                }
            } else {
                time_remaining = 0.0;
            }
        } else {
            if collision_count == MAXIMUM_PROJECTILE_COLLISIONS_PER_UPDATE {
                set_action(state, ACTION_DETONATE);
            }
            time_remaining = 0.0;
        }

        if moved {
            let movement = [
                new_position[0] - state.position[0],
                new_position[1] - state.position[1],
                new_position[2] - state.position[2],
            ];
            state.odometer += magnitude3(movement);
            translate(state, new_position, location);
            state.velocity = new_velocity;
        }
    }
    summary.collisions = collision_count;
    Ok(summary)
}

/// `projectile_collision_test_line`: the line from the projectile to its
/// next position, and for a fat projectile two lines either side of it
/// (against objects only).
fn collision_test_line(
    world: &World<'_>,
    definition: &ProjectileDefinition,
    state: &ProjectileState,
    new_position: [f32; 3],
) -> collision::CollisionResult {
    let p0 = state.position;
    let vector = [new_position[0] - p0[0], new_position[1] - p0[1], new_position[2] - p0[2]];
    let ignore = state.ignore_object_index as u32;
    let result = world.test(COLLISION_FLAGS, p0, vector, ignore);
    if result.kind.is_some() || definition.collision_radius < 0.0001 {
        return result;
    }
    let mut left = cross3([0.0, 0.0, 1.0], vector);
    if normalize3(&mut left) == 0.0 {
        left = [0.0, 1.0, 0.0];
    }
    let r = definition.collision_radius;
    let mut fat = result;
    for side in [r, -r] {
        let a = [left[0] * side + p0[0], left[1] * side + p0[1], left[2] * side + p0[2]];
        let b = [left[0] * side + new_position[0], left[1] * side + new_position[1], left[2] * side + new_position[2]];
        fat = world.test(FAT_COLLISION_FLAGS, a, [b[0] - a[0], b[1] - a[1], b[2] - a[2]], ignore);
        if fat.kind.is_some() {
            break;
        }
    }
    // (on a miss, the right line's result: its location, which the
    // projectile then moves to)
    fat
}

/// `object_translate` for a projectile (no parent): the new position, and
/// the location if it is in a cluster, else the object is outside the map.
fn translate(state: &mut ProjectileState, position: [f32; 3], location: (i32, i16)) {
    state.position = position;
    if location.1 != -1 {
        (state.location_leaf, state.location_cluster) = location;
        state.object_flags &= !object_flags::OUTSIDE_OF_MAP;
    } else {
        state.object_flags |= object_flags::OUTSIDE_OF_MAP;
    }
}

/// `projectile_collision`: how the projectile answers what it hit.
#[allow(clippy::too_many_arguments)]
fn projectile_collision(
    world: &World<'_>,
    definition: &ProjectileDefinition,
    state: &mut ProjectileState,
    collision: &collision::CollisionResult,
    new_position: &mut [f32; 3],
    new_velocity: &mut [f32; 3],
    random: &mut Random<'_>,
    context: &ProjectileContext<'_>,
    collision_index: usize,
    summary: &mut UpdateSummary,
) -> std::result::Result<(), MissingInput> {
    let mut material_type = collision.material_type;
    let n = [collision.plane[0], collision.plane[1], collision.plane[2]];
    let hit_object = if collision.kind == Some(CollisionKind::Object) { collision.object } else { None };
    // (the object's impact damage, done by the game: the material type it
    // reports back)
    if hit_object.is_some() && definition.has_impact_damage {
        match context.damage_material_types.get(collision_index) {
            Some(&-2) | None => return Err(MissingInput::Damage),
            Some(&damage_material_type) => {
                if damage_material_type != -1 {
                    material_type = damage_material_type;
                }
                if let Some(&seed) = context.damage_seeds.get(collision_index) {
                    random.seed = seed;
                }
            }
        }
        summary.damaged_objects += 1;
    }
    let object_info = |index: u32| context.objects.iter().find(|o| o.index == index);
    state.hit_material_type = material_type;
    let response_definition = if material_type >= 0 && (material_type as usize) < definition.material_responses.len() {
        definition.material_responses[material_type as usize]
    } else {
        DEFAULT_MATERIAL_RESPONSE
    };
    let m = &response_definition;
    let impact_velocity = -dot3(*new_velocity, n) + random.range(-m.velocity_noise, 0.0);
    let impact_angle =
        angle_between_vectors3d(*new_velocity, n) - std::f32::consts::PI / 2.0 + random.range(-m.angular_noise, m.angular_noise);
    let only_against_units = m.potential_flags & 1 != 0;
    let mut response = if m.potential_response != 0
        && (m.potential_angle_upper_bound == 0.0
            || (impact_angle >= m.potential_angle_lower_bound && impact_angle <= m.potential_angle_upper_bound))
        && (m.potential_velocity_upper_bound == 0.0
            || (impact_velocity >= m.potential_velocity_lower_bound && impact_velocity <= m.potential_velocity_upper_bound))
        && (!only_against_units || hit_object.and_then(object_info).is_some_and(|o| o.kind == 0 || o.kind == 1))
        && random.real() >= m.potential_skip_fraction
    {
        m.potential_response
    } else {
        m.default_response
    };

    *new_position = collision.point;

    if response == RESPONSE_OVERPENETRATE {
        if collision.kind == Some(CollisionKind::Media) {
            state.object_flags ^= object_flags::WHOLLY_UNDER_MEDIA;
            calculate_deceleration(definition, state);
            new_position[0] -= n[0] * 0.001;
            new_position[1] -= n[1] * 0.001;
            new_position[2] -= n[2] * 0.001;
        } else if collision.kind == Some(CollisionKind::Object) {
            *new_velocity = scale3(*new_velocity, 1.0 - m.initial_friction);
            state.ignore_object_index = collision.object.map_or(-1, |o| o as i32);
        } else if definition.timer_upper_bound != 0.0 {
            state.flags |= flags::COLLIDED_ONCE | flags::STOPPED_AFTER_COLLISION;
            response = RESPONSE_ATTACH;
        } else {
            response = RESPONSE_DETONATE;
        }
    }

    if response == RESPONSE_REFLECT {
        // (component_vectors_from_direction3d)
        let dd = dot3(n, n);
        let (parallel, perpendicular) = if dd != 0.0 {
            let vd_dd = dot3(*new_velocity, n) / dd;
            let parallel = scale3(n, vd_dd);
            (parallel, [new_velocity[0] - parallel[0], new_velocity[1] - parallel[1], new_velocity[2] - parallel[2]])
        } else {
            ([0.0; 3], *new_velocity)
        };
        let keep_perpendicular = 1.0 - m.perpendicular_friction;
        let keep_parallel = 1.0 - m.parallel_friction;
        for i in 0..3 {
            new_velocity[i] = keep_perpendicular * perpendicular[i] - keep_parallel * parallel[i];
        }
    } else if response != RESPONSE_OVERPENETRATE {
        *new_velocity = [0.0; 3];
    }

    if m.angular_noise != 0.0 {
        *new_velocity = random.vector_in_cone(*new_velocity, 0.0, m.angular_noise);
    }
    if m.velocity_noise != 0.0 {
        let magnitude = normalize3(new_velocity);
        if magnitude != 0.0 {
            *new_velocity = scale3(*new_velocity, random.range(-m.velocity_noise, m.velocity_noise) + magnitude);
        }
    }

    let speed_squared = dot3(*new_velocity, *new_velocity);
    if response != RESPONSE_ATTACH && speed_squared < definition.minimum_velocity * definition.minimum_velocity {
        set_action(state, ACTION_DETONATE);
    }
    if speed_squared < 0.0001 {
        state.flags |= flags::STOPPED_AFTER_COLLISION;
        if n[2] > 0.3 {
            state.object_flags |= object_flags::AT_REST;
            if definition.timer_upper_bound == 0.0 {
                set_action(state, ACTION_DETONATE);
            }
        }
    }

    match response {
        RESPONSE_DISAPPEAR => set_action(state, ACTION_DISAPPEAR),
        RESPONSE_DETONATE => set_action(state, ACTION_DETONATE),
        RESPONSE_ATTACH => {
            // (the needles already stuck in it: enough of them, and this
            // one sets off a super-combining explosion)
            if let Some(object) = hit_object
                && definition.flags & SUPER_COMBINING_EXPLOSION != 0
            {
                let mut combining_projectile_count = 0usize;
                for child in context.objects.iter().filter(|o| o.parent == object) {
                    if child.definition_index == context.definition_index && child.projectile_flags & flags::ALREADY_SUPER_EXPLODED == 0 {
                        // (their arming and detonation timers restart: the game's)
                        combining_projectile_count += 1;
                    }
                    if combining_projectile_count >= MAXIMUM_COMBINING_PROJECTILES {
                        state.flags |= flags::WILL_SUPER_EXPLODE;
                        break;
                    }
                }
                summary.combining_projectiles += combining_projectile_count;
            }
            state.velocity = [0.0; 3];
            state.flags |= flags::ATTACHED;
            state.object_flags |= object_flags::AT_REST;
            translate(state, *new_position, (collision.location_leaf, collision.location_cluster));
            // (object_attach_to_node: into the hit node's space, the
            // location's cluster none, as a child's)
            if let Some(object) = hit_object {
                let Some(parent) = world.objects.iter().find(|o| o.index == object) else { return Err(MissingInput::Object) };
                let Some(node_matrix) = parent.matrices.get(collision.node.max(0) as usize) else { return Err(MissingInput::Object) };
                state.position = collision::transform_point(&collision::matrix_inverse(node_matrix), state.position);
                state.parent_object_index = object as i32;
                state.location_cluster = -1;
                summary.attached_to = Some(object);
            }
            if definition.flags & DETONATION_MAX_TIME_IF_ATTACHED != 0 {
                let detonation_ticks = definition.timer_upper_bound * 30.0;
                if detonation_ticks >= 1.0 {
                    state.detonation_timer_delta = 1.0 / detonation_ticks;
                }
            }
        }
        _ => {}
    }
    Ok(())
}
