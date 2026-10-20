//! A Halo level as Bevy entities: its structure BSPs and the objects its
//! scenario places.

use crate::{
    material::{
        HaloCombinerParams, HaloEnvironment, HaloEnvironmentMaterial, HaloEnvironmentParams, HaloTransparentMaterial,
    },
    textures::TextureCache,
};
use bevy_solarik::prelude::RaytracingMesh3d;
use bevy::{
    asset::RenderAssetUsages,
    ecs::system::EntityCommands,
    mesh::{
        skinning::{SkinnedMesh, SkinnedMeshInverseBindposes},
        Indices, PrimitiveTopology, VertexAttributeValues,
    },
    math::Affine3A,
    platform::collections::HashMap,
    prelude::*,
};
use halo_map::{
    bsp,
    math::{halo_to_bevy, METERS_PER_WORLD_UNIT},
    model::{self, Model},
    objects::{self, Placement},
    shader::{self, ShaderInfo, ShaderType},
    CacheFile, TagRef,
};
use std::sync::Arc;

/// An object's model shaders whose self-illumination animates
/// (`shader::ModelSelfIlluminationAnimation`): each its own copy of the
/// shader's material, whose emissive [`animate_self_illumination`] sets
/// every frame, the object's phase, and the object's change colours.
#[derive(Component, Clone)]
pub struct AnimatedSelfIllumination {
    pub materials: Vec<(Handle<StandardMaterial>, Option<Handle<HaloEnvironmentMaterial>>, shader::ModelSelfIlluminationAnimation)>,
    pub phase: f32,
    pub change_colors: [[f32; 3]; 4],
}

/// A moving object's mesh whose material its change colours can make anew
/// in place ([`LevelBuilder::recolour`]): the object's root, the piece's
/// uncoloured surface and shader, and whether this entity is the rays' (or
/// shares the rays' material) or the camera's alone.
#[derive(Component, Clone)]
pub struct Recolourable {
    pub root: Entity,
    base: Surface,
    shader: Option<ShaderUse>,
    traced: bool,
}

/// A model's shader and its permutation (which of its bitmaps' images it
/// draws with, see [`TextureCache::permuted`]).
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct ShaderUse {
    datum: u32,
    permutation: u16,
}

/// The game's periodic functions, for the animations evaluated here.
#[derive(Resource)]
pub struct PeriodicTables(pub [[u8; 1024]; 12]);

impl Default for PeriodicTables {
    fn default() -> Self {
        PeriodicTables(halo_map::object_functions::periodic_function_tables())
    }
}

/// Each animated model shader's emissive: its colour between the bounds by
/// its periodic function (rasterizer_xbox_models.c), times the object's
/// change colour when it says, at the lighting's emissive scale.
pub fn animate_self_illumination(
    time: Res<Time>,
    tables: Res<PeriodicTables>,
    settings: Res<crate::LightingSettings>,
    objects: Query<&AnimatedSelfIllumination>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut raster_materials: ResMut<Assets<HaloEnvironmentMaterial>>,
) {
    let functions = halo_map::object_functions::FunctionTables {
        periodic: std::array::from_fn(|index| Some(&tables.0[index])),
        // (the transition functions aren't used here; any table will do)
        transition: std::array::from_fn(|_| Some(&tables.0[0])),
    };
    let seconds = time.elapsed_secs();
    for object in &objects {
        for (material, raster, animation) in &object.materials {
            let phase = if animation.random_phase { object.phase } else { 0.0 };
            let fraction = functions.periodic(animation.function, seconds / animation.period + phase);
            let mut color = Vec3::from(animation.lower) + (Vec3::from(animation.upper) - Vec3::from(animation.lower)) * fraction;
            if let Some(source) = animation.color_source {
                color *= Vec3::from(object.change_colors[source]).clamp(Vec3::ZERO, Vec3::ONE);
            }
            let radiance = color * SELF_ILLUMINATION_RADIANCE * settings.emissive_scale;
            let emissive = LinearRgba::rgb(radiance.x, radiance.y, radiance.z);
            // (only when it changes: each change prepares the material
            // again, and a material changed every frame flickers)
            if materials.get(material).is_some_and(|m| m.emissive != emissive)
                && let Some(mut material) = materials.get_mut(material)
            {
                material.emissive = emissive;
            }
            if let Some(raster) = raster
                && raster_materials.get(raster).is_some_and(|m| m.base.emissive != emissive)
                && let Some(mut raster) = raster_materials.get_mut(raster)
            {
                raster.base.emissive = emissive;
            }
        }
    }
}

/// rasterizer_xbox_models.c's self-illumination phase: the first of the
/// object's random numbers, seeded by its object index.
pub fn self_illumination_phase(object_index: u32) -> f32 {
    let seed = object_index.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
    (seed >> 16) as f32 / 65535.0
}

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
/// particular size, behind the level (the game draws it first); this puts it
/// far beyond any level. Its domes are ~10⁵ units across, but some of its
/// parts are much smaller (a50's fog band: a ring ~3200 units across), and
/// at a metre per unit, the level beyond them was hidden behind them.
const SKY_SCALE: f32 = 100.0;

fn sky_scale() -> f32 {
    std::env::var("HALO_RT_SKY_SCALE").ok().and_then(|v| v.parse().ok()).unwrap_or(SKY_SCALE)
}
/// Transparent meshes sort back to front by their bounds' centres plus a
/// bias. Sky layers are drawn first (behind every transparent surface of the
/// level), in Halo's part order: each step is larger than the sky is.
const SKY_BIAS: f32 = -1.0e8;
const SKY_LAYER_BIAS: f32 = 1.0e6;

/// Light fixtures become point lights, a sphere this size (metres).
const LIGHT_PROXY_RADIUS: f32 = 0.12;
/// How far in front of its fixture (metres) a light sits, clear of the wall.
const LIGHT_PROXY_OFFSET: f32 = 0.3;
/// Radiance per unit of fixture intensity, relative to shader radiosity
/// power (a fixture's sphere is far smaller than a glowing panel).
const LIGHT_PROXY_RADIANCE: f32 = 4000.0;
/// How far a fixture's light reaches (metres): Halo's fixtures give none.
const FIXTURE_LIGHT_RANGE: f32 = 30.0;
/// Radiance of fully self-illuminated texels, relative to radiosity power
/// (with the default emissive scale and exposure, about display white).
const SELF_ILLUMINATION_RADIANCE: f32 = 2.5;

/// What a loaded level contains, for the overlay.
#[derive(Resource, Default, Clone)]
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
    pub inverse_bindposes: &'a mut Assets<SkinnedMeshInverseBindposes>,
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
    /// For the rays only: a transparent shader's radiosity emitter (c10's
    /// light cards, an invisible bitmap added), which lit the lightmaps but
    /// which the camera doesn't see (the game's overlay draws whatever it
    /// shows).
    unseen: bool,
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

/// A model split by node for an animated object (see
/// [`LevelBuilder::animated_model`]).
pub struct AnimatedModel {
    pub name: String,
    /// The parts whose every triangle follows one node wholly, split by node
    /// and rigid, for the rays (Solari traces no skinned meshes); with
    /// `skinned`, the camera sees those instead.
    pieces: Vec<ModelPiece>,
    /// The parts that bend between nodes, for the rays: skinned here each
    /// time the nodes move, so that they keep to the surface the camera sees.
    pub traced_skins: Vec<TracedSkin>,
    skinned: Vec<SkinnedPart>,
    /// Identity, one per node: the nodes' transforms already include their
    /// default inverse.
    inverse_bindposes: Handle<SkinnedMeshInverseBindposes>,
    /// Per node, its default inverse matrix (Halo space).
    pub default_inverses: Vec<Affine3A>,
}

struct ModelPiece {
    node: usize,
    mesh: Handle<Mesh>,
    surface: Surface,
    /// Its shader, for change colours.
    shader: Option<ShaderUse>,
}

/// A whole model part as one mesh skinned to the model's nodes (each vertex
/// to its two, weighted), for the camera.
struct SkinnedPart {
    mesh: Handle<Mesh>,
    surface: Surface,
    shader: Option<ShaderUse>,
}

/// How far inside a skinned model its rigid ray-traced copy sits (world
/// units: 2 cm).
const TRACED_INSET: f32 = 0.02 / METERS_PER_WORLD_UNIT;

/// StandardMaterial's reflectance for a surface Halo gives no reflection (1%
/// head-on, not the usual 4%, and half at grazing angles, not all: Solarik
/// lowers grazing reflectance below 2%): Halo shades these diffuse only, and
/// the usual sheen of the sky on them washed grass out grey looking toward the
/// sun (a30's fields).
const MATTE_REFLECTANCE: f32 = 0.25;

