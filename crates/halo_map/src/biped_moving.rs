//! Halo's biped movement control (ROADMAP phase 7, objects and physics):
//! `biped_update_moving` in OpenCE's `source/units/bipeds.c`, with the
//! pieces it calls (`biped_get_physics_pill`, `biped_snap_facing`,
//! `biped_start_landing`, `biped_update_jumping`, `biped_jump`,
//! `biped_bumped_object`), ported as they are, in the same single-precision
//! arithmetic.
//!
//! Each tick it turns a biped's throttle, crouch, animation and definition
//! into what the physics wants ([`BipedPhysics`]): the speed and direction
//! it means to move (from the animation's root motion for most bipeds, from
//! the throttle and the globals' player speeds for players, from its flying
//! speeds for flying ones), how fast it may change, the pill it moves.
//! After the physics it moves the biped, keeps what it stands on, starts a
//! landing, jumps, and counts the bumps.
//!
//! What the rest of the game answers (the AI's jump aim and leaping, the
//! difficulty, cinematics, the aiming vector) comes in as [`MovingInputs`];
//! damage (falling, melee impacts), effects and the AI's notices are not
//! ported.

use crate::biped::{BipedPhysics, in_flags, out_flags};
use crate::collision::{CollisionBsp, StructureClusters};
use crate::features::{FeatureObject, cross3, dot3, magnitude_squared3, magnitude3, normalize3, scale3};
use crate::{CacheFile, Result, animation::FrameInfo, layout};

/// `_biped_definition` flags.
pub mod biped_flags {
    pub const USES_PLAYER_PHYSICS: u32 = 1 << 1;
    pub const FLYING: u32 = 1 << 2;
    pub const PILL_CENTERED_AT_ORIGIN: u32 = 1 << 3;
    pub const SPHERICAL: u32 = 1 << 4;
    pub const PASSES_THROUGH_BIPEDS: u32 = 1 << 5;
    pub const CLIMBS_ANYTHING: u32 = 1 << 6;
    pub const RANDOM_SPEED_INCREASE: u32 = 1 << 11;
    pub const USES_OLD_PLAYER_PHYSICS: u32 = 1 << 12;
}

/// `_unit_definition` flags.
const UNIT_SIMPLE_CREATURE: u32 = 1 << 11;

/// `_biped_datum` flags.
pub mod state_flags {
    pub const AIRBORNE: u32 = 1 << 0;
    pub const SLIPPING: u32 = 1 << 1;
    pub const ABSOLUTE_MOVEMENT: u32 = 1 << 2;
    pub const NO_COLLISION: u32 = 1 << 3;
    pub const MOVEMENT_PASSES_THROUGH_BIPEDS: u32 = 1 << 4;
}

/// The object flags movement reads and sets.
pub mod object_flags {
    pub const ON_GROUND: u32 = 1 << 1;
    pub const AT_REST: u32 = 1 << 5;
    pub const OUTSIDE_OF_MAP: u32 = 1 << 21;
    pub const MOVIE_STAR: u32 = 1 << 22;
}

/// `_unit_control` flags.
pub mod control_flags {
    pub const CROUCH_MODIFIER: u32 = 1 << 0;
    pub const JUMP: u32 = 1 << 1;
    pub const EXACT_FACING: u32 = 1 << 5;
    pub const LOOK_DONT_TURN: u32 = 1 << 8;
}

/// `_unit_state`s movement uses.
pub mod unit_state {
    pub const IDLE: i8 = 0;
    pub const TURN_LEFT: i8 = 2;
    pub const TURN_RIGHT: i8 = 3;
    pub const MOVE_FRONT: i8 = 4;
    pub const STUNNED_MOVE_FRONT: i8 = 8;
    pub const USER_ANIMATION: i8 = 28;
}

const BASE_SEAT_ALERT: i8 = 1;
const BASE_SEAT_CROUCH: i8 = 3;
const NONE: i32 = -1;
const EPSILON: f32 = 0.0001;
const TICKS_PER_SECOND: f32 = 30.0;
const UP: [f32; 3] = [0.0, 0.0, 1.0];
const FORWARD: [f32; 3] = [1.0, 0.0, 0.0];
const LEFT: [f32; 3] = [0.0, 1.0, 0.0];
/// `BIPED_CLIMBING_SNAP_ANGLE`: 10 degrees, as a float.
const CLIMBING_SNAP_ANGLE: f32 = (10.0 * std::f64::consts::PI / 180.0) as f32;

