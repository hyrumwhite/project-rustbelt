//! Prints a light tag's gel as beams (see `light_gel`): `gel_beams <map> <light name part>`.
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let cache = halo_map::CacheFile::open(std::path::Path::new(&args[1])).unwrap();
    for tag in cache.tags.iter().filter(|t| halo_map::fourcc_str(t.group) == "ligh" && t.name.contains(&args[2])) {
        println!("{}:", tag.name);
        for beam in halo_map::light_gel::gel_beams(&cache, tag.datum).unwrap_or_default() {
            println!("  {beam:?}");
        }
    }
}
