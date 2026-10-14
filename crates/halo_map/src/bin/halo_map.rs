//! Inspect Halo Xbox cache files.
//!
//! ```text
//! halo_map info    <map>
//! halo_map tags    <map> [group]
//! halo_map bsp     <map>
//! halo_map shaders <map>
//! halo_map sky     <map>
//! halo_map bitmap  <map> <tag name> <out.png> [bitmap index]
//! halo_map bitmaps <map>          (decode every bitmap; report by format)
//! halo_map objects <map> [list]   (placements; decode every placed model)
//! halo_map skypano <map> <out.png> (the sky model as an equirectangular panorama)
//! ```

use halo_map::{bitmap, bsp, fourcc, fourcc_str, groups, model, objects, scenario, shader, sky, CacheFile};
use std::{collections::BTreeMap, process::ExitCode, time::Instant};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (Some(command), Some(path)) = (args.first(), args.get(1)) else {
        eprintln!("usage: halo_map <info|tags|bsp|shaders|shader|sky|bitmap|bitmaps|objects|names|skypano> <map> [args]");
        return ExitCode::FAILURE;
    };
    let start = Instant::now();
    let cache = match CacheFile::open(path) {
        Ok(cache) => cache,
        Err(e) => {
            eprintln!("{path}: {e}");
            return ExitCode::FAILURE;
        }
    };
    let loaded = start.elapsed();
    let result = match command.as_str() {
        "info" => info(&cache, loaded),
        "tags" => tags(&cache, args.get(2)),
        "bsp" => bsps(&cache),
        "shaders" => shaders(&cache),
        "shader" => shader(&cache, &args[2..]),
        "sky" => skies(&cache),
        "bitmap" => export_bitmap(&cache, &args[2..]),
        "bitmaps" => all_bitmaps(&cache),
        "skypano" => sky_panorama(&cache, &args[2..]),
        "objects" => objects(&cache, args.get(2).is_some_and(|a| a == "list")),
        "names" => names(&cache),
        other => Err(format!("unknown command {other}").into()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

type CliResult = Result<(), Box<dyn std::error::Error>>;

/// One shader tag (by name, any shader group): what the renderer reads from
/// it, and a transparent shader's combiner program.
fn shader(cache: &CacheFile, args: &[String]) -> CliResult {
    let name = args.first().ok_or("usage: halo_map shader <map> <tag name>")?;
    let tag = cache
        .tags
        .iter()
        .find(|t| t.name.eq_ignore_ascii_case(name) && (t.group == halo_map::groups::SHADER || t.parent_groups.contains(&halo_map::groups::SHADER)))
        .ok_or_else(|| format!("no shader {name}"))?;
    let reference = halo_map::TagRef { group: tag.group, datum: Some(tag.datum) };
    println!("{:#?}", halo_map::shader::shader_info(cache, reference)?);
    if let Some(program) = halo_map::combiner::program(cache, reference)? {
        println!("{program:#x?}");
    }
    Ok(())
}

fn info(cache: &CacheFile, loaded: std::time::Duration) -> CliResult {
    let h = &cache.header;
    println!("name         {}", h.name);
    println!("build        {}", h.build);
    println!("version      {}", h.version);
    println!("file length  {:#x} ({:.1} MiB inflated in {loaded:.2?})", h.file_length, h.file_length as f64 / 1048576.0);
    println!("tag data     {:#x} bytes at {:#x}", h.tag_data_size, h.tag_data_offset);
    println!("tags         {}", cache.tags.len());
    println!("scenario     {}", cache.tag(cache.scenario_datum)?.name);
    for start in scenario::player_starts(cache)?.iter().take(4) {
        println!("player start {:?} facing {:.2} bsp {}", start.position, start.facing, start.bsp_index);
    }
    let mut by_group: BTreeMap<String, usize> = BTreeMap::new();
    for tag in &cache.tags {
        *by_group.entry(fourcc_str(tag.group)).or_default() += 1;
    }
    println!("groups       {}", by_group.iter().map(|(g, n)| format!("{g}:{n}")).collect::<Vec<_>>().join(" "));
    Ok(())
}

fn tags(cache: &CacheFile, group: Option<&String>) -> CliResult {
    let group = group.map(|g| {
        let mut code = [b' '; 4];
        code[..g.len().min(4)].copy_from_slice(&g.as_bytes()[..g.len().min(4)]);
        fourcc(&code)
    });
    for tag in cache.tags.iter().filter(|t| group.is_none_or(|g| t.is_a(g))) {
        println!("{:#010x} {} {:#010x} {}", tag.datum, fourcc_str(tag.group), tag.base_address, tag.name);
    }
    Ok(())
}

fn bsps(cache: &CacheFile) -> CliResult {
    for reference in bsp::references(cache)? {
        println!(
            "bsp {} {} — {:#x} bytes at file {:#x}, loaded at {:#010x}",
            reference.index, reference.name, reference.space.size, reference.space.file_offset, reference.space.base
        );
        let start = Instant::now();
        let loaded = bsp::Bsp::load(cache, &reference)?;
        let materials = loaded.materials()?;
        let bounds = loaded.world_bounds()?;
        let vertices: usize = materials.iter().map(|m| m.vertices.len()).sum();
        let triangles: usize = materials.iter().map(|m| m.indices.len() / 3).sum();

        // Winding: compare each triangle's geometric normal with its vertices'
        // normals, to know which way the game's triangles face.
        let (mut agree, mut disagree) = (0usize, 0usize);
        for material in &materials {
            for tri in material.indices.chunks_exact(3) {
                let [a, b, c] = [0, 1, 2].map(|i| material.vertices[tri[i] as usize]);
                let e1 = sub(b.position, a.position);
                let e2 = sub(c.position, a.position);
                let n = cross(e1, e2);
                let vn = add(add(a.normal, b.normal), c.normal);
                let d = dot(n, vn);
                if d > 0.0 {
                    agree += 1;
                } else if d < 0.0 {
                    disagree += 1;
                }
            }
        }
        let normal_lengths: Vec<f32> = materials
            .iter()
            .flat_map(|m| m.vertices.iter().map(|v| dot(v.normal, v.normal).sqrt()))
            .collect();
        let mean_normal = normal_lengths.iter().sum::<f32>() / normal_lengths.len().max(1) as f32;
        println!(
            "  {} materials, {vertices} vertices, {triangles} triangles, read in {:.2?}",
            materials.len(),
            start.elapsed()
        );
        println!("  bounds x {:?} y {:?} z {:?}", bounds[0], bounds[1], bounds[2]);
        println!("  counter-clockwise triangles (cross(b-a, c-a) along normals): {agree}, clockwise: {disagree}");
        println!("  mean |normal| {mean_normal:.3}");
        let outside = materials
            .iter()
            .flat_map(|m| &m.vertices)
            .filter(|v| (0..3).any(|i| v.position[i] < bounds[i][0] - 0.01 || v.position[i] > bounds[i][1] + 0.01))
            .count();
        println!("  vertices outside the bounds: {outside}");
    }
    Ok(())
}

fn shaders(cache: &CacheFile) -> CliResult {
    let mut seen = std::collections::BTreeSet::new();
    for reference in bsp::references(cache)? {
        let loaded = bsp::Bsp::load(cache, &reference)?;
        for material in loaded.materials()? {
            let Some(datum) = material.shader.datum else { continue };
            if !seen.insert(datum) {
                continue;
            }
            match shader::shader_info(cache, material.shader)? {
                Some(info) => {
                    let base = info.base_map.and_then(|r| r.datum).map(|d| cache.tag(d).map(|t| t.name.clone()));
                    println!(
                        "{} {:?} {} emit {:?}x{:.1} base {:?}",
                        fourcc_str(info.group),
                        info.shader_type,
                        info.name,
                        info.emitted_light,
                        info.emitted_power,
                        base.transpose()?.unwrap_or_default()
                    );
                }
                None => println!("(none)"),
            }
        }
    }
    Ok(())
}

fn skies(cache: &CacheFile) -> CliResult {
    for sky in sky::skies(cache)? {
        println!("{}", sky.name);
        for light in &sky.lights {
            println!(
                "  light color {:?} power {:.3} direction {:?} diameter {:.4}",
                light.color, light.power, light.direction, light.diameter
            );
        }
    }
    Ok(())
}

fn export_bitmap(cache: &CacheFile, args: &[String]) -> CliResult {
    let [name, out, rest @ ..] = args else {
        return Err("usage: halo_map bitmap <map> <tag name> <out.png> [index]".into());
    };
    let index: usize = rest.first().map(|s| s.parse()).transpose()?.unwrap_or(0);
    let tag = cache.find_tag(groups::BITMAP, name).ok_or_else(|| format!("no bitmap tag {name}"))?;
    let bitmaps = bitmap::bitmaps(cache, tag.datum)?;
    for (i, b) in bitmaps.iter().enumerate() {
        println!(
            "  [{i}] {}x{}x{} {:?} {:?} flags {:#06x} levels {} pixels {:#x}+{:#x}",
            b.width, b.height, b.depth, b.kind, b.format, b.flags, b.levels, b.pixels_offset, b.pixels_size
        );
    }
    let b = bitmaps.get(index).ok_or("bitmap index out of range")?;
    let decoded = b.decode_rgba8(cache)?;
    image::save_buffer(out, &decoded.data, decoded.width, decoded.height, image::ColorType::Rgba8)?;
    println!("wrote {out}");
    Ok(())
}

fn all_bitmaps(cache: &CacheFile) -> CliResult {
    let mut counts: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    let mut failures = Vec::new();
    for tag in cache.tags.iter().filter(|t| t.group == groups::BITMAP) {
        let bitmaps = match bitmap::bitmaps(cache, tag.datum) {
            Ok(b) => b,
            Err(e) => {
                failures.push(format!("{}: {e}", tag.name));
                continue;
            }
        };
        for (i, b) in bitmaps.iter().enumerate() {
            let swizzled = if b.flags & (1 << 3) != 0 { " swizzled" } else { "" };
            let entry = counts.entry(format!("{:?} {:?}{swizzled}", b.kind, b.format)).or_default();
            match b.decode(cache, u32::MAX) {
                Ok(_) => entry.0 += 1,
                Err(e) => {
                    entry.1 += 1;
                    if b.kind == bitmap::Kind::Texture2d {
                        failures.push(format!("{}[{i}]: {e}", tag.name));
                    }
                }
            }
        }
    }
    for (key, (ok, failed)) in &counts {
        println!("{key:40} decoded {ok:5}  unsupported/failed {failed:5}");
    }
    for failure in failures.iter().take(20) {
        println!("FAIL {failure}");
    }
    println!("{} unexpected failures", failures.len());
    Ok(())
}

/// The scenario's object names (what scripts call objects by) and its
/// encounters with their squads (what they place AI by).
fn names(cache: &CacheFile) -> CliResult {
    use halo_map::layout::{encounter_definition as e, scenario as s, squad_definition as q};
    /// a scenario object name: its name, then its type and index
    const OBJECT_NAME_SIZE: usize = 36;
    let text = |bytes: &[u8]| String::from_utf8_lossy(bytes.split(|&b| b == 0).next().unwrap_or(&[])).into_owned();
    let scenario = cache.scenario()?;
    for (index, name) in scenario.block(s::OBJECT_NAMES, OBJECT_NAME_SIZE)?.iter().enumerate() {
        println!("object name {index}: {}", text(name.bytes(0, 32)?));
    }
    for (index, encounter) in scenario.block(s::AI_ENCOUNTERS, e::SIZEOF)?.iter().enumerate() {
        let squads: Vec<String> =
            encounter.block(e::SQUADS, q::SIZEOF)?.iter().map(|q| q.bytes(q::NAME, 32).map(text)).collect::<Result<_, _>>()?;
        println!("encounter {index}: {} (squads {})", text(encounter.bytes(e::NAME, 32)?), squads.join(", "));
    }
    Ok(())
}

fn objects(cache: &CacheFile, list: bool) -> CliResult {
    let placements = objects::placements(cache)?;
    if list {
        for p in &placements {
            let name = p.definition.datum.map(|d| cache.tag(d).map(|t| t.name.clone())).transpose()?.unwrap_or_default();
            println!(
                "{:?} {name} at {:.2},{:.2},{:.2} yaw {:.1} bsps {:#x}{}{}",
                p.kind,
                p.position[0],
                p.position[1],
                p.position[2],
                p.rotation[0].to_degrees(),
                p.on_bsp_flags,
                if p.not_automatic { " (scripted)" } else { "" },
                p.light.map(|l| format!(" light {:?} x{} falloff {:.2} cutoff {:.2}", l.color, l.intensity, l.falloff_angle, l.cutoff_angle)).unwrap_or_default()
            );
        }
    }
    let mut by_kind: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    for p in &placements {
        let e = by_kind.entry(format!("{:?}", p.kind)).or_default();
        e.0 += 1;
        if p.not_automatic {
            e.1 += 1;
        }
    }
    for (kind, (n, scripted)) in &by_kind {
        println!("{kind:13} {n:4} placed ({scripted} by scripts only)");
    }

    let mut models = std::collections::BTreeSet::new();
    for p in &placements {
        if let Some(m) = objects::object_model(cache, p.definition)?.and_then(|m| m.datum) {
            models.insert(m);
        }
    }
    let (mut agree, mut disagree, mut triangles, mut failures) = (0usize, 0usize, 0usize, 0usize);
    let (mut normal_sum, mut normal_count) = (0f32, 0usize);
    for &datum in &models {
        let result = (|| -> halo_map::Result<()> {
            let m = model::Model::load(cache, datum)?;
            for g in m.geometries_for(&[], model::HIGHEST_DETAIL) {
                for part in m.geometry(g)? {
                    triangles += part.indices.len() / 3;
                    for tri in part.indices.chunks_exact(3) {
                        let [a, b, c] = [0, 1, 2].map(|i| part.vertices[tri[i] as usize]);
                        let n = cross(sub(b.position, a.position), sub(c.position, a.position));
                        let d = dot(n, add(add(a.normal, b.normal), c.normal));
                        if d > 0.0 {
                            agree += 1;
                        } else if d < 0.0 {
                            disagree += 1;
                        }
                    }
                    for v in &part.vertices {
                        normal_sum += dot(v.normal, v.normal).sqrt();
                        normal_count += 1;
                    }
                }
            }
            Ok(())
        })();
        if let Err(e) = result {
            failures += 1;
            println!("FAIL {}: {e}", cache.tag(datum).map(|t| t.name.as_str()).unwrap_or("?"));
        }
    }
    println!(
        "{} models: {triangles} triangles at highest detail, counter-clockwise {agree} / clockwise {disagree}, mean |normal| {:.3}, {failures} failed",
        models.len(),
        normal_sum / normal_count.max(1) as f32
    );
    Ok(())
}

fn sky_panorama(cache: &CacheFile, args: &[String]) -> CliResult {
    let out = args.first().ok_or("usage: halo_map skypano <map> <out.png>")?;
    let model = sky::skies(cache)?.into_iter().find_map(|s| s.model).and_then(|m| m.datum).ok_or("no sky model")?;
    let renderer = sky::SkyRenderer::new(cache, model)?;
    let (w, h) = (1024u32, 512u32);
    let start = Instant::now();
    let mut pixels = Vec::with_capacity((w * h * 3) as usize);
    for y in 0..h {
        let pitch = std::f32::consts::FRAC_PI_2 - (y as f32 + 0.5) / h as f32 * std::f32::consts::PI;
        for x in 0..w {
            let yaw = (x as f32 + 0.5) / w as f32 * std::f32::consts::TAU;
            let direction = [pitch.cos() * yaw.cos(), pitch.cos() * yaw.sin(), pitch.sin()];
            pixels.extend(renderer.sample(direction).map(|c| (c * 255.0).round() as u8));
        }
    }
    image::save_buffer(out, &pixels, w, h, image::ColorType::Rgb8)?;
    println!("wrote {out} in {:.2?}", start.elapsed());
    Ok(())
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn add(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
