//! Writes one geometry of a model as Wavefront OBJ (positions, normals, one
//! group per part, the game's winding): `model_obj <map> <tag name part>
//! <geometry index> <out.obj>`.
use std::fmt::Write;
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let cache = halo_map::CacheFile::open(std::path::Path::new(&args[1])).unwrap();
    let index: usize = args[3].parse().unwrap();
    let tag = cache.tags.iter().find(|t| t.name.contains(&args[2]) && t.is_a(u32::from_be_bytes(*b"mode"))).expect("no such model");
    let model = halo_map::model::Model::load(&cache, tag.datum).unwrap();
    let mut out = String::new();
    let mut base = 1;
    for (p, part) in model.geometry(index).unwrap().iter().enumerate() {
        writeln!(out, "g part{p}_shader{}", part.shader_index).unwrap();
        let shader = model.shaders.get(part.shader_index as usize).copied();
        if let Some(Ok(Some(info))) = shader.map(|s| halo_map::shader::shader_info(&cache, s)) {
            eprintln!("part {p}: {} two sided {} alpha tested {}", info.name, info.two_sided, info.alpha_tested);
        }
        for v in &part.vertices {
            if std::env::var_os("MODEL_OBJ_NODES").is_some() {
                eprintln!("  v {:.3} {:.3} {:.3} nodes {:?} weights {:?}", v.position[0], v.position[1], v.position[2], v.nodes, v.node_weights);
            }
            writeln!(out, "v {} {} {}\nvn {} {} {}", v.position[0], v.position[1], v.position[2], v.normal[0], v.normal[1], v.normal[2]).unwrap();
        }
        for t in part.indices.chunks_exact(3) {
            let [a, b, c] = [t[0], t[1], t[2]].map(|i| i as usize + base);
            writeln!(out, "f {a}//{a} {b}//{b} {c}//{c}").unwrap();
        }
        base += part.vertices.len();
    }
    std::fs::write(&args[4], out).unwrap();
}
