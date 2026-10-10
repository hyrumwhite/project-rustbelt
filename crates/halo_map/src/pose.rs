//! Halo's object poses (ROADMAP phase 7, objects and physics): an object's
//! node orientations from its animation's frame (`animation_get_node_
//! orientations` in OpenCE's `source/models/model_animations.c`, both the
//! plain frames and the keyframe-compressed ones) or its model's default
//! pose, and its node matrices (`object_compute_node_matrices` in
//! `source/objects/objects.c`: the interpolation from the previous pose,
//! and the node hierarchy placed by the object's position, axes, centre of
//! mass, origin offset and parent node), ported as they are, in the same
//! single-precision arithmetic.
//!
//! It also applies animations over a pose, as the types' preprocesses do
//! (`replacement_animation_apply`, `overlay_animation_apply` and its
//! scaled and continuous forms, `aiming_screen_apply`); a unit's is in
//! `unit_pose`. Objects' overlay animations (driven by their function
//! values) and the other types' preprocesses (vehicles', weapons',
//! devices') are not ported yet.

use crate::model::Matrix4x3;
use crate::{CacheFile, Result, layout};

/// The most nodes an animation (and a model's pose) has.
pub const MAXIMUM_NODES: usize = 64;

/// A node's orientation (`real_orientation`).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Orientation {
    /// i, j, k, w.
    pub rotation: [f32; 4],
    pub translation: [f32; 3],
    pub scale: f32,
}

impl Orientation {
    /// From the bridge's eight floats (rotation, translation, scale).
    pub fn from_floats(f: [f32; 8]) -> Self {
        Orientation { rotation: [f[0], f[1], f[2], f[3]], translation: [f[4], f[5], f[6]], scale: f[7] }
    }

    pub fn to_floats(self) -> [f32; 8] {
        let [i, j, k, w] = self.rotation;
        let [x, y, z] = self.translation;
        [i, j, k, w, x, y, z, self.scale]
    }
}

/// A model's node, as posing reads it.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct PoseNode {
    pub next_sibling: i16,
    pub first_child: i16,
    pub parent: i16,
    pub default_translation: [f32; 3],
    pub default_rotation: [f32; 4],
}

/// A model, as posing reads it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PoseModel {
    pub node_list_checksum: i32,
    pub nodes: Vec<PoseNode>,
    pub node_names: Vec<String>,
}

impl PoseModel {
    pub fn load(cache: &CacheFile, datum: u32) -> Result<Self> {
        use layout::{model as m, model_node as n};
        let root = cache.tag_root(datum, crate::groups::MODEL)?;
        let mut nodes = Vec::new();
        let mut node_names = Vec::new();
        for node in root.block(m::NODES, n::SIZEOF)?.iter() {
            node_names.push(node.add(n::NAME).c_string(32)?);
            let r = n::DEFAULT_ROTATION;
            nodes.push(PoseNode {
                next_sibling: node.i16(n::NEXT_SIBLING_NODE_INDEX)?,
                first_child: node.i16(n::FIRST_CHILD_NODE_INDEX)?,
                parent: node.i16(n::PARENT_NODE_INDEX)?,
                default_translation: node.vec3(n::DEFAULT_TRANSLATION)?,
                default_rotation: [node.f32(r)?, node.f32(r + 4)?, node.f32(r + 8)?, node.f32(r + 12)?],
            });
        }
        Ok(PoseModel { node_list_checksum: root.i32(m::NODE_LIST_CHECKSUM)?, nodes, node_names })
    }

    /// `model_find_node`: the node with this name.
    pub fn find_node(&self, name: &str) -> Option<usize> {
        self.node_names.iter().position(|n| n == name)
    }
}

/// An animation, as posing reads it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PoseAnimation {
    pub kind: i16,
    pub frame_count: i16,
    pub frame_size: i16,
    pub node_list_checksum: i32,
    pub node_count: i16,
    pub flags: u16,
    pub translation_flags: [u32; 2],
    pub rotation_flags: [u32; 2],
    pub scale_flags: [u32; 2],
    pub compressed_data_offset: i32,
    pub default_data: Vec<u8>,
    pub data: Vec<u8>,
}

impl PoseAnimation {
    /// The animation `index` of the graph `graph`.
    pub fn load(cache: &CacheFile, graph: u32, index: i16) -> Result<Option<Self>> {
        use layout::animation as a;
        let root = cache.tag_root(graph, crate::groups::ANIMATION_GRAPH)?;
        let animations = root.block(layout::animation_graph::ANIMATIONS, a::SIZEOF)?;
        let Some(animation) = (index >= 0).then(|| animations.get(index as usize)).flatten() else { return Ok(None) };
        let pair = |rel: usize| -> Result<[u32; 2]> { Ok([animation.u32(rel)?, animation.u32(rel + 4)?]) };
        Ok(Some(PoseAnimation {
            kind: animation.i16(a::TYPE)?,
            frame_count: animation.i16(a::FRAME_COUNT)?,
            frame_size: animation.i16(a::FRAME_SIZE)?,
            node_list_checksum: animation.i32(a::NODE_LIST_CHECKSUM)?,
            node_count: animation.i16(a::NODE_COUNT)?,
            flags: animation.u16(a::FLAGS)?,
            translation_flags: pair(a::NODES_WITH_TRANSLATION_FLAGS)?,
            rotation_flags: pair(a::NODES_WITH_ROTATION_FLAGS)?,
            scale_flags: pair(a::NODES_WITH_SCALE_FLAGS)?,
            compressed_data_offset: animation.i32(a::COMPRESSED_DATA_OFFSET)?,
            default_data: animation.data_bytes(a::DEFAULT_DATA)?.to_vec(),
            data: animation.data_bytes(a::DATA)?.to_vec(),
        }))
    }

    fn compressed(&self) -> bool {
        self.flags & ANIMATION_COMPRESSED != 0
    }

    /// A base animation (`_animation_base`), which sets every node.
    pub fn is_base(&self) -> bool {
        self.kind == ANIMATION_BASE
    }

    pub fn world_relative(&self) -> bool {
        self.flags & ANIMATION_WORLD_RELATIVE != 0
    }
}

const ANIMATION_COMPRESSED: u16 = 1 << 0;
const ANIMATION_WORLD_RELATIVE: u16 = 1 << 1;
const ANIMATION_BASE: i16 = 0;
const ANIMATION_OVERLAY: i16 = 1;
const ANIMATION_REPLACEMENT: i16 = 2;
const COMPRESSED_QUATERNION_SCALE: f32 = 1.0 / 32767.0;
const KEYFRAME_COUNT_BITS: u32 = 12;
/// `offsetof(struct compressed_animation_header, rotation_node_headers)`.
const HEADER_SIZE: i64 = 0x2c;

