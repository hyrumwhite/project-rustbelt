//! Prints each shader's reflection settings: `shader_shine <map> [name part]`.
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
        let Some(r) = info.reflection else { continue };
        println!(
            "{} ({:?}, env type {:?}): perpendicular {:.2} parallel {:.2} tint {:?} cube {} mask {:?}",
            info.name,
            info.shader_type,
            info.environment_type,
            r.perpendicular_brightness,
            r.parallel_brightness,
            r.tint,
            r.cube_map.is_some(),
            r.mask
        );
    }
}
