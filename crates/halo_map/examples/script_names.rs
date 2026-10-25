//! Lists a scenario's scripts (name, type): `script_names <map>`.
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let cache = halo_map::CacheFile::open(std::path::Path::new(&args[1])).unwrap();
    let scenario = cache.scenario().unwrap();
    // (struct scenario: hs_scripts follows ai_conversations, hs_syntax_data
    // and hs_string_constants; struct hs_script: a name, its type)
    let scripts = scenario.block(halo_map::layout::scenario::AI_CONVERSATIONS + 12 + 20 + 20, 0x5c).unwrap();
    const TYPES: [&str; 5] = ["startup", "dormant", "continuous", "static", "stub"];
    for script in scripts.iter() {
        let name = String::from_utf8_lossy(script.bytes(0, 32).unwrap()).trim_end_matches('\0').to_string();
        let kind = script.bytes(32, 2).unwrap();
        let kind = i16::from_le_bytes([kind[0], kind[1]]);
        println!("{name} ({})", TYPES.get(kind as usize).unwrap_or(&"?"));
    }
}