// the compressed header's fields (longs)
const ROTATION_KEYFRAME_FRAME_INDICES: usize = 0;
const DEFAULT_ROTATIONS: usize = 1;
const ROTATION_KEYFRAMES: usize = 2;
const TRANSLATION_NODE_HEADERS: usize = 3;
const TRANSLATION_KEYFRAME_FRAME_INDICES: usize = 4;
const DEFAULT_TRANSLATIONS: usize = 5;
const TRANSLATION_KEYFRAMES: usize = 6;
const SCALE_NODE_HEADERS: usize = 7;
const SCALE_KEYFRAME_FRAME_INDICES: usize = 8;
const DEFAULT_SCALES: usize = 9;
const SCALE_KEYFRAMES: usize = 10;

fn i16_at(bytes: &[u8], at: usize) -> i16 {
    bytes.get(at..at + 2).map_or(0, |b| i16::from_le_bytes([b[0], b[1]]))
}
fn u16_at(bytes: &[u8], at: usize) -> u16 {
    i16_at(bytes, at) as u16
}
fn u32_at(bytes: &[u8], at: usize) -> u32 {
    bytes.get(at..at + 4).map_or(0, |b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}
fn f32_at(bytes: &[u8], at: usize) -> f32 {
    f32::from_bits(u32_at(bytes, at))
}
fn vec3_at(bytes: &[u8], at: usize) -> [f32; 3] {
    [f32_at(bytes, at), f32_at(bytes, at + 4), f32_at(bytes, at + 8)]
}

/// `quaternion_decompress_8byte`.
fn decompress_8byte(bytes: &[u8], at: usize) -> [f32; 4] {
    [
        i16_at(bytes, at) as f32 * COMPRESSED_QUATERNION_SCALE,
        i16_at(bytes, at + 2) as f32 * COMPRESSED_QUATERNION_SCALE,
        i16_at(bytes, at + 4) as f32 * COMPRESSED_QUATERNION_SCALE,
        i16_at(bytes, at + 6) as f32 * COMPRESSED_QUATERNION_SCALE,
    ]
}

/// `quaternion_decompress_6byte`: four 12-bit components packed in three
/// words.
fn decompress_6byte(bytes: &[u8], at: usize) -> [f32; 4] {
    let w0 = u16_at(bytes, at) as u32;
    let w1 = u16_at(bytes, at + 2) as u32;
    let w2 = u16_at(bytes, at + 4) as u32;
    let i = ((w0 >> 12) | (w0 & 0xfff0)) as u16 as i16;
    let j = (((w1 >> 4) & 0x0ff0) | (w0 & 0x000f) | (w0 << 12)) as u16 as i16;
    let k = (((((w2 >> 4) & 0x0f00) | (w1 & 0x00f0)) >> 4) | (w1 << 8)) as u16 as i16;
    let w = (((w2 >> 8) & 0x000f) | (w2 << 4)) as u16 as i16;
    [
        i as f32 * COMPRESSED_QUATERNION_SCALE,
        j as f32 * COMPRESSED_QUATERNION_SCALE,
        k as f32 * COMPRESSED_QUATERNION_SCALE,
        w as f32 * COMPRESSED_QUATERNION_SCALE,
    ]
}

/// `quaternion_normalize`.
pub fn quaternion_normalize(q: [f32; 4]) -> [f32; 4] {
    let magnitude_squared = q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3];
    if magnitude_squared > 0.0 {
        // (reciprocal_square_root: 1 / the float square root)
        let one_over = 1.0 / magnitude_squared.sqrt();
        [one_over * q[0], one_over * q[1], one_over * q[2], one_over * q[3]]
    } else {
        [0.0, 0.0, 0.0, 1.0]
    }
}

/// `quaternions_interpolate_and_normalize`.
pub fn quaternions_interpolate_and_normalize(q0: [f32; 4], q1: [f32; 4], t: f32) -> [f32; 4] {
    let v = 1.0 - t;
    let t = if q0[0] * q1[0] + q0[1] * q1[1] + q0[2] * q1[2] + q0[3] * q1[3] < 0.0 { -t } else { t };
    quaternion_normalize([q0[0] * v + q1[0] * t, q0[1] * v + q1[1] * t, q0[2] * v + q1[2] * t, q0[3] * v + q1[3] * t])
}

/// `model_get_node_orientations`: the model's default pose.
pub fn model_node_orientations(model: &PoseModel) -> Vec<Orientation> {
    model
        .nodes
        .iter()
        .take(MAXIMUM_NODES)
        .map(|n| Orientation { rotation: n.default_rotation, translation: n.default_translation, scale: 1.0 })
        .collect()
}

impl PoseAnimation {
    /// `animation_data_contains`.
    fn contains(&self, offset: i64, relative_offset: i64, first_element_index: i64, element_count: i64, element_size: i64) -> bool {
        let mut size = self.data.len() as i64;
        if offset < 0 || offset > size {
            return false;
        }
        size -= offset;
        if relative_offset < 0 || relative_offset > size {
            return false;
        }
        size -= relative_offset;
        if first_element_index < 0
            || element_count < 0
            || element_size < 0
            || first_element_index > i16::MAX as i64
            || element_count > u16::MAX as i64
        {
            return false;
        }
        element_size == 0 || first_element_index + element_count <= size / element_size
    }

    /// `animation_frame_data_offset`.
    fn frame_data_offset(&self, frame_index: i16) -> i64 {
        if self.compressed() { self.compressed_data_offset as i64 } else { frame_index as i64 * self.frame_size as i64 }
    }

    /// `animation_frame_valid` (whether the frame reads the default data
    /// too: a base animation's does, the overlays' don't).
    fn frame_valid(&self, frame_index: i16, reads_default_data: bool) -> bool {
        let frame_offset = self.frame_data_offset(frame_index);
        let node_count = self.node_count;
        if node_count < 0 || node_count as usize > MAXIMUM_NODES {
            return false;
        }
        if self.compressed() {
            self.contains(frame_offset, 0, 0, 1, HEADER_SIZE) && self.contains(self.compressed_data_offset as i64, 0, 0, 1, HEADER_SIZE)
        } else {
            let count = |flags: [u32; 2]| -> i64 {
                let mut total = 0i64;
                for (long_index, &flags) in flags.iter().enumerate() {
                    let bits = node_count as i64 - long_index as i64 * 32;
                    if bits <= 0 {
                        break;
                    }
                    let word = if bits < 32 { flags & ((1u32 << bits) - 1) } else { flags };
                    total += word.count_ones() as i64;
                }
                total
            };
            let (rotations, translations, scales) = (count(self.rotation_flags), count(self.translation_flags), count(self.scale_flags));
            let frame_bytes = rotations * 8 + translations * 12 + scales * 4;
            let n = node_count as i64;
            let default_bytes = (n - rotations) * 8 + (n - translations) * 12 + (n - scales) * 4;
            self.contains(frame_offset, 0, 0, 1, frame_bytes) && (!reads_default_data || default_bytes <= self.default_data.len() as i64)
        }
    }

