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
        // (and the rest of the definition, rasterizer_xbox_transparent_geometry.c's layout)
        if let Ok(root) = cache.tag_root(tag.datum, tag.group) {
            let reference = |offset| root.reference(offset).ok().and_then(|r| name(Some(r)));
            println!(
                "  flags {:#x} reflection type {} map {:?} bump map {:?} (scale {}) detail map {:?} (scale {})",
                root.u16(0x28).unwrap_or(0),
                root.i16(0x8a).unwrap_or(-1),
                reference(0xac),
                reference(0xc0),
                root.f32(0xbc).unwrap_or(0.0),
                reference(0x16c),
                root.f32(0x168).unwrap_or(0.0)
            );
        }
    }
}
