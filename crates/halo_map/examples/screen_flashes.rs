//! Prints each damage effect's screen flash: `screen_flashes <map> [name part]`.
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let cache = halo_map::CacheFile::open(std::path::Path::new(&args[1])).unwrap();
    let filter = args.get(2).map(String::as_str).unwrap_or("");
    let group = u32::from_be_bytes(*b"jpt!");
    for tag in &cache.tags {
        if !tag.is_a(group) || !tag.name.contains(filter) {
            continue;
        }
        let Ok(root) = cache.tag_root(tag.datum, group) else { continue };
        let flash = root.add(halo_map::layout::damage_effect_definition::SCREEN_FLASH);
        let (Ok(kind), Ok(priority)) = (flash.i16(0), flash.i16(2)) else { continue };
        if kind == 0 {
            continue;
        }
        let f = |offset| flash.f32(offset).unwrap_or(f32::NAN);
        println!(
            "{}: type {kind} priority {priority} duration {:.2} fade {} max intensity {:.2} zero scale {:.2} argb ({:.2}, {:.2}, {:.2}, {:.2})",
            tag.name,
            f(0x10),
            flash.i16(0x14).unwrap_or(-1),
            f(0x20),
            f(0x24),
            f(0x28),
            f(0x2c),
            f(0x30),
            f(0x34)
        );
    }
}
