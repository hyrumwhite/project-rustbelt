//! Bitmaps (`bitm` tags): the Xbox texture formats.
//!
//! A bitmap group tag has a block of `bitmap_data` headers. Each one's pixels
//! are in the map file at `pixels_offset` (made absolute when the map was
//! built; see [`FLAG_CACHED`]), mip level after mip level from the largest.
//! DXT levels are padded to whole 4x4 blocks. Other power-of-two formats are
//! swizzled (Morton order per level) when the swizzled flag is set
//! (`xbox_textures.c` in the port).

use crate::{layout, CacheFile, Error, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    A8,
    Y8,
    Ay8,
    A8y8,
    R5g6b5,
    A1r5g5b5,
    A4r4g4b4,
    X8r8g8b8,
    A8r8g8b8,
    Dxt1,
    Dxt3,
    Dxt5,
    P8Bump,
}

impl Format {
    /// `_bitmap_format_*` (bitmaps.c).
    fn from_raw(raw: i16) -> Option<Self> {
        Some(match raw {
            0 => Self::A8,
            1 => Self::Y8,
            2 => Self::Ay8,
            3 => Self::A8y8,
            6 => Self::R5g6b5,
            8 => Self::A1r5g5b5,
            9 => Self::A4r4g4b4,
            10 => Self::X8r8g8b8,
            11 => Self::A8r8g8b8,
            14 => Self::Dxt1,
            15 => Self::Dxt3,
            16 => Self::Dxt5,
            17 => Self::P8Bump,
            _ => return None,
        })
    }

    pub fn is_compressed(self) -> bool {
        matches!(self, Self::Dxt1 | Self::Dxt3 | Self::Dxt5)
    }

    /// Bytes per texel, or per 4x4 block for DXT.
    fn unit_bytes(self) -> usize {
        match self {
            Self::A8 | Self::Y8 | Self::Ay8 | Self::P8Bump => 1,
            Self::A8y8 | Self::R5g6b5 | Self::A1r5g5b5 | Self::A4r4g4b4 => 2,
            Self::X8r8g8b8 | Self::A8r8g8b8 => 4,
            Self::Dxt1 => 8,
            Self::Dxt3 | Self::Dxt5 => 16,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Texture2d,
    Texture3d,
    CubeMap,
}

/// `_bitmap_*_bit` flags.
const FLAG_SWIZZLED: u16 = 1 << 3;
/// Set by `texture_cache_bitmap_new` (xbox_texture_cache.c) as the map is
/// built, when it makes `pixels_offset` absolute (adds the group's
/// `pixel_data.file_offset`).
const FLAG_CACHED: u16 = 1 << 7;

/// One `bitmap_data`.
#[derive(Debug, Clone)]
pub struct BitmapInfo {
    pub width: u32,
    pub height: u32,
    pub depth: u32,
    pub kind: Kind,
    pub format: Format,
    pub flags: u16,
    /// Mip levels including the base level.
    pub levels: u32,
    /// File offset and size of all the pixels.
    pub pixels_offset: usize,
    pub pixels_size: usize,
}

/// The bitmaps of a bitmap group tag.
pub fn bitmaps(cache: &CacheFile, datum: u32) -> Result<Vec<BitmapInfo>> {
    use layout::bitmap_data as b;
    let group = cache.tag_root(datum, crate::groups::BITMAP)?;
    let pixel_data = group.data(layout::bitmap_group::PIXEL_DATA)?;
    let block = group.block(layout::bitmap_group::BITMAPS, b::SIZEOF)?;
    block
        .iter()
        .map(|bitmap| {
            let raw_format = bitmap.i16(b::FORMAT)?;
            let format = Format::from_raw(raw_format)
                .ok_or_else(|| Error::Unsupported(format!("bitmap format {raw_format}")))?;
            let kind = match bitmap.i16(b::TYPE)? {
                0 => Kind::Texture2d,
                1 => Kind::Texture3d,
                2 => Kind::CubeMap,
                other => return Err(Error::Unsupported(format!("bitmap type {other}"))),
            };
            let flags = bitmap.u16(b::FLAGS)?;
            let pixels_offset = bitmap.i32(b::PIXELS_OFFSET)?.max(0) as usize;
            let pixels_offset =
                if flags & FLAG_CACHED != 0 { pixels_offset } else { pixel_data.file_offset + pixels_offset };
            Ok(BitmapInfo {
                width: bitmap.i16(b::WIDTH)?.max(1) as u32,
                height: bitmap.i16(b::HEIGHT)?.max(1) as u32,
                depth: bitmap.i16(b::DEPTH)?.max(1) as u32,
                kind,
                format,
                flags,
                levels: bitmap.i16(b::MIPMAP_COUNT)?.max(0) as u32 + 1,
                pixels_offset,
                pixels_size: bitmap.i32(b::PIXELS_SIZE)?.max(0) as usize,
            })
        })
        .collect()
}

/// GPU-ready pixel data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PixelFormat {
    /// 8-bit RGBA, sRGB-encoded colour.
    Rgba8,
    /// BC1/BC2/BC3 blocks (DXT1/3/5), as the file stores them.
    Bc1,
    Bc2,
    Bc3,
}

#[derive(Debug, Clone)]
pub struct Decoded {
    pub width: u32,
    pub height: u32,
    pub levels: u32,
    pub format: PixelFormat,
    /// Every level, largest first, tightly packed.
    pub data: Vec<u8>,
}

impl BitmapInfo {
    fn level_size(&self, level: u32) -> (u32, u32) {
        ((self.width >> level).max(1), (self.height >> level).max(1))
    }

