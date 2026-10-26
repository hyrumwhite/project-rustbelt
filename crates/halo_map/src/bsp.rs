//! Structure BSPs: the level geometry.
//!
//! A scenario lists its BSPs as `scenario_structure_bsp_reference`s. Each is a
//! separate image in the file (`file_offset`, `file_size`) that the game
//! loads at `base_address` (`scenario_structure_bsp_load`, cache_files.c).
//! The image starts with a `cache_file_structure_bsp_header` whose first
//! field points at the `structure_bsp`.
//!
//! Render geometry is grouped as lightmaps → materials. A material is one
//! shader's triangles: `surface_count` surfaces (triangles of 16-bit indices)
//! from `first_surface_index` in the BSP's surface block, indexing the
//! material's own vertices in `compressed_vertex_data` (rendered vertices,
//! then lightmap vertices).

use crate::{layout, math, CacheFile, Error, Ptr, Result, Space, TagRef};

/// `_rasterizer_vertex_type_*` (rasterizer_geometry.h).
const VERTEX_TYPE_ENVIRONMENT_UNCOMPRESSED: i16 = 0;
const VERTEX_TYPE_ENVIRONMENT_COMPRESSED: i16 = 1;
const VERTEX_TYPE_LIGHTMAP_UNCOMPRESSED: i16 = 2;
const VERTEX_TYPE_LIGHTMAP_COMPRESSED: i16 = 3;
const COMPRESSED_VERTEX_SIZE: usize = 32;
const UNCOMPRESSED_VERTEX_SIZE: usize = 56;
const COMPRESSED_LIGHTMAP_VERTEX_SIZE: usize = 8;
const UNCOMPRESSED_LIGHTMAP_VERTEX_SIZE: usize = 20;

#[derive(Debug, Clone)]
pub struct BspReference {
    pub index: usize,
    pub name: String,
    pub space: Space,
    pub tag: TagRef,
}

/// The scenario's structure BSPs.
pub fn references(cache: &CacheFile) -> Result<Vec<BspReference>> {
    use layout::scenario_structure_bsp_reference as r;
    let scenario = cache.scenario()?;
    let block = scenario.block(layout::scenario::STRUCTURE_BSP_REFERENCES, r::SIZEOF)?;
    block
        .iter()
        .enumerate()
        .map(|(index, element)| {
            let tag = element.reference(r::STRUCTURE_BSP)?;
            let name = tag.datum.and_then(|d| cache.tag(d).ok()).map(|t| t.name.clone()).unwrap_or_default();
            Ok(BspReference {
                index,
                name,
                space: Space {
                    base: element.u32(r::BASE_ADDRESS)?,
                    file_offset: element.i32(r::FILE_OFFSET)?.max(0) as usize,
                    size: element.i32(r::FILE_SIZE)?.max(0) as usize,
                },
                tag,
            })
        })
        .collect()
}

/// A loaded structure BSP.
pub struct Bsp<'a> {
    pub root: Ptr<'a>,
}

impl<'a> Bsp<'a> {
    pub fn load(cache: &'a CacheFile, reference: &BspReference) -> Result<Self> {
        let space = reference.space;
        if space.file_offset.checked_add(space.size).is_none_or(|end| end > cache.bytes().len()) {
            return Err(Error::Format(format!("bsp {} leaves the file", reference.name)));
        }
        let header = cache.root(space, space.base)?;
        let root = header.at(header.u32(0)?)?;
        // the whole structure must be inside the image
        root.bytes(0, layout::structure_bsp::SIZEOF)?;
        Ok(Bsp { root })
    }

    /// `world_bounds`: x, y and z bounds in world units.
    pub fn world_bounds(&self) -> Result<[[f32; 2]; 3]> {
        let b = layout::structure_bsp::WORLD_BOUNDS;
        Ok([self.root.vec2(b)?, self.root.vec2(b + 8)?, self.root.vec2(b + 16)?])
    }

    /// The lightmap bitmap tag (one bitmap per lightmap, by `bitmap_index`).
    pub fn lightmap_bitmaps(&self) -> Result<TagRef> {
        self.root.reference(layout::structure_bsp::LIGHTMAP_GROUP)
    }

    /// Every material's render geometry, in Halo's coordinates.
    pub fn materials(&self) -> Result<Vec<Material>> {
        use layout::{structure_lightmap as lm, structure_material as m};
        let surfaces = self.root.block(layout::structure_bsp::SURFACES, layout::structure_surface::SIZEOF)?;
        let lightmaps = self.root.block(layout::structure_bsp::LIGHTMAPS, lm::SIZEOF)?;

        let mut out = Vec::new();
        for (lightmap_index, lightmap) in lightmaps.iter().enumerate() {
            let bitmap_index = lightmap.i16(lm::BITMAP_INDEX)?;
            let materials = lightmap.block(lm::MATERIALS, m::SIZEOF)?;
            for material in materials.iter() {
                out.push(read_material(material, &surfaces, lightmap_index, bitmap_index)?);
            }
        }
        Ok(out)
    }
}

/// An environment vertex (`environment_vertex_uncompressed`).
#[derive(Debug, Clone, Copy, Default)]
pub struct Vertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    pub binormal: [f32; 3],
    pub tangent: [f32; 3],
    pub texcoord: [f32; 2],
}

