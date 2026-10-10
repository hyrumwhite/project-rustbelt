//! Halo's biped update, up to its animation (ROADMAP phase 7, objects and
//! physics): `biped_update` in OpenCE's `source/units/bipeds.c` for a biped
//! with no parent, with `biped_update_turning` (and, for flying bipeds,
//! `unit_euler_aiming_update`'s acceleration-limited turning from
//! `units.c`), the movement ([`crate::biped_moving`]) and the dead,
//! airborne and landing updates, ported as they are, in the same
//! single-precision arithmetic.
//!
//! A player's melee step ([`player_melee`]: when it starts, timed by the
//! first-person weapon's melee animation, and the tick its damage lands)
//! is here too; the animation update that follows is
//! [`crate::unit_animation`]. What has no bearing on the biped's state
//! (footsteps, a limp body's relaxing onto the ground) is the game's,
//! through an [`UpdateHost`] in a running game (as is its random seed, which
//! a tumble draws from); the AI's falling danger and discarding follow.

use crate::biped_moving::{
    self, BipedDefinition, MovingInputs, MovingOutcome, MovingState, PhysicsWorld, PlayerInformation, UNIT_IS_SPECIAL, biped_flags,
    control_flags, cos, rotate_vector_about_axis, sin, snap_facing, state_flags, unit_state,
};
use crate::features::{cross3, dot3, magnitude_squared3, normalize3, scale3};

/// What `biped_update` changes of an unparented biped before its animation
/// updates, laid out as the bridge shares it.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct UpdateState {
    pub moving: MovingState,
    pub throttle: [f32; 3],
    pub desired_facing_vector: [f32; 3],
    pub biped_state: i8,
    pub slipping_ticks: i8,
    pub pad: i16,
}

const _: () = assert!(std::mem::size_of::<UpdateState>() == 164);

/// What `biped_update` reads of the biped that it doesn't change.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct UpdateInputs {
    pub dead: bool,
    pub control_flags: u32,
    pub aim_without_turning: bool,
    pub unit_animation_state: i8,
    pub base_seat_index: i8,
    /// `unit_animation_enhanced` (a port option).
    pub enhanced_animation: bool,
    /// Its limp body still relaxing onto the ground (not ported).
    pub limp_body: bool,
    pub player_index: i32,
    /// The global random seed as its airborne update begins.
    pub airborne_seed: Option<u32>,
}

/// A running game, as a biped's update calls it. The state is the update's
/// as it stands (the host writes it to the biped first, and reads back what
/// the call changed).
pub trait UpdateHost {
    /// Its movement begins (the update's own state written to the biped,
    /// which the movement's calls may read) and ends.
    fn moving_begins(&self, state: &mut UpdateState);
    fn moving_ends(&self, state: &mut UpdateState);
    /// `biped_limp_noodle_relax_nodes_onto_environment`.
    fn limp_body(&self, state: &mut MovingState);
    /// The game's random seed as a tumble draws from it, and after.
    fn random_seed(&self) -> u32;
    fn set_random_seed(&self, seed: u32);
    /// Its landing's and slipping's footsteps (as their time comes).
    fn landing_footsteps(&self, state: &mut MovingState);
    fn slipping_footsteps(&self, state: &mut MovingState);
}

/// `biped_state`.
pub const BIPED_STATE_IDLE: i8 = 0;
pub const BIPED_STATE_MOVING: i8 = 1;
pub const BIPED_STATE_UNKNOWN: i8 = 2;

const NONE: i32 = -1;
const EPSILON: f32 = 0.0001;
const TICKS_PER_SECOND: f32 = 30.0;
/// `_pi`, as a float.
const PI: f32 = std::f32::consts::PI;
const UP: [f32; 3] = [0.0, 0.0, 1.0];
const FORWARD: [f32; 3] = [1.0, 0.0, 0.0];
const BASE_SEAT_ASLEEP: i8 = 0;
const BASE_SEAT_FLAMING: i8 = 5;
const STATE_AIRBORNE: i8 = 20;
const STATE_LAND_SOFT: i8 = 21;
const STATE_LAND_HARD: i8 = 22;
const STATE_DYING_AIRBORNE: i8 = 24;
const STATE_DYING: i8 = 25;
const STATE_MELEE_AIRBORNE: i8 = 31;
const STATE_THROW_GRENADE: i8 = 33;
const STATE_LEAP_START: i8 = 39;
const STATE_LEAP_AIRBORNE: i8 = 40;
const STATE_LEAP_MELEE: i8 = 41;

