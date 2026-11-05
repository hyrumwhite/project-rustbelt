//! Which of a level's BSPs hold a point, and in which cluster: `point_bsp
//! <map> x,y,z [x,y,z ...]` (world units, as a capture's `--camera` begins).
//! For the level viewer's `--bsp N` at a capture's spot, and for checking
//! a light's or a cell's place.
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let cache = halo_map::CacheFile::open(std::path::Path::new(&args[1])).unwrap();
    let references = halo_map::bsp::references(&cache).unwrap();
    let bsps: Vec<_> = references
        .iter()
        .filter_map(|reference| {
            let bsp = halo_map::bsp::Bsp::load(&cache, reference).ok()?;
            let collision = halo_map::collision::CollisionBsp::load(&bsp).ok()?;
            let clusters = halo_map::collision::StructureClusters::load(&cache, &bsp).ok()?;
            Some((reference.index, reference.name.clone(), collision, clusters))
        })
        .collect();
    for text in &args[2..] {
        let values: Vec<f32> = text.split(',').take(3).filter_map(|v| v.trim().parse().ok()).collect();
        let [x, y, z] = values[..] else {
            eprintln!("{text}: expected x,y,z");
            continue;
        };
        let mut found = Vec::new();
        for (index, name, collision, clusters) in &bsps {
            let leaf = collision.leaf_from_point([x, y, z]);
            if leaf < 0 {
                continue;
            }
            found.push(format!("bsp {index} ({name}) leaf {leaf} cluster {}", clusters.cluster_from_leaf(leaf)));
        }
        if found.is_empty() {
            println!("{x},{y},{z}: outside every BSP");
        } else {
            println!("{x},{y},{z}: {}", found.join("; "));
        }
    }
}
