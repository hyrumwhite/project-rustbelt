//! The game's sprites, drawn here (ROADMAP: effects in the viewer, step 3).
//! With `game.bevy_bridge_viewer_effects`, the game draws no sprites into
//! its overlay (particles, particle systems, weather, glows, antennas: all
//! of `build_sprites_end`'s): each frame it shares the batches it would
//! have drawn ([`halo_bridge::SpriteBatch`]). Each frame's batches are
//! meshes in a pool, each at the point the game sorts the batch from, so
//! the transparent pass draws them in the game's order over the lit, traced
//! picture (before DLSS and the fog), with the effect shader's colour and
//! framebuffer blend (`shaders/halo_sprite.wesl`). Unlit, as the game's
//! effects are.
//!
//! The same batches carry contrails and lightning, as quads. Lens flares'
//! reflections come with them too, but are drawn after tonemapping, in the
//! display's encoding (`src/lens_flares.rs`), so they're left out here.

use crate::bridge::{BridgeLevel, BridgeLink};
use bevy::{
    asset::{embedded_asset, RenderAssetUsages},
    mesh::{Indices, MeshVertexBufferLayoutRef, PrimitiveTopology},
    pbr::{MaterialPipeline, MaterialPipelineKey},
    platform::collections::HashMap,
    prelude::*,
    render::render_resource::{
        AsBindGroup, BlendComponent, BlendFactor, BlendOperation, BlendState, RenderPipelineDescriptor, ShaderType,
        SpecializedMeshPipelineError,
    },
    shader::ShaderRef,
};
use halo_bridge::{
    SpriteBatch, DECAL_BLEND_ADD, DECAL_BLEND_ALPHA, DECAL_BLEND_ALPHA_MULTIPLY_ADD, DECAL_BLEND_DOUBLE_MULTIPLY,
    DECAL_BLEND_MAX, DECAL_BLEND_MIN, DECAL_BLEND_MULTIPLY, DECAL_BLEND_REVERSE_SUBTRACT, SPRITE_LENS_FLARE,
    SPRITE_NONLINEAR_TINT,
};
use halo_map::math::{halo_to_bevy, METERS_PER_WORLD_UNIT};

pub struct EffectsPlugin;

impl Plugin for EffectsPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "shaders/halo_sprite.wesl");
        app.add_plugins(MaterialPlugin::<SpriteMaterial>::default())
            .init_resource::<Sprites>()
            .add_systems(Update, follow_sprites);
    }
}

#[derive(ShaderType, Clone, Copy, Debug, Default, PartialEq)]
struct SpriteParams {
    /// framebuffer blend function, what a displayed 1 is written as, 1 for
    /// nonlinear tint, 1 to show it in cyan
    mode: Vec4,
    /// a zsprite's radius (metres)
    zsprite: Vec4,
    /// the game's environment fog ([`crate::fog::HaloFog::layers`]): the
    /// sprite fades by its own, as the game's effects do
    fog: [Vec4; 5],
}

#[derive(Asset, AsBindGroup, TypePath, Debug, Clone)]
#[bind_group_data(SpriteKey)]
pub struct SpriteMaterial {
    #[uniform(0)]
    params: SpriteParams,
    #[texture(1)]
    #[sampler(2)]
    map: Handle<Image>,
    /// a zsprite's z-map (its secondary map): each pixel's depth brought
    /// toward the camera by its height
    #[texture(3)]
    #[sampler(4)]
    zmap: Option<Handle<Image>>,
    blend: u16,
}

/// The pipeline a batch needs: its blend, and whether it's a zsprite.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct SpriteKey {
    blend: u16,
    zsprite: bool,
}

impl From<&SpriteMaterial> for SpriteKey {
    fn from(material: &SpriteMaterial) -> Self {
        SpriteKey { blend: material.blend, zsprite: material.zmap.is_some() }
    }
}