/// `biped_update` of an unparented biped, up to its animation update. The
/// animation update's wishes end in the state's `animation_state_desired`
/// and `animation_crouching`. `moving` is what the movement reads of the
/// rest of the game; its throttle and desired facing are this update's.
#[allow(clippy::too_many_arguments)]
pub fn biped_update(
    world: &PhysicsWorld<'_>,
    definition: &BipedDefinition,
    player_information: &PlayerInformation,
    inputs: &UpdateInputs,
    moving: &MovingInputs,
    stand_test: Option<crate::biped::StandTest>,
    directions: &[[f32; 3]],
    state: &mut UpdateState,
    host: Option<&dyn UpdateHost>,
) -> MovingOutcome {
    let flags = definition.flags;
    let mut moving_inputs = *moving;
    moving_inputs.dead = inputs.dead;

    snap_facing(definition, &moving_inputs, &mut state.moving);
    if inputs.dead || flags & (biped_flags::FLYING | biped_flags::CLIMBS_ANYTHING) == 0 {
        state.desired_facing_vector[2] = 0.0;
        if normalize3(&mut state.desired_facing_vector) == 0.0 {
            state.desired_facing_vector = FORWARD;
        }
    }

    state.biped_state = match inputs.unit_animation_state {
        4..=7 => BIPED_STATE_MOVING,
        0 | 2 | 3 => BIPED_STATE_IDLE,
        _ => BIPED_STATE_UNKNOWN,
    };

    if magnitude_squared3(state.throttle) < 0.1 * 0.1 {
        state.throttle = [0.0; 3];
    }

    let m = &mut state.moving;
    if m.biped_flags & state_flags::AIRBORNE != 0 {
        if m.airborne_ticks < i8::MAX {
            m.airborne_ticks += 1;
        }
    } else {
        m.airborne_ticks = 0;
    }
    if m.biped_flags & state_flags::SLIPPING != 0 {
        if state.slipping_ticks < i8::MAX {
            state.slipping_ticks += 1;
        }
    } else {
        state.slipping_ticks = 0;
    }

    state.moving.animation_state_desired = unit_state::IDLE;
    state.moving.animation_crouching = i8::from(inputs.control_flags & control_flags::CROUCH_MODIFIER != 0);

    if !inputs.dead {
        update_turning(definition, inputs, &moving_inputs, state);
    }

    moving_inputs.throttle = state.throttle;
    moving_inputs.desired_facing_vector = state.desired_facing_vector;
    if let Some(host) = host {
        host.moving_begins(state);
    }
    let outcome = biped_moving::biped_update_moving(world, definition, player_information, &moving_inputs, &mut state.moving, stand_test);
    if let Some(host) = host {
        host.moving_ends(state);
    }

    if inputs.dead {
        update_dead(definition, inputs, &moving_inputs, &mut state.moving, host);
    } else if state.moving.biped_flags & state_flags::AIRBORNE != 0 {
        update_airborne(definition, inputs, directions, &mut state.moving, host);
    } else if state.moving.landing != NONE as i16 {
        update_landing(&mut state.moving);
        if let Some(host) = host {
            host.landing_footsteps(&mut state.moving);
        }
    } else if state.moving.biped_flags & state_flags::SLIPPING != 0
        && let Some(host) = host
    {
        host.slipping_footsteps(&mut state.moving);
    }
    outcome
}

