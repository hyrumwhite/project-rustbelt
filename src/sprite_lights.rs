//! Glowing particles light the scene by their own brightness (ROADMAP:
//! effects in the viewer, particles). The game's added sprites (fire,
//! sparks, plasma's glow, energy: everything `build_sprites_end` adds to
//! the picture) are drawn unlit (`src/effects.rs`), and lit nothing unless
//! their effect attached a light. Here each frame's added sprites are
//! gathered into clusters, and each cluster is a sphere light where it
//! glows: as bright as its sprites show (each quad's share of its bitmap,
//! times its vertex colour and fade, as the effect shader adds them), over
//! the cluster's spread. Where an effect's own light shines (muzzle
//! flashes, bolts, an explosion's flash: the game's dynamic lights,
//! `src/lights.rs`), its sprites light nothing more; a grenade's fire,
//! outliving its flash, does.
//!
//! The lights keep their entities from frame to frame by place (each
//! cluster takes the last frame's nearest), as the dynamic lights do by
//! their tags: lights trading places took their lighting's history with
//! them.
//!
//! Tuning: `HALO_RT_PARTICLE_LIGHT=<gain>` (0 none);
//! `HALO_RT_PARTICLE_LIGHT_LOG` logs the brightest clusters.

use crate::{
    bridge::{BridgeLevel, BridgeLink},
    level::{sphere_lumens_per_radiance, LightSource},
    LightingSettings,
};
use bevy::{camera::visibility::NoFrustumCulling, prelude::*};
use halo_bridge::{DECAL_BLEND_ADD, DECAL_BLEND_ALPHA_MULTIPLY_ADD, LIGHT_CONTRAIL, SPRITE_LENS_FLARE};
use halo_map::math::{halo_to_bevy, METERS_PER_WORLD_UNIT};

/// The particles' light, over what they show. The sprites are drawn in the
/// display's range, where a fire is no brighter than white: lit as they show
/// (1), they lit nothing to see beside the game's own lights. At this, a
/// frag grenade's fire (its sprites' radiance times area near 9 at first)
/// lights the ground round it orange at night under the game's white flash,
/// fading over most of a second (a50's canyon, with `HALO_TEST_INPUT=throw`;
/// at 300 the orange outshone the flash).
const PARTICLE_LIGHT: f32 = 100.0;
/// Radiance of a displayed white sprite, as self-illumination's
/// (`level::SELF_ILLUMINATION_RADIANCE`: with the default emissive scale and
/// exposure, about display white).
const SPRITE_WHITE_RADIANCE: f32 = 2.5;
/// Sprites within this of a cluster's middle (metres) join it.
const CLUSTER_REACH: f32 = 2.0;
/// The most clusters gathered (the rest join their nearest), and the most
/// that light the scene (the brightest).
const CLUSTERS_GATHERED: usize = 64;
const MOST_LIGHTS: usize = 16;
/// A cluster's sphere's radius (metres): its sprites' spread, within these.
const SMALLEST: f32 = 0.1;
const LARGEST: f32 = 1.5;
/// A cluster dimmer than this (its radiance times area, summed) lights
/// nothing.
const DIMMEST: f32 = 0.002;
/// A light reaches as far as it lights a surface facing it this much
/// (radiance), within these (metres).
const REACH_IRRADIANCE: f32 = 0.01;
const NEAREST_REACH: f32 = 1.0;
const FARTHEST_REACH: f32 = 20.0;
/// A cluster within this of a game light (metres) is its effect's, which
/// lights it already.
const OWN_LIGHT_DISTANCE: f32 = 1.0;
/// A light takes the entity of last frame's nearest within this (metres).
const FOLLOW_DISTANCE: f32 = 1.5;

pub struct SpriteLightsPlugin;

impl Plugin for SpriteLightsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SpriteLights>().add_systems(Update, sprite_lights);
    }
}

/// The lights, each with where it was last.
#[derive(Resource, Default)]
struct SpriteLights {
    lights: Vec<(Entity, Vec3, bool)>,
    last_frame_index: u32,
}

/// Sprites gathered: their radiance times area (summed), and that's
/// luminance-weighted sums of their places, and of their places' squares.
#[derive(Clone, Copy, Default)]
struct Cluster {
    power: Vec3,
    weight: f32,
    place: Vec3,
    square: f32,
    /// The brightest batch's bitmap and its share (for the log).
    brightest: (u32, f32),
}

