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
        "transparent" => transparent_survey(&cache),
        "shader" => shader(&cache, &args[2..]),
        "model" => model_dump(&cache, &args[2..]),
        "where" => where_point(&cache, &args[2..]),
        "skyless" => skyless_survey(&cache),
        "sky" => skies(&cache),
        "bitmap" => export_bitmap(&cache, &args[2..]),
        "bitmaps" => all_bitmaps(&cache),
        "skypano" => sky_panorama(&cache, &args[2..]),
        "objects" => objects(&cache, args.get(2).is_some_and(|a| a == "list")),
        "names" => names(&cache),
        "light" => light(&cache, &args[2..]),
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
/// One model tag (by name): its nodes, regions' permutations, and each
/// geometry's parts (shader, vertex and triangle counts, bounds).
/// The skyless clusters that see clusters with sky, in each BSP, and which
/// of their portals' centres another BSP holds in a cluster that sees none
/// (where the viewer draws that other BSP, bridge.rs `DrawnBsp`).
fn skyless_survey(cache: &CacheFile) -> CliResult {
    use halo_map::collision::{CollisionBsp, StructureClusters};
    use halo_map::layout::{structure_bsp, structure_cluster};
    struct Loaded {
        index: usize,
        collision: CollisionBsp,
        clusters: StructureClusters,
        skies: Vec<i16>,
        pvs: Vec<u32>,
    }
    impl Loaded {
        fn sees_sky(&self, from: usize) -> bool {
            let words = self.skies.len().div_ceil(32);
            (0..self.skies.len()).any(|to| self.skies[to] >= 0 && self.pvs.get(from * words + to / 32).is_some_and(|b| b & (1 << (to % 32)) != 0))
        }
    }
    let mut all = Vec::new();
    for reference in bsp::references(cache)? {
        let loaded = bsp::Bsp::load(cache, &reference)?;
        let blocks = loaded.root.block(structure_bsp::CLUSTERS, structure_cluster::SIZEOF)?;
        all.push(Loaded {
            index: reference.index,
            collision: CollisionBsp::load(&loaded)?,
            clusters: StructureClusters::load(cache, &loaded)?,
            skies: blocks.iter().map(|c| c.i16(structure_cluster::SKY_INDEX).unwrap_or(0)).collect(),
            pvs: loaded.root.data_bytes(structure_bsp::CLUSTER_DATA)?.chunks_exact(4).map(|w| u32::from_le_bytes(w.try_into().unwrap())).collect(),
        });
    }
    for a in &all {
        for c in 0..a.skies.len() {
            if a.skies[c] >= 0 || !a.sees_sky(c) {
                continue;
            }
            // (the viewer's rule needs the other BSP to hold every portal of
            // each skyless cluster this one sees)
            let words = a.skies.len().div_ceil(32);
            let seen: Vec<usize> = (0..a.skies.len()).filter(|&o| a.skies[o] < 0 && a.pvs.get(c * words + o / 32).is_some_and(|b| b & (1 << (o % 32)) != 0)).collect();
            for o in all.iter().filter(|o| o.index != a.index) {
                let held = |p: [f32; 3]| o.clusters.cluster_from_leaf(o.collision.leaf_from_point(p)) >= 0;
                let all_held = seen.iter().all(|&s| a.clusters.cluster_portals.get(s).map(|v| v.as_slice()).unwrap_or(&[]).iter().all(|&p| held(a.clusters.portals[p as usize].centroid)));
                // (cameras sampled at the cluster's portals' centres)
                let cameras: Vec<i16> = a.clusters.cluster_portals.get(c).map(|v| v.as_slice()).unwrap_or(&[]).iter().map(|&p| o.clusters.cluster_from_leaf(o.collision.leaf_from_point(a.clusters.portals[p as usize].centroid))).collect();
                let firing: Vec<i16> = cameras.iter().copied().filter(|&oc| oc >= 0 && !o.sees_sky(oc as usize)).collect();
                // (the sky clusters seen: whether a portal joins each to the
                // skyless clusters seen, or the camera's own)
                let seen_sky: Vec<(usize, bool)> = (0..a.skies.len())
                    .filter(|&s| a.skies[s] >= 0 && a.pvs.get(c * words + s / 32).is_some_and(|b| b & (1 << (s % 32)) != 0))
                    .map(|s| {
                        let joined = a.clusters.cluster_portals.get(s).map(|v| v.as_slice()).unwrap_or(&[]).iter().any(|&p| {
                            let pair = a.clusters.portals[p as usize].clusters;
                            pair.iter().any(|&k| k >= 0 && (k as usize == c || seen.contains(&(k as usize))))
                        });
                        (s, joined)
                    })
                    .collect();
                if all_held && !firing.is_empty() {
                    println!("    seen sky clusters (cluster, joined by a portal): {seen_sky:?}");
                    println!("  RULE: bsp {} cluster {c} draws bsp {} (its clusters {:?}, {} clusters, sky {:?})", a.index, o.index, firing, o.skies.len(), firing.iter().map(|&f| o.skies[f as usize]).collect::<Vec<_>>());
                }
            }
            let mut hits = Vec::new();
            for &p in a.clusters.cluster_portals.get(c).map(|v| v.as_slice()).unwrap_or(&[]) {
                let portal = &a.clusters.portals[p as usize];
                for o in all.iter().filter(|o| o.index != a.index) {
                    let oc = o.clusters.cluster_from_leaf(o.collision.leaf_from_point(portal.centroid));
                    if oc >= 0 && !o.sees_sky(oc as usize) {
                        hits.push(format!("bsp {} cluster {oc} at {:?}", o.index, portal.centroid.map(|v| (v * 10.0).round() / 10.0)));
                    }
                }
            }
            println!("bsp {} cluster {c}: skyless, sees sky; {}", a.index, if hits.is_empty() { "no other BSP holds it".to_string() } else { hits.join("; ") });
        }
    }
    Ok(())
}