/// `biped_update_turning`.
fn update_turning(definition: &BipedDefinition, inputs: &UpdateInputs, moving: &MovingInputs, state: &mut UpdateState) {
    let flags = definition.flags;
    if flags & biped_flags::FLYING != 0 && !inputs.dead {
        let m = &mut state.moving;
        let mut nearly_stationary = false;
        if magnitude_squared3(m.translational_velocity) < 0.00027777778
            && magnitude_squared3(m.angular_velocity) < 0.0000013538552
            && magnitude_squared3(state.throttle) < 0.010000001
        {
            let threshold = if inputs.control_flags & control_flags::EXACT_FACING != 0 {
                0.99000001
            } else {
                definition.runtime_cosine_stationary_turning_threshold
            };
            if dot3(state.desired_facing_vector, m.forward) > threshold {
                nearly_stationary = true;
            }
        }
        let target_facing = if nearly_stationary {
            m.forward
        } else {
            let pitch = definition.flying_pitch_amount * state.throttle[2];
            let mut target = state.desired_facing_vector;
            if pitch != 0.0 {
                target[2] += pitch;
                if normalize3(&mut target) == 0.0 {
                    target = state.desired_facing_vector;
                }
            }
            target
        };

        let turn_axis = cross3(m.forward, m.up);
        let mut turn_rate = dot3(turn_axis, state.desired_facing_vector) * (10.0 / 3.0) * state.throttle[0] - state.throttle[1];
        if turn_rate > 1.5 {
            turn_rate = 1.5;
        }
        let target_bank = definition.flying_bank_amount * turn_rate;
        let bank_blend = if m.bank * target_bank > 0.0 {
            let mut bank_ratio = m.bank / target_bank;
            if bank_ratio > 1.0 {
                bank_ratio = 1.0;
            }
            1.0 - bank_ratio
        } else {
            1.0
        };
        let bank_time = definition.flying_bank_apply_time * bank_blend + definition.flying_bank_decay_time * (1.0 - bank_blend);
        if bank_time > 0.0 {
            m.bank += (target_bank - m.bank) / (bank_time * TICKS_PER_SECOND);
        } else {
            m.bank = target_bank;
        }

        let facing_bounds = [-PI, PI, -PI / 2.0, PI / 2.0];
        let angular_velocity_limit = definition.flying_angular_velocity / TICKS_PER_SECOND;
        let angular_acceleration_limit = definition.flying_angular_acceleration / (TICKS_PER_SECOND * TICKS_PER_SECOND);
        if angular_acceleration_limit == 0.0 {
            m.forward = target_facing;
        } else {
            euler_aiming_update(
                &mut m.forward,
                target_facing,
                &mut m.angular_velocity,
                facing_bounds,
                angular_velocity_limit,
                angular_acceleration_limit,
            );
        }
        snap_facing(definition, moving, &mut state.moving);
    } else if inputs.base_seat_index != BASE_SEAT_ASLEEP {
        let flaming = inputs.base_seat_index == BASE_SEAT_FLAMING;
        let throwing_turn =
            inputs.player_index != NONE && inputs.unit_animation_state == STATE_THROW_GRENADE && inputs.enhanced_animation;
        let climbs = flags & biped_flags::CLIMBS_ANYTHING != 0;
        let m = &mut state.moving;
        let mut turn_axis;
        let turn_error;
        let facing_alignment;
        if climbs {
            let cross = cross3(m.up, state.desired_facing_vector);
            turn_axis = cross3(cross, m.up);
            if normalize3(&mut turn_axis) == 0.0 {
                turn_axis = m.forward;
            }
            let error = cross3(turn_axis, m.forward);
            turn_error = dot3(error, m.up);
            facing_alignment = dot3(turn_axis, m.forward);
        } else {
            turn_axis = state.desired_facing_vector;
            turn_axis[2] = 0.0;
            // (normalize2d)
            let mut axis2 = [turn_axis[0], turn_axis[1]];
            let magnitude = (axis2[0] * axis2[0] + axis2[1] * axis2[1]).sqrt();
            if !(EPSILON > magnitude.abs()) {
                let inverse = 1.0 / magnitude;
                axis2 = [inverse * axis2[0], inverse * axis2[1]];
                turn_axis[0] = axis2[0];
                turn_axis[1] = axis2[1];
            } else {
                turn_axis = m.forward;
            }
            let forward2 = [m.forward[0], m.forward[1]];
            turn_error = turn_axis[0] * forward2[1] - turn_axis[1] * forward2[0];
            facing_alignment = turn_axis[0] * forward2[0] + turn_axis[1] * forward2[1];
        }

        let mut turn_right = turn_error > 0.0;
        if facing_alignment < -0.89999998 {
            if inputs.unit_animation_state == unit_state::TURN_RIGHT {
                turn_right = true;
            } else if inputs.unit_animation_state == unit_state::TURN_LEFT {
                turn_right = false;
            }
        }

        if state.biped_state == BIPED_STATE_MOVING || flags & biped_flags::TURNS_WITHOUT_ANIMATING != 0 || throwing_turn {
            if inputs.control_flags & control_flags::LOOK_DONT_TURN == 0 {
                let turn_step = definition.moving_turning_speed / TICKS_PER_SECOND;
                let cosine = cos(turn_step);
                let mut sine = sin(turn_step);
                if turn_right {
                    sine = -sine;
                }
                let progress = if climbs {
                    m.forward = rotate_vector_about_axis(m.forward, m.up, sine, cosine);
                    let cross = cross3(turn_axis, m.forward);
                    dot3(cross, m.up)
                } else {
                    // (rotate_vector2d)
                    let (i, j) = (m.forward[0], m.forward[1]);
                    let new_j = sine * i + cosine * j;
                    let new_i = cosine * i - sine * j;
                    m.forward[0] = new_i;
                    m.forward[1] = new_j;
                    turn_axis[0] * new_j - turn_axis[1] * new_i
                };
                if (!turn_right || progress < 0.0) && (turn_right || progress > 0.0) {
                    if climbs {
                        let mut rotation_axis = cross3(m.up, turn_axis);
                        if normalize3(&mut rotation_axis) > 0.0 {
                            m.forward = cross3(rotation_axis, m.up);
                        }
                    } else {
                        m.forward = turn_axis;
                        m.forward[2] = 0.0;
                        m.up = UP;
                    }
                    normalize3(&mut m.forward);
                }
            }
        } else if state.biped_state == BIPED_STATE_IDLE
            && !flaming
            && !inputs.aim_without_turning
            && inputs.control_flags & control_flags::LOOK_DONT_TURN == 0
        {
            let threshold = if inputs.control_flags & control_flags::EXACT_FACING != 0 {
                0.99000001
            } else {
                definition.runtime_cosine_stationary_turning_threshold
            };
            if facing_alignment < threshold && definition.unit_flags & UNIT_IS_SPECIAL == 0 {
                m.animation_state_desired = if turn_right { unit_state::TURN_RIGHT } else { unit_state::TURN_LEFT };
            }
        }
    }
}

