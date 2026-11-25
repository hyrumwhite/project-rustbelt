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

/// Levels whose sun stands elsewhere than their sky tag says: (map, yaw,
/// pitch), degrees, in the tag's own terms. Their lightmaps were baked with
/// this sun (fitted against them by halo_map's `lightmap_sun` example); b40's
/// sky tag holds yaw 45, pitch 69, at which the canyon floors lie almost
/// wholly in the cliffs' shadow, where the lightmaps (and MCC) light most of
/// them and shade only near the cliffs. c40 (Two Betrayals, the same canyon
/// as b40 at night; its sky tag holds yaw 120, pitch 69) takes the same
/// offset from its tag as b40, the tag's yaw and 14 degrees more pitch, at
/// the user's word (14 November 2026: "can we move the sun in Two Betrayals
/// to the same offset used for Assault on the Control Room?"); its lightmaps
/// fit about yaw 120 to 150, pitch 84 to 87 by `lightmap_sun`.
/// `HALO_RT_SUN_DIRECTION=tag` the old.
const BAKED_SUNS: &[(&str, f32, f32)] = &[("b40", 45.0, 83.0), ("c40", 120.0, 83.0)];

/// The sun's yaw and pitch (degrees) on `map`, the sky tag's being `tag`:
/// [`BAKED_SUNS`]'s where it lists the map, else the tag's. `text` is
/// `HALO_RT_SUN_DIRECTION`: `tag` the tag's on every level (the old rule),
/// `yaw,pitch` that direction on any level (tuning).
fn sun_angles(map: &str, tag: (f32, f32), text: Option<&str>) -> (f32, f32) {
    match text.map(str::trim) {
        Some("tag") => return tag,
        Some(text) => {
            let tuned = text
                .split_once(',')
                .and_then(|(yaw, pitch)| Some((yaw.trim().parse().ok()?, pitch.trim().parse().ok()?)));
            match tuned {
                Some(angles) => return angles,
                None => warn!("HALO_RT_SUN_DIRECTION={text}: expected tag or yaw,pitch (degrees); ignored"),
            }
        }
        None => {}
    }
    BAKED_SUNS
        .iter()
        .find(|(name, _, _)| map.eq_ignore_ascii_case(name))
        .map_or(tag, |&(_, yaw, pitch)| (yaw, pitch))
}

