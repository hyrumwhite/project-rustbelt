//! The game's dynamic lights (muzzle flashes, plasma, explosions, the
//! flashlight, headlights) in the ray-traced scene: each light the bridge
//! shares is a point light, or a spot light for one with a cone of up to a
//! hemisphere, which Solarik samples as a small sphere, full inside the
//! light's falloff angle and fading to none at its cutoff. Muzzle flashes
//! (their tag's, protocol 38), which shine every way from the weapon, are
//! spot lights too: the player's out ahead along the camera's aim (round the
//! camera, in first person, they lit a circle round the player), others'
//! along their weapon's. Explosions, grenades and bolts stay round. A light
//! shining every way through a gel of a few narrow discs (a10's spinning
//! alarm lights: two) is a spot light for each, turned with the light. They
//! are kept in pools and reused frame to frame; unused ones are hidden.
//! Projectiles glow too: the plasma bolts by the lights their tags attach,
//! which in the game are only lens flares and light nothing (the game
//! shares them anyway, at their tags' colour and reach); rockets, needles,
//! and the Wraith's mortar, whose tags carry no light, by their own here.
//! The Sentinels' beams glow along their contrails (the beam's projectile
//! never outlives the tick that fired it), which the game shares with the
//! lights.

use crate::{
    bridge::{BridgeLevel, BridgeLink},
    level::{sphere_lumens_per_radiance, LightSource},
    LightingSettings,
};
use bevy::{camera::visibility::NoFrustumCulling, platform::collections::HashMap, prelude::*};
use halo_bridge::{Light, LIGHT_CONTRAIL, LIGHT_MUZZLE_FLASH};
use halo_map::{
    light_gel::{gel_beams, GelBeam},
    math::{halo_to_bevy, METERS_PER_WORLD_UNIT},
};

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
/// Others' spot lights (the Warthog's headlights, on a marker atop its
/// windscreen, one light 90 degrees wide that lit its hood): this far
/// further ahead and this far down (metres, the world's down: about its
/// bumper), their cone this share of the tag's, and this share of the
/// brightness.
const VEHICLE_SPOT_FORWARD: f32 = 1.6;
const VEHICLE_SPOT_DROP: f32 = 0.6;
const VEHICLE_SPOT_CONE: f32 = 0.6;
const VEHICLE_SPOT_SCALE: f32 = 0.5;
/// The player's spot light (their flashlight; the game marks it first
/// person, or failing that it is this near the camera, in world units) is
/// aimed where the camera looks: the game's light keeps the unit's level
/// facing.
const FLASHLIGHT_REACH: f32 = 1.0;
/// The player's flashlight's cone, as a share of its tag's (by the tag's it
/// lit most of the view), and its share of a spot light's brightness.
const FLASHLIGHT_CONE: f32 = 0.7;
const FLASHLIGHT_SCALE: f32 = 0.8;
/// A muzzle flash is a spot light: the player's this far out along the aim
/// (metres, about the muzzle), others' this far out along their weapon's,
/// with this cone (radians: full to the first, none at the second), and this
/// share of a point light's brightness.
const MUZZLE_FORWARD: f32 = 1.0;
const OTHERS_MUZZLE_FORWARD: f32 = 0.3;
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
/// A gel is drawn as spot lights when each of its discs is dark beyond this
/// angle (radians; a10's alarm beams, 40 degrees): the holograms' and the
/// Monitor's spill everywhere, and stay a point light.
const GEL_WIDEST: f32 = 1.05;
/// A gel's beams' share of the light's brightness (by the gel's own, its
/// discs lit the rooms as brightly as the light unmasked).
const GEL_LIGHT_SCALE: f32 = 0.5;

