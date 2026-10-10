//! Halo's unit poses (ROADMAP phase 7, objects and physics): what a unit's
//! type does to the pose its animation gives it before its node matrices
//! are built (`unit_preprocess_node_orientations` in OpenCE's
//! `source/units/units.c`), ported as it is, in the same single-precision
//! arithmetic.
//!
//! Over the base animation go the action (a replacement: a reload, a
//! melee), the overlay action and the soft ping (a flinch), then the seat's
//! emotion, mouth and acceleration overlays, and last the aiming screen
//! (the weapon class's grid of poses blended at where the unit aims,
//! relative to its body) and the looking screen (the head, at where it
//! looks relative to where it aims).
//!
//! The port's own addition comes first: a unit throwing a grenade on the
//! move keeps its legs in the walk (blended by its speed and eased in and
//! out a step a tick), floating in the air, or a rider's seated pose.

use crate::features::{cross3, dot3};
use crate::pose::{
    Orientation, PoseAnimation, PoseModel, ScreenBounds, aiming_screen_apply, animation_node_orientations, orientations_blend,
    overlay_animation_apply, overlay_animation_apply_continuous, overlay_animation_apply_scaled, replacement_animation_apply,
};
use crate::unit_animation::AnimationGraph;

const NONE: i16 = -1;
const UNIT_ANIMATION_SHOWING_ACCELERATION: u16 = 1 << 1;
const MAXIMUM_NODES_PER_MODEL: usize = 64;

// `_unit_state`s the preprocess tests
const STATE_HARD_PING: i8 = 23;
const STATE_RESURRECT_BACK: i8 = 35;
const STATE_THROW_GRENADE: i8 = 33;
const STATE_LEAP_MELEE: i8 = 41;

// `_unit_seat_animation`s
const SEAT_ANIMATION_ACCELERATION_FRONT_BACK: usize = 2;
const SEAT_ANIMATION_PUSH_IMPACT: usize = 5;
const SEAT_ANIMATION_MOUTH_APERTURE: usize = 10;
const SEAT_ANIMATION_EMOTIONS: usize = 11;

// `_unit_weapon_class_animation`s
const WEAPON_CLASS_ANIMATION_IDLE: usize = 0;
const WEAPON_CLASS_ANIMATION_MOVING_FRONT: usize = 8;
const WEAPON_CLASS_ANIMATION_MOVING_BACK: usize = 9;
const WEAPON_CLASS_ANIMATION_MOVING_LEFT: usize = 10;
const WEAPON_CLASS_ANIMATION_MOVING_RIGHT: usize = 11;
const WEAPON_CLASS_ANIMATION_AIRBORNE: usize = 16;

const BASE_SEAT_CROUCH: i8 = 3;

/// The speed (world units a second) at and above which a throwing unit's
/// legs move fully.
const GRENADE_THROW_FULL_MOVE_SPEED: f32 = 0.75;
/// The ticks the legs take to blend fully in or out.
const GRENADE_THROW_BLEND_TICKS: i32 = 6;
const TICKS_PER_SECOND: i32 = 30;

/// What a unit's preprocess reads of it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct UnitPoseInputs {
    /// The unit definition's flags (`_unit_has_no_aiming_bit`,
    /// `_unit_simple_creature_bit`).
    pub has_no_aiming: bool,
    pub simple_creature: bool,
    /// `unit_animation_enhanced` (the port's animations for a grenade
    /// throw on the move).
    pub enhanced: bool,
    /// `unit_flying_through_air`.
    pub airborne: bool,
    pub parented: bool,
    pub has_weapon: bool,
    pub player: bool,
    pub game_time: i32,
    pub animation_flags: u16,
    pub aiming_screen_index: i16,
    pub looking_screen_index: i16,
    pub override_emotion_animation_index: i16,
    pub seat_index: i8,
    pub weapon_index: i8,
    pub state: i8,
    pub action: i8,
    pub base_seat_index: i8,
    pub emotion_index: i8,
    /// (Animation, frame.)
    pub action_animation: (i16, i16),
    pub overlay_action_animation: (i16, i16),
    pub soft_ping_animation: (i16, i16),
    pub mouth_aperture: f32,
    pub seat_acceleration: [f32; 3],
    pub aiming_vector: [f32; 3],
    pub looking_vector: [f32; 3],
    /// Its orientation as `object_get_orientation` gives it (through its
    /// parent's node, for a rider).
    pub orientation_forward: [f32; 3],
    pub orientation_up: [f32; 3],
    /// Its own axes and velocity.
    pub forward: [f32; 3],
    pub up: [f32; 3],
    pub translational_velocity: [f32; 3],
}

