//! Lists an animation graph's animations (index, name, type, frames):
//! `animation_names <map> <graph tag name>`.
fn main() {
    use halo_map::layout::{animation as a, animation_graph as g};
    let args: Vec<String> = std::env::args().collect();
    let cache = halo_map::CacheFile::open(std::path::Path::new(&args[1])).unwrap();
    let antr = halo_map::fourcc(b"antr");
    let tag = cache.find_tag(antr, &args[2]).expect("no such animation graph");
    let graph = cache.tag_root(tag.datum, antr).unwrap();
    const TYPES: [&str; 3] = ["base", "overlay", "replacement"];
    for (index, animation) in graph.block(g::ANIMATIONS, a::SIZEOF).unwrap().iter().enumerate() {
        let name = animation.c_string(32).unwrap();
        let kind = animation.i16(a::TYPE).unwrap();
        let frames = animation.i16(a::FRAME_COUNT).unwrap();
        println!("{index}: {name} ({}, {frames} frames)", TYPES.get(kind as usize).unwrap_or(&"?"));
    }
}
