//! Which sun direction best explains a BSP's baked lightmaps: `lightmap_sun
//! <map> <bsp index> [terrain shader name parts, comma-joined]` (the parts
//! default to b40's cliffs). For checking a sky tag's sun against the light
//! the lightmapper baked (b40's tag says pitch 69 degrees; its lightmaps fit
//! 83).
//!
//! Every opaque render triangle of the BSP is an occluder. Points sampled on
//! the terrain materials carry their lightmap colour. For each candidate
//! direction a sample is lit if it faces the sun and no triangle covers it
//! from the sun's side (the scene sheared along the direction so the rays
//! are vertical, its triangles binned in a 2D grid); the direction scores the
//! Pearson correlation, over the floor samples, of the sun's light there (lit
//! times n.d) with the lightmap's luminance. A coarse grid of directions,
//! then a fine one round the best; the scenario's suns are scored at their
//! own direction for comparison.

/// Shaders that cast no shadow in the bake (glass, light cards, grates and
/// the like), by name part.
const SKIP: [&str; 10] =
    ["glass", "energyrope", "grate", "foglamp", "flashlight", "invis", "light_", "crevlight", "pitlights", "mirror"];
/// b40's canyon terrain.
const DEFAULT_TERRAIN: &str = "cliff_snow,cliff_wall,cliff_cmap,cliff_ice";
/// World units squared per sample, on average (a b40 BSP 30 000 to 100 000
/// samples).
const AREA_PER_SAMPLE: f32 = 2.0;
/// The occluder grid's cell size, world units.
const CELL: f32 = 3.0;
/// Samples whose normal's z is above this are floor; the score counts them
/// alone (walls are lit by their slope as much as by shadow).
const FLOOR: f32 = 0.6;
/// Sky lights wider than this (radians) are sky, not suns.
const SUN_MAX_DIAMETER: f32 = 0.1;

type Triangle = [[f32; 3]; 3];

/// A (yaw, pitch) in degrees and its (correlation, share of the floor lit).
type Scored = ((f32, f32), (f32, f32));

struct Sample {
    position: [f32; 3],
    normal: [f32; 3],
    /// The lightmap's luminance there (0..255).
    luminance: f32,
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!("usage: lightmap_sun <map> <bsp index> [terrain shader name parts, comma-joined]");
        std::process::exit(1);
    }
    let cache = halo_map::CacheFile::open(std::path::Path::new(&args[1])).unwrap();
    let only: usize = args[2].parse().unwrap();
    let parts: Vec<&str> = args.get(3).map_or(DEFAULT_TERRAIN, |a| a.as_str()).split(',').collect();
    let reference = halo_map::bsp::references(&cache).unwrap().into_iter().find(|r| r.index == only).unwrap();
    let bsp = halo_map::bsp::Bsp::load(&cache, &reference).unwrap();
    let (triangles, samples) = scene(&cache, &bsp, &parts);
    let floor = samples.iter().filter(|s| s.normal[2] > FLOOR).count();
    println!("{} triangles, {} samples ({floor} floor)", triangles.len(), samples.len());

    // coarse: pitch 30 to 90 by 3, yaw every 15
    let coarse: Vec<(f32, f32)> =
        (0..=20).flat_map(|p| (0..24).map(move |y| (y as f32 * 15.0, 30.0 + p as f32 * 3.0))).collect();
    let (best_yaw, best_pitch) = best(scored(&triangles, &samples, &coarse))[0].0;
    // fine: pitch by 1 within 3, yaw by 5 within 15, round the coarse best
    let fine: Vec<(f32, f32)> = (-3..=3)
        .map(|p| best_pitch + p as f32)
        .filter(|&p| p < 90.0)
        .flat_map(|p| (-3..=3).map(move |y| ((best_yaw + y as f32 * 5.0).rem_euclid(360.0), p)))
        .collect();
    println!("best directions:");
    for ((yaw, pitch), (correlation, lit)) in best(scored(&triangles, &samples, &fine)).iter().take(5) {
        println!("  yaw {yaw:5.1} pitch {pitch:4.1}: correlation {correlation:.3}, {:.0}% of the floor lit", lit * 100.0);
    }
    for sky in halo_map::sky::skies(&cache).unwrap() {
        for light in sky.lights.iter().filter(|l| l.diameter <= SUN_MAX_DIAMETER && l.power > 0.0) {
            let [x, y, z] = light.direction;
            let (correlation, lit) = score(&triangles, &samples, light.direction);
            println!(
                "{}'s sun, yaw {:.1} pitch {:.1}: correlation {correlation:.3}, {:.0}% of the floor lit",
                sky.name,
                y.atan2(x).to_degrees(),
                z.clamp(-1.0, 1.0).asin().to_degrees(),
                lit * 100.0
            );
        }
    }
}