    fn level_bytes(&self, level: u32) -> usize {
        let (w, h) = self.level_size(level);
        if self.format.is_compressed() {
            (w.div_ceil(4) * h.div_ceil(4)) as usize * self.format.unit_bytes()
        } else {
            (w * h) as usize * self.format.unit_bytes()
        }
    }

    /// Decodes a 2D bitmap.
    ///
    /// DXT bitmaps whose base level is a whole number of blocks are passed
    /// through as BC blocks (all levels down to 4x4, which wgpu needs); the
    /// rest are converted to RGBA8. `max_levels` limits the mip chain.
    pub fn decode(&self, cache: &CacheFile, max_levels: u32) -> Result<Decoded> {
        if self.kind != Kind::Texture2d {
            return Err(Error::Unsupported(format!("{:?} bitmaps", self.kind)));
        }
        let mut levels = self.levels.min(max_levels.max(1));
        let total: usize = (0..levels).map(|l| self.level_bytes(l)).sum();
        let pixels = cache.file_bytes(self.pixels_offset, total)?;

        let passthrough = self.format.is_compressed() && self.width.is_multiple_of(4) && self.height.is_multiple_of(4);
        if passthrough {
            // wgpu requires every level of a BC texture to be whole blocks
            while levels > 1 {
                let (w, h) = self.level_size(levels - 1);
                if w.is_multiple_of(4) && h.is_multiple_of(4) {
                    break;
                }
                levels -= 1;
            }
            let len: usize = (0..levels).map(|l| self.level_bytes(l)).sum();
            let format = match self.format {
                Format::Dxt1 => PixelFormat::Bc1,
                Format::Dxt3 => PixelFormat::Bc2,
                _ => PixelFormat::Bc3,
            };
            return Ok(Decoded { width: self.width, height: self.height, levels, format, data: pixels[..len].to_vec() });
        }

        let mut data = Vec::new();
        let mut offset = 0;
        for level in 0..levels {
            let (w, h) = self.level_size(level);
            let bytes = &pixels[offset..offset + self.level_bytes(level)];
            offset += self.level_bytes(level);
            if self.format.is_compressed() {
                data.extend(decode_dxt(self.format, bytes, w, h));
            } else {
                let swizzled = self.flags & FLAG_SWIZZLED != 0;
                data.extend(decode_uncompressed(self.format, bytes, w, h, swizzled));
            }
        }
        Ok(Decoded { width: self.width, height: self.height, levels, format: PixelFormat::Rgba8, data })
    }

    /// A cube map's six faces' base levels as RGBA8, in Direct3D's order
    /// (+X, -X, +Y, -Y, +Z, -Z). The Xbox keeps each face's whole mip chain
    /// together, faces 128-byte aligned.
    pub fn decode_cube_faces_rgba8(&self, cache: &CacheFile) -> Result<Vec<Decoded>> {
        if self.kind != Kind::CubeMap {
            return Err(Error::Unsupported(format!("{:?} bitmaps as cube faces", self.kind)));
        }
        let chain: usize = (0..self.levels.max(1)).map(|l| self.level_bytes(l)).sum();
        let stride = chain.next_multiple_of(128);
        let pixels = cache.file_bytes(self.pixels_offset, stride * 5 + chain)?;
        let (w, h) = (self.width, self.height);
        Ok((0..6)
            .map(|face| {
                let bytes = &pixels[face * stride..face * stride + self.level_bytes(0)];
                let data = if self.format.is_compressed() {
                    decode_dxt(self.format, bytes, w, h)
                } else {
                    decode_uncompressed(self.format, bytes, w, h, self.flags & FLAG_SWIZZLED != 0)
                };
                Decoded { width: w, height: h, levels: 1, format: PixelFormat::Rgba8, data }
            })
            .collect())
    }

