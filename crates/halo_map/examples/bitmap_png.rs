//! Writes a bitmap tag's first image as raw RGBA: `bitmap_png <map> <tag name> <out.rgba>`; prints its size.
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let cache = halo_map::CacheFile::open(std::path::Path::new(&args[1])).unwrap();
    let tag = cache.tags.iter().find(|t| t.name == args[2] && halo_map::fourcc_str(t.group) == "bitm").expect("no such bitmap");
    let bitmaps = halo_map::bitmap::bitmaps(&cache, tag.datum).unwrap();
    let d = bitmaps[0].decode_rgba8(&cache).unwrap();
    let n = (d.width * d.height) as usize;
    std::fs::write(&args[3], &d.data[..n * 4]).unwrap();
    println!("{} {}", d.width, d.height);
}