/// `biped_apply_rotation`: the biped turned by its angular velocity.
fn apply_rotation(state: &mut MovingState) {
    let mut axis = state.angular_velocity;
    let angle = normalize3(&mut axis);
    let cosine = cos(angle);
    let sine = sin(angle);
    state.forward = rotate_vector_about_axis(state.forward, axis, sine, cosine);
    normalize3(&mut state.forward);
    let rotated_up = rotate_vector_about_axis(state.up, axis, sine, cosine);
    let left = cross3(state.forward, rotated_up);
    state.up = cross3(left, state.forward);
    if normalize3(&mut state.up) == 0.0 {
        state.forward = FORWARD;
        state.up = UP;
    }
}

/// `biped_update_dead`: tumbling through the air, then lying dying.
fn update_dead(definition: &BipedDefinition, inputs: &UpdateInputs, moving: &MovingInputs, state: &mut MovingState, host: Option<&dyn UpdateHost>) {
    if inputs.limp_body {
        if let Some(host) = host {
            host.limp_body(state);
        }
        state.animation_crouching = 0;
        return;
    }
    if state.airborne_ticks >= 3 && definition.flags & biped_flags::HAS_NO_DYING_AIRBORNE == 0 {
        if inputs.unit_animation_state == STATE_DYING_AIRBORNE {
            apply_rotation(state);
        }
        state.animation_state_desired = STATE_DYING_AIRBORNE;
    } else {
        if inputs.unit_animation_state == STATE_DYING_AIRBORNE {
            state.bank = 0.0;
            snap_facing(definition, moving, state);
        }
        state.animation_state_desired = STATE_DYING;
    }
}

/// `biped_update_airborne`: a biped thrown through the air may tumble.
fn update_airborne(
    definition: &BipedDefinition,
    inputs: &UpdateInputs,
    directions: &[[f32; 3]],
    state: &mut MovingState,
    host: Option<&dyn UpdateHost>,
) {
    let _ = directions;
    let mut flying = false;
    // (biped_flying_through_air)
    if state.airborne_ticks > 3 && (definition.flags & biped_flags::FLYING == 0 || inputs.dead) {
        flying = true;
        if definition.flags & biped_flags::ROTATE_WHILE_AIRBORNE != 0 {
            if inputs.unit_animation_state != STATE_MELEE_AIRBORNE && inputs.unit_animation_state != STATE_LEAP_MELEE {
                let mut seed = host.map_or(inputs.airborne_seed.unwrap_or(0), |h| h.random_seed());
                let mut real_random = || {
                    seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                    (seed >> 16) as f32 / 65535.0
                };
                let random = real_random();
                let impulse = 0.052359879 + (0.08726646 - 0.052359879) * random;
                let mut axis = [0.0f32; 3];
                let mut ready = false;
                if state.up[2] < 0.8 {
                    axis = cross3(state.up, UP);
                    ready = normalize3(&mut axis) > 0.0;
                }
                if !ready {
                    let random = real_random();
                    let angle = 0.0 + (2.0 * PI - 0.0) * random;
                    // (vector3d_from_angle)
                    axis = [cos(angle), sin(angle), 0.0];
                }
                let v = state.angular_velocity;
                state.angular_velocity = [axis[0] * impulse + v[0], axis[1] * impulse + v[1], axis[2] * impulse + v[2]];
                if let Some(host) = host {
                    host.set_random_seed(seed);
                }
            }
            apply_rotation(state);
        }
    }
    if inputs.unit_animation_state == STATE_LEAP_START || inputs.unit_animation_state == STATE_LEAP_AIRBORNE {
        state.animation_state_desired = STATE_LEAP_AIRBORNE;
    } else if inputs.unit_animation_state == STATE_AIRBORNE || flying {
        state.animation_state_desired = STATE_AIRBORNE;
    }
}

/// `biped_update_landing`: the landing's recovery counts down.
fn update_landing(state: &mut MovingState) {
    state.landing_recovery_counter = state.landing_recovery_counter.wrapping_add(1);
    if state.landing_recovery_counter >= state.landing_recovery_time {
        state.landing = NONE as i16;
    }
    state.animation_state_desired = if state.landing == 1 { STATE_LAND_HARD } else { STATE_LAND_SOFT };
}

// ---------- unit_euler_aiming_update

/// `unit_acceleration_plan`: an axis's turn as acceleration, coasting and
/// deceleration.
#[derive(Debug, Clone, Copy, Default)]
struct AccelerationPlan {
    at_rest: bool,
    initial_position: f32,
    initial_velocity: f32,
    acceleration: f32,
    acceleration_time: f32,
    coast_time: f32,
    deceleration: f32,
    deceleration_time: f32,
}

