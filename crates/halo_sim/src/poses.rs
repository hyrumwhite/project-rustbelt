//! Poses (`object_compute_node_matrices`): the orientations from the
//! animation, overlays and scale, and the interpolation and node matrices.
//! The object types' preprocess between them is still the game's.

use halo_map::model::Matrix4x3;
use halo_map::pose::{self, Orientation};

use crate::TAGS;

const NONE16: i16 = -1;
/// `_object_animates_automatically_bit`, `_object_mirrored_bit`.
const OBJECT_ANIMATES_AUTOMATICALLY: u32 = 1 << 7;
const OBJECT_MIRRORED: u32 = 1 << 12;

/// What the orientations of a pose read (`object_compute_node_matrices`,
/// up to the object type's preprocess).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct PoseOrientationsCall {
    pub object_index: u32,
    pub definition_index: u32,
    /// The object's animation (its graph NONE, or its index NONE, for the
    /// model's pose) and its frame (written back: the frame the pose is
    /// from, NONE for the model's pose).
    pub animation_graph_index: u32,
    pub animation_index: i16,
    pub frame_index: i16,
    pub object_flags: u32,
    pub game_time: i32,
    pub scale: f32,
    pub function_values: [f32; 4],
    /// The orientations the game holds (the model's nodes, at most 64): 8
    /// floats each (rotation i j k w, translation, scale), written.
    pub node_count: i16,
    pub pad: i16,
    pub orientations: *mut f32,
}

fn write_orientations(out: *mut f32, orientations: &[Orientation], count: usize) {
    for (index, orientation) in orientations.iter().take(count).enumerate() {
        let floats = orientation.to_floats();
        // SAFETY: the caller's buffer holds `count` orientations
        unsafe { std::ptr::copy_nonoverlapping(floats.as_ptr(), out.add(index * 8), 8) };
    }
}

fn read_orientations(from: *const f32, count: usize) -> Vec<Orientation> {
    (0..count)
        .map(|index| {
            let mut floats = [0.0f32; 8];
            // SAFETY: the caller's buffer holds `count` orientations
            unsafe { std::ptr::copy_nonoverlapping(from.add(index * 8), floats.as_mut_ptr(), 8) };
            Orientation::from_floats(floats)
        })
        .collect()
}

fn matrix_from_floats(f: &[f32; 13]) -> Matrix4x3 {
    Matrix4x3 { scale: f[0], forward: [f[1], f[2], f[3]], left: [f[4], f[5], f[6]], up: [f[7], f[8], f[9]], position: [f[10], f[11], f[12]] }
}

/// A pose's orientations: the animation's frame (a frame of the time for
/// an object that animates by itself) or the model's pose, the object's
/// overlays, and its scale. Returns whether the animation is world relative
/// (0 or 1), or -1 if the tags aren't there (the game then runs its own).
///
/// # Safety
/// `call` must be valid, its `orientations` hold `node_count` orientations.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn halo_sim_pose_orientations(call: *mut PoseOrientationsCall) -> i32 {
    if call.is_null() {
        return -1;
    }
    // SAFETY: the caller's call
    let call = unsafe { &mut *call };
    if call.orientations.is_null() || call.node_count <= 0 || call.node_count as usize > pose::MAXIMUM_NODES {
        return -1;
    }
    let mut tags = TAGS.lock().unwrap_or_else(|e| e.into_inner());
    let Some(tags) = tags.as_mut() else { return -1 };
    let Some(definition) = tags.definition(call.definition_index) else { return -1 };
    let Some(model) = definition.model.and_then(|m| tags.model(m)) else { return -1 };
    let mut world_relative = false;
    let mut orientations = if call.animation_graph_index != u32::MAX && call.animation_index != NONE16 {
        let Some(animation) = tags.animation(call.animation_graph_index, call.animation_index) else { return -1 };
        let frame_index = if call.object_flags & OBJECT_ANIMATES_AUTOMATICALLY != 0 && animation.frame_count > 0 {
            // (OBJECT_FRAME_INDEX_GET: the object's index plus the time)
            (call.object_index.wrapping_add(call.game_time as u32) % animation.frame_count as u32) as i16
        } else {
            call.frame_index
        };
        world_relative = animation.world_relative();
        call.frame_index = frame_index;
        pose::animation_node_orientations(&model, &animation, frame_index)
    } else {
        call.frame_index = NONE16;
        pose::model_node_orientations(&model)
    };
    let node_count = call.node_count as usize;
    if orientations.len() < node_count {
        orientations.resize(node_count, Orientation::default());
    }
    if let Some(graph) = definition.animation_graph {
        for overlay in &definition.overlays {
            tags.animation(graph, overlay.animation_index);
        }
        let animations = &tags.animations;
        let animation = |index: i16| animations.get(&(graph, index)).and_then(|a| a.as_deref());
        pose::object_overlays_apply(
            &definition,
            &animation,
            node_count,
            call.function_values,
            call.object_index.wrapping_add(call.game_time as u32),
            &mut orientations,
        );
    }
    if call.scale > 0.0 {
        let o = &mut orientations[0];
        o.scale *= call.scale;
        o.translation = [o.translation[0] * call.scale, o.translation[1] * call.scale, o.translation[2] * call.scale];
    }
    write_orientations(call.orientations, &orientations, node_count);
    world_relative as i32
}

