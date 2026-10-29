//! Prints each mip level's alpha of a DXT bitmap tag's first image (how many
//! texels are below half), as the GPU gets its blocks: `bitmap_alpha <map>
//! <tag name>`.
fn main() {
    use halo_map::bitmap::{Format, PixelFormat};
    let args: Vec<String> = std::env::args().collect();
    let cache = halo_map::CacheFile::open(std::path::Path::new(&args[1])).unwrap();
    let tag = cache.tags.iter().find(|t| t.name == args[2] && t.group == halo_map::groups::BITMAP).expect("no such bitmap");
    let bitmaps = halo_map::bitmap::bitmaps(&cache, tag.datum).unwrap();
    let decoded = bitmaps[0].decode(&cache, 16).unwrap();
    let (format, block) = match decoded.format {
        PixelFormat::Bc1 => (Format::Dxt1, 8),
        PixelFormat::Bc2 => (Format::Dxt3, 16),
        PixelFormat::Bc3 => (Format::Dxt5, 16),
        _ => panic!("not passed through as blocks: {:?}", decoded.format),
    };
    println!("{format:?} {}x{}, {} levels", decoded.width, decoded.height, decoded.levels);
    let mut offset = 0;
    for level in 0..decoded.levels {
        let (w, h) = ((decoded.width >> level).max(1), (decoded.height >> level).max(1));
        let bytes = (w.div_ceil(4) * h.div_ceil(4)) as usize * block;
        let data = halo_map::bitmap::decode_dxt(format, &decoded.data[offset..offset + bytes], w, h);
        offset += bytes;
        let below = data.chunks_exact(4).filter(|t| t[3] < 128).count();
        let black = data.chunks_exact(4).filter(|t| t[0] < 4 && t[1] < 4 && t[2] < 4).count();
        println!("  level {level}: {w}x{h}, {below} of {} texels below half alpha, {black} black", data.len() / 4);
    }
}
