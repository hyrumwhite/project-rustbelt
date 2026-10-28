//! Prints the BSP shaders with triangles near a point: `bsp_shaders_near <map> x,y,z [radius]`.
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let cache = halo_map::CacheFile::open(std::path::Path::new(&args[1])).unwrap();
    let p: Vec<f32> = args[2].split(',').map(|v| v.parse().unwrap()).collect();
    let radius: f32 = args.get(3).map_or(3.0, |r| r.parse().unwrap());
    for (index, reference) in halo_map::bsp::references(&cache).unwrap().iter().enumerate() {
        let Ok(bsp) = halo_map::bsp::Bsp::load(&cache, reference) else { continue };
        for material in bsp.materials().unwrap_or_default() {
            let near = material.indices.chunks_exact(3).filter(|t| {
                t.iter().any(|&i| {
                    let v = material.vertices[i as usize].position;
                    (0..3).map(|k| (v[k] - p[k]).powi(2)).sum::<f32>() < radius * radius
                })
            });
            let count = near.count();
            if count == 0 {
                continue;
            }
            let name = material.shader.datum.and_then(|d| cache.tag(d).ok()).map_or("?".to_string(), |t| format!("{} ({})", t.name, halo_map::fourcc_str(t.group)));
            println!("bsp {index}: {name}: {count} triangles");
            if std::env::var("VERTS").is_ok_and(|v| name.contains(&v)) {
                for t in material.indices.chunks_exact(3) {
                    let v: Vec<[f32; 3]> = t.iter().map(|&i| material.vertices[i as usize].position).collect();
                    if v.iter().any(|v| (0..3).map(|k| (v[k] - p[k]).powi(2)).sum::<f32>() < radius * radius) {
                        println!("  {:?} uv {:?}", v, t.iter().map(|&i| material.vertices[i as usize].texcoord).collect::<Vec<_>>());
                    }
                }
            }
        }
    }
}
