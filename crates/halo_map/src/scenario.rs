//! The scenario: what a level places in its BSPs.

use crate::{layout, CacheFile, Result};

/// `struct player_starting_location`.
#[derive(Debug, Clone)]
pub struct PlayerStart {
    pub position: [f32; 3],
    /// Yaw in radians, counter-clockwise from +x about +z.
    pub facing: f32,
    /// The BSP this start is in; -1 for any.
    pub bsp_index: i16,
}

pub fn player_starts(cache: &CacheFile) -> Result<Vec<PlayerStart>> {
    use layout::player_starting_location as p;
    let scenario = cache.scenario()?;
    scenario
        .block(layout::scenario::PLAYERS, p::SIZEOF)?
        .iter()
        .map(|start| {
            Ok(PlayerStart {
                position: start.vec3(p::POSITION)?,
                facing: start.f32(p::FACING)?,
                bsp_index: start.i16(p::STRUCTURE_BSP_REFERENCE_INDEX)?,
            })
        })
        .collect()
}
