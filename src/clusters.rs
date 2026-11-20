//! A level's clusters and what they see: each BSP's collision tree (the
//! cluster a point is in), its clusters' portals and skies, and its
//! potentially visible sets (`structure_bsp_get_cluster_pvs`). The bridge
//! hides what the game's camera can't see by them (crate::bridge), and the
//! lights in sight of the camera's cluster get the larger share of the rays'
//! light samples ([`mark_seen_lights`]; Solarik's `SolarikSeenLights`): a
//! room lit by a few small strips had nine pixels in ten pick a light behind
//! a wall, and the shadow ray dropped it (b40's elevator shaft, 92%; the
//! lights' selection is global, by power, blind to where they shine).
//! (The BSPs' data and the rule for the BSP drawn in place of the game's are
//! halo_map::collision's, which the `halo_map` tool prints too: its `where`
//! and `skyless`.)

use crate::level::EmissiveSurface;
use bevy::prelude::*;
use bevy_solarik::scene::SolarikSeenLights;
use halo_map::math::METERS_PER_WORLD_UNIT;
use std::sync::Arc;

pub use halo_map::collision::ClusterVisibility;
use halo_map::collision::{OverrideDecision, OverrideRule};

/// The rule for the BSP drawn in place of the game's (crate::bridge
/// `DrawnBsp`), from `HALO_RT_BSP_OVERRIDE`: unset, the clusters' rule
/// while no opening the other BSP leaves undrawn is in the camera's sight;
/// `old` the clusters' rule alone (b30's Shaft A hall drew the void through
/// its door); `off` none, the game's BSP always.
pub(crate) fn override_rule_from_env() -> Option<OverrideRule> {
    match std::env::var("HALO_RT_BSP_OVERRIDE").ok().as_deref() {
        None | Some("") => Some(OverrideRule::Sight),
        Some("old") => Some(OverrideRule::Clusters),
        Some("off") => None,
        Some(other) => {
            warn!("HALO_RT_BSP_OVERRIDE={other}: not old or off; the sight rule");
            Some(OverrideRule::Sight)
        }
    }
}

/// How many frames in a row an opening that stopped the override must stay
/// out of the camera's sight before the override draws the other BSP again
/// ([`OverrideHold`]).
pub(crate) const OVERRIDE_HOLD_FRAMES: u32 = 30;

/// The override's hold: off at once when an unclosed opening comes into
/// sight, on again only after [`OVERRIDE_HOLD_FRAMES`] frames without one,
/// so the drawn BSP doesn't flicker as a sight ray grazes a pillar or the
/// door's frame.
pub(crate) struct OverrideHold {
    /// Frames since an opening was last in sight (at most the hold).
    clear: u32,
}

impl Default for OverrideHold {
    fn default() -> Self {
        OverrideHold { clear: OVERRIDE_HOLD_FRAMES }
    }
}

impl OverrideHold {
    /// The BSP drawn this frame, by this frame's decision.
    pub(crate) fn step(&mut self, decision: &OverrideDecision) -> Option<usize> {
        if decision.drawn.is_none() && decision.stopped.is_some() {
            self.clear = 0;
            return None;
        }
        self.clear = (self.clear + 1).min(OVERRIDE_HOLD_FRAMES);
        decision.drawn.filter(|_| self.clear >= OVERRIDE_HOLD_FRAMES)
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

#[cfg(test)]
mod tests {
    use super::*;

    const DRAWS: OverrideDecision = OverrideDecision { drawn: Some(1), stopped: None };
    const STOPPED: OverrideDecision = OverrideDecision { drawn: None, stopped: Some((1, 186, [0.0; 3])) };
    const NEITHER: OverrideDecision = OverrideDecision { drawn: None, stopped: None };

    #[test]
    fn hold_draws_at_once_when_nothing_was_in_sight() {
        let mut hold = OverrideHold::default();
        assert_eq!(hold.step(&DRAWS), Some(1));
        assert_eq!(hold.step(&NEITHER), None);
        assert_eq!(hold.step(&DRAWS), Some(1));
    }

    #[test]
    fn hold_waits_after_an_opening_in_sight() {
        let mut hold = OverrideHold::default();
        assert_eq!(hold.step(&DRAWS), Some(1));
        assert_eq!(hold.step(&STOPPED), None);
        for _ in 1..OVERRIDE_HOLD_FRAMES {
            assert_eq!(hold.step(&DRAWS), None);
        }
        assert_eq!(hold.step(&DRAWS), Some(1));
    }

    #[test]
    fn hold_starts_again_when_the_opening_is_grazed() {
        let mut hold = OverrideHold::default();
        hold.step(&STOPPED);
        for _ in 0..OVERRIDE_HOLD_FRAMES - 2 {
            assert_eq!(hold.step(&DRAWS), None);
        }
        assert_eq!(hold.step(&STOPPED), None);
        for _ in 1..OVERRIDE_HOLD_FRAMES {
            assert_eq!(hold.step(&DRAWS), None);
        }
        assert_eq!(hold.step(&DRAWS), Some(1));
        // (frames with no rule at all count as clear: outdoors, then back in)
        hold.step(&STOPPED);
        for _ in 0..OVERRIDE_HOLD_FRAMES {
            hold.step(&NEITHER);
        }
        assert_eq!(hold.step(&DRAWS), Some(1));
    }
}
