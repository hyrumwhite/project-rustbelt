//! Prints an object definition's model's shaders (with their permutations)
//! and regions: `object_shaders <map> <object name part>`.
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let cache = halo_map::CacheFile::open(std::path::Path::new(&args[1])).unwrap();
    let mut seen = std::collections::HashSet::new();
    for placement in halo_map::objects::placements(&cache).unwrap() {
        let Some(definition) = placement.definition.datum.and_then(|d| cache.tag(d).ok()) else { continue };
        if !definition.name.contains(&args[2]) || !seen.insert(definition.name.clone()) {
            continue;
        }
        let Some(model) = halo_map::objects::object_model(&cache, placement.definition).ok().flatten().and_then(|m| m.datum) else { continue };
        let Ok(model) = halo_map::model::Model::load(&cache, model) else { continue };
        println!("{} ({}):", definition.name, model.name);
        for (index, shader) in model.shaders.iter().enumerate() {
            let name = shader.datum.and_then(|d| cache.tag(d).ok()).map_or("?".to_string(), |t| t.name.clone());
            println!("  shader {index}: {name} permutation {}", model.shader_permutations.get(index).copied().unwrap_or(0));
        }
        for region in &model.regions {
            println!("  region {}: {} permutations", region.name, region.permutations.len());
        }
    }
}