    /// The base level as RGBA8 regardless of format (for previews).
    pub fn decode_rgba8(&self, cache: &CacheFile) -> Result<Decoded> {
        let decoded = self.decode(cache, 1)?;
        if decoded.format == PixelFormat::Rgba8 {
            return Ok(decoded);
        }
        let data = decode_dxt(self.format, &decoded.data, self.width, self.height);
        Ok(Decoded { format: PixelFormat::Rgba8, data, ..decoded })
    }
}

/// Bits of x, y alternate from the lowest until one dimension runs out
/// (`swizzle_masks` in xbox_textures.c).
fn swizzle_masks(width: u32, height: u32) -> (u32, u32) {
    let (mut mask_x, mut mask_y) = (0, 0);
    let (mut bit, mut mask_bit) = (1u32, 1u32);
    loop {
        let mut done = true;
        if bit < width {
            mask_x |= mask_bit;
            mask_bit <<= 1;
            done = false;
        }
        if bit < height {
            mask_y |= mask_bit;
            mask_bit <<= 1;
            done = false;
        }
        if done {
            return (mask_x, mask_y);
        }
        bit <<= 1;
    }
}

/// Spreads the bits of `value` over the set bits of `mask`.
fn spread(mask: u32, mut value: u32) -> u32 {
    let mut result = 0;
    let mut bit = 1u32;
    while bit != 0 && bit <= mask {
        if mask & bit != 0 {
            if value & 1 != 0 {
                result |= bit;
            }
            value >>= 1;
        }
        bit = bit.wrapping_shl(1);
    }
    result
}

fn decode_uncompressed(format: Format, bytes: &[u8], width: u32, height: u32, swizzled: bool) -> Vec<u8> {
    let unit = format.unit_bytes();
    let (mask_x, mask_y) = swizzle_masks(width, height);
    let mut out = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        for x in 0..width {
            let index = if swizzled { (spread(mask_x, x) | spread(mask_y, y)) as usize } else { (y * width + x) as usize };
            let t = &bytes[index * unit..index * unit + unit];
            let u16v = || u16::from_le_bytes([t[0], t[1]]);
            let e4 = |v: u16| (v as u8 & 0xF) * 17;
            let e5 = |v: u16| ((v as u32 & 0x1F) * 255 / 31) as u8;
            let e6 = |v: u16| ((v as u32 & 0x3F) * 255 / 63) as u8;
            let rgba = match format {
                Format::A8 => [255, 255, 255, t[0]],
                Format::Y8 => [t[0], t[0], t[0], 255],
                Format::Ay8 => [t[0], t[0], t[0], t[0]],
                Format::A8y8 => [t[0], t[0], t[0], t[1]],
                Format::R5g6b5 => {
                    let v = u16v();
                    [e5(v >> 11), e6(v >> 5), e5(v), 255]
                }
                Format::A1r5g5b5 => {
                    let v = u16v();
                    [e5(v >> 10), e5(v >> 5), e5(v), if v & 0x8000 != 0 { 255 } else { 0 }]
                }
                Format::A4r4g4b4 => {
                    let v = u16v();
                    [e4(v >> 8), e4(v >> 4), e4(v), e4(v >> 12)]
                }
                Format::X8r8g8b8 => [t[2], t[1], t[0], 255],
                Format::A8r8g8b8 => [t[2], t[1], t[0], t[3]],
                // a tangent-space normal from the renderer's vector palette
                Format::P8Bump => {
                    let c = crate::palette::VECTOR_PALETTE[t[0] as usize];
                    [(c >> 16) as u8, (c >> 8) as u8, c as u8, (c >> 24) as u8]
                }
                Format::Dxt1 | Format::Dxt3 | Format::Dxt5 => unreachable!(),
            };
            out.extend_from_slice(&rgba);
        }
    }
    out
}

