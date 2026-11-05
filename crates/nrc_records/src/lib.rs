//! NVIDIA Neural Radiance Cache: the shader-side record formats.
//!
//! [`WGSL`] (`shaders/nrc_records.wgsl`) is a port of the SDK's HLSL
//! (`vendor/NRC/Include/NrcStructures.h`, `NrcHelpers.hlsli` and the
//! packedData helpers of `Nrc.hlsli`) for the path tracer; [`WGSL_BUFFERS`]
//! (`shaders/nrc_buffers.wgsl`) adds the record read/write helpers over the
//! library's buffers. This crate is also the CPU reference: every function
//! below is the same HLSL ported line by line with `f32` arithmetic in the
//! same operation order (`mul_add` only where the HLSL says `mad`), and
//! `tests/gpu_roundtrip.rs` checks the WGSL against it on a Vulkan device.
//!
//! Build options: the SDK defaults, `NRC_PACK_PATH_16BITS 0` and
//! `TCNN_USES_FIXED_POINT_POSITIONS 0`.
//!
//! Exactness: the integer encoders whose inputs reach the quantizer without
//! transcendental or division steps (unorm16, snorm16, R11G11B10, packedData,
//! the indices, the record layouts) are bit-exact between the two. The ones
//! that go through `log2` or a reciprocal first (LogLuv, the octahedral
//! normal) quantize a float the GPU computes with its own approximate
//! `log2`/`rcp`, so a value that lands within a few ulp of a quantization
//! step can come out one step apart; the GPU test proves the quantization is
//! bit-exact given the GPU's own intermediate and bounds how often the step
//! differs. Float results (decoders, spherical directions) agree within a
//! few ulp, see the test.

#![allow(
    clippy::manual_clamp,
    reason = "max-then-min is HLSL's NaN-ignoring DX min/max; f32::clamp propagates NaN"
)]
#![allow(
    clippy::excessive_precision,
    clippy::approx_constant,
    reason = "NVIDIA's HLSL literals, kept digit for digit"
)]

use bytemuck::{Pod, Zeroable};
use core::mem::offset_of;

/// `shaders/nrc_records.wgsl`: the structs, constants and pure functions.
pub const WGSL: &str = include_str!("../shaders/nrc_records.wgsl");
/// `shaders/nrc_buffers.wgsl`: the record accessors over buffers the
/// including shader declares (see the file's header). Include after [`WGSL`].
pub const WGSL_BUFFERS: &str = include_str!("../shaders/nrc_buffers.wgsl");

pub type Float2 = [f32; 2];
pub type Float3 = [f32; 3];

// -------------------------------------------------------------------------
// Structures (NrcStructures.h)
// -------------------------------------------------------------------------

/// `enum class NrcCounter`
pub mod nrc_counter {
    pub const QUERIES: u32 = 0;
    pub const TRAINING_RECORDS: u32 = 1;
    pub const COUNT: u32 = 2;
}

/// `enum class NrcDebugPathTerminationReason`
pub mod nrc_debug_path_termination_reason {
    pub const UNSET: u32 = 0;
    pub const PATH_MISS_EXIT: u32 = 1;
    pub const CREATE_QUERY_IMMEDIATE: u32 = 2;
    pub const MAX_PATH_VERTICES: u32 = 3;
    pub const CREATE_QUERY_AFTER_DIRECT_LIGHTING: u32 = 4;
    pub const RUSSIAN_ROULETTE: u32 = 5;
    pub const BRDF_ABSORPTION: u32 = 6;
}

/// `struct NrcConstants`: 96 bytes, filled by the library's
/// `PopulateShaderConstants`, bound as a uniform.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct NrcConstants {
    pub frame_dimensions: [u32; 2],
    pub training_dimensions: [u32; 2],
    pub scene_pos_scale: Float3,
    pub samples_per_pixel: u32,
    pub scene_pos_bias: Float3,
    pub max_path_vertices: u32,
    pub learn_irradiance: u32,
    pub radiance_cache_direct: u32,
    pub radiance_unpack_multiplier: f32,
    /// `NrcResolveMode`
    pub resolve_mode: u32,
    pub enable_termination_heuristic: u32,
    pub skip_delta_vertices: u32,
    pub termination_heuristic_threshold: f32,
    pub training_termination_heuristic_threshold: f32,
    pub proportion_unbiased: f32,
    pub pad0: u32,
    pub pad1: u32,
    pub pad2: u32,
}

