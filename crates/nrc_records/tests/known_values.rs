//! The Rust reference against values derived by hand from the HLSL formulas
//! (the derivation is in each comment), not by running the code.

use nrc_records::*;

#[test]
fn constant_one_over_65535_matches_the_wgsl_bits() {
    // The WGSL writes 1.f / 65535 as bitcast<f32>(0x37800080u): 1/65535 =
    // 2^-16 * 65536/65535 = 2^-16 * (1 + 2^-16 + ...), mantissa
    // 2^-16 * 2^23 = 0x80 (the next bit, 2^-32 relative, rounds down).
    assert_eq!(NRC_ONE_OVER_65535.to_bits(), 0x3780_0080);
}

#[test]
fn unorm16() {
    // trunc(v * 65535 + 0.5): 0.5 -> trunc(32768.0) = 32768
    assert_eq!(nrc_pack_unorm16(0.5), 0x8000);
    assert_eq!(nrc_pack_unorm16(1.0), 0xffff);
    assert_eq!(nrc_pack_unorm16(0.0), 0);
    // saturate
    assert_eq!(nrc_pack_unorm16(-1.0), 0);
    assert_eq!(nrc_pack_unorm16(2.0), 0xffff);
    // isnan -> 0
    assert_eq!(nrc_pack_unorm16(f32::NAN), 0);
    // 1/65535 -> trunc(1 + 0.5) = 1
    assert_eq!(nrc_pack_unorm16(1.0 / 65535.0), 1);
    // y in the high half: (0.5, 1) -> 0xffff << 16 | 0x8000
    assert_eq!(nrc_pack_unorm2x16([0.5, 1.0]), 0xffff_8000);
    // 0xffff * (1/65535) = 1 (65535 * 0x37800080 rounds to 1.0)
    assert_eq!(nrc_unpack_unorm2x16(0xffff_0000), [0.0, 1.0]);
}

#[test]
fn snorm16() {
    // 0.5 * 32767 + 0.5 = 16384.0 -> 16384
    assert_eq!(nrc_float_to_snorm16(0.5), 16384);
    // -0.5 * 32767 - 0.5 = -16384.0 -> -16384
    assert_eq!(nrc_float_to_snorm16(-0.5), -16384);
    // clamped to +-1 -> +-32767 (trunc of +-32767.5)
    assert_eq!(nrc_float_to_snorm16(3.0), 32767);
    assert_eq!(nrc_float_to_snorm16(-3.0), -32767);
    assert_eq!(nrc_float_to_snorm16(f32::NAN), 0);
    // -16384 = 0xc000 in the high half, 16384 = 0x4000 in the low half
    assert_eq!(nrc_pack_snorm2x16([0.5, -0.5]), 0xc000_4000);
    // -32768 / 32767 < -1, so max(.., -1) = -1; 32767 / 32767 = 1
    assert_eq!(nrc_unpack_snorm2x16(0x7fff_8000), [-1.0, 1.0]);
}

#[test]
fn octahedral_normals() {
    // +Z: p = (0, 0) / 1, z >= 0 so no wrap -> snorm (0, 0) -> 0
    assert_eq!(nrc_encode_normal2x16([0.0, 0.0, 1.0]), 0);
    // -Z: p = (0, 0) wraps to ((1 - 0) * +1, (1 - 0) * +1) = (1, 1) -> 0x7fff in both halves
    assert_eq!(nrc_encode_normal2x16([0.0, 0.0, -1.0]), 0x7fff_7fff);
    // +X: p = (1, 0) -> 0x7fff low
    assert_eq!(nrc_encode_normal2x16([1.0, 0.0, 0.0]), 0x0000_7fff);
    // -X: p = (-1, 0) -> -32767 = 0x8001 low
    assert_eq!(nrc_encode_normal2x16([-1.0, 0.0, 0.0]), 0x0000_8001);
    // +Y, not unit length (the projection divides by |x|+|y|+|z|): p = (0, 1)
    assert_eq!(nrc_encode_normal2x16([0.0, 5.0, 0.0]), 0x7fff_0000);
    // (1, 1, 0) -> p = (0.5, 0.5) -> 16384 both
    assert_eq!(nrc_encode_normal2x16([1.0, 1.0, 0.0]), 0x4000_4000);
    // Decode 0x7fff7fff: (1, 1) -> n = (1, 1, -1) -> wrap (0, 0) -> (0, 0, -1)
    assert_eq!(nrc_decode_normal2x16(0x7fff_7fff), [0.0, 0.0, -1.0]);
    // Decode 0: (0, 0, 1)
    assert_eq!(nrc_decode_normal2x16(0), [0.0, 0.0, 1.0]);
    // Decode 0x7fff: (1, 0, 0)
    assert_eq!(nrc_decode_normal2x16(0x7fff), [1.0, 0.0, 0.0]);
}

