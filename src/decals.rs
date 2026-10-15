//! The game's decals, drawn here (ROADMAP: effects in the viewer, step 2).
//! With `game.bevy_bridge_viewer_decals`, the game draws no decals into its
//! overlay: each frame it shares those it would have drawn
//! ([`halo_bridge::Decal`]: quads, bitmap, blend, colour, intensity). Each
//! becomes a mesh of its own, kept from frame to frame by its datum index,
//! drawn unlit over the lit, traced picture (the transparent pass, before
//! DLSS and the fog) with its framebuffer blend: bullet holes and blood
//! multiply the light on the surface as they would its colour; burns' glows
//! add radiance (`shaders/halo_decal.wesl`). Decals are not in the
//! ray-traced scene, so reflections don't show them.

use crate::bridge::{BridgeLevel, BridgeLink};
use bevy::{
    asset::{embedded_asset, RenderAssetUsages},
    mesh::{Indices, MeshVertexBufferLayoutRef, PrimitiveTopology},
    pbr::{MaterialPipeline, MaterialPipelineKey},
    platform::collections::HashMap,
    prelude::*,
    render::render_resource::{
        AsBindGroup, BlendComponent, BlendFactor, BlendOperation, BlendState, DepthBiasState, RenderPipelineDescriptor,
        ShaderType, SpecializedMeshPipelineError,
    },
    shader::ShaderRef,
};
use halo_bridge::{
    Decal, DECAL_BLEND_ADD, DECAL_BLEND_ALPHA, DECAL_BLEND_ALPHA_MULTIPLY_ADD, DECAL_BLEND_DOUBLE_MULTIPLY,
    DECAL_BLEND_MAX, DECAL_BLEND_MIN, DECAL_BLEND_MULTIPLY, DECAL_BLEND_REVERSE_SUBTRACT,
};
use halo_map::math::{halo_to_bevy, METERS_PER_WORLD_UNIT};

pub struct DecalsPlugin;

impl Plugin for DecalsPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "shaders/halo_decal.wesl");
        app.add_plugins(MaterialPlugin::<DecalMaterial>::default())
            .init_resource::<Decals>()
            .add_systems(Update, follow_decals);
    }
}

#[derive(ShaderType, Clone, Copy, Debug, Default, PartialEq)]
struct DecalParams {
    /// the decal's colour (display-encoded), its intensity
    color: Vec4,
    /// framebuffer blend function, radiance a displayed 1 is, 1 to cut away
    /// below half the bitmap's alpha
    mode: Vec4,
}

#[derive(Asset, AsBindGroup, TypePath, Debug, Clone)]
#[bind_group_data(DecalKey)]
pub struct DecalMaterial {
    #[uniform(0)]
    params: DecalParams,
    #[texture(1)]
    #[sampler(2)]
    map: Handle<Image>,
    blend: u8,
}

/// The pipeline a decal needs: its blend.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct DecalKey {
    blend: u8,
}

impl From<&DecalMaterial> for DecalKey {
    fn from(material: &DecalMaterial) -> Self {
        DecalKey { blend: material.blend }
    }
}