    /// A long of the compressed header.
    fn header(&self, field: usize) -> i64 {
        u32_at(&self.data, self.compressed_data_offset as usize + field * 4) as i32 as i64
    }

    /// The compressed data, from its header.
    fn compressed_bytes(&self) -> &[u8] {
        self.data.get(self.compressed_data_offset.max(0) as usize..).unwrap_or(&[])
    }
}

/// `animation_keyframe_search`: the keyframe a frame falls after.
fn keyframe_search(indices: &[i32], target: i32) -> usize {
    let count = indices.len() as i32;
    let (mut low, mut high) = (0i32, count - 1);
    let mut keyframe_index;
    let mut guard = 0;
    loop {
        keyframe_index = (low + high) >> 1;
        if keyframe_index + 1 < count && indices[(keyframe_index + 1) as usize] <= target {
            low = keyframe_index;
        } else if indices[keyframe_index as usize] > target {
            high = keyframe_index;
        } else {
            break;
        }
        guard += 1;
        if guard >= 200 {
            break;
        }
    }
    keyframe_index.clamp(0, count - 2) as usize
}

/// Which keyframes a frame lies between, as the keyframe readers choose:
/// `None` past the last; otherwise (this frame, this keyframe, next frame,
/// next keyframe), a keyframe being `Some(index)` or `None` for the
/// default.
#[allow(clippy::type_complexity)]
fn keyframe_span(indices: &[i32], frame_index: i32) -> Option<(i32, Option<usize>, i32, Option<usize>)> {
    let count = indices.len();
    let last = indices[count - 1];
    if frame_index > last {
        return None;
    }
    Some(if frame_index < indices[0] {
        (0, None, indices[0], Some(0))
    } else if frame_index == last {
        (last, Some(count - 1), last + 1, None)
    } else {
        let k = keyframe_search(indices, frame_index);
        (indices[k], Some(k), indices[k + 1], Some(k + 1))
    })
}

/// `animation_get_keyframe_rotation`.
fn keyframe_rotation(animation: &PoseAnimation, real_frame_index: f32, adjusted_node_index: i64, node_index: i64) -> [f32; 4] {
    let identity = [0.0, 0.0, 0.0, 1.0];
    let base = animation.compressed_data_offset as i64;
    let default_rotations = animation.header(DEFAULT_ROTATIONS);
    if !animation.contains(base, 0, 0, 1, HEADER_SIZE)
        || !animation.contains(base, HEADER_SIZE, adjusted_node_index, 1, 4)
        || !animation.contains(base, default_rotations, node_index, 1, 6)
    {
        return identity;
    }
    let data = animation.compressed_bytes();
    let node_header = u32_at(data, (HEADER_SIZE + adjusted_node_index * 4) as usize);
    let first = (node_header >> KEYFRAME_COUNT_BITS) as u16 as i16 as i64;
    let count = (node_header & ((1 << KEYFRAME_COUNT_BITS) - 1)) as i64;
    let keyframes = animation.header(ROTATION_KEYFRAMES);
    let frame_indices = animation.header(ROTATION_KEYFRAME_FRAME_INDICES);
    if count > 0 && (!animation.contains(base, keyframes, first, count, 6) || !animation.contains(base, frame_indices, first, count, 2)) {
        return identity;
    }
    let default_at = (default_rotations + node_index * 6) as usize;
    if count == 0 {
        return quaternion_normalize(decompress_6byte(data, default_at));
    }
    let keyframe_at = |k: usize| (keyframes + (first + k as i64) * 6) as usize;
    let indices: Vec<i32> = (0..count).map(|k| u16_at(data, (frame_indices + (first + k) * 2) as usize) as i32).collect();
    let frame_index = real_frame_index.floor() as i32 as i16 as i32;
    let Some((this_frame, this, next_frame, next)) = keyframe_span(&indices, frame_index) else {
        return quaternion_normalize(decompress_6byte(data, keyframe_at(count as usize - 1)));
    };
    let at = |k: Option<usize>| k.map_or(default_at, keyframe_at);
    if real_frame_index == this_frame as f32 {
        quaternion_normalize(decompress_6byte(data, at(this)))
    } else {
        let fraction = (real_frame_index - this_frame as f32) / (next_frame - this_frame) as f32;
        quaternions_interpolate_and_normalize(decompress_6byte(data, at(this)), decompress_6byte(data, at(next)), fraction)
    }
}

/// `animation_get_keyframe_translation`.
fn keyframe_translation(animation: &PoseAnimation, real_frame_index: f32, adjusted_node_index: i64, node_index: i64) -> [f32; 3] {
    let base = animation.compressed_data_offset as i64;
    let node_headers = animation.header(TRANSLATION_NODE_HEADERS);
    let defaults = animation.header(DEFAULT_TRANSLATIONS);
    if !animation.contains(base, 0, 0, 1, HEADER_SIZE)
        || !animation.contains(base, node_headers, adjusted_node_index, 1, 4)
        || !animation.contains(base, defaults, node_index, 1, 12)
    {
        return [0.0; 3];
    }
    let data = animation.compressed_bytes();
    let node_header = u32_at(data, (node_headers + adjusted_node_index * 4) as usize);
    let first = (node_header >> KEYFRAME_COUNT_BITS) as u16 as i16 as i64;
    let count = (node_header & ((1 << KEYFRAME_COUNT_BITS) - 1)) as i64;
    let keyframes = animation.header(TRANSLATION_KEYFRAMES);
    let frame_indices = animation.header(TRANSLATION_KEYFRAME_FRAME_INDICES);
    if count > 0 && (!animation.contains(base, keyframes, first, count, 12) || !animation.contains(base, frame_indices, first, count, 2)) {
        return [0.0; 3];
    }
    let default_at = (defaults + node_index * 12) as usize;
    if count == 0 {
        return vec3_at(data, default_at);
    }
    let keyframe_at = |k: usize| (keyframes + (first + k as i64) * 12) as usize;
    let indices: Vec<i32> = (0..count).map(|k| u16_at(data, (frame_indices + (first + k) * 2) as usize) as i32).collect();
    let frame_index = real_frame_index.floor() as i32 as i16 as i32;
    let Some((this_frame, this, next_frame, next)) = keyframe_span(&indices, frame_index) else {
        return vec3_at(data, keyframe_at(count as usize - 1));
    };
    let at = |k: Option<usize>| k.map_or(default_at, keyframe_at);
    let a = vec3_at(data, at(this));
    if real_frame_index == this_frame as f32 {
        a
    } else {
        let t = (real_frame_index - this_frame as f32) / (next_frame - this_frame) as f32;
        let b = vec3_at(data, at(next));
        // (points_interpolate)
        let one_minus_t = 1.0 - t;
        [one_minus_t * a[0] + t * b[0], one_minus_t * a[1] + t * b[1], one_minus_t * a[2] + t * b[2]]
    }
}

