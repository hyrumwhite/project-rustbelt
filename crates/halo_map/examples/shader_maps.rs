//! Prints each model shader's map channel averages: `shader_maps <map> [name part] [raw dump dir]`.
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
        println!("{} ({:?}) alpha tested {} change colour {:?}", info.name, info.shader_type, info.alpha_tested, info.change_color);
        for (label, map) in [("base", info.base_map), ("multipurpose", info.multipurpose_map)] {
            let Some(datum) = map.and_then(|m| m.datum) else { continue };
            let Ok(bitmaps) = halo_map::bitmap::bitmaps(&cache, datum) else { continue };
            let Some(first) = bitmaps.first() else { continue };
            let Ok(d) = first.decode_rgba8(&cache) else { continue };
            let n = (d.width * d.height) as usize;
            let mut sum = [0f64; 4];
            for t in d.data.chunks_exact(4).take(n) {
                for c in 0..4 {
                    sum[c] += t[c] as f64;
                }
            }
            let avg: Vec<String> = sum.iter().map(|s| format!("{:.0}", s / n as f64)).collect();
            println!("  {label} {}x{} rgba avg {}", d.width, d.height, avg.join(" "));
            if let Some(dir) = args.get(3) {
                // raw RGBA beside a name ffmpeg can read: -f rawvideo -pix_fmt rgba -s WxH
                let file = format!("{dir}/{}_{label}_{}x{}.rgba", info.name.replace(['\\', ' '], "_"), d.width, d.height);
                std::fs::write(file, &d.data[..n * 4]).unwrap();
            }
        }
    }
}
