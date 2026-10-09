//! A Halo level as Bevy entities: its structure BSPs and the objects its
//! scenario places.

use crate::{
    material::{
        HaloCombinerParams, HaloEnvironment, HaloEnvironmentMaterial, HaloEnvironmentParams, HaloTransparentMaterial,
    },
    textures::TextureCache,
};
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
/// Transparent meshes sort back to front by their bounds' centres plus a
/// bias. Sky layers are drawn first (behind every transparent surface of the
/// level), in Halo's part order: each step is larger than the sky is.
const SKY_BIAS: f32 = -1.0e8;
const SKY_LAYER_BIAS: f32 = 1.0e6;

/// Light fixtures become small emissive spheres (Solari lights only with
/// emissive geometry, directional lights and the environment).
const LIGHT_PROXY_RADIUS: f32 = 0.12;
/// How far in front of its fixture (metres) a light sits, clear of the wall.
const LIGHT_PROXY_OFFSET: f32 = 0.3;
/// Radiance per unit of fixture intensity, relative to shader radiosity
/// power (a fixture's sphere is far smaller than a glowing panel).
const LIGHT_PROXY_RADIANCE: f32 = 4000.0;
/// Radiance of fully self-illuminated texels, relative to radiosity power
/// (with the default emissive scale and exposure, about display white).
const SELF_ILLUMINATION_RADIANCE: f32 = 2.5;

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
    /// Transparent-shader geometry not drawn (glass, water, ...: the
    /// shader types the combiner programs do not cover yet).
    pub skipped_transparent: usize,
    /// Transparent geometry drawn with its combiner program.
    pub transparent_meshes: usize,
    pub texture_failures: usize,
}

/// The asset collections a level is built into.
pub struct LevelAssets<'a> {
    pub meshes: &'a mut Assets<Mesh>,
    pub materials: &'a mut Assets<StandardMaterial>,
    pub environment_materials: &'a mut Assets<HaloEnvironmentMaterial>,
    pub transparent_materials: &'a mut Assets<HaloTransparentMaterial>,
    pub images: &'a mut Assets<Image>,
    /// The game's default detail texture (see [`crate::material::NeutralDetail`]).
    pub neutral_detail: Handle<Image>,
    /// A flat normal map (see [`crate::material::flat_normal_image`]).
    pub flat_normal: Handle<Image>,
}

/// How a shader's surfaces are drawn.
#[derive(Clone)]
enum Surface {
    /// Ray traced and rasterized.
    Solid(SolidSurface),
    /// Rasterized only, blended over the scene (Solari traces opaque
    /// surfaces only).
    Transparent(Handle<HaloTransparentMaterial>),
    /// Glass and water: rasterized only, a blended `StandardMaterial`.
    Blended(Handle<StandardMaterial>),
}

#[derive(Clone)]
struct SolidSurface {
    /// What the ray tracer sees, and the camera too without `raster`.
    standard: Handle<StandardMaterial>,
    /// A Halo material for the camera where `StandardMaterial` falls short
    /// (see [`crate::material`]).
    raster: Option<Handle<HaloEnvironmentMaterial>>,
    /// Emitted light (see [`EmissiveSurface`]).
    radiance: Option<Vec3>,
}

