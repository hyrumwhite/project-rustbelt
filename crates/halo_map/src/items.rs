//! Halo's item update (ROADMAP phase 7, objects and physics): items.c's
//! `item_update` with `item_align_to_normal_and_point` and
//! `item_adjust_for_angular_velocity_change`, ported as they are, in the
//! same single-precision arithmetic.
//!
//! A dropped weapon or piece of equipment, free in the world: kept upright
//! if its definition says so; falling under gravity (unless antigravity),
//! its line through the world tested each tick; bouncing off what it hits
//! (its material effect and sound), or coming to rest on a gentle enough
//! slope of the structure, scenery or a device, aligned there by its
//! "ground point" marker; at rest, staying on its object as that moves, or
//! falling again when what it rests on goes; spinning by its angular
//! velocity; and counting down to its detonation.
//!
//! What it asks of the game goes through an [`ItemHost`] ([`call`]'s kinds,
//! the ray and point tests, and other objects).

use crate::collision::{CollisionKind, CollisionResult, inverse_transform_point, transform_point};
use crate::markers::{ModelMarkers, ObjectMarker, align_marker_to_matrix, matrix_from_point_and_vectors, normalize3, object_marker_by_name};
use crate::model::Matrix4x3;
use crate::{CacheFile, Result, layout};

const NONE: i32 = -1;

/// `ITEM_UPDATE_COLLISION_TEST_FLAGS`.
pub const COLLISION_TEST_FLAGS: u32 = 0x1FF3E9;
/// `item_maximum_impact_velocity`.
const MAXIMUM_IMPACT_VELOCITY: f32 = 0.1;

// object flags
const OBJECT_AT_REST: u32 = 1 << 5;
const OBJECT_MIRRORED: u32 = 1 << 12;
const OBJECT_CONNECTED_TO_MAP: u32 = 1 << 11;
// item flags (the datum's)
const ITEM_ATTACHED_TO_UNIT: u32 = 1 << 0;
const ITEM_HAS_NONZERO_ANGULAR_VELOCITY: u32 = 1 << 2;
const ITEM_ON_STRUCTURE: u32 = 1 << 3;
const ITEM_ON_OBJECT: u32 = 1 << 4;
// item definition flags
const ITEM_ALWAYS_MAINTAINS_Z_UP: u32 = 1 << 0;
const ITEM_ANTIGRAVITY: u32 = 1 << 2;
// collision surface flags
const SURFACE_BREAKABLE: u8 = 1 << 3;
// object types: scenery and the devices (machines, controls, light fixtures)
const OBJECT_MASK_SCENERY_AND_DEVICES: u32 = (1 << 6) | (1 << 7) | (1 << 8) | (1 << 9);

/// An item as its update reads and changes it (the game's fields, laid out
/// for the game's side, `halo_sim.c`).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ItemState {
    pub object_index: u32,
    pub definition_index: u32,
    pub object_flags: u32,
    pub owner_player_index: i32,
    pub parent_object_index: i32,
    pub position: [f32; 3],
    pub forward: [f32; 3],
    pub up: [f32; 3],
    pub translational_velocity: [f32; 3],
    pub angular_velocity: [f32; 3],
    pub region_permutations: [u8; 8],
    pub item_flags: u32,
    pub detonation_ticks: i16,
    pub rested_surface_index: i16,
    pub bsp_index: i16,
    pub pad: i16,
    pub ignore_object_index: i32,
    pub last_owned_time: i32,
    pub item_on_rest_object_index: i32,
    pub item_rest_object_offset: [f32; 3],
    pub rotation_axis: [f32; 3],
    pub rotation_sine: f32,
    pub rotation_cosine: f32,
}

const _: () = assert!(std::mem::size_of::<ItemState>() == 144);

/// An item definition's flags and effects (`_item_definition`), and its
/// model's markers.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ItemDefinition {
    pub flags: u32,
    pub markers: Option<ModelMarkers>,
}

impl ItemDefinition {
    pub fn load(cache: &CacheFile, datum: u32) -> Result<Self> {
        let root = cache.tag_root(datum, crate::groups::OBJECT)?;
        let item = root.add(layout::item_definition::ITEM);
        let model = root.reference(layout::_object_definition::MODEL)?;
        let markers = match model.datum {
            Some(model) => Some(ModelMarkers::load(cache, model)?),
            None => None,
        };
        Ok(ItemDefinition { flags: item.u32(layout::_item_definition::FLAGS)?, markers })
    }
}

