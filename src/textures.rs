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
    /// Bitmaps' mean linear colours (each texel's times its alpha, for an
    /// additive shader's light), by tag and image.
    means: HashMap<(u32, u16), Option<[f32; 3]>>,
    /// Bitmap images' linear colours summed (a summed-area table), for a
    /// sprite's share of its sheet, by tag and image.
    sums: HashMap<(u32, u16), Option<SummedArea>>,
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
    /// `curve` (a power's and a gain's bits, or None) reshapes the mask
    /// first: `min(mask^power * gain, 1)`.
    MetallicRoughness { mask: u32, channel: u8, unmasked: u32, roughness: u32, metallic: u32, cloth: Option<(u8, u32)>, curve: Option<[u32; 2]> },
    /// A model base map tinted by an object's change colour where the
    /// multipurpose map's alpha says, as the Xbox model shader does:
    /// `base * lerp(1, color, multipurpose.a)` (8-bit, display-encoded).
    ChangeColor { base: Option<u32>, multipurpose: u32, color: [u8; 3] },
    /// A human weapon's metal finish from its base map and its reflection
    /// mask (the multipurpose map's red), at the base map's resolution: its
    /// colour (`color`, sRGB, the base map's alpha kept) or its
    /// metallic/roughness texture (G roughness, B metallic). See
    /// [`GunmetalFinish`]'s fields, as f32 bits in its order.
    Gunmetal { base: u32, mask: u32, color: bool, finish: [u32; 9] },
}

/// A human weapon's finish (see [`Bake::Gunmetal`]). The base map's light
/// greys are bare metal, its near-blacks and colours paint; where the mask
/// is bright, worn edges and scratches show bare steel through either.
#[derive(Clone, Copy, Debug)]
pub struct GunmetalFinish {
    /// The finish's roughness where the mask is dark.
    pub roughness: f32,
    /// Roughness where the mask is full (worn, polished metal).
    pub worn_roughness: f32,
    /// The black paint's least albedo (linear): the base map's near-black,
    /// painted for Halo's diffuse light only, is darker than any paint and
    /// showed no shape in the sun.
    pub paint_floor: f32,
    /// The base map's luminance (linear) from which a texel turns from paint
    /// to bare metal, and to which it is all metal.
    pub metal_from: f32,
    pub metal_to: f32,
    /// A metal's reflectance: the base map's colour (linear) times this, at
    /// most `metal_cap` (bare steel's).
    pub metal_scale: f32,
    pub metal_cap: f32,
    /// A full mask's bare steel reflectance at the least (worn edges on
    /// black paint).
    pub worn_metal: f32,
    /// The mask's gain: its wear, dim in most of these maps, stronger.
    pub worn_gain: f32,
}

impl GunmetalFinish {
    pub fn bits(&self) -> [u32; 9] {
        [
            self.roughness,
            self.worn_roughness,
            self.paint_floor,
            self.metal_from,
            self.metal_to,
            self.metal_scale,
            self.metal_cap,
            self.worn_metal,
            self.worn_gain,
        ]
        .map(f32::to_bits)
    }

    pub fn from_bits(bits: [u32; 9]) -> Self {
        let [roughness, worn_roughness, paint_floor, metal_from, metal_to, metal_scale, metal_cap, worn_metal, worn_gain] =
            bits.map(f32::from_bits);
        GunmetalFinish { roughness, worn_roughness, paint_floor, metal_from, metal_to, metal_scale, metal_cap, worn_metal, worn_gain }
    }
}

/// Roughness where a surface's specular mask is zero.
pub const MATTE_ROUGHNESS: f32 = 0.9;

/// An image's linear colours summed from its corner: entry (x, y) is the
/// sum of the texels left of x and above y.
pub struct SummedArea {
    width: u32,
    height: u32,
    sums: Vec<[f32; 3]>,
}

impl SummedArea {
    fn new(image: &bitmap::Decoded) -> Self {
        let (width, height) = (image.width, image.height);
        let stride = width as usize + 1;
        let mut sums = vec![[0.0f32; 3]; stride * (height as usize + 1)];
        for y in 0..height as usize {
            let mut row = [0.0f32; 3];
            for x in 0..width as usize {
                let texel = &image.data[(y * width as usize + x) * 4..][..3];
                for c in 0..3 {
                    row[c] += (texel[c] as f32 / 255.0).powf(2.2);
                    sums[(y + 1) * stride + x + 1][c] = sums[y * stride + x + 1][c] + row[c];
                }
            }
        }
        SummedArea { width, height, sums }
    }