/// The projectiles that glow in flight with no light in their tags (the
/// fuel rods, charged bolts and grenades have their own, the other bolts
/// their lens flares'): their tag's name, their light's colour and reach
/// (world units, as a light tag's radius).
const PROJECTILE_GLOWS: [(&str, [f32; 3], f32); 3] = [
    // (its exhaust's flame)
    ("weapons\\rocket launcher\\rocket", [1.0, 0.55, 0.2], 2.0),
    // (pink crystal, in flight and stuck in what it hit)
    ("weapons\\needler\\needle", [1.0, 0.3, 0.8], 0.75),
    // (a ball of blue plasma)
    ("vehicles\\wraith\\wraith mortar", [0.5, 0.45, 1.0], 2.0),
];

/// The contrails that glow (by their contrail tag's name), with the colour
/// and reach (world units) of the lights along them: one each this far
/// apart (world units), up to this many.
const CONTRAIL_GLOWS: [(&str, [f32; 3], f32); 1] = [
    // (the Sentinels' beam: an orange laser)
    ("characters\\sentinel\\beam", [1.0, 0.45, 0.15], 1.0),
];
const CONTRAIL_GLOW_SPACING: f32 = 3.0;
const CONTRAIL_GLOW_MOST: usize = 4;

/// Lights dimmed from their tags' brightness (by the light tag's name, or
/// a folder's, ending in a backslash): a wide reach is a bright light here
/// (brightness grows with its square).
const LIGHT_DIMMING: [(&str, f32); 8] = [
    // (Guilty Spark's, reaching 5.5 world units: it lit rooms like
    // floodlights)
    ("characters\\monitor\\monitor dynamic", 0.15),
    // (the grenades' glow in flight, and their explosions', reaching 10)
    ("weapons\\frag grenade\\frag grenade", 0.7),
    ("weapons\\plasma grenade\\plasma grenade", 0.7),
    ("weapons\\frag grenade\\explosion", 0.6),
    ("weapons\\plasma grenade\\explosion", 0.6),
    // (b30's control room light, reaching 3.5 around its hologram until the
    // terminal turns it off: it washed out the hologram, which the game
    // draws brighter than the walls; play feedback: to 15%)
    ("levels\\b30\\devices\\interior tech objects\\holo control room display\\light", 0.15),
    // (c10's coloured point lights, flanking its doors, some blinking: they
    // flooded the rooms green and red; play feedback: dramatically down,
    // then half that again)
    ("levels\\c10\\scenery\\", 0.05),
    // (the batons, glow sticks: c10's landing zone is strewn with them;
    // play feedback: too bright)
    ("scenery\\baton\\baton light dynamic", 0.5),
];

/// Lights smaller than [`LIGHT_RADIUS`] (by the light tag's name), raised
/// this much (metres, the world's up) to clear the ground they lie on: a
/// baton's light is at its middle, on the ground, and the ground within its
/// sphere, under the light's surface, went unlit, a dark disc round the
/// stick.
const SMALL_LIGHTS: [(&str, f32); 1] = [("scenery\\baton\\baton light dynamic", 0.05)];

/// Whether a light tag's name is the dimmed one, or in the dimmed folder.
fn dimmed_by(name: &str, dimmed: &str) -> bool {
    if dimmed.ends_with('\\') {
        name.len() > dimmed.len() && name.is_char_boundary(dimmed.len()) && name[..dimmed.len()].eq_ignore_ascii_case(dimmed)
    } else {
        name.eq_ignore_ascii_case(dimmed)
    }
}

