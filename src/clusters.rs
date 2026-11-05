//! A level's clusters and what they see: each BSP's collision tree (the
//! cluster a point is in), its clusters' portals and skies, and its
//! potentially visible sets (`structure_bsp_get_cluster_pvs`). The bridge
//! hides what the game's camera can't see by them (crate::bridge), and the
//! lights in sight of the camera's cluster get the larger share of the rays'
//! light samples ([`mark_seen_lights`]; Solarik's `SolarikSeenLights`): a
//! room lit by a few small strips had nine pixels in ten pick a light behind
//! a wall, and the shadow ray dropped it (b40's elevator shaft, 92%; the
//! lights' selection is global, by power, blind to where they shine).

use crate::level::EmissiveSurface;
use bevy::prelude::*;
use bevy_solarik::scene::SolarikSeenLights;
use halo_map::math::METERS_PER_WORLD_UNIT;
use std::sync::Arc;

pub struct ClusterVisibility {
    pub(crate) collision: halo_map::collision::CollisionBsp,
    pub(crate) clusters: halo_map::collision::StructureClusters,
    /// A bit vector of the clusters each cluster sees (32-bit words).
    pvs: Vec<u32>,
    words_per_cluster: usize,
    /// Each cluster's sky (the scenario's), -1 for none: the game draws no
    /// sky from a camera in such a cluster (`structure_visibility_find_camera`).
    pub(crate) skies: Vec<i16>,
}

impl ClusterVisibility {
    /// Each of the level's BSPs', by BSP index.
    pub(crate) fn load_all(cache: &halo_map::CacheFile) -> Vec<Option<Self>> {
        let count = halo_map::bsp::references(cache).map(|r| r.iter().map(|r| r.index as usize + 1).max().unwrap_or(0)).unwrap_or(0);
        (0..count).map(|index| Self::load(cache, index as i32)).collect()
    }

    /// BSP `index`'s, None if it can't be read.
    pub(crate) fn load(cache: &halo_map::CacheFile, index: i32) -> Option<Self> {
        use halo_map::layout::{structure_bsp, structure_cluster};
        let reference = halo_map::bsp::references(cache).ok()?.into_iter().find(|r| r.index as i32 == index)?;
        let bsp = halo_map::bsp::Bsp::load(cache, &reference).ok()?;
        let clusters = bsp.root.block(structure_bsp::CLUSTERS, structure_cluster::SIZEOF).ok()?;
        let count = clusters.count;
        let skies = clusters.iter().map(|c| c.i16(structure_cluster::SKY_INDEX).unwrap_or(0)).collect();
        let pvs: Vec<u32> = bsp.root.data_bytes(structure_bsp::CLUSTER_DATA).ok()?.chunks_exact(4).map(|w| u32::from_le_bytes(w.try_into().unwrap())).collect();
        let words_per_cluster = count.div_ceil(32);
        if count == 0 || pvs.len() < count * words_per_cluster {
            return None;
        }
        Some(ClusterVisibility {
            collision: halo_map::collision::CollisionBsp::load(&bsp).ok()?,
            clusters: halo_map::collision::StructureClusters::load(cache, &bsp).ok()?,
            pvs,
            words_per_cluster,
            skies,
        })
    }

    /// The cluster a point (world units) is in, -1 outside the BSP.
    pub(crate) fn cluster(&self, point: [f32; 3]) -> i16 {
        self.clusters.cluster_from_leaf(self.collision.leaf_from_point(point))
    }

    /// Whether cluster `from` sees any cluster with a sky.
    pub(crate) fn sees_sky(&self, from: i16) -> bool {
        from >= 0 && self.skies.iter().enumerate().any(|(to, &sky)| sky >= 0 && self.sees(from, to as i16))
    }

    /// Whether this BSP holds the interior that `active`'s cluster `from`
    /// sees: the portals of each cluster without sky it sees, by their
    /// centres.
    pub(crate) fn holds_interior_seen(&self, active: &ClusterVisibility, from: i16) -> bool {
        (0..active.skies.len()).filter(|&to| active.skies[to] < 0 && active.sees(from, to as i16)).all(|to| {
            let portals = active.clusters.cluster_portals.get(to).map(|p| p.as_slice()).unwrap_or(&[]);
            portals.iter().all(|&p| active.clusters.portals.get(p as usize).is_none_or(|portal| self.cluster(portal.centroid) >= 0))
        })
    }