const _: () = {
    assert!(size_of::<NrcConstants>() == 96);
    assert!(offset_of!(NrcConstants, frame_dimensions) == 0);
    assert!(offset_of!(NrcConstants, training_dimensions) == 8);
    assert!(offset_of!(NrcConstants, scene_pos_scale) == 16);
    assert!(offset_of!(NrcConstants, samples_per_pixel) == 28);
    assert!(offset_of!(NrcConstants, scene_pos_bias) == 32);
    assert!(offset_of!(NrcConstants, max_path_vertices) == 44);
    assert!(offset_of!(NrcConstants, learn_irradiance) == 48);
    assert!(offset_of!(NrcConstants, radiance_cache_direct) == 52);
    assert!(offset_of!(NrcConstants, radiance_unpack_multiplier) == 56);
    assert!(offset_of!(NrcConstants, resolve_mode) == 60);
    assert!(offset_of!(NrcConstants, enable_termination_heuristic) == 64);
    assert!(offset_of!(NrcConstants, skip_delta_vertices) == 68);
    assert!(offset_of!(NrcConstants, termination_heuristic_threshold) == 72);
    assert!(offset_of!(NrcConstants, training_termination_heuristic_threshold) == 76);
    assert!(offset_of!(NrcConstants, proportion_unbiased) == 80);
    assert!(offset_of!(NrcConstants, pad0) == 84);
    assert!(offset_of!(NrcConstants, pad2) == 92);
};

/// `struct NrcRadianceParams`: 56 bytes, tightly packed (14 words).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct NrcRadianceParams {
    pub encoded_position: Float3,
    pub roughness: f32,
    pub normal: Float2,
    pub view_direction: Float2,
    pub albedo: Float3,
    pub specular: Float3,
}

const _: () = {
    assert!(size_of::<NrcRadianceParams>() == 56);
    assert!(offset_of!(NrcRadianceParams, encoded_position) == 0);
    assert!(offset_of!(NrcRadianceParams, roughness) == 12);
    assert!(offset_of!(NrcRadianceParams, normal) == 16);
    assert!(offset_of!(NrcRadianceParams, view_direction) == 24);
    assert!(offset_of!(NrcRadianceParams, albedo) == 32);
    assert!(offset_of!(NrcRadianceParams, specular) == 44);
};

/// `struct NrcPackedPathVertex`: 48 bytes (12 words).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct NrcPackedPathVertex {
    pub data: [u32; 7],
    pub pad0: u32,
    pub encoded_position: Float3,
    pub pad1: u32,
}

const _: () = {
    assert!(size_of::<NrcPackedPathVertex>() == 48);
    assert!(offset_of!(NrcPackedPathVertex, data) == 0);
    assert!(offset_of!(NrcPackedPathVertex, pad0) == 28);
    assert!(offset_of!(NrcPackedPathVertex, encoded_position) == 32);
    assert!(offset_of!(NrcPackedPathVertex, pad1) == 44);
};

/// `struct NrcPackedQueryPathInfo`: 8 bytes.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Pod, Zeroable)]
pub struct NrcPackedQueryPathInfo {
    /// LogLuv (`NrcEncodeLogLuvHdr`)
    pub prefix_throughput: u32,
    pub query_buffer_index: u32,
}

/// `struct NrcPackedTrainingPathInfo`: 8 bytes.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Pod, Zeroable)]
pub struct NrcPackedTrainingPathInfo {
    pub packed_data: u32,
    pub query_buffer_index: u32,
}

const _: () = {
    assert!(size_of::<NrcPackedQueryPathInfo>() == 8);
    assert!(offset_of!(NrcPackedQueryPathInfo, query_buffer_index) == 4);
    assert!(size_of::<NrcPackedTrainingPathInfo>() == 8);
    assert!(offset_of!(NrcPackedTrainingPathInfo, query_buffer_index) == 4);
};

/// `struct NrcTrainingPathInfo`
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NrcTrainingPathInfo {
    pub packed_data: u32,
    pub query_buffer_index: u32,
}

/// `struct NrcQueryPathInfo`
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct NrcQueryPathInfo {
    pub prefix_throughput: Float3,
    pub query_buffer_index: u32,
}

/// `struct NrcPathVertex`
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct NrcPathVertex {
    pub radiance: Float3,
    pub throughput: Float3,
    pub encoded_position: Float3,
    pub linear_roughness: f32,
    pub normal: Float3,
    pub view_direction: Float3,
    pub albedo: Float3,
    pub specular: Float3,
}