/// Each BSP's view from a point: its cluster there, the cluster's sky, what
/// its potentially visible set holds, its portals, and how far rays round
/// the point go before they meet a surface.
fn where_point(cache: &CacheFile, args: &[String]) -> CliResult {
    use halo_map::collision::{CollisionBsp, StructureClusters};
    use halo_map::layout::{structure_bsp, structure_cluster};
    let point: Vec<f32> = args.first().ok_or("usage: halo_map where <map> x,y,z")?.split(',').map(|v| v.parse()).collect::<Result<_, _>>()?;
    let point = [point[0], point[1], point[2]];
    for reference in bsp::references(cache)? {
        let loaded = bsp::Bsp::load(cache, &reference)?;
        let collision = CollisionBsp::load(&loaded)?;
        let clusters = StructureClusters::load(cache, &loaded)?;
        let blocks = loaded.root.block(structure_bsp::CLUSTERS, structure_cluster::SIZEOF)?;
        let skies: Vec<i16> = blocks.iter().map(|c| c.i16(structure_cluster::SKY_INDEX).unwrap_or(0)).collect();
        let pvs: Vec<u32> = loaded.root.data_bytes(structure_bsp::CLUSTER_DATA)?.chunks_exact(4).map(|w| u32::from_le_bytes(w.try_into().unwrap())).collect();
        let words = skies.len().div_ceil(32);
        let cluster = clusters.cluster_from_leaf(collision.leaf_from_point(point));
        print!("bsp {} {}: {} clusters; at the point cluster {cluster}", reference.index, reference.name, skies.len());
        if cluster < 0 {
            println!();
            continue;
        }
        let c = cluster as usize;
        let seen: Vec<usize> = (0..skies.len()).filter(|&o| pvs.get(c * words + o / 32).is_some_and(|b| b & (1 << (o % 32)) != 0)).collect();
        let with_sky = seen.iter().filter(|&&o| skies[o] >= 0).count();
        println!(", sky {}, sees {} clusters ({} with sky): {:?}", skies[c], seen.len(), with_sky, seen);
        for &p in clusters.cluster_portals.get(c).map(|v| v.as_slice()).unwrap_or(&[]) {
            let portal = &clusters.portals[p as usize];
            println!("  portal {p} {:?} at {:?} radius {:.2}", portal.clusters, portal.centroid, portal.bounding_radius);
        }
        let mut line = String::new();
        for step in 0..24 {
            let yaw = (step as f32 * 15.0).to_radians();
            let vector = [yaw.cos() * 40.0, yaw.sin() * 40.0, 0.0];
            let (hit, leaves) = collision.test_vector_leaves(3, point, vector, 1.0);
            let end = hit.map(|h| h.t * 40.0);
            let last = leaves.iter().rev().map(|&l| clusters.cluster_from_leaf(l)).find(|&c| c >= 0).unwrap_or(-1);
            line += &match end {
                Some(d) => format!(" {:>3}:{d:.1}(f{:x},c{last})", step * 15, hit.unwrap().flags),
                None => format!(" {:>3}:--(c{last})", step * 15),
            };
        }
        println!("  rays (yaw:distance):{line}");
    }
    Ok(())
}