    /// The mean linear colour within texture coordinates `min` to `max`
    /// (a span of a whole image or more: the image's).
    pub fn mean(&self, min: [f32; 2], max: [f32; 2]) -> [f32; 3] {
        let span = |lo: f32, hi: f32, size: u32| -> (usize, usize) {
            if hi - lo >= 1.0 {
                return (0, size as usize);
            }
            // (wrapped into the image, its span cut at its edge)
            let width = hi - lo;
            let lo = lo - lo.floor();
            let hi = (lo + width).min(1.0);
            let a = ((lo * size as f32).floor() as usize).min(size as usize - 1);
            let b = ((hi * size as f32).ceil() as usize).clamp(a + 1, size as usize);
            (a, b)
        };
        let (x0, x1) = span(min[0], max[0], self.width);
        let (y0, y1) = span(min[1], max[1], self.height);
        let stride = self.width as usize + 1;
        let at = |x: usize, y: usize| self.sums[y * stride + x];
        let count = ((x1 - x0) * (y1 - y0)) as f32;
        let mut mean = [0.0; 3];
        for (c, m) in mean.iter_mut().enumerate() {
            *m = (at(x1, y1)[c] - at(x0, y1)[c] - at(x1, y0)[c] + at(x0, y0)[c]) / count;
        }
        mean
    }
}

impl TextureCache {
    /// A bitmap image's summed linear colours (see [`SummedArea`]).
    pub fn summed_area(&mut self, cache: &CacheFile, datum: u32, index: u16) -> Option<&SummedArea> {
        self.sums
            .entry((datum, index))
            .or_insert_with(|| rgba8(cache, datum, index).ok().filter(|image| image.width > 0 && image.height > 0).map(|image| SummedArea::new(&image)))
            .as_ref()
    }

    /// A bitmap image's mean linear colour, each texel's weighted by its
    /// alpha where `alpha` (an alpha-multiplied additive blend), at the base
    /// level (the image for a shader permutation, as [`TextureCache::permuted`]).
    pub fn mean_linear(&mut self, cache: &CacheFile, datum: u32, permutation: u16, alpha: bool) -> Option<[f32; 3]> {
        *self.means.entry((datum, permutation | if alpha { 0x8000 } else { 0 })).or_insert_with(|| {
            let image = rgba8(cache, datum, permutation).ok()?;
            let mut sum = [0.0f64; 3];
            let mut count = 0usize;
            for texel in image.data.chunks_exact(4).take((image.width * image.height) as usize) {
                let a = if alpha { texel[3] as f64 / 255.0 } else { 1.0 };
                for c in 0..3 {
                    sum[c] += (texel[c] as f64 / 255.0).powf(2.2) * a;
                }
                count += 1;
            }
            (count > 0).then(|| sum.map(|s| (s / count as f64) as f32))
        })
    }

