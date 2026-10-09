//! A Halo level as Bevy entities: its structure BSPs and the objects its
//! scenario places.

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
    model::{self, Model},
    objects::{self, Placement},
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

/// An object placed from the scenario.
#[derive(Component)]
pub struct HaloObject;

/// The sky model; it keeps its centre on the camera (see [`follow_camera`]).
#[derive(Component)]
pub struct SkyDome;

/// Metres per sky model unit. A sky is drawn around the camera at no
/// particular size; this puts it far beyond any level (skies are ~10⁵ units).
const SKY_SCALE: f32 = 1.0;

/// Light fixtures become small emissive spheres (Solari lights only with
/// emissive geometry, directional lights and the environment).
const LIGHT_PROXY_RADIUS: f32 = 0.12;
/// How far in front of its fixture (metres) a light sits, clear of the wall.
const LIGHT_PROXY_OFFSET: f32 = 0.3;
/// Radiance per unit of fixture intensity, relative to shader radiosity
/// power (a fixture's sphere is far smaller than a glowing panel).
const LIGHT_PROXY_RADIANCE: f32 = 4000.0;

/// What a loaded level contains, for the overlay.
#[derive(Resource, Default)]
pub struct LevelStats {
    pub map_name: String,
    pub bsps: usize,
    pub meshes: usize,
    pub triangles: usize,
    pub emissive_meshes: usize,
    pub objects: usize,
    pub object_triangles: usize,
    pub object_failures: usize,
    /// Light fixtures turned into emissive proxies.
    pub lights: usize,
    /// Transparent-shader geometry not drawn yet (glass, water, ...).
    pub skipped_transparent: usize,
    pub texture_failures: usize,
}

/// How a shader's surfaces are drawn.
#[derive(Clone)]
enum Surface {
    Opaque(Handle<StandardMaterial>),
    Emissive(Handle<StandardMaterial>, Vec3),
    /// Not drawn yet (transparent shaders without light emission).
    Skip,
}

/// A shader's material and the texture coordinate scale it applies.
#[derive(Clone)]
struct ShaderSurface {
    surface: Surface,
    uv_scale: [f32; 2],
}

/// A model's meshes for one permutation choice, made once and instanced.
type ModelMeshes = Vec<(Handle<Mesh>, Surface, usize)>;

pub struct LevelBuilder<'a> {
    pub cache: &'a CacheFile,
    pub textures: TextureCache,
    surfaces: HashMap<Option<u32>, ShaderSurface>,
    models: HashMap<(u32, [u8; 8]), Option<ModelMeshes>>,
    loaded_bsps: Vec<usize>,
    light_proxy: Option<Handle<Mesh>>,
    /// Whether light fixtures get emissive proxies.
    pub fixture_lights: bool,
    /// Rasterize the light proxies too (to see where they are).
    pub show_light_proxies: bool,
    pub stats: LevelStats,
}

/// One vertex in Halo's convention, as BSPs and models both provide.
struct SourceVertex {
    position: [f32; 3],
    normal: [f32; 3],
    binormal: [f32; 3],
    tangent: [f32; 3],
    texcoord: [f32; 2],
}

