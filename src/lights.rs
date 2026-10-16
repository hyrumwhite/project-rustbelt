//! The game's dynamic lights (muzzle flashes, plasma, explosions, the
//! flashlight, headlights) in the ray-traced scene: each light the bridge
//! shares is a point light, or a spot light for one with a cone of up to a
//! hemisphere, which Solarik samples as a small sphere, full inside the
//! light's falloff angle and fading to none at its cutoff. The player's own
//! weapon lights (muzzle flashes, shining every way from the weapon, at the
//! camera in first person) are spot lights too, out ahead along the aim:
//! round the camera they lit a circle round the player. They are kept in
//! pools and reused frame to frame; unused ones are hidden.

use crate::{
    bridge::BridgeLink,
    level::{sphere_lumens_per_radiance, LightSource},
    LightingSettings,
};
use bevy::prelude::*;
use halo_map::math::{halo_to_bevy, METERS_PER_WORLD_UNIT};

/// The lights' size (metres): larger, softer round the light (a muzzle
/// flash's bright spot on the weapon and the wall beside it).
const LIGHT_RADIUS: f32 = 0.25;
/// A spot light's size: the disc that stood in for one, emitting from its
/// front.
const SPOT_RADIUS: f32 = 0.15;
/// Radiance per unit of light colour and squared reach (world units), before
/// the emissive scale: a muzzle flash (reaching 2 world units) visibly lights
/// the weapon and the ground nearby in daylight, and a dark room.
const RADIANCE_PER_REACH2: f32 = 30000.0;
/// Point lights' share of that (muzzle flashes, plasma, explosions).
const POINT_LIGHT_SCALE: f32 = 0.25;
/// Spot lights' (the flashlight's): its cone holds its light, and at full
/// strength it washed the view white.
const SPOT_LIGHT_SCALE: f32 = 0.1;
/// A spot light's brightness grows with its reach only linearly, from the
/// flashlight's (6 world units): the Warthog's headlights reach 12, and by
/// its square they glared.
const SPOT_REFERENCE_REACH: f32 = 6.0;
/// How far ahead of the game's position a spot light shines from (metres):
/// the flashlight sits at the eye, inside the first-person weapon, which
/// it lit glaring white.
const SPOT_FORWARD: f32 = 0.6;
/// A spot light this near the game's camera (world units) is the player's
/// flashlight, aimed where the camera looks: the game's light keeps the
/// unit's level facing.
const FLASHLIGHT_REACH: f32 = 1.0;
/// A light shining every way this near the camera is the player's weapon's
/// (a muzzle flash, plasma): a spot light this far out along the aim
/// (metres, about the muzzle), with this cone (radians: full to the first,
/// none at the second), and this share of a point light's brightness.
const MUZZLE_FORWARD: f32 = 1.0;
const MUZZLE_FALLOFF: f32 = 0.45;
const MUZZLE_CUTOFF: f32 = 1.2;
const MUZZLE_LIGHT_SCALE: f32 = 0.15;
/// Up to this cutoff angle (radians, from the light's axis) a light is a
/// spot light (the Warthog's headlights: a hemisphere, 90 degrees).
const SPOT_CUTOFF: f32 = std::f32::consts::FRAC_PI_2 + 0.001;
/// The widest spot cone: Bevy's cones stop short of a hemisphere.
const SPOT_WIDEST: f32 = std::f32::consts::FRAC_PI_2 - 0.01;
/// How far a light reaches, over its radius: Bevy's range window fades it
/// to none at the range, and at the radius the flashlight stopped short.
const RANGE_PER_RADIUS: f32 = 1.75;

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
        let near = snapshot.camera.as_ref().filter(|camera| Vec3::from(camera.position).distance(Vec3::from(light.position)) < FLASHLIGHT_REACH);
        let muzzle = light.cutoff_angle > SPOT_CUTOFF && near.is_some();
        let spot = light.cutoff_angle <= SPOT_CUTOFF || muzzle;
        let near_camera = near.filter(|_| spot);
        // (HALO_RT_LIGHT_LOG: each light, in the log)
        if std::env::var_os("HALO_RT_LIGHT_LOG").is_some() {
            let distance = snapshot.camera.as_ref().map(|c| Vec3::from(c.position).distance(Vec3::from(light.position)));
            info!(
                "light at tick {}: position {:?} forward {:?} colour {:?} radius {} cutoff {:.1} falloff {:.1} deg, {distance:?} from the camera (forward {:?}), flashlight {} muzzle {muzzle}",
                snapshot.game_tick, light.position, light.forward, light.color, light.radius, light.cutoff_angle.to_degrees(), light.falloff_angle.to_degrees(),
                snapshot.camera.as_ref().map(|c| c.forward), near_camera.is_some()
            );
        }
        let forward = Vec3::from(halo_to_bevy(near_camera.map_or(light.forward, |camera| camera.forward), 1.0)).normalize_or(Vec3::NEG_Z);
        let ahead = if muzzle { MUZZLE_FORWARD } else if spot { SPOT_FORWARD } else { 0.0 };
        let position = Vec3::from(halo_to_bevy(light.position, METERS_PER_WORLD_UNIT)) + forward * ahead;
        let scale = if muzzle { MUZZLE_LIGHT_SCALE } else if spot { SPOT_LIGHT_SCALE } else { POINT_LIGHT_SCALE };
        let reach2 = if spot && !muzzle { light.radius * SPOT_REFERENCE_REACH } else { light.radius * light.radius };
        let radiance = Vec3::from(light.color) * RADIANCE_PER_REACH2 * scale * reach2;
        let range = light.radius * METERS_PER_WORLD_UNIT * RANGE_PER_RADIUS;
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
            let (cutoff, falloff) = if muzzle { (MUZZLE_CUTOFF, MUZZLE_FALLOFF) } else { (light.cutoff_angle, light.falloff_angle) };
            let outer = cutoff.clamp(0.01, SPOT_WIDEST);
            *light_component = SpotLight {
                color,
                intensity,
                range,
                radius,
                outer_angle: outer,
                inner_angle: falloff.clamp(0.0, outer * 0.99),
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
