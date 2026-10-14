//! The game's dynamic lights (muzzle flashes, plasma, explosions, the
//! flashlight) in the ray-traced scene: each light the bridge shares is a
//! point light, or a spot light for one with a cone narrower than a
//! hemisphere, which Solarik samples as a small sphere. They are kept in
//! pools and reused frame to frame; unused ones are hidden.

use crate::{
    bridge::BridgeLink,
    level::{sphere_lumens_per_radiance, LightSource},
    LightingSettings,
};
use bevy::prelude::*;
use halo_map::math::{halo_to_bevy, METERS_PER_WORLD_UNIT};

/// The lights' size (metres): as bright as the emissive sphere that stood
/// in for one under Solari.
const LIGHT_RADIUS: f32 = 0.1;
/// A spot light's size: the disc that stood in for one, emitting from its
/// front.
const SPOT_RADIUS: f32 = 0.15;
/// Radiance per unit of light colour and squared reach (world units), before
/// the emissive scale: a muzzle flash (reaching 2 world units) visibly lights
/// the weapon and the ground nearby in daylight, and a dark room.
const RADIANCE_PER_REACH2: f32 = 30000.0;
/// Point lights' share of that (muzzle flashes, plasma, explosions): toned
/// down from the flashlight's.
const POINT_LIGHT_SCALE: f32 = 0.5;
/// Below this cutoff angle (radians, from the light's axis) a light is a
/// spot light; Bevy's cones stop short of a hemisphere.
const SPOT_CUTOFF: f32 = std::f32::consts::FRAC_PI_2 - 0.01;
/// A spot light's full brightness reaches this share of its cone (Halo's
/// dynamic lights give only the cutoff).
const SPOT_INNER: f32 = 0.75;

pub struct DynamicLightsPlugin;

impl Plugin for DynamicLightsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<DynamicLights>().add_systems(Update, sync_lights);
    }
}

#[derive(Resource, Default)]
struct DynamicLights {
    points: Vec<Entity>,
    spots: Vec<Entity>,
}

fn sync_lights(
    mut commands: Commands,
    link: Res<BridgeLink>,
    settings: Res<LightingSettings>,
    mut pools: ResMut<DynamicLights>,
    mut points: Query<(&mut PointLight, &mut Transform, &mut Visibility), Without<SpotLight>>,
    mut spots: Query<(&mut SpotLight, &mut Transform, &mut Visibility), Without<PointLight>>,
) {
    let Some(snapshot) = &link.latest else { return };
    let (mut used_points, mut used_spots) = (0, 0);
    for light in &snapshot.lights {
        let spot = light.cutoff_angle < SPOT_CUTOFF;
        let position = Vec3::from(halo_to_bevy(light.position, METERS_PER_WORLD_UNIT));
        let forward = Vec3::from(halo_to_bevy(light.forward, 1.0)).normalize_or(Vec3::NEG_Z);
        let scale = if spot { 1.0 } else { POINT_LIGHT_SCALE };
        let radiance = Vec3::from(light.color) * RADIANCE_PER_REACH2 * scale * light.radius * light.radius;
        let range = light.radius * METERS_PER_WORLD_UNIT;
        let radius = if spot { SPOT_RADIUS } else { LIGHT_RADIUS };
        let source = LightSource { power: radiance * sphere_lumens_per_radiance(radius) };
        let (color, intensity) = source.color_and_intensity(settings.emissive_scale);
        if spot {
            if used_spots == pools.spots.len() {
                let entity = commands
                    .spawn((Name::new("spotlight"), SpotLight { radius, shadow_maps_enabled: false, ..default() }, Visibility::Hidden))
                    .id();
                pools.spots.push(entity);
            }
            let entity = pools.spots[used_spots];
            used_spots += 1;
            let Ok((mut light_component, mut transform, mut visibility)) = spots.get_mut(entity) else { continue };
            let outer = light.cutoff_angle.max(0.01);
            *light_component = SpotLight {
                color,
                intensity,
                range,
                radius,
                outer_angle: outer,
                inner_angle: outer * SPOT_INNER,
                shadow_maps_enabled: false,
                ..default()
            };
            // (a spot light shines along its -Z)
            *transform = Transform::from_translation(position).looking_to(forward, if forward.y.abs() > 0.99 { Vec3::X } else { Vec3::Y });
            visibility.set_if_neq(Visibility::Inherited);
        } else {
            if used_points == pools.points.len() {
                let entity = commands
                    .spawn((Name::new("light"), PointLight { radius, shadow_maps_enabled: false, ..default() }, Visibility::Hidden))
                    .id();
                pools.points.push(entity);
            }
            let entity = pools.points[used_points];
            used_points += 1;
            let Ok((mut light_component, mut transform, mut visibility)) = points.get_mut(entity) else { continue };
            *light_component = PointLight { color, intensity, range, radius, shadow_maps_enabled: false, ..default() };
            *transform = Transform::from_translation(position);
            visibility.set_if_neq(Visibility::Inherited);
        }
    }
    // (the rest go dark)
    for &entity in pools.points.iter().skip(used_points) {
        if let Ok((_, _, mut visibility)) = points.get_mut(entity) {
            visibility.set_if_neq(Visibility::Hidden);
        }
    }
    for &entity in pools.spots.iter().skip(used_spots) {
        if let Ok((_, _, mut visibility)) = spots.get_mut(entity) {
            visibility.set_if_neq(Visibility::Hidden);
        }
    }
}
