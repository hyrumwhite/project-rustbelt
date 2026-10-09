//! Halo bitmaps → Bevy images.

use bevy::{
    asset::RenderAssetUsages,
    image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor},
    platform::collections::HashMap,
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};
use halo_map::{bitmap, CacheFile};

/// Converted bitmaps, by (bitmap tag datum, bitmap index), so shaders that
/// share a bitmap share its image.
#[derive(Default)]
pub struct TextureCache {
    images: HashMap<(u32, usize), Option<Handle<Image>>>,
    /// Whether the GPU takes BC (DXT) textures as they are.
    pub bc_supported: bool,
    pub failures: usize,
}

impl TextureCache {
    pub fn new(bc_supported: bool) -> Self {
        TextureCache { bc_supported, ..default() }
    }

    pub fn get(
        &mut self,
        cache: &CacheFile,
        datum: u32,
        index: usize,
        srgb: bool,
        images: &mut Assets<Image>,
    ) -> Option<Handle<Image>> {
        let bc_supported = self.bc_supported;
        let entry = self.images.entry((datum, index)).or_insert_with(|| {
            let name = cache.tag(datum).map(|t| t.name.clone()).unwrap_or_default();
            match convert(cache, datum, index, srgb, bc_supported) {
                Ok(image) => Some(images.add(image)),
                Err(e) => {
                    warn!("bitmap {name}[{index}]: {e}");
                    None
                }
            }
        });
        if entry.is_none() {
            self.failures += 1;
        }
        entry.clone()
    }
}

fn convert(cache: &CacheFile, datum: u32, index: usize, srgb: bool, bc_supported: bool) -> halo_map::Result<Image> {
    let bitmaps = bitmap::bitmaps(cache, datum)?;
    let info = bitmaps
        .get(index)
        .ok_or_else(|| halo_map::Error::BadTag(format!("no bitmap {index} of {}", bitmaps.len())))?;
    let decoded = if bc_supported { info.decode(cache, u32::MAX)? } else { info.decode_rgba8(cache)? };

    let format = match (decoded.format, srgb) {
        (bitmap::PixelFormat::Rgba8, true) => TextureFormat::Rgba8UnormSrgb,
        (bitmap::PixelFormat::Rgba8, false) => TextureFormat::Rgba8Unorm,
        (bitmap::PixelFormat::Bc1, true) => TextureFormat::Bc1RgbaUnormSrgb,
        (bitmap::PixelFormat::Bc1, false) => TextureFormat::Bc1RgbaUnorm,
        (bitmap::PixelFormat::Bc2, true) => TextureFormat::Bc2RgbaUnormSrgb,
        (bitmap::PixelFormat::Bc2, false) => TextureFormat::Bc2RgbaUnorm,
        (bitmap::PixelFormat::Bc3, true) => TextureFormat::Bc3RgbaUnormSrgb,
        (bitmap::PixelFormat::Bc3, false) => TextureFormat::Bc3RgbaUnorm,
    };
    let size = Extent3d { width: decoded.width, height: decoded.height, depth_or_array_layers: 1 };
    let mut image = Image::new_uninit(size, TextureDimension::D2, format, RenderAssetUsages::RENDER_WORLD);
    image.texture_descriptor.mip_level_count = decoded.levels;
    image.data = Some(decoded.data);
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Linear,
        anisotropy_clamp: 16,
        ..default()
    });
    Ok(image)
}
