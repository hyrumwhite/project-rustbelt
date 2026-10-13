//! Lists a scenario's cutscene flags (name, position) and which structure
//! BSPs' world bounds hold each: `cutscene_flags <map> [name part]`.
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let cache = halo_map::CacheFile::open(std::path::Path::new(&args[1])).unwrap();
    let scenario = cache.scenario().unwrap();
    // (struct scenario: the cutscene flags come 192 bytes before the
    // structure BSP references)
    let flags = scenario.block(halo_map::layout::scenario::STRUCTURE_BSP_REFERENCES - 192, 0x5c).unwrap();
    let bsps: Vec<_> = halo_map::bsp::references(&cache)
        .unwrap()
        .into_iter()
        .map(|reference| {
            let bounds = halo_map::bsp::Bsp::load(&cache, &reference).and_then(|b| b.world_bounds()).ok();
            (reference.index, reference.name, bounds)
        })
        .collect();
    for flag in flags.iter() {
        let name = String::from_utf8_lossy(flag.bytes(4, 32).unwrap()).trim_end_matches('\0').to_string();
        if args.get(2).is_some_and(|part| !name.contains(part.as_str())) {
            continue;
        }
        let position = [flag.f32(36).unwrap(), flag.f32(40).unwrap(), flag.f32(44).unwrap()];
        let inside: Vec<String> = bsps
            .iter()
            .filter(|(_, _, bounds)| {
                bounds.is_some_and(|b| (0..3).all(|k| position[k] >= b[k][0] && position[k] <= b[k][1]))
            })
            .map(|(index, name, _)| format!("{index} ({name})"))
            .collect();
        println!("{name}: {position:?} in bsp {}", inside.join(", "));
    }
}
