//! Prints the placed light fixtures' lights (colour, intensity, angles,
//! forward axis): `fixture_lights <map> [name part]`.
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let cache = halo_map::CacheFile::open(std::path::Path::new(&args[1])).unwrap();
    let filter = args.get(2).map(String::as_str).unwrap_or("");
    for placement in halo_map::objects::placements(&cache).unwrap() {
        let Some(light) = placement.light else { continue };
        let name = placement.definition.datum.and_then(|d| cache.tag(d).ok()).map_or("?".to_string(), |t| t.name.clone());
        if !name.contains(filter) {
            continue;
        }
        let forward = placement.axes()[0];
        println!(
            "{name} at {:.2?} bsps {:#x}: colour {:.2?} intensity {:.2} falloff {:.1} cutoff {:.1} forward {:.2?}",
            placement.position, placement.on_bsp_flags, light.color, light.intensity, light.falloff_angle.to_degrees(), light.cutoff_angle.to_degrees(), forward
        );
    }
}
