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
        // (a bitmap's first image's format and size)
        let format = |r: Option<halo_map::TagRef>| {
            r.and_then(|r| r.datum)
                .and_then(|d| halo_map::bitmap::bitmaps(&cache, d).ok())
                .and_then(|b| b.first().map(|b| format!("{:?} {}x{}", b.format, b.width, b.height)))
        };
        println!("{}: tint {:?} tint map {:?} diffuse map {:?} reflection perpendicular {:?} parallel {:?}", info.name, glass.tint, name(glass.tint_map), name(glass.diffuse_map), glass.reflection_perpendicular, glass.reflection_parallel);
        println!(
            "  flags {:#x} reflection type {} map {:?} bump map {:?} [{}] (scale {}) bumped {} diffuse scale {} detail map {:?} (scale {}) uv scale {:?}",
            glass.flags,
            glass.reflection_type,
            name(glass.reflection_map),
            name(glass.reflection_bump_map),
            format(glass.reflection_bump_map).unwrap_or_default(),
            glass.reflection_bump_map_scale,
            glass.bumped_reflection(),
            glass.diffuse_map_scale,
            name(glass.diffuse_detail_map),
            glass.diffuse_detail_map_scale,
            info.uv_scale
        );
    }
}