impl Cluster {
    fn middle(&self) -> Vec3 {
        self.place / self.weight.max(1e-12)
    }

    fn add(&mut self, power: Vec3, place: Vec3, map: u32) {
        let weight = power.dot(Vec3::new(0.2126, 0.7152, 0.0722));
        self.power += power;
        self.weight += weight;
        self.place += place * weight;
        self.square += place.length_squared() * weight;
        if weight > self.brightest.1 {
            self.brightest = (map, weight);
        }
    }

    fn merge(&mut self, other: &Cluster) {
        self.power += other.power;
        self.weight += other.weight;
        self.place += other.place;
        self.square += other.square;
        if other.brightest.1 > self.brightest.1 {
            self.brightest = other.brightest;
        }
    }

    /// Its sprites' spread about its middle (metres).
    fn spread(&self) -> f32 {
        (self.square / self.weight.max(1e-12) - self.middle().length_squared()).max(0.0).sqrt()
    }
}

#[allow(clippy::too_many_arguments)]
fn sprite_lights(
    mut commands: Commands,
    link: Res<BridgeLink>,
    settings: Res<LightingSettings>,
    mut state: ResMut<SpriteLights>,
    mut level: Option<ResMut<BridgeLevel>>,
    mut points: Query<(&mut PointLight, &mut Transform, &mut Visibility)>,
) {
    let Some(snapshot) = link.latest.as_ref() else { return };
    if snapshot.frame_index == state.last_frame_index {
        return;
    }
    state.last_frame_index = snapshot.frame_index;
    let gain = std::env::var("HALO_RT_PARTICLE_LIGHT").ok().and_then(|v| v.parse().ok()).unwrap_or(PARTICLE_LIGHT);
    let log = std::env::var_os("HALO_RT_PARTICLE_LIGHT_LOG").is_some();
    let mut clusters: Vec<Cluster> = Vec::new();
    if let Some(level) = level.as_deref_mut().filter(|_| gain > 0.0) {
        let builder = &mut level.builder;
        for batch in snapshot.sprite_batches.iter() {
            // added sprites in the world (the first-person weapon's own:
            // its muzzle flash's light)
            if batch.kind == SPRITE_LENS_FLARE
                || batch.first_person != 0
                || !matches!(batch.blend_function as u8, DECAL_BLEND_ADD | DECAL_BLEND_ALPHA_MULTIPLY_ADD)
            {
                continue;
            }
            let first = batch.first_vertex as usize;
            let Some(vertices) = snapshot.sprite_vertices.get(first..first + batch.quad_count as usize * 4) else { continue };
            let Some(sheet) = builder.textures.summed_area(&builder.cache, batch.map, batch.bitmap_index) else { continue };
            for quad in vertices.chunks_exact(4) {
                let corner = |i: usize| Vec3::from(halo_to_bevy(quad[i].position, METERS_PER_WORLD_UNIT));
                let (p0, p1, p2, p3) = (corner(0), corner(1), corner(2), corner(3));
                let area = 0.5 * ((p1 - p0).cross(p2 - p0).length() + (p2 - p0).cross(p3 - p0).length());
                // its share of the sheet
                let (mut min, mut max) = ([f32::MAX; 2], [f32::MIN; 2]);
                for v in quad {
                    for k in 0..2 {
                        min[k] = min[k].min(v.texcoord[k]);
                        max[k] = max[k].max(v.texcoord[k]);
                    }
                }
                let texels = Vec3::from(sheet.mean(min, max));
                // times the vertices' colour and fade (display-encoded, as
                // the texels: c a, decoded, is t^2.2 (v a)^2.2)
                let mut tint = Vec3::ZERO;
                for v in quad {
                    let c = v.color;
                    let a = (c >> 24) as f32 / 255.0;
                    let rgb = Vec3::new(((c >> 16) & 255) as f32, ((c >> 8) & 255) as f32, (c & 255) as f32) / 255.0;
                    tint += (rgb * a).powf(2.2) / 4.0;
                }
                let power = texels * tint * SPRITE_WHITE_RADIANCE * gain * area;
                if power.max_element() <= 0.0 {
                    continue;
                }
                let place = (p0 + p1 + p2 + p3) / 4.0;
                match clusters.iter_mut().find(|cluster| cluster.middle().distance(place) < CLUSTER_REACH) {
                    Some(cluster) => cluster.add(power, place, batch.map),
                    None => {
                        let mut cluster = Cluster::default();
                        cluster.add(power, place, batch.map);
                        clusters.push(cluster);
                        // (too many: the dimmest joins its nearest)
                        if clusters.len() > CLUSTERS_GATHERED {
                            let dimmest = (0..clusters.len()).min_by(|&a, &b| clusters[a].weight.total_cmp(&clusters[b].weight)).unwrap();
                            let gone = clusters.swap_remove(dimmest);
                            let middle = gone.middle();
                            if let Some(nearest) = clusters.iter_mut().min_by(|a, b| a.middle().distance(middle).total_cmp(&b.middle().distance(middle))) {
                                nearest.merge(&gone);
                            }
                        }
                    }
                }
            }
        }
    }
    // (those an effect's own light lights already: theirs)
    let game_lights: Vec<Vec3> = snapshot
        .lights
        .iter()
        .filter(|light| light.definition_flags & LIGHT_CONTRAIL == 0)
        .map(|light| Vec3::from(halo_to_bevy(light.position, METERS_PER_WORLD_UNIT)))
        .collect();
    clusters.retain(|cluster| {
        cluster.weight > DIMMEST && !game_lights.iter().any(|light| light.distance(cluster.middle()) < OWN_LIGHT_DISTANCE)
    });
    clusters.sort_by(|a, b| b.weight.total_cmp(&a.weight));
    clusters.truncate(MOST_LIGHTS);
    if log && !clusters.is_empty() {
        let name = |datum: u32| level.as_deref().and_then(|level| level.builder.cache.tag(datum).ok()).map_or("-".to_string(), |t| t.name.clone());
        for cluster in &clusters {
            info!(
                "particle light at tick {}: {:?} power {:.3?} spread {:.2} m, brightest {}",
                snapshot.game_tick,
                cluster.middle(),
                cluster.power,
                cluster.spread(),
                name(cluster.brightest.0)
            );
        }
    }

    // each light at its cluster, by last frame's nearest
    let mut taken = vec![false; state.lights.len()];
    for cluster in &clusters {
        let middle = cluster.middle();
        let radius = cluster.spread().clamp(SMALLEST, LARGEST);
        // (a glowing ball of the sprites' brightness and size: its radiance
        // times its projected area, π r², is theirs)
        let radiance = cluster.power / (std::f32::consts::PI * radius * radius);
        let (color, intensity) = LightSource { power: radiance * sphere_lumens_per_radiance(radius) }.color_and_intensity(settings.emissive_scale);
        let range = (cluster.power.max_element() / REACH_IRRADIANCE).sqrt().clamp(NEAREST_REACH, FARTHEST_REACH) + radius;
        let light = PointLight { color, intensity, range, radius, shadow_maps_enabled: false, ..default() };
        let transform = Transform::from_translation(middle);
        // (last frame's nearest shown nearby, or a dark one, or any)
        let free = |i: &usize| !taken[*i];
        let distance = |i: usize| state.lights[i].1.distance(middle);
        let reuse = (0..state.lights.len())
            .filter(|i| free(i) && state.lights[*i].2 && distance(*i) < FOLLOW_DISTANCE)
            .min_by(|&a, &b| distance(a).total_cmp(&distance(b)))
            .or_else(|| (0..state.lights.len()).find(|i| free(i) && !state.lights[*i].2))
            .or_else(|| (0..state.lights.len()).find(free));
        let index = match reuse.and_then(|i| points.get_mut(state.lights[i].0).ok().map(|q| (i, q))) {
            Some((i, (mut point, mut place, mut visibility))) => {
                *point = light;
                *place = transform;
                visibility.set_if_neq(Visibility::Inherited);
                i
            }
            None => {
                let entity = commands.spawn((Name::new("particle light"), light, transform, Visibility::Inherited, NoFrustumCulling)).id();
                state.lights.push((entity, middle, true));
                taken.push(false);
                state.lights.len() - 1
            }
        };
        taken[index] = true;
        state.lights[index].1 = middle;
        state.lights[index].2 = true;
    }
    // (the rest go dark)
    for (i, (entity, _, shown)) in state.lights.iter_mut().enumerate() {
        if !taken[i] && *shown {
            if let Ok((_, _, mut visibility)) = points.get_mut(*entity) {
                visibility.set_if_neq(Visibility::Hidden);
            }
            *shown = false;
        }
    }
}
