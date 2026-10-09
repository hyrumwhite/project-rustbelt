//! Skies (`sky ` tags): the scenario's backdrop and its distant lights.
//!
//! The game reads only a sky light's lens flare at run time; the lightmapper
//! used the rest (colour, power, direction), which the decompilation leaves as
//! `unused30[0x44]` in `struct sky_light`. Those fields are read here by the
//! tag definition's layout (`sky_light_block` in the Halo Editing Kit), which
//! fills that space.

use crate::{groups, layout, CacheFile, Result, TagRef};

#[derive(Debug, Clone)]
pub struct SkyLight {
    pub color: [f32; 3],
    pub power: f32,
    /// Unit vector from the world towards the light, in Halo coordinates.
    pub direction: [f32; 3],
    /// Angular diameter in radians.
    pub diameter: f32,
}

#[derive(Debug, Clone)]
pub struct Sky {
    pub name: String,
    pub model: Option<TagRef>,
    pub lights: Vec<SkyLight>,
}

/// Offsets inside `sky_light` beyond the decompiled fields.
const LIGHT_COLOR: usize = 0x50;
const LIGHT_POWER: usize = 0x5C;
const LIGHT_DIRECTION: usize = 0x68;
const LIGHT_DIAMETER: usize = 0x70;

/// The scenario's skies.
pub fn skies(cache: &CacheFile) -> Result<Vec<Sky>> {
    let scenario = cache.scenario()?;
    let references = scenario.block(layout::scenario::SKY_REFERENCES, layout::tag_reference::SIZEOF)?;
    let mut out = Vec::new();
    for reference in references.iter() {
        let Some(datum) = reference.reference(0)?.datum else { continue };
        let root = cache.tag_root(datum, groups::SKY)?;
        let model = root.reference(layout::sky_definition::MODEL)?;
        let lights = root
            .block(layout::sky_definition::LIGHTS, layout::sky_light::SIZEOF)?
            .iter()
            .map(|light| {
                let [yaw, pitch] = light.vec2(LIGHT_DIRECTION)?;
                Ok(SkyLight {
                    color: light.vec3(LIGHT_COLOR)?,
                    power: light.f32(LIGHT_POWER)?,
                    direction: [pitch.cos() * yaw.cos(), pitch.cos() * yaw.sin(), pitch.sin()],
                    diameter: light.f32(LIGHT_DIAMETER)?,
                })
            })
            .collect::<Result<_>>()?;
        out.push(Sky { name: cache.tag(datum)?.name.clone(), model: model.datum.map(|_| model), lights });
    }
    Ok(out)
}
