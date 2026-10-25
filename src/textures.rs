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
    images: HashMap<(u32, usize, bool), Option<Handle<Image>>>,
    baked: HashMap<(Bake, u16), Option<Handle<Image>>>,
    /// Each bitmap tag's image count, for [`TextureCache::permuted`].
    counts: HashMap<u32, usize>,
    /// 3D bitmaps (plasma's noise), by tag.
    volumes: HashMap<u32, Option<Handle<Image>>>,
    /// Whether the GPU takes BC (DXT) textures as they are.
    pub bc_supported: bool,
    pub failures: usize,
}

/// An emissive texture made on the CPU from Halo maps.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub enum Bake {
    /// Base map times the multipurpose map's green (self-illumination) channel.
    ModelSelfIllumination { base: Option<u32>, multipurpose: u32 },
    /// Base map times (R*primary + G*secondary + B*plasma) of an environment
    /// shader's self-illumination map: Halo adds self-illumination to the
    /// lightmap, so it lights the base map (colours as f32 bits).
    EnvironmentSelfIllumination { base: Option<u32>, map: u32, colors: [[u32; 3]; 3] },
    /// A StandardMaterial metallic/roughness texture (G roughness, B metallic)
    /// from a specular mask: `channel` (0..=3) of `mask`'s first bitmap mixes
    /// from a dielectric of roughness `unmasked` (mask 0) to
    /// `roughness`/`metallic` (mask 1), as f32 bits. With `cloth` (a
    /// channel of the same map and a roughness's bits), that channel keeps
    /// the result and its zero is a matte dielectric of that roughness
    /// (Chief's plates, the change colour's mask, over his cloth suit).
    MetallicRoughness { mask: u32, channel: u8, unmasked: u32, roughness: u32, metallic: u32, cloth: Option<(u8, u32)> },
    /// A model base map tinted by an object's change colour where the
    /// multipurpose map's alpha says, as the Xbox model shader does:
    /// `base * lerp(1, color, multipurpose.a)` (8-bit, display-encoded).
    ChangeColor { base: Option<u32>, multipurpose: u32, color: [u8; 3] },
}

/// Roughness where a surface's specular mask is zero.
pub const MATTE_ROUGHNESS: f32 = 0.9;

impl TextureCache {
    /// A bake from its bitmaps' images for a shader permutation (see
    /// [`TextureCache::permuted`]).
    pub fn baked(&mut self, cache: &CacheFile, bake: Bake, permutation: u16, images: &mut Assets<Image>) -> Option<Handle<Image>> {
        if let Some(cached) = self.baked.get(&(bake, permutation)) {
            return cached.clone();
        }
        let result = match bake_rgba(cache, bake, permutation) {
            Ok((width, height, data, srgb)) => {
                let format = if srgb { TextureFormat::Rgba8UnormSrgb } else { TextureFormat::Rgba8Unorm };
                let mut image = Image::new(
                    Extent3d { width, height, depth_or_array_layers: 1 },
                    TextureDimension::D2,
                    data,
                    format,
                    RenderAssetUsages::RENDER_WORLD,
                );
                image.sampler = repeat_sampler();
                Some(images.add(image))
            }
            Err(e) => {
                warn!("emissive bake: {e}");
                self.failures += 1;
                None
            }
        };
        self.baked.insert((bake, permutation), result.clone());
        result
    }

    /// A bitmap's image for a shader permutation: image `permutation % count`,
    /// as the game's `rasterizer_set_texture` picks it.
    pub fn permuted(
        &mut self,
        cache: &CacheFile,
        datum: u32,
        permutation: u16,
        srgb: bool,
        images: &mut Assets<Image>,
    ) -> Option<Handle<Image>> {
        let count = *self.counts.entry(datum).or_insert_with(|| bitmap::bitmaps(cache, datum).map_or(1, |b| b.len().max(1)));
        self.get(cache, datum, permutation as usize % count, srgb, images)
    }

    /// A 3D bitmap's first level (plasma's noise), as data, repeating.
    pub fn volume(&mut self, cache: &CacheFile, datum: u32, images: &mut Assets<Image>) -> Option<Handle<Image>> {
        if let Some(volume) = self.volumes.get(&datum) {
            return volume.clone();
        }
        let volume = bitmap::bitmaps(cache, datum)
            .ok()
            .and_then(|b| b.into_iter().next())
            .and_then(|info| info.decode_3d_rgba8(cache).ok())
            .map(|decoded| {
                let mut image = Image::new(
                    Extent3d { width: decoded.width, height: decoded.height, depth_or_array_layers: decoded.depth },
                    TextureDimension::D3,
                    decoded.data,
                    TextureFormat::Rgba8Unorm,
                    RenderAssetUsages::RENDER_WORLD,
                );
                image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
                    address_mode_u: ImageAddressMode::Repeat,
                    address_mode_v: ImageAddressMode::Repeat,
                    address_mode_w: ImageAddressMode::Repeat,
                    mag_filter: ImageFilterMode::Linear,
                    min_filter: ImageFilterMode::Linear,
                    ..default()
                });
                images.add(image)
            });
        if volume.is_none() {
            let name = cache.tag(datum).map(|t| t.name.clone()).unwrap_or_default();
            warn!("bitmap {name}: no 3D texture");
        }
        self.volumes.insert(datum, volume.clone());
        volume
    }

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
        let entry = self.images.entry((datum, index, srgb)).or_insert_with(|| {
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
    image.sampler = repeat_sampler();
    Ok(image)
}

fn repeat_sampler() -> ImageSampler {
    ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Linear,
        anisotropy_clamp: 16,
        ..default()
    })
}

