//! Vertex decompression and coordinate conventions.

/// Metres per Halo world unit (1 world unit = 10 feet).
pub const METERS_PER_WORLD_UNIT: f32 = 3.048;

/// `uncompress_int32_to_real_vector3d` (rasterizer_geometry.c): a unit
/// vector packed as signed 11:11:10 fixed point, x in the low bits.
pub fn unpack_vector_11_11_10(packed: u32) -> [f32; 3] {
    let x = ((packed << 21) as i32) as f32;
    let y = (((packed >> 11) << 21) as i32) as f32;
    let z = (((packed >> 22) << 22) as i32) as f32;
    [
        (x * (1.0 / 1_048_576.0) + 1.0) * (1.0 / 2047.0),
        (y * (1.0 / 1_048_576.0) + 1.0) * (1.0 / 2047.0),
        (z * (1.0 / 2_097_152.0) + 1.0) * (1.0 / 1023.0),
    ]
}

/// A 16-bit texture coordinate as compressed model and lightmap vertices
/// store it: `(value * 2 + 1) / 65535`.
pub fn unpack_texcoord_i16(value: i16) -> f32 {
    (value as f32 * 2.0 + 1.0) * (1.0 / 65535.0)
}

/// A Halo point or direction (Z up, world units) in Bevy's convention
/// (Y up, right-handed, metres when `scale` is [`METERS_PER_WORLD_UNIT`]).
/// The mapping (x, y, z) -> (x, z, -y) is a rotation, so triangle winding is
/// kept.
pub fn halo_to_bevy([x, y, z]: [f32; 3], scale: f32) -> [f32; 3] {
    [x * scale, z * scale, -y * scale]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Packs as the game's compressor does (`compress_real_vector3d_to_int32`
    /// in custom_edition_geometry.c, which inverts the unpacking above).
    fn pack(v: [f32; 3]) -> u32 {
        let q = |c: f32, max: f32, bits: u32| ((c * max).round() as i32 as u32) & ((1 << bits) - 1);
        q(v[0], 1023.0, 11) | (q(v[1], 1023.0, 11) << 11) | (q(v[2], 511.0, 10) << 22)
    }

    #[test]
    fn unit_axes_round_trip() {
        for v in [[1.0, 0.0, 0.0], [0.0, -1.0, 0.0], [0.0, 0.0, 1.0], [0.0, 0.0, -1.0]] {
            let u = unpack_vector_11_11_10(pack(v));
            for i in 0..3 {
                assert!((u[i] - v[i]).abs() < 2.0 / 1023.0, "{v:?} -> {u:?}");
            }
        }
    }
}