fn rgb565(v: u16) -> [u8; 3] {
    let r = (v >> 11) as u32 & 0x1F;
    let g = (v >> 5) as u32 & 0x3F;
    let b = v as u32 & 0x1F;
    [(r * 255 / 31) as u8, (g * 255 / 63) as u8, (b * 255 / 31) as u8]
}

/// Decodes DXT1/3/5 blocks to RGBA8 (width x height, cropping padding).
pub fn decode_dxt(format: Format, bytes: &[u8], width: u32, height: u32) -> Vec<u8> {
    let block_bytes = format.unit_bytes();
    let blocks_x = width.div_ceil(4) as usize;
    let mut out = vec![0u8; (width * height * 4) as usize];
    for (block_index, block) in bytes.chunks_exact(block_bytes).enumerate() {
        let bx = (block_index % blocks_x) as u32 * 4;
        let by = (block_index / blocks_x) as u32 * 4;
        if by >= height {
            break;
        }
        let (alpha_part, color_part) = if format == Format::Dxt1 { (&[][..], block) } else { block.split_at(8) };

        let c0 = u16::from_le_bytes([color_part[0], color_part[1]]);
        let c1 = u16::from_le_bytes([color_part[2], color_part[3]]);
        let (p0, p1) = (rgb565(c0), rgb565(c1));
        let mix = |a: u8, b: u8, wa: u32, wb: u32| ((a as u32 * wa + b as u32 * wb) / (wa + wb)) as u8;
        let mut palette = [[0u8; 4]; 4];
        palette[0] = [p0[0], p0[1], p0[2], 255];
        palette[1] = [p1[0], p1[1], p1[2], 255];
        if c0 > c1 || format != Format::Dxt1 {
            palette[2] = [mix(p0[0], p1[0], 2, 1), mix(p0[1], p1[1], 2, 1), mix(p0[2], p1[2], 2, 1), 255];
            palette[3] = [mix(p0[0], p1[0], 1, 2), mix(p0[1], p1[1], 1, 2), mix(p0[2], p1[2], 1, 2), 255];
        } else {
            palette[2] = [mix(p0[0], p1[0], 1, 1), mix(p0[1], p1[1], 1, 1), mix(p0[2], p1[2], 1, 1), 255];
            palette[3] = [0, 0, 0, 0];
        }
        let color_bits = u32::from_le_bytes(color_part[4..8].try_into().unwrap());

        let alpha: [u8; 16] = match format {
            Format::Dxt3 => {
                let bits = u64::from_le_bytes(alpha_part.try_into().unwrap());
                std::array::from_fn(|i| ((bits >> (i * 4)) & 0xF) as u8 * 17)
            }
            Format::Dxt5 => {
                let (a0, a1) = (alpha_part[0] as u32, alpha_part[1] as u32);
                let mut table = [0u32; 8];
                table[0] = a0;
                table[1] = a1;
                if a0 > a1 {
                    for i in 1..7 {
                        table[i + 1] = (a0 * (7 - i as u32) + a1 * i as u32) / 7;
                    }
                } else {
                    for i in 1..5 {
                        table[i + 1] = (a0 * (5 - i as u32) + a1 * i as u32) / 5;
                    }
                    table[6] = 0;
                    table[7] = 255;
                }
                let mut bits = 0u64;
                for (i, &b) in alpha_part[2..8].iter().enumerate() {
                    bits |= (b as u64) << (8 * i);
                }
                std::array::from_fn(|i| table[((bits >> (i * 3)) & 7) as usize] as u8)
            }
            _ => [255; 16],
        };

        for i in 0..16u32 {
            let (x, y) = (bx + i % 4, by + i / 4);
            if x >= width || y >= height {
                continue;
            }
            let mut texel = palette[((color_bits >> (i * 2)) & 3) as usize];
            if format != Format::Dxt1 {
                texel[3] = alpha[i as usize];
            }
            let o = ((y * width + x) * 4) as usize;
            out[o..o + 4].copy_from_slice(&texel);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn swizzle_square_is_morton() {
        let (mx, my) = swizzle_masks(4, 4);
        assert_eq!((mx, my), (0b0101, 0b1010));
        assert_eq!(spread(mx, 3) | spread(my, 1), 0b0111);
    }

    #[test]
    fn swizzle_wide_runs_out_of_y() {
        // 8x2: x0 y0 x1 x2
        let (mx, my) = swizzle_masks(8, 2);
        assert_eq!((mx, my), (0b1101, 0b0010));
    }
}
