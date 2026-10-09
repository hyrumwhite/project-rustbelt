//! The level's sun and sky light, from the scenario's sky tag.
//!
//! Halo skies list the distant lights the lightmapper used: a small-diameter
//! sun and usually a wide hemisphere "sky" light. The sun becomes a
//! directional light. The sky light becomes an environment light, which
//! Solari samples as sky lighting: a cubemap of the sky model itself (its
//! layers composited on the CPU, see [`halo_map::sky::SkyRenderer`]), or a
//! gradient of the sky light's colour for skies without a model.

use bevy::{
    asset::RenderAssetUsages,
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat, TextureViewDescriptor, TextureViewDimension},
};
use halo_map::{math::halo_to_bevy, sky, CacheFile};

/// Halo light diameters above this (radians) are area/sky lights, not suns.
const SUN_MAX_DIAMETER: f32 = 0.1;

#[derive(Clone, Debug)]
pub struct SkyLighting {
    /// Direction towards the sun (Bevy coordinates), its colour and Halo power.
    pub sun: Option<(Vec3, Color, f32)>,
    /// The sky's colour and Halo power.
    pub sky: Option<(Color, f32)>,
}

impl SkyLighting {
    pub fn from_map(cache: &CacheFile) -> Self {
        let skies = sky::skies(cache).unwrap_or_else(|e| {
            warn!("sky: {e}");
            Vec::new()
        });
        let lights: Vec<_> = skies.iter().flat_map(|s| &s.lights).collect();
        let sun = lights
            .iter()
            .filter(|l| l.diameter <= SUN_MAX_DIAMETER && l.power > 0.0)
            .max_by(|a, b| a.power.total_cmp(&b.power))
            .map(|l| (Vec3::from(halo_to_bevy(l.direction, 1.0)).normalize(), color(l.color), l.power));
        let sky = lights
            .iter()
            .filter(|l| l.diameter > SUN_MAX_DIAMETER && l.power > 0.0)
            .max_by(|a, b| a.power.total_cmp(&b.power))
            .map(|l| (color(l.color), l.power))
            // Some skies (Blood Gulch's) light the level with the sun alone and
            // leave the rest to the sky model's look; until the sky model is
            // drawn (ROADMAP phase 2), stand in a blue sky at half the sun's power.
            .or_else(|| {
                sun.map(|(_, sun_color, power)| (sun_color.mix(&Color::srgb(0.55, 0.68, 0.9), 0.65), power * 0.5))
            });
        SkyLighting { sun, sky }
    }
}

fn color([r, g, b]: [f32; 3]) -> Color {
    Color::srgb(r, g, b)
}

/// A small cubemap: `sky` above the horizon fading to a dim ground below.
pub fn gradient_cubemap(sky: Color, images: &mut Assets<Image>) -> Handle<Image> {
    let sky = sky.to_linear();
    let ground = LinearRgba::rgb(sky.red * 0.12, sky.green * 0.11, sky.blue * 0.1);
    cubemap(16, images, |dir| {
        let t = dir.y.clamp(-1.0, 1.0);
        let c = if t >= 0.0 {
            let k = 0.55 + 0.45 * t;
            LinearRgba::rgb(sky.red * k, sky.green * k, sky.blue * k)
        } else {
            ground
        };
        let srgb = Color::from(c).to_srgba();
        [srgb.red, srgb.green, srgb.blue]
    })
}

/// The sky model's layers, composited in every direction (see
/// [`halo_map::sky::SkyRenderer`]).
pub fn sky_model_cubemap(cache: &CacheFile, model_datum: u32, images: &mut Assets<Image>) -> halo_map::Result<Option<Handle<Image>>> {
    let renderer = halo_map::sky::SkyRenderer::new(cache, model_datum)?;
    if renderer.is_empty() {
        return Ok(None);
    }
    // Bevy (x, y, z) is Halo (x, -z, y); the composite is 8-bit gamma, which
    // the sRGB texture stores as is
    Ok(Some(cubemap(64, images, |dir| renderer.sample([dir.x, -dir.z, dir.y]))))
}

/// A cubemap whose texel colours (sRGB, 0..1) `colour` gives per direction
/// (Bevy coordinates, unit length).
fn cubemap(size: u32, images: &mut Assets<Image>, colour: impl Fn(Vec3) -> [f32; 3]) -> Handle<Image> {
    let mut data = Vec::with_capacity((size * size * 6 * 4) as usize);
    for face in 0..6 {
        for y in 0..size {
            for x in 0..size {
                let u = (x as f32 + 0.5) / size as f32 * 2.0 - 1.0;
                let v = (y as f32 + 0.5) / size as f32 * 2.0 - 1.0;
                // wgpu cube face order and orientation: +X, -X, +Y, -Y, +Z, -Z
                let dir = match face {
                    0 => Vec3::new(1.0, -v, -u),
                    1 => Vec3::new(-1.0, -v, u),
                    2 => Vec3::new(u, 1.0, v),
                    3 => Vec3::new(u, -1.0, -v),
                    4 => Vec3::new(u, -v, 1.0),
                    _ => Vec3::new(-u, -v, -1.0),
                }
                .normalize();
                let [r, g, b] = colour(dir);
                data.extend([r, g, b, 1.0].map(|c| (c.clamp(0.0, 1.0) * 255.0).round() as u8));
            }
        }
    }
    let mut image = Image::new(
        Extent3d { width: size, height: size, depth_or_array_layers: 6 },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.texture_view_descriptor =
        Some(TextureViewDescriptor { dimension: Some(TextureViewDimension::Cube), ..default() });
    images.add(image)
}
