//! Lists a scenario's device groups (name, initial value), for script
//! commands that name one: `device_groups <map>`.
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let cache = halo_map::CacheFile::open(std::path::Path::new(&args[1])).unwrap();
    let scenario = cache.scenario().unwrap();
    // (struct scenario's device_groups, of struct scenario_device_group: a
    // name, then its initial value at 0x20; tag_layout's probe)
    let groups = scenario.block(0x288, 0x34).unwrap();
    for (index, group) in groups.iter().enumerate() {
        let name = String::from_utf8_lossy(group.bytes(0, 32).unwrap()).trim_end_matches('\0').to_string();
        let value = group.bytes(0x20, 4).unwrap();
        println!("{index}: {name} ({})", f32::from_le_bytes([value[0], value[1], value[2], value[3]]));
    }
}