/// The first bitmap of a group as RGBA8.
fn rgba8(cache: &CacheFile, datum: u32, permutation: u16) -> halo_map::Result<bitmap::Decoded> {
    let bitmaps = bitmap::bitmaps(cache, datum)?;
    if bitmaps.is_empty() {
        return Err(halo_map::Error::BadTag("bitmap group has no bitmaps".into()));
    }
    let first = &bitmaps[permutation as usize % bitmaps.len()];
    first.decode_rgba8(cache)
}

fn metallic_roughness(
    mask: &bitmap::Decoded,
    channel: usize,
    unmasked: f32,
    roughness: f32,
    metallic: f32,
    cloth: Option<(usize, f32)>,
) -> (u32, u32, Vec<u8>, bool) {
    let mut out = Vec::with_capacity(mask.data.len());
    for texel in mask.data.chunks_exact(4) {
        let mut m = texel[channel] as f32 / 255.0;
        let mut r = unmasked + (roughness - unmasked) * m;
        if let Some((keep, cloth)) = cloth {
            let k = texel[keep] as f32 / 255.0;
            r = cloth + (r - cloth) * k;
            m *= k;
        }
        // red: the mask itself (a clear coat's, where a shader has one)
        out.extend([(m * 255.0).round() as u8, (r * 255.0).round() as u8, (metallic * m * 255.0).round() as u8, 255]);
    }
    (mask.width, mask.height, out, false)
}

/// At the mask's resolution: the base map's nearest texel (sRGB) times the
/// light `light(mask texel)` gives it, or the light alone without a base map.
fn modulate_base(
    mask: &bitmap::Decoded,
    base: Option<&bitmap::Decoded>,
    light: impl Fn(&[u8]) -> [f32; 3],
) -> (u32, u32, Vec<u8>, bool) {
    let (w, h) = (mask.width, mask.height);
    let mut out = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            let o = ((y * w + x) * 4) as usize;
            let l = light(&mask.data[o..o + 4]);
            let rgb = match base {
                Some(b) => {
                    let (bx, by) = (x * b.width / w, y * b.height / h);
                    let bo = ((by * b.width + bx) * 4) as usize;
                    [b.data[bo], b.data[bo + 1], b.data[bo + 2]]
                }
                None => [255, 255, 255],
            };
            for c in 0..3 {
                out.push((rgb[c] as f32 * l[c].clamp(0.0, 1.0)).round() as u8);
            }
            out.push(255);
        }
    }
    (w, h, out, true)
}

/// The base map (at its own size; white without one) times `lerp(1, color,
/// mask.b)`, its alpha kept: the multipurpose map's blue channel is its
/// colour change mask in the Xbox order (its alpha, in the PC's, is opaque
/// in the Xbox maps' DXT1, which tinted whole surfaces).
fn change_color(mask: &bitmap::Decoded, base: Option<&bitmap::Decoded>, color: [u8; 3]) -> (u32, u32, Vec<u8>, bool) {
    let (w, h) = base.map_or((mask.width, mask.height), |b| (b.width, b.height));
    let mut out = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            let (mx, my) = (x * mask.width / w, y * mask.height / h);
            let amount = mask.data[((my * mask.width + mx) * 4 + 2) as usize] as f32 / 255.0;
            let texel = match base {
                Some(b) => {
                    let o = ((y * w + x) * 4) as usize;
                    [b.data[o], b.data[o + 1], b.data[o + 2], b.data[o + 3]]
                }
                None => [255; 4],
            };
            for c in 0..3 {
                let tint = 1.0 + (color[c] as f32 / 255.0 - 1.0) * amount;
                out.push((texel[c] as f32 * tint).round() as u8);
            }
            out.push(texel[3]);
        }
    }
    (w, h, out, true)
}

/// (width, height, RGBA8 texels, sRGB-encoded)
fn bake_rgba(cache: &CacheFile, bake: Bake, permutation: u16) -> halo_map::Result<(u32, u32, Vec<u8>, bool)> {
    match bake {
        Bake::ModelSelfIllumination { base, multipurpose } => {
            let mask = rgba8(cache, multipurpose, permutation)?;
            let base = base.map(|b| rgba8(cache, b, permutation)).transpose()?;
            Ok(modulate_base(&mask, base.as_ref(), |texel| [texel[1] as f32 / 255.0; 3]))
        }
        Bake::ChangeColor { base, multipurpose, color } => {
            let mask = rgba8(cache, multipurpose, permutation)?;
            let base = base.map(|b| rgba8(cache, b, permutation)).transpose()?;
            Ok(change_color(&mask, base.as_ref(), color))
        }
        Bake::MetallicRoughness { mask, channel, unmasked, roughness, metallic, cloth } => {
            let mask = rgba8(cache, mask, permutation)?;
            let cloth = cloth.map(|(channel, roughness)| (channel as usize, f32::from_bits(roughness)));
            Ok(metallic_roughness(&mask, channel as usize, f32::from_bits(unmasked), f32::from_bits(roughness), f32::from_bits(metallic), cloth))
        }
        Bake::EnvironmentSelfIllumination { base, map, colors } => {
            let map = rgba8(cache, map, permutation)?;
            let base = base.map(|b| rgba8(cache, b, permutation)).transpose()?;
            let colors = colors.map(|c| c.map(f32::from_bits));
            Ok(modulate_base(&map, base.as_ref(), |texel| {
                let weights = [texel[0], texel[1], texel[2]].map(|c| c as f32 / 255.0);
                std::array::from_fn(|channel| (0..3).map(|i| weights[i] * colors[i][channel]).sum::<f32>())
            }))
        }
    }
}
