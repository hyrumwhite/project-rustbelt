//! Writes one geometry's triangles with their texture coordinates, one line
//! per triangle (`shader x y z u v` for each corner): `model_uvs <map> <tag
//! name part> <geometry index> <out.txt>`.
use std::fmt::Write;
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let cache = halo_map::CacheFile::open(std::path::Path::new(&args[1])).unwrap();
    let index: usize = args[3].parse().unwrap();
    let tag = cache.tags.iter().find(|t| t.name.contains(&args[2]) && t.is_a(u32::from_be_bytes(*b"mode"))).expect("no such model");
    let model = halo_map::model::Model::load(&cache, tag.datum).unwrap();
    let mut out = String::new();
    for part in model.geometry(index).unwrap().iter() {
        for t in part.indices.chunks_exact(3) {
            write!(out, "{}", part.shader_index).unwrap();
            for &i in t {
                let v = &part.vertices[i as usize];
                write!(out, " {} {} {} {} {}", v.position[0], v.position[1], v.position[2], v.texcoord[0], v.texcoord[1]).unwrap();
            }
            out.push('\n');
        }
    }
    std::fs::write(&args[4], out).unwrap();
}