/// `animation_get_keyframe_scale`.
fn keyframe_scale(animation: &PoseAnimation, real_frame_index: f32, adjusted_node_index: i64) -> f32 {
    let base = animation.compressed_data_offset as i64;
    let node_headers = animation.header(SCALE_NODE_HEADERS);
    let defaults = animation.header(DEFAULT_SCALES);
    if !animation.contains(base, 0, 0, 1, HEADER_SIZE)
        || !animation.contains(base, node_headers, adjusted_node_index, 1, 4)
        || !animation.contains(base, defaults, adjusted_node_index, 1, 4)
    {
        return 1.0;
    }
    let data = animation.compressed_bytes();
    let node_header = u32_at(data, (node_headers + adjusted_node_index * 4) as usize);
    let first = (node_header >> KEYFRAME_COUNT_BITS) as u16 as i16 as i64;
    let count = (node_header & ((1 << KEYFRAME_COUNT_BITS) - 1)) as i64;
    let keyframes = animation.header(SCALE_KEYFRAMES);
    let frame_indices = animation.header(SCALE_KEYFRAME_FRAME_INDICES);
    if count > 0 && (!animation.contains(base, keyframes, first, count, 4) || !animation.contains(base, frame_indices, first, count, 2)) {
        return 1.0;
    }
    let default = f32_at(data, (defaults + adjusted_node_index * 4) as usize);
    if count == 0 {
        return default;
    }
    let keyframe = |k: usize| f32_at(data, (keyframes + (first + k as i64) * 4) as usize);
    let indices: Vec<i32> = (0..count).map(|k| u16_at(data, (frame_indices + (first + k) * 2) as usize) as i32).collect();
    let frame_index = real_frame_index.floor() as i32 as i16 as i32;
    let Some((this_frame, this, next_frame, next)) = keyframe_span(&indices, frame_index) else {
        return keyframe(count as usize - 1);
    };
    let value = |k: Option<usize>| k.map_or(default, keyframe);
    if real_frame_index == this_frame as f32 {
        value(this)
    } else {
        let t = (real_frame_index - this_frame as f32) / (next_frame - this_frame) as f32;
        // (scalars_interpolate)
        (1.0 - t) * value(this) + value(next) * t
    }
}

/// `animation_get_node_orientations`: an animation's frame, or (for an
/// animation that doesn't fit the model) the model's default pose.
pub fn animation_node_orientations(model: &PoseModel, animation: &PoseAnimation, frame_index: i16) -> Vec<Orientation> {
    let fits = animation.kind == ANIMATION_BASE
        && (animation.node_list_checksum == 0 || animation.node_list_checksum == model.node_list_checksum || model.node_list_checksum == 0)
        && model.nodes.len() == animation.node_count as usize
        && animation.frame_valid(frame_index, true);
    if !fits {
        return model_node_orientations(model);
    }
    let compressed = animation.compressed();
    let data_offset = animation.frame_data_offset(frame_index);
    let mut data = data_offset as usize;
    let mut default_data = 0usize;
    let (mut rotation_index, mut translation_index, mut scale_index) = (0i64, 0i64, 0i64);
    let (mut rotation_flags, mut translation_flags, mut scale_flags) = (0u32, 0u32, 0u32);
    let real_frame = frame_index as f32;
    let mut out = Vec::with_capacity(animation.node_count as usize);
    for node_index in 0..animation.node_count as usize {
        if node_index & 31 == 0 {
            let long_index = node_index >> 5;
            translation_flags = animation.translation_flags[long_index];
            rotation_flags = animation.rotation_flags[long_index];
            scale_flags = animation.scale_flags[long_index];
        }
        let mut orientation = Orientation::default();

        orientation.rotation = if rotation_flags & 1 != 0 {
            if compressed {
                let r = keyframe_rotation(animation, real_frame, rotation_index, node_index as i64);
                rotation_index += 1;
                r
            } else {
                let r = decompress_8byte(&animation.data, data);
                data += 8;
                r
            }
        } else if compressed {
            let defaults = animation.header(DEFAULT_ROTATIONS);
            if animation.contains(data_offset, defaults, node_index as i64, 1, 6) {
                quaternion_normalize(decompress_6byte(&animation.data, (data_offset + defaults) as usize + node_index * 6))
            } else {
                [0.0, 0.0, 0.0, 1.0]
            }
        } else {
            let r = decompress_8byte(&animation.default_data, default_data);
            default_data += 8;
            r
        };
        rotation_flags >>= 1;

        orientation.translation = if translation_flags & 1 != 0 {
            if compressed {
                let t = keyframe_translation(animation, real_frame, translation_index, node_index as i64);
                translation_index += 1;
                t
            } else {
                let t = vec3_at(&animation.data, data);
                data += 12;
                t
            }
        } else if compressed {
            let defaults = animation.header(DEFAULT_TRANSLATIONS);
            if animation.contains(data_offset, defaults, node_index as i64, 1, 12) {
                vec3_at(&animation.data, (data_offset + defaults) as usize + node_index * 12)
            } else {
                [0.0; 3]
            }
        } else {
            let t = vec3_at(&animation.default_data, default_data);
            default_data += 12;
            t
        };
        translation_flags >>= 1;

        orientation.scale = if scale_flags & 1 != 0 {
            if compressed {
                let s = keyframe_scale(animation, real_frame, scale_index);
                scale_index += 1;
                s
            } else {
                let s = f32_at(&animation.data, data);
                data += 4;
                s
            }
        } else if compressed {
            1.0
        } else {
            let s = f32_at(&animation.default_data, default_data);
            default_data += 4;
            s
        };
        scale_flags >>= 1;
        out.push(orientation);
    }
    out
}

// ---------- applying animations over a pose