/// How deep an actively camouflaged body's glass is (metres): how far it
/// bends the picture behind it.
const CAMOUFLAGE_THICKNESS: f32 = 0.6;

/// Roughness where a reflective model shader's specular mask is zero: a
/// plastic's sheen, not the matte of surfaces Halo gives no reflection.
const MODEL_UNMASKED_ROUGHNESS: f32 = 0.45;

/// Models duller than their shaders' reflections make them (by the shader
/// tag's path): the share of their gloss and metal they keep, the rest
/// toward [`DULL_ROUGHNESS`] (lit by the level's own lights, Chief's
/// armour and the vehicles shone like chrome).
const MODEL_SHINE: [(&str, f32); 8] = [
    ("characters\\cyborg\\", 0.5),
    ("characters\\marine", 0.5),
    ("characters\\grunt\\", 0.8),
    ("vehicles\\scorpion\\", 0.45),
    ("vehicles\\warthog\\", 0.5),
    // (Covenant crates)
    ("scenery\\c_storage\\", 0.7),
    ("scenery\\rocks\\", 0.6),
    ("scenery\\cutscene_small_rock\\", 0.6),
];

/// The roughness a model's gloss fades toward as [`MODEL_SHINE`] dulls it.
const DULL_ROUGHNESS: f32 = 0.6;

/// Chief's visor's shader, drawn as a smooth metal of this colour (Halo's
/// visor reflects gold) instead of by its maps.
const VISOR_SHADER: &str = "characters\\cyborg\\shaders\\visor";
const VISOR_COLOR: Color = Color::srgb(0.8, 0.55, 0.2);
const VISOR_ROUGHNESS: f32 = 0.12;

/// How far inside the camera's skinned surface the rays' skinned copy sits
/// (meters): the two are skinned alike, but on different processors.
const TRACED_SKIN_INSET: f32 = 0.005;

/// A model part that bends between nodes, as the rays see it: its vertices
/// at rest (Bevy space) with their two nodes and weights, skinned by
/// [`TracedSkin::mesh`] to the nodes' current transforms.
pub struct TracedSkin {
    positions: Vec<Vec3>,
    normals: Vec<Vec3>,
    tangents: Vec<Vec4>,
    uvs: Vec<[f32; 2]>,
    joints: Vec<[u16; 2]>,
    weights: Vec<[f32; 2]>,
    /// Counter-clockwise (Bevy's front faces).
    indices: Vec<u32>,
    surface: Surface,
    shader: Option<ShaderUse>,
}

impl TracedSkin {
    pub fn vertex_count(&self) -> usize {
        self.positions.len()
    }

    /// The part skinned to the nodes' transforms (Bevy space, each node's
    /// default inverse included), as the camera's skinned mesh is: each
    /// vertex blended between its two nodes by their weights.
    pub fn mesh(&self, nodes: &[Mat4]) -> Mesh {
        let node = |nodes: &[Mat4], index: u16| nodes.get(index as usize).copied().unwrap_or(Mat4::IDENTITY);
        let count = self.positions.len();
        let (mut positions, mut normals, mut tangents) =
            (Vec::with_capacity(count), Vec::with_capacity(count), Vec::with_capacity(count));
        for i in 0..count {
            let ([a, b], [wa, wb]) = (self.joints[i], self.weights[i]);
            let (ma, mb) = (node(nodes, a), node(nodes, b));
            let (p, n, t) = (self.positions[i], self.normals[i], self.tangents[i]);
            let normal = (wa * ma.transform_vector3(n) + wb * mb.transform_vector3(n)).normalize_or_zero();
            let skinned = wa * ma.transform_point3(p) + wb * mb.transform_point3(p);
            let position = skinned - normal * TRACED_SKIN_INSET;
            let tangent =
                (wa * ma.transform_vector3(t.truncate()) + wb * mb.transform_vector3(t.truncate())).normalize_or_zero();
            positions.push(position.to_array());
            normals.push(normal.to_array());
            tangents.push(tangent.extend(t.w).to_array());
        }
        Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD)
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
            .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
            .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, self.uvs.clone())
            .with_inserted_attribute(Mesh::ATTRIBUTE_TANGENT, tangents)
            .with_inserted_indices(Indices::U32(self.indices.clone()))
    }
}

/// What a mesh entity is for.
enum MeshUse {
    /// The camera and the rays.
    Both,
    /// The rays only.
    Traced,
    /// The camera only, skinned to these joints if given.
    Raster(Option<SkinnedMesh>),
}

/// A Halo `real_matrix4x3` as an affine transform (Halo space).
pub fn halo_affine(m: &halo_map::model::Matrix4x3) -> Affine3A {
    Affine3A::from_cols(
        Vec3A::from(m.forward) * m.scale,
        Vec3A::from(m.left) * m.scale,
        Vec3A::from(m.up) * m.scale,
        Vec3A::from(m.position),
    )
}

/// Halo space (Z up, world units) to Bevy's (Y up, metres) as an affine map.
pub fn halo_to_bevy_affine() -> Affine3A {
    Affine3A::from_mat3(Mat3::from_cols(Vec3::X, Vec3::NEG_Z, Vec3::Y) * METERS_PER_WORLD_UNIT)
}

pub struct LevelBuilder {
    pub cache: Arc<CacheFile>,
    /// While a moving object's piece is spawned: its root, uncoloured
    /// surface and shader, put on its entities as [`Recolourable`].
    recolour: Option<(Entity, Surface, Option<ShaderUse>)>,
    pub textures: TextureCache,
    /// By shader and permutation.
    surfaces: HashMap<(Option<u32>, u16), ShaderSurface>,
    /// While a surface is made: its shader's permutation.
    permutation: u16,
    models: HashMap<(u32, [u8; 8]), Option<ModelMeshes>>,
    animated_models: HashMap<(u32, [u8; 8]), Option<Arc<AnimatedModel>>>,
    /// The glassy look of an actively camouflaged object.
    camouflage: Option<Handle<StandardMaterial>>,
    /// Its liquid glass, in the rays' scene.
    camouflage_glass: Option<Handle<StandardMaterial>>,
    /// Surfaces tinted by a change colour, by shader and colour.
    coloured_surfaces: HashMap<(ShaderUse, [u8; 3]), Surface>,
    loaded_bsps: Vec<usize>,
    /// Each loaded BSP's root entity, by BSP index.
    pub bsp_roots: Vec<(usize, Entity)>,
    light_proxy: Option<Handle<Mesh>>,
    /// Whether light fixtures get emissive proxies.
    pub fixture_lights: bool,
    /// Rasterize the light proxies too (to see where they are).
    pub show_light_proxies: bool,
    /// Ignore Halo's reflection settings: every surface a rough dielectric.
    pub matte: bool,
    /// Leave out transparent surfaces (the game's overlay draws them)...
    pub skip_transparent: bool,
    /// ... but glass and water (thin panes Solarik ray traces), when the
    /// game leaves those out of its overlay.
    pub draw_blended: bool,
    /// The parent of everything this level spawns, so that despawning it
    /// unloads the level (when the bridged game changes level).
    pub root: Option<Entity>,
    /// Each glass or water pane's sky mask (see
    /// [`crate::material::GlassSkyMask`]).
    pub glass_sky_mask: Option<crate::material::GlassSkyMaskMaterial>,
    /// Whether the models spawned are the first-person weapon and hands
    /// (their panes' masks at their own depth).
    pub first_person: bool,
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

impl LevelBuilder {
    pub fn new(cache: Arc<CacheFile>, bc_supported: bool) -> Self {
        LevelBuilder {
            recolour: None,
            glass_sky_mask: None,
            first_person: false,
            textures: TextureCache::new(bc_supported),
            surfaces: HashMap::default(),
            permutation: 0,
            models: HashMap::default(),
            animated_models: HashMap::default(),
            coloured_surfaces: HashMap::default(),
            camouflage: None,
            camouflage_glass: None,
            loaded_bsps: Vec::new(),
            bsp_roots: Vec::new(),
            light_proxy: None,
            fixture_lights: true,
            show_light_proxies: false,
            matte: false,
            skip_transparent: false,
            draw_blended: true,
            root: None,
            stats: LevelStats { map_name: cache.header.name.clone(), ..default() },
            cache,
        }
    }