impl Material for SpriteMaterial {
    fn fragment_shader() -> ShaderRef {
        "embedded://halo_rt/shaders/halo_sprite.wesl".into()
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
        // (the effect shader draws both sides)
        descriptor.primitive.cull_mode = None;
        if key.bind_group_data.zsprite
            && let Some(fragment) = descriptor.fragment.as_mut()
        {
            fragment.shader_defs.push("ZSPRITE".into());
        }
        if let Some(depth) = descriptor.depth_stencil.as_mut() {
            depth.depth_write_enabled = Some(false);
            // (HALO_RT_SPRITE_DEBUG=2: seen through everything)
            if std::env::var("HALO_RT_SPRITE_DEBUG").is_ok_and(|v| v == "2") {
                depth.depth_compare = Some(bevy::render::render_resource::CompareFunction::Always);
            }
        }
        let component = |src_factor, dst_factor, operation| BlendComponent { src_factor, dst_factor, operation };
        let keep_alpha = component(BlendFactor::Zero, BlendFactor::One, BlendOperation::Add);
        let color = match key.bind_group_data.blend as u8 {
            DECAL_BLEND_ALPHA => component(BlendFactor::SrcAlpha, BlendFactor::OneMinusSrcAlpha, BlendOperation::Add),
            DECAL_BLEND_MULTIPLY | DECAL_BLEND_DOUBLE_MULTIPLY => {
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

/// What a batch's material is made of.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct MaterialKey {
    map: u32,
    bitmap_index: u16,
    blend: u16,
    nonlinear_tint: bool,
    /// a zsprite's z-map and radius (its bits; none: `u32::MAX`, 0)
    zmap: u32,
    zsprite_radius: u32,
}

/// The sprite meshes (one a batch, reused frame to frame) and materials.
#[derive(Resource, Default)]
struct Sprites {
    scenario: String,
    pool: Vec<(Entity, Handle<Mesh>, bool)>,
    materials: HashMap<MaterialKey, Option<Handle<SpriteMaterial>>>,
    last_frame_index: u32,
    radiance_scale: f32,
    /// The fog the materials have.
    fog: [Vec4; 5],
    /// How many batches the game shared last (for the log).
    shared: usize,
}

/// A batch's quads as a mesh about its centroid.
fn batch_mesh(vertices: &[halo_bridge::SpriteVertex], centroid: Vec3) -> Mesh {
    let positions: Vec<[f32; 3]> = vertices
        .iter()
        .map(|v| (Vec3::from(halo_to_bevy(v.position, METERS_PER_WORLD_UNIT)) - centroid).to_array())
        .collect();
    let uvs: Vec<[f32; 2]> = vertices.iter().map(|v| v.texcoord).collect();
    // each quad's corners, as the game's zsprite texture coordinate stream
    // gives them (a z-map spans its quad)
    const CORNERS: [[f32; 2]; 4] = [[0.0, 0.0], [0.0, 1.0], [1.0, 1.0], [1.0, 0.0]];
    let corners: Vec<[f32; 2]> = (0..vertices.len()).map(|i| CORNERS[i % 4]).collect();
    // (a8r8g8b8, display-encoded: the shader takes them as they are)
    let colors: Vec<[f32; 4]> = vertices
        .iter()
        .map(|v| {
            let c = v.color;
            [((c >> 16) & 255) as f32 / 255.0, ((c >> 8) & 255) as f32 / 255.0, (c & 255) as f32 / 255.0, (c >> 24) as f32 / 255.0]
        })
        .collect();
    let indices: Vec<u32> = (0..vertices.len() as u32 / 4)
        .flat_map(|quad| {
            let first = quad * 4;
            [first, first + 1, first + 2, first, first + 2, first + 3]
        })
        .collect();
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD)
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_1, corners)
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, colors)
        .with_inserted_indices(Indices::U32(indices))
}

