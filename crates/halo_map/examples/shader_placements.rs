//! Prints the placed objects whose models use a shader: `shader_placements <map> <shader name part>`.
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let cache = halo_map::CacheFile::open(std::path::Path::new(&args[1])).unwrap();
    for placement in halo_map::objects::placements(&cache).unwrap() {
        let Some(model) = halo_map::objects::object_model(&cache, placement.definition).ok().flatten().and_then(|m| m.datum) else { continue };
        let Ok(model) = halo_map::model::Model::load(&cache, model) else { continue };
        let names: Vec<String> = model.shaders.iter().filter_map(|s| s.datum).filter_map(|d| cache.tag(d).ok()).map(|t| t.name.clone()).collect();
        if names.iter().any(|n| n.contains(&args[2])) {
            let definition = placement.definition.datum.and_then(|d| cache.tag(d).ok()).map(|t| t.name.clone());
            println!("{definition:?} at {:?} bsps {:#x}", placement.position, placement.on_bsp_flags);
        }
    }
}
