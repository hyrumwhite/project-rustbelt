//! Models (`mode` tags): the geometry of objects.
//!
//! A model has nodes (a skeleton), regions with permutations, and geometries.
//! A permutation names one geometry per level of detail; index 4 is the most
//! detailed (`render_model`, models.c). A geometry is parts, one shader each.
//!
//! On the Xbox a part's geometry is in the tag data, not its tag blocks: its
//! `vertex_buffer.hardware_format` points at a Direct3D vertex buffer
//! resource (Common, Data, Lock) in the tag header's array, whose Data is the
//! address of compressed model vertices; its `triangle_buffer` addresses a
//! triangle strip of 16-bit indices, `count + 2` long.
//!
//! Vertices are in model space at the default pose: the renderer skins with
//! `node_matrix * runtime_default_inverse_matrix`, which is the object's own
//! matrix while every node is at its default.

use crate::{groups, layout, math, CacheFile, Error, Ptr, Result, TagRef};

/// `_rasterizer_vertex_type_model_compressed`.
const VERTEX_TYPE_MODEL_COMPRESSED: i16 = 5;
const VERTEX_TYPE_MODEL_UNCOMPRESSED: i16 = 4;
const COMPRESSED_VERTEX_SIZE: usize = 32;
/// `_triangle_buffer_type_precompiled_strip`.
const TRIANGLE_BUFFER_STRIP: i16 = 1;

pub const LEVELS_OF_DETAIL: usize = 5;
pub const HIGHEST_DETAIL: usize = LEVELS_OF_DETAIL - 1;

/// `real_matrix4x3`: `position + scale * (x * forward + y * left + z * up)`.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Matrix4x3 {
    pub scale: f32,
    pub forward: [f32; 3],
    pub left: [f32; 3],
    pub up: [f32; 3],
    pub position: [f32; 3],
}

#[derive(Debug, Clone)]
pub struct Node {
    pub name: String,
    pub parent: Option<usize>,
    pub default_translation: [f32; 3],
    /// i, j, k, w.
    pub default_rotation: [f32; 4],
    /// The inverse of the node's default (rest pose) matrix in model space,
    /// as the map was built (`runtime_default_inverse_matrix`): a part
    /// attached to the node is drawn at `node matrix * default inverse`.
    pub default_inverse: Matrix4x3,
}

#[derive(Debug, Clone)]
pub struct Permutation {
    pub name: String,
    /// Geometry index per level of detail (super low .. super high); -1 for none.
    pub geometry_indices: [i16; LEVELS_OF_DETAIL],
}

#[derive(Debug, Clone)]
pub struct Region {
    pub name: String,
    pub permutations: Vec<Permutation>,
}

/// A model vertex (`model_vertex_uncompressed`), texture coordinates already
/// scaled by the model's base map scale.
#[derive(Debug, Clone, Copy, Default)]
pub struct Vertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    pub binormal: [f32; 3],
    pub tangent: [f32; 3],
    pub texcoord: [f32; 2],
    pub nodes: [u8; 2],
    pub node_weights: [f32; 2],
}

#[derive(Debug, Clone)]
pub struct Part {
    pub shader_index: i16,
    pub vertices: Vec<Vertex>,
    /// Triangles, three indices each (strip unrolled, degenerates dropped),
    /// in the game's winding.
    pub indices: Vec<u32>,
}

pub struct Model<'a> {
    pub name: String,
    root: Ptr<'a>,
    pub base_map_scale: [f32; 2],
    pub nodes: Vec<Node>,
    pub regions: Vec<Region>,
    /// The model's shaders; parts index these.
    pub shaders: Vec<TagRef>,
    pub geometry_count: usize,
}