/// The latest frame's sprites into the pool's meshes; the rest hidden.
#[allow(clippy::too_many_arguments)]
fn follow_sprites(
    mut commands: Commands,
    link: Res<BridgeLink>,
    mut sprites: ResMut<Sprites>,
    mut level: Option<ResMut<BridgeLevel>>,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<SpriteMaterial>>,
    camera_fog: Query<&crate::fog::HaloFog, With<Camera3d>>,
) {
    // the camera's fog onto every material (`HALO_RT_NO_SPRITE_FOG`: none, the
    // fog pass alone fogging them by what lies behind)
    let fog = camera_fog
        .iter()
        .next()
        .filter(|_| std::env::var_os("HALO_RT_NO_SPRITE_FOG").is_none())
        .map_or([Vec4::ZERO; 5], |fog| fog.layers());
    if fog != sprites.fog {
        sprites.fog = fog;
        for material in sprites.materials.values().flatten() {
            if let Some(mut material) = materials.get_mut(material) {
                material.params.fog = fog;
            }
        }
    }
    let Some(snapshot) = link.latest.as_ref() else { return };
    if snapshot.frame_index == sprites.last_frame_index {
        return;
    }
    sprites.last_frame_index = snapshot.frame_index;
    // a new level: its own bitmaps
    if snapshot.scenario_name != sprites.scenario {
        sprites.materials.clear();
        sprites.scenario = snapshot.scenario_name.clone();
    }
    if snapshot.sprite_batches.len() != sprites.shared {
        sprites.shared = snapshot.sprite_batches.len();
        if sprites.shared == 0 || std::env::var_os("HALO_RT_SPRITE_LOG").is_some() {
            info!(
                "the game's sprites: {} batches ({} vertices, {} lens flares) at tick {}",
                snapshot.sprite_batches.len(),
                snapshot.sprite_vertices.len(),
                snapshot.sprite_batches.iter().filter(|b| b.kind == SPRITE_LENS_FLARE).count(),
                snapshot.game_tick
            );
        }
    }
    // (a colour written as it is shows as it is: Bevy's own materials apply
    // the camera's exposure in their shaders, and these write past them)
    let radiance_scale = 1.0;
    if (radiance_scale - sprites.radiance_scale).abs() > radiance_scale * 1e-3 {
        sprites.radiance_scale = radiance_scale;
        for material in sprites.materials.values().flatten() {
            if let Some(mut material) = materials.get_mut(material) {
                material.params.mode.y = radiance_scale;
            }
        }
    }
    // (HALO_RT_SPRITE_DEBUG=3: each sprite as its fog, red its own coverage,
    // green the raise for what lies behind)
    let debug = match std::env::var("HALO_RT_SPRITE_DEBUG").as_deref() {
        Ok("1") => 1.0,
        Ok("3") => 3.0,
        _ => 0.0,
    };

    let mut used = 0;
    for batch in snapshot.sprite_batches.iter().filter(|b| b.kind != SPRITE_LENS_FLARE) {
        let SpriteBatch { map, bitmap_index, blend_function, shader_flags, centroid, first_vertex, quad_count, .. } =
            *batch;
        let first = first_vertex as usize;
        let Some(vertices) = snapshot.sprite_vertices.get(first..first + quad_count as usize * 4) else { continue };
        // (a zsprite: its z-map lifts its pixels toward the camera; the game
        // leaves it out for the first-person weapon's, and HALO_RT_NO_ZSPRITES
        // here)
        let zsprite = batch.secondary_anchor == 2
            && batch.secondary_map != u32::MAX
            && batch.secondary_radius > 0.0
            && batch.first_person == 0
            && std::env::var_os("HALO_RT_NO_ZSPRITES").is_none();
        let key = MaterialKey {
            map,
            bitmap_index,
            blend: blend_function,
            nonlinear_tint: shader_flags & SPRITE_NONLINEAR_TINT != 0,
            zmap: if zsprite { batch.secondary_map } else { u32::MAX },
            zsprite_radius: if zsprite { batch.secondary_radius.to_bits() } else { 0 },
        };
        let material = match sprites.materials.get(&key) {
            Some(material) => material.clone(),
            None => {
                // (HALO_RT_SPRITE_LOG: each kind of batch, in the log)
                if std::env::var_os("HALO_RT_SPRITE_LOG").is_some() {
                    let name = |datum: u32| {
                        level.as_deref().and_then(|level| level.builder.cache.tag(datum).ok()).map_or("-".to_string(), |t| t.name.clone())
                    };
                    info!(
                        "sprite batch: {}[{}] blend {} flags {:#x} map flags {:#x} first person {} secondary {} anchor {} quads {} centroid {:?} first vertex {:?} {:?} camera {:?}",
                        name(map), bitmap_index, blend_function, shader_flags, batch.map_flags, batch.first_person,
                        name(batch.secondary_map), batch.secondary_anchor, quad_count, batch.centroid,
                        vertices.first().map(|v| v.position), vertices.get(2).map(|v| v.position),
                        snapshot.camera.map(|c| c.position)
                    );
                }
                // its bitmap, from the level's map (display-encoded)
                let material = level.as_deref_mut().and_then(|level| {
                    let builder = &mut level.builder;
                    let map = builder.textures.get(&builder.cache, map, bitmap_index as usize, true, &mut images)?;
                    // (the z-map's heights are data, not colours)
                    let zmap = (key.zmap != u32::MAX)
                        .then(|| builder.textures.get(&builder.cache, key.zmap, 0, false, &mut images))
                        .flatten();
                    Some((map, zmap))
                }).map(|(map, zmap)| {
                    let radius = f32::from_bits(key.zsprite_radius) * METERS_PER_WORLD_UNIT;
                    materials.add(SpriteMaterial {
                        params: SpriteParams {
                            mode: Vec4::new(blend_function as f32, radiance_scale, if key.nonlinear_tint { 1.0 } else { 0.0 }, debug),
                            zsprite: Vec4::new(radius, 0.0, 0.0, 0.0),
                            fog: sprites.fog,
                        },
                        map,
                        zmap,
                        blend: blend_function,
                    })
                });
                sprites.materials.insert(key, material.clone());
                material
            }
        };
        let Some(material) = material else { continue };
        // (HALO_RT_SPRITE_LOG: batches near the camera or huge, in the log)
        if std::env::var_os("HALO_RT_SPRITE_LOG").is_some() {
            if let Some(camera) = snapshot.camera {
                let camera = Vec3::from(camera.position);
                let nearest = vertices.iter().map(|v| Vec3::from(v.position).distance(camera)).fold(f32::MAX, f32::min);
                let largest = vertices.chunks(4).map(|q| Vec3::from(q[0].position).distance(Vec3::from(q[2].position))).fold(0.0, f32::max);
                if nearest < 5.0 || largest > 10.0 {
                    let colors: Vec<String> = vertices.iter().step_by(4).take(6).map(|v| format!("{:08x}", v.color)).collect();
                    info!("sprite batch {map:#010x}[{bitmap_index}] blend {blend_function} quads {quad_count}: nearest {nearest:.2}, largest {largest:.2}, first person {}, colours {colors:?} at tick {}", batch.first_person, snapshot.game_tick);
                }
            }
        }
        let centroid = Vec3::from(halo_to_bevy(centroid, METERS_PER_WORLD_UNIT));
        let mesh = batch_mesh(vertices, centroid);
        if used == sprites.pool.len() {
            let handle = meshes.add(mesh);
            let entity = commands
                .spawn((
                    Name::new("Game sprites"),
                    // (its mesh changes every frame; its bounds, once made, wouldn't)
                    bevy::camera::visibility::NoFrustumCulling,
                    Mesh3d(handle.clone()),
                    MeshMaterial3d(material),
                    Transform::from_translation(centroid),
                ))
                .id();
            sprites.pool.push((entity, handle, true));
        } else {
            let (entity, handle, shown) = &mut sprites.pool[used];
            meshes.insert(handle.id(), mesh).ok();
            commands.entity(*entity).insert((MeshMaterial3d(material), Transform::from_translation(centroid), Visibility::Inherited));
            *shown = true;
        }
        used += 1;
    }
    for (entity, _, shown) in sprites.pool.iter_mut().skip(used) {
        if *shown {
            commands.entity(*entity).insert(Visibility::Hidden);
            *shown = false;
        }
    }
}
