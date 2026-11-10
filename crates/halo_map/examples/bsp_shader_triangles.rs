//! Prints a BSP shader's triangles, one a line, tab-separated, for working
//! out a light's output offline: `bsp_shader_triangles <map> <shader name
//! part> [bsp index]`. Positions in world units, texcoords as stored
//! (unscaled), the area, and the normal of the face the game draws (its
//! triangles are clockwise from the front: the negated right-handed cross
//! product). Material indices count every material, as the level's do.
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let cache = halo_map::CacheFile::open(std::path::Path::new(&args[1])).unwrap();
    let only: Option<usize> = args.get(3).and_then(|a| a.parse().ok());
    println!(
        "bsp\tmaterial\ttriangle\tax\tay\taz\tbx\tby\tbz\tcx\tcy\tcz\tau\tav\tbu\tbv\tcu\tcv\tarea\tnx\tny\tnz"
    );
    for reference in halo_map::bsp::references(&cache).unwrap() {
        if only.is_some_and(|i| i != reference.index) {
            continue;
        }
        let Ok(bsp) = halo_map::bsp::Bsp::load(&cache, &reference) else { continue };
        for (material_index, material) in bsp.materials().unwrap_or_default().into_iter().enumerate() {
            let name = material.shader.datum.and_then(|d| cache.tag(d).ok()).map_or(String::new(), |t| t.name.clone());
            if !name.contains(&args[2]) {
                continue;
            }
            for (triangle, t) in material.indices.chunks_exact(3).enumerate() {
                let [a, b, c] = [0, 1, 2].map(|i| &material.vertices[t[i] as usize]);
                let sub = |p: [f32; 3], q: [f32; 3]| [p[0] - q[0], p[1] - q[1], p[2] - q[2]];
                let (e, f) = (sub(b.position, a.position), sub(c.position, a.position));
                let cross = [e[1] * f[2] - e[2] * f[1], e[2] * f[0] - e[0] * f[2], e[0] * f[1] - e[1] * f[0]];
                let length = (cross[0] * cross[0] + cross[1] * cross[1] + cross[2] * cross[2]).sqrt();
                // (clockwise from the front: the cross product points back)
                let normal = if length > 0.0 { cross.map(|x| -x / length) } else { [0.0; 3] };
                let mut fields = vec![reference.index.to_string(), material_index.to_string(), triangle.to_string()];
                for v in [a, b, c] {
                    fields.extend(v.position.iter().map(|x| format!("{x:.6}")));
                }
                for v in [a, b, c] {
                    fields.extend(v.texcoord.iter().map(|x| format!("{x:.6}")));
                }
                fields.push(format!("{:.6}", length / 2.0));
                fields.extend(normal.iter().map(|x| format!("{x:.6}")));
                println!("{}", fields.join("\t"));
            }
        }
    }
}