/// The BSP's occluders and its terrain samples (shader name containing one
/// of `parts`).
fn scene(cache: &halo_map::CacheFile, bsp: &halo_map::bsp::Bsp, parts: &[&str]) -> (Vec<Triangle>, Vec<Sample>) {
    let lightmaps = bsp
        .lightmap_bitmaps()
        .unwrap()
        .datum
        .map(|d| halo_map::bitmap::bitmaps(cache, d).unwrap())
        .unwrap_or_default();
    let decoded: Vec<_> = lightmaps.iter().map(|b| b.decode_rgba8(cache).ok()).collect();
    let mut triangles = Vec::new();
    let mut samples = Vec::new();
    // (a fixed seed: the same samples every run)
    let mut seed: u32 = 12345;
    let mut random = move || {
        seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
        (seed >> 8) as f32 / 16777216.0
    };
    for material in &bsp.materials().unwrap() {
        let name = material.shader.datum.and_then(|d| cache.tag(d).ok()).map_or(String::new(), |t| t.name.clone());
        if SKIP.iter().any(|s| name.contains(s)) {
            continue;
        }
        let map = usize::try_from(material.lightmap_bitmap_index).ok().and_then(|i| decoded.get(i)).and_then(|d| d.as_ref());
        let terrain = parts.iter().any(|s| name.contains(s));
        for t in material.indices.chunks_exact(3) {
            let p = [0, 1, 2].map(|i| material.vertices[t[i] as usize].position);
            triangles.push(p);
            let Some(map) = map else { continue };
            if !terrain || material.lightmap_texcoords.len() != material.vertices.len() {
                continue;
            }
            let uv = [0, 1, 2].map(|i| material.lightmap_texcoords[t[i] as usize]);
            let sub = |a: [f32; 3], b: [f32; 3]| [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
            let (e, f) = (sub(p[1], p[0]), sub(p[2], p[0]));
            let cross = [e[1] * f[2] - e[2] * f[1], e[2] * f[0] - e[0] * f[2], e[0] * f[1] - e[1] * f[0]];
            let length = (cross[0] * cross[0] + cross[1] * cross[1] + cross[2] * cross[2]).sqrt();
            if length <= 0.0 {
                continue;
            }
            // (clockwise from the front: the cross product points back)
            let normal = cross.map(|x| -x / length);
            let count = (length / 2.0 / AREA_PER_SAMPLE + random()).floor() as usize;
            for _ in 0..count {
                // a uniform point in the triangle
                let (mut a, mut b) = (random(), random());
                if a + b > 1.0 {
                    a = 1.0 - a;
                    b = 1.0 - b;
                }
                let w = [1.0 - a - b, a, b];
                let position: [f32; 3] = std::array::from_fn(|c| w[0] * p[0][c] + w[1] * p[1][c] + w[2] * p[2][c]);
                let st = [0, 1].map(|c| w[0] * uv[0][c] + w[1] * uv[1][c] + w[2] * uv[2][c]);
                let [r, g, b] = bilinear(map, st);
                samples.push(Sample { position, normal, luminance: 0.3 * r + 0.59 * g + 0.11 * b });
            }
        }
    }
    (triangles, samples)
}

/// The lightmap's colour (0..255) at `st`, bilinear, clamped at the edges.
fn bilinear(map: &halo_map::bitmap::Decoded, st: [f32; 2]) -> [f32; 3] {
    let (fx, fy) = (st[0] * map.width as f32 - 0.5, st[1] * map.height as f32 - 0.5);
    let (x0, y0) = (fx.floor(), fy.floor());
    let (tx, ty) = (fx - x0, fy - y0);
    let texel = |x: f32, y: f32| {
        let xi = (x.max(0.0) as u32).min(map.width - 1);
        let yi = (y.max(0.0) as u32).min(map.height - 1);
        let o = ((yi * map.width + xi) * 4) as usize;
        [map.data[o] as f32, map.data[o + 1] as f32, map.data[o + 2] as f32]
    };
    let (c00, c10, c01, c11) = (texel(x0, y0), texel(x0 + 1.0, y0), texel(x0, y0 + 1.0), texel(x0 + 1.0, y0 + 1.0));
    std::array::from_fn(|c| (c00[c] * (1.0 - tx) + c10[c] * tx) * (1.0 - ty) + (c01[c] * (1.0 - tx) + c11[c] * tx) * ty)
}

/// Each (yaw, pitch) in degrees with its score, a thread a share of them.
fn scored(triangles: &[Triangle], samples: &[Sample], directions: &[(f32, f32)]) -> Vec<Scored> {
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get());
    let chunk = directions.len().div_ceil(threads).max(1);
    std::thread::scope(|scope| {
        let handles: Vec<_> = directions
            .chunks(chunk)
            .map(|share| {
                scope.spawn(move || {
                    share
                        .iter()
                        .map(|&(yaw, pitch)| {
                            let d = halo_map::sky::light_direction(yaw.to_radians(), pitch.to_radians());
                            ((yaw, pitch), score(triangles, samples, d))
                        })
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        handles.into_iter().flat_map(|h| h.join().unwrap()).collect()
    })
}

/// Best first.
fn best(mut scored: Vec<Scored>) -> Vec<Scored> {
    scored.sort_by(|a, b| b.1.0.total_cmp(&a.1.0));
    scored
}

/// A sun direction's (correlation, share of the floor lit): see the header.
/// A direction that lights no floor sample (no variance) correlates -1.
fn score(triangles: &[Triangle], samples: &[Sample], d: [f32; 3]) -> (f32, f32) {
    let lit = lit(triangles, samples, d);
    let (mut n, mut lit_count) = (0.0f64, 0.0f64);
    let (mut sx, mut sy, mut sxx, mut syy, mut sxy) = (0.0f64, 0.0f64, 0.0f64, 0.0f64, 0.0f64);
    for (sample, &lit) in samples.iter().zip(&lit) {
        if sample.normal[2] <= FLOOR {
            continue;
        }
        let facing = sample.normal[0] * d[0] + sample.normal[1] * d[1] + sample.normal[2] * d[2];
        let x = if lit { facing.max(0.0) as f64 } else { 0.0 };
        let y = sample.luminance as f64;
        n += 1.0;
        if lit {
            lit_count += 1.0;
        }
        sx += x;
        sy += y;
        sxx += x * x;
        syy += y * y;
        sxy += x * y;
    }
    let correlation = (sxy - sx * sy / n) / ((sxx - sx * sx / n) * (syy - sy * sy / n)).sqrt();
    let correlation = if correlation.is_finite() { correlation as f32 } else { -1.0 };
    (correlation, (lit_count / n.max(1.0)) as f32)
}

/// Per sample, whether the sun along `d` (unit, towards the sun) reaches it.
fn lit(triangles: &[Triangle], samples: &[Sample], d: [f32; 3]) -> Vec<bool> {
    // sheared along the direction, rays to the sun are vertical
    let (sx, sy) = (d[0] / d[2], d[1] / d[2]);
    let shear = |q: [f32; 3]| [q[0] - q[2] * sx, q[1] - q[2] * sy, q[2]];
    let sheared: Vec<Triangle> = triangles.iter().map(|t| t.map(shear)).collect();
    let (mut lo, mut hi) = ([f32::MAX; 2], [f32::MIN; 2]);
    for v in sheared.iter().flatten() {
        for c in 0..2 {
            lo[c] = lo[c].min(v[c]);
            hi[c] = hi[c].max(v[c]);
        }
    }
    let nx = ((hi[0] - lo[0]) / CELL) as usize + 1;
    let ny = ((hi[1] - lo[1]) / CELL) as usize + 1;
    let mut grid: Vec<Vec<u32>> = vec![Vec::new(); nx * ny];
    for (i, t) in sheared.iter().enumerate() {
        let low = |c: usize| ((t.iter().map(|v| v[c]).fold(f32::MAX, f32::min) - lo[c]) / CELL) as usize;
        let high = |c: usize| ((t.iter().map(|v| v[c]).fold(f32::MIN, f32::max) - lo[c]) / CELL) as usize;
        for gy in low(1)..=high(1).min(ny - 1) {
            for gx in low(0)..=high(0).min(nx - 1) {
                grid[gy * nx + gx].push(i as u32);
            }
        }
    }
    samples
        .iter()
        .map(|sample| {
            let (q, n) = (sample.position, sample.normal);
            if n[0] * d[0] + n[1] * d[1] + n[2] * d[2] <= 0.0 {
                return false;
            }
            // (lifted off its own face)
            let o = shear([q[0] + n[0] * 0.03, q[1] + n[1] * 0.03, q[2] + n[2] * 0.03]);
            let gx = ((o[0] - lo[0]) / CELL) as usize;
            let gy = ((o[1] - lo[1]) / CELL) as usize;
            if gx >= nx || gy >= ny {
                return true;
            }
            // lit unless a triangle over the point lies above it
            !grid[gy * nx + gx].iter().any(|&i| {
                let t = &sheared[i as usize];
                let den = (t[1][1] - t[2][1]) * (t[0][0] - t[2][0]) + (t[2][0] - t[1][0]) * (t[0][1] - t[2][1]);
                if den.abs() < 1e-9 {
                    return false;
                }
                let a = ((t[1][1] - t[2][1]) * (o[0] - t[2][0]) + (t[2][0] - t[1][0]) * (o[1] - t[2][1])) / den;
                let b = ((t[2][1] - t[0][1]) * (o[0] - t[2][0]) + (t[0][0] - t[2][0]) * (o[1] - t[2][1])) / den;
                let c = 1.0 - a - b;
                a >= 0.0 && b >= 0.0 && c >= 0.0 && a * t[0][2] + b * t[1][2] + c * t[2][2] > o[2] + 0.05
            })
        })
        .collect()
}
