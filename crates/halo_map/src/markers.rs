//! A model's markers, where an object's pose puts them (ROADMAP phase 7):
//! models.c's `model_find_marker` and `model_get_marker_by_name`, and
//! objects.c's `object_get_marker_by_name` and
//! `object_align_marker_to_matrix`, ported as they are.
//!
//! A marker is named points on a model's nodes, one instance per region
//! permutation it belongs to; an object's markers are the instances of its
//! permutations, each its node's matrix times the instance's own.

use crate::collision::matrix_inverse;
use crate::model::Matrix4x3;
use crate::pose::matrix_multiply;
use crate::{CacheFile, Result, layout};

const MAXIMUM_NODES_PER_MODEL: usize = 64;
const MAXIMUM_REGIONS_PER_OBJECT: u8 = 8;

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct MarkerInstance {
    pub region: u8,
    pub permutation: u8,
    pub node: u8,
    pub translation: [f32; 3],
    pub rotation: [f32; 4],
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Marker {
    /// Its name's bytes, up to its NUL (32 at most).
    pub name: Vec<u8>,
    pub instances: Vec<MarkerInstance>,
}

/// A model's markers, in the tag's order (sorted by name, as the game's
/// binary search expects), and its node count.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ModelMarkers {
    pub markers: Vec<Marker>,
    pub node_count: usize,
}

/// An object's marker (`struct object_marker`): its node, the instance's
/// matrix in the node's space, and the marker's matrix in the world.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ObjectMarker {
    pub node_index: i16,
    pub node_matrix: Matrix4x3,
    pub matrix: Matrix4x3,
}

/// `_stricmp`: ASCII letters compared as lower case, to the first NUL.
fn stricmp(a: &[u8], b: &[u8]) -> i32 {
    let mut index = 0;
    loop {
        let x = a.get(index).copied().unwrap_or(0).to_ascii_lowercase() as i32;
        let y = b.get(index).copied().unwrap_or(0).to_ascii_lowercase() as i32;
        if x != y || x == 0 {
            return x - y;
        }
        index += 1;
    }
}

/// `matrix4x3_from_point_and_quaternion`.
pub fn matrix_from_point_and_quaternion(point: [f32; 3], q: [f32; 4]) -> Matrix4x3 {
    let orientation = crate::pose::Orientation { rotation: q, translation: point, scale: 1.0 };
    crate::pose::matrix_from_orientation(&orientation)
}