/// `quaternions_multiply`: `q0 * q1`, summed in the game's order.
pub fn quaternions_multiply(q0: [f32; 4], q1: [f32; 4]) -> [f32; 4] {
    let [i0, j0, k0, w0] = q0;
    let [i1, j1, k1, w1] = q1;
    [
        j0 * k1 + i0 * w1 + w0 * i1 - k0 * j1,
        w0 * j1 + j0 * w1 + k0 * i1 - i0 * k1,
        k0 * w1 + i0 * j1 + w0 * k1 - j0 * i1,
        w0 * w1 - i0 * i1 - j0 * j1 - k0 * k1,
    ]
}

/// `quaternions_interpolate` (not normalized).
pub fn quaternions_interpolate(q0: [f32; 4], q1: [f32; 4], t: f32) -> [f32; 4] {
    let v = 1.0 - t;
    let t = if q0[0] * q1[0] + q0[1] * q1[1] + q0[2] * q1[2] + q0[3] * q1[3] < 0.0 { -t } else { t };
    [q0[0] * v + q1[0] * t, q0[1] * v + q1[1] * t, q0[2] * v + q1[2] * t, q0[3] * v + q1[3] * t]
}

/// `points_interpolate`.
fn points_interpolate(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    let one_minus_t = 1.0 - t;
    [one_minus_t * a[0] + t * b[0], one_minus_t * a[1] + t * b[1], one_minus_t * a[2] + t * b[2]]
}

/// `scalars_interpolate`.
fn scalars_interpolate(a: f32, b: f32, t: f32) -> f32 {
    (1.0 - t) * a + b * t
}

/// Poses blended node by node (the port's `unit_node_orientations_blend`:
/// rotations interpolated and normalized, translations and scales
/// interpolated).
pub fn orientations_blend(from: &[Orientation], to: &[Orientation], fraction: f32, node_count: usize) -> Vec<Orientation> {
    (0..node_count)
        .map(|n| Orientation {
            rotation: quaternions_interpolate_and_normalize(from[n].rotation, to[n].rotation, fraction),
            translation: points_interpolate(from[n].translation, to[n].translation, fraction),
            scale: scalars_interpolate(from[n].scale, to[n].scale, fraction),
        })
        .collect()
}

/// An aiming (or looking, or steering) screen's grid: how many frames to
/// either side in yaw and pitch, and the angle a frame turns
/// (`animation_aiming_screen_bounds`).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ScreenBounds {
    pub negative_yaw_delta: f32,
    pub positive_yaw_delta: f32,
    pub negative_yaw_frame_count: i16,
    pub positive_yaw_frame_count: i16,
    pub negative_pitch_delta: f32,
    pub positive_pitch_delta: f32,
    pub negative_pitch_frame_count: i16,
    pub positive_pitch_frame_count: i16,
}

impl ScreenBounds {
    pub fn read(ptr: crate::Ptr<'_>) -> Result<Self> {
        Ok(ScreenBounds {
            negative_yaw_delta: ptr.f32(0)?,
            positive_yaw_delta: ptr.f32(4)?,
            negative_yaw_frame_count: ptr.i16(8)?,
            positive_yaw_frame_count: ptr.i16(10)?,
            negative_pitch_delta: ptr.f32(12)?,
            positive_pitch_delta: ptr.f32(16)?,
            negative_pitch_frame_count: ptr.i16(20)?,
            positive_pitch_frame_count: ptr.i16(22)?,
        })
    }

    /// The rectangle of angles the screen covers (x0, x1, y0, y1), as a
    /// unit keeps it.
    pub fn rectangle(&self) -> [f32; 4] {
        [
            -(self.negative_yaw_frame_count as f32 * self.negative_yaw_delta),
            self.positive_yaw_frame_count as f32 * self.positive_yaw_delta,
            -(self.negative_pitch_frame_count as f32 * self.negative_pitch_delta),
            self.positive_pitch_frame_count as f32 * self.positive_pitch_delta,
        ]
    }
}

/// The nodes an animation's frame has data for, in order, each with
/// whether it has a rotation, a translation and a scale.
fn animated_nodes(animation: &PoseAnimation) -> impl Iterator<Item = (usize, bool, bool, bool)> + '_ {
    (0..animation.node_count.max(0) as usize).map(move |node_index| {
        let (long_index, bit) = (node_index >> 5, node_index & 31);
        (
            node_index,
            animation.rotation_flags[long_index] >> bit & 1 != 0,
            animation.translation_flags[long_index] >> bit & 1 != 0,
            animation.scale_flags[long_index] >> bit & 1 != 0,
        )
    })
}

/// `replacement_animation_apply`: the frame's nodes replace the pose's.
pub fn replacement_animation_apply(animation: &PoseAnimation, frame_index: i16, orientations: &mut [Orientation]) {
    if animation.kind != ANIMATION_REPLACEMENT || frame_index < 0 || frame_index >= animation.frame_count || !animation.frame_valid(frame_index, false) {
        return;
    }
    let compressed = animation.compressed();
    let mut data = animation.frame_data_offset(frame_index) as usize;
    let (mut rotation_index, mut translation_index, mut scale_index) = (0i64, 0i64, 0i64);
    let real_frame = frame_index as f32;
    for (node_index, rotates, translates, scales) in animated_nodes(animation) {
        let orientation = &mut orientations[node_index];
        if rotates {
            if compressed {
                orientation.rotation = keyframe_rotation(animation, real_frame, rotation_index, node_index as i64);
                rotation_index += 1;
            } else {
                orientation.rotation = decompress_8byte(&animation.data, data);
                data += 8;
            }
        }
        if translates {
            if compressed {
                orientation.translation = keyframe_translation(animation, real_frame, translation_index, node_index as i64);
                translation_index += 1;
            } else {
                orientation.translation = vec3_at(&animation.data, data);
                data += 12;
            }
        }
        if scales {
            if compressed {
                orientation.scale = keyframe_scale(animation, real_frame, scale_index);
                scale_index += 1;
            } else {
                orientation.scale = f32_at(&animation.data, data);
                data += 4;
            }
        }
    }
}