impl<'a> LevelBuilder<'a> {
    pub fn new(cache: &'a CacheFile, bc_supported: bool) -> Self {
        LevelBuilder {
            cache,
            textures: TextureCache::new(bc_supported),
            surfaces: HashMap::default(),
            models: HashMap::default(),
            loaded_bsps: Vec::new(),
            light_proxy: None,
            fixture_lights: true,
            show_light_proxies: false,
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
                let shader = self.surface(material.shader, materials, images);
                if matches!(shader.surface, Surface::Skip) {
                    self.stats.skipped_transparent += 1;
                    continue;
                }
                let vertices = material.vertices.iter().map(|v| SourceVertex {
                    position: v.position,
                    normal: v.normal,
                    binormal: v.binormal,
                    tangent: v.tangent,
                    texcoord: v.texcoord,
                });
                let mesh = meshes.add(build_mesh(vertices, &material.indices, shader.uv_scale));
                let name = format!("{} material {material_index}", reference.name);
                self.spawn_mesh(commands, level, name, mesh, &shader.surface, raster);
                self.stats.triangles += material.indices.len() / 3;
            }
            self.loaded_bsps.push(reference.index);
            self.stats.bsps += 1;
        }
        self.stats.texture_failures = self.textures.failures;
        Ok(())
    }

    /// Spawns the objects the game would create in the loaded BSPs.
    pub fn spawn_objects(
        &mut self,
        raster: bool,
        commands: &mut Commands,
        meshes: &mut Assets<Mesh>,
        materials: &mut Assets<StandardMaterial>,
        images: &mut Assets<Image>,
    ) -> halo_map::Result<()> {
        let placements = objects::placements(self.cache)?;
        let objects_root = commands.spawn((Name::new("objects"), Transform::default(), Visibility::default())).id();
        for placement in &placements {
            // (multiplayer vehicles have no BSP flags: the game type spawns them)
            let in_loaded_bsp =
                placement.on_bsp_flags == 0 || self.loaded_bsps.iter().any(|&b| placement.in_bsp(b));
            if placement.not_automatic || !in_loaded_bsp {
                continue;
            }
            // (light fixtures often have no model, only their light)
            let model = objects::object_model(self.cache, placement.definition)?.and_then(|m| m.datum);
            let parts = match model {
                Some(model) => self.model_meshes(model, placement.region_permutations, meshes, materials, images),
                None => None,
            };
            if parts.is_none() && placement.light.is_none() {
                continue;
            }
            let name = self.cache.tag(placement.definition.datum.unwrap())?.name.clone();
            let object = commands
                .spawn((
                    Name::new(name.clone()),
                    HaloObject,
                    placement_transform(placement),
                    Visibility::default(),
                    ChildOf(objects_root),
                ))
                .id();
            for (mesh, surface, triangles) in parts.into_iter().flatten() {
                self.spawn_mesh(commands, object, name.clone(), mesh, &surface, raster);
                self.stats.object_triangles += triangles;
            }
            if let Some((color, intensity)) = placement.light
                && intensity > 0.0
                && self.fixture_lights
            {
                self.spawn_light_proxy(commands, object, Vec3::from(color) * intensity, meshes, materials);
            }
            self.stats.objects += 1;
        }
        self.stats.texture_failures = self.textures.failures;
        Ok(())
    }

    /// Spawns a sky model as raster-only, unlit, alpha-blended layers in the
    /// model's part order. Solari never sees it: the sky's light comes from
    /// the environment map and the sun.
    pub fn spawn_sky(
        &mut self,
        model_datum: u32,
        commands: &mut Commands,
        meshes: &mut Assets<Mesh>,
        materials: &mut Assets<StandardMaterial>,
        images: &mut Assets<Image>,
    ) -> halo_map::Result<()> {
        use bevy::light::NotShadowCaster;
        let model = Model::load(self.cache, model_datum)?;
        let dome = commands
            .spawn((Name::new(model.name.clone()), SkyDome, Transform::default(), Visibility::default()))
            .id();
        let mut layer = 0;
        for geometry in model.geometries_for(&[], model::HIGHEST_DETAIL) {
            for part in model.geometry(geometry)? {
                let Some(shader_ref) = model.shaders.get(part.shader_index.max(0) as usize).copied() else { continue };
                let Some(info) = shader::shader_info(self.cache, shader_ref)? else { continue };
                let (texture, uv_scale, uv_offset) = match sky_layer_map(&info) {
                    Some(map) => (
                        map.bitmap.datum.and_then(|d| self.textures.get(self.cache, d, 0, true, images)),
                        map.uv_scale,
                        map.uv_offset,
                    ),
                    None => (None, info.uv_scale, [0.0, 0.0]),
                };
                let additive = info
                    .transparent
                    .as_ref()
                    .is_some_and(|t| matches!(t.blend, shader::BlendFunction::Add | shader::BlendFunction::AlphaMultiplyAdd));
                let material = materials.add(StandardMaterial {
                    base_color_texture: texture,
                    unlit: true,
                    alpha_mode: if additive { AlphaMode::Add } else { AlphaMode::Blend },
                    cull_mode: None,
                    fog_enabled: false,
                    // keeps Halo's layer order among the transparent sky parts
                    depth_bias: layer as f32 * 100.0,
                    ..default()
                });
                let vertices = part.vertices.iter().map(|v| SourceVertex {
                    position: v.position.map(|c| c * SKY_SCALE / METERS_PER_WORLD_UNIT),
                    normal: v.normal,
                    binormal: v.binormal,
                    tangent: v.tangent,
                    texcoord: [v.texcoord[0] + uv_offset[0] / uv_scale[0].max(1e-6), v.texcoord[1] + uv_offset[1] / uv_scale[1].max(1e-6)],
                });
                let mesh = meshes.add(build_mesh(vertices, &part.indices, uv_scale));
                commands.spawn((
                    Name::new(format!("{} layer {layer}", info.name)),
                    Mesh3d(mesh),
                    MeshMaterial3d(material),
                    NotShadowCaster,
                    Transform::default(),
                    ChildOf(dome),
                ));
                layer += 1;
            }
        }
        Ok(())
    }

    /// An invisible emissive sphere in front of a light fixture: it lights
    /// the ray-traced scene (and shows in reflections) but is never rasterized.
    fn spawn_light_proxy(
        &mut self,
        commands: &mut Commands,
        object: Entity,
        light: Vec3,
        meshes: &mut Assets<Mesh>,
        materials: &mut Assets<StandardMaterial>,
    ) {
        let mesh = self
            .light_proxy
            .get_or_insert_with(|| {
                meshes.add(
                    Sphere::new(LIGHT_PROXY_RADIUS)
                        .mesh()
                        .build()
                        .with_generated_tangents()
                        .expect("sphere has normals and UVs"),
                )
            })
            .clone();
        let material = materials.add(StandardMaterial { base_color: Color::BLACK, ..default() });
        let mut proxy = commands.spawn((
            Name::new("light fixture light"),
            RaytracingMesh3d(mesh.clone()),
            MeshMaterial3d(material),
            EmissiveSurface { radiance: light * LIGHT_PROXY_RADIANCE },
            // object space is Bevy-converted Halo space: Halo forward (+x) is +x
            Transform::from_xyz(LIGHT_PROXY_OFFSET, 0.0, 0.0),
            ChildOf(object),
        ));
        if self.show_light_proxies {
            proxy.insert(Mesh3d(mesh));
        }
        self.stats.emissive_meshes += 1;
        self.stats.lights += 1;
    }

    fn spawn_mesh(
        &mut self,
        commands: &mut Commands,
        parent: Entity,
        name: String,
        mesh: Handle<Mesh>,
        surface: &Surface,
        raster: bool,
    ) {
        let (material, radiance) = match surface {
            Surface::Opaque(h) => (h.clone(), None),
            Surface::Emissive(h, radiance) => (h.clone(), Some(*radiance)),
            Surface::Skip => return,
        };
        let mut entity = commands.spawn((
            Name::new(name),
            RaytracingMesh3d(mesh.clone()),
            MeshMaterial3d(material),
            Transform::default(),
            ChildOf(parent),
        ));
        if raster {
            entity.insert(Mesh3d(mesh));
        }
        if let Some(radiance) = radiance {
            entity.insert(EmissiveSurface { radiance });
            self.stats.emissive_meshes += 1;
        }
        self.stats.meshes += 1;
    }

    /// A model's meshes at the highest level of detail, made once per
    /// (model, permutations).
    fn model_meshes(
        &mut self,
        datum: u32,
        permutations: [u8; 8],
        meshes: &mut Assets<Mesh>,
        materials: &mut Assets<StandardMaterial>,
        images: &mut Assets<Image>,
    ) -> Option<ModelMeshes> {
        if let Some(cached) = self.models.get(&(datum, permutations)) {
            return cached.clone();
        }
        let built = match self.build_model(datum, &permutations, meshes, materials, images) {
            Ok(parts) => Some(parts),
            Err(e) => {
                let name = self.cache.tag(datum).map(|t| t.name.clone()).unwrap_or_default();
                warn!("model {name}: {e}");
                self.stats.object_failures += 1;
                None
            }
        };
        self.models.insert((datum, permutations), built.clone());
        built
    }

    fn build_model(
        &mut self,
        datum: u32,
        permutations: &[u8],
        meshes: &mut Assets<Mesh>,
        materials: &mut Assets<StandardMaterial>,
        images: &mut Assets<Image>,
    ) -> halo_map::Result<ModelMeshes> {
        let model = Model::load(self.cache, datum)?;
        let mut out = Vec::new();
        for geometry in model.geometries_for(permutations, model::HIGHEST_DETAIL) {
            for part in model.geometry(geometry)? {
                if part.indices.is_empty() {
                    continue;
                }
                let shader_ref = model
                    .shaders
                    .get(part.shader_index.max(0) as usize)
                    .copied()
                    .unwrap_or(TagRef { group: 0, datum: None });
                let shader = self.surface(shader_ref, materials, images);
                if matches!(shader.surface, Surface::Skip) {
                    self.stats.skipped_transparent += 1;
                    continue;
                }
                let vertices = part.vertices.iter().map(|v| SourceVertex {
                    position: v.position,
                    normal: v.normal,
                    binormal: v.binormal,
                    tangent: v.tangent,
                    texcoord: v.texcoord,
                });
                let mesh = meshes.add(build_mesh(vertices, &part.indices, shader.uv_scale));
                out.push((mesh, shader.surface, part.indices.len() / 3));
            }
        }
        Ok(out)
    }

    /// The material for a shader, made once per shader.
    fn surface(
        &mut self,
        shader: TagRef,
        materials: &mut Assets<StandardMaterial>,
        images: &mut Assets<Image>,
    ) -> ShaderSurface {
        if let Some(surface) = self.surfaces.get(&shader.datum) {
            return surface.clone();
        }
        let info = match shader::shader_info(self.cache, shader) {
            Ok(info) => info,
            Err(e) => {
                warn!("shader {:?}: {e}", shader.datum);
                None
            }
        };
        let surface = ShaderSurface {
            surface: self.make_surface(info.as_ref(), materials, images),
            uv_scale: info.as_ref().map_or([1.0, 1.0], |i| i.uv_scale),
        };
        self.surfaces.insert(shader.datum, surface.clone());
        surface
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

/// The map that carries a sky layer's look: transparent shaders composite
/// several, so until their combiners are reproduced (ROADMAP phase 3) take the
/// least-tiled 2D map, the last one on ties (later stages draw over earlier).
fn sky_layer_map(info: &ShaderInfo) -> Option<shader::TransparentMap> {
    let transparent = info.transparent.as_ref()?;
    // a first map of a cube type is a reflection, not the layer's colour
    let skip = usize::from(transparent.first_map_type != 0);
    transparent
        .maps
        .iter()
        .skip(skip)
        .rev()
        .min_by(|a, b| (a.uv_scale[0] * a.uv_scale[1]).total_cmp(&(b.uv_scale[0] * b.uv_scale[1])))
        .copied()
}

/// Keeps the sky centred on the camera.
pub fn follow_camera(
    camera: Query<&Transform, (With<Camera3d>, Without<SkyDome>)>,
    mut domes: Query<&mut Transform, With<SkyDome>>,
) {
    let Ok(camera) = camera.single() else { return };
    for mut dome in &mut domes {
        dome.translation = camera.translation;
    }
}

/// A placement's position and orientation in Bevy's convention. Halo's
/// object axes (forward, left, up) form the rotation's columns; the change to
/// Bevy coordinates C makes it C·R·Cᵀ.
fn placement_transform(placement: &Placement) -> Transform {
    let [forward, left, up] = placement.axes();
    let halo_rotation = Mat3::from_cols(Vec3::from(forward), Vec3::from(left), Vec3::from(up));
    let to_bevy = Mat3::from_cols(Vec3::X, Vec3::NEG_Z, Vec3::Y);
    let rotation = Quat::from_mat3(&(to_bevy * halo_rotation * to_bevy.transpose()));
    Transform::from_translation(Vec3::from(placement.bevy_position())).with_rotation(rotation.normalize())
}

/// Halo triangles as a Solari-compatible mesh: positions, normals, UV0 and
/// tangents, u32 indices, no UV1.
fn build_mesh(vertices: impl Iterator<Item = SourceVertex>, indices: &[u32], uv_scale: [f32; 2]) -> Mesh {
    let (mut positions, mut normals, mut uvs, mut tangents) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for v in vertices {
        positions.push(halo_to_bevy(v.position, METERS_PER_WORLD_UNIT));
        normals.push(halo_to_bevy(v.normal, 1.0));
        uvs.push([v.texcoord[0] * uv_scale[0], v.texcoord[1] * uv_scale[1]]);
        let (n, t, b) = (Vec3::from(v.normal), Vec3::from(v.tangent), Vec3::from(v.binormal));
        let handedness = if n.cross(t).dot(b) < 0.0 { -1.0 } else { 1.0 };
        let [x, y, z] = halo_to_bevy(v.tangent, 1.0);
        tangents.push([x, y, z, handedness]);
    }
    // Halo's triangles are clockwise seen from the front (Direct3D);
    // Bevy's front faces are counter-clockwise.
    let indices: Vec<u32> = indices.chunks_exact(3).flat_map(|t| [t[0], t[2], t[1]]).collect();

    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD)
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
        .with_inserted_attribute(Mesh::ATTRIBUTE_TANGENT, tangents)
        .with_inserted_indices(Indices::U32(indices))
}

#[cfg(test)]
mod tests {
    use super::*;
    use halo_map::objects::ObjectKind;

    fn placement(rotation: [f32; 3]) -> Placement {
        Placement {
            kind: ObjectKind::Scenery,
            definition: TagRef { group: 0, datum: None },
            position: [0.0; 3],
            rotation,
            on_bsp_flags: 1,
            not_automatic: false,
            region_permutations: [0; 8],
            light: None,
        }
    }

    /// An object's local forward (Halo +x) must end up along its yawed
    /// Halo forward, converted to Bevy.
    #[test]
    fn yaw_turns_forward_counter_clockwise() {
        let t = placement_transform(&placement([std::f32::consts::FRAC_PI_2, 0.0, 0.0]));
        // Halo +x in Bevy is +x; after a quarter turn it should face Halo +y,
        // which is Bevy -z.
        let forward = t.rotation * Vec3::X;
        assert!(forward.abs_diff_eq(Vec3::NEG_Z, 1e-5), "{forward}");
        // up stays up
        assert!((t.rotation * Vec3::Y).abs_diff_eq(Vec3::Y, 1e-5));
    }
}
