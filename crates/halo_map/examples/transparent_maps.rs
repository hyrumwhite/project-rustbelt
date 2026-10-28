//! Prints each transparent shader's maps (bitmap, images, size, transform,
//! animation) and stage colours: `transparent_maps <map> [name part]`.
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
        let Some(t) = info.transparent.as_ref() else { continue };
        println!(
            "{} ({:?}): blend {:?} flags {:#04x} first map type {} fade {} {} extra layers {} extra flags {:#x}",
            info.name, info.shader_type, t.blend, t.flags, t.first_map_type, t.fade_mode, t.fade_source, t.extra_layers.len(), t.extra_flags
        );
        for (stage, map) in t.maps.iter().enumerate() {
            let Some(map) = map else {
                println!("  map {stage}: none");
                continue;
            };
            let bitmap = map.bitmap.datum.and_then(|d| cache.tag(d).ok()).map_or("?".to_string(), |t| t.name.clone());
            let images = map.bitmap.datum.and_then(|d| halo_map::bitmap::bitmaps(&cache, d).ok()).unwrap_or_default();
            let sizes: Vec<String> = images.iter().take(4).map(|b| format!("{}x{} {:?}", b.width, b.height, b.format)).collect();
            println!(
                "  map {stage}: {bitmap} ({} images: {}) scale {:?} offset {:?} rotation {} flags {:#x} animation {:?}",
                images.len(),
                sizes.join(", "),
                map.uv_scale,
                map.uv_offset,
                map.rotation,
                map.flags,
                map.animation
            );
        }
        for (stage, color) in t.stage_colors.iter().enumerate() {
            println!("  stage {stage} colour: {color:?}");
        }
    }
}