/// An overlay's frame read node by node: each node's rotation, translation
/// and scale, those it has.
fn overlay_frame(animation: &PoseAnimation, frame_index: i16) -> Vec<(usize, Option<[f32; 4]>, Option<[f32; 3]>, Option<f32>)> {
    let compressed = animation.compressed();
    let mut data = animation.frame_data_offset(frame_index) as usize;
    let (mut rotation_index, mut translation_index, mut scale_index) = (0i64, 0i64, 0i64);
    let real_frame = frame_index as f32;
    let mut out = Vec::new();
    for (node_index, rotates, translates, scales) in animated_nodes(animation) {
        let rotation = rotates.then(|| {
            if compressed {
                rotation_index += 1;
                keyframe_rotation(animation, real_frame, rotation_index - 1, node_index as i64)
            } else {
                data += 8;
                decompress_8byte(&animation.data, data - 8)
            }
        });
        let translation = translates.then(|| {
            if compressed {
                translation_index += 1;
                keyframe_translation(animation, real_frame, translation_index - 1, node_index as i64)
            } else {
                data += 12;
                vec3_at(&animation.data, data - 12)
            }
        });
        let scale = scales.then(|| {
            if compressed {
                scale_index += 1;
                keyframe_scale(animation, real_frame, scale_index - 1)
            } else {
                data += 4;
                f32_at(&animation.data, data - 4)
            }
        });
        out.push((node_index, rotation, translation, scale));
    }
    out
}

/// `overlay_animation_apply`: the frame's rotations turn the pose's, its
/// translations add to them and its scales multiply them.
pub fn overlay_animation_apply(animation: &PoseAnimation, frame_index: i16, orientations: &mut [Orientation]) {
    if animation.kind != ANIMATION_OVERLAY || frame_index < 0 || frame_index >= animation.frame_count || !animation.frame_valid(frame_index, false) {
        return;
    }
    for (node_index, rotation, translation, scale) in overlay_frame(animation, frame_index) {
        let orientation = &mut orientations[node_index];
        if let Some(rotation) = rotation {
            orientation.rotation = quaternions_multiply(rotation, orientation.rotation);
        }
        if let Some(t) = translation {
            orientation.translation[0] += t[0];
            orientation.translation[1] += t[1];
            orientation.translation[2] += t[2];
        }
        if let Some(scale) = scale {
            orientation.scale *= scale;
        }
    }
}

/// `overlay_animation_apply_scaled`: an overlay applied `animation_scale`
/// of the way.
pub fn overlay_animation_apply_scaled(animation: &PoseAnimation, frame_index: i16, animation_scale: f32, orientations: &mut [Orientation]) {
    let inverse_animation_scale = 1.0 - animation_scale;
    if animation.kind != ANIMATION_OVERLAY || frame_index < 0 || frame_index >= animation.frame_count || !animation.frame_valid(frame_index, false) {
        return;
    }
    for (node_index, rotation, translation, scale) in overlay_frame(animation, frame_index) {
        let orientation = &mut orientations[node_index];
        if let Some(rotation) = rotation {
            let rotation = quaternions_interpolate([0.0, 0.0, 0.0, 1.0], rotation, animation_scale);
            orientation.rotation = quaternions_multiply(rotation, orientation.rotation);
        }
        if let Some(t) = translation {
            orientation.translation[0] += t[0] * animation_scale;
            orientation.translation[1] += t[1] * animation_scale;
            orientation.translation[2] += t[2] * animation_scale;
        }
        if let Some(scale) = scale {
            orientation.scale *= scale * animation_scale + inverse_animation_scale;
        }
    }
}

/// `overlay_animation_apply_continuous`: an overlay at a point between two
/// frames (the last blending into the first). A compressed animation's
/// rotations are read at the whole frame, as the game's are.
pub fn overlay_animation_apply_continuous(animation: &PoseAnimation, real_frame_index: f32, orientations: &mut [Orientation]) {
    if animation.frame_count <= 0 {
        return;
    }
    let mut real_frame_index = real_frame_index;
    let mut fraction = (real_frame_index as f64 % 1.0) as f32;
    // (fabs and floor in double, then fast_ftol: rounded to nearest)
    let whole = (real_frame_index as f64).abs().floor() as f32;
    let mut frame_index = (whole as f64).round_ties_even() as i32 as i16;
    if frame_index >= animation.frame_count {
        frame_index = animation.frame_count - 1;
        fraction = 1.0;
        real_frame_index = frame_index as f32;
    }
    let next_frame_index = if frame_index == animation.frame_count - 1 { 0 } else { frame_index + 1 };
    if animation.kind != ANIMATION_OVERLAY || !animation.frame_valid(frame_index, false) || !animation.frame_valid(next_frame_index, false) {
        return;
    }
    let compressed = animation.compressed();
    let mut data = animation.frame_data_offset(frame_index) as usize;
    let mut next_data = animation.frame_data_offset(next_frame_index) as usize;
    let (mut rotation_index, mut translation_index, mut scale_index) = (0i64, 0i64, 0i64);
    for (node_index, rotates, translates, scales) in animated_nodes(animation) {
        let orientation = &mut orientations[node_index];
        if rotates {
            let rotation = if compressed {
                rotation_index += 1;
                keyframe_rotation(animation, frame_index as f32, rotation_index - 1, node_index as i64)
            } else {
                let this_rotation = decompress_8byte(&animation.data, data);
                data += 8;
                let next_rotation = decompress_8byte(&animation.data, next_data);
                next_data += 8;
                quaternions_interpolate_and_normalize(this_rotation, next_rotation, fraction)
            };
            orientation.rotation = quaternions_multiply(rotation, orientation.rotation);
        }
        if translates {
            let t = if compressed {
                translation_index += 1;
                keyframe_translation(animation, real_frame_index, translation_index - 1, node_index as i64)
            } else {
                let this_translation = vec3_at(&animation.data, data);
                data += 12;
                let next_translation = vec3_at(&animation.data, next_data);
                next_data += 12;
                points_interpolate(this_translation, next_translation, fraction)
            };
            orientation.translation[0] += t[0];
            orientation.translation[1] += t[1];
            orientation.translation[2] += t[2];
        }
        if scales {
            let scale = if compressed {
                scale_index += 1;
                keyframe_scale(animation, real_frame_index, scale_index - 1)
            } else {
                let this_scale = f32_at(&animation.data, data);
                data += 4;
                let next_scale = f32_at(&animation.data, next_data);
                next_data += 4;
                scalars_interpolate(this_scale, next_scale, fraction)
            };
            orientation.scale *= scale;
        }
    }
}

/// One axis of an aiming screen: the cell an angle falls in (counted from
/// the most negative) and how far across it.
fn screen_cell(angle: f32, negative_delta: f32, positive_delta: f32, negative_frame_count: i16, positive_frame_count: i16) -> (i16, f32) {
    let delta = if angle < 0.0 { negative_delta } else { positive_delta };
    let frame = if delta == 0.0 { 0.0 } else { angle / delta };
    let mut frame_index = frame as i32 as i16;
    let mut fraction = (frame as f64 % 1.0) as f32;
    if fraction < 0.0 {
        fraction += 1.0;
        frame_index = frame_index.wrapping_sub(1);
    }
    if frame_index >= positive_frame_count {
        frame_index = positive_frame_count.wrapping_sub(1);
        fraction = 1.0;
    }
    if (frame_index as i32) < -(negative_frame_count as i32) {
        frame_index = (-(negative_frame_count as i32)) as i16;
        fraction = 0.0;
    }
    (frame_index.wrapping_add(negative_frame_count), fraction)
}