fn model_dump(cache: &CacheFile, args: &[String]) -> CliResult {
    let name = args.first().ok_or("usage: halo_map model <map> <tag name>")?;
    let tag = cache
        .tags
        .iter()
        .find(|t| t.name.eq_ignore_ascii_case(name) && t.group == groups::MODEL)
        .ok_or_else(|| format!("no model {name}"))?;
    let model = model::Model::load(cache, tag.datum)?;
    println!("detail cutoffs {:?}, finest played {}", model.detail_cutoff_pixels, model.finest_played_detail());
    for (index, node) in model.nodes.iter().enumerate() {
        println!(
            "node {index} {} parent {:?} at {:?} rotation {:?} inverse scale {}",
            node.name, node.parent, node.default_translation, node.default_rotation, node.default_inverse.scale
        );
    }
    for region in &model.regions {
        for permutation in &region.permutations {
            println!("region {} permutation {} geometries {:?}", region.name, permutation.name, permutation.geometry_indices);
        }
    }
    for geometry in 0..model.geometry_count {
        for part in model.geometry(geometry)? {
            let shader = model
                .shaders
                .get(part.shader_index as usize)
                .and_then(|s| s.datum)
                .and_then(|d| cache.tag(d).ok())
                .map_or(String::new(), |t| t.name.clone());
            let (mut low, mut high) = ([f32::MAX; 3], [f32::MIN; 3]);
            let mut nodes = std::collections::BTreeSet::new();
            let weights: std::collections::BTreeSet<String> =
                part.vertices.iter().take(400).map(|v| format!("{:?}/{:.2?}", v.nodes, v.node_weights)).collect();
            println!("  weights {:?}", weights.iter().take(6).collect::<Vec<_>>());
            for v in &part.vertices {
                for c in 0..3 {
                    low[c] = low[c].min(v.position[c]);
                    high[c] = high[c].max(v.position[c]);
                }
                nodes.insert(v.nodes[0]);
            }
            println!(
                "geometry {geometry}: {shader}: {} vertices, {} triangles, nodes {nodes:?}, from {low:.2?} to {high:.2?}",
                part.vertices.len(),
                part.indices.len() / 3
            );
        }
    }
    Ok(())
}

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
        // (HALO_MAP_MATERIALS: each material's shader and triangles, and
        // the centre of its vertices)
        if std::env::var_os("HALO_MAP_MATERIALS").is_some() {
            for material in &materials {
                let name = material.shader.datum.and_then(|d| cache.tags.iter().find(|t| t.datum == d)).map_or("?", |t| t.name.as_str());
                let n = material.vertices.len().max(1) as f32;
                let centre = material.vertices.iter().fold([0.0f32; 3], |c, v| [c[0] + v.position[0] / n, c[1] + v.position[1] / n, c[2] + v.position[2] / n]);
                println!("  material {name}: {} triangles, centre {:.1?}", material.indices.len() / 3, centre);
            }
        }

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

