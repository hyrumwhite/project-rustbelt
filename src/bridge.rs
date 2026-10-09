//! `--bridge`: draw a running OpenCE game (ROADMAP phase 4).
//!
//! OpenCE, built from its `bevy-bridge` branch and started with
//! `HALO_BEVY_BRIDGE=1`, shares each frame's camera and every object's node
//! matrices ([`halo_bridge`]). The level's BSPs, sky and lights load from the
//! map file as usual; the objects come from the game: spawned when they
//! appear, posed by their node matrices every frame, despawned when they go.
//! The camera follows the game's (F detaches it into a free camera).

use crate::level::{halo_affine, halo_to_bevy_affine, AnimatedModel, LevelAssets, LevelBuilder};
use crate::material::{HaloEnvironmentMaterial, HaloTransparentMaterial, NeutralDetail};
use bevy::{
    camera_controller::free_camera::FreeCamera,
    math::Affine3A,
    platform::collections::HashMap,
    prelude::*,
};
use halo_bridge::{Bridge, Snapshot, OBJECT_HIDDEN};
use std::{sync::Arc, time::Duration};

/// Waits for a running game to share a frame, so the viewer knows which map
/// to load. `None` after `timeout`.
pub fn wait_for_game(timeout: Duration) -> Option<(Bridge, Snapshot)> {
    let start = std::time::Instant::now();
    let mut announced = false;
    while start.elapsed() < timeout {
        match Bridge::open() {
            Ok(mut bridge) => {
                if let Some(snapshot) = bridge.read()
                    && !snapshot.scenario_name.is_empty()
                {
                    return Some((bridge, snapshot));
                }
            }
            Err(e) if !announced => {
                eprintln!("waiting for OpenCE: {e}");
                announced = true;
            }
            Err(_) => {}
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    None
}

pub struct BridgePlugin;

impl Plugin for BridgePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<BridgeObjects>()
            .init_resource::<FollowGame>()
            .add_systems(First, read_frame)
            .add_systems(Update, (toggle_follow, sync_objects, sync_camera).chain());
    }
}

/// The open bridge and the latest frame read from it.
#[derive(Resource)]
pub struct BridgeLink {
    pub bridge: Bridge,
    pub latest: Option<Snapshot>,
    /// Frames read since the last overlay update (for its rate).
    pub frames: u32,
}

/// The level builder, kept to make objects' meshes as the game creates them.
#[derive(Resource)]
pub struct BridgeLevel {
    pub builder: LevelBuilder,
    pub raster: bool,
}

/// Whether the camera follows the game's.
#[derive(Resource)]
pub struct FollowGame(pub bool);

impl Default for FollowGame {
    fn default() -> Self {
        FollowGame(true)
    }
}

struct Tracked {
    root: Entity,
    definition: u32,
    permutations: [u8; 8],
    model: Arc<AnimatedModel>,
    nodes: Vec<Option<Entity>>,
}

/// The game's objects drawn so far, by object datum index.
#[derive(Resource, Default)]
pub struct BridgeObjects {
    tracked: HashMap<u32, Tracked>,
    /// Definitions without a model (nothing to draw).
    modelless: HashMap<(u32, [u8; 8]), ()>,
}

impl BridgeObjects {
    pub fn count(&self) -> usize {
        self.tracked.len()
    }
}

fn read_frame(mut link: ResMut<BridgeLink>) {
    // only the newest frame matters; older ones were already superseded
    while let Some(snapshot) = link.bridge.read() {
        link.latest = Some(snapshot);
        link.frames += 1;
    }
}

/// F: follow the game's camera, or fly free.
fn toggle_follow(
    keys: Res<ButtonInput<KeyCode>>,
    mut follow: ResMut<FollowGame>,
    camera: Single<Entity, With<Camera3d>>,
    mut commands: Commands,
) {
    if !keys.just_pressed(KeyCode::KeyF) {
        return;
    }
    follow.0 = !follow.0;
    if follow.0 {
        commands.entity(*camera).remove::<FreeCamera>();
    } else {
        commands.entity(*camera).insert(FreeCamera { walk_speed: 6.0, run_speed: 25.0, ..default() });
    }
}

