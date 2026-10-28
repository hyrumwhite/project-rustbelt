//! Prints the tags whose names hold a word, with their groups: `tag_names <map> <name part>`.
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let cache = halo_map::CacheFile::open(std::path::Path::new(&args[1])).unwrap();
    for tag in cache.tags.iter().filter(|t| t.name.contains(&args[2])) {
        println!("{} ({})", tag.name, halo_map::fourcc_str(tag.group));
    }
}