/// What a pose's node matrices read (`object_compute_node_matrices`, after
/// the object type's preprocess).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct PoseMatricesCall {
    pub definition_index: u32,
    pub node_count: i16,
    pub interpolation_frame_index: i16,
    pub interpolation_frame_count: i16,
    pub world_relative: i16,
    pub object_flags: u32,
    pub position: [f32; 3],
    pub forward: [f32; 3],
    pub up: [f32; 3],
    pub has_parent: u32,
    pub parent_mirrored: u32,
    pub parent_node_matrix: [f32; 13],
    /// The orientations it interpolates from (when interpolating).
    pub original_orientations: *const f32,
    /// The orientations (interpolated in place) and the node matrices (13
    /// floats each: scale, forward, left, up, position), written.
    pub orientations: *mut f32,
    pub matrices: *mut f32,
}

/// A pose's interpolation from the previous one and its node matrices.
/// Returns 0, or -1 if the tags aren't there (the game then runs its own).
///
/// # Safety
/// `call` must be valid, its buffers hold `node_count` orientations and
/// matrices.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn halo_sim_pose_matrices(call: *const PoseMatricesCall) -> i32 {
    if call.is_null() {
        return -1;
    }
    // SAFETY: the caller's call
    let call = unsafe { &*call };
    if call.orientations.is_null() || call.matrices.is_null() || call.node_count <= 0 || call.node_count as usize > pose::MAXIMUM_NODES {
        return -1;
    }
    let mut tags = TAGS.lock().unwrap_or_else(|e| e.into_inner());
    let Some(tags) = tags.as_mut() else { return -1 };
    let Some(definition) = tags.definition(call.definition_index) else { return -1 };
    let Some(model) = definition.model.and_then(|m| tags.model(m)) else { return -1 };
    let node_count = call.node_count as usize;
    let mut orientations = read_orientations(call.orientations, node_count);
    if call.interpolation_frame_count > 0 {
        if call.original_orientations.is_null() {
            return -1;
        }
        let original = read_orientations(call.original_orientations, node_count);
        pose::interpolate_node_orientations(&original, &mut orientations, call.interpolation_frame_index, call.interpolation_frame_count);
        write_orientations(call.orientations, &orientations, node_count);
    }
    let placement = pose::Placement {
        position: call.position,
        forward: call.forward,
        up: call.up,
        mirrored: call.object_flags & OBJECT_MIRRORED != 0,
        world_relative: call.world_relative != 0,
        center_of_mass: definition.center_of_mass,
        origin_offset: definition.origin_offset,
        parent: (call.has_parent != 0).then(|| (matrix_from_floats(&call.parent_node_matrix), call.parent_mirrored != 0)),
    };
    let matrices = pose::node_matrices(&model, &orientations, &placement);
    for (index, m) in matrices.iter().take(node_count).enumerate() {
        let floats = [
            m.scale, m.forward[0], m.forward[1], m.forward[2], m.left[0], m.left[1], m.left[2], m.up[0], m.up[1], m.up[2], m.position[0],
            m.position[1], m.position[2],
        ];
        // SAFETY: the caller's buffer holds `node_count` matrices
        unsafe { std::ptr::copy_nonoverlapping(floats.as_ptr(), call.matrices.add(index * 13), 13) };
    }
    0
}