/// A biped definition (`bipd` tag), what movement reads.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct BipedDefinition {
    pub unit_flags: u32,
    pub stunned_movement_threshold: f32,
    pub flags: u32,
    pub flying_velocity: f32,
    pub flying_sidestep_velocity: f32,
    pub flying_acceleration: f32,
    pub flying_deceleration: f32,
    pub flying_crouch_velocity_modifier: f32,
    pub downhill_velocity_scale: f32,
    pub uphill_velocity_scale: f32,
    pub jump_velocity: f32,
    pub maximum_soft_landing_time: f32,
    pub maximum_hard_landing_time: f32,
    pub minimum_soft_landing_velocity: f32,
    pub minimum_hard_landing_velocity: f32,
    pub maximum_hard_landing_velocity: f32,
    pub collision_height_standing: f32,
    pub collision_height_crouching: f32,
    pub collision_radius: f32,
    pub runtime_crouch_transition_velocity: f32,
    pub runtime_minimum_normal_k: f32,
    pub runtime_downhill_k0: f32,
    pub runtime_downhill_k1: f32,
    pub runtime_uphill_k0: f32,
    pub runtime_uphill_k1: f32,
}

impl BipedDefinition {
    pub fn load(cache: &CacheFile, datum: u32) -> Result<Self> {
        use layout::{_biped_definition as b, _unit_definition as u};
        let root = cache.tag_root(datum, crate::groups::BIPED)?;
        let unit = root.add(layout::biped_definition::UNIT);
        let biped = root.add(layout::biped_definition::BIPED);
        Ok(BipedDefinition {
            unit_flags: unit.u32(u::FLAGS)?,
            stunned_movement_threshold: unit.f32(u::STUNNED_MOVEMENT_THRESHOLD)?,
            flags: biped.u32(b::FLAGS)?,
            flying_velocity: biped.f32(b::FLYING_VELOCITY)?,
            flying_sidestep_velocity: biped.f32(b::FLYING_SIDESTEP_VELOCITY)?,
            flying_acceleration: biped.f32(b::FLYING_ACCELERATION)?,
            flying_deceleration: biped.f32(b::FLYING_DECELERATION)?,
            flying_crouch_velocity_modifier: biped.f32(b::FLYING_CROUCH_VELOCITY_MODIFIER)?,
            downhill_velocity_scale: biped.f32(b::DOWNHILL_VELOCITY_SCALE)?,
            uphill_velocity_scale: biped.f32(b::UPHILL_VELOCITY_SCALE)?,
            jump_velocity: biped.f32(b::JUMP_VELOCITY)?,
            maximum_soft_landing_time: biped.f32(b::MAXIMUM_SOFT_LANDING_TIME)?,
            maximum_hard_landing_time: biped.f32(b::MAXIMUM_HARD_LANDING_TIME)?,
            minimum_soft_landing_velocity: biped.f32(b::MINIMUM_SOFT_LANDING_VELOCITY)?,
            minimum_hard_landing_velocity: biped.f32(b::MINIMUM_HARD_LANDING_VELOCITY)?,
            maximum_hard_landing_velocity: biped.f32(b::MAXIMUM_HARD_LANDING_VELOCITY)?,
            collision_height_standing: biped.f32(b::COLLISION_HEIGHT_STANDING)?,
            collision_height_crouching: biped.f32(b::COLLISION_HEIGHT_CROUCHING)?,
            collision_radius: biped.f32(b::COLLISION_RADIUS)?,
            runtime_crouch_transition_velocity: biped.f32(b::RUNTIME_CROUCH_TRANSITION_VELOCITY)?,
            runtime_minimum_normal_k: biped.f32(b::RUNTIME_MINIMUM_NORMAL_K)?,
            runtime_downhill_k0: biped.f32(b::RUNTIME_DOWNHILL_K0)?,
            runtime_downhill_k1: biped.f32(b::RUNTIME_DOWNHILL_K1)?,
            runtime_uphill_k0: biped.f32(b::RUNTIME_UPHILL_K0)?,
            runtime_uphill_k1: biped.f32(b::RUNTIME_UPHILL_K1)?,
        })
    }
}

/// The globals' player movement (`game_globals_player_information`).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct PlayerInformation {
    pub walking_speed: f32,
    pub double_speed_multiplier: f32,
    pub run_forward_speed: f32,
    pub run_backward_speed: f32,
    pub run_sideways_speed: f32,
    pub run_acceleration: f32,
    pub sneak_forward_speed: f32,
    pub sneak_backward_speed: f32,
    pub sneak_sideways_speed: f32,
    pub sneak_acceleration: f32,
    pub airborne_acceleration: f32,
    pub stun_movement_penalty: f32,
    pub stun_turning_penalty: f32,
}