/// A shader's material and the texture coordinate scale it applies.
#[derive(Clone)]
struct ShaderSurface {
    /// `None`: not drawn (a transparent shader the combiners cannot draw).
    surface: Option<Surface>,
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
    /// Ignore Halo's reflection settings: every surface a rough dielectric.
    pub matte: bool,
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
            matte: false,
            stats: LevelStats { map_name: cache.header.name.clone(), ..default() },
        }
    }

    /// Spawns the meshes of the given BSPs (all when `only` is `None`).
    pub fn spawn_bsps(
        &mut self,
        only: Option<usize>,
        raster: bool,
        commands: &mut Commands,
        assets: &mut LevelAssets,
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
                let shader = self.surface(material.shader, assets);
                let Some(surface) = shader.surface else {
                    self.stats.skipped_transparent += 1;
                    continue;
                };
                let vertices = material.vertices.iter().map(|v| SourceVertex {
                    position: v.position,
                    normal: v.normal,
                    binormal: v.binormal,
                    tangent: v.tangent,
                    texcoord: v.texcoord,
                });
                let mesh = assets.meshes.add(build_mesh(vertices, &material.indices, shader.uv_scale));
                let name = format!("{} material {material_index}", reference.name);
                self.spawn_mesh(commands, level, name, mesh, &surface, raster);
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
        assets: &mut LevelAssets,
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
                Some(model) => self.model_meshes(model, placement.region_permutations, assets),
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
                self.spawn_light_proxy(commands, object, Vec3::from(color) * intensity, assets);
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
        assets: &mut LevelAssets,
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
                let depth_bias = SKY_BIAS + layer as f32 * SKY_LAYER_BIAS;
                if let Some(material) = self.transparent_material(&info, depth_bias, assets) {
                    let vertices = part.vertices.iter().map(|v| SourceVertex {
                        position: v.position.map(|c| c * SKY_SCALE / METERS_PER_WORLD_UNIT),
                        normal: v.normal,
                        binormal: v.binormal,
                        tangent: v.tangent,
                        texcoord: v.texcoord,
                    });
                    let mesh = assets.meshes.add(build_mesh(vertices, &part.indices, [1.0, 1.0]));
                    commands.spawn((
                        Name::new(format!("{} layer {layer}", info.name)),
                        Mesh3d(mesh),
                        MeshMaterial3d(material),
                        NotShadowCaster,
                        Transform::default(),
                        ChildOf(dome),
                    ));
                    layer += 1;
                    continue;
                }
                let (texture, uv_scale, uv_offset) = match sky_layer_map(&info) {
                    Some(map) => (
                        map.bitmap.datum.and_then(|d| self.textures.get(self.cache, d, 0, true, assets.images)),
                        map.uv_scale,
                        map.uv_offset,
                    ),
                    None => (None, info.uv_scale, [0.0, 0.0]),
                };
                let additive = info
                    .transparent
                    .as_ref()
                    .is_some_and(|t| matches!(t.blend, shader::BlendFunction::Add | shader::BlendFunction::AlphaMultiplyAdd));
                let material = assets.materials.add(StandardMaterial {
                    base_color_texture: texture,
                    unlit: true,
                    alpha_mode: if additive { AlphaMode::Add } else { AlphaMode::Blend },
                    cull_mode: None,
                    fog_enabled: false,
                    depth_bias: SKY_BIAS + layer as f32 * SKY_LAYER_BIAS,
                    ..default()
                });
                let vertices = part.vertices.iter().map(|v| SourceVertex {
                    position: v.position.map(|c| c * SKY_SCALE / METERS_PER_WORLD_UNIT),
                    normal: v.normal,
                    binormal: v.binormal,
                    tangent: v.tangent,
                    texcoord: [v.texcoord[0] + uv_offset[0] / uv_scale[0].max(1e-6), v.texcoord[1] + uv_offset[1] / uv_scale[1].max(1e-6)],
                });
                let mesh = assets.meshes.add(build_mesh(vertices, &part.indices, uv_scale));
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
        assets: &mut LevelAssets,
    ) {
        let mesh = self
            .light_proxy
            .get_or_insert_with(|| {
                assets.meshes.add(
                    Sphere::new(LIGHT_PROXY_RADIUS)
                        .mesh()
                        .build()
                        .with_generated_tangents()
                        .expect("sphere has normals and UVs"),
                )
            })
            .clone();
        let material = assets.materials.add(StandardMaterial { base_color: Color::BLACK, ..default() });
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
        let surface = match surface {
            Surface::Solid(solid) => solid,
            Surface::Transparent(material) => {
                if raster {
                    commands.spawn((
                        Name::new(name),
                        Mesh3d(mesh),
                        MeshMaterial3d(material.clone()),
                        bevy::light::NotShadowCaster,
                        Transform::default(),
                        ChildOf(parent),
                    ));
                    self.stats.meshes += 1;
                    self.stats.transparent_meshes += 1;
                }
                return;
            }
            Surface::Blended(material) => {
                if raster {
                    commands.spawn((
                        Name::new(name),
                        Mesh3d(mesh),
                        MeshMaterial3d(material.clone()),
                        bevy::light::NotShadowCaster,
                        Transform::default(),
                        ChildOf(parent),
                    ));
                    self.stats.meshes += 1;
                    self.stats.transparent_meshes += 1;
                }
                return;
            }
        };
        let mut traced = commands.spawn((
            Name::new(name.clone()),
            RaytracingMesh3d(mesh.clone()),
            MeshMaterial3d(surface.standard.clone()),
            Transform::default(),
            ChildOf(parent),
        ));
        if let Some(radiance) = surface.radiance {
            traced.insert(EmissiveSurface { radiance });
            self.stats.emissive_meshes += 1;
        }
        if raster {
            match &surface.raster {
                // the camera sees the Halo material; rays see the standard one
                Some(material) => {
                    let mut drawn = commands.spawn((
                        Name::new(name),
                        Mesh3d(mesh),
                        MeshMaterial3d(material.clone()),
                        Transform::default(),
                        ChildOf(parent),
                    ));
                    if let Some(radiance) = surface.radiance {
                        drawn.insert(EmissiveSurface { radiance });
                    }
                }
                None => {
                    traced.insert(Mesh3d(mesh));
                }
            }
        }
        self.stats.meshes += 1;
    }

    /// A model's meshes at the highest level of detail, made once per
    /// (model, permutations).
    fn model_meshes(
        &mut self,
        datum: u32,
        permutations: [u8; 8],
        assets: &mut LevelAssets,
    ) -> Option<ModelMeshes> {
        if let Some(cached) = self.models.get(&(datum, permutations)) {
            return cached.clone();
        }
        let built = match self.build_model(datum, &permutations, assets) {
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
        assets: &mut LevelAssets,
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
                let shader = self.surface(shader_ref, assets);
                let Some(surface) = shader.surface else {
                    self.stats.skipped_transparent += 1;
                    continue;
                };
                let vertices = part.vertices.iter().map(|v| SourceVertex {
                    position: v.position,
                    normal: v.normal,
                    binormal: v.binormal,
                    tangent: v.tangent,
                    texcoord: v.texcoord,
                });
                let mesh = assets.meshes.add(build_mesh(vertices, &part.indices, shader.uv_scale));
                out.push((mesh, surface, part.indices.len() / 3));
            }
        }
        Ok(out)
    }

    /// The material for a shader, made once per shader.
    fn surface(
        &mut self,
        shader: TagRef,
        assets: &mut LevelAssets,
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
            surface: self.make_surface(info.as_ref(), assets),
            uv_scale: info.as_ref().map_or([1.0, 1.0], |i| i.uv_scale),
        };
        self.surfaces.insert(shader.datum, surface.clone());
        surface
    }

    fn make_surface(&mut self, info: Option<&ShaderInfo>, assets: &mut LevelAssets) -> Option<Surface> {
        let Some(info) = info else {
            let standard = assets.materials.add(StandardMaterial {
                base_color: Color::srgb(0.5, 0.5, 0.5),
                perceptual_roughness: 0.9,
                ..default()
            });
            return Some(Surface::Solid(SolidSurface { standard, raster: None, radiance: None }));
        };
        let base_color_texture = info
            .base_map
            .and_then(|r| r.datum)
            .and_then(|datum| self.textures.get(self.cache, datum, 0, true, assets.images));
        let radiance = Vec3::from(info.emitted_light) * info.emitted_power;
        let emits = info.emitted_power > 0.0 && radiance != Vec3::ZERO;

        if info.shader_type.is_transparent() && !emits {
            if let Some(material) = self.glass_or_water_material(info, assets) {
                return Some(Surface::Blended(material));
            }
            return self.transparent_material(info, 0.0, assets).map(Surface::Transparent);
        }
        let self_illumination = info.self_illumination.and_then(|si| match si {
            shader::SelfIllumination::Model { multipurpose_map, color_upper, .. } => {
                let bake = crate::textures::Bake::ModelSelfIllumination {
                    base: info.base_map.and_then(|b| b.datum),
                    multipurpose: multipurpose_map.datum?,
                };
                Some((self.textures.baked(self.cache, bake, assets.images)?, Vec3::from(color_upper)))
            }
            shader::SelfIllumination::Environment { map, primary, secondary, plasma } => {
                let bake = crate::textures::Bake::EnvironmentSelfIllumination {
                    base: info.base_map.and_then(|b| b.datum),
                    map: map.datum?,
                    colors: [primary, secondary, plasma].map(|c| c.map(f32::to_bits)),
                };
                Some((self.textures.baked(self.cache, bake, assets.images)?, Vec3::ONE))
            }
        });
        let shine = self.shininess(info, assets);
        let material = StandardMaterial {
            base_color: if base_color_texture.is_some() || !emits {
                Color::WHITE
            } else {
                Color::srgb(0.2, 0.2, 0.2)
            },
            base_color_texture: base_color_texture.clone(),
            perceptual_roughness: shine.roughness,
            metallic: shine.metallic,
            metallic_roughness_texture: shine.texture,
            reflectance: 0.5,
            // A surface that lit the lightmaps (radiosity emission) keeps that
            // as its brightness, the real light output; self-illumination
            // otherwise.
            emissive_texture: match &self_illumination {
                Some((texture, _)) => Some(texture.clone()),
                None if emits && info.shader_type == ShaderType::Environment => base_color_texture.clone(),
                None => None,
            },
            ..default()
        };
        let radiance = match &self_illumination {
            _ if emits => Some(radiance),
            Some((_, color)) => Some(*color * SELF_ILLUMINATION_RADIANCE),
            None => None,
        };
        // the ray-traced surface gets the bump map too, where it tiles as the
        // base map does (StandardMaterial has one set of texture coordinates)
        let mut material = material;
        if let Some(environment) = &info.environment {
            let base_size = info.base_map.and_then(|b| b.datum).and_then(|d| bitmap_size(self.cache, d));
            if let Some((normal_map, scale)) = self.bump_map(environment, base_size, assets.images)
                && scale.abs_diff_eq(Vec2::ONE, 1e-3)
            {
                material.normal_map_texture = Some(normal_map);
            }
        }
        let raster = match (&info.environment, &base_color_texture) {
            (Some(environment), Some(base_map)) => {
                Some(self.environment_material(environment, base_map.clone(), &material, info, assets))
            }
            _ => None,
        };
        Some(Surface::Solid(SolidSurface { standard: assets.materials.add(material), raster, radiance }))
    }

    /// Glass and water as smooth, blended surfaces: Halo draws them in
    /// several passes (tint, reflection, ripples); this keeps their colour,
    /// transparency and a reflection of the sky from the environment map.
    fn glass_or_water_material(&mut self, info: &ShaderInfo, assets: &mut LevelAssets) -> Option<Handle<StandardMaterial>> {
        let texture = |map: Option<TagRef>, this: &mut Self, assets: &mut LevelAssets| {
            map.and_then(|m| m.datum).and_then(|d| this.textures.get(this.cache, d, 0, true, assets.images))
        };
        let material = if let Some(glass) = info.glass {
            let [r, g, b] = glass.tint;
            StandardMaterial {
                base_color: Color::srgba(r, g, b, 0.35),
                base_color_texture: texture(glass.diffuse_map.or(glass.tint_map), self, assets),
                perceptual_roughness: 0.05,
                reflectance: 0.6,
                alpha_mode: AlphaMode::Blend,
                cull_mode: None,
                ..default()
            }
        } else if let Some(water) = info.water {
            let [r, g, b, a] = water.perpendicular_tint;
            StandardMaterial {
                base_color: Color::srgba(r, g, b, a.clamp(0.3, 0.9)),
                base_color_texture: texture(water.base_map, self, assets),
                perceptual_roughness: 0.08,
                reflectance: 0.5,
                alpha_mode: AlphaMode::Blend,
                cull_mode: None,
                ..default()
            }
        } else {
            return None;
        };
        Some(assets.materials.add(material))
    }

    /// A transparent chicago or generic shader's combiner material: its maps
    /// in stage order, their texture transforms, and Bevy's nearest blend
    /// mode to the shader's framebuffer blend function.
    fn transparent_material(
        &mut self,
        info: &ShaderInfo,
        depth_bias: f32,
        assets: &mut LevelAssets,
    ) -> Option<Handle<HaloTransparentMaterial>> {
        use shader::BlendFunction;
        let transparent = info.transparent.as_ref()?;
        let shader_ref = TagRef { group: info.group, datum: Some(info.datum) };
        let program = match halo_map::combiner::program(self.cache, shader_ref) {
            Ok(program) => program?,
            Err(e) => {
                warn!("shader {}: {e}", info.name);
                return None;
            }
        };
        let mut params = HaloCombinerParams::from_program(&program);
        let mut maps: [Handle<Image>; 4] = std::array::from_fn(|_| assets.neutral_detail.clone());
        let mut present = 0;
        for (stage, slot) in transparent.maps.iter().enumerate().take(4) {
            // a cube map first stage (reflections) is not drawn yet
            if stage == 0 && transparent.first_map_type != 0 {
                continue;
            }
            let Some(map) = slot else { continue };
            let Some(image) = map.bitmap.datum.and_then(|d| self.textures.get(self.cache, d, 0, true, assets.images))
            else {
                continue;
            };
            maps[stage] = image;
            present |= 1 << stage;
            params.map_transforms[stage] = Vec4::new(map.uv_scale[0], map.uv_scale[1], map.uv_offset[0], map.uv_offset[1]);
        }
        let (alpha_mode, output) = match transparent.blend {
            BlendFunction::AlphaBlend => (AlphaMode::Blend, 0),
            BlendFunction::Add | BlendFunction::Subtract | BlendFunction::ComponentMax => (AlphaMode::Add, 1),
            BlendFunction::Multiply | BlendFunction::DoubleMultiply | BlendFunction::ComponentMin => {
                (AlphaMode::Multiply, 2)
            }
            BlendFunction::AlphaMultiplyAdd => (AlphaMode::Premultiplied, 3),
        };
        params.modes = UVec4::new(output, present, 0, 0);
        let [map0, map1, map2, map3] = maps;
        Some(assets.transparent_materials.add(HaloTransparentMaterial {
            params,
            map0,
            map1,
            map2,
            map3,
            alpha_mode,
            depth_bias,
        }))
    }

    /// Roughness and metalness from Halo's reflection settings. Halo has no
    /// roughness: a surface reflects its cube map, scaled by a brightness and
    /// a specular mask. Brighter reflections become smoother and more metallic
    /// surfaces; the mask (when the shader has one) becomes a texture.
    fn shininess(&mut self, info: &ShaderInfo, assets: &mut LevelAssets) -> Shininess {
        use crate::textures::{Bake, MATTE_ROUGHNESS};
        use shader::SpecularMask;
        let matte = Shininess { roughness: MATTE_ROUGHNESS, metallic: 0.0, texture: None };
        let Some(reflection) = info.reflection.filter(|_| !self.matte) else { return matte };
        let r = reflection.perpendicular_brightness.max(reflection.parallel_brightness).clamp(0.0, 1.0);
        if r < 0.02 || reflection.cube_map.is_none() {
            return matte;
        }
        let roughness = 0.8 - 0.65 * r.sqrt();
        let metallic = 0.5 * r;
        let mask = match reflection.mask {
            // an alpha-tested base map's alpha is its cut-out, not a mask
            SpecularMask::BaseAlpha if !info.alpha_tested => info.base_map.and_then(|b| b.datum).map(|d| (d, 3)),
            SpecularMask::MultipurposeRed => info.multipurpose_map.and_then(|m| m.datum).map(|d| (d, 0)),
            _ => None,
        };
        let texture = mask.and_then(|(mask, channel)| {
            let bake = Bake::MetallicRoughness {
                mask,
                channel,
                roughness: roughness.to_bits(),
                metallic: metallic.to_bits(),
            };
            self.textures.baked(self.cache, bake, assets.images)
        });
        match texture {
            // the texture holds the values; the factors multiply them
            Some(texture) => Shininess { roughness: 1.0, metallic: 1.0, texture: Some(texture) },
            None => Shininess { roughness, metallic, texture: None },
        }
    }

    /// A `shader_environment`'s camera material: its detail layers over
    /// `standard` (see `halo_environment.wesl`).
    fn environment_material(
        &mut self,
        environment: &shader::EnvironmentDiffuse,
        base_map: Handle<Image>,
        standard: &StandardMaterial,
        info: &ShaderInfo,
        assets: &mut LevelAssets,
    ) -> Handle<HaloEnvironmentMaterial> {
        use shader::{DetailFunction, EnvironmentType};
        let base_size = info.base_map.and_then(|b| b.datum).and_then(|d| bitmap_size(self.cache, d));
        let mut layer = |map: Option<shader::DetailMap>| -> (Handle<Image>, Vec2) {
            let Some(map) = map else { return (assets.neutral_detail.clone(), Vec2::ONE) };
            let Some(datum) = map.bitmap.datum else { return (assets.neutral_detail.clone(), Vec2::ONE) };
            let image = self.textures.get(self.cache, datum, 0, true, assets.images);
            // rescaled detail maps repeat relative to the base map's size
            let rescale = match (environment.rescale_detail_maps, base_size, bitmap_size(self.cache, datum)) {
                (true, Some(base), Some(detail)) => base / detail,
                _ => Vec2::ONE,
            };
            (image.unwrap_or_else(|| assets.neutral_detail.clone()), rescale * map.scale)
        };
        let (primary, primary_scale) = layer(environment.primary_detail);
        let (secondary, secondary_scale) = layer(environment.secondary_detail);
        let (micro, micro_scale) = layer(environment.micro_detail);
        let bump = self.bump_map(environment, base_size, assets.images);
        let mut flags = 0;
        if bump.is_some() {
            flags |= crate::material::ENVIRONMENT_HAS_BUMP_MAP;
        }
        if info.alpha_tested && bump.is_some() {
            flags |= crate::material::ENVIRONMENT_ALPHA_TESTED;
        }
        let (bump_map, bump_scale) = bump.unwrap_or((assets.flat_normal.clone(), Vec2::ONE));
        let function = |f: DetailFunction| match f {
            DetailFunction::DoubleBiasedMultiply => 0,
            DetailFunction::Multiply => 1,
            DetailFunction::DoubleBiasedAdd => 2,
        };
        let mut base = standard.clone();
        if flags & crate::material::ENVIRONMENT_ALPHA_TESTED != 0 {
            base.alpha_mode = AlphaMode::Mask(0.5);
        }
        assets.environment_materials.add(HaloEnvironmentMaterial {
            base,
            extension: HaloEnvironment {
                params: HaloEnvironmentParams {
                    detail_scales: primary_scale.extend(secondary_scale.x).extend(secondary_scale.y),
                    micro_scale: micro_scale.extend(bump_scale.x).extend(bump_scale.y),
                    modes: UVec4::new(
                        u32::from(environment.kind != EnvironmentType::Normal),
                        function(environment.detail_function),
                        function(environment.micro_detail_function),
                        flags,
                    ),
                },
                base_map,
                primary_detail_map: primary,
                secondary_detail_map: secondary,
                micro_detail_map: micro,
                bump_map,
            },
        })
    }

    /// An environment shader's bump map as a linear normal map, and how many
    /// times it repeats across the base map.
    fn bump_map(
        &mut self,
        environment: &shader::EnvironmentDiffuse,
        base_size: Option<Vec2>,
        images: &mut Assets<Image>,
    ) -> Option<(Handle<Image>, Vec2)> {
        let map = environment.bump_map?;
        let datum = map.bitmap.datum?;
        let image = self.textures.get(self.cache, datum, 0, false, images)?;
        let rescale = match (environment.rescale_bump_map, base_size, bitmap_size(self.cache, datum)) {
            (true, Some(base), Some(bump)) => base / bump,
            _ => Vec2::ONE,
        };
        Some((image, rescale * map.scale))
    }
}

/// StandardMaterial roughness and metalness for a Halo shader.
struct Shininess {
    roughness: f32,
    metallic: f32,
    texture: Option<Handle<Image>>,
}

/// A bitmap group's first bitmap's size.
fn bitmap_size(cache: &CacheFile, datum: u32) -> Option<Vec2> {
    let bitmaps = halo_map::bitmap::bitmaps(cache, datum).ok()?;
    bitmaps.first().map(|b| Vec2::new(b.width as f32, b.height as f32))
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
        .flatten()
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