/// `aiming_screen_apply`: the overlay's grid of frames (yaw across, pitch
/// down) blended at the angles, as rotations turning the pose's and
/// translations added to it. (It reads no scales, and an uncompressed
/// frame's scales are not skipped, as the game's aren't.)
pub fn aiming_screen_apply(animation: &PoseAnimation, bounds: &ScreenBounds, yaw: f32, pitch: f32, orientations: &mut [Orientation]) {
    let grid_width = bounds.negative_yaw_frame_count as i32 + bounds.positive_yaw_frame_count as i32 + 1;
    let grid_height = bounds.negative_pitch_frame_count as i32 + bounds.positive_pitch_frame_count as i32 + 1;
    let (grid_width, grid_height) = (grid_width as i16 as i32, grid_height as i16 as i32);
    if animation.kind != ANIMATION_OVERLAY || (animation.frame_count as i32) < grid_width * grid_height {
        return;
    }
    let compressed = animation.compressed();
    let (yaw_frame_index, yaw_fraction) =
        screen_cell(yaw, bounds.negative_yaw_delta, bounds.positive_yaw_delta, bounds.negative_yaw_frame_count, bounds.positive_yaw_frame_count);
    let (pitch_frame_index, pitch_fraction) = screen_cell(
        pitch,
        bounds.negative_pitch_delta,
        bounds.positive_pitch_delta,
        bounds.negative_pitch_frame_count,
        bounds.positive_pitch_frame_count,
    );
    let (yaw_frame_index, pitch_frame_index) = (yaw_frame_index as i32, pitch_frame_index as i32);
    if pitch_frame_index < 0 || pitch_frame_index >= grid_height || yaw_frame_index < 0 || yaw_frame_index >= grid_width {
        return;
    }
    let next_yaw_cell = if yaw_frame_index + 1 == grid_width { yaw_frame_index } else { yaw_frame_index + 1 };
    let next_pitch_cell = if pitch_frame_index + 1 == grid_height { pitch_frame_index } else { pitch_frame_index + 1 };
    let frames = [
        (yaw_frame_index + pitch_frame_index * grid_width) as i16,
        (next_yaw_cell + pitch_frame_index * grid_width) as i16,
        (yaw_frame_index + next_pitch_cell * grid_width) as i16,
        (next_yaw_cell + next_pitch_cell * grid_width) as i16,
    ];
    if frames.iter().any(|&f| !animation.frame_valid(f, false)) {
        return;
    }
    let mut data = frames.map(|f| animation.frame_data_offset(f) as usize);
    let (mut rotation_index, mut translation_index) = (0i64, 0i64);
    for (node_index, rotates, translates, _) in animated_nodes(animation) {
        let orientation = &mut orientations[node_index];
        if rotates {
            let r: [[f32; 4]; 4] = if compressed {
                let r = frames.map(|f| keyframe_rotation(animation, f as f32, rotation_index, node_index as i64));
                rotation_index += 1;
                r
            } else {
                let r = data.map(|d| decompress_8byte(&animation.data, d));
                data = data.map(|d| d + 8);
                r
            };
            let yaw_rotation0 = quaternions_interpolate_and_normalize(r[0], r[1], yaw_fraction);
            let yaw_rotation1 = quaternions_interpolate_and_normalize(r[2], r[3], yaw_fraction);
            let rotation = quaternions_interpolate_and_normalize(yaw_rotation0, yaw_rotation1, pitch_fraction);
            orientation.rotation = quaternions_multiply(rotation, orientation.rotation);
        }
        if translates {
            let inverse_yaw_fraction = 1.0 - yaw_fraction;
            let inverse_pitch_fraction = 1.0 - pitch_fraction;
            let t: [[f32; 3]; 4] = if compressed {
                let t = frames.map(|f| keyframe_translation(animation, f as f32, translation_index, node_index as i64));
                translation_index += 1;
                t
            } else {
                let t = data.map(|d| vec3_at(&animation.data, d));
                data = data.map(|d| d + 12);
                t
            };
            let (t00, t10, t01, t11) = (t[0], t[1], t[2], t[3]);
            orientation.translation[0] += (t01[0] * inverse_yaw_fraction + t11[0] * yaw_fraction) * pitch_fraction
                + (t00[0] * inverse_yaw_fraction + t10[0] * yaw_fraction) * inverse_pitch_fraction;
            orientation.translation[1] += (t01[1] * inverse_yaw_fraction + t11[1] * yaw_fraction) * pitch_fraction
                + (t00[1] * inverse_yaw_fraction + t10[1] * yaw_fraction) * inverse_pitch_fraction;
            orientation.translation[2] += (t00[2] * inverse_yaw_fraction + t10[2] * yaw_fraction) * inverse_pitch_fraction
                + (t01[2] * inverse_yaw_fraction + t11[2] * yaw_fraction) * pitch_fraction;
        }
    }
}

/// `interpolate_node_orientations`: the pose blended from `original`
/// toward `target`, `frame_index + 1` of `frame_count` of the way.
pub fn interpolate_node_orientations(original: &[Orientation], target: &mut [Orientation], frame_index: i16, frame_count: i16) {
    let fraction = (frame_index as i32 + 1) as f32 / frame_count as f32;
    let inverse_fraction = 1.0 - fraction;
    for (t, o) in target.iter_mut().zip(original).take(MAXIMUM_NODES) {
        t.scale = o.scale * inverse_fraction + t.scale * fraction;
        t.rotation = quaternions_interpolate_and_normalize(o.rotation, t.rotation, fraction);
        for c in 0..3 {
            t.translation[c] = o.translation[c] * inverse_fraction + t.translation[c] * fraction;
        }
    }
}

// ---------- matrices

/// `matrix4x3_from_orientation` (`matrix4x3_rotation_from_quaternion`).
pub fn matrix_from_orientation(o: &Orientation) -> Matrix4x3 {
    let [qi, qj, qk, qw] = o.rotation;
    let mut scale = qi * qi + qj * qj + qk * qk + qw * qw;
    scale = if scale != 0.0 { 2.0 / scale } else { 0.0 };
    let (x, y, z) = (scale * qi, scale * qj, scale * qk);
    let (wx, wy, wz) = (qw * x, qw * y, qw * z);
    let (xx, xy, xz) = (qi * x, qi * y, qi * z);
    let (yy, yz, zz) = (qj * y, qj * z, qk * z);
    Matrix4x3 {
        scale: o.scale,
        forward: [1.0 - (yy + zz), xy - wz, xz + wy],
        left: [xy + wz, 1.0 - (xx + zz), yz - wx],
        up: [xz - wy, yz + wx, 1.0 - (xx + yy)],
        position: o.translation,
    }
}