#[test]
fn f16_conversion() {
    assert_eq!(f32_to_f16(1.0), 0x3c00);
    assert_eq!(f32_to_f16(-2.0), 0xc000);
    assert_eq!(f32_to_f16(65504.0), 0x7bff);
    assert_eq!(f32_to_f16(65520.0), 0x7c00); // the midpoint to 2^16 rounds (even) to infinity
    // 1 + 2^-11 is the midpoint between 1 and 1 + 2^-10: ties to even -> 1
    assert_eq!(f32_to_f16(1.0 + 1.0 / 2048.0), 0x3c00);
    // 1 + 3 * 2^-11: midpoint between 0x3c01 and 0x3c02: even -> 0x3c02
    assert_eq!(f32_to_f16(1.0 + 3.0 / 2048.0), 0x3c02);
    // 2^-24: the smallest half subnormal; 2^-25 is the tie to 0 (even)
    assert_eq!(f32_to_f16(2.0f32.powi(-24)), 0x0001);
    assert_eq!(f32_to_f16(2.0f32.powi(-25)), 0x0000);
    assert_eq!(f32_to_f16(1.5 * 2.0f32.powi(-25)), 0x0001);
    // 2^-14: the smallest normal half
    assert_eq!(f32_to_f16(2.0f32.powi(-14)), 0x0400);
    assert_eq!(f16_to_f32(0x0001), 2.0f32.powi(-24));
    assert_eq!(f16_to_f32(0x3c00), 1.0);
    assert_eq!(f16_to_f32(0xc000), -2.0);
    assert_eq!(f16_to_f32(0x7c00), f32::INFINITY);
}

#[test]
fn r11g11b10() {
    // f32tof16(1) = 0x3c00; r = (0x3c00 + 8) >> 4 = 0x3c0
    assert_eq!(nrc_pack_r11g11b10([1.0, 0.0, 0.0]), 0x0000_03c0);
    // g = (0x3c00 + 8) << 7 = 0x1e0400, & 0x3ff800 = 0x1e0000
    assert_eq!(nrc_pack_r11g11b10([0.0, 1.0, 0.0]), 0x001e_0000);
    // b = (0x3c00 + 16) << 17 = 0x78200000, & 0xffc00000 = 0x78000000
    assert_eq!(nrc_pack_r11g11b10([0.0, 0.0, 1.0]), 0x7800_0000);
    // zero: (0 + 8) >> 4 = 0, (0 + 8) << 7 = 0x400 masked off, (0 + 16) << 17 = 0x200000 masked off
    assert_eq!(nrc_pack_r11g11b10([0.0, 0.0, 0.0]), 0);
    // 1e5 clamps to 64512 = f16 0x7be0; (0x7be0 + 8) >> 4 = 0x7be
    assert_eq!(nrc_pack_r11g11b10([1e5, 0.0, 0.0]), 0x7be);
    // 0.5 = f16 0x3800: r 0x380, g 0x380 << 11 = 0x1c0000, b 0x380 >> 1 << 22 = 0x70000000
    assert_eq!(nrc_pack_r11g11b10([0.5, 0.5, 0.5]), 0x701c_0380);
    // Unpack: r = f16tof32((0x3c0 << 4) & 0x7ff0) = f16tof32(0x3c00) = 1
    assert_eq!(nrc_unpack_r11g11b10(0x0000_03c0), [1.0, 0.0, 0.0]);
    assert_eq!(nrc_unpack_r11g11b10(0x701c_0380), [0.5, 0.5, 0.5]);
    assert_eq!(nrc_unpack_r11g11b10(0x7800_0000), [0.0, 0.0, 1.0]);
}

