//! Prints each shader's radiosity emission and self-illumination:
//! `shader_emission <map> [name part]`.
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
        println!(
            "{} ({:?}): emitted {:?} × {}, self-illumination {:?}",
            info.name, info.shader_type, info.emitted_light, info.emitted_power, info.self_illumination
        );
    }
}