/// `MIN`, `MAX` and `PIN`, as the macros compare.
fn min(a: f32, b: f32) -> f32 {
    if a > b { b } else { a }
}
fn max(a: f32, b: f32) -> f32 {
    if a > b { a } else { b }
}
fn pin(n: f32, floor: f32, ceiling: f32) -> f32 {
    if n < floor {
        floor
    } else if n > ceiling {
        ceiling
    } else {
        n
    }
}

/// `euler_angles2d_from_vector3d`: (yaw, pitch).
fn euler_from_vector(v: [f32; 3]) -> [f32; 2] {
    let horizontal = (v[0] * v[0] + v[1] * v[1]).sqrt();
    [(v[1] as f64).atan2(v[0] as f64) as f32, (v[2] as f64).atan2(horizontal as f64) as f32]
}

/// `vector3d_from_euler_angles2d`.
fn vector_from_euler(angles: [f32; 2]) -> [f32; 3] {
    let cosine_pitch = cos(angles[1]);
    [cos(angles[0]) * cosine_pitch, sin(angles[0]) * cosine_pitch, sin(angles[1])]
}

/// `unit_euler_axis_buildplan`: how to bring an axis from `position` (its
/// error) and `velocity` to rest at zero, as fast as its limits allow.
fn build_plan(position: f32, velocity: f32, maximum_velocity: f32, maximum_acceleration: f32) -> AccelerationPlan {
    let mut plan = AccelerationPlan {
        acceleration: f32::MAX,
        acceleration_time: f32::MAX,
        coast_time: f32::MAX,
        deceleration: f32::MAX,
        deceleration_time: f32::MAX,
        initial_position: position,
        initial_velocity: velocity,
        at_rest: position.abs() < 0.001 && velocity.abs() < 0.001,
    };
    if plan.at_rest {
        plan.acceleration = 0.0;
        plan.acceleration_time = 0.0;
        plan.coast_time = 0.0;
        plan.deceleration = 0.0;
        plan.deceleration_time = 0.0;
        return plan;
    }
    // (fabs: a double division)
    let time = ((velocity as f64).abs() / maximum_acceleration as f64) as f32;
    let moving_positive = velocity > 0.0;
    let half_time = time * 0.5;
    if half_time * velocity * 0.5 + position < 0.0 {
        let mut mirrored = build_plan(-position, -velocity, maximum_velocity, maximum_acceleration);
        mirrored.initial_position *= -1.0;
        mirrored.initial_velocity *= -1.0;
        mirrored.acceleration *= -1.0;
        mirrored.deceleration *= -1.0;
        return mirrored;
    }
    let half_velocity = velocity * 0.5;
    let stopping_position = half_velocity * time + position;
    if stopping_position < 0.0 {
        plan.acceleration = 0.0;
        plan.acceleration_time = 0.0;
        plan.deceleration = plan.initial_velocity * plan.initial_velocity / (plan.initial_position + plan.initial_position);
        plan.deceleration_time = -(plan.initial_velocity / plan.deceleration);
        plan.coast_time = 0.0;
        return plan;
    }
    let acceleration_time;
    let mut skip_to_limit = false;
    if moving_positive {
        acceleration_time = (stopping_position / maximum_acceleration).sqrt();
    } else {
        let negative_acceleration = -maximum_acceleration;
        let velocity2 = velocity + velocity;
        let discriminant = velocity2 * velocity2 - negative_acceleration * stopping_position * 4.0;
        let second_root = discriminant.sqrt();
        let first_root = (-velocity2 - second_root) / (negative_acceleration + negative_acceleration);
        let second_time = (second_root - velocity2) / (negative_acceleration + negative_acceleration);
        if first_root >= 0.0 && (second_time < 0.0 || first_root < second_time) {
            acceleration_time = first_root;
        } else if 0.0 > second_time {
            acceleration_time = 0.0;
            skip_to_limit = true;
        } else {
            acceleration_time = second_time;
        }
    }
    let _ = skip_to_limit;
    let first_root = if maximum_velocity > 0.0 {
        let maximum_velocity_time =
            (if moving_positive { maximum_velocity } else { maximum_velocity + plan.initial_velocity }) / maximum_acceleration;
        let maximum_velocity_time = max(0.0, maximum_velocity_time);
        min(acceleration_time, maximum_velocity_time)
    } else {
        acceleration_time
    };
    plan.acceleration = -maximum_acceleration;
    plan.deceleration = maximum_acceleration;
    if moving_positive {
        plan.acceleration_time = first_root + time;
        plan.deceleration_time = first_root;
    } else {
        plan.acceleration_time = first_root;
        plan.deceleration_time = first_root + time;
    }
    if first_root < acceleration_time {
        let coasting_velocity = plan.acceleration * plan.acceleration_time + plan.initial_velocity;
        let discriminant = ((acceleration_time - first_root) * coasting_velocity) * 2.0
            - (acceleration_time - first_root) * (acceleration_time - first_root) * maximum_acceleration;
        plan.coast_time = discriminant / coasting_velocity;
    } else {
        plan.coast_time = 0.0;
    }
    plan
}

