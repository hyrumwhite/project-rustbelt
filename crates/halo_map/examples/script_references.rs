//! Lists the tags a scenario's scripts reference (group, name), the only
//! tag names a console command's strings resolve to:
//! `script_references <map> [group]`.
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let cache = halo_map::CacheFile::open(std::path::Path::new(&args[1])).unwrap();
    let scenario = cache.scenario().unwrap();
    // (struct scenario: hs_references follows ai_conversations,
    // hs_syntax_data, hs_string_constants, hs_scripts and hs_globals;
    // struct hs_reference: 24 bytes, then its tag reference)
    let references = scenario.block(halo_map::layout::scenario::AI_CONVERSATIONS + 12 + 20 + 20 + 12 + 12, 0x28).unwrap();
    for reference in references.iter() {
        let group = reference.bytes(24, 4).unwrap();
        let group: String = group.iter().rev().map(|&b| b as char).collect();
        if args.get(2).is_some_and(|wanted| *wanted != group) {
            continue;
        }
        let index = reference.u32(24 + 12).unwrap();
        let name = cache.tag(index).map(|tag| tag.name.clone()).unwrap_or_else(|_| "?".into());
        println!("{group} {name}");
    }
}
