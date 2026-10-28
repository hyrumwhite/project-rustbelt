//! Prints a map's skies and the skies its BSPs' clusters use: `skies <map>`.
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let cache = halo_map::CacheFile::open(std::path::Path::new(&args[1])).unwrap();
    for (index, sky) in halo_map::sky::skies(&cache).unwrap().iter().enumerate() {
        let model = sky.model.and_then(|m| m.datum).and_then(|d| cache.tag(d).ok()).map(|t| t.name.clone());
        println!("sky {index}: {} model {:?} lights {}", sky.name, model, sky.lights.len());
    }
    use halo_map::layout::{structure_bsp, structure_cluster};
    for reference in halo_map::bsp::references(&cache).unwrap() {
        let bsp = halo_map::bsp::Bsp::load(&cache, &reference).unwrap();
        let clusters = bsp.root.block(structure_bsp::CLUSTERS, structure_cluster::SIZEOF).unwrap();
        let mut counts = std::collections::BTreeMap::new();
        for c in clusters.iter() {
            *counts.entry(c.i16(structure_cluster::SKY_INDEX).unwrap()).or_insert(0) += 1;
        }
        println!("bsp {}: clusters by sky index {:?}", reference.index, counts);
    }
}
