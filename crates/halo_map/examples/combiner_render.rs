//! Renders a transparent generic or chicago shader's combiner program over
//! the unit square of its texture coordinates (each map at its own scale, not
//! animated; each stage's colour at its animation's function value, one for
//! `_periodic_function_one`, a half otherwise) as raw RGBA, and prints the
//! program: `combiner_render <map> <shader name part> <out.rgba> [size]`.
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let cache = halo_map::CacheFile::open(std::path::Path::new(&args[1])).unwrap();
    let size: usize = args.get(4).and_then(|s| s.parse().ok()).unwrap_or(512);
    for tag in &cache.tags {
        if !tag.name.contains(&args[2]) {
            continue;
        }
        let reference = halo_map::TagRef { group: 0, datum: Some(tag.datum) };
        let Ok(Some(info)) = halo_map::shader::shader_info(&cache, reference) else { continue };
        let Some(t) = info.transparent.as_ref() else { continue };
        let Ok(Some(mut program)) = halo_map::combiner::program(&cache, reference) else { continue };
        for (stage, color) in t.stage_colors.iter().enumerate().take(halo_map::combiner::MAXIMUM_STAGES) {
            program.constant0[stage] = color.evaluate(None, None, 0.0, |function, _| if function == 0 { 1.0 } else { 0.5 });
        }
        println!("{}: {} stages, flags {:#x}, final {:#010x} {:#010x}", info.name, program.stage_count, program.flags, program.final_abcd, program.final_efg);
        for stage in 0..program.stage_count {
            println!(
                "  stage {stage}: rgb {:#010x} -> {:#07x}, alpha {:#010x} -> {:#07x}, c0 {:?}, c1 {:?}",
                program.rgb_inputs[stage], program.rgb_outputs[stage], program.alpha_inputs[stage], program.alpha_outputs[stage], program.constant0[stage], program.constant1[stage]
            );
        }
        let maps: Vec<Option<(halo_map::bitmap::Decoded, [f32; 2])>> = t
            .maps
            .iter()
            .map(|map| {
                let map = map.as_ref()?;
                let bitmaps = halo_map::bitmap::bitmaps(&cache, map.bitmap.datum?).ok()?;
                Some((bitmaps.first()?.decode_rgba8(&cache).ok()?, map.uv_scale))
            })
            .collect();
        let mut out = vec![0u8; size * size * 4];
        for y in 0..size {
            for x in 0..size {
                let uv = [(x as f32 + 0.5) / size as f32, (y as f32 + 0.5) / size as f32];
                let mut textures = [[0.0f32; 4]; 4];
                for (stage, map) in maps.iter().enumerate().take(4) {
                    let Some((image, scale)) = map else { continue };
                    let (w, h) = (image.width as usize, image.height as usize);
                    let u = ((uv[0] * scale[0]).rem_euclid(1.0) * w as f32) as usize % w;
                    let v = ((uv[1] * scale[1]).rem_euclid(1.0) * h as f32) as usize % h;
                    let at = (v * w + u) * 4;
                    textures[stage] = std::array::from_fn(|c| image.data[at + c] as f32 / 255.0);
                }
                let rgba = halo_map::combiner::evaluate(&program, textures);
                for c in 0..4 {
                    out[(y * size + x) * 4 + c] = (rgba[c].clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
                }
            }
        }
        std::fs::write(&args[3], &out).unwrap();
        println!("  {size}x{size} written");
        break;
    }
}
