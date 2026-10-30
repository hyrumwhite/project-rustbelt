//! Lists a scenario's AI encounters' names, and their squads':
//! `encounter_names <map>`.
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let cache = halo_map::CacheFile::open(std::path::Path::new(&args[1])).unwrap();
    let scenario = cache.scenario().unwrap();
    let name = |bytes: &[u8]| String::from_utf8_lossy(bytes.split(|&b| b == 0).next().unwrap_or(&[])).into_owned();
    // (struct encounter: its name first, ended by a zero; its squads'
    // block at 0x80, each squad's name first)
    let encounters = scenario.block(halo_map::layout::scenario::AI_ENCOUNTERS, 0xb0).unwrap();
    for encounter in encounters.iter() {
        let squads = encounter.block(0x80, 0xe8).map(|squads| squads.iter().map(|s| name(s.bytes(0, 32).unwrap())).collect::<Vec<_>>());
        println!("{}: {:?}", name(encounter.bytes(0, 32).unwrap()), squads.unwrap_or_default());
    }
}
