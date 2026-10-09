//! A Halo level's structure BSPs as Bevy meshes and materials.

use crate::textures::TextureCache;
use bevy::{
    asset::RenderAssetUsages,
    mesh::{Indices, PrimitiveTopology},
    platform::collections::HashMap,
    prelude::*,
    solari::prelude::RaytracingMesh3d,
};
use halo_map::{
    bsp,
    math::{halo_to_bevy, METERS_PER_WORLD_UNIT},
    shader::{self, ShaderInfo, ShaderType},
    CacheFile, TagRef,
};

/// A level material that emits light; its emissive strength follows
/// [`crate::LightingSettings::emissive_scale`].
#[derive(Component, Clone)]
pub struct EmissiveSurface {
    /// Radiosity colour × power from the Halo shader.
    pub radiance: Vec3,
}

/// What a loaded level contains, for the overlay.
#[derive(Resource, Default)]
pub struct LevelStats {
    pub map_name: String,
    pub bsps: usize,
    pub meshes: usize,
    pub triangles: usize,
    pub emissive_meshes: usize,
    /// Transparent-shader geometry not drawn yet (glass, water, ...).
    pub skipped_transparent: usize,
    pub texture_failures: usize,
}

/// How a shader's surfaces are drawn.
enum Surface {
    Opaque(Handle<StandardMaterial>),
    Emissive(Handle<StandardMaterial>, Vec3),
    /// Not drawn yet (transparent shaders without light emission).
    Skip,
}

pub struct LevelBuilder<'a> {
    pub cache: &'a CacheFile,
    pub textures: TextureCache,
    surfaces: HashMap<Option<u32>, Surface>,
    pub stats: LevelStats,
}

impl<'a> LevelBuilder<'a> {
    pub fn new(cache: &'a CacheFile, bc_supported: bool) -> Self {
        LevelBuilder {
            cache,
            textures: TextureCache::new(bc_supported),
            surfaces: HashMap::default(),
            stats: LevelStats { map_name: cache.header.name.clone(), ..default() },
        }
    }

    /// Spawns the meshes of the given BSPs (all when `only` is `None`).
    pub fn spawn_bsps(
        &mut self,
        only: Option<usize>,
        raster: bool,
        commands: &mut Commands,
        meshes: &mut Assets<Mesh>,
        materials: &mut Assets<StandardMaterial>,
        images: &mut Assets<Image>,
    ) -> halo_map::Result<()> {
        for reference in bsp::references(self.cache)? {
            if only.is_some_and(|i| i != reference.index) {
                continue;
            }
            let loaded = bsp::Bsp::load(self.cache, &reference)?;
            let level = commands
                .spawn((Name::new(reference.name.clone()), Transform::default(), Visibility::default()))
                .id();
            for (material_index, material) in loaded.materials()?.into_iter().enumerate() {
                if material.indices.is_empty() {
                    continue;
                }
                let surface = self.surface(material.shader, materials, images);
                let (handle, radiance) = match surface {
                    Surface::Opaque(h) => (h.clone(), None),
                    Surface::Emissive(h, radiance) => (h.clone(), Some(*radiance)),
                    Surface::Skip => {
                        self.stats.skipped_transparent += 1;
                        continue;
                    }
                };
                let mesh = meshes.add(build_mesh(&material));
                let mut entity = commands.spawn((
                    Name::new(format!("{} material {material_index}", reference.name)),
                    RaytracingMesh3d(mesh.clone()),
                    MeshMaterial3d(handle),
                    Transform::default(),
                    ChildOf(level),
                ));
                if raster {
                    entity.insert(Mesh3d(mesh));
                }
                if let Some(radiance) = radiance {
                    entity.insert(EmissiveSurface { radiance });
                    self.stats.emissive_meshes += 1;
                }
                self.stats.meshes += 1;
                self.stats.triangles += material.indices.len() / 3;
            }
            self.stats.bsps += 1;
        }
        self.stats.texture_failures = self.textures.failures;
        Ok(())
    }