/// What an item's update asks of the game (`ItemCall::kind`).
pub mod call {
    /// A bounce's or a landing's impact: the definition's material effect
    /// (if visible there) and collision sound. `a` the material type hit,
    /// `b` and `c` the leaf and cluster there; `f` the point, the normal, the
    /// sound's position, and the impact's scale.
    pub const IMPACT: i32 = 1;
    /// `object_set_garbage(item, a)`.
    pub const SET_GARBAGE: i32 = 2;
    /// `object_translate(item, f, location)`: `b` and `c` the leaf and
    /// cluster.
    pub const TRANSLATE: i32 = 3;
    /// `item_accelerate(item, f, FALSE)`.
    pub const ACCELERATE: i32 = 4;
    /// Its detonation: its definition's detonation effect, and the item
    /// deleted.
    pub const DETONATE: i32 = 5;
}

/// One call of the game.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ItemCall {
    pub kind: i32,
    pub a: i32,
    pub b: i32,
    pub c: i32,
    pub f: [f32; 10],
}

/// Another object, as an item's update reads it: its type and node
/// matrices.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ItemObject {
    pub object_type: i16,
    pub node_matrices: Vec<Matrix4x3>,
}

/// What an item's update reads of the game besides the item.
#[derive(Debug, Clone, Default)]
pub struct ItemInputs {
    pub global_gravity: f32,
    pub game_engine_running: bool,
    pub game_time: i32,
    pub structure_bsp_index: i16,
    /// The item's node matrices (its markers' nodes).
    pub node_matrices: Vec<Matrix4x3>,
}

/// A running game, as an item's update asks it. The item's state is the
/// update's as it stands (the host writes it to the game first, and reads
/// back what the call changed).
pub trait ItemHost {
    /// A call: whether the item is still there after.
    fn call(&self, state: &mut ItemState, call: &ItemCall) -> bool;
    /// `collision_test_line`'s whole ray test.
    fn test_vector(&self, flags: u32, point: [f32; 3], vector: [f32; 3], ignore_object: u32) -> CollisionResult;
    /// `collision_test_point`.
    fn test_point(&self, flags: u32, point: [f32; 3], ignore_object: u32) -> bool;
    /// Another object (`None` if it is gone).
    fn object(&self, index: u32) -> Option<ItemObject>;
    /// The structure's collision surface's flags and breakable surface
    /// index, and whether that breakable surface stands.
    fn surface(&self, index: i16) -> Option<(u8, bool)>;
}