/// `struct NrcPathState` (`NRC_PACK_PATH_16BITS 0`)
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct NrcPathState {
    pub packed_prefix_throughput: u32,
    pub query_buffer_index: u32,
    pub primary_spread_radius: f32,
    pub cumul_spread_radius: f32,
    pub packed_data: u32,
    pub brdf_pdf: f32,
}

// -------------------------------------------------------------------------
// HLSL intrinsics
// -------------------------------------------------------------------------

/// HLSL `f32tof16`, round to nearest even (what NVIDIA's `F2F.F16.F32`
/// and the SPIR-V `PackHalf2x16` do), the half in the low 16 bits.
/// Overflow gives infinity, NaN a quiet NaN.
pub fn f32_to_f16(x: f32) -> u32 {
    let b = x.to_bits();
    let sign = (b >> 16) & 0x8000;
    let exp = ((b >> 23) & 0xff) as i32;
    let man = b & 0x7f_ffff;
    if exp == 0xff {
        return sign | 0x7c00 | if man != 0 { 0x200 } else { 0 };
    }
    let e = exp - 127 + 15;
    if e >= 0x1f {
        return sign | 0x7c00;
    }
    if e <= 0 {
        // A half subnormal (or zero): the value is m * 2^(exp - 150) with the
        // implicit bit, and a half subnormal's unit is 2^-24.
        let shift = (14 - e) as u32;
        if shift > 24 || exp == 0 {
            return sign;
        }
        let m = man | 0x80_0000;
        let half = 1u32 << (shift - 1);
        let mut r = m >> shift;
        let rest = m & ((1u32 << shift) - 1);
        if rest > half || (rest == half && (r & 1) == 1) {
            r += 1;
        }
        return sign | r;
    }
    let mut h = ((e as u32) << 10) | (man >> 13);
    let rest = man & 0x1fff;
    if rest > 0x1000 || (rest == 0x1000 && (h & 1) == 1) {
        h += 1; // a carry into the exponent is right, up to infinity
    }
    sign | h
}

/// HLSL `f16tof32` of the low 16 bits (exact).
pub fn f16_to_f32(h: u32) -> f32 {
    let h = h & 0xffff;
    let sign = (h & 0x8000) << 16;
    let exp = (h >> 10) & 0x1f;
    let man = h & 0x3ff;
    let bits = if exp == 0 {
        // zero or subnormal: man * 2^-24, exact in f32
        let v = man as f32 * f32::from_bits(0x3380_0000);
        v.to_bits() | sign
    } else if exp == 0x1f {
        sign | 0x7f80_0000 | (man << 13)
    } else {
        sign | ((exp + 112) << 23) | (man << 13)
    };
    f32::from_bits(bits)
}

/// HLSL `saturate`
fn saturate(v: f32) -> f32 {
    v.max(0.0).min(1.0)
}

/// HLSL `trunc` then the `(uint)`/`(int)` conversion of an in-range value.
fn trunc(v: f32) -> f32 {
    v.trunc()
}

