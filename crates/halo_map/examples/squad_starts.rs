//! Lists a scenario's AI squads with their actor type and where they start:
//! `squad_starts <map> [name part]` (an encounter's or an actor's).
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let cache = halo_map::CacheFile::open(std::path::Path::new(&args[1])).unwrap();
    let scenario = cache.scenario().unwrap();
    let name = |bytes: &[u8]| String::from_utf8_lossy(bytes.split(|&b| b == 0).next().unwrap_or(&[])).into_owned();
    // (the actor palette, tag references, stands just before the encounters)
    let palette = scenario.block(halo_map::layout::scenario::AI_ENCOUNTERS - 12, 16).unwrap();
    let actor = |index: i16| {
        let datum = palette.iter().nth(index.max(0) as usize).and_then(|e| e.reference(0).ok()).and_then(|r| r.datum);
        datum.and_then(|datum| cache.tag(datum).ok()).map_or(String::from("none"), |tag| tag.name.to_string())
    };
    // (struct encounter: its squads' block at 0x80; struct squad: the actor
    // type at 0x20, the starting locations' block at 0xd0, each a position,
    // a facing and, at 0x18, an actor type of its own or NONE; the squad's
    // flags at 0x28 and its actors' count on normal and on legendary at 0x7c)
    let encounters = scenario.block(halo_map::layout::scenario::AI_ENCOUNTERS, 0xb0).unwrap();
    for encounter in encounters.iter() {
        let encounter_name = name(encounter.bytes(0, 32).unwrap());
        let Ok(squads) = encounter.block(0x80, 0xe8) else { continue };
        for squad in squads.iter() {
            let squad_actor = actor(squad.i16(0x20).unwrap());
            let starts = squad.block(0xd0, 0x1c).map(|starts| {
                starts.iter().map(|s| (s.vec3(0).unwrap(), s.i16(0x18).unwrap())).collect::<Vec<_>>()
            });
            let starts = starts.unwrap_or_default();
            let line = format!(
                "{encounter_name}/{}: {squad_actor}, flags {:#x}, {} to {} actors, starts {}",
                name(squad.bytes(0, 32).unwrap()),
                squad.u16(0x28).unwrap(),
                squad.i16(0x7c).unwrap(),
                squad.i16(0x7e).unwrap(),
                starts.iter().map(|(p, a)| format!("[{:.1}, {:.1}, {:.1}]{}", p[0], p[1], p[2], if *a >= 0 { format!(" ({})", actor(*a)) } else { String::new() })).collect::<Vec<_>>().join(" ")
            );
            if args.get(2).is_none_or(|part| line.contains(part.as_str())) {
                println!("{line}");
            }
        }
    }
}