impl PlayerInformation {
    /// The map's globals' (`globals\globals`) first player information.
    pub fn load(cache: &CacheFile) -> Result<Self> {
        use layout::game_globals_player_information as p;
        let globals = cache
            .find_tag(crate::groups::GLOBALS, "globals\\globals")
            .ok_or_else(|| crate::Error::BadTag("no globals\\globals".into()))?;
        let root = cache.tag_root(globals.datum, crate::groups::GLOBALS)?;
        let info = root
            .block(layout::game_globals::PLAYER_INFORMATION, p::SIZEOF)?
            .get(0)
            .ok_or_else(|| crate::Error::BadTag("the globals have no player information".into()))?;
        Ok(PlayerInformation {
            walking_speed: info.f32(p::WALKING_SPEED)?,
            double_speed_multiplier: info.f32(p::DOUBLE_SPEED_MULTIPLIER)?,
            run_forward_speed: info.f32(p::RUN_FORWARD_SPEED)?,
            run_backward_speed: info.f32(p::RUN_BACKWARD_SPEED)?,
            run_sideways_speed: info.f32(p::RUN_SIDEWAYS_SPEED)?,
            run_acceleration: info.f32(p::RUN_ACCELERATION)?,
            sneak_forward_speed: info.f32(p::SNEAK_FORWARD_SPEED)?,
            sneak_backward_speed: info.f32(p::SNEAK_BACKWARD_SPEED)?,
            sneak_sideways_speed: info.f32(p::SNEAK_SIDEWAYS_SPEED)?,
            sneak_acceleration: info.f32(p::SNEAK_ACCELERATION)?,
            airborne_acceleration: info.f32(p::AIRBORNE_ACCELERATION)?,
            stun_movement_penalty: info.f32(p::STUN_MOVEMENT_PENALTY)?,
            stun_turning_penalty: info.f32(p::STUN_TURNING_PENALTY)?,
        })
    }
}

/// What `biped_update_moving` changes of a biped (and of the animation
/// update it is given), laid out as the bridge shares it.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct MovingState {
    pub position: [f32; 3],
    pub forward: [f32; 3],
    pub up: [f32; 3],
    pub translational_velocity: [f32; 3],
    pub angular_velocity: [f32; 3],
    pub object_flags: u32,
    pub location_leaf: i32,
    pub location_cluster: i16,
    pub landing: i16,
    pub biped_flags: u32,
    pub landing_recovery_counter: i8,
    pub landing_recovery_time: i8,
    pub elevator_ticks: i8,
    pub bump_ticks: i8,
    pub jump_recovery_timer: i8,
    pub airborne_ticks: i8,
    pub animation_state_desired: i8,
    pub animation_crouching: i8,
    pub elevator_object_index: i32,
    pub support_surface_index: i32,
    pub pathfinding_surface_index: i32,
    pub pathfinding_point: [f32; 3],
    pub bump_object_index: i32,
    pub crouch: f32,
    pub bank: f32,
    pub ground_plane: [f32; 4],
}

const _: () = assert!(std::mem::size_of::<MovingState>() == 136);

/// What `biped_update_moving` reads of the biped that it doesn't change,
/// and what it asks of the rest of the game.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct MovingInputs {
    pub biped_index: u32,
    pub dead: bool,
    pub ignore_translation: bool,
    pub suspended: bool,
    pub control_flags: u32,
    pub unit_animation_state: i8,
    pub base_seat_index: i8,
    pub aiming_speed: i8,
    /// It has an actor (or a swarm actor).
    pub actor: bool,
    pub player_index: i32,
    pub throttle: [f32; 3],
    pub body_stun: f32,
    pub recent_body_damage: f32,
    pub desired_facing_vector: [f32; 3],
    /// `unit_get_aiming_vector` (its forward for a simple creature).
    pub aiming_vector: [f32; 3],
    /// Its bounding sphere's centre, where moving it looks for its location.
    pub bounding_center: [f32; 3],
    /// Its animation's frame's root motion, if it has an animation.
    pub frame_info: Option<FrameInfo>,
    pub difficulty_infection_forms: f32,
    pub speed_multiplier: f32,
    pub cinematic: bool,
    pub double_speed: bool,
    pub coop_player_collisions: bool,
    pub actor_leaping: bool,
    pub recorded_animation: bool,
    /// What its actor's jump aim answers (whether it jumps, and how fast).
    pub actor_jump: Option<(bool, [f32; 3])>,
}

/// The physics' world for this update: the structure, and the objects its
/// broad phase visits each time it gathers features.
pub struct PhysicsWorld<'a> {
    pub bsp: &'a CollisionBsp,
    pub clusters: &'a StructureClusters,
    pub objects: &'a [Vec<FeatureObject>],
}

/// What an update did besides the state: the physics it asked for (`None`
/// when it didn't move), and whether it aborted the unit's animation.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct MovingOutcome {
    pub physics: Option<BipedPhysics>,
    pub aborted_animation: bool,
}

