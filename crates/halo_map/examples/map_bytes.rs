//! Writes a map's decompressed bytes (to grep for script and object names):
//! `map_bytes <map> <out>`.
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let cache = halo_map::CacheFile::open(std::path::Path::new(&args[1])).unwrap();
    std::fs::write(&args[2], cache.bytes()).unwrap();
}