#[test]
fn log_luv() {
    // Black: Y = 0, log2(0) = -inf, logY = -inf, clamp -> 0, Le = 0 -> 0
    assert_eq!(nrc_encode_log_luv_hdr([0.0, 0.0, 0.0]), 0);
    assert_eq!(nrc_decode_log_luv_hdr(0), [0.0, 0.0, 0.0]);
    // A luminance below 2^-20 (Le = 0) is black too
    assert_eq!(nrc_encode_log_luv_hdr([1e-7, 1e-7, 1e-7]), 0);
    // White (1, 1, 1): Y = 0.21264 + 0.71517 + 0.07219 = 1 (to f32
    // rounding), log2 Y ~ 0, 20 + ~0 = 20, Le = 409.6 * 20 = 8192.
    // X = 0.950456, Z = 1.089058, denominator -2X + 12Y + 3(X+Y+Z) =
    // -1.900912 + 12 + 9.118541 = 19.217629; u = 4X/d = 0.197830 -> 820u =
    // 162.22 -> 162; v = 9/d = 0.468321 -> 820v = 384.02 -> 384.
    // 8192 << 18 | 162 << 9 | 384 = 0x80000000 | 0x14400 | 0x180
    assert_eq!(nrc_encode_log_luv_hdr([1.0, 1.0, 1.0]), 0x8001_4580);
    // Gray 2: Le = 409.6 * 21 = 8601.6 -> 8601, chroma unchanged
    assert_eq!(
        nrc_encode_log_luv_hdr([2.0, 2.0, 2.0]),
        (8601 << 18) | (162 << 9) | 384
    );
    // Huge: log2 Y >= 20 saturates Le at 16383
    assert_eq!(nrc_encode_log_luv_hdr([1e7, 1e7, 1e7]) >> 18, 16383);
    // Decode white: Y = 2^((8192.5 / 409.6) - 20) = 2^0.0012207 = 1.000846;
    // xy back to about D65 (0.3127, 0.3290), so RGB is about 1.
    let w = nrc_decode_log_luv_hdr(0x8001_4580);
    for c in w {
        assert!((c - 1.0).abs() < 0.01, "{w:?}");
    }
    // The initialized vertex's throughput is white
    let v = nrc_initialize_packed_path_vertex(
        0.25,
        [0.0, 0.0, 1.0],
        [1.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0],
        [0.5, 0.25, 0.75],
    );
    assert_eq!(
        v.data,
        [
            0,
            0x8001_4580,
            0.25f32.to_bits(),
            0,
            0x7fff,
            0x3c0,
            0x7800_0000
        ]
    );
    assert_eq!((v.pad0, v.pad1), (0, 0));
    assert_eq!(v.encoded_position, [0.5, 0.25, 0.75]);
}

#[test]
fn spherical() {
    // (-1, 0, 0): acos(0) / pi = 0.5; atan2(-0, 1) = -0 -> -0 / 2pi + 0.5 = 0.5
    let s = nrc_cartesian_to_spherical_unorm([-2.0, 0.0, 0.0]);
    assert!((s[0] - 0.5).abs() < 1e-6 && s[1] == 0.5, "{s:?}");
    // (0, -1, 0): acos(0) / pi = 0.5; atan2(1, -0) = pi/2 -> 0.25 + 0.5 = 0.75
    let s = nrc_cartesian_to_spherical_unorm([0.0, -1.0, 0.0]);
    assert!(
        (s[0] - 0.5).abs() < 1e-6 && (s[1] - 0.75).abs() < 1e-6,
        "{s:?}"
    );
    // (0, 0, -1): acos(-1) / pi = 1
    let s = nrc_cartesian_to_spherical_unorm([0.0, 0.0, -1.0]);
    assert!((s[0] - 1.0).abs() < 1e-6, "{s:?}");
    // Safe: anything not finite -> (0, 0)
    assert_eq!(
        nrc_safe_cartesian_to_spherical_unorm([f32::INFINITY, 0.0, 1.0]),
        [0.0, 0.0]
    );
    assert_eq!(
        nrc_safe_cartesian_to_spherical_unorm([0.0, f32::NAN, 1.0]),
        [0.0, 0.0]
    );
}

#[test]
fn nan_inf() {
    assert!(nrc_is_nan(f32::NAN) && !nrc_is_nan(f32::INFINITY) && !nrc_is_nan(1.0));
    assert!(nrc_is_inf(f32::NEG_INFINITY) && !nrc_is_inf(f32::NAN) && !nrc_is_inf(f32::MAX));
    assert!(nrc_is_nan3([0.0, 0.0, f32::NAN]) && nrc_is_inf3([f32::INFINITY, 0.0, 0.0]));
    assert_eq!(nrc_sanitize_nans_infs([1.0, f32::NAN, 2.0]), [0.0; 3]);
    assert_eq!(nrc_sanitize_nans_infs([1.0, f32::INFINITY, 2.0]), [0.0; 3]);
    assert_eq!(nrc_sanitize_nans_infs([1.0, -3.0, 2.0]), [1.0, -3.0, 2.0]);
}

#[test]
fn indices() {
    // sample 1 + x 3 * spp 2 + y 2 * (spp 2 * width 1280) = 1 + 6 + 5120
    assert_eq!(
        nrc_calculate_query_path_index([1280, 720], [3, 2], 1, 2),
        5127
    );
    // 197 * 7 + 5
    assert_eq!(nrc_calculate_training_path_index([197, 111], [5, 7]), 1384);
    // 1384 * 8 + 3
    assert_eq!(
        nrc_calculate_training_path_vertex_index([197, 111], [5, 7], 3, 8),
        11075
    );
}

