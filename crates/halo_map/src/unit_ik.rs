//! A unit's inverse kinematics (ROADMAP phase 7): units.c's
//! `unit_postprocess_node_matrices` (the only type's postprocess of its node
//! matrices), with objects.c's `object_inverse_kinematics` and
//! model_animations.c's `inverse_kinematics_adjust_matrices`, ported as they
//! are; and objects.c's `object_connect_lights`' choice of lights.
//!
//! A unit that isn't a simple creature, in a seat: riding a vehicle (unless
//! its animation lets go, or a grenade throw releases it), its hands on the
//! vehicle's markers the seat's points name; and holding a weapon (unless
//! its animation lets go), its hands on the weapon's markers its weapon
//! class's points name. Each point moves the hand's node onto where the
//! grip's marker puts it, bending the arm (its shoulder and elbow nodes,
//! the hand's parent's parent and parent) to reach.

// (the C's component-by-component arithmetic)
#![allow(clippy::needless_range_loop)]

use crate::collision::matrix_inverse;
use crate::markers::{ModelMarkers, normalize3, object_marker_by_name};
use crate::model::Matrix4x3;
use crate::pose::matrix_multiply;
use crate::unit_animation::AnimationGraph;

const NONE: i32 = -1;

// unit states
const HARD_PING: i8 = 23;
const DYING_AIRBORNE: i8 = 24;
const DYING: i8 = 25;
const ENTERING_SEAT: i8 = 26;
const EXITING_SEAT: i8 = 27;
const USER_ANIMATION: i8 = 28;
const AI_IMPULSE: i8 = 29;
const MELEE_ATTACK: i8 = 30;
const MELEE_AIRBORNE: i8 = 31;
const MELEE_CONTINUOUS: i8 = 32;
const THROW_GRENADE: i8 = 33;
const RESURRECT_FRONT: i8 = 34;
const RESURRECT_BACK: i8 = 35;
const LEAP_START: i8 = 39;
const LEAP_AIRBORNE: i8 = 40;
const LEAP_MELEE: i8 = 41;
const FLYING_FRONT: i8 = 16;
const FLYING_RIGHT: i8 = 19;

/// What a unit's inverse kinematics read of it.
#[derive(Debug, Clone, Copy, Default)]
pub struct IkInputs {
    pub simple_creature: bool,
    pub seat_index: i16,
    pub weapon_class_index: i16,
    pub parent_object_index: i32,
    pub animation_state: i8,
    pub action: i8,
    pub soft_ping_animation_index: i16,
    /// Its current weapon's slot (NONE for none), and the weapon.
    pub current_weapon_slot: i16,
    pub current_weapon_object_index: i32,
    pub region_permutations: [u8; 8],
    pub mirrored: bool,
}

/// An object a hand grips: its markers and pose.
#[derive(Debug, Clone, Default)]
pub struct GripObject {
    pub markers: Option<std::sync::Arc<ModelMarkers>>,
    pub node_matrices: Vec<Matrix4x3>,
    pub region_permutations: [u8; 8],
    pub mirrored: bool,
}

/// A running game, as a unit's inverse kinematics ask it.
pub trait IkHost {
    /// `unit_grenade_throw_releases_vehicle_ik` (which keeps its own record).
    fn grenade_throw_releases_vehicle_ik(&self) -> bool;
    fn grip_object(&self, index: i32) -> Option<GripObject>;
}

/// `unit_animation_vehicle_ik`.
fn vehicle_ik(state: i8) -> bool {
    !matches!(state, HARD_PING | DYING_AIRBORNE | DYING | ENTERING_SEAT | EXITING_SEAT | AI_IMPULSE | RESURRECT_FRONT | RESURRECT_BACK)
}

/// `unit_animation_weapon_ik`.
fn weapon_ik(inputs: &IkInputs) -> bool {
    let mut result = inputs.soft_ping_animation_index as i32 == NONE;
    if inputs.action != 0 {
        result = false;
    }
    if matches!(
        inputs.animation_state,
        FLYING_FRONT..=FLYING_RIGHT
            | HARD_PING
            | DYING_AIRBORNE
            | DYING
            | ENTERING_SEAT
            | EXITING_SEAT
            | USER_ANIMATION
            | AI_IMPULSE
            | MELEE_ATTACK
            | MELEE_AIRBORNE
            | MELEE_CONTINUOUS
            | THROW_GRENADE
            | RESURRECT_FRONT
            | RESURRECT_BACK
            | LEAP_START
            | LEAP_AIRBORNE
            | LEAP_MELEE
    ) {
        result = false;
    }
    result
}