#[derive(Debug, Clone)]
pub struct Material {
    pub shader: TagRef,
    pub lightmap_index: usize,
    /// The lightmap's bitmap in the BSP's lightmap bitmap tag; -1 for none.
    pub lightmap_bitmap_index: i16,
    pub vertices: Vec<Vertex>,
    /// One per vertex when the material is lightmapped.
    pub lightmap_texcoords: Vec<[f32; 2]>,
    /// Triangles, three indices each, into `vertices`.
    pub indices: Vec<u32>,
    /// The breakable surface it shows (the BSP's, drawn while it stands),
    /// or -1.
    pub breakable_surface_index: i16,
}

fn read_material(
    material: Ptr<'_>,
    surfaces: &crate::Block<'_>,
    lightmap_index: usize,
    lightmap_bitmap_index: i16,
) -> Result<Material> {
    use layout::{structure_material as m, vertex_buffer as vb};
    let shader = material.reference(m::SHADER)?;
    let vertex_type = material.i16(m::VERTICES + vb::TYPE)?;
    let vertex_count = material.i32(m::VERTICES + vb::COUNT)?.max(0) as usize;
    let lightmap_type = material.i16(m::LIGHTMAP_VERTICES + vb::TYPE)?;
    let lightmap_count = material.i32(m::LIGHTMAP_VERTICES + vb::COUNT)?.max(0) as usize;

    let (vertex_size, lightmap_size) = match (vertex_type, lightmap_type) {
        (VERTEX_TYPE_ENVIRONMENT_COMPRESSED, VERTEX_TYPE_LIGHTMAP_COMPRESSED) => {
            (COMPRESSED_VERTEX_SIZE, COMPRESSED_LIGHTMAP_VERTEX_SIZE)
        }
        (VERTEX_TYPE_ENVIRONMENT_UNCOMPRESSED, VERTEX_TYPE_LIGHTMAP_UNCOMPRESSED) => {
            (UNCOMPRESSED_VERTEX_SIZE, UNCOMPRESSED_LIGHTMAP_VERTEX_SIZE)
        }
        _ if lightmap_count == 0 && vertex_type == VERTEX_TYPE_ENVIRONMENT_COMPRESSED => {
            (COMPRESSED_VERTEX_SIZE, COMPRESSED_LIGHTMAP_VERTEX_SIZE)
        }
        _ if lightmap_count == 0 && vertex_type == VERTEX_TYPE_ENVIRONMENT_UNCOMPRESSED => {
            (UNCOMPRESSED_VERTEX_SIZE, UNCOMPRESSED_LIGHTMAP_VERTEX_SIZE)
        }
        _ => {
            return Err(Error::Unsupported(format!("bsp vertex types {vertex_type}/{lightmap_type}")));
        }
    };
    let compressed = vertex_size == COMPRESSED_VERTEX_SIZE;
    let data = if compressed {
        material.data_bytes(m::COMPRESSED_VERTEX_DATA)?
    } else {
        material.data_bytes(m::UNCOMPRESSED_VERTEX_DATA)?
    };
    let needed = vertex_count * vertex_size + lightmap_count * lightmap_size;
    if data.len() < needed {
        return Err(Error::Format(format!(
            "material vertex data is {:#x} bytes; {vertex_count} + {lightmap_count} vertices need {needed:#x}",
            data.len()
        )));
    }

    let f32_at = |o: usize| f32::from_le_bytes(data[o..o + 4].try_into().unwrap());
    let u32_at = |o: usize| u32::from_le_bytes(data[o..o + 4].try_into().unwrap());
    let i16_at = |o: usize| i16::from_le_bytes(data[o..o + 2].try_into().unwrap());
    let vec3_at = |o: usize| [f32_at(o), f32_at(o + 4), f32_at(o + 8)];

    let vertices = (0..vertex_count)
        .map(|i| {
            let o = i * vertex_size;
            if compressed {
                Vertex {
                    position: vec3_at(o),
                    normal: math::unpack_vector_11_11_10(u32_at(o + 12)),
                    binormal: math::unpack_vector_11_11_10(u32_at(o + 16)),
                    tangent: math::unpack_vector_11_11_10(u32_at(o + 20)),
                    texcoord: [f32_at(o + 24), f32_at(o + 28)],
                }
            } else {
                Vertex {
                    position: vec3_at(o),
                    normal: vec3_at(o + 12),
                    binormal: vec3_at(o + 24),
                    tangent: vec3_at(o + 36),
                    texcoord: [f32_at(o + 48), f32_at(o + 52)],
                }
            }
        })
        .collect();

    let lightmap_base = vertex_count * vertex_size;
    let lightmap_texcoords = (0..lightmap_count)
        .map(|i| {
            let o = lightmap_base + i * lightmap_size;
            if compressed {
                [math::unpack_texcoord_i16(i16_at(o + 4)), math::unpack_texcoord_i16(i16_at(o + 6))]
            } else {
                [f32_at(o + 12), f32_at(o + 16)]
            }
        })
        .collect();

    let first = material.i32(m::FIRST_SURFACE_INDEX)?.max(0) as usize;
    let count = material.i32(m::SURFACE_COUNT)?.max(0) as usize;
    let mut indices = Vec::with_capacity(count * 3);
    for s in first..first + count {
        let surface = surfaces
            .get(s)
            .ok_or_else(|| Error::Format(format!("surface {s} past the bsp's {}", surfaces.count)))?;
        for corner in 0..3 {
            let index = surface.u16(corner * 2)? as u32;
            if index as usize >= vertex_count {
                return Err(Error::Format(format!("surface {s} indexes vertex {index} of {vertex_count}")));
            }
            indices.push(index);
        }
    }

    let breakable_surface_index = material.i16(m::BREAKABLE_SURFACE_INDEX)?;
    Ok(Material { shader, lightmap_index, lightmap_bitmap_index, vertices, lightmap_texcoords, indices, breakable_surface_index })
}
