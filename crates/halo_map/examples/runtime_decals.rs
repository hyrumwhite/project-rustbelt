//! Counts each structure BSP's runtime decals (the level's placed decals):
//! `runtime_decals <map>`.
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let cache = halo_map::CacheFile::open(std::path::Path::new(&args[1])).unwrap();
    for reference in halo_map::bsp::references(&cache).unwrap() {
        let loaded = halo_map::bsp::Bsp::load(&cache, &reference).unwrap();
        // (struct structure_bsp: the runtime decals after the sound cluster
        // data, its unused longs, the markers and the detail objects)
        let offset = halo_map::layout::structure_bsp::SOUND_CLUSTER_DATA + 0x14 + 0x18 + 0x0c + 0x0c;
        let count = loaded.root.block(offset, 16).map(|b| b.iter().count()).unwrap_or(0);
        println!("bsp {} {}: {count} runtime decals", reference.index, reference.name);
    }
}