fn sub3(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn cross3(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

/// `distance3d`.
fn distance3(a: [f32; 3], b: [f32; 3]) -> f32 {
    let d = sub3(b, a);
    (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt()
}

/// `inverse_kinematics_adjust_matrices`: the shoulder and elbow bent so the
/// hand reaches the desired matrix (98% of the arm's length at most), and
/// the hand put there.
pub fn inverse_kinematics_adjust_matrices(
    desired_hand: &mut Matrix4x3,
    shoulder: &mut Matrix4x3,
    elbow: &mut Matrix4x3,
    hand: &mut Matrix4x3,
) {
    let upper_arm_length = distance3(shoulder.position, elbow.position);
    let forearm_length = distance3(elbow.position, hand.position);
    let mut hand_distance = distance3(desired_hand.position, shoulder.position);
    let shoulder_to_elbow = sub3(elbow.position, shoulder.position);
    let inverse_hand_distance = 1.0 / hand_distance;
    let hand_direction = [
        (desired_hand.position[0] - shoulder.position[0]) * inverse_hand_distance,
        (desired_hand.position[1] - shoulder.position[1]) * inverse_hand_distance,
        (desired_hand.position[2] - shoulder.position[2]) * inverse_hand_distance,
    ];
    let mut bend_plane_normal = cross3(hand_direction, shoulder_to_elbow);
    normalize3(&mut bend_plane_normal);
    let bend_direction = cross3(bend_plane_normal, hand_direction);

    let maximum_reach = (upper_arm_length + forearm_length) * 0.98;
    if maximum_reach < hand_distance {
        for c in 0..3 {
            desired_hand.position[c] = shoulder.position[c] + hand_direction[c] * maximum_reach;
        }
        hand_distance = maximum_reach;
    }

    let upper_arm_length_squared = upper_arm_length * upper_arm_length;
    let elbow_projection =
        (hand_distance * hand_distance + upper_arm_length_squared - forearm_length * forearm_length) / (hand_distance + hand_distance);
    let elbow_remainder = hand_distance - elbow_projection;
    let elbow_height = (upper_arm_length_squared - elbow_projection * elbow_projection).sqrt();

    for c in 0..3 {
        shoulder.forward[c] = elbow_projection * hand_direction[c] + elbow_height * bend_direction[c];
    }
    normalize3(&mut shoulder.forward);
    shoulder.up = cross3(shoulder.forward, shoulder.left);
    normalize3(&mut shoulder.up);
    shoulder.left = cross3(shoulder.up, shoulder.forward);
    let mut new_elbow_position = [0.0; 3];
    for c in 0..3 {
        new_elbow_position[c] = shoulder.position[c] + shoulder.forward[c] * upper_arm_length;
    }

    for c in 0..3 {
        elbow.forward[c] = elbow_remainder * hand_direction[c] - elbow_height * bend_direction[c];
    }
    normalize3(&mut elbow.forward);
    elbow.up = cross3(elbow.forward, elbow.left);
    normalize3(&mut elbow.up);
    elbow.left = cross3(elbow.up, elbow.forward);
    elbow.position = new_elbow_position;

    *hand = *desired_hand;
}

/// The unit's own model, as its hands read it: its markers and its nodes'
/// parents.
pub struct HandModel<'a> {
    pub markers: Option<&'a ModelMarkers>,
    pub node_parents: &'a [i16],
}

/// `object_inverse_kinematics`: the unit's hand marker moved onto the grip
/// object's marker.
fn object_inverse_kinematics(
    hand: &HandModel,
    inputs: &IkInputs,
    hand_marker_name: &str,
    grip: Option<&GripObject>,
    grip_marker_name: &str,
    node_matrices: &mut [Matrix4x3],
) {
    let Some(hand_marker) = object_marker_by_name(hand.markers, hand_marker_name, &inputs.region_permutations, node_matrices, inputs.mirrored)
    else {
        return;
    };
    let Some(grip) = grip else { return };
    let Some(grip_marker) =
        object_marker_by_name(grip.markers.as_deref(), grip_marker_name, &grip.region_permutations, &grip.node_matrices, grip.mirrored)
    else {
        return;
    };
    let hand_node_index = hand_marker.node_index as usize;
    let Some(&hand_parent_node_index) = hand.node_parents.get(hand_node_index) else { return };
    let desired_hand_index = if hand_parent_node_index as i32 != NONE {
        hand.node_parents.get(hand_parent_node_index as usize).copied().unwrap_or(NONE as i16)
    } else {
        NONE as i16
    };
    if desired_hand_index as i32 == NONE {
        return;
    }
    let (shoulder_index, elbow_index) = (desired_hand_index as usize, hand_parent_node_index as usize);
    if shoulder_index >= node_matrices.len() || elbow_index >= node_matrices.len() || hand_node_index >= node_matrices.len() {
        return;
    }
    let mut desired_hand_matrix = matrix_inverse(&hand_marker.node_matrix);
    desired_hand_matrix = matrix_multiply(&grip_marker.matrix, &desired_hand_matrix);
    let mut shoulder = node_matrices[shoulder_index];
    let mut elbow = node_matrices[elbow_index];
    let mut hand_matrix = node_matrices[hand_node_index];
    inverse_kinematics_adjust_matrices(&mut desired_hand_matrix, &mut shoulder, &mut elbow, &mut hand_matrix);
    node_matrices[shoulder_index] = shoulder;
    node_matrices[elbow_index] = elbow;
    node_matrices[hand_node_index] = hand_matrix;
}

/// `unit_postprocess_node_matrices`: the unit's node matrices adjusted in
/// place. Returns whether its weapon's inverse kinematics ran (the game then
/// clears its animation's postponing of them).
pub fn unit_postprocess_node_matrices(
    graph: Option<&AnimationGraph>,
    hand: &HandModel,
    inputs: &IkInputs,
    host: &dyn IkHost,
    node_matrices: &mut [Matrix4x3],
) -> bool {
    if inputs.simple_creature || inputs.seat_index as i32 == NONE {
        return false;
    }
    let Some(seat) = graph.and_then(|g| usize::try_from(inputs.seat_index).ok().and_then(|i| g.unit_seats.get(i))) else {
        return false;
    };
    let weapon_class = usize::try_from(inputs.weapon_class_index).ok().and_then(|i| seat.weapon_classes.get(i));

    if inputs.parent_object_index != NONE && vehicle_ik(inputs.animation_state) && !host.grenade_throw_releases_vehicle_ik() {
        let grip = host.grip_object(inputs.parent_object_index);
        for (marker_name, attached_to_marker_name) in &seat.ik_points {
            object_inverse_kinematics(hand, inputs, marker_name, grip.as_ref(), attached_to_marker_name, node_matrices);
        }
    }

    if inputs.current_weapon_slot as i32 != NONE && weapon_ik(inputs) {
        let grip = host.grip_object(inputs.current_weapon_object_index);
        if let Some(weapon_class) = weapon_class {
            for (marker_name, attached_to_marker_name) in &weapon_class.ik_points {
                object_inverse_kinematics(hand, inputs, marker_name, grip.as_ref(), attached_to_marker_name, node_matrices);
            }
        }
        return true;
    }
    false
}

/// `object_connect_lights`' lights: the object's light attachments (types
/// 0) that are there, if it has attached lights (its flags' bit 8).
pub fn object_lights(object_flags: u32, attachment_count: usize, attachment_types: &[i8; 8], attachment_indices: &[i32; 8]) -> Vec<i32> {
    const OBJECT_HAS_ATTACHED_LIGHTS: u32 = 1 << 8;
    if object_flags & OBJECT_HAS_ATTACHED_LIGHTS == 0 {
        return Vec::new();
    }
    (0..attachment_count.min(8)).filter(|&i| attachment_types[i] == 0 && attachment_indices[i] != NONE).map(|i| attachment_indices[i]).collect()
}