impl SkyLighting {
    /// `map` is the map's name (`b40`), for [`BAKED_SUNS`].
    pub fn from_map(cache: &CacheFile, map: &str) -> Self {
        let skies = sky::skies(cache).unwrap_or_else(|e| {
            warn!("sky: {e}");
            Vec::new()
        });
        let lights: Vec<_> = skies.iter().flat_map(|s| &s.lights).collect();
        let sun = lights
            .iter()
            .filter(|l| l.diameter <= SUN_MAX_DIAMETER && l.power > 0.0)
            .max_by(|a, b| a.power.total_cmp(&b.power))
            .map(|l| {
                let [x, y, z] = l.direction;
                let tag = (y.atan2(x).to_degrees(), z.clamp(-1.0, 1.0).asin().to_degrees());
                let (yaw, pitch) = sun_angles(map, tag, std::env::var("HALO_RT_SUN_DIRECTION").ok().as_deref());
                if (yaw - tag.0).abs() > 0.01 || (pitch - tag.1).abs() > 0.01 {
                    info!("sun: yaw {yaw:.1} pitch {pitch:.1} (the sky tag's: {:.1}, {:.1})", tag.0, tag.1);
                } else {
                    info!("sun: yaw {yaw:.1} pitch {pitch:.1}");
                }
                let direction = sky::light_direction(yaw.to_radians(), pitch.to_radians());
                (Vec3::from(halo_to_bevy(direction, 1.0)).normalize(), color(l.color), l.power)
            });
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

/// How far above the horizon (the sine of the elevation) the sky model's
/// colour is taken for every direction below it.
const HORIZON: f32 = 0.03;

/// The sky model's cubemap's size (texels a face): water and glossy
/// surfaces reflect it (at 64 the ring blurred in the sea).
const SKY_CUBEMAP_SIZE: u32 = 512;

/// The sky model's layers, composited in every direction (see
/// [`halo_map::sky::SkyRenderer`]): the environment light's cubemap, and the
/// camera's skybox's.
pub struct SkyModelCubemaps {
    /// Below the horizon, the colour at the horizon (see [`HORIZON`]).
    pub lighting: Handle<Image>,
    /// The layers as they are in every direction, behind the drawn sky
    /// model: the lighting's showed through its translucent layers below the
    /// horizon, the horizon's colours streaked downwards (the stars of the
    /// menus' space).
    pub background: Handle<Image>,
}

pub fn sky_model_cubemap(cache: &CacheFile, model_datum: u32, images: &mut Assets<Image>) -> halo_map::Result<Option<SkyModelCubemaps>> {
    let renderer = halo_map::sky::SkyRenderer::new(cache, model_datum)?;
    if renderer.is_empty() {
        return Ok(None);
    }
    // Bevy (x, y, z) is Halo (x, -z, y); the composite is 8-bit gamma, which
    // the sRGB texture stores as is. Below the horizon, where the sky model
    // has nothing (the game fogs or never shows it), its colour at the
    // horizon: rays out through the sea saw black there.
    let started = std::time::Instant::now();
    let lighting = cubemap(SKY_CUBEMAP_SIZE, images, |dir| {
        let dir = if dir.y < HORIZON { Vec3::new(dir.x, 0.0, dir.z).normalize_or(Vec3::X).with_y(HORIZON).normalize() } else { dir };
        renderer.sample([dir.x, -dir.z, dir.y])
    });
    let background = cubemap(SKY_CUBEMAP_SIZE, images, |dir| renderer.sample([dir.x, -dir.z, dir.y]));
    info!("sky environment map: {SKY_CUBEMAP_SIZE} texels a face, twice, in {:.2} s", started.elapsed().as_secs_f32());
    Ok(Some(SkyModelCubemaps { lighting, background }))
}

/// A cube texel's direction in the texture's own terms (wgpu's face order
/// and orientation: +X, -X, +Y, -Y, +Z, -Z), `u` and `v` in -1..1 across the
/// face, not normalised.
fn face_direction(face: u32, u: f32, v: f32) -> Vec3 {
    match face {
        0 => Vec3::new(1.0, -v, -u),
        1 => Vec3::new(-1.0, -v, u),
        2 => Vec3::new(u, 1.0, v),
        3 => Vec3::new(u, -1.0, -v),
        4 => Vec3::new(u, -v, 1.0),
        _ => Vec3::new(-u, -v, -1.0),
    }
}

/// The world direction (Bevy coordinates) whose colour the cube texel at
/// `cube` (its direction in the texture's own terms, [`face_direction`])
/// must hold. Bevy's skybox and environment light and Solarik's sky
/// sampling all read a cube texture at the world direction with z negated
/// (cube textures are left-handed), so the texel at (x, y, z) is the one
/// seen along (x, y, -z). Filled by the cube direction itself (`handed`
/// false, `HALO_RT_SKY_CUBE_HANDED=0`, the old), every sky model's cubemap
/// came out mirrored front to back in Bevy terms (across Halo's x-z plane):
/// the main menu showed its gas giant twice, the drawn sky layer and its
/// mirror image in the skybox behind the layers, and every level's sky light
/// and reflections of the sky were mirrored likewise.
fn texel_world_direction(cube: Vec3, handed: bool) -> Vec3 {
    if handed { Vec3::new(cube.x, cube.y, -cube.z) } else { cube }
}

/// `HALO_RT_SKY_CUBE_HANDED`: anything but `0` (the default) fills the
/// cubemaps the way they are read (see [`texel_world_direction`]).
fn handed_cubes() -> bool {
    std::env::var("HALO_RT_SKY_CUBE_HANDED").map_or(true, |v| v.trim() != "0")
}

/// A cubemap whose texel colours (sRGB, 0..1) `colour` gives per direction
/// (Bevy coordinates, unit length): the direction from which the texel is
/// seen through Bevy's and Solarik's sampling (see [`texel_world_direction`]).
fn cubemap(size: u32, images: &mut Assets<Image>, colour: impl Fn(Vec3) -> [f32; 3] + Sync) -> Handle<Image> {
    let handed = handed_cubes();
    // (a face a thread)
    let face_texels = |face: u32| {
        let mut data = Vec::with_capacity((size * size * 4) as usize);
        for y in 0..size {
            for x in 0..size {
                let u = (x as f32 + 0.5) / size as f32 * 2.0 - 1.0;
                let v = (y as f32 + 0.5) / size as f32 * 2.0 - 1.0;
                let dir = texel_world_direction(face_direction(face, u, v).normalize(), handed);
                let [r, g, b] = colour(dir);
                data.extend([r, g, b, 1.0].map(|c| (c.clamp(0.0, 1.0) * 255.0).round() as u8));
            }
        }
        data
    };
    let data: Vec<u8> = std::thread::scope(|scope| {
        let faces: Vec<_> = (0..6).map(|face| scope.spawn(move || face_texels(face))).collect();
        faces.into_iter().flat_map(|face| face.join().unwrap()).collect()
    });
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

#[cfg(test)]
mod tests {
    use super::*;

    const TAG: (f32, f32) = (45.0, 69.0);

    #[test]
    fn baked_sun_replaces_the_tags() {
        assert_eq!(sun_angles("b40", TAG, None), (45.0, 83.0));
        assert_eq!(sun_angles("B40", TAG, None), (45.0, 83.0));
        assert_eq!(sun_angles("c40", (120.0, 69.0), None), (120.0, 83.0));
    }

    #[test]
    fn other_levels_keep_the_tags() {
        assert_eq!(sun_angles("b30", (50.0, 35.0), None), (50.0, 35.0));
    }

    #[test]
    fn tag_switch_keeps_the_tags() {
        assert_eq!(sun_angles("b40", TAG, Some("tag")), TAG);
    }

    #[test]
    fn tuned_direction_on_any_level() {
        assert_eq!(sun_angles("b40", TAG, Some("10,20")), (10.0, 20.0));
        assert_eq!(sun_angles("a10", TAG, Some(" 10 , 20 ")), (10.0, 20.0));
    }

    /// The face and texel direction a GPU reads for cube-space direction
    /// `d` (wgpu's / Vulkan's selection by the major axis), inverting
    /// [`face_direction`].
    fn addressed_texel(d: Vec3) -> Vec3 {
        let a = d.abs();
        let (face, u, v) = if a.x >= a.y && a.x >= a.z {
            if d.x > 0.0 { (0, -d.z / a.x, -d.y / a.x) } else { (1, d.z / a.x, -d.y / a.x) }
        } else if a.y >= a.z {
            if d.y > 0.0 { (2, d.x / a.y, d.z / a.y) } else { (3, d.x / a.y, -d.z / a.y) }
        } else if d.z > 0.0 {
            (4, d.x / a.z, -d.y / a.z)
        } else {
            (5, -d.x / a.z, -d.y / a.z)
        };
        face_direction(face, u, v).normalize()
    }

    #[test]
    fn sampled_texel_holds_the_world_direction() {
        // (Threshold's direction on the menu, in Bevy terms, and others)
        for world in [Vec3::new(0.85, 0.20, -0.45), Vec3::new(-0.3, 0.9, 0.2), Vec3::new(0.1, -0.2, 0.97), Vec3::new(-0.7, 0.1, -0.7)] {
            let world = world.normalize();
            // (as Bevy's skybox and Solarik's sample_sky read it)
            let texel = addressed_texel(Vec3::new(world.x, world.y, -world.z));
            assert!(texel_world_direction(texel, true).distance(world) < 1e-5, "{world}");
        }
    }

    #[test]
    fn old_rule_mirrors_front_to_back() {
        let world = Vec3::new(0.85, 0.20, -0.45).normalize();
        let texel = addressed_texel(Vec3::new(world.x, world.y, -world.z));
        assert!(texel_world_direction(texel, false).distance(Vec3::new(world.x, world.y, -world.z)) < 1e-5);
    }

    #[test]
    fn garbage_is_ignored() {
        assert_eq!(sun_angles("b40", TAG, Some("high")), (45.0, 83.0));
        assert_eq!(sun_angles("b30", TAG, Some("10,x")), TAG);
    }
}