impl<'a> Model<'a> {
    pub fn load(cache: &'a CacheFile, datum: u32) -> Result<Self> {
        use layout::{model as m, model_node as n, model_region as r, model_region_permutation as p};
        let root = cache.tag_root(datum, groups::MODEL)?;
        let nodes = root
            .block(m::NODES, n::SIZEOF)?
            .iter()
            .map(|node| {
                let parent = node.i16(n::PARENT_NODE_INDEX)?;
                let t = n::DEFAULT_TRANSLATION;
                let q = n::DEFAULT_ROTATION;
                Ok(Node {
                    name: node.c_string(32)?,
                    parent: (parent >= 0).then_some(parent as usize),
                    default_translation: node.vec3(t)?,
                    default_rotation: [node.f32(q)?, node.f32(q + 4)?, node.f32(q + 8)?, node.f32(q + 12)?],
                    default_inverse: {
                        let m = n::RUNTIME_DEFAULT_INVERSE_MATRIX;
                        Matrix4x3 {
                            scale: node.f32(m)?,
                            forward: node.vec3(m + 4)?,
                            left: node.vec3(m + 16)?,
                            up: node.vec3(m + 28)?,
                            position: node.vec3(m + 40)?,
                        }
                    },
                })
            })
            .collect::<Result<_>>()?;
        let regions = root
            .block(m::REGIONS, r::SIZEOF)?
            .iter()
            .map(|region| {
                let permutations = region
                    .block(r::PERMUTATIONS, p::SIZEOF)?
                    .iter()
                    .map(|perm| {
                        let g = p::GEOMETRY_INDICES;
                        Ok(Permutation {
                            name: perm.c_string(32)?,
                            geometry_indices: [
                                perm.i16(g)?,
                                perm.i16(g + 2)?,
                                perm.i16(g + 4)?,
                                perm.i16(g + 6)?,
                                perm.i16(g + 8)?,
                            ],
                        })
                    })
                    .collect::<Result<_>>()?;
                Ok(Region { name: region.c_string(32)?, permutations })
            })
            .collect::<Result<_>>()?;
        let shaders = root
            .block(m::SHADERS, layout::model_shader_reference::SIZEOF)?
            .iter()
            .map(|s| s.reference(layout::model_shader_reference::SHADER))
            .collect::<Result<_>>()?;
        let scale = root.vec2(m::BASE_MAP_SCALE)?;
        Ok(Model {
            name: cache.tag(datum)?.name.clone(),
            root,
            base_map_scale: [nonzero(scale[0]), nonzero(scale[1])],
            nodes,
            regions,
            shaders,
            geometry_count: root.block(m::GEOMETRIES, layout::model_geometry::SIZEOF)?.count,
        })
    }

    /// The geometry indices to draw: for each region, the given permutation
    /// (or its first when out of range), at `lod` or the nearest coarser level
    /// that has geometry.
    pub fn geometries_for(&self, permutations: &[u8], lod: usize) -> Vec<usize> {
        let mut out = Vec::new();
        for (region_index, region) in self.regions.iter().enumerate() {
            let wanted = permutations.get(region_index).copied().unwrap_or(0) as usize;
            let Some(permutation) = region.permutations.get(wanted).or_else(|| region.permutations.first()) else {
                continue;
            };
            let geometry = (0..=lod.min(HIGHEST_DETAIL))
                .rev()
                .map(|l| permutation.geometry_indices[l])
                .find(|&g| g >= 0 && (g as usize) < self.geometry_count);
            if let Some(g) = geometry
                && !out.contains(&(g as usize))
            {
                out.push(g as usize);
            }
        }
        out
    }

