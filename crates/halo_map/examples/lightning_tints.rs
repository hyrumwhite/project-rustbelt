//! Prints each lightning tag's markers (tint, thickness, jitter) and colour
//! sources: `lightning_tints <map> [name part]`.
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let cache = halo_map::CacheFile::open(std::path::Path::new(&args[1])).unwrap();
    let filter = args.get(2).map(String::as_str).unwrap_or("");
    let elec = u32::from_be_bytes(*b"elec");
    for tag in cache.tags.iter().filter(|t| t.group == elec && t.name.contains(filter)) {
        let Ok(root) = cache.tag_root(tag.datum, elec) else { continue };
        let short = |o: usize| root.u16(o).unwrap_or(0) as i16;
        println!(
            "{}: sources jitter {} thickness {} tint {} brightness {}",
            tag.name, short(0x2c), short(0x2e), short(0x30), short(0x32)
        );
        let Ok(markers) = root.block(0x98, 0xe4) else { continue };
        for marker in markers.iter() {
            let f = |o: usize| marker.f32(o).unwrap_or(0.0);
            println!(
                "  octaves {} jitter {:.3} thickness {:.3} tint argb [{:.2}, {:.2}, {:.2}, {:.2}]",
                marker.u16(0x24).unwrap_or(0), f(0x80), f(0x84), f(0x88), f(0x8c), f(0x90), f(0x94)
            );
        }
    }
}