/// Every transparent shader in the map (chicago, generic, meter, plasma):
/// what the viewer needs to draw them as the game does. `bsp` marks the
/// level's own; the rest are objects' and effects'.
fn transparent_survey(cache: &CacheFile) -> CliResult {
    let mut in_bsp = std::collections::BTreeSet::new();
    for reference in bsp::references(cache)? {
        for material in bsp::Bsp::load(cache, &reference)?.materials()? {
            if let Some(datum) = material.shader.datum {
                in_bsp.insert(datum);
            }
        }
    }
    // a map's texture animation: each of u, v and rotation as source/function
    // (nonzero when animated)
    let animation = |p: halo_map::Ptr<'_>, at: usize| -> Result<String, Box<dyn std::error::Error>> {
        let mut parts = Vec::new();
        for (name, o) in [("u", 0usize), ("v", 0x10), ("r", 0x20)] {
            let (source, function, period, scale) = (p.i16(at + o)?, p.i16(at + o + 2)?, p.f32(at + o + 4)?, p.f32(at + o + 0xc)?);
            if source != 0 || period != 0.0 || scale != 0.0 {
                parts.push(format!("{name}:src{source}/fn{function}/T{period}/x{scale}"));
            }
        }
        Ok(parts.join(" "))
    };
    for tag in cache.tags.iter().filter(|t| t.is_a(halo_map::groups::SHADER)) {
        let Ok(root) = cache.tag_root(tag.datum, halo_map::groups::SHADER) else { continue };
        let shader_type = shader::shader_type(root.i16(0x24).unwrap_or(-1));
        let kind = match shader_type {
            shader::ShaderType::TransparentChicago => "chicago",
            shader::ShaderType::TransparentGeneric => "generic",
            shader::ShaderType::Meter => "meter",
            shader::ShaderType::Plasma => "plasma",
            _ => continue,
        };
        let place = if in_bsp.contains(&tag.datum) { "bsp" } else { "obj" };
        if matches!(kind, "meter" | "plasma") {
            println!("{place} {kind} {}", tag.name);
            continue;
        }
        let t = 0x28;
        let flags = root.u8(t + 1)?;
        let (map_type, blend, fade, fade_source) = (root.i16(t + 2)?, root.i16(t + 4)?, root.i16(t + 6)?, root.i16(t + 8)?);
        let layers = root.block(t + 0x20, 16)?.count;
        let (map_size, animation_at) = if kind == "chicago" { (0xdc, 0xa4) } else { (0x64, 0x2c) };
        let maps = root.block(t + 0x2c, map_size)?;
        let mut animated = Vec::new();
        for index in 0..maps.count {
            let map = maps.get(index).unwrap();
            let text = animation(map, animation_at)?;
            if !text.is_empty() {
                animated.push(format!("m{index}[{text}]"));
            }
        }
        if kind == "generic" {
            let stages = root.block(0x60, 0x70)?;
            for index in 0..stages.count {
                let stage = stages.get(index).unwrap();
                let (source, function, period) = (stage.i16(4)?, stage.i16(6)?, stage.f32(8)?);
                if source != 0 || period != 0.0 {
                    animated.push(format!("c0s{index}[src{source}/fn{function}/T{period}]"));
                }
            }
        }
        let extra = if kind == "chicago" { root.u16(t + 0x38)? } else { 0 };
        println!(
            "{place} {kind} type{map_type} blend{blend} fade{fade}/src{fade_source} flags{flags:#04x} extra{extra:#x} layers{layers} maps{} {} {}",
            maps.count,
            animated.join(" "),
            tag.name
        );
    }
    Ok(())
}