impl Material for DecalMaterial {
    fn fragment_shader() -> ShaderRef {
        "embedded://halo_rt/shaders/halo_decal.wesl".into()
    }

    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Blend
    }

    fn enable_prepass() -> bool {
        false
    }

    fn enable_shadows() -> bool {
        false
    }

    fn specialize(
        _pipeline: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayoutRef,
        key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        // (either side: the game's quads face either way)
        descriptor.primitive.cull_mode = None;
        // drawn on the surface, in front of it (the game's z bias)
        if let Some(depth) = descriptor.depth_stencil.as_mut() {
            depth.depth_write_enabled = Some(false);
            depth.bias = DepthBiasState { constant: 8, slope_scale: 2.0, clamp: 0.0 };
            // (HALO_RT_DECAL_DEBUG=2: seen through everything)
            if std::env::var("HALO_RT_DECAL_DEBUG").is_ok_and(|v| v == "2") {
                depth.depth_compare = Some(bevy::render::render_resource::CompareFunction::Always);
            }
        }
        let component = |src_factor, dst_factor, operation| BlendComponent { src_factor, dst_factor, operation };
        let keep_alpha = component(BlendFactor::Zero, BlendFactor::One, BlendOperation::Add);
        let color = match key.bind_group_data.blend {
            // dst * f (an alpha blend multiplies toward its colour)
            DECAL_BLEND_MULTIPLY | DECAL_BLEND_DOUBLE_MULTIPLY | DECAL_BLEND_ALPHA => {
                component(BlendFactor::Dst, BlendFactor::Zero, BlendOperation::Add)
            }
            DECAL_BLEND_ADD => component(BlendFactor::One, BlendFactor::One, BlendOperation::Add),
            DECAL_BLEND_REVERSE_SUBTRACT => component(BlendFactor::One, BlendFactor::One, BlendOperation::ReverseSubtract),
            DECAL_BLEND_MIN => component(BlendFactor::One, BlendFactor::One, BlendOperation::Min),
            DECAL_BLEND_MAX => component(BlendFactor::One, BlendFactor::One, BlendOperation::Max),
            DECAL_BLEND_ALPHA_MULTIPLY_ADD => component(BlendFactor::One, BlendFactor::OneMinusSrcAlpha, BlendOperation::Add),
            _ => component(BlendFactor::Zero, BlendFactor::One, BlendOperation::Add),
        };
        if let Some(fragment) = descriptor.fragment.as_mut() {
            for target in fragment.targets.iter_mut().flatten() {
                target.blend = Some(BlendState { color, alpha: keep_alpha });
            }
        }
        Ok(())
    }
}

/// The decals made, by datum index, for the level shown.
#[derive(Resource, Default)]
struct Decals {
    scenario: String,
    tracked: HashMap<u32, Tracked>,
    /// Counts the game's frames seen, for forgetting decals long unseen.
    frame: u64,
    last_frame_index: u32,
    /// How many the game shared last (for the log).
    shared: usize,
    radiance_scale: f32,
}

struct Tracked {
    entity: Entity,
    material: Handle<DecalMaterial>,
    /// What makes it the same decal: a datum index the game reuses for a
    /// new decal gets new geometry
    shape: (u32, u8, u8, u32, [i32; 3]),
    params: DecalParams,
    last_seen: u64,
    shown: bool,
}

/// A decal unseen for this many of the game's frames (out of view, or gone)
/// is let go of; one seen again is made again.
const FORGET_AFTER_FRAMES: u64 = 600;

impl Tracked {
    fn shape(decal: &Decal, vertices: &[halo_bridge::DecalVertex]) -> (u32, u8, u8, u32, [i32; 3]) {
        let first = vertices.first().map_or([0; 3], |v| v.position.map(|x| (x * 1024.0) as i32));
        (decal.map, decal.bitmap_index, decal.blend_function, decal.quad_count, first)
    }
}

fn decal_params(decal: &Decal, radiance_scale: f32) -> DecalParams {
    let [r, g, b, _] = decal.color;
    DecalParams {
        color: Vec4::new(r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, decal.intensity as f32 / 255.0),
        mode: Vec4::new(
            decal.blend_function as f32,
            radiance_scale,
            if decal.layer == 3 { 1.0 } else { 0.0 },
            // (HALO_RT_DECAL_DEBUG: each decal in magenta, to find them)
            if std::env::var_os("HALO_RT_DECAL_DEBUG").is_some() { 1.0 } else { 0.0 },
        ),
    }
}

/// A decal's quads as a mesh, in this world's terms.
fn decal_mesh(vertices: &[halo_bridge::DecalVertex]) -> Mesh {
    let positions: Vec<[f32; 3]> = vertices.iter().map(|v| halo_to_bevy(v.position, METERS_PER_WORLD_UNIT)).collect();
    let uvs: Vec<[f32; 2]> =
        vertices.iter().map(|v| [v.texcoord[0] as f32 / 32768.0, v.texcoord[1] as f32 / 32768.0]).collect();
    let indices: Vec<u32> = (0..vertices.len() as u32 / 4)
        .flat_map(|quad| {
            let first = quad * 4;
            [first, first + 1, first + 2, first, first + 2, first + 3]
        })
        .collect();
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD)
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
        .with_inserted_indices(Indices::U32(indices))
}