/// The weights a grenade throw on the move eases the legs by, and the game
/// times they were eased at (the port keeps them per unit).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct GrenadeThrowLegs {
    pub leg_weight: f32,
    pub leg_time: i32,
    pub air_weight: f32,
    pub air_time: i32,
}

/// What the preprocess sets of the unit's animation: whether it aims and
/// looks with euler screens, and the rectangles of angles they cover
/// (x0, x1, y0, y1; `None` for those it doesn't set).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct UnitPoseOutputs {
    pub aiming_with_euler_screen: bool,
    pub looking_with_euler_screen: bool,
    pub aiming_screen_bounds: Option<[f32; 4]>,
    pub looking_screen_bounds: Option<[f32; 4]>,
}

/// `unit_animation_aiming_screen`: whether the unit's state aims.
fn aiming_screen(state: i8, action: i8) -> bool {
    let mut result = action == 0;
    if state >= STATE_HARD_PING && (state <= STATE_RESURRECT_BACK || state == STATE_LEAP_MELEE) {
        result = false;
    }
    result
}

/// `euler_angles2d_from_vector3d`: (yaw, pitch).
fn euler_angles2d(v: [f32; 3]) -> (f32, f32) {
    let yaw = (v[1] as f64).atan2(v[0] as f64) as f32;
    let pitch = (v[2] as f64).atan2((v[0] * v[0] + v[1] * v[1]).sqrt() as f64) as f32;
    (yaw, pitch)
}

/// `matrix4x3_inverse_transform_normal` by the unit's axes (the left from
/// up × forward).
fn relative_to(forward: [f32; 3], up: [f32; 3], v: [f32; 3]) -> [f32; 3] {
    let left = cross3(up, forward);
    let [i, j, k] = v;
    [
        i * forward[0] + j * forward[1] + k * forward[2],
        i * left[0] + j * left[1] + k * left[2],
        i * up[0] + j * up[1] + k * up[2],
    ]
}

/// `unit_node_is_at_or_below`.
fn node_is_at_or_below(model: &PoseModel, node_index: usize, ancestor: usize) -> bool {
    let mut node = node_index as i16;
    let mut guard = 0;
    while node != NONE && guard <= model.nodes.len() {
        if node as usize == ancestor {
            return true;
        }
        node = model.nodes.get(node as usize).map_or(NONE, |n| n.parent);
        guard += 1;
    }
    false
}

/// `unit_grenade_throw_ease`: a weight eased toward its target a step a
/// tick; a new throw starts at the target.
fn ease(weight: &mut f32, time: &mut i32, now: i32, target: f32) -> f32 {
    if *time == now {
        return *weight;
    }
    let mut result = target;
    if *time == now - 1 {
        let step = 1.0 / GRENADE_THROW_BLEND_TICKS as f32;
        let difference = target - *weight;
        // (PIN)
        result = *weight + if difference < -step { -step } else if difference > step { step } else { difference };
    }
    *weight = result;
    *time = now;
    result
}

/// `unit_animation_pose_at_phase`: an animation's pose at a point (0 to 1)
/// through its cycle.
fn pose_at_phase(model: &PoseModel, animation: &PoseAnimation, phase: f32) -> Vec<Orientation> {
    let frame_index = (phase * animation.frame_count as f32) as i32;
    // (PIN)
    let last = animation.frame_count as i32 - 1;
    let frame_index = if frame_index < 0 { 0 } else if frame_index > last { last } else { frame_index };
    let mut out = animation_node_orientations(model, animation, frame_index as i16);
    out.resize(model.nodes.len(), Orientation::default());
    out
}

/// The phase of a cycle at the game time.
fn cycle_phase(now: i32, frame_count: i16) -> f32 {
    (now % frame_count as i32) as f32 / frame_count as f32
}