    /// Puts a top-level entity of this level under [`Self::root`].
    pub fn adopt(&self, commands: &mut Commands, entity: Entity) {
        if let Some(root) = self.root {
            commands.entity(entity).insert(ChildOf(root));
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
        for reference in bsp::references(&self.cache)? {
            if only.is_some_and(|i| i != reference.index) {
                continue;
            }
            let loaded = bsp::Bsp::load(&self.cache, &reference)?;
            let level = commands
                .spawn((Name::new(reference.name.clone()), Transform::default(), Visibility::default()))
                .id();
            self.adopt(commands, level);
            for (material_index, material) in loaded.materials()?.into_iter().enumerate() {
                if material.indices.is_empty() {
                    continue;
                }
                let shader = self.surface(material.shader, 0, assets);
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
            self.bsp_roots.push((reference.index, level));
            self.stats.bsps += 1;
        }
        self.stats.texture_failures = self.textures.failures;
        Ok(())
    }

    /// Spawns the objects the game would create in the loaded BSPs; with
    /// `lights_only`, only the light fixtures' lights (when a running game
    /// places the objects, see [`crate::bridge`]).
    pub fn spawn_objects(
        &mut self,
        raster: bool,
        lights_only: bool,
        commands: &mut Commands,
        assets: &mut LevelAssets,
    ) -> halo_map::Result<()> {
        let placements = objects::placements(&self.cache)?;
        let objects_root = commands.spawn((Name::new("objects"), Transform::default(), Visibility::default())).id();
        self.adopt(commands, objects_root);
        for placement in &placements {
            // (multiplayer vehicles have no BSP flags: the game type spawns them)
            let in_loaded_bsp =
                placement.on_bsp_flags == 0 || self.loaded_bsps.iter().any(|&b| placement.in_bsp(b));
            if placement.not_automatic || !in_loaded_bsp {
                continue;
            }
            if lights_only && placement.light.is_none() {
                continue;
            }
            // (light fixtures often have no model, only their light)
            let model = if lights_only {
                None
            } else {
                objects::object_model(&self.cache, placement.definition)?.and_then(|m| m.datum)
            };
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
            if placement.on_bsp_flags != 0 {
                commands.entity(object).insert(OnBsps(placement.on_bsp_flags as u32));
            }
            for (mesh, surface, triangles) in parts.into_iter().flatten() {
                self.spawn_mesh(commands, object, name.clone(), mesh, &surface, raster);
                self.stats.object_triangles += triangles;
            }
            if let Some(light) = placement.light
                && light.intensity > 0.0
                && self.fixture_lights
            {
                self.spawn_fixture_light(commands, object, Vec3::from(light.color) * light.intensity, assets);
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
        let cache = self.cache.clone();
        let model = Model::load(&cache, model_datum)?;
        let dome = commands
            .spawn((Name::new(model.name.clone()), SkyDome, Transform::default(), Visibility::default()))
            .id();
        self.adopt(commands, dome);
        let mut layer = 0;
        for geometry in model.geometries_for(&[], model::HIGHEST_DETAIL) {
            for part in model.geometry(geometry)? {
                let Some(shader_ref) = model.shaders.get(part.shader_index.max(0) as usize).copied() else { continue };
                let Some(info) = shader::shader_info(&self.cache, shader_ref)? else { continue };
                let depth_bias = SKY_BIAS + layer as f32 * SKY_LAYER_BIAS;
                if let Some(material) = self.transparent_material(&info, depth_bias, assets) {
                    let vertices = part.vertices.iter().map(|v| SourceVertex {
                        position: v.position.map(|c| c * sky_scale() / METERS_PER_WORLD_UNIT),
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
                        map.bitmap.datum.and_then(|d| self.textures.get(&self.cache, d, 0, true, assets.images)),
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
                    position: v.position.map(|c| c * sky_scale() / METERS_PER_WORLD_UNIT),
                    normal: v.normal,
                    binormal: v.binormal,
                    tangent: v.tangent,
                    texcoord: [v.texcoord[0] + uv_offset[0] / uv_scale[0].max(1e-6), v.texcoord[1] + uv_offset[1] / uv_scale[1].max(1e-6)],
                });
                let mesh = assets.meshes.add(build_model_mesh(vertices, &part.indices, uv_scale));
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

    /// A light fixture's light: a point light just in front of it, as
    /// bright as the emissive sphere that stood in for it under Solari (a
    /// sphere of radiance L and radius r is L π r² candela, 4π times that in
    /// lumens). With `show_light_proxies` the camera sees that sphere too.
    fn spawn_fixture_light(
        &mut self,
        commands: &mut Commands,
        object: Entity,
        light: Vec3,
        assets: &mut LevelAssets,
    ) {
        let power = light * LIGHT_PROXY_RADIANCE * sphere_lumens_per_radiance(LIGHT_PROXY_RADIUS);
        let mut source = commands.spawn((
            Name::new("light fixture light"),
            PointLight { radius: LIGHT_PROXY_RADIUS, range: FIXTURE_LIGHT_RANGE, shadow_maps_enabled: false, ..default() },
            LightSource { power },
            // (lighting the rays' scene from off screen too: Bevy's frustum
            // culling would leave it out with its range off screen)
            bevy::camera::visibility::NoFrustumCulling,
            // object space is Bevy-converted Halo space: Halo forward (+x) is +x
            Transform::from_xyz(LIGHT_PROXY_OFFSET, 0.0, 0.0),
            ChildOf(object),
        ));
        if self.show_light_proxies {
            let mesh = self
                .light_proxy
                .get_or_insert_with(|| assets.meshes.add(Sphere::new(LIGHT_PROXY_RADIUS).mesh().build()))
                .clone();
            let material = assets.materials.add(StandardMaterial {
                base_color: Color::BLACK,
                emissive: LinearRgba::rgb(light.x, light.y, light.z) * LIGHT_PROXY_RADIANCE,
                unlit: true,
                ..default()
            });
            source.insert((Mesh3d(mesh), MeshMaterial3d(material)));
        }
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
        let mode = if raster { MeshUse::Both } else { MeshUse::Traced };
        self.spawn_mesh_as(commands, parent, name, mesh, surface, mode);
    }

    /// Spawns a mesh for the camera, for the rays, or both (see [`MeshUse`]);
    /// the rays' entity, if any.
    fn spawn_mesh_as(
        &mut self,
        commands: &mut Commands,
        parent: Entity,
        name: String,
        mesh: Handle<Mesh>,
        surface: &Surface,
        mode: MeshUse,
    ) -> Option<Entity> {
        let (traced_use, raster_use) = match &mode {
            MeshUse::Both => (true, true),
            MeshUse::Traced => (true, false),
            MeshUse::Raster(_) => (false, true),
        };
        let skin = match mode {
            MeshUse::Raster(skin) => skin,
            _ => None,
        };
        // (a skinned mesh is placed by its joints: its rest-pose bounds
        // would cull it wrongly)
        let skinned = |entity: &mut EntityCommands, skin: Option<SkinnedMesh>| {
            if let Some(skin) = skin {
                entity.insert((skin, bevy::camera::visibility::NoFrustumCulling));
            }
        };
        let surface = match surface {
            Surface::Solid(solid) => solid,
            Surface::Transparent(_) | Surface::Blended(_) => {
                let skipped = match surface {
                    Surface::Blended(_) => !self.draw_blended,
                    _ => self.skip_transparent,
                };
                if raster_use && !skipped {
                    let mut drawn = commands.spawn((
                        Name::new(name),
                        Mesh3d(mesh.clone()),
                        bevy::light::NotShadowCaster,
                        Transform::default(),
                        ChildOf(parent),
                    ));
                    match surface {
                        Surface::Transparent(material) => drawn.insert(MeshMaterial3d(material.clone())),
                        // glass and water are thin panes in the ray-traced
                        // scene too: Solarik reflects, tints and shadows
                        // through them, and composites the camera's panes in
                        // place of the raster draw once it can
                        Surface::Blended(material) if traced_use => {
                            drawn.insert((MeshMaterial3d(material.clone()), RaytracingMesh3d(mesh.clone()), TracedMesh(mesh.clone())))
                        }
                        Surface::Blended(material) => drawn.insert(MeshMaterial3d(material.clone())),
                        Surface::Solid(_) => unreachable!(),
                    };
                    skinned(&mut drawn, skin.clone());
                    // (a pane's sky mask: the sky's layers not over it)
                    if let (Surface::Blended(_), Some(masks)) = (surface, &self.glass_sky_mask) {
                        let mask = if self.first_person { &masks.near } else { &masks.far };
                        let mut masked = commands.spawn((
                            Name::new("sky mask"),
                            Mesh3d(mesh.clone()),
                            MeshMaterial3d(mask.clone()),
                            bevy::light::NotShadowCaster,
                            Transform::default(),
                            ChildOf(parent),
                        ));
                        skinned(&mut masked, skin);
                    }
                    self.stats.meshes += 1;
                    self.stats.transparent_meshes += 1;
                }
                return None;
            }
        };
        let mut traced = None;
        if traced_use {
            let mut entity = commands.spawn((
                Name::new(name.clone()),
                RaytracingMesh3d(mesh.clone()),
                TracedMesh(mesh.clone()),
                Visibility::Inherited,
                MeshMaterial3d(surface.standard.clone()),
                Transform::default(),
                ChildOf(parent),
            ));
            if let Some(radiance) = surface.radiance {
                entity.insert(EmissiveSurface { radiance });
                self.stats.emissive_meshes += 1;
            }
            if let Some((root, base, shader)) = &self.recolour {
                entity.insert(Recolourable { root: *root, base: base.clone(), shader: *shader, traced: true });
            }
            traced = Some(entity.id());
        }
        if raster_use && !surface.unseen {
            match (&surface.raster, traced) {
                // the same entity, when the camera sees what the rays do and
                // nothing moves it differently
                (None, Some(traced)) if skin.is_none() => {
                    commands.entity(traced).insert(Mesh3d(mesh));
                }
                (raster, _) => {
                    let mut drawn = commands.spawn((Name::new(name), Mesh3d(mesh), Transform::default(), ChildOf(parent)));
                    match raster {
                        // the camera sees the Halo material; rays see the standard one
                        Some(material) => drawn.insert(MeshMaterial3d(material.clone())),
                        None => drawn.insert(MeshMaterial3d(surface.standard.clone())),
                    };
                    if let Some(radiance) = surface.radiance {
                        drawn.insert(EmissiveSurface { radiance });
                    }
                    if let Some((root, base, shader)) = &self.recolour {
                        drawn.insert(Recolourable { root: *root, base: base.clone(), shader: *shader, traced: false });
                    }
                    skinned(&mut drawn, skin);
                }
            }
        }
        self.stats.meshes += 1;
        traced
    }

    /// A model's meshes split by node, for an object the game moves (see
    /// [`crate::bridge`]): each triangle goes with the node that moves its
    /// first vertex most (a rigid stand-in for Halo's two-node skinning), and
    /// a node's pieces are drawn at `node matrix * default inverse`, as the
    /// game draws its parts. Made once per (model, permutations).
    pub fn animated_model(
        &mut self,
        datum: u32,
        permutations: [u8; 8],
        assets: &mut LevelAssets,
    ) -> Option<Arc<AnimatedModel>> {
        if let Some(cached) = self.animated_models.get(&(datum, permutations)) {
            return cached.clone();
        }
        let built = match self.build_animated_model(datum, &permutations, assets) {
            Ok(model) => Some(Arc::new(model)),
            Err(e) => {
                let name = self.cache.tag(datum).map(|t| t.name.clone()).unwrap_or_default();
                warn!("model {name}: {e}");
                self.stats.object_failures += 1;
                None
            }
        };
        self.animated_models.insert((datum, permutations), built.clone());
        built
    }

    fn build_animated_model(
        &mut self,
        datum: u32,
        permutations: &[u8],
        assets: &mut LevelAssets,
    ) -> halo_map::Result<AnimatedModel> {
        let cache = self.cache.clone();
        let model = Model::load(&cache, datum)?;
        let mut pieces = Vec::new();
        let mut traced_skins = Vec::new();
        let mut skinned = Vec::new();
        for geometry in model.geometries_for(permutations, model.finest_played_detail()) {
            for part in model.geometry(geometry)? {
                if part.indices.is_empty() {
                    continue;
                }
                let shader_ref = model
                    .shaders
                    .get(part.shader_index.max(0) as usize)
                    .copied()
                    .unwrap_or(TagRef { group: 0, datum: None });
                let permutation = model.shader_permutations.get(part.shader_index.max(0) as usize).copied().unwrap_or(0);
                let shader_use = shader_ref.datum.map(|datum| ShaderUse { datum, permutation });
                let shader = self.surface(shader_ref, permutation, assets);
                let Some(surface) = shader.surface else {
                    self.stats.skipped_transparent += 1;
                    continue;
                };
                let last_node = model.nodes.len().saturating_sub(1) as u16;
                let joints: Vec<[u16; 4]> = part
                    .vertices
                    .iter()
                    .map(|v| [(v.nodes[0] as u16).min(last_node), (v.nodes[1] as u16).min(last_node), 0, 0])
                    .collect();
                let weights: Vec<[f32; 4]> = part
                    .vertices
                    .iter()
                    .map(|v| {
                        let (a, b) = (v.node_weights[0].max(0.0), v.node_weights[1].max(0.0));
                        let total = a + b;
                        if total > 0.0 { [a / total, b / total, 0.0, 0.0] } else { [1.0, 0.0, 0.0, 0.0] }
                    })
                    .collect();
                // (a vertex's one node, where it follows only that)
                let sole_node = |index: u32| {
                    let (j, w) = (joints[index as usize], weights[index as usize]);
                    if w[1] == 0.0 || j[0] == j[1] {
                        Some(j[0])
                    } else if w[0] == 0.0 {
                        Some(j[1])
                    } else {
                        None
                    }
                };
                let rigid = part.indices.chunks_exact(3).all(|t| {
                    let node = sole_node(t[0]);
                    node.is_some() && sole_node(t[1]) == node && sole_node(t[2]) == node
                });
                if !rigid {
                    let source: Vec<SourceVertex> = part
                        .vertices
                        .iter()
                        .map(|v| SourceVertex {
                            position: v.position,
                            normal: v.normal,
                            binormal: v.binormal,
                            tangent: v.tangent,
                            texcoord: v.texcoord,
                        })
                        .collect();
                    let rest = build_model_mesh(source.into_iter(), &part.indices, shader.uv_scale);
                    let vec3s = |attribute| match rest.attribute(attribute) {
                        Some(VertexAttributeValues::Float32x3(values)) => values.iter().map(|&v| Vec3::from(v)).collect(),
                        _ => Vec::new(),
                    };
                    let tangents = match rest.attribute(Mesh::ATTRIBUTE_TANGENT) {
                        Some(VertexAttributeValues::Float32x4(values)) => values.iter().map(|&v| Vec4::from(v)).collect(),
                        _ => Vec::new(),
                    };
                    let uvs = match rest.attribute(Mesh::ATTRIBUTE_UV_0) {
                        Some(VertexAttributeValues::Float32x2(values)) => values.clone(),
                        _ => Vec::new(),
                    };
                    let indices = match rest.indices() {
                        Some(Indices::U32(indices)) => indices.clone(),
                        _ => Vec::new(),
                    };
                    traced_skins.push(TracedSkin {
                        positions: vec3s(Mesh::ATTRIBUTE_POSITION),
                        normals: vec3s(Mesh::ATTRIBUTE_NORMAL),
                        tangents,
                        uvs,
                        joints: joints.iter().map(|j| [j[0], j[1]]).collect(),
                        weights: weights.iter().map(|w| [w[0], w[1]]).collect(),
                        indices,
                        surface: surface.clone(),
                        shader: shader_use,
                    });
                }
                let mut by_node: HashMap<usize, Vec<u32>> = HashMap::default();
                for triangle in part.indices.chunks_exact(3).filter(|_| rigid) {
                    let v = &part.vertices[triangle[0] as usize];
                    let node = if v.node_weights[0] >= v.node_weights[1] { v.nodes[0] } else { v.nodes[1] } as usize;
                    by_node.entry(node.min(model.nodes.len().saturating_sub(1))).or_default().extend_from_slice(triangle);
                }
                let source = || {
                    part.vertices.iter().map(|v| SourceVertex {
                        position: v.position,
                        normal: v.normal,
                        binormal: v.binormal,
                        tangent: v.tangent,
                        texcoord: v.texcoord,
                    })
                };
                // (the rays' rigid copy sits a little inside the skinned
                // surface the camera sees: where it bends differently, rays
                // leaving that surface would otherwise hit it, and the model
                // would shadow itself in blotches)
                let inset = |v: SourceVertex| SourceVertex {
                    position: std::array::from_fn(|i| v.position[i] - v.normal[i] * TRACED_INSET),
                    ..v
                };
                for (node, indices) in by_node {
                    let mesh = assets.meshes.add(build_model_mesh(source().map(inset), &indices, shader.uv_scale));
                    pieces.push(ModelPiece { node, mesh, surface: surface.clone(), shader: shader_use });
                }
                // the whole part, skinned: each vertex follows its nodes by
                // their weights, so joints bend without seams
                let mesh = build_model_mesh(source(), &part.indices, shader.uv_scale)
                    .with_inserted_attribute(Mesh::ATTRIBUTE_JOINT_INDEX, VertexAttributeValues::Uint16x4(joints))
                    .with_inserted_attribute(Mesh::ATTRIBUTE_JOINT_WEIGHT, weights);
                skinned.push(SkinnedPart { mesh: assets.meshes.add(mesh), surface: surface.clone(), shader: shader_use });
            }
        }
        let default_inverses: Vec<Affine3A> = model.nodes.iter().map(|n| halo_affine(&n.default_inverse)).collect();
        let inverse_bindposes =
            assets.inverse_bindposes.add(SkinnedMeshInverseBindposes::from(vec![Mat4::IDENTITY; default_inverses.len().max(1)]));
        Ok(AnimatedModel { name: model.name, pieces, traced_skins, skinned, inverse_bindposes, default_inverses })
    }

    /// Spawns an animated model: a root, an entity per node it draws (whose
    /// transforms the caller sets each frame) and its pieces under them.
    pub fn spawn_animated_model(
        &mut self,
        commands: &mut Commands,
        model: &AnimatedModel,
        raster: bool,
        change_colors: &[[f32; 3]; 4],
        camouflaged: bool,
        object_index: u32,
        assets: &mut LevelAssets,
    ) -> (Entity, Vec<Option<Entity>>, Vec<Entity>) {
        let root = commands.spawn((Name::new(model.name.clone()), Transform::default(), Visibility::default())).id();
        // the shaders whose self-illumination animates: a copy of each one's
        // material for this object (its phase is its own)
        let mut animated: Vec<(Handle<StandardMaterial>, Option<Handle<HaloEnvironmentMaterial>>, shader::ModelSelfIlluminationAnimation)> =
            Vec::new();
        let mut animated_by_shader: HashMap<ShaderUse, Surface> = HashMap::new();
        let mut animate = |builder: &mut Self, surface: Surface, shader: Option<ShaderUse>, assets: &mut LevelAssets| -> Surface {
            let (Surface::Solid(solid), Some(shader)) = (&surface, shader) else { return surface };
            if let Some(copy) = animated_by_shader.get(&shader) {
                return copy.clone();
            }
            let info = shader::shader_info(&builder.cache, TagRef { group: 0, datum: Some(shader.datum) }).ok().flatten();
            let Some(animation) = info.and_then(|i| i.model_self_illumination_animation) else { return surface };
            // (one that holds its upper colour is the shader's own material:
            // function 0 is "one")
            if animation.color_source.is_none() && (animation.function == 0 || animation.lower == animation.upper) {
                return surface;
            }
            let Some(material) = assets.materials.get(&solid.standard).cloned() else { return surface };
            let handle = assets.materials.add(material);
            let raster = solid.raster.as_ref().and_then(|raster| assets.environment_materials.get(raster).cloned()).map(|raster| assets.environment_materials.add(raster));
            animated.push((handle.clone(), raster.clone(), animation));
            let copy = Surface::Solid(SolidSurface { standard: handle, raster: raster.or(solid.raster.clone()), ..solid.clone() });
            animated_by_shader.insert(shader, copy.clone());
            copy
        };
        self.adopt(commands, root);
        // every node, as the skinned parts' joints
        let nodes: Vec<Entity> = (0..model.default_inverses.len().max(1))
            .map(|_| commands.spawn((Transform::default(), Visibility::default(), ChildOf(root))).id())
            .collect();
        // camouflaged: a body of liquid glass in the rays' scene, which
        // Solarik composites for the camera, bending the picture behind it
        // and reflecting what is round it (and, as glass, it casts no
        // shadow); the game's own shimmer refracts the picture behind, which
        // its overlay doesn't have. The first-person arms and weapon, posed
        // for the camera and unbent for the rays, keep a faint shell.
        if camouflaged && !self.first_person {
            let material = self.camouflage_glass(assets);
            let glass = |commands: &mut Commands, parent: Entity, mesh: Handle<Mesh>| {
                commands
                    .spawn((
                        Name::new(format!("{} (camouflaged)", model.name)),
                        RaytracingMesh3d(mesh.clone()),
                        TracedMesh(mesh),
                        Visibility::Inherited,
                        MeshMaterial3d(material.clone()),
                        Transform::default(),
                        ChildOf(parent),
                    ))
                    .id()
            };
            for piece in &model.pieces {
                glass(commands, nodes[piece.node], piece.mesh.clone());
            }
            // (its bending parts skinned anew as its nodes move, as below)
            let rest = vec![Mat4::IDENTITY; model.default_inverses.len()];
            let skin_parent = nodes.first().copied().unwrap_or(root);
            let skins = model.traced_skins.iter().map(|skin| glass(commands, skin_parent, assets.meshes.add(skin.mesh(&rest)))).collect();
            return (root, nodes.into_iter().map(Some).collect(), skins);
        }
        if camouflaged {
            if raster {
                // (drawn even where the game's overlay has the other
                // transparent surfaces: this one is the viewer's own)
                let material = self.camouflage_material(assets);
                for part in &model.skinned {
                    let skin = SkinnedMesh { inverse_bindposes: model.inverse_bindposes.clone(), joints: nodes.clone() };
                    commands.spawn((
                        Name::new(format!("{} (camouflaged)", model.name)),
                        Mesh3d(part.mesh.clone()),
                        MeshMaterial3d(material.clone()),
                        bevy::light::NotShadowCaster,
                        skin,
                        bevy::camera::visibility::NoFrustumCulling,
                        Transform::default(),
                        ChildOf(root),
                    ));
                }
            }
            return (root, nodes.into_iter().map(Some).collect(), Vec::new());
        }
        // the rays see the rigid pieces; the camera sees the skinned parts,
        // but glass and water: a pane both see, which Solarik ray traces
        // and composites for the camera as the map's own panes
        for piece in &model.pieces {
            let surface = self.coloured_surface(&piece.surface, piece.shader, change_colors, assets);
            let surface = animate(self, surface, piece.shader, assets);
            let pane = matches!(surface, Surface::Blended(_));
            let mode = if raster && (model.skinned.is_empty() || pane) { MeshUse::Both } else { MeshUse::Traced };
            // (the first-person arms and weapon are the camera's alone, but
            // their panes: posed for its view, in the rays' scene they'd be
            // reflected and shadow where nothing is, and the panes traced
            // from the camera, the sea's, showed their outline around them)
            let mode = match mode {
                _ if !self.first_person || pane => mode,
                MeshUse::Both => MeshUse::Raster(None),
                _ => continue,
            };
            self.recolour = Some((root, piece.surface.clone(), piece.shader));
            let _ = self.spawn_mesh_as(commands, nodes[piece.node], model.name.clone(), piece.mesh.clone(), &surface, mode);
            self.recolour = None;
        }
        // the bending parts, under the model's root node, given a mesh
        // skinned anew (relative to that node) as its nodes move
        // (crate::bridge)
        let rest = vec![Mat4::IDENTITY; model.default_inverses.len()];
        let skin_parent = nodes.first().copied().unwrap_or(root);
        let mut skins = Vec::new();
        for skin in &model.traced_skins {
            let surface = self.coloured_surface(&skin.surface, skin.shader, change_colors, assets);
            let surface = animate(self, surface, skin.shader, assets);
            let mesh = assets.meshes.add(skin.mesh(&rest));
            let mode = if raster && matches!(surface, Surface::Blended(_)) { MeshUse::Both } else { MeshUse::Traced };
            // (none for the first-person arms and weapon, as above)
            if self.first_person && !matches!(mode, MeshUse::Both) {
                continue;
            }
            self.recolour = Some((root, skin.surface.clone(), skin.shader));
            // (a pane is the camera's too, unbent, under the root as before;
            // the first-person one stays unbent for the rays too)
            if matches!(mode, MeshUse::Both) {
                let pane = if self.first_person { MeshUse::Both } else { MeshUse::Raster(None) };
                let _ = self.spawn_mesh_as(commands, root, model.name.clone(), mesh.clone(), &surface, pane);
            }
            if !self.first_person
                && let Some(traced) =
                    self.spawn_mesh_as(commands, skin_parent, model.name.clone(), mesh, &surface, MeshUse::Traced)
            {
                skins.push(traced);
            }
            self.recolour = None;
        }
        if raster {
            for part in &model.skinned {
                // (its panes are drawn above)
                if matches!(part.surface, Surface::Blended(_)) {
                    continue;
                }
                let surface = self.coloured_surface(&part.surface, part.shader, change_colors, assets);
                let surface = animate(self, surface, part.shader, assets);
                let skin = SkinnedMesh { inverse_bindposes: model.inverse_bindposes.clone(), joints: nodes.clone() };
                self.recolour = Some((root, part.surface.clone(), part.shader));
                let _ =
                    self.spawn_mesh_as(commands, root, model.name.clone(), part.mesh.clone(), &surface, MeshUse::Raster(Some(skin)));
                self.recolour = None;
            }
        }
        if !animated.is_empty() {
            commands.entity(root).insert(AnimatedSelfIllumination {
                materials: animated,
                phase: self_illumination_phase(object_index),
                change_colors: *change_colors,
            });
        }
        (root, nodes.into_iter().map(Some).collect(), skins)
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
        let cache = self.cache.clone();
        let model = Model::load(&cache, datum)?;
        let mut out = Vec::new();
        for geometry in model.geometries_for(permutations, model.finest_played_detail()) {
            for part in model.geometry(geometry)? {
                if part.indices.is_empty() {
                    continue;
                }
                let shader_ref = model
                    .shaders
                    .get(part.shader_index.max(0) as usize)
                    .copied()
                    .unwrap_or(TagRef { group: 0, datum: None });
                let permutation = model.shader_permutations.get(part.shader_index.max(0) as usize).copied().unwrap_or(0);
                let shader = self.surface(shader_ref, permutation, assets);
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
                let mesh = assets.meshes.add(build_model_mesh(vertices, &part.indices, shader.uv_scale));
                out.push((mesh, surface, part.indices.len() / 3));
            }
        }
        Ok(out)
    }

    /// An actively camouflaged object's liquid glass: clear but for a cool
    /// tinge, smooth, as thick as a limb (Solarik bends the picture behind
    /// it through that depth).
    fn camouflage_glass(&mut self, assets: &mut LevelAssets) -> Handle<StandardMaterial> {
        self.camouflage_glass
            .get_or_insert_with(|| {
                assets.materials.add(StandardMaterial {
                    base_color: Color::srgb(0.93, 0.97, 1.0),
                    perceptual_roughness: 0.05,
                    reflectance: 0.5,
                    specular_transmission: 1.0,
                    thickness: CAMOUFLAGE_THICKNESS,
                    alpha_mode: AlphaMode::Blend,
                    cull_mode: None,
                    double_sided: true,
                    ..default()
                })
            })
            .clone()
    }

    /// The glassy look of an actively camouflaged object: nearly clear,
    /// smooth, reflecting what is around it.
    fn camouflage_material(&mut self, assets: &mut LevelAssets) -> Handle<StandardMaterial> {
        self.camouflage
            .get_or_insert_with(|| {
                assets.materials.add(StandardMaterial {
                    base_color: Color::srgba(0.55, 0.65, 0.8, 0.08),
                    alpha_mode: AlphaMode::Blend,
                    perceptual_roughness: 0.15,
                    reflectance: 0.3,
                    ..default()
                })
            })
            .clone()
    }

    /// A piece's surface with the object's change colour, where its shader
    /// takes one (made once per shader and colour).
    /// An object's meshes in new change colours, their materials swapped in
    /// place: remaking the model left it missing a frame and lost its
    /// motion's history, and the plasma rifle's colours follow its heat.
    pub fn recolour<'a>(
        &mut self,
        commands: &mut Commands,
        entities: impl Iterator<Item = (Entity, &'a Recolourable)>,
        change_colors: &[[f32; 3]; 4],
        assets: &mut LevelAssets,
    ) {
        for (entity, recolourable) in entities {
            let Surface::Solid(solid) = self.coloured_surface(&recolourable.base, recolourable.shader, change_colors, assets) else {
                continue;
            };
            match (&solid.raster, recolourable.traced) {
                (Some(raster), false) => commands.entity(entity).insert(MeshMaterial3d(raster.clone())),
                _ => commands.entity(entity).insert(MeshMaterial3d(solid.standard.clone())),
            };
        }
    }

    fn coloured_surface(
        &mut self,
        base: &Surface,
        shader: Option<ShaderUse>,
        change_colors: &[[f32; 3]; 4],
        assets: &mut LevelAssets,
    ) -> Surface {
        let Surface::Solid(solid) = base else { return base.clone() };
        let Some(shader) = shader else { return base.clone() };
        let info = shader::shader_info(&self.cache, TagRef { group: 0, datum: Some(shader.datum) }).ok().flatten();
        let Some((source, multipurpose)) =
            info.as_ref().and_then(|i| Some((i.change_color?, i.multipurpose_map.and_then(|m| m.datum)?)))
        else {
            return base.clone();
        };
        let color = change_colors[source].map(|c| (c.clamp(0.0, 1.0) * 255.0).round() as u8);
        // (Chief's visor is its own colour, not its base map's)
        if color == [255; 3] || info.as_ref().is_some_and(|i| i.name.eq_ignore_ascii_case(VISOR_SHADER)) {
            return base.clone();
        }
        if let Some(surface) = self.coloured_surfaces.get(&(shader, color)) {
            return surface.clone();
        }
        let bake = crate::textures::Bake::ChangeColor {
            base: info.as_ref().and_then(|i| i.base_map).and_then(|b| b.datum),
            multipurpose,
            color,
        };
        let surface = match (self.textures.baked(&self.cache, bake, shader.permutation, assets.images), assets.materials.get(&solid.standard)) {
            (Some(texture), Some(material)) => {
                let material = StandardMaterial { base_color_texture: Some(texture.clone()), ..material.clone() };
                // (and the camera's, a model's detail, over the coloured base)
                let raster = solid.raster.as_ref().and_then(|raster| assets.environment_materials.get(raster).cloned()).map(|mut raster| {
                    raster.base.base_color_texture = Some(texture.clone());
                    raster.extension.base_map = texture;
                    assets.environment_materials.add(raster)
                });
                Surface::Solid(SolidSurface { standard: assets.materials.add(material), raster: raster.or(solid.raster.clone()), ..solid.clone() })
            }
            _ => base.clone(),
        };
        debug!("change colour {color:?} on {}", info.as_ref().map_or("", |i| i.name.as_str()));
        self.coloured_surfaces.insert((shader, color), surface.clone());
        surface
    }

    /// The material for a shader, made once per shader and permutation.
    fn surface(
        &mut self,
        shader: TagRef,
        permutation: u16,
        assets: &mut LevelAssets,
    ) -> ShaderSurface {
        if let Some(surface) = self.surfaces.get(&(shader.datum, permutation)) {
            return surface.clone();
        }
        self.permutation = permutation;
        let info = match shader::shader_info(&self.cache, shader) {
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
        if permutation != 0 {
            debug!("shader {} at permutation {permutation}", info.as_ref().map_or("-", |i| i.name.as_str()));
        }
        self.permutation = 0;
        self.surfaces.insert((shader.datum, permutation), surface.clone());
        surface
    }

    fn make_surface(&mut self, info: Option<&ShaderInfo>, assets: &mut LevelAssets) -> Option<Surface> {
        // (HALO_RT_SKIP_SHADERS, debugging: shaders whose names hold any of
        // these comma-separated words, not drawn)
        if let (Some(info), Ok(skip)) = (info, std::env::var("HALO_RT_SKIP_SHADERS"))
            && skip.split(',').any(|word| !word.is_empty() && info.name.contains(word))
        {
            return None;
        }
        let Some(info) = info else {
            let standard = assets.materials.add(StandardMaterial {
                base_color: Color::srgb(0.5, 0.5, 0.5),
                perceptual_roughness: 0.9,
                ..default()
            });
            return Some(Surface::Solid(SolidSurface { standard, raster: None, radiance: None, unseen: false }));
        };
        let base_color_texture = info
            .base_map
            .and_then(|r| r.datum)
            .and_then(|datum| self.textures.permuted(&self.cache, datum, self.permutation, true, assets.images));
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
                Some((self.textures.baked(&self.cache, bake, self.permutation, assets.images)?, Vec3::from(color_upper)))
            }
            shader::SelfIllumination::Environment { map, primary, secondary, plasma } => {
                let bake = crate::textures::Bake::EnvironmentSelfIllumination {
                    base: info.base_map.and_then(|b| b.datum),
                    map: map.datum?,
                    colors: [primary, secondary, plasma].map(|c| c.map(f32::to_bits)),
                };
                Some((self.textures.baked(&self.cache, bake, self.permutation, assets.images)?, Vec3::ONE))
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
            reflectance: shine.reflectance,
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
        // Chief's visor: a gold mirror of what is round it (its base map is
        // black, which as a metal reflects nothing, with a reflection
        // painted on in grey)
        let base_color_texture = if info.name.eq_ignore_ascii_case(VISOR_SHADER) {
            material.base_color = VISOR_COLOR;
            material.base_color_texture = None;
            material.metallic = 1.0;
            material.perceptual_roughness = VISOR_ROUGHNESS;
            material.metallic_roughness_texture = None;
            None
        } else {
            base_color_texture
        };
        // a model shader's cut-out (the game's alpha test, reference 0x7f, on
        // the base map's alpha: leaves, grilles) and its two sides
        if info.environment.is_none() && info.alpha_tested && base_color_texture.is_some() {
            material.alpha_mode = AlphaMode::Mask(0.5);
        }
        if info.two_sided {
            material.double_sided = true;
            material.cull_mode = None;
            // leaves let light through: Solarik's thin foliage, for a
            // two-sided cut-out of a plant (not a grille); deferred, which
            // Bevy's automatic choice isn't for transmission
            if material.alpha_mode == AlphaMode::Mask(0.5) && is_foliage(&info.name) {
                material.diffuse_transmission = FOLIAGE_TRANSMISSION;
                material.opaque_render_method = bevy::material::OpaqueRendererMethod::Deferred;
            }
        }
        if let Some(environment) = &info.environment {
            let base_size = info.base_map.and_then(|b| b.datum).and_then(|d| bitmap_size(&self.cache, d));
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
            // (a model's detail map: the camera's; the rays keep the base)
            (None, Some(base_map)) if info.model_detail.is_some() => {
                self.model_detail_material(base_map.clone(), &material, info, assets)
            }
            _ => None,
        };
        let unseen = info.shader_type.is_transparent();
        // (and the rays don't see it either: masked out everywhere, every
        // ray passes through it, so nothing reflects it and it shadows
        // nothing, while Solarik's light sampling still takes its emission,
        // which a mask doesn't scale)
        if unseen {
            material.base_color = material.base_color.with_alpha(0.0);
            material.alpha_mode = AlphaMode::Mask(0.5);
        }
        Some(Surface::Solid(SolidSurface { standard: assets.materials.add(material), raster, radiance, unseen }))
    }

    /// A `shader_model`'s camera material with its detail map (see
    /// `shader::ModelDetail` and `halo_environment.wesl`'s model mode), over
    /// `standard`; None if its detail map can't be had.
    fn model_detail_material(
        &mut self,
        base_map: Handle<Image>,
        standard: &StandardMaterial,
        info: &ShaderInfo,
        assets: &mut LevelAssets,
    ) -> Option<Handle<HaloEnvironmentMaterial>> {
        let detail = info.model_detail?;
        let detail_map = self.textures.permuted(&self.cache, detail.map.datum?, self.permutation, true, assets.images)?;
        // (the mask, read as it is stored)
        let multipurpose = info
            .multipurpose_map
            .and_then(|m| m.datum)
            .and_then(|datum| self.textures.permuted(&self.cache, datum, self.permutation, false, assets.images))
            .unwrap_or_else(|| assets.neutral_detail.clone());
        // (the mesh's texture coordinates are the base map's, already scaled by
        // the shader's map scale; the detail's scale is against the model's own)
        let scale = Vec2::from(detail.scale) / Vec2::from(info.uv_scale).max(Vec2::splat(1e-4));
        Some(assets.environment_materials.add(HaloEnvironmentMaterial {
            base: standard.clone(),
            extension: HaloEnvironment {
                params: HaloEnvironmentParams {
                    detail_scales: scale.extend(1.0).extend(1.0),
                    micro_scale: Vec4::ONE,
                    modes: UVec4::new(detail.mask as u32, detail.function as u32, 0, crate::material::MODEL_DETAIL),
                    ..default()
                },
                base_map,
                primary_detail_map: detail_map,
                secondary_detail_map: multipurpose,
                micro_detail_map: assets.neutral_detail.clone(),
                bump_map: assets.flat_normal.clone(),
                self_illumination_map: assets.flat_normal.clone(),
                periodic_tables: crate::material::PERIODIC_TABLES.get().cloned().unwrap_or_default(),
            },
        }))
    }

    /// Glass and water as smooth, thin panes: Halo draws them in several
    /// passes (tint, reflection, ripples); this keeps their colour as the
    /// tint of what is seen through them, and a reflection. Solarik's thin
    /// glass is a blended material with full specular transmission; its
    /// alpha is coverage (zero a hole), so a pane is fully covered and
    /// tinted, and reflects by Fresnel.
    fn glass_or_water_material(&mut self, info: &ShaderInfo, assets: &mut LevelAssets) -> Option<Handle<StandardMaterial>> {
        let texture = |map: Option<TagRef>, this: &mut Self, assets: &mut LevelAssets| {
            map.and_then(|m| m.datum).and_then(|d| this.textures.permuted(&this.cache, d, this.permutation, true, assets.images))
        };
        let material = if let Some(glass) = info.glass {
            let [r, g, b] = glass.tint;
            StandardMaterial {
                base_color: Color::srgba(r, g, b, 1.0),
                base_color_texture: texture(glass.tint_map, self, assets),
                perceptual_roughness: 0.05,
                reflectance: 0.6,
                specular_transmission: 1.0,
                alpha_mode: AlphaMode::Blend,
                cull_mode: None,
                double_sided: true,
                ..default()
            }
        } else {
            let water = info.water?;
            let [r, g, b, _] = water.perpendicular_tint;
            StandardMaterial {
                base_color: Color::srgba(r, g, b, 1.0),
                perceptual_roughness: 0.08,
                reflectance: 0.5,
                specular_transmission: 1.0,
                alpha_mode: AlphaMode::Blend,
                cull_mode: None,
                double_sided: true,
                ..default()
            }
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
        let program = match halo_map::combiner::program(&self.cache, shader_ref) {
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
            let Some(image) = map.bitmap.datum.and_then(|d| self.textures.permuted(&self.cache, d, self.permutation, true, assets.images))
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
        let matte = Shininess { roughness: MATTE_ROUGHNESS, metallic: 0.0, reflectance: MATTE_REFLECTANCE, texture: None };
        let Some(reflection) = info.reflection.filter(|_| !self.matte) else { return matte };
        let r = reflection.perpendicular_brightness.max(reflection.parallel_brightness).clamp(0.0, 1.0);
        if r < 0.02 || reflection.cube_map.is_none() {
            return matte;
        }
        // Models (armour, weapons, vehicles) gloss the most: Halo gives them
        // a bright cube map whatever lies round them; here they reflect the
        // level itself, near mirror-like, tinted by their colour where their
        // mask says (metallic, which the mask scales; reflectance is the
        // whole material's, and raised it hazed the matte parts like dust).
        let model = info.shader_type == ShaderType::Model;
        let roughness = if model { 0.4 - 0.34 * r.sqrt() } else { 0.8 - 0.65 * r.sqrt() };
        let metallic = if model { 0.9 * r } else { 0.5 * r };
        // (some duller still)
        let shine = MODEL_SHINE.iter().find(|(path, _)| model && info.name.starts_with(path)).map_or(1.0, |&(_, shine)| shine);
        let dull = |roughness: f32| roughness + (DULL_ROUGHNESS.max(roughness) - roughness) * (1.0 - shine);
        let (roughness, metallic) = (dull(roughness), metallic * shine);
        let reflectance = 0.5;
        let mask = match reflection.mask {
            // an alpha-tested base map's alpha is its cut-out, not a mask
            SpecularMask::BaseAlpha if !info.alpha_tested => info.base_map.and_then(|b| b.datum).map(|d| (d, 3)),
            SpecularMask::MultipurposeRed => info.multipurpose_map.and_then(|m| m.datum).map(|d| (d, 0)),
            _ => None,
        };
        // Halo's mask dims a reflection, never blurs it: a model's unmasked
        // parts stay semi-glossy (fading them to matte while partly metallic
        // left Covenant crates looking dusty).
        let unmasked = if model { dull(MODEL_UNMASKED_ROUGHNESS) } else { MATTE_ROUGHNESS };
        let texture = mask.and_then(|(mask, channel)| {
            let bake = Bake::MetallicRoughness {
                mask,
                channel,
                unmasked: unmasked.to_bits(),
                roughness: roughness.to_bits(),
                metallic: metallic.to_bits(),
            };
            self.textures.baked(&self.cache, bake, self.permutation, assets.images)
        });
        match texture {
            // the texture holds the values; the factors multiply them
            Some(texture) => Shininess { roughness: 1.0, metallic: 1.0, reflectance, texture: Some(texture) },
            None => Shininess { roughness, metallic, reflectance, texture: None },
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
        let base_size = info.base_map.and_then(|b| b.datum).and_then(|d| bitmap_size(&self.cache, d));
        let mut layer = |map: Option<shader::DetailMap>| -> (Handle<Image>, Vec2) {
            let Some(map) = map else { return (assets.neutral_detail.clone(), Vec2::ONE) };
            let Some(datum) = map.bitmap.datum else { return (assets.neutral_detail.clone(), Vec2::ONE) };
            let image = self.textures.permuted(&self.cache, datum, self.permutation, true, assets.images);
            // rescaled detail maps repeat relative to the base map's size
            let rescale = match (environment.rescale_detail_maps, base_size, bitmap_size(&self.cache, datum)) {
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
        // the self-illumination and textures as they animate
        let mut params = HaloEnvironmentParams::default();
        let mut self_illumination_map = assets.flat_normal.clone();
        if let (Some(animation), Some(shader::SelfIllumination::Environment { map, .. })) =
            (info.self_illumination_animation, info.self_illumination)
            && let Some(image) = map.datum.and_then(|datum| self.textures.permuted(&self.cache, datum, self.permutation, false, assets.images))
        {
            self_illumination_map = image;
            flags |= crate::material::ENVIRONMENT_ANIMATED_ILLUMINATION;
            for channel in 0..3 {
                params.illumination_on[channel] = Vec3::from(animation.on_colors[channel]).extend(0.0);
                params.illumination_off[channel] = Vec3::from(animation.off_colors[channel]).extend(0.0);
            }
            let [primary, secondary, plasma] = animation.functions.map(|f| f.max(0) as u32);
            params.illumination_functions = UVec4::new(primary, secondary, plasma, 0);
            params.illumination_periods = Vec3::from(animation.periods).extend(1.0);
            params.illumination_phases = Vec3::from(animation.phases).extend(0.0);
            let band = halo_map::combiner::environment_plasma_band(crate::material::PLASMA_BAND_SAMPLES);
            for (index, samples) in band.chunks(4).enumerate() {
                params.plasma_band[index] = Vec4::from_slice(samples);
            }
        }
        if let Some(animation) = info.texture_animation {
            flags |= crate::material::ENVIRONMENT_TEXTURE_ANIMATION;
            params.texture_functions = UVec4::new(animation.functions[0].max(0) as u32, animation.functions[1].max(0) as u32, 0, 0);
            params.texture_animation = Vec4::new(animation.periods[0], animation.periods[1], animation.scales[0], animation.scales[1]);
        }
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
                    ..params
                },
                base_map,
                primary_detail_map: primary,
                secondary_detail_map: secondary,
                micro_detail_map: micro,
                bump_map,
                self_illumination_map,
                periodic_tables: crate::material::PERIODIC_TABLES.get().cloned().unwrap_or_default(),
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
        let image = self.textures.permuted(&self.cache, datum, self.permutation, false, images)?;
        let rescale = match (environment.rescale_bump_map, base_size, bitmap_size(&self.cache, datum)) {
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
    /// StandardMaterial's reflectance (0.5: the usual 4% head-on).
    reflectance: f32,
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

/// The share of the light a leaf lets through to its other side.
const FOLIAGE_TRANSMISSION: f32 = 0.4;

/// Whether a shader (by its tag name) is a plant's: trees, plants and
/// vines, which Halo keeps under `scenery\trees` and `scenery\plants`.
fn is_foliage(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    ["\\trees\\", "\\plants\\", "leaves", "leaf", "vine", "bush", "fern", "palm"].iter().any(|word| name.contains(word))
}

/// A point or spot light the emissive scale applies to: its power (lumens
/// per channel) at a scale of one.
#[derive(Component)]
pub struct LightSource {
    pub power: Vec3,
}

impl LightSource {
    /// The light's colour and intensity (lumens) at the emissive scale.
    pub fn color_and_intensity(&self, scale: f32) -> (Color, f32) {
        let peak = self.power.max_element();
        if peak <= 0.0 {
            return (Color::BLACK, 0.0);
        }
        let color = self.power / peak;
        (Color::linear_rgb(color.x, color.y, color.z), peak * scale)
    }
}

/// A light the size of a sphere of radius `radius` (metres): its lumens
/// per unit of the sphere's surface radiance (π r² candela, 4π lumens per
/// candela, as bevy_pbr counts for point and spot lights alike).
pub fn sphere_lumens_per_radiance(radius: f32) -> f32 {
    std::f32::consts::PI * radius * radius * 4.0 * std::f32::consts::PI
}

/// Keeps the sky centred on the camera.
/// A ray-traced mesh, kept so that it can leave the ray-traced scene while
/// hidden and come back: Solari traces every `RaytracingMesh3d`, visible or
/// not.
#[derive(Component)]
pub struct TracedMesh(pub Handle<Mesh>);

/// A placed object's BSPs (a bit per BSP index): shown only while one of
/// them is the active BSP (bridged).
#[derive(Component)]
pub struct OnBsps(pub u32);

/// Puts hidden ray-traced meshes out of the ray-traced scene, and shown ones
/// back.
pub fn sync_traced_visibility(
    mut commands: Commands,
    traced: Query<(Entity, &TracedMesh, &InheritedVisibility, Has<RaytracingMesh3d>), Changed<InheritedVisibility>>,
) {
    for (entity, mesh, visibility, in_scene) in &traced {
        if visibility.get() && !in_scene {
            commands.entity(entity).insert(RaytracingMesh3d(mesh.0.clone()));
        } else if !visibility.get() && in_scene {
            commands.entity(entity).remove::<RaytracingMesh3d>();
        }
    }
}

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
    build_mesh_oriented(vertices, indices, uv_scale, false)
}

/// [`build_mesh`] for a model's part, its triangles turned to face their
/// vertices' normals (see below).
fn build_model_mesh(vertices: impl Iterator<Item = SourceVertex>, indices: &[u32], uv_scale: [f32; 2]) -> Mesh {
    build_mesh_oriented(vertices, indices, uv_scale, true)
}

fn build_mesh_oriented(vertices: impl Iterator<Item = SourceVertex>, indices: &[u32], uv_scale: [f32; 2], orient: bool) -> Mesh {
    let (mut positions, mut normals, mut uvs, mut tangents) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    let mut sources = Vec::new();
    for v in vertices {
        sources.push((Vec3::from(v.position), Vec3::from(v.normal)));
        positions.push(halo_to_bevy(v.position, METERS_PER_WORLD_UNIT));
        normals.push(halo_to_bevy(v.normal, 1.0));
        uvs.push([v.texcoord[0] * uv_scale[0], v.texcoord[1] * uv_scale[1]]);
        let (n, t, b) = (Vec3::from(v.normal), Vec3::from(v.tangent), Vec3::from(v.binormal));
        let handedness = if n.cross(t).dot(b) < 0.0 { -1.0 } else { 1.0 };
        let [x, y, z] = halo_to_bevy(v.tangent, 1.0);
        tangents.push([x, y, z, handedness]);
    }
    // Halo's triangles are clockwise seen from the front (Direct3D);
    // Bevy's front faces are counter-clockwise. A few are wound against
    // their vertices' normals (a model's handful): the game culls them where
    // they face the camera and lights them by their normals where they face
    // away, where the rays' lighting finds them black. Turned to face their
    // normals (models only) where all three disagree, they fill their holes
    // and stay unseen from behind. Where only some do, they're seams whose
    // normals bend round an edge (the root of Johnson's cap bill, which the
    // game draws facing down): drawn from both sides, as turned they left a
    // notch seen from below.
    let mut wound = Vec::with_capacity(indices.len());
    for t in indices.chunks_exact(3) {
        let corner = |i: u32| sources.get(i as usize).copied().unwrap_or_default();
        let ((a, na), (b, nb), (c, nc)) = (corner(t[0]), corner(t[1]), corner(t[2]));
        // (clockwise from the front: the cross product points back)
        let back = (b - a).cross(c - a);
        let game = [t[0], t[2], t[1]];
        let turned = [t[0], t[1], t[2]];
        if !orient || back.dot(na + nb + nc) <= 0.0 {
            wound.extend(game);
        } else if [na, nb, nc].iter().all(|n| back.dot(*n) > 0.0) {
            wound.extend(turned);
        } else {
            wound.extend(game);
            wound.extend(turned);
        }
    }
    let indices = wound;

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
