//! Prints a model's regions, permutations and the geometry (with its triangle
//! count) at each level of detail: `model_lods <map> <tag name part>`.
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let cache = halo_map::CacheFile::open(std::path::Path::new(&args[1])).unwrap();
    for tag in &cache.tags {
        let datum = tag.datum;
        if !tag.name.contains(&args[2]) || !tag.is_a(u32::from_be_bytes(*b"mode")) {
            continue;
        }
        let Ok(model) = halo_map::model::Model::load(&cache, datum) else { continue };
        println!("{} ({} geometries, {} nodes, cutoffs {:?} px)", model.name, model.geometry_count, model.nodes.len(), model.detail_cutoff_pixels);
        for region in &model.regions {
            for permutation in &region.permutations {
                let counts: Vec<String> = permutation
                    .geometry_indices
                    .iter()
                    .map(|&g| {
                        if g < 0 {
                            "-".into()
                        } else {
                            let triangles: usize = model.geometry(g as usize).map(|p| p.iter().map(|p| p.indices.len() / 3).sum()).unwrap_or(0);
                            format!("{g}:{triangles}")
                        }
                    })
                    .collect();
                println!("  {} / {}: {}", region.name, permutation.name, counts.join(" "));
                // the triangles whose winding (clockwise from the front, the
                // game's) faces away from their vertices' normals
                let top = permutation.geometry_indices[halo_map::model::HIGHEST_DETAIL];
                if top >= 0 {
                    for (part_index, part) in model.geometry(top as usize).unwrap_or_default().iter().enumerate() {
                        let mut against = 0;
                        for t in part.indices.chunks_exact(3) {
                            let [a, b, c] = [t[0], t[1], t[2]].map(|i| part.vertices[i as usize]);
                            let e1 = [b.position[0] - a.position[0], b.position[1] - a.position[1], b.position[2] - a.position[2]];
                            let e2 = [c.position[0] - a.position[0], c.position[1] - a.position[1], c.position[2] - a.position[2]];
                            let face = [e1[1] * e2[2] - e1[2] * e2[1], e1[2] * e2[0] - e1[0] * e2[2], e1[0] * e2[1] - e1[1] * e2[0]];
                            let normal: Vec<f32> = (0..3).map(|k| a.normal[k] + b.normal[k] + c.normal[k]).collect();
                            // (clockwise from the front: the cross product points back)
                            if face[0] * normal[0] + face[1] * normal[1] + face[2] * normal[2] > 0.0 {
                                against += 1;
                                if std::env::var_os("MODEL_LODS_TRIANGLES").is_some() {
                                    let p = |v: &halo_map::model::Vertex| {
                                        format!("({:.3} {:.3} {:.3} n {:.2} {:.2} {:.2})", v.position[0], v.position[1], v.position[2], v.normal[0], v.normal[1], v.normal[2])
                                    };
                                    println!("      {} {} {}", p(&a), p(&b), p(&c));
                                }
                            }
                        }
                        if against > 0 {
                            println!("    part {part_index} (shader {}): {against} of {} triangles wound against their normals", part.shader_index, part.indices.len() / 3);
                        }
                    }
                }
            }
        }
    }
}
