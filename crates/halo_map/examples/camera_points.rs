//! Lists a scenario's cutscene camera points (index, name, position) and
//! trigger volumes and starting profiles (index, name): `camera_points <map>`.
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let cache = halo_map::CacheFile::open(std::path::Path::new(&args[1])).unwrap();
    let scenario = cache.scenario().unwrap();
    // (struct scenario's cutscene_camera_points; struct
    // scenario_cutscene_camera_point: flags, a name, padding, a position)
    let points = scenario.block(0x4f0, 0x68).unwrap();
    for (index, point) in points.iter().enumerate() {
        let name = point.add(4).c_string(32).unwrap();
        println!("{index}: {name} {:?}", point.vec3(0x28).unwrap());
    }
    // (struct scenario's trigger_volumes follow its players; struct
    // scenario_trigger_volume: a type, padding, a name)
    let volumes = scenario.block(halo_map::layout::scenario::PLAYERS + 12, 0x60).unwrap();
    for (index, volume) in volumes.iter().enumerate() {
        println!("trigger volume {index}: {}", volume.add(4).c_string(32).unwrap());
    }
    // (struct scenario's starting_profiles precede its players; a profile's
    // name first)
    let profiles = scenario.block(halo_map::layout::scenario::PLAYERS - 12, 0x68).unwrap();
    for (index, profile) in profiles.iter().enumerate() {
        println!("starting profile {index}: {}", profile.c_string(32).unwrap());
    }
}
