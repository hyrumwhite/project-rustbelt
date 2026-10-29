//! Prints the placed objects near a point: `placements_near <map> x,y,z [radius]`.
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let cache = halo_map::CacheFile::open(std::path::Path::new(&args[1])).unwrap();
    let p: Vec<f32> = args[2].split(',').map(|v| v.parse().unwrap()).collect();
    let radius: f32 = args.get(3).map_or(5.0, |r| r.parse().unwrap());
    for placement in halo_map::objects::placements(&cache).unwrap() {
        let d = (0..3).map(|k| (placement.position[k] - p[k]).powi(2)).sum::<f32>().sqrt();
        if d > radius {
            continue;
        }
        let name = placement.definition.datum.and_then(|d| cache.tag(d).ok()).map_or("?".to_string(), |t| t.name.clone());
        println!("{d:.1}: {name} at {:.2?} rotation {:.2?} bsps {:#x}", placement.position, placement.rotation, placement.on_bsp_flags);
    }
}