fn cross3(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

/// `matrix4x3_from_point_and_vectors`.
pub fn matrix_from_point_and_vectors(point: [f32; 3], forward: [f32; 3], up: [f32; 3]) -> Matrix4x3 {
    Matrix4x3 { scale: 1.0, forward, left: cross3(up, forward), up, position: point }
}

impl ModelMarkers {
    pub fn load(cache: &CacheFile, model: u32) -> Result<Self> {
        use layout::{model as m, model_marker as k, model_marker_instance as i};
        let root = cache.tag_root(model, crate::groups::MODEL)?;
        let mut markers = Vec::new();
        for marker in root.block(m::MARKERS, k::SIZEOF)?.iter() {
            let bytes = marker.bytes(k::NAME, 32)?;
            let name = bytes.iter().take_while(|&&b| b != 0).copied().collect();
            let mut instances = Vec::new();
            for instance in marker.block(k::INSTANCES, i::SIZEOF)?.iter() {
                let r = i::ROTATION;
                instances.push(MarkerInstance {
                    region: instance.u8(i::REGION_INDEX)?,
                    permutation: instance.u8(i::PERMUTATION_INDEX)?,
                    node: instance.u8(i::NODE_INDEX)?,
                    translation: instance.vec3(i::TRANSLATION)?,
                    rotation: [instance.f32(r)?, instance.f32(r + 4)?, instance.f32(r + 8)?, instance.f32(r + 12)?],
                });
            }
            markers.push(Marker { name, instances });
        }
        let node_count = root.block(m::NODES, layout::model_node::SIZEOF)?.iter().count();
        Ok(ModelMarkers { markers, node_count })
    }

    /// `model_find_marker`: the marker's index by its name (a binary search).
    pub fn find(&self, name: &str) -> Option<usize> {
        if name.is_empty() {
            return None;
        }
        let (mut lower, mut upper) = (0i32, self.markers.len() as i32 - 1);
        while lower <= upper {
            let index = (lower + upper) / 2;
            let comparison = stricmp(name.as_bytes(), &self.markers[index as usize].name);
            if comparison == 0 {
                return Some(index as usize);
            }
            if comparison < 0 {
                upper = index - 1;
            } else {
                lower = index + 1;
            }
        }
        None
    }

    /// `model_get_marker_by_name` with an object's permutations and node
    /// matrices (at most `maximum`).
    pub fn get_by_name(
        &self,
        name: &str,
        region_permutations: &[u8; 8],
        node_matrices: &[Matrix4x3],
        mirrored: bool,
        maximum: usize,
    ) -> Vec<ObjectMarker> {
        let mut result = Vec::new();
        let Some(marker_index) = self.find(name) else { return result };
        let node_limit = self.node_count.min(MAXIMUM_NODES_PER_MODEL);
        let matrix_count = node_limit.min(node_matrices.len());
        for instance in &self.markers[marker_index].instances {
            // (a marker on a region or node the model doesn't have is
            // skipped)
            if instance.region >= MAXIMUM_REGIONS_PER_OBJECT || instance.node as usize >= node_limit {
                continue;
            }
            if region_permutations[instance.region as usize] == instance.permutation {
                if result.len() >= maximum {
                    break;
                }
                let node_index = instance.node as usize;
                if node_index >= matrix_count {
                    continue;
                }
                let node_matrix = matrix_from_point_and_quaternion(instance.translation, instance.rotation);
                let mut matrix = matrix_multiply(&node_matrices[node_index], &node_matrix);
                if mirrored {
                    matrix.left = [-matrix.left[0], -matrix.left[1], -matrix.left[2]];
                }
                result.push(ObjectMarker { node_index: node_index as i16, node_matrix, matrix });
            }
        }
        result
    }
}

/// `object_get_marker_by_name` for one marker: the object's first marker of
/// the name, or (`None`, as the game's count of 0) none, the game's
/// stand-in for which is the object's root node.
pub fn object_marker_by_name(
    markers: Option<&ModelMarkers>,
    name: &str,
    region_permutations: &[u8; 8],
    node_matrices: &[Matrix4x3],
    mirrored: bool,
) -> Option<ObjectMarker> {
    markers.and_then(|markers| markers.get_by_name(name, region_permutations, node_matrices, mirrored, 1).into_iter().next())
}

/// `object_align_marker_to_matrix`: the object placed so that its marker
/// sits on `destination`. Returns its position, forward and up.
pub fn align_marker_to_matrix(
    position: [f32; 3],
    forward: [f32; 3],
    up: [f32; 3],
    marker: &ObjectMarker,
    destination: &Matrix4x3,
) -> ([f32; 3], [f32; 3], [f32; 3]) {
    let final_matrix = matrix_from_point_and_vectors(position, forward, up);
    let mut marker_transform = matrix_inverse(&final_matrix);
    marker_transform = matrix_multiply(&marker_transform, &marker.matrix);
    marker_transform = matrix_inverse(&marker_transform);
    let final_matrix = matrix_multiply(destination, &marker_transform);
    let mut forward = final_matrix.forward;
    let vector = cross3(final_matrix.forward, final_matrix.up);
    let mut up = cross3(vector, final_matrix.forward);
    normalize3(&mut forward);
    normalize3(&mut up);
    (final_matrix.position, forward, up)
}

/// `normalize3d`: the magnitude, 0 (leaving the vector) below epsilon.
pub(crate) fn normalize3(v: &mut [f32; 3]) -> f32 {
    let magnitude = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    if !(0.0001 > magnitude.abs()) {
        *v = [v[0] * (1.0 / magnitude), v[1] * (1.0 / magnitude), v[2] * (1.0 / magnitude)];
        magnitude
    } else {
        0.0
    }
}
