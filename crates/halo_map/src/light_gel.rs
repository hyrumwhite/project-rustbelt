//! A light tag's gel as beams. The game masks a light by its gel, a cube map
//! round it turned to the light's forward and up (rasterizer_xbox_
//! environment.c): a10's alarm lights shine every way through one with two
//! opposite discs, so that their spinning lamp sweeps two beams round the
//! room. A renderer without the gel's lookup can draw each bright patch of
//! it as a spot light: [`gel_beams`].

use crate::{bitmap, groups, CacheFile, Result};

/// `light_definition.gel_map` (light_definitions.h).
const GEL_MAP: usize = 0x64;

/// One bright patch of a gel.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GelBeam {
    /// Its axis in the light's frame: along its forward, left and up.
    pub direction: [f32; 3],
    /// The angle (radians, from the axis) to which the gel stays at least
    /// half its brightest, and the angle beyond which it is dark.
    pub falloff_angle: f32,
    pub cutoff_angle: f32,
    /// Its brightest (0..1: how much of the light it lets through).
    pub strength: f32,
}

/// Texels at most this share of the brightest are dark.
const DARK: f32 = 0.02;
/// A patch dimmer than this share of the strongest's light is left out (the
/// alarm gel's two faint spots beside its beams).
const FAINT: f32 = 0.15;
/// Texels within this angle (radians) of a patch's brightest are its.
const PATCH_ANGLE: f32 = 1.3;

/// The light tag `light`'s gel, as up to four beams, strongest first; none
/// for a light without a gel (or one that is dark).
pub fn gel_beams(cache: &CacheFile, light: u32) -> Result<Vec<GelBeam>> {
    let root = cache.tag_root(light, groups::LIGHT)?;
    let Some(gel) = root.reference(GEL_MAP)?.datum else { return Ok(Vec::new()) };
    let Some(cube) = bitmap::bitmaps(cache, gel)?.into_iter().next().filter(|b| b.kind == bitmap::Kind::CubeMap) else {
        return Ok(Vec::new());
    };
    let faces = cube.decode_cube_faces_rgba8(cache)?;
    // each texel's direction (the light's frame), brightness and solid angle
    let (w, h) = (cube.width as usize, cube.height as usize);
    let mut texels = Vec::with_capacity(6 * w * h);
    for (face, decoded) in faces.iter().enumerate() {
        for y in 0..h {
            for x in 0..w {
                let o = (y * w + x) * 4;
                let brightness = decoded.data[o..o + 3].iter().copied().max().unwrap_or(0) as f32 / 255.0;
                let (s, t) = (2.0 * (x as f32 + 0.5) / w as f32 - 1.0, 2.0 * (y as f32 + 0.5) / h as f32 - 1.0);
                // (Direct3D's faces: +X -X +Y -Y +Z -Z)
                let c = match face {
                    0 => [1.0, -t, -s],
                    1 => [-1.0, -t, s],
                    2 => [s, 1.0, t],
                    3 => [s, -1.0, -t],
                    4 => [s, -t, 1.0],
                    _ => [-s, -t, -1.0],
                };
                let length2 = c[0] * c[0] + c[1] * c[1] + c[2] * c[2];
                let length = length2.sqrt();
                let solid_angle = 4.0 / (w * h) as f32 / (length2 * length);
                // (along the light's forward, left and up: the flashlights'
                // discs lie ahead)
                texels.push(([c[0] / length, c[1] / length, c[2] / length], brightness, solid_angle));
            }
        }
    }
    let peak = texels.iter().map(|t| t.1).fold(0.0, f32::max);
    if peak <= 0.0 {
        return Ok(Vec::new());
    }
    let dot = |a: [f32; 3], b: [f32; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let mut taken = vec![false; texels.len()];
    let mut beams: Vec<(GelBeam, f32)> = Vec::new();
    while beams.len() < 4 {
        let Some(brightest) = (0..texels.len()).filter(|&i| !taken[i] && texels[i].1 > DARK * peak).max_by(|&a, &b| texels[a].1.total_cmp(&texels[b].1)) else {
            break;
        };
        let (centre, strength, _) = texels[brightest];
        let members: Vec<usize> = (0..texels.len())
            .filter(|&i| !taken[i] && texels[i].1 > DARK * peak && dot(texels[i].0, centre) > PATCH_ANGLE.cos())
            .collect();
        let mut axis = [0.0f32; 3];
        let mut power = 0.0;
        for &i in &members {
            let (d, b, a) = texels[i];
            for k in 0..3 {
                axis[k] += d[k] * b * a;
            }
            power += b * a;
            taken[i] = true;
        }
        let length = dot(axis, axis).sqrt();
        if length <= 0.0 {
            break;
        }
        let axis = axis.map(|v| v / length);
        // its profile: the mean brightness at each angle from the axis (by
        // degree), out to where it first falls below a share of its brightest
        let mut sums = [(0.0f32, 0.0f32); 91];
        for &i in &members {
            let angle = dot(texels[i].0, axis).clamp(-1.0, 1.0).acos().to_degrees() as usize;
            if let Some(sum) = sums.get_mut(angle) {
                sum.0 += texels[i].1 * texels[i].2;
                sum.1 += texels[i].2;
            }
        }
        let profile: Vec<f32> = sums.iter().map(|(b, a)| if *a > 0.0 { b / a } else { 0.0 }).collect();
        let top = profile.iter().copied().fold(0.0, f32::max);
        let until = |share: f32| {
            let first = profile.iter().position(|&b| b > 0.0).unwrap_or(0);
            let end = profile.iter().skip(first).position(|&b| b < share * top).map_or(90, |n| first + n);
            (end as f32).to_radians()
        };
        let beam = GelBeam { direction: axis, falloff_angle: until(0.5), cutoff_angle: until(0.1), strength };
        beams.push((beam, power));
    }
    let strongest = beams.iter().map(|b| b.1).fold(0.0, f32::max);
    Ok(beams.into_iter().filter(|b| b.1 >= FAINT * strongest).map(|b| b.0).collect())
}