/// `unit_euler_axis_couple`: the quicker axis slowed to finish with the
/// slower, so the turn runs straight.
fn couple_plans(first: &mut AccelerationPlan, second: &mut AccelerationPlan, maximum_acceleration: f32) {
    if first.at_rest || second.at_rest {
        return;
    }
    let first_total = first.deceleration_time + first.coast_time + first.acceleration_time;
    let second_total = second.deceleration_time + second.coast_time + second.acceleration_time;
    let (adjust, time_extension) = if first.acceleration_time > 0.0 && first_total < second_total {
        (first, second_total - first_total)
    } else if second.acceleration_time > 0.0 && second_total < first_total {
        (second, first_total - second_total)
    } else {
        return;
    };
    let time_factor = (time_extension + adjust.coast_time) * maximum_acceleration;
    let peak_velocity = adjust.acceleration_time * adjust.acceleration + adjust.initial_velocity;
    // ((real)((-t) * fabs(v)): a double product)
    let product = ((-time_extension) as f64 * (peak_velocity as f64).abs()) as f32;
    let discriminant = time_factor * time_factor - product * maximum_acceleration * 4.0;
    let mut adjustment = (discriminant.sqrt() - time_factor) / (maximum_acceleration + maximum_acceleration);
    adjustment = min(adjustment, min(adjust.acceleration_time, adjust.deceleration_time));
    if !(adjustment > 0.0) {
        return;
    }
    let acceleration_time = adjust.acceleration_time - adjustment;
    let peak_velocity = acceleration_time * adjust.acceleration + adjust.initial_velocity;
    adjust.acceleration_time = acceleration_time;
    adjust.deceleration_time -= adjustment;
    adjust.coast_time = ((peak_velocity + peak_velocity + adjustment * adjust.acceleration) * adjustment) / peak_velocity;
}

/// `unit_euler_axis_doplan`: the axis after `delta_time` of its plan, and
/// whether the plan is done.
fn do_plan(plan: &AccelerationPlan, delta_time: f32, position: f32, velocity: f32) -> (bool, f32, f32) {
    let mut result = plan.at_rest;
    let mut current_position = position;
    let mut current_velocity = velocity;
    if result {
        return (result, current_position, current_velocity);
    }
    let mut remaining_time = delta_time;
    if !(remaining_time > 0.0) {
        return (result, current_position, current_velocity);
    }
    if plan.acceleration_time > 0.0 {
        let time = min(remaining_time, plan.acceleration_time);
        current_position += (time * plan.acceleration * 0.5 + current_velocity) * time;
        current_velocity += time * plan.acceleration;
        remaining_time -= time;
    }
    if !(remaining_time > 0.0) {
        return (result, current_position, current_velocity);
    }
    if plan.coast_time > 0.0 {
        let time = min(remaining_time, plan.coast_time);
        current_position += current_velocity * time;
        remaining_time -= time;
    }
    if !(remaining_time > 0.0) {
        return (result, current_position, current_velocity);
    }
    if plan.deceleration_time > 0.0 {
        let time = min(remaining_time, plan.deceleration_time);
        current_position += (0.5 * time * plan.deceleration + current_velocity) * time;
        current_velocity += time * plan.deceleration;
        remaining_time -= time;
    }
    if remaining_time > 0.0 {
        result = true;
    }
    (result, current_position, current_velocity)
}

/// `unit_euler_aiming_update` (with no orientation): `aiming_vector` turned
/// toward `desired_aiming_vector` within `aiming_bounds` (yaw x0..x1, pitch
/// y0..y1), its angular velocity `aiming_velocity` kept within the limits.
pub fn euler_aiming_update(
    aiming_vector: &mut [f32; 3],
    desired_aiming_vector: [f32; 3],
    aiming_velocity: &mut [f32; 3],
    aiming_bounds: [f32; 4],
    angular_velocity_limit: f32,
    angular_acceleration_limit: f32,
) {
    euler_aiming_update_oriented(
        None,
        aiming_vector,
        desired_aiming_vector,
        aiming_velocity,
        aiming_bounds,
        angular_velocity_limit,
        angular_acceleration_limit,
    );
}

/// `matrix4x3_transform_normal` and its inverse, for a rotation.
fn transform_normal(m: &crate::model::Matrix4x3, n: [f32; 3]) -> [f32; 3] {
    let [i, j, k] = n;
    [
        i * m.forward[0] + j * m.left[0] + k * m.up[0],
        i * m.forward[1] + j * m.left[1] + k * m.up[1],
        i * m.forward[2] + j * m.left[2] + k * m.up[2],
    ]
}
fn inverse_transform_normal(m: &crate::model::Matrix4x3, n: [f32; 3]) -> [f32; 3] {
    let [i, j, k] = n;
    [
        i * m.forward[0] + j * m.forward[1] + k * m.forward[2],
        i * m.left[0] + j * m.left[1] + k * m.left[2],
        i * m.up[0] + j * m.up[1] + k * m.up[2],
    ]
}

