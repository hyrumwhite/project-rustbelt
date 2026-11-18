//! How a BSP's sunlight falls on its faces, drawn from one camera, to set
//! faceted ray traced shadows beside the baked lightmaps: `cliff_shading
//! <map> <bsp index> <x,y,z,yaw,pitch> <out prefix> [sun yaw,pitch | tag]
//! [terrain shader name parts, comma-joined]` (the camera as the viewer's
//! `--camera`, Halo world units and degrees; the sun defaults to the
//! scenario's brightest small sky light, the parts to `cliff`).
//!
//! Writes `<prefix>_<mode>.png`, 1280x720, each but the first a grey picture
//! of the sun's term alone (0.25 ambient + 0.75 sun):
//! - `lightmap`: the baked lightmap, as the game draws it.
//! - `smooth`: the vertex normals' n.l, no shadows.
//! - `traced`: what a shadow ray from the face gives: dark where the face is
//!   turned from the sun or a triangle covers it, else the vertex normals' n.l.
//! - `shifted`: the ray's origin moved to the smooth surface the vertex
//!   normals describe (Hanika, "Hacking the shadow terminator", 2021).
//! - `fronts`: the ray started behind a face turned from the sun and stopped
//!   only by triangles it meets from their front.
//! - `sun2`, `sun5`, `sun10`: `traced` under a sun disc of that many degrees
//!   across (24 directions).
//! - `blur03`, `blur06`, `blur12`: `traced`'s shadow (not its n.l) averaged
//!   over the surface within that many tenths of a world unit, as a lightmap's
//!   texels do (gathered on screen, so only from what the camera sees).
//! - `shifted_sun5`: `shifted` under the 5 degree sun.
//! - `soft03`, `soft06`, `soft12`: `fronts` from 32 points of a disc of that
//!   radius in the face's own plane, just under it (a shortcut tried for the
//!   realtime blur: no better than `fronts`, light leaking under overhangs).
//! - `fblur03`, `fblur06`, `fblur12`: `fronts`' shadow averaged as the blurs
//!   average `traced`'s: the blur and the terminator rule together.
//! `REACH=<world units>` in the environment: a face's back further along the
//! ray than that stops a `fronts` ray too (the rock it started in is that
//! thick: the underside of an overhang, not a bulge).
//! And prints, over the pixels of the named terrain shaders, each mode's
//! correlation with the lightmap's luminance, and the terrain's normals'
//! statistics.

const SKIP: [&str; 10] =
    ["glass", "energyrope", "grate", "foglamp", "flashlight", "invis", "light_", "crevlight", "pitlights", "mirror"];
const SUN_MAX_DIAMETER: f32 = 0.1;
const CELL: f32 = 3.0;
const WIDTH: usize = 1280;
const HEIGHT: usize = 720;
/// Vertical field of view, degrees (the game's 70 across at 4:3).
const FOV: f32 = 55.4;
const NEAR: f32 = 0.05;
/// A ray's lift off its face, world units.
const LIFT: f32 = 0.01;
/// The wide suns' diameters, degrees, and the directions sampled for each.
const SUNS: [f32; 3] = [2.0, 5.0, 10.0];
const SUN_SAMPLES: usize = 24;
/// The blurs' radii, world units, and the screen samples gathered for each.
const BLURS: [f32; 3] = [0.3, 0.6, 1.2];
const BLUR_SAMPLES: usize = 96;

type V3 = [f32; 3];

/// The origins tried for each pixel of the `soft` modes.
const SOFT_SAMPLES: usize = 32;

fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn mul(a: V3, s: f32) -> V3 {
    [a[0] * s, a[1] * s, a[2] * s]
}
fn dot(a: V3, b: V3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: V3, b: V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn unit(a: V3) -> V3 {
    let l = dot(a, a).sqrt();
    if l > 0.0 { mul(a, 1.0 / l) } else { a }
}

struct Face {
    p: [V3; 3],
    n: [V3; 3],
    /// The face's own normal.
    g: V3,
    uv: Option<[[f32; 2]; 3]>,
    map: Option<usize>,
    terrain: bool,
    occluder: bool,
}

/// The sheared grid of `lightmap_sun`: rays to the sun are vertical.
struct Grid {
    sx: f32,
    sy: f32,
    lo: [f32; 2],
    nx: usize,
    ny: usize,
    cells: Vec<Vec<u32>>,
    sheared: Vec<[V3; 3]>,
    /// Whether a ray to the sun meets the face from its front.
    front: Vec<bool>,
}

impl Grid {
    fn new(faces: &[Face], d: V3) -> Self {
        let (sx, sy) = (d[0] / d[2], d[1] / d[2]);
        let shear = |q: V3| [q[0] - q[2] * sx, q[1] - q[2] * sy, q[2]];
        let sheared: Vec<[V3; 3]> = faces.iter().map(|f| f.p.map(shear)).collect();
        let (mut lo, mut hi) = ([f32::MAX; 2], [f32::MIN; 2]);
        for v in sheared.iter().flatten() {
            for c in 0..2 {
                lo[c] = lo[c].min(v[c]);
                hi[c] = hi[c].max(v[c]);
            }
        }
        let nx = ((hi[0] - lo[0]) / CELL) as usize + 1;
        let ny = ((hi[1] - lo[1]) / CELL) as usize + 1;
        let mut cells: Vec<Vec<u32>> = vec![Vec::new(); nx * ny];
        for (i, t) in sheared.iter().enumerate() {
            if !faces[i].occluder {
                continue;
            }
            let low = |c: usize| ((t.iter().map(|v| v[c]).fold(f32::MAX, f32::min) - lo[c]) / CELL) as usize;
            let high = |c: usize| ((t.iter().map(|v| v[c]).fold(f32::MIN, f32::max) - lo[c]) / CELL) as usize;
            for gy in low(1)..=high(1).min(ny - 1) {
                for gx in low(0)..=high(0).min(nx - 1) {
                    cells[gy * nx + gx].push(i as u32);
                }
            }
        }
        let front = faces.iter().map(|f| dot(f.g, d) < 0.0).collect();
        Grid { sx, sy, lo, nx, ny, cells, sheared, front }
    }

    /// Whether a triangle covers `origin` from the sun's side: any triangle,
    /// or with `fronts_only` those the ray meets from their front. `skip` is
    /// the face the ray leaves.
    fn covered(&self, origin: V3, fronts_only: bool, skip: usize) -> bool {
        let o = [origin[0] - origin[2] * self.sx, origin[1] - origin[2] * self.sy, origin[2]];
        if o[0] < self.lo[0] || o[1] < self.lo[1] {
            return false;
        }
        let gx = ((o[0] - self.lo[0]) / CELL) as usize;
        let gy = ((o[1] - self.lo[1]) / CELL) as usize;
        if gx >= self.nx || gy >= self.ny {
            return false;
        }
        for &i in &self.cells[gy * self.nx + gx] {
            let i = i as usize;
            if i == skip {
                continue;
            }
            // (REACH=<world units>: a back the ray meets further than that
            // stops it too, the rock it started in being that thick)
            let back = fronts_only && !self.front[i];
            if back && reach() <= 0.0 {
                continue;
            }
            let [a, b, c] = self.sheared[i];
            let det = (b[1] - c[1]) * (a[0] - c[0]) + (c[0] - b[0]) * (a[1] - c[1]);
            if det.abs() < 1e-12 {
                continue;
            }
            let u = ((b[1] - c[1]) * (o[0] - c[0]) + (c[0] - b[0]) * (o[1] - c[1])) / det;
            let v = ((c[1] - a[1]) * (o[0] - c[0]) + (a[0] - c[0]) * (o[1] - c[1])) / det;
            let w = 1.0 - u - v;
            if u < 0.0 || v < 0.0 || w < 0.0 {
                continue;
            }
            let above = u * a[2] + v * b[2] + w * c[2] - o[2];
            if back {
                if above * (1.0 + self.sx * self.sx + self.sy * self.sy).sqrt() > reach() {
                    return true;
                }
                continue;
            }
            if above > 1e-4 {
                return true;
            }
        }
        false
    }
}

fn reach() -> f32 {
    static REACH: std::sync::OnceLock<f32> = std::sync::OnceLock::new();
    *REACH.get_or_init(|| std::env::var("REACH").ok().and_then(|v| v.parse().ok()).unwrap_or(0.0))
}

fn bilinear(map: &halo_map::bitmap::Decoded, st: [f32; 2]) -> V3 {
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

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 5 {
        eprintln!(
            "usage: cliff_shading <map> <bsp index> <x,y,z,yaw,pitch> <out prefix> [sun yaw,pitch | tag] [terrain shader parts]"
        );
        std::process::exit(1);
    }
    let cache = halo_map::CacheFile::open(std::path::Path::new(&args[1])).unwrap();
    let only: usize = args[2].parse().unwrap();
    let cam: Vec<f32> = args[3].split(',').map(|x| x.trim().parse().unwrap()).collect();
    let prefix = &args[4];
    let parts: Vec<&str> = args.get(6).map_or("cliff", |a| a.as_str()).split(',').collect();
    let reference = halo_map::bsp::references(&cache).unwrap().into_iter().find(|r| r.index == only).unwrap();
    let bsp = halo_map::bsp::Bsp::load(&cache, &reference).unwrap();

    let sun: V3 = match args.get(5).filter(|a| a.as_str() != "tag") {
        Some(text) => {
            let a: Vec<f32> = text.split(',').map(|x| x.trim().parse().unwrap()).collect();
            halo_map::sky::light_direction(a[0].to_radians(), a[1].to_radians())
        }
        None => {
            let skies = halo_map::sky::skies(&cache).unwrap();
            let light = skies
                .iter()
                .flat_map(|s| &s.lights)
                .filter(|l| l.diameter <= SUN_MAX_DIAMETER && l.power > 0.0)
                .max_by(|a, b| a.power.total_cmp(&b.power))
                .expect("no sun");
            light.direction
        }
    };
    println!(
        "sun: yaw {:.1} pitch {:.1}",
        sun[1].atan2(sun[0]).to_degrees(),
        sun[2].clamp(-1.0, 1.0).asin().to_degrees()
    );

    let lightmaps = bsp
        .lightmap_bitmaps()
        .unwrap()
        .datum
        .map(|d| halo_map::bitmap::bitmaps(&cache, d).unwrap())
        .unwrap_or_default();
    let decoded: Vec<_> = lightmaps.iter().map(|b| b.decode_rgba8(&cache).ok()).collect();

    let mut faces = Vec::new();
    let mut names: std::collections::BTreeMap<String, f32> = Default::default();
    for material in &bsp.materials().unwrap() {
        let name = material.shader.datum.and_then(|d| cache.tag(d).ok()).map_or(String::new(), |t| t.name.clone());
        let occluder = !SKIP.iter().any(|s| name.contains(s));
        let map =
            usize::try_from(material.lightmap_bitmap_index).ok().filter(|&i| decoded.get(i).is_some_and(|d| d.is_some()));
        let terrain = parts.iter().any(|s| name.contains(s));
        let has_uv = material.lightmap_texcoords.len() == material.vertices.len();
        for t in material.indices.chunks_exact(3) {
            let v = [0, 1, 2].map(|i| &material.vertices[t[i] as usize]);
            let p = v.map(|v| v.position);
            let c = cross(sub(p[1], p[0]), sub(p[2], p[0]));
            let length = dot(c, c).sqrt();
            if length <= 0.0 {
                continue;
            }
            *names.entry(name.clone()).or_default() += length / 2.0;
            faces.push(Face {
                p,
                n: v.map(|v| unit(v.normal)),
                // (clockwise from the front: the cross product points back)
                g: mul(c, -1.0 / length),
                uv: has_uv.then(|| [0, 1, 2].map(|i| material.lightmap_texcoords[t[i] as usize])),
                map,
                terrain,
                occluder,
            });
        }
    }
    println!("shaders by area:");
    for (name, area) in &names {
        println!("  {area:10.0}  {name}{}", if parts.iter().any(|s| name.contains(s)) { "  (terrain)" } else { "" });
    }

    // the terrain's normals
    let (mut area, mut turned, mut terminator, mut tilt_sum) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
    let (mut mapped_area, mut texels) = (0.0f64, 0.0f64);
    let mut tilts: Vec<(f32, f32)> = Vec::new();
    for f in faces.iter().filter(|f| f.terrain) {
        let a = (dot(cross(sub(f.p[1], f.p[0]), sub(f.p[2], f.p[0])), f.g).abs() / 2.0) as f64;
        area += a;
        if let (Some(map), Some(uv)) = (f.map, f.uv) {
            let d = decoded[map].as_ref().unwrap();
            let (e, g) = ([uv[1][0] - uv[0][0], uv[1][1] - uv[0][1]], [uv[2][0] - uv[0][0], uv[2][1] - uv[0][1]]);
            mapped_area += a;
            texels += ((e[0] * g[1] - e[1] * g[0]).abs() / 2.0 * d.width as f32 * d.height as f32) as f64;
        }
        let tilt = f.n.iter().map(|n| dot(*n, f.g).clamp(-1.0, 1.0).acos().to_degrees()).fold(0.0f32, f32::max);
        tilts.push((tilt, a as f32));
        tilt_sum += tilt as f64 * a;
        if dot(f.g, sun) <= 0.0 {
            turned += a;
            if f.n.iter().any(|n| dot(*n, sun) > 0.0) {
                terminator += a;
            }
        }
    }
    tilts.sort_by(|a, b| a.0.total_cmp(&b.0));
    let quantile = |q: f64| {
        let mut run = 0.0f64;
        for &(t, a) in &tilts {
            run += a as f64;
            if run >= q * area {
                return t;
            }
        }
        0.0
    };
    println!(
        "terrain: {} faces, area {area:.0}; a face's largest vertex normal tilt from its own: mean {:.1}, median {:.1}, 90% {:.1}, 99% {:.1} degrees",
        tilts.len(),
        tilt_sum / area,
        quantile(0.5),
        quantile(0.9),
        quantile(0.99)
    );
    println!(
        "terrain lightmap: {:.2} texels a world unit ({:.2} world units a texel)",
        (texels / mapped_area.max(1e-9)).sqrt(),
        (mapped_area / texels.max(1e-9)).sqrt()
    );
    println!(
        "terrain turned from the sun: {:.1}% of the area; of it {:.1}% has a vertex normal turned to the sun ({:.1}% of all)",
        100.0 * turned / area,
        100.0 * terminator / turned.max(1e-9),
        100.0 * terminator / area
    );

    // the camera
    let (yaw, pitch) = (cam[3].to_radians(), cam[4].to_radians());
    let eye = [cam[0], cam[1], cam[2]];
    let forward = [pitch.cos() * yaw.cos(), pitch.cos() * yaw.sin(), pitch.sin()];
    let right = unit(cross(forward, [0.0, 0.0, 1.0]));
    let up = cross(right, forward);
    let tan_v = (FOV.to_radians() / 2.0).tan();
    let tan_h = tan_v * WIDTH as f32 / HEIGHT as f32;

    // rasterize: per pixel the nearest face and the point's weights on it
    let mut depth = vec![f32::MAX; WIDTH * HEIGHT];
    let mut hit: Vec<(u32, [f32; 3])> = vec![(u32::MAX, [0.0; 3]); WIDTH * HEIGHT];
    for (index, f) in faces.iter().enumerate() {
        let to_eye = sub(eye, f.p[0]);
        if dot(to_eye, f.g) <= 0.0 {
            continue;
        }
        // camera space (x right, y up, z forward) with the corner's weights
        let mut polygon: Vec<(V3, V3)> = (0..3)
            .map(|i| {
                let v = sub(f.p[i], eye);
                let mut w = [0.0; 3];
                w[i] = 1.0;
                ([dot(v, right), dot(v, up), dot(v, forward)], w)
            })
            .collect();
        // clip at the near plane
        let mut clipped = Vec::new();
        for i in 0..polygon.len() {
            let (a, b) = (polygon[i], polygon[(i + 1) % polygon.len()]);
            let (ina, inb) = (a.0[2] >= NEAR, b.0[2] >= NEAR);
            if ina {
                clipped.push(a);
            }
            if ina != inb {
                let t = (NEAR - a.0[2]) / (b.0[2] - a.0[2]);
                clipped.push((add(a.0, mul(sub(b.0, a.0), t)), add(a.1, mul(sub(b.1, a.1), t))));
            }
        }
        polygon = clipped;
        if polygon.len() < 3 {
            continue;
        }
        let screen: Vec<[f32; 3]> = polygon
            .iter()
            .map(|(v, _)| {
                [
                    (v[0] / v[2] / tan_h * 0.5 + 0.5) * WIDTH as f32,
                    (0.5 - v[1] / v[2] / tan_v * 0.5) * HEIGHT as f32,
                    1.0 / v[2],
                ]
            })
            .collect();
        for k in 1..polygon.len() - 1 {
            let (s, w) = ([screen[0], screen[k], screen[k + 1]], [polygon[0].1, polygon[k].1, polygon[k + 1].1]);
            let x0 = s.iter().map(|p| p[0]).fold(f32::MAX, f32::min).floor().max(0.0) as usize;
            let x1 = (s.iter().map(|p| p[0]).fold(f32::MIN, f32::max).ceil().max(0.0) as usize).min(WIDTH - 1);
            let y0 = s.iter().map(|p| p[1]).fold(f32::MAX, f32::min).floor().max(0.0) as usize;
            let y1 = (s.iter().map(|p| p[1]).fold(f32::MIN, f32::max).ceil().max(0.0) as usize).min(HEIGHT - 1);
            let det = (s[1][1] - s[2][1]) * (s[0][0] - s[2][0]) + (s[2][0] - s[1][0]) * (s[0][1] - s[2][1]);
            if det.abs() < 1e-9 {
                continue;
            }
            for y in y0..=y1 {
                for x in x0..=x1 {
                    let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
                    let a = ((s[1][1] - s[2][1]) * (px - s[2][0]) + (s[2][0] - s[1][0]) * (py - s[2][1])) / det;
                    let b = ((s[2][1] - s[0][1]) * (px - s[2][0]) + (s[0][0] - s[2][0]) * (py - s[2][1])) / det;
                    let c = 1.0 - a - b;
                    if a < 0.0 || b < 0.0 || c < 0.0 {
                        continue;
                    }
                    let inverse = a * s[0][2] + b * s[1][2] + c * s[2][2];
                    let z = 1.0 / inverse;
                    if z >= depth[y * WIDTH + x] {
                        continue;
                    }
                    depth[y * WIDTH + x] = z;
                    // perspective-correct weights on the face
                    let weights: V3 = std::array::from_fn(|i| {
                        (a * s[0][2] * w[0][i] + b * s[1][2] * w[1][i] + c * s[2][2] * w[2][i]) / inverse
                    });
                    hit[y * WIDTH + x] = (index as u32, weights);
                }
            }
        }
    }

    let grid = Grid::new(&faces, sun);
    // the wide suns' directions: a disc's points turned to each diameter
    let side_a = unit(cross(sun, [0.0, 0.0, 1.0]));
    let side_b = cross(sun, side_a);
    let wide: Vec<Vec<(V3, Grid)>> = SUNS
        .iter()
        .map(|diameter| {
            let radius = (diameter.to_radians() / 2.0).tan();
            (0..SUN_SAMPLES)
                .map(|i| {
                    // (a sunflower's points: even over the disc)
                    let (r, a) = (((i as f32 + 0.5) / SUN_SAMPLES as f32).sqrt() * radius, i as f32 * 2.399963);
                    let d = unit(add(sun, add(mul(side_a, r * a.cos()), mul(side_b, r * a.sin()))));
                    (d, Grid::new(&faces, d))
                })
                .collect()
        })
        .collect();
    const MODES: [&str; 18] = [
        "lightmap", "smooth", "traced", "shifted", "fronts", "sun2", "sun5", "sun10", "blur03", "blur06", "blur12",
        "shifted_sun5", "soft03", "soft06", "soft12", "fblur03", "fblur06", "fblur12",
    ];
    // per pixel: the traced shadow alone (1 lit), the smooth n.l, the world position
    let mut shadow = vec![-1.0f32; WIDTH * HEIGHT];
    // (and `fronts`' shadow)
    let mut front_shadow = vec![-1.0f32; WIDTH * HEIGHT];
    let mut position = vec![[0.0f32; 3]; WIDTH * HEIGHT];
    let mut values_of = vec![[0.0f32; 18]; WIDTH * HEIGHT];
    let mut baked_of: Vec<Option<V3>> = vec![None; WIDTH * HEIGHT];
    let mut pictures = vec![vec![0u8; WIDTH * HEIGHT * 3]; MODES.len()];
    // per mode: sums for the correlation with the lightmap over terrain pixels
    let mut sums = [[0.0f64; 6]; MODES.len()];
    let (mut shift_sum, mut shift_count, mut shift_max) = (0.0f64, 0.0f64, 0.0f32);
    for (pixel, &(index, w)) in hit.iter().enumerate() {
        if index == u32::MAX {
            continue;
        }
        let f = &faces[index as usize];
        let p = add(add(mul(f.p[0], w[0]), mul(f.p[1], w[1])), mul(f.p[2], w[2]));
        let n = unit(add(add(mul(f.n[0], w[0]), mul(f.n[1], w[1])), mul(f.n[2], w[2])));
        let smooth = dot(n, sun).max(0.0);
        let baked = match (f.map, f.uv) {
            (Some(map), Some(uv)) => {
                let st = [0, 1].map(|c| w[0] * uv[0][c] + w[1] * uv[1][c] + w[2] * uv[2][c]);
                Some(bilinear(decoded[map].as_ref().unwrap(), st))
            }
            _ => None,
        };
        // traced: the face's own side, then any triangle
        let facing = dot(f.g, sun) > 0.0;
        let open = facing && !grid.covered(add(p, mul(n, LIFT)), false, index as usize);
        let traced = if open { smooth } else { 0.0 };
        shadow[pixel] = if open { 1.0 } else { 0.0 };
        position[pixel] = p;
        // shifted: onto the vertex normals' smooth surface
        let mut moved = p;
        for i in 0..3 {
            let d = dot(sub(p, f.p[i]), f.n[i]).min(0.0);
            moved = add(moved, mul(f.n[i], -d * w[i]));
        }
        let shift = dot(sub(moved, p), sub(moved, p)).sqrt();
        if f.terrain {
            shift_sum += shift as f64;
            shift_count += 1.0;
            shift_max = shift_max.max(shift);
        }
        let shifted = if smooth > 0.0 && !grid.covered(add(moved, mul(n, LIFT)), false, usize::MAX) { smooth } else { 0.0 };
        // fronts: from the side of the face the ray leaves by, stopped by fronts alone
        let side = if facing { 1.0 } else { -1.0 };
        let fronts_open = !grid.covered(add(p, mul(f.g, side * LIFT)), true, index as usize);
        front_shadow[pixel] = if fronts_open { 1.0 } else { 0.0 };
        let fronts = if smooth > 0.0 && fronts_open { smooth } else { 0.0 };
        let mut values = [0.0f32; 18];
        values[1..5].copy_from_slice(&[smooth, traced, shifted, fronts]);
        for (k, directions) in wide.iter().enumerate() {
            let mut sum = 0.0;
            for (d, grid) in directions {
                if dot(f.g, *d) > 0.0 && !grid.covered(add(p, mul(n, LIFT)), false, index as usize) {
                    sum += dot(n, *d).max(0.0);
                }
            }
            values[5 + k] = sum / SUN_SAMPLES as f32;
        }
        values[11] = wide[1]
            .iter()
            .filter(|(d, grid)| dot(n, *d) > 0.0 && !grid.covered(add(moved, mul(n, LIFT)), false, usize::MAX))
            .map(|(d, _)| dot(n, *d))
            .sum::<f32>()
            / SUN_SAMPLES as f32;
        // soft: the planned realtime rule, the ray started from a random point
        // of a disc in the face's own plane, just under it, and stopped by
        // fronts alone
        let axis = if f.g[2].abs() < 0.9 { [0.0, 0.0, 1.0] } else { [1.0, 0.0, 0.0] };
        let plane_a = unit(cross(f.g, axis));
        let plane_b = cross(f.g, plane_a);
        for (k, radius) in BLURS.iter().enumerate() {
            let mut open = 0.0;
            for i in 0..SOFT_SAMPLES {
                let (r, a) = (((i as f32 + 0.5) / SOFT_SAMPLES as f32).sqrt() * radius, i as f32 * 2.399963 + pixel as f32 * 0.618);
                let origin = add(add(p, mul(f.g, -LIFT)), add(mul(plane_a, r * a.cos()), mul(plane_b, r * a.sin())));
                if !grid.covered(origin, true, usize::MAX) {
                    open += 1.0;
                }
            }
            values[12 + k] = open / SOFT_SAMPLES as f32 * smooth;
        }
        values[8] = smooth;
        values_of[pixel] = values;
        baked_of[pixel] = baked;
    }
    // the blurs: the shadow averaged over the screen's pixels within the radius
    let mut seed: u32 = 12345;
    let mut random = move || {
        seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
        (seed >> 8) as f32 / 16777216.0
    };
    for pixel in 0..WIDTH * HEIGHT {
        if hit[pixel].0 == u32::MAX {
            continue;
        }
        let smooth = values_of[pixel][8];
        let (x, y) = ((pixel % WIDTH) as f32, (pixel / WIDTH) as f32);
        for (k, radius) in BLURS.iter().enumerate() {
            let pixels = radius / depth[pixel] / tan_v * HEIGHT as f32 / 2.0;
            let (mut sum, mut count) = (shadow[pixel], 1.0);
            let mut front_sum = front_shadow[pixel];
            for _ in 0..BLUR_SAMPLES {
                let (r, a) = (random().sqrt() * pixels * 1.5, random() * std::f32::consts::TAU);
                let (sx, sy) = ((x + r * a.cos()).round(), (y + r * a.sin()).round());
                if sx < 0.0 || sy < 0.0 || sx >= WIDTH as f32 || sy >= HEIGHT as f32 {
                    continue;
                }
                let other = sy as usize * WIDTH + sx as usize;
                let apart = sub(position[other], position[pixel]);
                if shadow[other] < 0.0 || dot(apart, apart) > radius * radius {
                    continue;
                }
                sum += shadow[other];
                front_sum += front_shadow[other];
                count += 1.0;
            }
            values_of[pixel][8 + k] = sum / count * smooth;
            // fblur: `fronts`' shadow averaged the same way
            values_of[pixel][15 + k] = front_sum / count * smooth;
        }
    }
    for (pixel, &(index, _)) in hit.iter().enumerate() {
        if index == u32::MAX {
            for picture in pictures.iter_mut() {
                picture[pixel * 3..pixel * 3 + 3].copy_from_slice(&[40, 60, 110]);
            }
            continue;
        }
        let f = &faces[index as usize];
        let (values, baked) = (values_of[pixel], baked_of[pixel]);
        for (mode, picture) in pictures.iter_mut().enumerate() {
            let rgb = if mode == 0 {
                baked.map_or([255, 0, 255], |c| c.map(|x| x.min(255.0) as u8))
            } else {
                [((0.25 + 0.75 * values[mode]) * 255.0) as u8; 3]
            };
            picture[pixel * 3..pixel * 3 + 3].copy_from_slice(&rgb);
            if let (true, Some(c)) = (f.terrain && mode > 0, baked) {
                let (x, y) = (values[mode] as f64, (0.3 * c[0] + 0.59 * c[1] + 0.11 * c[2]) as f64);
                let s = &mut sums[mode];
                s[0] += 1.0;
                s[1] += x;
                s[2] += y;
                s[3] += x * x;
                s[4] += y * y;
                s[5] += x * y;
            }
        }
    }
    for (mode, picture) in pictures.iter().enumerate() {
        let path = format!("{prefix}_{}.png", MODES[mode]);
        image::save_buffer(&path, picture, WIDTH as u32, HEIGHT as u32, image::ColorType::Rgb8).unwrap();
        if mode > 0 {
            let [n, sx, sy, sxx, syy, sxy] = sums[mode];
            let r = (sxy - sx * sy / n) / ((sxx - sx * sx / n) * (syy - sy * sy / n)).sqrt();
            println!("{:8}: correlation with the lightmap over {n:.0} terrain pixels {r:.3}", MODES[mode]);
        }
    }
    println!(
        "shifted: the origin moved {:.3} world units on average over the terrain's pixels, {:.3} at most",
        shift_sum / shift_count.max(1.0),
        shift_max
    );
}