/// `unit_grenade_throw_keep_legs_moving` (the port's).
fn grenade_throw_keep_legs_moving<'a>(
    model: &PoseModel,
    graph: &AnimationGraph,
    animation: &dyn Fn(i16) -> Option<&'a PoseAnimation>,
    inputs: &UnitPoseInputs,
    legs_state: &mut GrenadeThrowLegs,
    orientations: &mut [Orientation],
) {
    if inputs.state != STATE_THROW_GRENADE || inputs.seat_index as i16 == NONE || !inputs.enhanced {
        return;
    }
    let Some(seat) = graph.unit_seats.get(inputs.seat_index as usize) else { return };
    let Some(weapon_class) = seat.weapon_classes.get(inputs.weapon_index as usize) else { return };
    let now = inputs.game_time;
    let node_count = model.nodes.len();
    if node_count > MAXIMUM_NODES_PER_MODEL {
        return;
    }
    let Some(spine) = model.find_node("bip01 spine") else { return };
    // `unit_weapon_class_base_animation`
    let base = |slot: usize| -> Option<&'a PoseAnimation> {
        let index = *weapon_class.animations.get(slot)?;
        if index == NONE {
            return None;
        }
        let a = animation(index)?;
        (a.is_base() && a.frame_count >= 1 && a.node_count as usize == node_count).then_some(a)
    };

    // a rider keeps the seat's idle pose below the spine
    if inputs.parented {
        if let Some(seated) = base(WEAPON_CLASS_ANIMATION_IDLE) {
            let rest = pose_at_phase(model, seated, cycle_phase(now, seated.frame_count));
            for node_index in 0..node_count {
                if !node_is_at_or_below(model, node_index, spine) {
                    orientations[node_index] = rest[node_index];
                }
            }
        }
        return;
    }

    let airborne = inputs.airborne;
    let air_weight = ease(&mut legs_state.air_weight, &mut legs_state.air_time, now, if airborne { 1.0 } else { 0.0 });

    let left = cross3(inputs.up, inputs.forward);
    let forward_speed = dot3(inputs.translational_velocity, inputs.forward) * TICKS_PER_SECOND as f32;
    let left_speed = dot3(inputs.translational_velocity, left) * TICKS_PER_SECOND as f32;
    let mut target_weight = (forward_speed * forward_speed + left_speed * left_speed).sqrt() / GRENADE_THROW_FULL_MOVE_SPEED;
    target_weight = if airborne {
        0.0
    } else if target_weight < 0.0 {
        0.0
    } else if target_weight > 1.0 {
        1.0
    } else {
        target_weight
    };
    let move_weight = ease(&mut legs_state.leg_weight, &mut legs_state.leg_time, now, target_weight);

    let crouching = inputs.base_seat_index == BASE_SEAT_CROUCH;
    let idle_animation = if crouching { base(WEAPON_CLASS_ANIMATION_IDLE) } else { None };
    if idle_animation.is_none() && move_weight <= 0.0 && air_weight <= 0.0 {
        return;
    }
    let rest = match idle_animation {
        Some(idle) => pose_at_phase(model, idle, cycle_phase(now, idle.frame_count)),
        None => orientations[..node_count].to_vec(),
    };

    let front_animation = base(WEAPON_CLASS_ANIMATION_MOVING_FRONT);
    let phase = front_animation.map_or(0.0, |a| cycle_phase(now, a.frame_count));
    let forward_animation =
        base(if forward_speed >= 0.0 { WEAPON_CLASS_ANIMATION_MOVING_FRONT } else { WEAPON_CLASS_ANIMATION_MOVING_BACK });
    let side_animation = base(if left_speed > 0.0 { WEAPON_CLASS_ANIMATION_MOVING_LEFT } else { WEAPON_CLASS_ANIMATION_MOVING_RIGHT });
    let side_total = ((forward_speed as f64).abs() + (left_speed as f64).abs()) as f32;
    let side_weight = if side_total > 0.0 { (left_speed as f64).abs() as f32 / side_total } else { 0.0 };

    let mut moving = None;
    if move_weight > 0.0 {
        moving = match (forward_animation, side_animation) {
            (Some(f), Some(s)) => {
                let forward = pose_at_phase(model, f, phase);
                let side = pose_at_phase(model, s, phase);
                Some(orientations_blend(&forward, &side, side_weight, node_count))
            }
            (Some(f), None) => Some(pose_at_phase(model, f, phase)),
            (None, Some(s)) => Some(pose_at_phase(model, s, phase)),
            (None, None) => None,
        };
    }
    let mut legs = match moving {
        Some(m) => orientations_blend(&rest, &m, move_weight, node_count),
        None => rest,
    };

    let air_animation = if air_weight > 0.0 { base(WEAPON_CLASS_ANIMATION_AIRBORNE) } else { None };
    if let Some(air) = air_animation {
        let air_pose = pose_at_phase(model, air, cycle_phase(now, air.frame_count));
        legs = orientations_blend(&legs, &air_pose, air_weight, node_count);
    }

    for node_index in 0..node_count {
        if !node_is_at_or_below(model, node_index, spine) {
            orientations[node_index] = legs[node_index];
        }
    }
}

