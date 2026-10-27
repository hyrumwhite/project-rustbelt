//! Lists a scenario's cutscene camera points (index, name, position):
//! `camera_points <map>`.
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
}