    /// A bake from its bitmaps' images for a shader permutation (see
    /// [`TextureCache::permuted`]).
    pub fn baked(&mut self, cache: &CacheFile, bake: Bake, permutation: u16, images: &mut Assets<Image>) -> Option<Handle<Image>> {
        if let Some(cached) = self.baked.get(&(bake, permutation)) {
            return cached.clone();
        }
        let result = match bake_rgba(cache, bake, permutation) {
            Ok((width, height, data, srgb)) => {
                let format = if srgb { TextureFormat::Rgba8UnormSrgb } else { TextureFormat::Rgba8Unorm };
                // (with mips, as the maps they replace have: without, they
                // shimmered at a distance)
                let (data, levels) = with_mips(width, height, data);
                let size = Extent3d { width, height, depth_or_array_layers: 1 };
                let mut image = Image::new_uninit(size, TextureDimension::D2, format, RenderAssetUsages::RENDER_WORLD);
                image.texture_descriptor.mip_level_count = levels;
                image.data = Some(data);
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

/// `image` at (u, v) (repeating), each channel by Catmull-Rom's cubic
/// through the four nearest texels each way (0 to 255).
fn bicubic(image: &bitmap::Decoded, u: f32, v: f32) -> [f32; 4] {
    let (w, h) = (image.width as i64, image.height as i64);
    let (x, y) = (u * w as f32 - 0.5, v * h as f32 - 0.5);
    let (x0, y0) = (x.floor(), y.floor());
    let (tx, ty) = (x - x0, y - y0);
    let weights = |t: f32| {
        let (t2, t3) = (t * t, t * t * t);
        [(-t3 + 2.0 * t2 - t) * 0.5, (3.0 * t3 - 5.0 * t2 + 2.0) * 0.5, (-3.0 * t3 + 4.0 * t2 + t) * 0.5, (t3 - t2) * 0.5]
    };
    let (wx, wy) = (weights(tx), weights(ty));
    let mut out = [0.0; 4];
    for (j, wy) in wy.iter().enumerate() {
        let row = (y0 as i64 - 1 + j as i64).rem_euclid(h);
        for (i, wx) in wx.iter().enumerate() {
            let column = (x0 as i64 - 1 + i as i64).rem_euclid(w);
            let o = ((row * w + column) * 4) as usize;
            for c in 0..4 {
                out[c] += image.data[o + c] as f32 * wx * wy;
            }
        }
    }
    out
}

/// RGBA8 texels followed by their mips, each the last's 2x2 average, to
/// 1x1 (an odd size's last row or column dropped): the data and its count
/// of levels.
fn with_mips(width: u32, height: u32, mut data: Vec<u8>) -> (Vec<u8>, u32) {
    let (mut w, mut h) = (width as usize, height as usize);
    let mut start = 0;
    let mut levels = 1;
    while w > 1 || h > 1 {
        let (nw, nh) = ((w / 2).max(1), (h / 2).max(1));
        let mut next = Vec::with_capacity(nw * nh * 4);
        for y in 0..nh {
            for x in 0..nw {
                for c in 0..4 {
                    let mut sum = 0u32;
                    for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                        let (sx, sy) = ((x * 2 + dx).min(w - 1), (y * 2 + dy).min(h - 1));
                        sum += data[start + (sy * w + sx) * 4 + c] as u32;
                    }
                    next.push(((sum + 2) / 4) as u8);
                }
            }
        }
        start = data.len();
        data.extend(next);
        (w, h) = (nw, nh);
        levels += 1;
    }
    (data, levels)
}

/// How far a weapon's base map is blurred to decide paint from bare metal
/// (a Gaussian's sigma, texels): its compression's 4x4 blocks, near the
/// threshold, flipped one by one into squares of metal and paint (the AR's
/// top cover).
const GUNMETAL_DECISION_BLUR: f32 = 2.0;

/// `image`'s red channel blurred by a Gaussian of `sigma` texels (repeating).
fn blur_red(image: &mut bitmap::Decoded, sigma: f32) {
    if sigma <= 0.0 {
        return;
    }
    let radius = (sigma * 3.0).ceil() as i64;
    let kernel: Vec<f32> = (-radius..=radius).map(|i| (-(i * i) as f32 / (2.0 * sigma * sigma)).exp()).collect();
    let total: f32 = kernel.iter().sum();
    let (w, h) = (image.width as i64, image.height as i64);
    let red: Vec<f32> = (0..w * h).map(|i| image.data[(i * 4) as usize] as f32).collect();
    let mut rows = vec![0.0; red.len()];
    for y in 0..h {
        for x in 0..w {
            let sum: f32 = kernel.iter().enumerate().map(|(k, weight)| red[(y * w + (x + k as i64 - radius).rem_euclid(w)) as usize] * weight).sum();
            rows[(y * w + x) as usize] = sum / total;
        }
    }
    for y in 0..h {
        for x in 0..w {
            let sum: f32 = kernel.iter().enumerate().map(|(k, weight)| rows[((y + k as i64 - radius).rem_euclid(h) * w + x) as usize] * weight).sum();
            image.data[((y * w + x) * 4) as usize] = (sum / total).round().clamp(0.0, 255.0) as u8;
        }
    }
}

/// See [`Bake::Gunmetal`].
fn gunmetal(base: &bitmap::Decoded, mask: &bitmap::Decoded, color: bool, finish: GunmetalFinish) -> (u32, u32, Vec<u8>, bool) {
    let to_linear = |c: u8| {
        let c = c as f32 / 255.0;
        if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
    };
    let to_srgb = |c: f32| {
        let c = c.clamp(0.0, 1.0);
        let c = if c <= 0.0031308 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 };
        (c * 255.0).round() as u8
    };
    let smoothstep = |from: f32, to: f32, x: f32| {
        let t = ((x - from) / (to - from).max(1e-6)).clamp(0.0, 1.0);
        t * t * (3.0 - 2.0 * t)
    };
    // upsampled smoothly to about 1024 (up to 8 times): in first person a
    // texel of the AR's 512 spans several pixels, and a dropped AR's map is
    // 128 wide; the wear's steps between paint and metal showed
    let scale = (1024 / base.width.max(base.height).max(1)).clamp(1, 8).next_power_of_two().min(8);
    // the decision's base map: blurred, its colour kept in the red, green
    // and blue as the copies' red channels
    let blurred = [0, 1, 2].map(|c| {
        let mut channel = bitmap::Decoded { data: base.data.chunks_exact(4).flat_map(|t| [t[c], 0, 0, 0]).collect(), ..base.clone() };
        blur_red(&mut channel, GUNMETAL_DECISION_BLUR);
        channel
    });
    let (w, h) = (base.width * scale, base.height * scale);
    let mut out = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            let (u, v) = ((x as f32 + 0.5) / w as f32, (y as f32 + 0.5) / h as f32);
            let worn = (bicubic(mask, u, v)[0] / 255.0 * finish.worn_gain).clamp(0.0, 1.0);
            let texel = bicubic(base, u, v).map(|c| c.round().clamp(0.0, 255.0) as u8);
            let linear = [0, 1, 2].map(|c| to_linear(texel[c]));
            let luminance = 0.2126 * linear[0] + 0.7152 * linear[1] + 0.0722 * linear[2];
            // (decided on the blurred map; light greys only: a colour is
            // paint however light, and a near-black's hue is noise)
            let smooth = [0, 1, 2].map(|c| to_linear(bicubic(&blurred[c], u, v)[0].round().clamp(0.0, 255.0) as u8));
            let smooth_luminance = 0.2126 * smooth[0] + 0.7152 * smooth[1] + 0.0722 * smooth[2];
            let (max, min) = (smooth.iter().copied().fold(0.0, f32::max), smooth.iter().copied().fold(1.0, f32::min));
            let saturation = (max - min) / (max + 0.05);
            let bare = smoothstep(finish.metal_from, finish.metal_to, smooth_luminance) * (1.0 - smoothstep(0.15, 0.35, saturation));
            let metal = bare.max(worn);
            if color {
                // paint: the base map's colour, lifted to the paint's floor;
                // bare metal: its reflectance, from the base map's colour;
                // worn through paint: grey steel
                let lift = (finish.paint_floor - luminance).max(0.0);
                let worn_through = worn * (1.0 - bare);
                let steel_grey = (luminance * finish.metal_scale).min(finish.metal_cap).max(finish.worn_metal);
                for c in 0..3 {
                    let paint = linear[c] + lift;
                    let tinted = (linear[c] * finish.metal_scale).min(finish.metal_cap);
                    let steel = tinted + (steel_grey - tinted) * worn_through;
                    out.push(to_srgb(paint + (steel - paint) * metal));
                }
                out.push(texel[3]);
            } else {
                let roughness = finish.roughness + (finish.worn_roughness - finish.roughness) * worn;
                out.extend([(worn * 255.0).round() as u8, (roughness * 255.0).round() as u8, (metal * 255.0).round() as u8, 255]);
            }
        }
    }
    (w, h, out, color)
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
        Bake::Gunmetal { base, mask, color, finish } => {
            let base = rgba8(cache, base, permutation)?;
            let mask = rgba8(cache, mask, permutation)?;
            Ok(gunmetal(&base, &mask, color, GunmetalFinish::from_bits(finish)))
        }
        Bake::MetallicRoughness { mask, channel, unmasked, roughness, metallic, cloth, curve } => {
            let mut mask = rgba8(cache, mask, permutation)?;
            if let Some([power, gain]) = curve.map(|c| c.map(f32::from_bits)) {
                for texel in mask.data.chunks_exact_mut(4) {
                    let m = texel[channel as usize] as f32 / 255.0;
                    texel[channel as usize] = ((m.powf(power) * gain).min(1.0) * 255.0).round() as u8;
                }
            }
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