fn dot3(a: Float3, b: Float3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// HLSL `normalize`: `v * rsqrt(dot(v, v))`.
fn normalize(v: Float3) -> Float3 {
    let inv = 1.0 / dot3(v, v).sqrt();
    [v[0] * inv, v[1] * inv, v[2] * inv]
}

fn mul3(a: Float3, s: f32) -> Float3 {
    [a[0] * s, a[1] * s, a[2] * s]
}

// -------------------------------------------------------------------------
// Packing helpers (NrcHelpers.hlsli)
// -------------------------------------------------------------------------

/// `NrcPackUnorm16Unsafe`
pub fn nrc_pack_unorm16_unsafe(v: f32) -> u32 {
    trunc(v * 65535.0 + 0.5) as u32
}

/// `NrcPackUnorm16`
pub fn nrc_pack_unorm16(v: f32) -> u32 {
    let v = if v.is_nan() { 0.0 } else { saturate(v) };
    nrc_pack_unorm16_unsafe(v)
}

/// `NrcPackUnorm2x16`
pub fn nrc_pack_unorm2x16(v: Float2) -> u32 {
    (nrc_pack_unorm16(v[1]) << 16) | nrc_pack_unorm16(v[0])
}

/// `1.f / 65535` as HLSL folds it.
pub const NRC_ONE_OVER_65535: f32 = 1.0 / 65535.0;

/// `NrcUnpackUnorm2x16`
pub fn nrc_unpack_unorm2x16(packed: u32) -> Float2 {
    [
        (packed & 0xffff) as f32 * NRC_ONE_OVER_65535,
        (packed >> 16) as f32 * NRC_ONE_OVER_65535,
    ]
}

/// `NrcFloatToSnorm16`
pub fn nrc_float_to_snorm16(v: f32) -> i32 {
    // min(max(v, -1), 1) with DX semantics (f32::max/min return the non-NaN
    // operand); NaN is taken care of first anyway.
    let v = if v.is_nan() {
        0.0
    } else {
        v.max(-1.0).min(1.0)
    };
    trunc(v * 32767.0 + if v >= 0.0 { 0.5 } else { -0.5 }) as i32
}

/// `NrcPackSnorm2x16`
pub fn nrc_pack_snorm2x16(v: Float2) -> u32 {
    ((nrc_float_to_snorm16(v[0]) & 0x0000ffff) | (nrc_float_to_snorm16(v[1]) << 16)) as u32
}

/// `NrcUnpackSnorm2x16`
pub fn nrc_unpack_snorm2x16(packed: u32) -> Float2 {
    let bits = [((packed << 16) as i32) >> 16, (packed as i32) >> 16];
    [
        (bits[0] as f32 / 32767.0).max(-1.0),
        (bits[1] as f32 / 32767.0).max(-1.0),
    ]
}

/// `NrcOctWrap`
pub fn nrc_oct_wrap(v: Float2) -> Float2 {
    [
        (1.0 - v[1].abs()) * if v[0] >= 0.0 { 1.0 } else { -1.0 },
        (1.0 - v[0].abs()) * if v[1] >= 0.0 { 1.0 } else { -1.0 },
    ]
}

/// `NrcOctToDirection`
pub fn nrc_oct_to_direction(p: Float2) -> Float3 {
    let mut n = [p[0], p[1], 1.0 - p[0].abs() - p[1].abs()];
    let tmp = if n[2] < 0.0 {
        nrc_oct_wrap([n[0], n[1]])
    } else {
        [n[0], n[1]]
    };
    n[0] = tmp[0];
    n[1] = tmp[1];
    normalize(n)
}

/// `NrcDirectionToOct`
pub fn nrc_direction_to_oct(n: Float3) -> Float2 {
    // Project the sphere onto the octahedron (|x|+|y|+|z| = 1) and then onto the xy-plane.
    let s = 1.0 / (n[0].abs() + n[1].abs() + n[2].abs());
    let p = [n[0] * s, n[1] * s];
    if n[2] < 0.0 { nrc_oct_wrap(p) } else { p }
}

/// `NrcEncodeNormal2x16`
pub fn nrc_encode_normal2x16(normal: Float3) -> u32 {
    let oct_normal = nrc_direction_to_oct(normal);
    nrc_pack_snorm2x16(oct_normal)
}

/// `NrcDecodeNormal2x16`
pub fn nrc_decode_normal2x16(packed_normal: u32) -> Float3 {
    let oct_normal = nrc_unpack_snorm2x16(packed_normal);
    nrc_oct_to_direction(oct_normal)
}

/// `NrcUnpackR11G11B10`
pub fn nrc_unpack_r11g11b10(packed: u32) -> Float3 {
    let r = f16_to_f32((packed << 4) & 0x7FF0);
    let g = f16_to_f32((packed >> 7) & 0x7FF0);
    let b = f16_to_f32((packed >> 17) & 0x7FE0);
    [r, g, b]
}

/// `NrcPackR11G11B10`
pub fn nrc_pack_r11g11b10(v: Float3) -> u32 {
    // Clamp upper bound so that it doesn't accidentally round up to INF
    let max = f32::from_bits(0x477C_0000);
    let v = [v[0].min(max), v[1].min(max), v[2].min(max)];
    // Exponent=15, Mantissa=1.11111
    let r = (f32_to_f16(v[0]).wrapping_add(8) >> 4) & 0x0000_07ff;
    let g = (f32_to_f16(v[1]).wrapping_add(8) << 7) & 0x003f_f800;
    let b = (f32_to_f16(v[2]).wrapping_add(16) << 17) & 0xffc0_0000;
    r | g | b
}

/// The HLSL `static const float3x3` initializers are double literals
/// converted to float (as WGSL's abstract floats are).
const fn m(rows: [[f64; 3]; 3]) -> [Float3; 3] {
    [
        [rows[0][0] as f32, rows[0][1] as f32, rows[0][2] as f32],
        [rows[1][0] as f32, rows[1][1] as f32, rows[1][2] as f32],
        [rows[2][0] as f32, rows[2][1] as f32, rows[2][2] as f32],
    ]
}

pub const NRC_RGB_TO_XYZ_REC709: [Float3; 3] = m([
    [0.4123907992659595, 0.3575843393838780, 0.1804807884018343],
    [0.2126390058715104, 0.7151686787677559, 0.0721923153607337],
    [0.0193308187155918, 0.1191947797946259, 0.9505321522496608],
]);

pub const NRC_XYZ_TO_RGB_REC709: [Float3; 3] = m([
    [3.240969941904522, -1.537383177570094, -0.4986107602930032],
    [-0.9692436362808803, 1.875967501507721, 0.04155505740717569],
    [0.05563007969699373, -0.2039769588889765, 1.056971514242878],
]);

/// `NrcRgbToXyzRec709`: `mul(M, c)`
pub fn nrc_rgb_to_xyz_rec709(c: Float3) -> Float3 {
    let m = NRC_RGB_TO_XYZ_REC709;
    [dot3(m[0], c), dot3(m[1], c), dot3(m[2], c)]
}

/// `NrcXyzToRgbRec709`: `mul(M, c)`
pub fn nrc_xyz_to_rgb_rec709(c: Float3) -> Float3 {
    let m = NRC_XYZ_TO_RGB_REC709;
    [dot3(m[0], c), dot3(m[1], c), dot3(m[2], c)]
}

/// HLSL `clamp(x, lo, hi)` = `min(max(x, lo), hi)` with DX min/max (a NaN
/// operand returns the other one, as `f32::max`/`f32::min` do).
fn clamp(x: f32, lo: f32, hi: f32) -> f32 {
    x.max(lo).min(hi)
}

/// The last step of `NrcEncodeLogLuvHdr`: quantize `logY` (already
/// `409.6 * (log2(Y) + 20)`) and chroma `uv`. Split out so the GPU test can
/// apply it to the GPU's own intermediates.
pub fn nrc_log_luv_quantize(log_y: f32, uv: Float2) -> u32 {
    let le = clamp(log_y, 0.0, 16383.0) as u32;
    if le == 0 {
        return 0;
    }
    let uve = [
        clamp(820.0 * uv[0], 0.0, 511.0) as u32,
        clamp(820.0 * uv[1], 0.0, 511.0) as u32,
    ];
    (le << 18) | (uve[0] << 9) | uve[1]
}

/// `NrcEncodeLogLuvHdr`: log-luminance in 14 bits (18..31), chroma u and v
/// in 9 bits each (9..17, 0..8). Black is exactly 0.
pub fn nrc_encode_log_luv_hdr(color: Float3) -> u32 {
    // Convert RGB to XYZ.
    let xyz = nrc_rgb_to_xyz_rec709(color);

    // Encode log2(Y) over the range [-20,20) in 14 bits (no sign bit).
    let log_y = 409.6 * (xyz[1].log2() + 20.0); // -inf if Y==0
    let le = clamp(log_y, 0.0, 16383.0) as u32;

    // Early out if zero luminance to avoid NaN in chroma computation.
    if le == 0 {
        return 0;
    }

    let inv_denom = 1.0 / (-2.0 * xyz[0] + 12.0 * xyz[1] + 3.0 * (xyz[0] + xyz[1] + xyz[2]));
    let uv = [4.0 * xyz[0] * inv_denom, 9.0 * xyz[1] * inv_denom];

    // Encode chroma (u,v) in 9 bits each.
    let uve = [
        clamp(820.0 * uv[0], 0.0, 511.0) as u32,
        clamp(820.0 * uv[1], 0.0, 511.0) as u32,
    ];

    (le << 18) | (uve[0] << 9) | uve[1]
}

/// `NrcDecodeLogLuvHdr`
pub fn nrc_decode_log_luv_hdr(packed_color: u32) -> Float3 {
    // Decode luminance Y from encoded log-luminance.
    let le = packed_color >> 18;
    if le == 0 {
        return [0.0, 0.0, 0.0];
    }

    let log_y = (le as f32 + 0.5) / 409.6 - 20.0;
    let y = 2.0f32.powf(log_y);

    let uve = [(packed_color >> 9) & 0x1ff, packed_color & 0x1ff];
    let uv = [(uve[0] as f32 + 0.5) / 820.0, (uve[1] as f32 + 0.5) / 820.0];

    let inv_denom = 1.0 / (6.0 * uv[0] - 16.0 * uv[1] + 12.0);
    let xy = [9.0 * uv[0] * inv_denom, 4.0 * uv[1] * inv_denom];

    //  X = Y / y * x
    //  Z = Y / y * (1 - x - y)
    let s = y / xy[1];
    let xyz = [s * xy[0], y, s * (1.0 - xy[0] - xy[1])];

    // Convert back to RGB and clamp to avoid out-of-gamut colors.
    let rgb = nrc_xyz_to_rgb_rec709(xyz);
    [rgb[0].max(0.0), rgb[1].max(0.0), rgb[2].max(0.0)]
}

pub const NRC_ONE_OVER_PI: f32 = 0.318309886183790671538_f64 as f32;
pub const NRC_ONE_OVER_TWO_PI: f32 = 0.159154943091895335769_f64 as f32;

/// `NrcCartesianToSphericalUnorm`
pub fn nrc_cartesian_to_spherical_unorm(p: Float3) -> Float2 {
    let p = normalize(p);
    [
        p[2].acos() * NRC_ONE_OVER_PI,
        (-p[1]).atan2(-p[0]) * NRC_ONE_OVER_TWO_PI + 0.5,
    ]
}

/// `NrcSafeCartesianToSphericalUnorm`
pub fn nrc_safe_cartesian_to_spherical_unorm(p: Float3) -> Float2 {
    if p.iter().any(|c| !c.is_finite()) {
        return [0.0, 0.0];
    }
    nrc_cartesian_to_spherical_unorm(p)
}

/// `NrcIsNan(float)`
pub fn nrc_is_nan(x: f32) -> bool {
    (x.to_bits() & 0x7FFF_FFFF) > 0x7F80_0000
}

/// `NrcIsInf(float)`
pub fn nrc_is_inf(x: f32) -> bool {
    (x.to_bits() & 0x7FFF_FFFF) == 0x7F80_0000
}

/// `NrcIsNan(float3)`
pub fn nrc_is_nan3(v: Float3) -> bool {
    nrc_is_nan(v[0]) || nrc_is_nan(v[1]) || nrc_is_nan(v[2])
}

/// `NrcIsInf(float3)`
pub fn nrc_is_inf3(v: Float3) -> bool {
    nrc_is_inf(v[0]) || nrc_is_inf(v[1]) || nrc_is_inf(v[2])
}

/// `NrcSanitizeNansInfs`
pub fn nrc_sanitize_nans_infs(input: Float3) -> Float3 {
    if nrc_is_inf3(input) || nrc_is_nan3(input) {
        return [0.0, 0.0, 0.0];
    }
    input
}

/// `NrcCalculateQueryPathIndex`
pub fn nrc_calculate_query_path_index(
    frame_dimensions: [u32; 2],
    pixel: [u32; 2],
    sample_index: u32,
    samples_per_pixel: u32,
) -> u32 {
    // Index in a linearized Tensor: (height, width, spp)
    let mut index = sample_index;
    let mut stride = samples_per_pixel;
    index = index.wrapping_add(pixel[0].wrapping_mul(stride));
    stride = stride.wrapping_mul(frame_dimensions[0]);
    index = index.wrapping_add(pixel[1].wrapping_mul(stride));
    index
}

/// `NrcCalculateTrainingPathIndex`
pub fn nrc_calculate_training_path_index(training_dimensions: [u32; 2], pixel: [u32; 2]) -> u32 {
    training_dimensions[0]
        .wrapping_mul(pixel[1])
        .wrapping_add(pixel[0])
}

/// `NrcCalculateTrainingPathVertexIndex`
pub fn nrc_calculate_training_path_vertex_index(
    training_dimensions: [u32; 2],
    pixel: [u32; 2],
    vertex_idx: u32,
    max_path_vertices: u32,
) -> u32 {
    let training_path_index = nrc_calculate_training_path_index(training_dimensions, pixel);
    training_path_index
        .wrapping_mul(max_path_vertices)
        .wrapping_add(vertex_idx)
}

/// `NrcUpdateTrainingPathVertex`
pub fn nrc_update_training_path_vertex(
    packed_path_vertex: NrcPackedPathVertex,
    radiance: Float3,
    throughput: Float3,
) -> NrcPackedPathVertex {
    let mut v = packed_path_vertex;
    v.data[0] = nrc_encode_log_luv_hdr(nrc_sanitize_nans_infs(radiance));
    v.data[1] = nrc_encode_log_luv_hdr(nrc_sanitize_nans_infs(throughput));
    v
}

/// `NrcUnpackPathVertex(packed, radiance, throughput)`
pub fn nrc_unpack_path_vertex_with(
    packed: NrcPackedPathVertex,
    radiance: Float3,
    throughput: Float3,
) -> NrcPathVertex {
    NrcPathVertex {
        radiance: nrc_sanitize_nans_infs(radiance),
        throughput: nrc_sanitize_nans_infs(throughput),
        linear_roughness: f32::from_bits(packed.data[2]),
        normal: nrc_decode_normal2x16(packed.data[3]),
        view_direction: nrc_decode_normal2x16(packed.data[4]),
        albedo: nrc_unpack_r11g11b10(packed.data[5]),
        specular: nrc_unpack_r11g11b10(packed.data[6]),
        encoded_position: packed.encoded_position,
    }
}

/// `NrcUnpackPathVertex(packed)`
pub fn nrc_unpack_path_vertex(packed: NrcPackedPathVertex) -> NrcPathVertex {
    NrcPathVertex {
        radiance: nrc_decode_log_luv_hdr(packed.data[0]),
        throughput: nrc_decode_log_luv_hdr(packed.data[1]),
        linear_roughness: f32::from_bits(packed.data[2]),
        normal: nrc_decode_normal2x16(packed.data[3]),
        view_direction: nrc_decode_normal2x16(packed.data[4]),
        albedo: nrc_unpack_r11g11b10(packed.data[5]),
        specular: nrc_unpack_r11g11b10(packed.data[6]),
        encoded_position: packed.encoded_position,
    }
}

/// `NrcEncodePosition(worldSpacePosition, nrcConstants)`: `mad`, so fused.
pub fn nrc_encode_position(world_space_position: Float3, nrc_constants: &NrcConstants) -> Float3 {
    let (s, b) = (nrc_constants.scene_pos_scale, nrc_constants.scene_pos_bias);
    [
        world_space_position[0].mul_add(s[0], b[0]),
        world_space_position[1].mul_add(s[1], b[1]),
        world_space_position[2].mul_add(s[2], b[2]),
    ]
}

/// `NrcEncodePosition(localPositionOffset, localOrigin, nrcConstants)`
pub fn nrc_encode_position_local(
    local_position_offset: Float3,
    local_origin: Float3,
    nrc_constants: &NrcConstants,
) -> Float3 {
    let p = [
        local_origin[0] + local_position_offset[0],
        local_origin[1] + local_position_offset[1],
        local_origin[2] + local_position_offset[2],
    ];
    nrc_encode_position(p, nrc_constants)
}

/// `NrcCreateRadianceParams`
pub fn nrc_create_radiance_params(vertex: &NrcPathVertex) -> NrcRadianceParams {
    NrcRadianceParams {
        encoded_position: vertex.encoded_position,
        roughness: vertex.linear_roughness,
        normal: nrc_safe_cartesian_to_spherical_unorm(vertex.normal),
        view_direction: nrc_safe_cartesian_to_spherical_unorm(vertex.view_direction),
        albedo: vertex.albedo,
        specular: vertex.specular,
    }
}

/// `NrcInitializePackedPathVertex`
pub fn nrc_initialize_packed_path_vertex(
    linear_roughness: f32,
    shading_normal: Float3,
    view_direction: Float3,
    diffuse_albedo: Float3,
    specular_albedo: Float3,
    encoded_position: Float3,
) -> NrcPackedPathVertex {
    NrcPackedPathVertex {
        data: [
            nrc_encode_log_luv_hdr([0.0, 0.0, 0.0]),
            nrc_encode_log_luv_hdr([1.0, 1.0, 1.0]),
            linear_roughness.to_bits(),
            nrc_encode_normal2x16(shading_normal),
            nrc_encode_normal2x16(view_direction),
            nrc_pack_r11g11b10(diffuse_albedo),
            nrc_pack_r11g11b10(specular_albedo),
        ],
        pad0: 0,
        encoded_position,
        pad1: 0,
    }
}

/// `NrcPackTrainingPathInfo`
pub fn nrc_pack_training_path_info(path_info: NrcTrainingPathInfo) -> NrcPackedTrainingPathInfo {
    NrcPackedTrainingPathInfo {
        packed_data: path_info.packed_data,
        query_buffer_index: path_info.query_buffer_index,
    }
}

/// `NrcUnpackTrainingPathInfo`
pub fn nrc_unpack_training_path_info(
    packed_path_info: NrcPackedTrainingPathInfo,
) -> NrcTrainingPathInfo {
    NrcTrainingPathInfo {
        packed_data: packed_path_info.packed_data,
        query_buffer_index: packed_path_info.query_buffer_index,
    }
}

/// `NrcUnpackQueryPathInfo`
pub fn nrc_unpack_query_path_info(packed_path_info: NrcPackedQueryPathInfo) -> NrcQueryPathInfo {
    NrcQueryPathInfo {
        prefix_throughput: nrc_decode_log_luv_hdr(packed_path_info.prefix_throughput),
        query_buffer_index: packed_path_info.query_buffer_index,
    }
}

/// `NrcUnpackQueryRadiance`
pub fn nrc_unpack_query_radiance(
    nrc_constants: &NrcConstants,
    packed_query_radiance: Float3,
) -> Float3 {
    mul3(
        packed_query_radiance,
        nrc_constants.radiance_unpack_multiplier,
    )
}

// -------------------------------------------------------------------------
// packedData and the termination heuristic (Nrc.hlsli)
// -------------------------------------------------------------------------
//   | 15 .. 12 Flags | 11 .. 7 Termination reason | 6 .. 0 Vertex count |

pub const NRC_TERMINATION_REASON_SHIFT: u32 = 7;
pub const NRC_PATH_FLAGS_SHIFT: u32 = 12;
pub const NRC_PATH_FLAG_HAS_EXITED_SCENE: u32 = 1 << NRC_PATH_FLAGS_SHIFT;
pub const NRC_PATH_FLAG_IS_UNBIASED: u32 = 1 << (NRC_PATH_FLAGS_SHIFT + 1);
pub const NRC_PATH_FLAG_PREVIOUS_HIT_WAS_DELTA_LOBE: u32 = 1 << (NRC_PATH_FLAGS_SHIFT + 2);
pub const NRC_PATH_FLAG_HEURISTIC_RESET: u32 = 1 << (NRC_PATH_FLAGS_SHIFT + 3);
pub const NRC_VERTEX_COUNT_MASK: u32 = (1 << NRC_TERMINATION_REASON_SHIFT) - 1;
pub const NRC_TERMINATION_REASON_MASK: u32 =
    ((1 << NRC_PATH_FLAGS_SHIFT) - 1) & !NRC_VERTEX_COUNT_MASK;

/// `NrcEvaluateTerminationHeuristic`
pub fn nrc_evaluate_termination_heuristic(path_state: &NrcPathState, threshold: f32) -> bool {
    (path_state.primary_spread_radius > 0.0)
        && (path_state.cumul_spread_radius > (threshold * path_state.primary_spread_radius))
}

/// `NrcSetFlag(packedData, flag)`
pub fn nrc_set_flag(packed_data: u32, flag: u32) -> u32 {
    packed_data | flag
}

/// `NrcClearFlag`
pub fn nrc_clear_flag(packed_data: u32, flag: u32) -> u32 {
    packed_data & !flag
}

/// `NrcSetFlag(packedData, flag, value)`
pub fn nrc_set_flag_value(packed_data: u32, flag: u32, value: bool) -> u32 {
    (packed_data & !flag) | if value { flag } else { 0 }
}

/// `NrcGetFlag`
pub fn nrc_get_flag(packed_data: u32, flag: u32) -> bool {
    (packed_data & flag) != 0
}

/// `NrcGetVertexCount`
pub fn nrc_get_vertex_count(packed_data: u32) -> u32 {
    packed_data & NRC_VERTEX_COUNT_MASK
}

/// `NrcSetVertexCount`
pub fn nrc_set_vertex_count(packed_data: u32, vertex_count: u32) -> u32 {
    (packed_data & !NRC_VERTEX_COUNT_MASK) | vertex_count
}

/// `NrcSetDebugPathTerminationReason` (on `pathState.packedData`)
pub fn nrc_set_debug_path_termination_reason(packed_data: u32, reason: u32) -> u32 {
    (packed_data & !NRC_TERMINATION_REASON_MASK) | (reason << NRC_TERMINATION_REASON_SHIFT)
}

/// `NrcGetDebugPathTerminationReason` (on `pathState.packedData`)
pub fn nrc_get_debug_path_termination_reason(packed_data: u32) -> u32 {
    (packed_data & NRC_TERMINATION_REASON_MASK) >> NRC_TERMINATION_REASON_SHIFT
}