/// The latest frame's decals: new ones made, those still drawn shown (their
/// intensity and colour as they fade), the rest hidden, and in time let go.
#[allow(clippy::too_many_arguments)]
fn follow_decals(
    mut commands: Commands,
    link: Res<BridgeLink>,
    mut decals: ResMut<Decals>,
    mut level: Option<ResMut<BridgeLevel>>,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<DecalMaterial>>,
    exposure: Single<&bevy::camera::Exposure, With<Camera3d>>,
) {
    let Some(snapshot) = link.latest.as_ref() else { return };
    if snapshot.frame_index == decals.last_frame_index {
        return;
    }
    decals.last_frame_index = snapshot.frame_index;
    // a new level: none of the old decals
    if snapshot.scenario_name != decals.scenario {
        for (_, tracked) in decals.tracked.drain() {
            commands.entity(tracked.entity).despawn();
        }
        decals.scenario = snapshot.scenario_name.clone();
    }
    decals.frame += 1;
    let frame = decals.frame;
    // (how many the game shares, in the log as that changes)
    if snapshot.decals.len() != decals.shared {
        decals.shared = snapshot.decals.len();
        info!(
            "the game's decals: {} ({} vertices) at tick {}, the camera at {:?}",
            snapshot.decals.len(),
            snapshot.decal_vertices.len(),
            snapshot.game_tick,
            snapshot.camera.map(|c| halo_to_bevy(c.position, METERS_PER_WORLD_UNIT))
        );
    }
    // (what shows as a displayed 1 at the camera's exposure)
    let radiance_scale = 1.0 / exposure.exposure().max(1e-9);
    let rescale = (radiance_scale - decals.radiance_scale).abs() > radiance_scale * 1e-3;
    decals.radiance_scale = radiance_scale;

    for decal in &snapshot.decals {
        let first = decal.first_vertex as usize;
        let Some(vertices) = snapshot.decal_vertices.get(first..first + decal.quad_count as usize * 4) else { continue };
        let shape = Tracked::shape(decal, vertices);
        let params = decal_params(decal, radiance_scale);
        if let Some(tracked) = decals.tracked.get_mut(&decal.decal_index).filter(|t| t.shape == shape) {
            tracked.last_seen = frame;
            if !tracked.shown {
                commands.entity(tracked.entity).insert(Visibility::Inherited);
                tracked.shown = true;
            }
            if tracked.params != params {
                if let Some(mut material) = materials.get_mut(&tracked.material) {
                    material.params = params;
                }
                tracked.params = params;
            }
            continue;
        }
        if let Some(old) = decals.tracked.remove(&decal.decal_index) {
            commands.entity(old.entity).despawn();
        }
        // its bitmap, from the level's map (display-encoded)
        let Some(level) = level.as_deref_mut() else { continue };
        let builder = &mut level.builder;
        let Some(map) = builder.textures.get(&builder.cache, decal.map, decal.bitmap_index as usize, true, &mut images)
        else {
            continue;
        };
        // (HALO_RT_DECAL_LOG: each decal made, and where, in the log)
        if std::env::var_os("HALO_RT_DECAL_LOG").is_some() {
            let name = builder.cache.tag(decal.map).map(|t| t.name.as_str()).unwrap_or("?");
            let at = halo_to_bevy(vertices[0].position, METERS_PER_WORLD_UNIT);
            info!(
                "decal {:#010x}: {name}[{}] blend {} layer {} colour {:?} intensity {} quads {} at {at:?}",
                decal.decal_index, decal.bitmap_index, decal.blend_function, decal.layer, decal.color, decal.intensity, decal.quad_count
            );
        }
        let material = materials.add(DecalMaterial { params, map, blend: decal.blend_function });
        let entity = commands
            .spawn((
                Name::new("Game decal"),
                Mesh3d(meshes.add(decal_mesh(vertices))),
                MeshMaterial3d(material.clone()),
                // (its vertices are where they are in the world)
                Transform::IDENTITY,
            ))
            .id();
        decals.tracked.insert(decal.decal_index, Tracked { entity, material, shape, params, last_seen: frame, shown: true });
    }

    decals.tracked.retain(|_, tracked| {
        if frame - tracked.last_seen > FORGET_AFTER_FRAMES {
            commands.entity(tracked.entity).despawn();
            return false;
        }
        if tracked.last_seen != frame && tracked.shown {
            commands.entity(tracked.entity).insert(Visibility::Hidden);
            tracked.shown = false;
        }
        true
    });
    if rescale {
        for tracked in decals.tracked.values_mut() {
            tracked.params.mode.y = radiance_scale;
            if let Some(mut material) = materials.get_mut(&tracked.material) {
                material.params.mode.y = radiance_scale;
            }
        }
    }
}