    /// Whether this BSP closes each opening onto the outdoors that `active`'s
    /// cluster `from` sees (the portals joining the clusters without sky it
    /// sees, or its own, to those with sky): a metre past each, this BSP has
    /// no cluster, its own wall stands there. (b30's control room, whose
    /// window BSP 1 closes with the shaft; where the other BSP's copy runs on
    /// past the opening into nothing it built, b40's tunnel out into the
    /// second canyon, from BSP 0, it would show the void the game doesn't:
    /// the game draws its own BSP's land there.)
    pub(crate) fn closes_openings_seen(&self, active: &ClusterVisibility, from: i16) -> bool {
        let interior = |k: i16| k >= 0 && (k == from || (active.skies.get(k as usize).is_some_and(|&sky| sky < 0) && active.sees(from, k)));
        active.clusters.portals.iter().all(|portal| {
            let [a, b] = portal.clusters;
            let outdoors = [(a, b), (b, a)].into_iter().find(|&(inside, outside)| {
                interior(inside) && outside >= 0 && active.skies.get(outside as usize).is_some_and(|&sky| sky >= 0) && active.sees(from, outside)
            });
            let Some((_, outside)) = outdoors else { return true };
            portal.past(1.0, |p| active.cluster(p) == outside).is_none_or(|p| self.cluster(p) < 0)
        })
    }

    /// Whether cluster `from` sees cluster `to` (`structure_bsp_get_cluster_pvs`).
    pub(crate) fn sees(&self, from: i16, to: i16) -> bool {
        let word = from as usize * self.words_per_cluster + to as usize / 32;
        self.pvs.get(word).is_none_or(|bits| bits & (1 << (to as u32 % 32)) != 0)
    }
}

/// The loaded level's BSPs' clusters, by BSP index (None where a BSP's
/// can't be read, or wasn't loaded).
#[derive(Resource, Default)]
pub struct LevelClusters(pub Vec<Option<Arc<ClusterVisibility>>>);

/// Where a BSP's emitter is: its BSP and cluster (the chunk of an emissive
/// material in that cluster, crate::level). Lights without one (the game's
/// lights, fixtures, objects' emitters) are placed by their position.
#[derive(Component, Clone, Copy, Debug)]
pub struct LightPlace {
    pub bsp: usize,
    pub cluster: i16,
}

/// A point in Bevy's space (metres, Y up) as Halo's (world units, Z up):
/// the inverse of `halo_to_bevy`.
fn bevy_to_halo(p: Vec3) -> [f32; 3] {
    [p.x / METERS_PER_WORLD_UNIT, -p.z / METERS_PER_WORLD_UNIT, p.y / METERS_PER_WORLD_UNIT]
}

/// Tells Solarik which lights are in sight of the camera's cluster, by the
/// active BSP's visibility sets: the BSP's emitters by their chunk's
/// cluster, the rest (the game's lights, fixtures' lights and emissive
/// proxies, objects' emitters) by their position's. A camera outside every
/// cluster (flying free beyond the walls) sees none, and selection stays as
/// it was. (`HALO_RT_NO_PVS_LIGHTS=1` for A/B: no lights seen.)
pub fn mark_seen_lights(
    clusters: Option<Res<LevelClusters>>,
    active: Res<crate::ActiveBsp>,
    camera: Single<&GlobalTransform, With<Camera3d>>,
    placed: Query<(Entity, &LightPlace)>,
    positioned: Query<(Entity, &GlobalTransform), (Or<(With<PointLight>, With<SpotLight>, With<EmissiveSurface>)>, Without<LightPlace>)>,
    mut seen: ResMut<SolarikSeenLights>,
    mut off: Local<Option<bool>>,
) {
    let off = *off.get_or_insert_with(|| std::env::var_os("HALO_RT_NO_PVS_LIGHTS").is_some());
    let visibility = match (&clusters, active.0) {
        (Some(clusters), Some(bsp)) if !off => clusters.0.get(bsp).and_then(|v| v.as_ref()).map(|v| (bsp, v)),
        _ => None,
    };
    let camera_cluster = visibility.map_or(-1, |(_, v)| v.cluster(bevy_to_halo(camera.translation())));
    let mut entities = bevy::platform::collections::HashSet::default();
    if let Some((bsp, visibility)) = visibility
        && camera_cluster >= 0
    {
        for (entity, place) in &placed {
            if place.bsp == bsp && place.cluster >= 0 && visibility.sees(camera_cluster, place.cluster) {
                entities.insert(entity);
            }
        }
        for (entity, transform) in &positioned {
            let cluster = visibility.cluster(bevy_to_halo(transform.translation()));
            if cluster >= 0 && visibility.sees(camera_cluster, cluster) {
                entities.insert(entity);
            }
        }
    }
    if seen.entities != entities {
        seen.entities = entities;
    }
}
