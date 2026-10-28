//! Prints each model shader's detail map: `model_details <map> [name part]`.
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
        let Some(detail) = info.model_detail else { continue };
        let name = detail.map.datum.and_then(|d| cache.tags.iter().find(|t| t.datum == d)).map(|t| t.name.clone());
        println!("{}: {:?} function {} mask {} scale {:?}", info.name, name, detail.function, detail.mask, detail.scale);
    }
}