/// What a light's tag says of it here.
#[derive(Clone, Copy)]
struct LightTag {
    /// A muzzle flash, by its name.
    muzzle: bool,
    /// Its share of its brightness ([`LIGHT_DIMMING`]).
    dimming: f32,
    /// Its size, if small ([`SMALL_LIGHTS`]).
    small: Option<f32>,
}

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
    level: Option<Res<BridgeLevel>>,
    in_solid: Res<crate::bridge::LightsInSolid>,
    mut light_tags: Local<HashMap<u32, LightTag>>,
    mut gels: Local<HashMap<u32, Vec<GelBeam>>>,
    mut glows: Local<HashMap<u32, Option<([f32; 3], f32)>>>,
    mut contrail_glows: Local<HashMap<u32, Option<([f32; 3], f32)>>>,
    mut scenario: Local<String>,
) {
    let Some(snapshot) = &link.latest else { return };
    // (a new level: its own tags)
    if snapshot.scenario_name != *scenario {
        light_tags.clear();
        gels.clear();
        glows.clear();
        contrail_glows.clear();
        *scenario = snapshot.scenario_name.clone();
    }
    // the glowing projectiles' lights, round from their middles (by no tag:
    // neither muzzle flashes nor gelled; hidden ones too: a projectile
    // without a model, a Sentinel's beam, the game hides)
    let projectile_lights: Vec<Light> = snapshot
        .objects
        .iter()
        .filter_map(|object| {
            let (color, radius) = (*glows.entry(object.definition_index).or_insert_with(|| {
                let tag = level.as_ref()?.builder.cache.tag(object.definition_index).ok()?;
                PROJECTILE_GLOWS.iter().find(|(name, ..)| tag.name.eq_ignore_ascii_case(name)).map(|&(_, color, radius)| (color, radius))
            }))?;
            Some(Light {
                position: object.bounding_center,
                forward: [1.0, 0.0, 0.0],
                up: [0.0, 0.0, 1.0],
                color,
                radius,
                cutoff_angle: std::f32::consts::PI,
                falloff_angle: std::f32::consts::PI,
                definition_index: u32::MAX,
                object_index: object.object_index,
                ..default()
            })
        })
        .collect();
    // the glowing contrails' lights, evenly along each (the rest of the
    // contrails light nothing)
    let mut contrail_lights: Vec<Light> = Vec::new();
    for contrail in snapshot.lights.iter().filter(|light| light.definition_flags & LIGHT_CONTRAIL != 0) {
        let Some((color, radius)) = *contrail_glows.entry(contrail.definition_index).or_insert_with(|| {
            let tag = level.as_ref()?.builder.cache.tag(contrail.definition_index).ok()?;
            CONTRAIL_GLOWS.iter().find(|(name, ..)| tag.name.eq_ignore_ascii_case(name)).map(|&(_, color, radius)| (color, radius))
        }) else {
            continue;
        };
        let (middle, along, length) = (Vec3::from(contrail.position), Vec3::from(contrail.forward), 2.0 * contrail.radius);
        let count = ((length / CONTRAIL_GLOW_SPACING).ceil() as usize).clamp(1, CONTRAIL_GLOW_MOST);
        for index in 0..count {
            let offset = ((index as f32 + 0.5) / count as f32 - 0.5) * length;
            contrail_lights.push(Light {
                position: (middle + along * offset).to_array(),
                forward: [1.0, 0.0, 0.0],
                up: [0.0, 0.0, 1.0],
                color,
                radius,
                cutoff_angle: std::f32::consts::PI,
                falloff_angle: std::f32::consts::PI,
                definition_index: u32::MAX,
                object_index: contrail.object_index,
                ..default()
            });
        }
    }
    // (but those in the drawn BSP's solid, another BSP's: crate::bridge)
    let lights = snapshot.lights.iter().filter(|light| {
        light.definition_flags & LIGHT_CONTRAIL == 0 && !in_solid.0.contains(&(light.definition_index, light.object_index))
    });
    let (mut used_points, mut used_spots) = (0, 0);
    for light in lights.chain(&projectile_lights).chain(&contrail_lights) {
        let near = snapshot.camera.as_ref().filter(|camera| Vec3::from(camera.position).distance(Vec3::from(light.position)) < FLASHLIGHT_REACH);
        let own = light.first_person != 0;
        // (a muzzle flash: its tag's flag, or its name)
        let tag = *light_tags.entry(light.definition_index).or_insert_with(|| {
            let name = level.as_ref().and_then(|level| level.builder.cache.tag(light.definition_index).ok()).map(|tag| tag.name.as_str());
            LightTag {
                muzzle: name.is_some_and(|name| name.ends_with("muzzle flash")),
                dimming: name.and_then(|name| LIGHT_DIMMING.iter().find(|(dimmed, _)| dimmed_by(name, dimmed))).map_or(1.0, |&(_, dimming)| dimming),
                small: name.and_then(|name| SMALL_LIGHTS.iter().find(|(small, _)| name.eq_ignore_ascii_case(small))).map(|&(_, size)| size),
            }
        });
        let muzzle_tag = light.definition_flags & LIGHT_MUZZLE_FLASH != 0 || tag.muzzle;
        let muzzle = light.cutoff_angle > SPOT_CUTOFF && muzzle_tag;
        let spot = light.cutoff_angle <= SPOT_CUTOFF || muzzle;
        // (the player's spot lights follow the camera's aim)
        let near_camera = snapshot.camera.as_ref().filter(|_| spot && (own || (!muzzle && near.is_some())));
        // (HALO_RT_LIGHT_LOG: each light, in the log)
        if std::env::var_os("HALO_RT_LIGHT_LOG").is_some() {
            let distance = snapshot.camera.as_ref().map(|c| Vec3::from(c.position).distance(Vec3::from(light.position)));
            info!(
                "light at tick {}: position {:?} forward {:?} colour {:?} radius {} cutoff {:.1} falloff {:.1} deg, {distance:?} from the camera (forward {:?}), own {own} muzzle {muzzle} aimed by the camera {} tag {:#x} flags {:#x} object {:#x}",
                snapshot.game_tick, light.position, light.forward, light.color, light.radius, light.cutoff_angle.to_degrees(), light.falloff_angle.to_degrees(),
                snapshot.camera.as_ref().map(|c| c.forward), near_camera.is_some(), light.definition_index, light.definition_flags, light.object_index
            );
        }
        let forward = Vec3::from(halo_to_bevy(near_camera.map_or(light.forward, |camera| camera.forward), 1.0)).normalize_or(Vec3::NEG_Z);
        let vehicle = spot && !muzzle && near_camera.is_none();
        let flashlight = spot && !muzzle && near_camera.is_some();
        let ahead = match (muzzle, spot) {
            (true, _) if near_camera.is_some() => MUZZLE_FORWARD,
            (true, _) => OTHERS_MUZZLE_FORWARD,
            (false, true) if vehicle => SPOT_FORWARD + VEHICLE_SPOT_FORWARD,
            (false, true) => SPOT_FORWARD,
            (false, false) => 0.0,
        };
        let drop = if vehicle { Vec3::NEG_Y * VEHICLE_SPOT_DROP } else { Vec3::ZERO };
        let position = Vec3::from(halo_to_bevy(light.position, METERS_PER_WORLD_UNIT)) + forward * ahead + drop;
        let scale = if muzzle {
            MUZZLE_LIGHT_SCALE
        } else if vehicle {
            SPOT_LIGHT_SCALE * VEHICLE_SPOT_SCALE
        } else if flashlight {
            SPOT_LIGHT_SCALE * FLASHLIGHT_SCALE
        } else if spot {
            SPOT_LIGHT_SCALE
        } else {
            POINT_LIGHT_SCALE
        } * tag.dimming;
        let reach2 = if spot && !muzzle { light.radius * SPOT_REFERENCE_REACH } else { light.radius * light.radius };
        let radiance = Vec3::from(light.color) * RADIANCE_PER_REACH2 * scale * reach2;
        let range = light.radius * METERS_PER_WORLD_UNIT * RANGE_PER_RADIUS;
        let radius = if spot { SPOT_RADIUS } else { tag.small.unwrap_or(LIGHT_RADIUS) };
        let position = position + Vec3::Y * tag.small.map_or(0.0, |size| size * 1.2);
        let source = LightSource { power: radiance * sphere_lumens_per_radiance(radius) };
        let (color, intensity) = source.color_and_intensity(settings.emissive_scale);
        // a light shining every way through a gel of narrow discs: a spot
        // light along each, by the light's forward, left and up (the game
        // turns the gel with them), each as bright in its cone as the light
        // was there, by the gel
        let beams = if spot {
            &[][..]
        } else {
            gels.entry(light.definition_index)
                .or_insert_with(|| {
                    let beams = level.as_ref().and_then(|level| gel_beams(&level.builder.cache, light.definition_index).ok()).unwrap_or_default();
                    if beams.iter().all(|beam| beam.cutoff_angle <= GEL_WIDEST) { beams } else { Vec::new() }
                })
                .as_slice()
        };
        let (light_forward, light_up) = (Vec3::from(light.forward), Vec3::from(light.up));
        let light_left = light_forward.cross(light_up);
        if !beams.is_empty() && light_left.length_squared() > 1e-6 {
            let light_left = light_left.normalize();
            let light_up = light_left.cross(light_forward).normalize();
            let source = LightSource { power: radiance * GEL_LIGHT_SCALE * sphere_lumens_per_radiance(SPOT_RADIUS) };
            for beam in beams {
                let [f, l, u] = beam.direction;
                let along = light_forward * f + light_left * l + light_up * u;
                let forward = Vec3::from(halo_to_bevy(along.to_array(), 1.0)).normalize_or(Vec3::NEG_Z);
                let (color, intensity) = LightSource { power: source.power * beam.strength }.color_and_intensity(settings.emissive_scale);
                // (never frustum culled, here and below: Bevy would leave a
                // light out of the scene with its range off screen, and its
                // bounces with it)
                if used_spots == pools.spots.len() {
                    let entity = commands
                        .spawn((Name::new("spotlight"), SpotLight { radius: SPOT_RADIUS, shadow_maps_enabled: false, ..default() }, Visibility::Hidden, NoFrustumCulling))
                        .id();
                    pools.spots.push(entity);
                }
                let entity = pools.spots[used_spots];
                used_spots += 1;
                let Ok((mut light_component, mut transform, mut visibility)) = spots.get_mut(entity) else { continue };
                let outer = beam.cutoff_angle.clamp(0.01, SPOT_WIDEST);
                *light_component = SpotLight {
                    color,
                    intensity,
                    range,
                    radius: SPOT_RADIUS,
                    outer_angle: outer,
                    inner_angle: beam.falloff_angle.clamp(0.0, outer * 0.99),
                    shadow_maps_enabled: false,
                    ..default()
                };
                *transform = Transform::from_translation(position).looking_to(forward, if forward.y.abs() > 0.99 { Vec3::X } else { Vec3::Y });
                visibility.set_if_neq(Visibility::Inherited);
            }
            continue;
        }
        if spot {
            if used_spots == pools.spots.len() {
                let entity = commands
                    .spawn((Name::new("spotlight"), SpotLight { radius, shadow_maps_enabled: false, ..default() }, Visibility::Hidden, NoFrustumCulling))
                    .id();
                pools.spots.push(entity);
            }
            let entity = pools.spots[used_spots];
            used_spots += 1;
            let Ok((mut light_component, mut transform, mut visibility)) = spots.get_mut(entity) else { continue };
            let (cutoff, falloff) = if muzzle {
                (MUZZLE_CUTOFF, MUZZLE_FALLOFF)
            } else if vehicle {
                (light.cutoff_angle * VEHICLE_SPOT_CONE, light.falloff_angle * VEHICLE_SPOT_CONE)
            } else if flashlight {
                (light.cutoff_angle * FLASHLIGHT_CONE, light.falloff_angle * FLASHLIGHT_CONE)
            } else {
                (light.cutoff_angle, light.falloff_angle)
            };
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
                    .spawn((Name::new("light"), PointLight { radius, shadow_maps_enabled: false, ..default() }, Visibility::Hidden, NoFrustumCulling))
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
