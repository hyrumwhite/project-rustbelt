//! Prints a model's regions, permutations and the geometry (with its triangle
//! count) at each level of detail: `model_lods <map> <tag name part>`.
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let cache = halo_map::CacheFile::open(std::path::Path::new(&args[1])).unwrap();
    for tag in &cache.tags {
        let datum = tag.datum;
        if !tag.name.contains(&args[2]) || !tag.is_a(u32::from_be_bytes(*b"mode")) {
            continue;
        }
        let Ok(model) = halo_map::model::Model::load(&cache, datum) else { continue };
        println!("{} ({} geometries, {} nodes)", model.name, model.geometry_count, model.nodes.len());
        for region in &model.regions {
            for permutation in &region.permutations {
                let counts: Vec<String> = permutation
                    .geometry_indices
                    .iter()
                    .map(|&g| {
                        if g < 0 {
                            "-".into()
                        } else {
                            let triangles: usize = model.geometry(g as usize).map(|p| p.iter().map(|p| p.indices.len() / 3).sum()).unwrap_or(0);
                            format!("{g}:{triangles}")
                        }
                    })
                    .collect();
                println!("  {} / {}: {}", region.name, permutation.name, counts.join(" "));
            }
        }
    }
}
