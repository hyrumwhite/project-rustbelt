//! Lists each structure BSP's lens flare markers (the level's own glows):
//! `bsp_lens_flares <map> [x,y,z radius]`, those within the radius only.
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let cache = halo_map::CacheFile::open(std::path::Path::new(&args[1])).unwrap();
    let near: Option<([f32; 3], f32)> = args.get(2).map(|p| {
        let v: Vec<f32> = p.split(',').map(|x| x.parse().unwrap()).collect();
        ([v[0], v[1], v[2]], args.get(3).map_or(10.0, |r| r.parse().unwrap()))
    });
    for reference in halo_map::bsp::references(&cache).unwrap() {
        let loaded = halo_map::bsp::Bsp::load(&cache, &reference).unwrap();
        // (struct structure_bsp: the lens flares, then their markers, before the clusters)
        let clusters = halo_map::layout::structure_bsp::CLUSTERS;
        let flares: Vec<String> = loaded
            .root
            .block(clusters - 0x18, 16)
            .map(|b| b.iter().map(|f| f.reference(0).ok().and_then(|r| r.datum).and_then(|d| cache.tag(d).ok()).map_or("?".into(), |t| t.name.clone())).collect())
            .unwrap_or_default();
        let Ok(markers) = loaded.root.block(clusters - 0xc, 16) else { continue };
        for marker in markers.iter() {
            let p = [marker.f32(0).unwrap(), marker.f32(4).unwrap(), marker.f32(8).unwrap()];
            if near.is_some_and(|(c, r)| (0..3).map(|k| (p[k] - c[k]).powi(2)).sum::<f32>().sqrt() > r) {
                continue;
            }
            let bytes = marker.bytes(12, 4).unwrap();
            let direction = [bytes[0] as i8 as f32 / 127.0, bytes[1] as i8 as f32 / 127.0, bytes[2] as i8 as f32 / 127.0];
            let flare = flares.get(bytes[3] as usize).cloned().unwrap_or_default();
            println!("bsp {} {}: {p:?} along {direction:.2?}: {flare}", reference.index, reference.name);
        }
    }
}