fn sync_objects(
    link: Res<BridgeLink>,
    mut level: ResMut<BridgeLevel>,
    mut objects: ResMut<BridgeObjects>,
    mut commands: Commands,
    mut transforms: Query<&mut Transform>,
    mut visibility: Query<&mut Visibility>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut environment_materials: ResMut<Assets<HaloEnvironmentMaterial>>,
    mut transparent_materials: ResMut<Assets<HaloTransparentMaterial>>,
    mut images: ResMut<Assets<Image>>,
    neutral_detail: Res<NeutralDetail>,
) {
    let Some(snapshot) = &link.latest else { return };
    let to_bevy = halo_to_bevy_affine();
    let from_bevy = to_bevy.inverse();
    let mut assets = LevelAssets {
        meshes: &mut meshes,
        materials: &mut materials,
        environment_materials: &mut environment_materials,
        transparent_materials: &mut transparent_materials,
        images: &mut images,
        neutral_detail: neutral_detail.0.clone(),
        flat_normal: Handle::default(),
    };
    assets.flat_normal = assets.images.add(crate::material::flat_normal_image());
    let BridgeLevel { builder, raster } = &mut *level;
    let raster = *raster;

    let mut seen = HashMap::<u32, ()>::default();
    for object in &snapshot.objects {
        seen.insert(object.object_index, ());
        // an object whose definition or permutations changed is made again
        let current = objects.tracked.get(&object.object_index);
        if current.is_some_and(|t| t.definition != object.definition_index || t.permutations != object.region_permutations) {
            let old = objects.tracked.remove(&object.object_index).unwrap();
            commands.entity(old.root).despawn();
        }
        if !objects.tracked.contains_key(&object.object_index) {
            let key = (object.definition_index, object.region_permutations);
            if objects.modelless.contains_key(&key) {
                continue;
            }
            let definition = halo_map::TagRef { group: 0, datum: Some(object.definition_index) };
            let model = halo_map::objects::object_model(&builder.cache, definition)
                .ok()
                .flatten()
                .and_then(|m| m.datum)
                .and_then(|m| builder.animated_model(m, object.region_permutations, &mut assets));
            let Some(model) = model else {
                objects.modelless.insert(key, ());
                continue;
            };
            let (root, nodes) = builder.spawn_animated_model(&mut commands, &model, raster);
            objects.tracked.insert(
                object.object_index,
                Tracked {
                    root,
                    definition: object.definition_index,
                    permutations: object.region_permutations,
                    model,
                    nodes,
                },
            );
            // newly spawned entities take their transforms next frame
            continue;
        }
        let tracked = &objects.tracked[&object.object_index];
        if let Ok(mut v) = visibility.get_mut(tracked.root) {
            let wanted = if object.flags & OBJECT_HIDDEN != 0 { Visibility::Hidden } else { Visibility::Inherited };
            if *v != wanted {
                *v = wanted;
            }
        }
        let nodes = snapshot.object_nodes(object);
        for (index, entity) in tracked.nodes.iter().enumerate() {
            let (Some(entity), Some(node), Some(default_inverse)) =
                (entity, nodes.get(index), tracked.model.default_inverses.get(index))
            else {
                continue;
            };
            if let Ok(mut transform) = transforms.get_mut(*entity) {
                *transform = node_transform(node, default_inverse, &to_bevy, &from_bevy);
            }
        }
    }

    // the game deleted these
    objects.tracked.retain(|index, tracked| {
        let keep = seen.contains_key(index);
        if !keep {
            commands.entity(tracked.root).despawn();
        }
        keep
    });
}

/// A node's draw transform in Bevy space: the game's node matrix times the
/// node's default inverse (model space to world), conjugated by the Halo to
/// Bevy change of space (the meshes are already in Bevy space).
fn node_transform(node: &halo_bridge::Node, default_inverse: &Affine3A, to_bevy: &Affine3A, from_bevy: &Affine3A) -> Transform {
    let world = halo_affine(&halo_map::model::Matrix4x3 {
        scale: node.scale,
        forward: node.forward,
        left: node.left,
        up: node.up,
        position: node.position,
    });
    Transform::from_matrix(Mat4::from(*to_bevy * world * *default_inverse * *from_bevy))
}

fn sync_camera(
    link: Res<BridgeLink>,
    follow: Res<FollowGame>,
    camera: Single<(&mut Transform, &mut Projection), With<Camera3d>>,
) {
    if !follow.0 {
        return;
    }
    let Some(game) = link.latest.as_ref().and_then(|s| s.camera) else { return };
    let (mut transform, mut projection) = camera.into_inner();
    let position = Vec3::from(halo_map::math::halo_to_bevy(game.position, halo_map::math::METERS_PER_WORLD_UNIT));
    let forward = Vec3::from(halo_map::math::halo_to_bevy(game.forward, 1.0));
    let up = Vec3::from(halo_map::math::halo_to_bevy(game.up, 1.0));
    *transform = Transform::from_translation(position).looking_to(forward, up);
    if let Projection::Perspective(perspective) = &mut *projection
        && game.vertical_field_of_view > 0.0
    {
        perspective.fov = game.vertical_field_of_view;
    }
}