/// `unit_euler_aiming_update` in an orientation's space (the vectors into
/// it; the clamped desired vector and the new one out of it, normalized).
pub fn euler_aiming_update_oriented(
    orientation: Option<&crate::model::Matrix4x3>,
    aiming_vector: &mut [f32; 3],
    desired_aiming_vector: [f32; 3],
    aiming_velocity: &mut [f32; 3],
    aiming_bounds: [f32; 4],
    angular_velocity_limit: f32,
    angular_acceleration_limit: f32,
) {
    let [x0, x1, y0, y1] = aiming_bounds;
    let (local_aiming_vector, local_desired_aiming_vector) = match orientation {
        Some(m) => (inverse_transform_normal(m, *aiming_vector), inverse_transform_normal(m, desired_aiming_vector)),
        None => (*aiming_vector, desired_aiming_vector),
    };
    let out = |v: [f32; 3]| match orientation {
        Some(m) => {
            let mut v = transform_normal(m, v);
            normalize3(&mut v);
            v
        }
        None => v,
    };
    let yaw_wraps = (x1 - x0) - 2.0 * PI > -EPSILON;
    let aiming_angles = euler_from_vector(local_aiming_vector);
    let mut desired_angles = euler_from_vector(local_desired_aiming_vector);

    let mut yaw_in_bounds = true;
    if yaw_wraps {
        if desired_angles[0] < x0 {
            desired_angles[0] += 2.0 * PI;
        } else if desired_angles[0] > x1 {
            desired_angles[0] -= 2.0 * PI;
        }
    } else if desired_angles[0] < x0 {
        desired_angles[0] = x0;
        yaw_in_bounds = false;
    } else if desired_angles[0] > x1 {
        desired_angles[0] = x1;
        yaw_in_bounds = false;
    }
    let clamped_desired = if desired_angles[1] < y0 {
        desired_angles[1] = y0;
        out(vector_from_euler(desired_angles))
    } else if desired_angles[1] > y1 {
        desired_angles[1] = y1;
        out(vector_from_euler(desired_angles))
    } else if yaw_in_bounds {
        desired_aiming_vector
    } else {
        out(vector_from_euler(desired_angles))
    };

    let mut velocity_axis = *aiming_velocity;
    let angular_speed = normalize3(&mut velocity_axis);
    let angular_velocity = if angular_speed != 0.0 {
        let rotated = rotate_vector_about_axis(local_aiming_vector, velocity_axis, sin(angular_speed), cos(angular_speed));
        let rotated_angles = euler_from_vector(rotated);
        [rotated_angles[0] - aiming_angles[0], rotated_angles[1] - aiming_angles[1]]
    } else {
        [0.0, 0.0]
    };

    let mut error = [aiming_angles[0] - desired_angles[0], aiming_angles[1] - desired_angles[1]];
    if yaw_wraps {
        if error[0] > PI {
            error[0] -= 2.0 * PI;
        } else if error[0] < -PI {
            error[0] += 2.0 * PI;
        }
    }

    let mut yaw_plan = build_plan(error[0], angular_velocity[0], angular_velocity_limit, angular_acceleration_limit);
    let mut pitch_plan = build_plan(error[1], angular_velocity[1], angular_velocity_limit, angular_acceleration_limit);
    couple_plans(&mut yaw_plan, &mut pitch_plan, angular_acceleration_limit);
    let (yaw_complete, yaw_error, yaw_velocity) = do_plan(&yaw_plan, 1.0, error[0], angular_velocity[0]);
    let (pitch_complete, pitch_error, pitch_velocity) = do_plan(&pitch_plan, 1.0, error[1], angular_velocity[1]);

    if yaw_complete && pitch_complete {
        *aiming_vector = clamped_desired;
        *aiming_velocity = [0.0; 3];
    } else {
        let mut end = [yaw_error + desired_angles[0], pitch_error + desired_angles[1]];
        if yaw_wraps {
            if end[0] < x0 {
                end[0] += 2.0 * PI;
            } else if end[0] > x1 {
                end[0] -= 2.0 * PI;
            }
        } else {
            end[0] = pin(end[0], x0, x1);
        }
        end[1] = pin(end[1], y0, y1);
        let end_vector = vector_from_euler(end);
        let further_vector = vector_from_euler([yaw_velocity + end[0], pitch_velocity + end[1]]);
        let dot = pin(dot3(end_vector, further_vector), -1.0, 1.0);
        *aiming_velocity = cross3(end_vector, further_vector);
        normalize3(aiming_velocity);
        let angle = min((dot as f64).acos() as f32, angular_velocity_limit);
        *aiming_velocity = scale3(*aiming_velocity, angle);
        *aiming_vector = out(end_vector);
    }
}

// ---------- a player's melee