fn cross3(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

fn dot3(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn scale3(v: [f32; 3], c: f32) -> [f32; 3] {
    [c * v[0], c * v[1], c * v[2]]
}

fn magnitude3(v: [f32; 3]) -> f32 {
    (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt()
}

/// `point_from_line3d`: `v * t + p`.
fn point_from_line3(p: [f32; 3], v: [f32; 3], t: f32) -> [f32; 3] {
    [v[0] * t + p[0], v[1] * t + p[1], v[2] * t + p[2]]
}

/// `rotate_vector_about_axis`.
fn rotate_vector_about_axis(v: &mut [f32; 3], n: [f32; 3], sine: f32, cosine: f32) {
    let one_minus_cosine_times_v_dot_n = dot3(n, *v) * (1.0 - cosine);
    let v_cross_n_i = n[2] * v[1] - v[2] * n[1];
    let v_cross_n_j = n[0] * v[2] - v[0] * n[2];
    let v_cross_n_k = n[1] * v[0] - v[1] * n[0];
    v[0] = cosine * v[0] + one_minus_cosine_times_v_dot_n * n[0] - sine * v_cross_n_i;
    v[1] = cosine * v[1] + one_minus_cosine_times_v_dot_n * n[1] - sine * v_cross_n_j;
    v[2] = cosine * v[2] + one_minus_cosine_times_v_dot_n * n[2] - sine * v_cross_n_k;
}

/// `quaternion_transform_point`.
fn quaternion_transform_point(q: [f32; 4], p: [f32; 3]) -> [f32; 3] {
    let (v, w) = ([q[0], q[1], q[2]], q[3]);
    let w_squared = w * w;
    let two_w_squared_minus_one = 2.0 * w_squared - 1.0;
    let v_dot_p = v[0] * p[0] + v[2] * p[2] + v[1] * p[1];
    let two_v_dot_p = v_dot_p + v_dot_p;
    let two_w = w + w;
    let v_cross_r_i = v[2] * p[0] - p[2] * v[0];
    let v_cross_r_j = v[0] * p[1] - v[1] * p[0];
    let v_cross_r_k = v[1] * p[2] - v[2] * p[1];
    [
        v_cross_r_k * two_w + two_v_dot_p * v[0] + two_w_squared_minus_one * p[0],
        two_v_dot_p * v[1] + two_w_squared_minus_one * p[1] + v_cross_r_i * two_w,
        two_v_dot_p * v[2] + two_w_squared_minus_one * p[2] + v_cross_r_j * two_w,
    ]
}

fn sine(a: f32) -> f32 {
    (a as f64).sin() as f32
}

fn cosine(a: f32) -> f32 {
    (a as f64).cos() as f32
}

/// `item_adjust_for_angular_velocity_change`.
fn adjust_for_angular_velocity_change(item: &mut ItemState) {
    let magnitude = magnitude3(item.angular_velocity);
    if magnitude != 0.0 {
        item.item_flags |= ITEM_HAS_NONZERO_ANGULAR_VELOCITY;
        if item.object_flags & OBJECT_AT_REST == 0 {
            item.rotation_axis = scale3(item.angular_velocity, 1.0 / magnitude);
        }
        item.rotation_sine = sine(magnitude);
        item.rotation_cosine = cosine(magnitude);
    } else {
        item.item_flags &= !ITEM_HAS_NONZERO_ANGULAR_VELOCITY;
        item.rotation_sine = 0.0;
        item.rotation_cosine = 1.0;
    }
}

fn ground_point(definition: &ItemDefinition, inputs: &ItemInputs, item: &ItemState) -> Option<ObjectMarker> {
    object_marker_by_name(
        definition.markers.as_ref(),
        "ground point",
        &item.region_permutations,
        &inputs.node_matrices,
        item.object_flags & OBJECT_MIRRORED != 0,
    )
}

/// `item_align_to_normal_and_point`: the item turned so that its ground
/// point's up is the normal, and moved so that it sits on `position` (its
/// ground point where it is, for none). Returns the item's new position, or
/// `None` if it has no ground point.
fn align_to_normal_and_point(
    definition: &ItemDefinition,
    inputs: &ItemInputs,
    item: &mut ItemState,
    normal: [f32; 3],
    position: Option<[f32; 3]>,
) -> Option<[f32; 3]> {
    let marker = ground_point(definition, inputs, item)?;
    let position = position.unwrap_or(marker.matrix.position);
    let scale = (2.0 * (dot3(marker.matrix.up, normal) + 1.0)).sqrt();
    let mut new_forward = if scale as f64 > 0.01 {
        let inverse_scale = 1.0 / scale;
        let v = scale3(cross3(marker.matrix.up, normal), inverse_scale);
        quaternion_transform_point([v[0], v[1], v[2], scale * 0.5], marker.matrix.forward)
    } else {
        let cross = cross3(normal, marker.matrix.forward);
        cross3(cross, normal)
    };
    normalize3(&mut new_forward);
    let ground_point_matrix = matrix_from_point_and_vectors(position, new_forward, normal);
    (item.position, item.forward, item.up) = align_marker_to_matrix(item.position, item.forward, item.up, &marker, &ground_point_matrix);
    Some(item.position)
}

/// `item_update` (a part of every item's: weapons', equipment's and
/// garbage's, before their own).
pub fn item_update(inputs: &ItemInputs, definition: &ItemDefinition, host: &dyn ItemHost, item: &mut ItemState) {
    if item.object_flags & OBJECT_CONNECTED_TO_MAP != 0 && item.parent_object_index == NONE {
        #[allow(clippy::neg_cmp_op_on_partial_ord)]
        if definition.flags & ITEM_ALWAYS_MAINTAINS_Z_UP != 0 && !((item.up[2] - 1.0).abs() < 0.0001) {
            item.up = [0.0, 0.0, 1.0];
            let left = cross3(item.up, item.forward);
            item.forward = cross3(left, item.up);
            if normalize3(&mut item.forward) == 0.0 {
                item.forward = [1.0, 0.0, 0.0];
            }
        }

        if item.object_flags & OBJECT_AT_REST == 0 {
            let mut new_velocity = item.translational_velocity;
            if definition.flags & ITEM_ANTIGRAVITY == 0 {
                new_velocity[2] -= inputs.global_gravity;
            }
            let mut new_position =
                [item.position[0] + new_velocity[0], item.position[1] + new_velocity[1], item.position[2] + new_velocity[2]];
            // (collision_test_line)
            let vector = [new_position[0] - item.position[0], new_position[1] - item.position[1], new_position[2] - item.position[2]];
            let collision = host.test_vector(COLLISION_TEST_FLAGS, item.position, vector, item.ignore_object_index as u32);
            if let Some(kind) = collision.kind {
                let n = [collision.plane[0], collision.plane[1], collision.plane[2]];
                new_position = point_from_line3(new_position, n, 0.05);

                let mut impact_scale = magnitude3(new_velocity);
                impact_scale /= MAXIMUM_IMPACT_VELOCITY;
                #[allow(clippy::manual_clamp)]
                {
                    impact_scale = if impact_scale < 0.0 {
                    0.0
                } else if impact_scale > 1.0 {
                    1.0
                    } else {
                        impact_scale
                    };
                }
                let impact = ItemCall {
                    kind: call::IMPACT,
                    a: collision.material_type as i32,
                    b: collision.location_leaf,
                    c: collision.location_cluster as i32,
                    f: [
                        collision.point[0],
                        collision.point[1],
                        collision.point[2],
                        n[0],
                        n[1],
                        n[2],
                        new_position[0],
                        new_position[1],
                        new_position[2],
                        impact_scale,
                    ],
                };
                host.call(item, &impact);

                let rests_on = match kind {
                    CollisionKind::Structure => true,
                    CollisionKind::Object => collision
                        .object
                        .and_then(|index| host.object(index))
                        .is_some_and(|o| o.object_type >= 0 && OBJECT_MASK_SCENERY_AND_DEVICES & (1 << o.object_type as u32) != 0),
                    CollisionKind::Media => false,
                };
                // (the game's 0.7071, not 1/sqrt(2))
                #[allow(clippy::approx_constant)]
                let gentle = n[2] > 0.7071;
                if rests_on && gentle && -dot3(n, new_velocity) < 0.05 {
                    new_position = collision.point;
                    if let Some(position) = align_to_normal_and_point(definition, inputs, item, n, Some(collision.point)) {
                        new_position = position;
                    }
                    new_velocity = [0.0; 3];
                    item.angular_velocity = scale3(n, dot3(item.angular_velocity, n));

                    if !inputs.game_engine_running && item.owner_player_index == NONE {
                        host.call(item, &ItemCall { kind: call::SET_GARBAGE, a: 1, ..Default::default() });
                    }

                    item.object_flags |= OBJECT_AT_REST;
                    match kind {
                        CollisionKind::Structure => {
                            item.item_flags |= ITEM_ON_STRUCTURE;
                            item.rested_surface_index = collision.surface as i16;
                            item.bsp_index = inputs.structure_bsp_index;
                        }
                        _ => {
                            item.item_flags |= ITEM_ON_OBJECT;
                            let rest_object = collision.object.map_or(NONE, |o| o as i32);
                            item.item_on_rest_object_index = rest_object;
                            if let Some(node_matrix) =
                                host.object(rest_object as u32).and_then(|o| o.node_matrices.first().copied())
                            {
                                item.item_rest_object_offset = inverse_transform_point(&node_matrix, collision.point);
                            }
                        }
                    }

                    item.rotation_axis = n;
                    adjust_for_angular_velocity_change(item);
                    item.ignore_object_index = NONE;
                } else {
                    let mut reflection = -1.4 * dot3(n, new_velocity);
                    if kind != CollisionKind::Structure {
                        reflection = if 1.5 < reflection { 1.5 } else { reflection };
                    }
                    new_position = collision.point;
                    new_velocity[0] += n[0] * reflection;
                    new_velocity[1] += n[1] * reflection;
                    new_velocity[2] += n[2] * reflection;
                    if host.test_point(COLLISION_TEST_FLAGS, new_position, item.object_index) {
                        new_position = point_from_line3(collision.point, n, 0.05);
                    }
                    // (the game tests the point again, and leaves it)
                }
            }

            item.translational_velocity = new_velocity;
            let translate = ItemCall {
                kind: call::TRANSLATE,
                b: collision.location_leaf,
                c: collision.location_cluster as i32,
                f: [new_position[0], new_position[1], new_position[2], 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
                ..Default::default()
            };
            host.call(item, &translate);
        } else if definition.flags & ITEM_ANTIGRAVITY == 0 {
            let gravity = [0.0 * inputs.global_gravity, 0.0 * inputs.global_gravity, -inputs.global_gravity];
            if item.item_flags & ITEM_ON_STRUCTURE != 0
                && item.rested_surface_index as i32 != NONE
                && item.bsp_index == inputs.structure_bsp_index
            {
                if let Some((flags, standing)) = host.surface(item.rested_surface_index)
                    && flags & SURFACE_BREAKABLE != 0
                    && !standing
                {
                    item.item_flags &= !ITEM_ON_STRUCTURE;
                    item.rested_surface_index = NONE as i16;
                    accelerate(host, item, gravity);
                }
            } else if item.item_flags & ITEM_ON_OBJECT != 0 {
                match host.object(item.item_on_rest_object_index as u32).and_then(|o| o.node_matrices.first().copied()) {
                    Some(node_matrix) => {
                        let origin = transform_point(&node_matrix, item.item_rest_object_offset);
                        let normal = item.rotation_axis;
                        align_to_normal_and_point(definition, inputs, item, normal, Some(origin));
                    }
                    None => {
                        item.item_flags &= !ITEM_ON_OBJECT;
                        accelerate(host, item, gravity);
                    }
                }
            }

            item.angular_velocity = [item.angular_velocity[0] * 0.9, item.angular_velocity[1] * 0.9, item.angular_velocity[2] * 0.9];
            adjust_for_angular_velocity_change(item);
        }

        if item.item_flags & ITEM_HAS_NONZERO_ANGULAR_VELOCITY != 0 {
            let marker = if item.object_flags & OBJECT_AT_REST == 0 { None } else { ground_point(definition, inputs, item) };
            match marker {
                None => {
                    rotate_vector_about_axis(&mut item.forward, item.rotation_axis, item.rotation_sine, item.rotation_cosine);
                    rotate_vector_about_axis(&mut item.up, item.rotation_axis, item.rotation_sine, item.rotation_cosine);
                }
                Some(marker) => {
                    let mut m = marker.matrix;
                    rotate_vector_about_axis(&mut m.forward, item.rotation_axis, item.rotation_sine, item.rotation_cosine);
                    rotate_vector_about_axis(&mut m.up, item.rotation_axis, item.rotation_sine, item.rotation_cosine);
                    m.left = cross3(m.up, m.forward);
                    m.forward = cross3(m.left, m.up);
                    normalize3(&mut m.forward);
                    normalize3(&mut m.left);
                    normalize3(&mut m.up);
                    (item.position, item.forward, item.up) = align_marker_to_matrix(item.position, item.forward, item.up, &marker, &m);
                }
            }
            normalize3(&mut item.up);
            let left = cross3(item.up, item.forward);
            item.forward = cross3(left, item.up);
            normalize3(&mut item.forward);
        }
    }

    if item.detonation_ticks > 0 {
        item.detonation_ticks -= 1;
        if item.detonation_ticks == 0 {
            host.call(item, &ItemCall { kind: call::DETONATE, ..Default::default() });
        }
    }

    if item.item_flags & ITEM_ATTACHED_TO_UNIT != 0 {
        item.last_owned_time = inputs.game_time;
    }
}

/// `item_accelerate(item, gravity, FALSE)`, the game's.
fn accelerate(host: &dyn ItemHost, item: &mut ItemState, acceleration: [f32; 3]) {
    let mut f = [0.0f32; 10];
    f[..3].copy_from_slice(&acceleration);
    host.call(item, &ItemCall { kind: call::ACCELERATE, f, ..Default::default() });
}
