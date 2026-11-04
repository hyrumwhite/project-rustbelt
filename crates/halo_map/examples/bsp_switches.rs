//! Lists a scenario's BSP switch trigger volumes (the game switches when a
//! player's unit enters one while in its source BSP, players.c): each
//! volume's name, source and destination, and its box: `bsp_switches <map>`.
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let cache = halo_map::CacheFile::open(std::path::Path::new(&args[1])).unwrap();
    let scenario = cache.scenario().unwrap();
    // (struct scenario: trigger_volumes follows players, and
    // bsp_switch_trigger_volumes is five blocks on; struct
    // scenario_trigger_volume: 0x60 bytes, its name at 4, forward and up at
    // 0x30 and 0x3c, its position and extents at 0x48)
    let players = halo_map::layout::scenario::PLAYERS;
    let volumes = scenario.block(players + 12, 0x60).unwrap();
    let switches = scenario.block(players + 12 * 6, 8).unwrap();
    eprintln!("{} trigger volumes, {} switches", volumes.count, switches.count);
    for switch in switches.iter() {
        let [volume, source, destination, flag] = [0, 2, 4, 6].map(|o| {
            let b = switch.bytes(o, 2).unwrap();
            i16::from_le_bytes([b[0], b[1]])
        });
        let Some(v) = volumes.iter().nth(volume as usize) else {
            println!("volume {volume} (missing): bsp {source} -> {destination}");
            continue;
        };
        let name = String::from_utf8_lossy(v.bytes(4, 32).unwrap()).trim_end_matches('\0').to_string();
        let f = |o: usize| [v.f32(o).unwrap(), v.f32(o + 4).unwrap(), v.f32(o + 8).unwrap()];
        println!(
            "{name} (volume {volume}): bsp {source} -> {destination}, flag {flag}, forward {:?} up {:?} position {:?} extents {:?}",
            f(0x30), f(0x3c), f(0x48), f(0x54)
        );
    }
}