    /// Decodes a geometry's parts.
    pub fn geometry(&self, index: usize) -> Result<Vec<Part>> {
        use layout::{model_geometry as g, model_geometry_part as gp, triangle_buffer as tb, vertex_buffer as vb};
        let geometries = self.root.block(layout::model::GEOMETRIES, g::SIZEOF)?;
        let geometry = geometries
            .get(index)
            .ok_or_else(|| Error::Format(format!("{}: no geometry {index}", self.name)))?;
        let mut parts = Vec::new();
        for part in geometry.block(g::PARTS, gp::SIZEOF)?.iter() {
            let vertex_type = part.i16(gp::VERTEX_BUFFER + vb::TYPE)?;
            let vertex_count = part.i32(gp::VERTEX_BUFFER + vb::COUNT)?.max(0) as usize;
            if vertex_type != VERTEX_TYPE_MODEL_COMPRESSED {
                let what = if vertex_type == VERTEX_TYPE_MODEL_UNCOMPRESSED { "uncompressed" } else { "unknown" };
                return Err(Error::Unsupported(format!("{}: {what} model vertices ({vertex_type})", self.name)));
            }
            let resource = part.at(part.u32(gp::VERTEX_BUFFER + vb::HARDWARE_FORMAT)?)?;
            let data = resource.at(resource.u32(4)?)?.bytes(0, vertex_count * COMPRESSED_VERTEX_SIZE)?;

            let strip_type = part.i16(gp::TRIANGLE_BUFFER + tb::TYPE)?;
            if strip_type != TRIANGLE_BUFFER_STRIP {
                return Err(Error::Unsupported(format!("{}: triangle buffer type {strip_type}", self.name)));
            }
            let triangle_count = part.i32(gp::TRIANGLE_BUFFER + tb::COUNT)?.max(0) as usize;
            let strip_bytes =
                part.at(part.u32(gp::TRIANGLE_BUFFER + tb::BASE_ADDRESS)?)?.bytes(0, (triangle_count + 2) * 2)?;
            let strip: Vec<u16> = strip_bytes.chunks_exact(2).map(|b| u16::from_le_bytes([b[0], b[1]])).collect();

            let vertices = (0..vertex_count)
                .map(|i| decode_vertex(&data[i * COMPRESSED_VERTEX_SIZE..][..COMPRESSED_VERTEX_SIZE], self.base_map_scale))
                .collect();
            let indices = unroll_strip(&strip, vertex_count)
                .ok_or_else(|| Error::Format(format!("{}: strip indexes past {vertex_count} vertices", self.name)))?;
            parts.push(Part { shader_index: part.i16(gp::SHADER_INDEX)?, vertices, indices });
        }
        Ok(parts)
    }
}

fn nonzero(v: f32) -> f32 {
    if v == 0.0 { 1.0 } else { v }
}

/// `model_vertex_compressed` (rasterizer_model_types.h).
fn decode_vertex(b: &[u8], uv_scale: [f32; 2]) -> Vertex {
    let f = |o: usize| f32::from_le_bytes(b[o..o + 4].try_into().unwrap());
    let u = |o: usize| u32::from_le_bytes(b[o..o + 4].try_into().unwrap());
    let s = |o: usize| i16::from_le_bytes([b[o], b[o + 1]]);
    // node indices are stored ×3 (rasterizer_geometry_uncompress_vertices)
    let weight = s(30) as u8 as f32 / 255.0;
    Vertex {
        position: [f(0), f(4), f(8)],
        normal: math::unpack_vector_11_11_10(u(12)),
        binormal: math::unpack_vector_11_11_10(u(16)),
        tangent: math::unpack_vector_11_11_10(u(20)),
        texcoord: [math::unpack_texcoord_i16(s(24)) * uv_scale[0], math::unpack_texcoord_i16(s(26)) * uv_scale[1]],
        nodes: [b[28] / 3, b[29] / 3],
        node_weights: [weight, 1.0 - weight],
    }
}

/// A triangle strip as a triangle list: every other triangle is flipped to
/// keep one winding, and degenerate (stitching) triangles are dropped.
fn unroll_strip(strip: &[u16], vertex_count: usize) -> Option<Vec<u32>> {
    let mut out = Vec::with_capacity(strip.len().saturating_sub(2) * 3);
    for i in 0..strip.len().saturating_sub(2) {
        let (a, b, c) = (strip[i], strip[i + 1], strip[i + 2]);
        if a == b || b == c || a == c {
            continue;
        }
        if [a, b, c].iter().any(|&v| v as usize >= vertex_count) {
            return None;
        }
        if i % 2 == 0 {
            out.extend([a as u32, b as u32, c as u32]);
        } else {
            out.extend([a as u32, c as u32, b as u32]);
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::unroll_strip;

    #[test]
    fn strip_alternates_winding_and_skips_degenerates() {
        // 0 1 2 3, then a stitch (3 3 4 4) into 4 5 6
        let strip = [0, 1, 2, 3, 3, 4, 4, 5, 6];
        let tris = unroll_strip(&strip, 7).unwrap();
        assert_eq!(tris, vec![0, 1, 2, 1, 3, 2, 4, 5, 6]);
    }
}