fn skies(cache: &CacheFile) -> CliResult {
    for sky in sky::skies(cache)? {
        println!("{}", sky.name);
        if let Some(datum) = sky.model.and_then(|m| m.datum) {
            let model = halo_map::model::Model::load(cache, datum)?;
            let mut parts = 0;
            let (mut low, mut high) = ([f32::MAX; 3], [f32::MIN; 3]);
            for geometry in model.geometries_for(&[], halo_map::model::HIGHEST_DETAIL) {
                for part in model.geometry(geometry)? {
                    parts += 1;
                    let (mut part_low, mut part_high) = ([f32::MAX; 3], [f32::MIN; 3]);
                    for v in &part.vertices {
                        for axis in 0..3 {
                            part_low[axis] = part_low[axis].min(v.position[axis]);
                            part_high[axis] = part_high[axis].max(v.position[axis]);
                        }
                    }
                    let shader = model.shaders.get(part.shader_index.max(0) as usize).and_then(|s| s.datum);
                    let shader = shader.and_then(|d| cache.tag(d).ok()).map_or("-".to_string(), |t| t.name.clone());
                    println!("    part {shader}: {} vertices, {part_low:?} to {part_high:?}", part.vertices.len());
                    for axis in 0..3 {
                        low[axis] = low[axis].min(part_low[axis]);
                        high[axis] = high[axis].max(part_high[axis]);
                    }
                }
            }
            println!("  model {} ({parts} parts): {low:?} to {high:?}", model.name);
        }
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
    // (a cube map: its faces side by side, +X -X +Y -Y +Z -Z)
    let decoded = if b.kind == bitmap::Kind::CubeMap {
        let faces = b.decode_cube_faces_rgba8(cache)?;
        let (w, h) = (b.width as usize, b.height as usize);
        let mut data = vec![0u8; w * 6 * h * 4];
        for (f, face) in faces.iter().enumerate() {
            for y in 0..h {
                let row = &face.data[y * w * 4..(y + 1) * w * 4];
                data[(y * w * 6 + f * w) * 4..(y * w * 6 + (f + 1) * w) * 4].copy_from_slice(row);
            }
        }
        bitmap::Decoded { width: b.width * 6, data, ..faces.into_iter().next().unwrap() }
    } else {
        b.decode_rgba8(cache)?
    };
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
/// A light tag's (`ligh`, `point_light_definition` in object_lights.c)
/// shape, reach and colours.
fn light(cache: &CacheFile, args: &[String]) -> CliResult {
    let name = args.first().ok_or("usage: halo_map light <map> <tag name>")?;
    let group = u32::from_be_bytes(*b"ligh");
    let tag = cache.find_tag(group, name).ok_or_else(|| format!("no light {name}"))?;
    let root = cache.tag_root(tag.datum, group)?;
    let f = |o: usize| root.f32(o).unwrap_or(f32::NAN);
    println!("{}", tag.name);
    println!("  flags {:#x} radius {} (x {}..{})", root.u32(0)?, f(4), f(8), f(0xC));
    println!("  falloff angle {:.1} deg, cutoff angle {:.1} deg, lens flare radius {}", f(0x10).to_degrees(), f(0x14).to_degrees(), f(0x18));
    println!("  colour lower {:?} upper {:?} (interpolation flags {:#x})", [f(0x38), f(0x3C), f(0x40), f(0x44)], [f(0x48), f(0x4C), f(0x50), f(0x54)], root.u32(0x34)?);
    println!("  transition duration {} falloff function {}", f(0xF4), root.u32(0xF8)? >> 16);
    // the gel (light_definition: a cube map projected round the light) and
    // its periodic turning
    let name = |o: usize| -> Result<String, Box<dyn std::error::Error>> {
        Ok(root.reference(o)?.datum.map(|d| cache.tag(d).map(|t| t.name.clone())).transpose()?.unwrap_or_else(|| "none".into()))
    };
    println!("  gel {} (secondary {}), lens flare {}", name(0x64)?, name(0x7C)?, name(0xAC)?);
    println!(
        "  yaw function {} period {}, roll {} period {}, pitch {} period {}",
        root.i16(0x8E)?, f(0x90), root.i16(0x96)?, f(0x98), root.i16(0x9E)?, f(0xA0)
    );
    for beam in halo_map::light_gel::gel_beams(cache, tag.datum)? {
        println!(
            "  gel beam along {:.2?} (forward, left, up), falloff {:.1} cutoff {:.1} deg, strength {:.2}",
            beam.direction, beam.falloff_angle.to_degrees(), beam.cutoff_angle.to_degrees(), beam.strength
        );
    }
    Ok(())
}

fn names(cache: &CacheFile) -> CliResult {
    use halo_map::layout::{encounter_definition as e, platoon_definition as p, scenario as s, squad_definition as q};
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
        let platoons: Vec<String> =
            encounter.block(e::PLATOONS, p::SIZEOF)?.iter().map(|p| p.bytes(p::NAME, 32).map(text)).collect::<Result<_, _>>()?;
        println!(
            "encounter {index}: {} (squads {}; platoons {})",
            text(encounter.bytes(e::NAME, 32)?),
            squads.join(", "),
            platoons.join(", ")
        );
    }
    // (a command list's name first)
    for (index, list) in scenario.block(s::AI_COMMAND_LISTS, halo_map::layout::ai_command_list_definition::SIZEOF)?.iter().enumerate() {
        println!("command list {index}: {}", text(list.bytes(0, 32)?));
    }
    for (index, conversation) in scenario.block(s::AI_CONVERSATIONS, halo_map::layout::ai_conversation::SIZEOF)?.iter().enumerate() {
        println!("conversation {index}: {}", text(conversation.bytes(halo_map::layout::ai_conversation::NAME, 32)?));
    }
    // (the HUD messages: the scenario's hud_message_text tag, its messages
    // block at 0x20 of struct hud_state_message_definition, 0x40 bytes, a
    // name first)
    const SCENARIO_HUD_MESSAGES: usize = 0x594;
    if let Some(datum) = scenario.reference(SCENARIO_HUD_MESSAGES)?.datum {
        let messages = cache.tag_root(datum, u32::from_be_bytes(*b"hmt "))?;
        for (index, message) in messages.block(0x20, 0x40)?.iter().enumerate() {
            println!("hud message {index}: {}", text(message.bytes(0, 32)?));
        }
    }
    // (camera points: scenario_cutscene_camera_point, its flags then a name)
    const CUTSCENE_CAMERA_POINTS: usize = 0x4f0;
    const CUTSCENE_CAMERA_POINT_SIZE: usize = 0x68;
    for (index, point) in scenario.block(CUTSCENE_CAMERA_POINTS, CUTSCENE_CAMERA_POINT_SIZE)?.iter().enumerate() {
        println!("camera point {index}: {}", text(point.bytes(4, 32)?));
    }
    // (chapter titles: scenario_cutscene_title, its flags then a name)
    const CUTSCENE_TITLES: usize = 0x4fc;
    const CUTSCENE_TITLE_SIZE: usize = 0x60;
    for (index, title) in scenario.block(CUTSCENE_TITLES, CUTSCENE_TITLE_SIZE)?.iter().enumerate() {
        println!("cutscene title {index}: {}", text(title.bytes(4, 32)?));
    }
    // (cutscene flags: scenario_cutscene_flag, a name then a position and facing)
    const CUTSCENE_FLAGS: usize = 0x4e4;
    const CUTSCENE_FLAG_SIZE: usize = 0x5c;
    for (index, flag) in scenario.block(CUTSCENE_FLAGS, CUTSCENE_FLAG_SIZE)?.iter().enumerate() {
        let position = flag.bytes(36, 12)?;
        let at = |i: usize| f32::from_le_bytes(position[i * 4..i * 4 + 4].try_into().unwrap());
        println!("cutscene flag {index}: {} at {:.2},{:.2},{:.2}", text(flag.bytes(4, 32)?), at(0), at(1), at(2));
    }
    Ok(())
}

fn objects(cache: &CacheFile, list: bool) -> CliResult {
    let placements = objects::placements(cache)?;
    if list {
        for p in &placements {
            let name = p.definition.datum.map(|d| cache.tag(d).map(|t| t.name.clone())).transpose()?.unwrap_or_default();
            println!(
                "{:?} {name} at {:.2},{:.2},{:.2} yaw {:.1} bsps {:#x}{}{}{}",
                p.kind,
                p.position[0],
                p.position[1],
                p.position[2],
                p.rotation[0].to_degrees(),
                p.on_bsp_flags,
                if p.not_automatic { " (scripted)" } else { "" },
                p.light.map(|l| format!(" light {:?} x{} falloff {:.2} cutoff {:.2}", l.color, l.intensity, l.falloff_angle, l.cutoff_angle)).unwrap_or_default(),
                if p.region_permutations != [0; 8] { format!(" permutations {:?}", p.region_permutations) } else { String::new() }
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