    /// The material for a shader, made once per shader.
    fn surface(
        &mut self,
        shader: TagRef,
        materials: &mut Assets<StandardMaterial>,
        images: &mut Assets<Image>,
    ) -> &Surface {
        if !self.surfaces.contains_key(&shader.datum) {
            let info = match shader::shader_info(self.cache, shader) {
                Ok(info) => info,
                Err(e) => {
                    warn!("shader {:?}: {e}", shader.datum);
                    None
                }
            };
            let surface = self.make_surface(info.as_ref(), materials, images);
            self.surfaces.insert(shader.datum, surface);
        }
        &self.surfaces[&shader.datum]
    }

    fn make_surface(
        &mut self,
        info: Option<&ShaderInfo>,
        materials: &mut Assets<StandardMaterial>,
        images: &mut Assets<Image>,
    ) -> Surface {
        let Some(info) = info else {
            return Surface::Opaque(materials.add(StandardMaterial {
                base_color: Color::srgb(0.5, 0.5, 0.5),
                perceptual_roughness: 0.9,
                ..default()
            }));
        };
        let base_color_texture = info
            .base_map
            .and_then(|r| r.datum)
            .and_then(|datum| self.textures.get(self.cache, datum, 0, true, images));
        let radiance = Vec3::from(info.emitted_light) * info.emitted_power;
        let emits = info.emitted_power > 0.0 && radiance != Vec3::ZERO;

        if info.shader_type.is_transparent() && !emits {
            return Surface::Skip;
        }
        let material = StandardMaterial {
            base_color: if base_color_texture.is_some() || !emits {
                Color::WHITE
            } else {
                Color::srgb(0.2, 0.2, 0.2)
            },
            base_color_texture: base_color_texture.clone(),
            // Phase 3 derives these from Halo's specular and reflection
            // settings; until then, surfaces are rough dielectrics.
            perceptual_roughness: 0.85,
            metallic: 0.0,
            reflectance: 0.4,
            emissive_texture: if emits && info.shader_type == ShaderType::Environment {
                base_color_texture
            } else {
                None
            },
            ..default()
        };
        let handle = materials.add(material);
        if emits { Surface::Emissive(handle, radiance) } else { Surface::Opaque(handle) }
    }
}

/// A BSP material's triangles as a Solari-compatible mesh: positions,
/// normals, UV0 and tangents, u32 indices, no UV1.
fn build_mesh(material: &bsp::Material) -> Mesh {
    let positions: Vec<[f32; 3]> =
        material.vertices.iter().map(|v| halo_to_bevy(v.position, METERS_PER_WORLD_UNIT)).collect();
    let normals: Vec<[f32; 3]> = material.vertices.iter().map(|v| halo_to_bevy(v.normal, 1.0)).collect();
    let uvs: Vec<[f32; 2]> = material.vertices.iter().map(|v| v.texcoord).collect();
    let tangents: Vec<[f32; 4]> = material
        .vertices
        .iter()
        .map(|v| {
            let (n, t, b) = (Vec3::from(v.normal), Vec3::from(v.tangent), Vec3::from(v.binormal));
            let handedness = if n.cross(t).dot(b) < 0.0 { -1.0 } else { 1.0 };
            let [x, y, z] = halo_to_bevy(v.tangent, 1.0);
            [x, y, z, handedness]
        })
        .collect();
    // Halo's triangles are clockwise seen from the front (Direct3D);
    // Bevy's front faces are counter-clockwise.
    let indices: Vec<u32> = material.indices.chunks_exact(3).flat_map(|t| [t[0], t[2], t[1]]).collect();

    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD)
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
        .with_inserted_attribute(Mesh::ATTRIBUTE_TANGENT, tangents)
        .with_inserted_indices(Indices::U32(indices))
}
