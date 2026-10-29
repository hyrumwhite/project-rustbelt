//! Prints where a BSP shader's triangles are (each BSP's count and bounds):
//! `bsp_shader_where <map> <shader name part>`.
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let cache = halo_map::CacheFile::open(std::path::Path::new(&args[1])).unwrap();
    for (index, reference) in halo_map::bsp::references(&cache).unwrap().iter().enumerate() {
        let Ok(bsp) = halo_map::bsp::Bsp::load(&cache, reference) else { continue };
        for material in bsp.materials().unwrap_or_default() {
            let name = material.shader.datum.and_then(|d| cache.tag(d).ok()).map_or(String::new(), |t| t.name.clone());
            if !name.contains(&args[2]) {
                continue;
            }
            let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
            for v in &material.vertices {
                for k in 0..3 {
                    lo[k] = lo[k].min(v.position[k]);
                    hi[k] = hi[k].max(v.position[k]);
                }
            }
            println!("bsp {index}: {name}: {} triangles, {lo:.1?} to {hi:.1?}", material.indices.len() / 3);
        }
    }
}