/// `biped_get_physics_pill`: the base of the biped's pill, its height
/// (between the two spheres) and width.
pub fn physics_pill(definition: &BipedDefinition, state: &MovingState, player: bool) -> ([f32; 3], f32, f32) {
    let mut base = state.position;
    if definition.flags & biped_flags::PILL_CENTERED_AT_ORIGIN == 0 {
        base[2] += definition.collision_radius;
    }
    let height = if definition.flags & biped_flags::SPHERICAL == 0 && (player || state.object_flags & object_flags::MOVIE_STAR != 0) {
        definition.collision_height_standing
            + (definition.collision_height_crouching - definition.collision_height_standing) * state.crouch
            - 2.0 * definition.collision_radius
    } else {
        0.0
    };
    (base, height, definition.collision_radius)
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

/// The C library's double functions, as floats.
fn sin(a: f32) -> f32 {
    (a as f64).sin() as f32
}
fn cos(a: f32) -> f32 {
    (a as f64).cos() as f32
}

/// `biped_snap_facing`: a flying biped banks, a climbing one leans onto
/// what it climbs, a dead one lies along the ground, the rest stand up.
pub fn snap_facing(definition: &BipedDefinition, inputs: &MovingInputs, state: &mut MovingState) {
    let flying = definition.flags & biped_flags::FLYING != 0;
    let climbs = definition.flags & biped_flags::CLIMBS_ANYTHING != 0;
    if flying && !inputs.dead {
        let mut left = cross3(state.forward, UP);
        let mut up = cross3(left, state.forward);
        if normalize3(&mut up) == 0.0 {
            up = FORWARD;
            left = LEFT;
        }
        let cosine = cos(state.bank);
        let sine = sin(state.bank);
        up = [up[0] * cosine, up[1] * cosine, up[2] * cosine];
        normalize3(&mut left);
        state.up = [left[0] * sine + up[0], left[1] * sine + up[1], left[2] * sine + up[2]];
    } else if climbs && !inputs.dead {
        let mut normal;
        if state.support_surface_index == NONE {
            normal = state.up;
        } else {
            let mut tilt_toward_surface = true;
            normal = [state.ground_plane[0], state.ground_plane[1], state.ground_plane[2]];
            let mut axis = cross3(state.up, normal);
            if normalize3(&mut axis) == 0.0 {
                if dot3(normal, state.up) > 0.0 {
                    tilt_toward_surface = false;
                } else {
                    axis = state.forward;
                }
            }
            if tilt_toward_surface {
                let tilted_up = rotate_vector_about_axis(state.up, axis, sin(CLIMBING_SNAP_ANGLE), cos(CLIMBING_SNAP_ANGLE));
                let vector = cross3(tilted_up, normal);
                if dot3(axis, vector) > 0.0 {
                    normal = tilted_up;
                }
            }
        }
        let mut vector = cross3(state.forward, normal);
        let mut forward = cross3(normal, vector);
        if normalize3(&mut forward) == 0.0 {
            vector = cross3(normal, state.up);
            forward = cross3(normal, vector);
            if normalize3(&mut forward) == 0.0 {
                normal = UP;
                forward = FORWARD;
            }
        }
        state.forward = forward;
        state.up = normal;
    } else if inputs.dead && state.biped_flags & state_flags::AIRBORNE == 0 {
        let n = [state.ground_plane[0], state.ground_plane[1], state.ground_plane[2]];
        let cosine_of_angle = dot3(state.up, n);
        if !((cosine_of_angle - 1.0).abs() < EPSILON) {
            let angle = (cosine_of_angle as f64).acos() as f32;
            if angle != 0.0 {
                let mut axis = cross3(state.up, n);
                if normalize3(&mut axis) != 0.0 {
                    let cosine = cos(angle);
                    let sine = sin(angle);
                    state.up = rotate_vector_about_axis(state.up, axis, sine, cosine);
                    state.forward = rotate_vector_about_axis(state.forward, axis, sine, cosine);
                    normalize3(&mut state.up);
                    normalize3(&mut state.forward);
                }
            }
        }
    } else {
        state.forward[2] = 0.0;
        if normalize3(&mut state.forward) == 0.0 {
            state.forward = FORWARD;
        }
        state.up = UP;
    }
}

/// `biped_start_landing`: a landing fast enough to slow the biped down for
/// a while (soft, or hard and longer).
fn start_landing(definition: &BipedDefinition, state: &mut MovingState, landing_velocity: f32) {
    let minimum_soft = definition.minimum_soft_landing_velocity * (1.0 / TICKS_PER_SECOND);
    let minimum_hard = definition.minimum_hard_landing_velocity * (1.0 / TICKS_PER_SECOND);
    if landing_velocity < minimum_soft {
        return;
    }
    let (mut velocity, velocity_range, mut recovery_time, landing_type) = if landing_velocity < minimum_hard {
        (landing_velocity - minimum_soft, minimum_hard - minimum_soft, definition.maximum_soft_landing_time, 0)
    } else {
        (
            landing_velocity,
            definition.maximum_hard_landing_velocity * (1.0 / TICKS_PER_SECOND) - minimum_hard,
            definition.maximum_hard_landing_time,
            1,
        )
    };
    recovery_time *= TICKS_PER_SECOND;
    if velocity_range > 0.0 {
        velocity /= velocity_range;
        velocity = if velocity < 0.0 {
            0.0
        } else if velocity > 1.0 {
            1.0
        } else {
            velocity
        };
        state.landing = landing_type;
        state.landing_recovery_counter = 0;
        // ((char)(long): truncated, then its low byte)
        state.landing_recovery_time = (recovery_time * velocity) as i32 as i8;
    }
}

/// `biped_jump`.
fn jump(definition: &BipedDefinition, player_information: &PlayerInformation, inputs: &MovingInputs, state: &mut MovingState) -> bool {
    if state.biped_flags & state_flags::AIRBORNE != 0 || state.landing == 1 {
        return false;
    }
    let mut jumped = true;
    let mut jump_magnitude = definition.jump_velocity;
    if inputs.player_index != NONE {
        jump_magnitude *= 1.0 - inputs.body_stun * player_information.stun_turning_penalty;
    }
    let mut jump_velocity = state.translational_velocity;
    let up = state.up;
    let upward_velocity = jump_velocity[0] * up[0] + (jump_velocity[2] * up[2] + jump_velocity[1] * up[1]);
    if upward_velocity < jump_magnitude {
        let velocity_delta = jump_magnitude - upward_velocity;
        jump_velocity[0] += up[0] * velocity_delta;
        jump_velocity[1] += up[1] * velocity_delta;
        jump_velocity[2] += up[2] * velocity_delta;
    }
    if inputs.actor
        && let Some((actor_jumped, actor_velocity)) = inputs.actor_jump
    {
        jumped = actor_jumped;
        jump_velocity = actor_velocity;
    }
    if jumped {
        state.translational_velocity = jump_velocity;
        state.biped_flags |= state_flags::AIRBORNE;
        state.jump_recovery_timer = 0;
        state.support_surface_index = NONE;
    }
    jumped
}

/// `biped_update_jumping`: on the ground, the jump control jumps once the
/// last jump has had five ticks to recover.
fn update_jumping(definition: &BipedDefinition, player_information: &PlayerInformation, inputs: &MovingInputs, state: &mut MovingState) {
    if state.biped_flags & state_flags::AIRBORNE == 0 && state.landing != 1 {
        if state.jump_recovery_timer < i8::MAX {
            state.jump_recovery_timer += 1;
        }
        if inputs.control_flags & control_flags::JUMP != 0 && state.jump_recovery_timer > 5 {
            jump(definition, player_information, inputs, state);
        }
    }
}

/// `biped_bumped_object`: a player's biped pushing the same object for
/// more than three ticks stops counting for half a second.
fn bumped_object(inputs: &MovingInputs, state: &mut MovingState, object_index: i32) {
    let bump_ticks = state.bump_ticks;
    if bump_ticks < 0 {
        state.bump_ticks = if object_index == NONE { bump_ticks + 1 } else { -15 };
    } else if object_index != NONE && (inputs.player_index != NONE || inputs.recorded_animation) {
        if state.bump_object_index != object_index {
            state.bump_object_index = object_index;
            state.bump_ticks = 0;
        } else {
            state.bump_ticks = state.bump_ticks.wrapping_add(1);
            if state.bump_ticks > 3 {
                state.bump_ticks = -15;
            }
        }
    }
}

/// `biped_update_moving`.
pub fn biped_update_moving(
    world: &PhysicsWorld<'_>,
    definition: &BipedDefinition,
    player_information: &PlayerInformation,
    inputs: &MovingInputs,
    state: &mut MovingState,
    stand_test: Option<crate::biped::StandTest>,
) -> MovingOutcome {
    let mut outcome = MovingOutcome::default();
    if state.object_flags & object_flags::AT_REST != 0 && inputs.dead && inputs.ignore_translation {
        return outcome;
    }
    let flags = definition.flags;
    let flying_definition = flags & biped_flags::FLYING != 0;
    let player = inputs.player_index != NONE;
    let mut physics = BipedPhysics { biped_index: inputs.biped_index as i32, ..Default::default() };
    physics.forward = state.forward;
    physics.aiming = if definition.unit_flags & UNIT_SIMPLE_CREATURE != 0 { state.forward } else { inputs.aiming_vector };
    physics.velocity = state.translational_velocity;
    physics.crouch_velocity = 0.0;
    physics.acceleration_maximum = 0.0053333333;
    physics.airborne_acceleration_maximum = 0.0;
    (physics.position, physics.height, physics.width) = physics_pill(definition, state, player);
    physics.minimum_normal_k = definition.runtime_minimum_normal_k;
    physics.downhill_k0 = definition.runtime_downhill_k0;
    physics.downhill_k1 = definition.runtime_downhill_k1;
    physics.downhill_velocity_scale = definition.downhill_velocity_scale;
    physics.uphill_k0 = definition.runtime_uphill_k0;
    physics.uphill_k1 = definition.runtime_uphill_k1;
    physics.uphill_velocity_scale = definition.uphill_velocity_scale;
    physics.ground_plane = state.ground_plane;
    physics.ground_tangential_velocity_max = f32::MAX;
    physics.ground_tangential_angle = 0.0;
    physics.existing_support_surface_index = state.support_surface_index;

    if state.landing == 1 {
        physics.movement_desired = [0.0; 3];
        physics.movement_penalty = 1.0;
    } else {
        let mut movement_scale = 1.0f32;
        if flags & biped_flags::RANDOM_SPEED_INCREASE != 0 && inputs.aiming_speed == 0 {
            let stagger_ordinal = inputs.biped_index % 137;
            movement_scale = inputs.difficulty_infection_forms * (stagger_ordinal as f32 * 0.00729927) + 1.0;
        }

        if !flying_definition || inputs.dead {
            let [throttle_x, throttle_y, throttle_z] = inputs.throttle;
            if throttle_x != 0.0 || throttle_y != 0.0 || throttle_z != 0.0 {
                let mut stunned = inputs.body_stun > 0.2;
                if definition.stunned_movement_threshold > 0.0 && inputs.recent_body_damage > definition.stunned_movement_threshold {
                    stunned = true;
                }
                let movement_base = if stunned { unit_state::STUNNED_MOVE_FRONT } else { unit_state::MOVE_FRONT };
                state.animation_state_desired = if throttle_x.abs() >= throttle_y.abs() {
                    if throttle_x >= 0.0 { movement_base } else { movement_base + 1 }
                } else if throttle_y >= 0.0 {
                    movement_base + 2
                } else {
                    movement_base + 3
                };
            }
        } else {
            state.animation_state_desired = unit_state::IDLE;
        }

        physics.movement_desired = [0.0; 3];
        if let Some(frame) = inputs.frame_info
            && (inputs.dead || !flying_definition)
            && !inputs.ignore_translation
        {
            let mut dy = 0.0f32;
            let mut dyaw = 0.0f32;
            match frame.kind {
                1 => {
                    physics.movement_desired[0] = frame.dx;
                    dy = frame.dy;
                }
                2 => {
                    physics.movement_desired[0] = frame.dx;
                    dy = frame.dy;
                    physics.movement_desired[1] = frame.dy;
                    dyaw = frame.dyaw;
                }
                3 => {
                    physics.movement_desired[0] = frame.dx;
                    dy = frame.dy;
                    physics.movement_desired[1] = frame.dy;
                    physics.movement_desired[2] = frame.dz;
                    dyaw = frame.dyaw;
                }
                _ => {}
            }
            physics.movement_desired[0] *= movement_scale;
            physics.movement_desired[1] = dy * movement_scale;
            physics.movement_desired[2] *= movement_scale;

            if !(dyaw.abs() < EPSILON) {
                let sine_value = sin(dyaw);
                let cosine_value = cos(dyaw);
                let mut rotated_forward = rotate_vector_about_axis(state.forward, state.up, sine_value, cosine_value);
                if inputs.control_flags & control_flags::EXACT_FACING != 0
                    && (inputs.unit_animation_state == unit_state::TURN_LEFT || inputs.unit_animation_state == unit_state::TURN_RIGHT)
                    && dot3(state.forward, inputs.desired_facing_vector) > 0.5
                {
                    let old_cross = cross3(inputs.desired_facing_vector, state.forward);
                    let new_cross = cross3(inputs.desired_facing_vector, rotated_forward);
                    if dot3(old_cross, state.up) * dot3(new_cross, state.up) <= 0.0 {
                        rotated_forward = inputs.desired_facing_vector;
                        outcome.aborted_animation = true;
                    }
                }
                state.forward = rotated_forward;
                snap_facing(definition, inputs, state);
            }
        }

        if !flying_definition || inputs.dead {
            if flags & biped_flags::USES_PLAYER_PHYSICS == 0 || inputs.unit_animation_state == unit_state::USER_ANIMATION {
                if state.biped_flags & (state_flags::SLIPPING | state_flags::AIRBORNE) == 0 {
                    state.translational_velocity[0] = physics.movement_desired[0];
                    state.translational_velocity[1] = physics.movement_desired[1];
                    physics.acceleration_maximum = f32::MAX;
                }
            } else {
                let mut info = *player_information;
                if inputs.cinematic || flags & biped_flags::USES_OLD_PLAYER_PHYSICS != 0 {
                    info.walking_speed = 0.51200002;
                    info.run_forward_speed = 2.25;
                    info.run_backward_speed = 2.0;
                    info.run_sideways_speed = 2.0;
                    info.run_acceleration = 0.31999999;
                }
                let body_stun_scale = if player { inputs.speed_multiplier } else { 1.0 };
                let crouch = state.crouch;
                let uncrouch = 1.0 - crouch;
                let stun_scale = -(info.stun_movement_penalty * inputs.body_stun - 1.0) * body_stun_scale * movement_scale;
                let (mut forward_speed, mut sideways_speed) = if inputs.throttle[0] <= 0.0 {
                    (info.sneak_backward_speed, info.run_backward_speed)
                } else {
                    (info.sneak_forward_speed, info.run_forward_speed)
                };
                forward_speed = sideways_speed * uncrouch + forward_speed * crouch;
                sideways_speed = info.run_sideways_speed * uncrouch + info.sneak_sideways_speed * crouch;
                let acceleration = info.run_acceleration * uncrouch + info.sneak_acceleration * crouch;
                if inputs.base_seat_index == BASE_SEAT_ALERT {
                    forward_speed = if inputs.throttle[0] <= 0.0 { 0.0 } else { info.walking_speed };
                    sideways_speed = 0.0;
                }
                physics.acceleration_maximum = acceleration / TICKS_PER_SECOND;
                physics.movement_desired[0] = inputs.throttle[0] * stun_scale * forward_speed / TICKS_PER_SECOND;
                physics.movement_desired[1] = inputs.throttle[1] * stun_scale * sideways_speed / TICKS_PER_SECOND;
                physics.movement_desired[2] = 0.0;
                physics.airborne_acceleration_maximum = info.airborne_acceleration / TICKS_PER_SECOND;
                if inputs.control_flags & control_flags::LOOK_DONT_TURN == 0 {
                    physics.forward = inputs.desired_facing_vector;
                }
                if inputs.double_speed {
                    physics.movement_desired = scale3(physics.movement_desired, info.double_speed_multiplier);
                }
            }
        } else {
            let throttle_magnitude = magnitude3(inputs.throttle);
            let deceleration_scale = 1.0 - if throttle_magnitude < 1.0 { throttle_magnitude } else { 1.0 };
            let mut crouch_modifier = 1.0f32;
            if definition.flying_crouch_velocity_modifier > 0.0 {
                if state.crouch == 1.0 {
                    crouch_modifier = definition.flying_crouch_velocity_modifier;
                } else if state.crouch > 0.0 {
                    crouch_modifier = (definition.flying_crouch_velocity_modifier - 1.0) * state.crouch + 1.0;
                }
            }
            physics.movement_desired[0] = definition.flying_velocity * crouch_modifier * movement_scale;
            physics.movement_desired[1] = definition.flying_sidestep_velocity * crouch_modifier * movement_scale;
            physics.movement_desired[2] = definition.flying_sidestep_velocity * crouch_modifier * movement_scale;
            let acceleration =
                (1.0 - deceleration_scale) * definition.flying_acceleration + definition.flying_deceleration * deceleration_scale;
            physics.movement_desired[0] *= inputs.throttle[0] / TICKS_PER_SECOND;
            physics.movement_desired[1] *= inputs.throttle[1] / TICKS_PER_SECOND;
            physics.movement_desired[2] *= inputs.throttle[2] / TICKS_PER_SECOND;
            physics.acceleration_maximum = acceleration * crouch_modifier * movement_scale / TICKS_PER_SECOND;
            physics.airborne_acceleration_maximum = physics.acceleration_maximum;
        }

        if state.biped_flags & state_flags::AIRBORNE != 0 && state.airborne_ticks < 22 && inputs.actor && inputs.actor_leaping {
            physics.ground_tangential_velocity_max = 0.1;
            physics.ground_tangential_angle = 0.5;
        }
        physics.movement_penalty = 0.0;
    }

    // crouching and standing up
    {
        let crouch_transition_velocity = definition.runtime_crouch_transition_velocity;
        let crouch_delta;
        if inputs.base_seat_index == BASE_SEAT_CROUCH {
            let delta = 1.0 - state.crouch;
            if delta <= crouch_transition_velocity {
                state.crouch = 1.0;
                crouch_delta = delta;
            } else {
                crouch_delta = crouch_transition_velocity;
                state.crouch += crouch_transition_velocity;
            }
        } else {
            let delta = -state.crouch;
            if delta >= -crouch_transition_velocity {
                state.crouch = 0.0;
                crouch_delta = delta;
            } else {
                crouch_delta = -crouch_transition_velocity;
                state.crouch -= crouch_transition_velocity;
            }
        }
        // (fabs(...) > 0.01: a double comparison)
        if (crouch_delta as f64).abs() > 0.01 && state.biped_flags & state_flags::AIRBORNE != 0 {
            physics.crouch_velocity = (definition.collision_height_standing - definition.collision_height_crouching) * crouch_delta;
        }
    }

    let mut flags_in = physics.in_flags;
    if state.crouch != 0.0 {
        flags_in |= in_flags::CROUCHED;
        if state.animation_crouching == 0 {
            flags_in |= in_flags::TRYING_TO_STAND;
        }
    }
    if state.biped_flags & state_flags::AIRBORNE != 0 {
        flags_in |= in_flags::AIRBORNE;
    }
    if state.biped_flags & state_flags::SLIPPING != 0 {
        flags_in |= in_flags::SLIPPING;
    }
    if state.biped_flags & state_flags::ABSOLUTE_MOVEMENT != 0 {
        flags_in |= in_flags::ABSOLUTE_MOVEMENT;
    }
    if state.biped_flags & state_flags::NO_COLLISION != 0 {
        flags_in |= in_flags::NO_COLLISION;
    }
    if inputs.dead {
        flags_in |= in_flags::DEAD;
    }
    if flying_definition && !inputs.dead {
        flags_in |= in_flags::FLYING;
    }
    if flags & biped_flags::PASSES_THROUGH_BIPEDS != 0 {
        flags_in |= in_flags::PASS_THROUGH_BIPEDS;
    }
    if player && !inputs.coop_player_collisions {
        flags_in |= in_flags::PASS_THROUGH_PLAYERS;
    }
    if flags & biped_flags::CLIMBS_ANYTHING != 0 && !inputs.dead {
        flags_in |= in_flags::CLIMB_ANYTHING;
    }
    physics.in_flags = flags_in;
    outcome.physics = Some(physics);

    crate::biped::biped_update_physics(world.bsp, &mut physics, stand_test, world.objects);

    if physics.elevator_object_index == NONE {
        if state.elevator_ticks <= 0 {
            state.elevator_object_index = NONE;
        } else {
            state.elevator_ticks -= 1;
        }
    } else {
        state.elevator_object_index = physics.elevator_object_index;
        state.elevator_ticks = 60;
    }

    if inputs.suspended {
        physics.new_position = physics.position;
        physics.out_flags &= !out_flags::AIRBORNE;
        physics.new_velocity = [0.0; 3];
        physics.velocity = [0.0; 3];
    }

    let mut new_biped_position = physics.new_position;
    let out = physics.out_flags;
    if flags & biped_flags::PILL_CENTERED_AT_ORIGIN == 0 {
        new_biped_position[2] -= physics.width;
    }
    translate(world, inputs, state, new_biped_position);

    state.support_surface_index = physics.support_surface_index;
    state.pathfinding_point = new_biped_position;
    state.pathfinding_surface_index = NONE;
    state.translational_velocity = physics.new_velocity;
    if state.animation_crouching == 0 && out & out_flags::CANNOT_STAND != 0 {
        state.animation_crouching = 1;
    }
    set_flag(&mut state.biped_flags, state_flags::AIRBORNE, out & out_flags::AIRBORNE != 0);
    set_flag(&mut state.biped_flags, state_flags::SLIPPING, out & out_flags::SLIPPING != 0);
    set_flag(&mut state.biped_flags, state_flags::MOVEMENT_PASSES_THROUGH_BIPEDS, flags & biped_flags::PASSES_THROUGH_BIPEDS != 0);
    state.ground_plane = physics.ground_plane;
    if physics.landing_velocity > 0.0 {
        start_landing(definition, state, physics.landing_velocity);
    }
    if out & out_flags::CANNOT_STAND == 0 {
        update_jumping(definition, player_information, inputs, state);
    }
    // (a melee impact's damage, and the AI's notice of a bump: not ported)
    bumped_object(inputs, state, physics.bumped_object_index);
    // (falling damage: not ported)

    let flying_in = physics.in_flags & in_flags::FLYING != 0;
    let airborne = state.biped_flags & state_flags::AIRBORNE != 0;
    set_flag(&mut state.object_flags, object_flags::ON_GROUND, !flying_in && !airborne);
    set_flag(
        &mut state.object_flags,
        object_flags::AT_REST,
        !flying_in && !airborne && out & out_flags::VOLATILE_COLLISION == 0 && magnitude_squared3(state.translational_velocity) < 0.0001,
    );
    if state.object_flags & object_flags::ON_GROUND != 0 {
        state.angular_velocity = [0.0; 3];
    }
    outcome
}

fn set_flag(flags: &mut u32, flag: u32, value: bool) {
    if value {
        *flags |= flag;
    } else {
        *flags &= !flag;
    }
}

/// `object_translate` with no location, for an object with no parent: its
/// location is where its bounding sphere's centre is (or, outside the
/// world, its position); with none, it is outside the map.
fn translate(world: &PhysicsWorld<'_>, inputs: &MovingInputs, state: &mut MovingState, position: [f32; 3]) {
    state.position = position;
    let mut location = world.clusters.location_from_point(world.bsp, inputs.bounding_center);
    if location.1 == NONE as i16 {
        location = world.clusters.location_from_point(world.bsp, state.position);
    }
    if location.1 != NONE as i16 {
        (state.location_leaf, state.location_cluster) = location;
        state.object_flags &= !object_flags::OUTSIDE_OF_MAP;
    } else {
        state.object_flags |= object_flags::OUTSIDE_OF_MAP;
    }
}
