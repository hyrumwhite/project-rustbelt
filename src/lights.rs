//! The game's dynamic lights (muzzle flashes, plasma, explosions, the
//! flashlight) in the ray-traced scene. Solari lights only with emissive
//! geometry, so each light the bridge shares lights a small emissive proxy
//! the rays see and the camera does not: a sphere, or for a spotlight a disc
//! facing where it points (emitting from its front only). Proxies are kept
//! in pools and reused frame to frame; unused ones leave the scene.

use crate::{bridge::BridgeLink, level::TracedMesh, LightingSettings};
use bevy::prelude::*;
use bevy_solarik::prelude::RaytracingMesh3d;
use halo_map::math::{halo_to_bevy, METERS_PER_WORLD_UNIT};

/// The proxies' size (metres).
const PROXY_RADIUS: f32 = 0.1;
/// Radiance per unit of light colour and squared reach (world units), before
/// the emissive scale: a muzzle flash (reaching 2 world units) visibly lights
/// the weapon and the ground nearby in daylight, and a dark room.
const RADIANCE_PER_REACH2: f32 = 30000.0;
/// Below this cutoff angle (radians) a light is a spotlight.
const SPOTLIGHT_CUTOFF: f32 = 3.0;

pub struct DynamicLightsPlugin;

impl Plugin for DynamicLightsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup).add_systems(Update, sync_lights);
    }
}

struct Proxy {
    entity: Entity,
    material: Handle<StandardMaterial>,
}

#[derive(Resource)]
struct DynamicLights {
    sphere: Handle<Mesh>,
    disc: Handle<Mesh>,
    spheres: Vec<Proxy>,
    discs: Vec<Proxy>,
}

fn setup(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>) {
    let sphere = meshes.add(Sphere::new(PROXY_RADIUS).mesh().ico(2).expect("a small sphere").with_generated_tangents().expect("sphere has normals and UVs"));
    // (a circle in the XY plane faces +Z; it is turned to face the light's
    // direction)
    let disc = meshes.add(Circle::new(PROXY_RADIUS * 1.5).mesh().build().with_generated_tangents().expect("circle has normals and UVs"));
    commands.insert_resource(DynamicLights { sphere, disc, spheres: Vec::new(), discs: Vec::new() });
}

fn proxy(commands: &mut Commands, materials: &mut Assets<StandardMaterial>, mesh: &Handle<Mesh>, name: &str) -> Proxy {
    let material = materials.add(StandardMaterial { base_color: Color::BLACK, ..default() });
    let entity = commands
        .spawn((
            Name::new(name.to_string()),
            RaytracingMesh3d(mesh.clone()),
            TracedMesh(mesh.clone()),
            MeshMaterial3d(material.clone()),
            Transform::default(),
            Visibility::Hidden,
        ))
        .id();
    Proxy { entity, material }
}

fn sync_lights(
    mut commands: Commands,
    link: Res<BridgeLink>,
    settings: Res<LightingSettings>,
    mut pools: ResMut<DynamicLights>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut proxies: Query<(&mut Transform, &mut Visibility)>,
) {
    let Some(snapshot) = &link.latest else { return };
    let DynamicLights { sphere, disc, spheres, discs } = &mut *pools;
    let (mut used_spheres, mut used_discs) = (0, 0);
    for light in &snapshot.lights {
        let spot = light.cutoff_angle < SPOTLIGHT_CUTOFF;
        let (pool, used, mesh, name) =
            if spot { (&mut *discs, &mut used_discs, &*disc, "spotlight") } else { (&mut *spheres, &mut used_spheres, &*sphere, "light") };
        if *used == pool.len() {
            pool.push(proxy(&mut commands, &mut materials, mesh, name));
        }
        let proxy = &pool[*used];
        *used += 1;
        let position = Vec3::from(halo_to_bevy(light.position, METERS_PER_WORLD_UNIT));
        let forward = Vec3::from(halo_to_bevy(light.forward, 1.0)).normalize_or(Vec3::NEG_Z);
        let radiance = Vec3::from(light.color) * RADIANCE_PER_REACH2 * light.radius * light.radius * settings.emissive_scale;
        if let Some(mut material) = materials.get_mut(&proxy.material) {
            material.emissive = LinearRgba::rgb(radiance.x, radiance.y, radiance.z);
        }
        if let Ok((mut transform, mut visibility)) = proxies.get_mut(proxy.entity) {
            *transform = if spot {
                // (the disc's front, +Z, along the light)
                Transform::from_translation(position)
                    .looking_to(-forward, if forward.y.abs() > 0.99 { Vec3::X } else { Vec3::Y })
            } else {
                Transform::from_translation(position)
            };
            if *visibility != Visibility::Inherited {
                *visibility = Visibility::Inherited;
            }
        }
    }
    // (the rest leave the ray-traced scene)
    for proxy in spheres.iter().skip(used_spheres).chain(discs.iter().skip(used_discs)) {
        if let Ok((_, mut visibility)) = proxies.get_mut(proxy.entity)
            && *visibility != Visibility::Hidden
        {
            *visibility = Visibility::Hidden;
        }
    }
}