/// `unit_preprocess_node_orientations`: the unit's actions, overlays and
/// aiming and looking screens over its animation's pose. `animation` gives
/// the graph's animation of an index (`None` for one it doesn't have);
/// those with more nodes than the model are skipped
/// (`unit_animation_get_fitting`).
pub fn unit_preprocess_node_orientations<'a>(
    model: &PoseModel,
    graph: &AnimationGraph,
    animation: &dyn Fn(i16) -> Option<&'a PoseAnimation>,
    inputs: &UnitPoseInputs,
    legs_state: &mut GrenadeThrowLegs,
    orientations: &mut [Orientation],
) -> UnitPoseOutputs {
    let model_node_count = model.nodes.len().min(MAXIMUM_NODES_PER_MODEL) as i16;
    let fitting = |index: i16| -> Option<&'a PoseAnimation> { animation(index).filter(|a| a.node_count <= model_node_count) };
    grenade_throw_keep_legs_moving(model, graph, animation, inputs, legs_state, orientations);

    let (index, frame) = inputs.action_animation;
    if let Some(a) = (index != NONE).then(|| fitting(index)).flatten() {
        replacement_animation_apply(a, frame, orientations);
    }
    let (index, frame) = inputs.overlay_action_animation;
    if let Some(a) = (index != NONE).then(|| fitting(index)).flatten() {
        overlay_animation_apply(a, frame, orientations);
    }
    let (index, frame) = inputs.soft_ping_animation;
    if let Some(a) = (index != NONE).then(|| fitting(index)).flatten() {
        overlay_animation_apply(a, frame, orientations);
    }

    let mut outputs = UnitPoseOutputs::default();
    if inputs.simple_creature || inputs.seat_index as i16 == NONE {
        return outputs;
    }
    let Some(seat) = graph.unit_seats.get(inputs.seat_index as usize) else { return outputs };
    let seat_animation = |slot: usize| seat.animations.get(slot).copied();

    if inputs.emotion_index as i16 != NONE {
        let mut animation_index = seat_animation(SEAT_ANIMATION_EMOTIONS).unwrap_or(NONE);
        if inputs.override_emotion_animation_index != NONE {
            animation_index = inputs.override_emotion_animation_index;
        }
        if let Some(a) = (animation_index != NONE).then(|| fitting(animation_index)).flatten() {
            if inputs.emotion_index >= 0 && (inputs.emotion_index as i16) < a.frame_count {
                overlay_animation_apply(a, inputs.emotion_index as i16, orientations);
            }
        }
    }

    if inputs.mouth_aperture > 0.0 {
        if let Some(animation_index) = seat_animation(SEAT_ANIMATION_MOUTH_APERTURE) {
            if let Some(a) = (animation_index != NONE).then(|| fitting(animation_index)).flatten() {
                overlay_animation_apply_scaled(a, 0, inputs.mouth_aperture, orientations);
            }
        }
    }

    if inputs.animation_flags & UNIT_ANIMATION_SHOWING_ACCELERATION != 0 {
        for acceleration_index in 0..SEAT_ANIMATION_PUSH_IMPACT - SEAT_ANIMATION_ACCELERATION_FRONT_BACK {
            let Some(animation_index) = seat_animation(SEAT_ANIMATION_ACCELERATION_FRONT_BACK + acceleration_index) else { continue };
            if let Some(a) = (animation_index != NONE).then(|| fitting(animation_index)).flatten() {
                let frame = (a.frame_count as i32 - 1) as f32 * inputs.seat_acceleration[acceleration_index];
                overlay_animation_apply_continuous(a, frame, orientations);
            }
        }
    }

    if !inputs.has_no_aiming && aiming_screen(inputs.state, inputs.action) {
        let Some(weapon_class) = seat.weapon_classes.get(inputs.weapon_index as usize) else { return outputs };
        let mut relative_aiming = (0.0f32, 0.0f32);
        let index = inputs.aiming_screen_index;
        if let Some(a) = (index != NONE).then(|| fitting(index)).flatten() {
            let bounds: &ScreenBounds = &weapon_class.aiming_screen_bounds;
            let relative = relative_to(inputs.orientation_forward, inputs.orientation_up, inputs.aiming_vector);
            relative_aiming = euler_angles2d(relative);
            outputs.aiming_with_euler_screen = true;
            outputs.aiming_screen_bounds = Some(bounds.rectangle());
            aiming_screen_apply(a, bounds, relative_aiming.0, relative_aiming.1, orientations);
        }
        if inputs.has_weapon || inputs.player {
            let index = inputs.looking_screen_index;
            if let Some(a) = (index != NONE).then(|| fitting(index)).flatten() {
                let bounds = &seat.looking_screen_bounds;
                let relative = relative_to(inputs.orientation_forward, inputs.orientation_up, inputs.looking_vector);
                let (mut yaw, mut pitch) = euler_angles2d(relative);
                yaw -= relative_aiming.0;
                pitch -= relative_aiming.1;
                outputs.looking_with_euler_screen = true;
                outputs.looking_screen_bounds = Some(bounds.rectangle());
                aiming_screen_apply(a, bounds, yaw, pitch, orientations);
            }
        }
    }
    outputs
}
