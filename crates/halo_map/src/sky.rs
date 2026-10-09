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

/// A sky model's look in any direction, composited on the CPU: each layer
/// (model part, in order) is a transparent shader whose combiner program is
/// evaluated where the view ray meets it, then blended by its framebuffer
/// blend function, as the game draws the sky around the camera. Used to make
/// the environment map that lights the level.
pub struct SkyRenderer {
    layers: Vec<SkyLayer>,
}

struct SkyLayer {
    /// Positions and texture coordinates per corner.
    triangles: Vec<[([f32; 3], [f32; 2]); 3]>,
    program: crate::combiner::CombinerProgram,
    /// Per stage: the map's texels, its scale and offset.
    maps: [Option<(crate::bitmap::Decoded, [f32; 2], [f32; 2])>; 4],
    blend: crate::shader::BlendFunction,
}

impl SkyRenderer {
    pub fn new(cache: &CacheFile, model_datum: u32) -> Result<Self> {
        use crate::{bitmap, combiner, model, shader};
        let model = model::Model::load(cache, model_datum)?;
        let mut layers = Vec::new();
        for geometry in model.geometries_for(&[], model::HIGHEST_DETAIL) {
            for part in model.geometry(geometry)? {
                let Some(shader_ref) = model.shaders.get(part.shader_index.max(0) as usize).copied() else { continue };
                let Some(info) = shader::shader_info(cache, shader_ref)? else { continue };
                let (Some(transparent), Some(program)) = (info.transparent.as_ref(), combiner::program(cache, shader_ref)?)
                else {
                    continue;
                };
                let mut maps: [Option<_>; 4] = Default::default();
                for (stage, slot) in transparent.maps.iter().enumerate().take(4) {
                    // a cube map first stage reads black, as on the GPU side
                    if stage == 0 && transparent.first_map_type != 0 {
                        continue;
                    }
                    let Some(map) = slot else { continue };
                    let Some(datum) = map.bitmap.datum else { continue };
                    let decoded = bitmap::bitmaps(cache, datum)?.first().map(|b| b.decode_rgba8(cache)).transpose()?;
                    maps[stage] = decoded.map(|d| (d, map.uv_scale, map.uv_offset));
                }
                let triangles = part
                    .indices
                    .chunks_exact(3)
                    .map(|t| {
                        std::array::from_fn(|i| {
                            let v = &part.vertices[t[i] as usize];
                            (v.position, v.texcoord)
                        })
                    })
                    .collect();
                layers.push(SkyLayer { triangles, program, maps, blend: transparent.blend });
            }
        }
        Ok(SkyRenderer { layers })
    }

    pub fn is_empty(&self) -> bool {
        self.layers.is_empty()
    }

    /// The sky's colour along `direction` (Halo coordinates, from the sky's
    /// centre), 8-bit gamma 0..1, over black.
    pub fn sample(&self, direction: [f32; 3]) -> [f32; 3] {
        use crate::shader::BlendFunction;
        let mut colour = [0.0f32; 3];
        for layer in &self.layers {
            let Some(uv) = layer.hit(direction) else { continue };
            let textures = std::array::from_fn(|stage| match &layer.maps[stage] {
                Some((texels, scale, offset)) => sample_wrapped(texels, [uv[0] * scale[0] + offset[0], uv[1] * scale[1] + offset[1]]),
                None => [0.0; 4],
            });
            let [r, g, b, a] = crate::combiner::evaluate(&layer.program, textures);
            let src = [r, g, b];
            for i in 0..3 {
                colour[i] = match layer.blend {
                    BlendFunction::AlphaBlend => colour[i] * (1.0 - a) + src[i] * a,
                    BlendFunction::Multiply | BlendFunction::ComponentMin => colour[i] * src[i],
                    BlendFunction::DoubleMultiply => colour[i] * src[i] * 2.0,
                    BlendFunction::Add => colour[i] + src[i],
                    BlendFunction::Subtract => colour[i] - src[i],
                    BlendFunction::ComponentMax => colour[i].max(src[i]),
                    BlendFunction::AlphaMultiplyAdd => colour[i] * (1.0 - a) + src[i],
                }
                .clamp(0.0, 1.0);
            }
        }
        colour
    }
}

impl SkyLayer {
    /// Texture coordinates where a ray from the origin first meets this
    /// layer (Moller-Trumbore), if it does.
    fn hit(&self, direction: [f32; 3]) -> Option<[f32; 2]> {
        let sub = |a: [f32; 3], b: [f32; 3]| [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
        let cross = |a: [f32; 3], b: [f32; 3]| {
            [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
        };
        let dot = |a: [f32; 3], b: [f32; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
        let mut best: Option<(f32, [f32; 2])> = None;
        for [(p0, t0), (p1, t1), (p2, t2)] in &self.triangles {
            let (e1, e2) = (sub(*p1, *p0), sub(*p2, *p0));
            let p = cross(direction, e2);
            let det = dot(e1, p);
            if det.abs() < 1e-12 {
                continue;
            }
            let inv = 1.0 / det;
            let s = sub([0.0; 3], *p0);
            let u = dot(s, p) * inv;
            if !(0.0..=1.0).contains(&u) {
                continue;
            }
            let q = cross(s, e1);
            let v = dot(direction, q) * inv;
            if v < 0.0 || u + v > 1.0 {
                continue;
            }
            let t = dot(e2, q) * inv;
            if t <= 0.0 || best.is_some_and(|(bt, _)| bt <= t) {
                continue;
            }
            let w = 1.0 - u - v;
            best = Some((t, [w * t0[0] + u * t1[0] + v * t2[0], w * t0[1] + u * t1[1] + v * t2[1]]));
        }
        best.map(|(_, uv)| uv)
    }
}

/// Nearest texel, wrapping, as 0..1 RGBA.
fn sample_wrapped(texels: &crate::bitmap::Decoded, uv: [f32; 2]) -> [f32; 4] {
    let (w, h) = (texels.width as i64, texels.height as i64);
    let x = ((uv[0] * w as f32).floor() as i64).rem_euclid(w);
    let y = ((uv[1] * h as f32).floor() as i64).rem_euclid(h);
    let o = ((y * w + x) * 4) as usize;
    std::array::from_fn(|i| texels.data[o + i] as f32 / 255.0)
}
