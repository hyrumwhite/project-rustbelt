//! Prints each sky's outdoor and indoor atmospheric fog (colour, maximum
//! density, start and opaque distances) and model: `sky_fog <map>`.
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let cache = halo_map::CacheFile::open(std::path::Path::new(&args[1])).unwrap();
    for tag in cache.tags.iter().filter(|t| t.group == halo_map::groups::SKY) {
        let Ok(root) = cache.tag_root(tag.datum, halo_map::groups::SKY) else { continue };
        let model = root.reference(0).ok().and_then(|r| r.datum);
        println!("{} (model {model:?}):", tag.name);
        for (name, at) in [("outdoor", 0x58), ("indoor", 0x78)] {
            let f = |o: usize| root.f32(at + o).unwrap_or(0.0);
            println!(
                "  {name} fog colour [{:.3}, {:.3}, {:.3}] maximum density {:.3} start {:.1} opaque {:.1}",
                f(0), f(4), f(8), f(0x14), f(0x18), f(0x1c)
            );
        }
    }
}