/// `matrix4x3_translation`.
fn translation_matrix(p: [f32; 3]) -> Matrix4x3 {
    Matrix4x3 { scale: 1.0, forward: [1.0, 0.0, 0.0], left: [0.0, 1.0, 0.0], up: [0.0, 0.0, 1.0], position: p }
}

/// `matrix4x3_multiply`: `a * b`, summed as the game's (the rows of `b`
/// combining the rows of `a`; the position scaled by `a`'s scale before
/// `a`'s own is added).
pub fn matrix_multiply(a: &Matrix4x3, b: &Matrix4x3) -> Matrix4x3 {
    let rows_a = [a.forward, a.left, a.up];
    let row = |r: [f32; 3], position: bool| -> [f32; 3] {
        let mut out = [0.0f32; 3];
        for c in 0..3 {
            let value = r[0] * rows_a[0][c] + r[1] * rows_a[1][c] + r[2] * rows_a[2][c];
            out[c] = if position { value * a.scale + a.position[c] } else { value };
        }
        out
    };
    Matrix4x3 {
        scale: a.scale * b.scale,
        forward: row(b.forward, false),
        left: row(b.left, false),
        up: row(b.up, false),
        position: row(b.position, true),
    }
}

/// What placing an object's nodes reads besides its orientations.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Placement {
    pub position: [f32; 3],
    pub forward: [f32; 3],
    pub up: [f32; 3],
    pub mirrored: bool,
    pub world_relative: bool,
    /// Its physics' centre of mass, if it has physics.
    pub center_of_mass: Option<[f32; 3]>,
    pub origin_offset: [f32; 3],
    /// Its parent's node matrix it hangs from, and whether the parent is
    /// mirrored.
    pub parent: Option<(Matrix4x3, bool)>,
}

/// `object_compute_node_matrices`' walk: the root placed in the world,
/// each other node under its parent, in the order the game visits them.
pub fn node_matrices(model: &PoseModel, orientations: &[Orientation], placement: &Placement) -> Vec<Matrix4x3> {
    let model_node_count = model.nodes.len().min(MAXIMUM_NODES);
    let mut matrices = vec![Matrix4x3::default(); model_node_count];
    if model_node_count == 0 {
        return matrices;
    }
    let mut stack = vec![0usize];
    let mut index = 0usize;
    while index < stack.len() {
        let node_index = stack[index];
        index += 1;
        let node = model.nodes[node_index];
        let orientation = orientations.get(node_index).copied().unwrap_or_default();
        if node_index == 0 {
            let node_matrix = matrix_from_orientation(&orientation);
            matrices[0] = if !placement.world_relative {
                let mut translation = translation_matrix(placement.position);
                let mut rotation = Matrix4x3 {
                    scale: 1.0,
                    forward: placement.forward,
                    left: crate::features::cross3(placement.up, placement.forward),
                    up: placement.up,
                    position: [0.0; 3],
                };
                if placement.mirrored {
                    rotation.left = [-rotation.left[0], -rotation.left[1], -rotation.left[2]];
                }
                if let Some(c) = placement.center_of_mass {
                    rotation = matrix_multiply(&rotation, &translation_matrix([-c[0], -c[1], -c[2]]));
                }
                rotation = matrix_multiply(&rotation, &translation_matrix(placement.origin_offset));
                if let Some((parent, parent_mirrored)) = placement.parent {
                    let mut parent = parent;
                    if parent.scale != 1.0 {
                        translation.position[0] *= parent.scale;
                        translation.position[1] *= parent.scale;
                        translation.position[2] *= parent.scale;
                        parent.scale = 1.0;
                    }
                    if parent_mirrored {
                        parent.left = [-parent.left[0], -parent.left[1], -parent.left[2]];
                    }
                    let m = matrix_multiply(&parent, &translation);
                    let m = matrix_multiply(&m, &rotation);
                    matrix_multiply(&m, &node_matrix)
                } else {
                    let m = matrix_multiply(&translation, &rotation);
                    matrix_multiply(&m, &node_matrix)
                }
            } else {
                node_matrix
            };
        } else {
            let local = matrix_from_orientation(&orientation);
            let parent = if node.parent >= 0 && (node.parent as usize) < model_node_count { node.parent as usize } else { 0 };
            matrices[node_index] = matrix_multiply(&matrices[parent], &local);
        }
        if node.next_sibling != -1 && node.next_sibling >= 0 && (node.next_sibling as usize) < model_node_count && stack.len() < MAXIMUM_NODES {
            stack.push(node.next_sibling as usize);
        }
        if node.first_child != -1 && node.first_child >= 0 && (node.first_child as usize) < model_node_count && stack.len() < MAXIMUM_NODES {
            stack.push(node.first_child as usize);
        }
    }
    matrices
}

/// An object definition's placement data: its model, animation graph,
/// origin offset and physics.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PoseObjectDefinition {
    pub model: Option<u32>,
    pub animation_graph: Option<u32>,
    pub origin_offset: [f32; 3],
    pub center_of_mass: Option<[f32; 3]>,
    /// Whether its graph has object overlays (not ported).
    pub has_overlays: bool,
}

impl PoseObjectDefinition {
    pub fn load(cache: &CacheFile, datum: u32) -> Result<Self> {
        use layout::_object_definition as o;
        let root = cache.tag_root(datum, crate::groups::OBJECT)?;
        let physics = root.reference(o::PHYSICS)?.datum;
        let center_of_mass = match physics {
            Some(p) => Some(cache.tag_root(p, crate::groups::PHYSICS)?.vec3(layout::physics_definition::CENTER_OF_MASS)?),
            None => None,
        };
        let animation_graph = root.reference(o::ANIMATION_GRAPH)?.datum;
        let has_overlays = match animation_graph {
            Some(g) => cache.tag_root(g, crate::groups::ANIMATION_GRAPH)?.block(layout::animation_graph::OBJECT_OVERLAYS, 1)?.count > 0,
            None => false,
        };
        Ok(PoseObjectDefinition {
            model: root.reference(o::MODEL)?.datum,
            animation_graph,
            origin_offset: root.vec3(o::ORIGIN_OFFSET)?,
            center_of_mass,
            has_overlays,
        })
    }
}