/// A player's melee with no weapon (the port's: a gametype's loadout of
/// none), in a weapon's usual time, the hit halfway.
const UNARMED_MELEE_TICKS: i8 = 16;
/// `_first_person_weapon_animation_melee`.
const FIRST_PERSON_WEAPON_ANIMATION_MELEE: usize = 13;
/// The weapon definition's "prevents melee attack" flag.
const WEAPON_PREVENTS_MELEE_ATTACK: u32 = 1 << 9;

/// What a player's melee reads of a weapon's definition: whether it
/// prevents melee, and its first-person melee animation's frame count and
/// key frame (`weapon_get_first_person_animation_time`, 0 for none).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct MeleeWeapon {
    pub prevents_melee_attack: bool,
    pub frame_count: i16,
    pub key_frame_index: i16,
}

impl MeleeWeapon {
    pub fn load(cache: &crate::CacheFile, datum: u32) -> crate::Result<Self> {
        use crate::layout::{
            _weapon_definition as w, animation as a, animation_graph as g, animation_graph_first_person_weapon_animations as f,
            weapon_definition, weapon_interface_definition as i,
        };
        let root = cache.tag_root(datum, crate::groups::OBJECT)?;
        let weapon = root.add(weapon_definition::WEAPON);
        let mut melee = MeleeWeapon { prevents_melee_attack: weapon.u32(w::FLAGS)? & WEAPON_PREVENTS_MELEE_ATTACK != 0, ..Default::default() };
        let Some(graph_index) = weapon.reference(w::INTERFACE_DEFINITION + i::FIRST_PERSON_ANIMATIONS)?.datum else { return Ok(melee) };
        let graph = cache.tag_root(graph_index, crate::groups::ANIMATION_GRAPH)?;
        let Some(first) = graph.block(g::FIRST_PERSON_WEAPON_ANIMATIONS, f::SIZEOF)?.iter().next() else { return Ok(melee) };
        let Some(animation_index) = first.block(f::ANIMATIONS, 2)?.iter().nth(FIRST_PERSON_WEAPON_ANIMATION_MELEE).map(|p| p.i16(0)).transpose()? else {
            return Ok(melee);
        };
        if animation_index == -1 {
            return Ok(melee);
        }
        if let Some(animation) = graph.block(g::ANIMATIONS, a::SIZEOF)?.get(animation_index as usize) {
            melee.frame_count = animation.i16(a::FRAME_COUNT)?;
            melee.key_frame_index = animation.i16(a::PRIVATE_KEY_FRAME_INDEX)?;
        }
        Ok(melee)
    }
}

/// What a player's melee step reads besides its counters.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct PlayerMeleeInputs {
    /// The player asks to melee (`_unit_control_use_equipment_bit`).
    pub use_equipment: bool,
    /// It rides in a seat, and zooms.
    pub in_seat: bool,
    pub zoomed: bool,
    /// Its weapon, and whether the weapon is overcharged (which prevents
    /// melee too); `None` for no weapon.
    pub weapon: Option<(MeleeWeapon, bool)>,
}

/// What a player's melee step did.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct PlayerMeleeOutcome {
    /// A melee started: the unit's melee action
    /// ([`crate::unit_animation::unit_animation_start_action`]), the
    /// weapon's reload stopped and the first-person weapon told.
    pub started: bool,
    /// Its damage landed this tick (`unit_cause_player_melee_damage`).
    pub damaged: bool,
}

/// `biped_update`'s player melee step: with no melee under way, a player
/// asking for one (with a weapon that allows it, or with none outside a
/// seat; not zoomed) starts one, lasting the first-person melee
/// animation's frames with the hit at its key frame, both sped up by a
/// quarter; under way, the damage lands at its tick and the counter runs
/// down.
pub fn player_melee(inputs: &PlayerMeleeInputs, ticks: &mut i8, attack_tick: &mut i8) -> PlayerMeleeOutcome {
    let mut outcome = PlayerMeleeOutcome::default();
    if *ticks == 0 {
        if !inputs.use_equipment {
            return outcome;
        }
        let allowed = match inputs.weapon {
            None => !inputs.in_seat,
            Some((weapon, overcharged)) => !(weapon.prevents_melee_attack || overcharged),
        };
        if allowed && !inputs.zoomed {
            outcome.started = true;
            match inputs.weapon {
                None => {
                    *ticks = UNARMED_MELEE_TICKS;
                    *attack_tick = UNARMED_MELEE_TICKS / 2;
                }
                Some((weapon, _)) => {
                    *ticks = weapon.frame_count as i8;
                    *attack_tick = (*ticks as i32 - weapon.key_frame_index as i32) as i8;
                }
            }
            let speedup = (*ticks as i32) >> 2;
            *ticks = (*ticks as i32 - speedup) as i8;
            *attack_tick = (*attack_tick as i32 - speedup) as i8;
        }
    } else {
        if *ticks == *attack_tick {
            outcome.damaged = true;
        }
        *ticks = ticks.wrapping_sub(1);
    }
    outcome
}