#[test]
fn packed_data() {
    // count 5 | RussianRoulette (5) << 7 | IsUnbiased (1 << 13) = 5 + 640 + 8192
    let mut p = 0;
    p = nrc_set_vertex_count(p, 5);
    p = nrc_set_debug_path_termination_reason(
        p,
        nrc_debug_path_termination_reason::RUSSIAN_ROULETTE,
    );
    p = nrc_set_flag(p, NRC_PATH_FLAG_IS_UNBIASED);
    assert_eq!(p, 0x2285);
    assert_eq!(NRC_VERTEX_COUNT_MASK, 0x7f);
    assert_eq!(NRC_TERMINATION_REASON_MASK, 0xf80);
    assert_eq!(
        [
            NRC_PATH_FLAG_HAS_EXITED_SCENE,
            NRC_PATH_FLAG_IS_UNBIASED,
            NRC_PATH_FLAG_PREVIOUS_HIT_WAS_DELTA_LOBE,
            NRC_PATH_FLAG_HEURISTIC_RESET
        ],
        [0x1000, 0x2000, 0x4000, 0x8000]
    );
    assert_eq!(nrc_get_vertex_count(p), 5);
    assert_eq!(nrc_get_debug_path_termination_reason(p), 5);
    assert!(
        nrc_get_flag(p, NRC_PATH_FLAG_IS_UNBIASED)
            && !nrc_get_flag(p, NRC_PATH_FLAG_HAS_EXITED_SCENE)
    );
    // Clearing and value-setting
    assert_eq!(nrc_clear_flag(p, NRC_PATH_FLAG_IS_UNBIASED), 0x0285);
    assert_eq!(
        nrc_set_flag_value(p, NRC_PATH_FLAG_HEURISTIC_RESET, true),
        0xa285
    );
    assert_eq!(
        nrc_set_flag_value(p, NRC_PATH_FLAG_IS_UNBIASED, false),
        0x0285
    );
    // A new count replaces the old one; reason PathMissExit (1) replaces 5
    assert_eq!(nrc_set_vertex_count(p, 2), 0x2282);
    assert_eq!(nrc_set_debug_path_termination_reason(p, 1), 0x2085);
}

#[test]
fn termination_heuristic() {
    let s = NrcPathState {
        primary_spread_radius: 2.0,
        cumul_spread_radius: 0.3,
        ..Default::default()
    };
    // 0.3 > 0.1 * 2
    assert!(nrc_evaluate_termination_heuristic(&s, 0.1));
    // 0.3 > 0.2 * 2 is false
    assert!(!nrc_evaluate_termination_heuristic(&s, 0.2));
    // primary radius 0 never terminates
    assert!(!nrc_evaluate_termination_heuristic(
        &NrcPathState {
            cumul_spread_radius: 1.0,
            ..Default::default()
        },
        0.0
    ));
}

#[test]
fn position_and_records() {
    let c = NrcConstants {
        scene_pos_scale: [0.5, 0.25, 0.125],
        scene_pos_bias: [0.5, 0.5, 0.5],
        radiance_unpack_multiplier: 4.0,
        ..Default::default()
    };
    // (1, 2, 3) * (0.5, 0.25, 0.125) + 0.5
    assert_eq!(nrc_encode_position([1.0, 2.0, 3.0], &c), [1.0, 1.0, 0.875]);
    // (0.5 + 0.5, 1 + 1, 4 - 1) -> same
    assert_eq!(
        nrc_encode_position_local([0.5, 1.0, -1.0], [0.5, 1.0, 4.0], &c),
        [1.0, 1.0, 0.875]
    );
    assert_eq!(
        nrc_unpack_query_radiance(&c, [1.0, 0.5, 0.25]),
        [4.0, 2.0, 1.0]
    );
    // Record bytes: the 56-byte NrcRadianceParams words in declaration order
    let p = NrcRadianceParams {
        encoded_position: [1.0, 2.0, 3.0],
        roughness: 4.0,
        normal: [5.0, 6.0],
        view_direction: [7.0, 8.0],
        albedo: [9.0, 10.0, 11.0],
        specular: [12.0, 13.0, 14.0],
    };
    let words: &[f32] = bytemuck::cast_slice(bytemuck::bytes_of(&p));
    assert_eq!(
        words,
        (1..=14).map(|i| i as f32).collect::<Vec<_>>().as_slice()
    );
    let info = nrc_pack_training_path_info(NrcTrainingPathInfo {
        packed_data: 0x2285,
        query_buffer_index: 77,
    });
    assert_eq!(bytemuck::cast::<_, [u32; 2]>(info), [0x2285, 77]);
    assert_eq!(
        nrc_unpack_training_path_info(info),
        NrcTrainingPathInfo {
            packed_data: 0x2285,
            query_buffer_index: 77
        }
    );
    let q = nrc_unpack_query_path_info(NrcPackedQueryPathInfo {
        prefix_throughput: 0,
        query_buffer_index: 9,
    });
    assert_eq!(
        q,
        NrcQueryPathInfo {
            prefix_throughput: [0.0; 3],
            query_buffer_index: 9
        }
    );
}
