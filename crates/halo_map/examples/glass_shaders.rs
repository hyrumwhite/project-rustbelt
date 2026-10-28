//! Prints each transparent glass shader's settings: `glass_shaders <map> [name part]`.
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let cache = halo_map::CacheFile::open(std::path::Path::new(&args[1])).unwrap();
    let filter = args.get(2).map(String::as_str).unwrap_or("");
    for tag in &cache.tags {
        if !tag.name.contains(filter) {
            continue;
        }
        let reference = halo_map::TagRef { group: 0, datum: Some(tag.datum) };
        let Ok(Some(info)) = halo_map::shader::shader_info(&cache, reference) else { continue };
        let Some(glass) = info.glass else { continue };
        let name = |r: Option<halo_map::TagRef>| r.and_then(|r| r.datum).and_then(|d| cache.tag(d).ok()).map(|t| t.name.clone());
        println!("{}: tint {:?} tint map {:?} diffuse map {:?} reflection perpendicular {:?} parallel {:?}", info.name, glass.tint, name(glass.tint_map), name(glass.diffuse_map), glass.reflection_perpendicular, glass.reflection_parallel);
    }
}
