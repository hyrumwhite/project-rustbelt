// NVIDIA Neural Radiance Cache: the shader-side record formats, ported from
// the SDK's HLSL (vendor/NRC/Include/NrcStructures.h, NrcHelpers.hlsli and
// the packedData helpers of Nrc.hlsli) to WGSL.
//
// Build options: the SDK defaults, NRC_PACK_PATH_16BITS 0 (NrcPackableUint and
// NrcPackableFloat are 32-bit) and TCNN_USES_FIXED_POINT_POSITIONS 0 (an
// encoded position is float3 = worldPosition * scenePosScale + scenePosBias).
//
// Every function names the HLSL function it ports. Names are NVIDIA's in
// snake_case with an nrc_ prefix; HLSL overloads get a suffix.
// asuint/asfloat become bitcast, f32tof16/f16tof32 become pack2x16float/
// unpack2x16float on one lane (naga capability SHADER_FLOAT16_IN_FLOAT32,
// which wgpu grants on Vulkan).
//
// This file stands alone (pure functions, structs, constants). The record
// read/write helpers over the library's buffers are in nrc_buffers.wgsl,
// which needs the buffers declared by the including shader.
//
// crates/nrc_records/src/lib.rs is the CPU reference of every function here,
// and tests/gpu_roundtrip.rs compares the two on a Vulkan device.
//
// NaN and infinity: WGSL lets an implementation assume floats are finite
// (there is no isnan/isinf builtin, and min/max/clamp with a NaN operand give
// an indeterminate result). The HLSL already avoids the builtins in its own
// checks ("Built-in isnan() and isinf() need special compiler option and might
// be optimized away") and tests the bits instead; these ports do the same,
// also for the builtin isnan in NrcPackUnorm16/NrcFloatToSnorm16 and isfinite
// in NrcSafeCartesianToSphericalUnorm, so the explicit NaN/Inf tests are
// reliable. What WGSL cannot promise is the DX min/max rule (a NaN operand
// returns the other operand) that the HLSL relies on implicitly in
// NrcPackR11G11B10's min and NrcEncodeLogLuvHdr's clamp of log2(Y) for a
// negative or NaN Y, nor log2(0) = -inf (SPIR-V leaves log2(x <= 0)
// undefined; the HLSL relies on -inf, clamped to 0). NVIDIA's Vulkan driver
// does both the DX way, so the results agree there; elsewhere feed those
// functions finite, non-negative colors, as NRC's own flow does after
// NrcSanitizeNansInfs.

// -------------------------------------------------------------------------
// Structures (NrcStructures.h)
// -------------------------------------------------------------------------

// enum class NrcCounter
const NRC_COUNTER_QUERIES: u32 = 0u;
const NRC_COUNTER_TRAINING_RECORDS: u32 = 1u;
const NRC_COUNTER_COUNT: u32 = 2u;

// enum class NrcDebugPathTerminationReason
const NRC_DEBUG_PATH_TERMINATION_REASON_UNSET: u32 = 0u;
const NRC_DEBUG_PATH_TERMINATION_REASON_PATH_MISS_EXIT: u32 = 1u;
const NRC_DEBUG_PATH_TERMINATION_REASON_CREATE_QUERY_IMMEDIATE: u32 = 2u;
const NRC_DEBUG_PATH_TERMINATION_REASON_MAX_PATH_VERTICES: u32 = 3u;
const NRC_DEBUG_PATH_TERMINATION_REASON_CREATE_QUERY_AFTER_DIRECT_LIGHTING: u32 = 4u;
const NRC_DEBUG_PATH_TERMINATION_REASON_RUSSIAN_ROULETTE: u32 = 5u;
const NRC_DEBUG_PATH_TERMINATION_REASON_BRDF_ABSORPTION: u32 = 6u;

// enum class NrcResolveMode
const NRC_RESOLVE_MODE_ADD_QUERY_RESULT_TO_OUTPUT: u32 = 0u;
const NRC_RESOLVE_MODE_REPLACE_OUTPUT_WITH_QUERY_RESULT: u32 = 1u;
const NRC_RESOLVE_MODE_TRAINING_BOUNCE_HEAT_MAP: u32 = 2u;
const NRC_RESOLVE_MODE_TRAINING_BOUNCE_HEAT_MAP_SMOOTHED: u32 = 3u;
const NRC_RESOLVE_MODE_PRIMARY_VERTEX_TRAINING_RADIANCE: u32 = 4u;
const NRC_RESOLVE_MODE_PRIMARY_VERTEX_TRAINING_RADIANCE_SMOOTHED: u32 = 5u;
const NRC_RESOLVE_MODE_SECONDARY_VERTEX_TRAINING_RADIANCE: u32 = 6u;
const NRC_RESOLVE_MODE_SECONDARY_VERTEX_TRAINING_RADIANCE_SMOOTHED: u32 = 7u;
const NRC_RESOLVE_MODE_QUERY_INDEX: u32 = 8u;
const NRC_RESOLVE_MODE_TRAINING_QUERY_INDEX: u32 = 9u;
const NRC_RESOLVE_MODE_DIRECT_CACHE_VIEW: u32 = 10u;

// struct NrcConstants: 96 bytes, for a uniform buffer. The byte offsets match
// the C layout (WGSL places a scalar straight after a vec3<f32>, whose size is
// 12, in the uniform address space too): frame_dimensions 0,
// training_dimensions 8, scene_pos_scale 16, samples_per_pixel 28,
// scene_pos_bias 32, max_path_vertices 44, learn_irradiance 48,
// radiance_cache_direct 52, radiance_unpack_multiplier 56, resolve_mode 60,
// enable_termination_heuristic 64, skip_delta_vertices 68,
// termination_heuristic_threshold 72,
// training_termination_heuristic_threshold 76, proportion_unbiased 80,
// pad0..pad2 84..96. (The crate's naga test checks every offset, and the GPU
// test reads every field back from a uniform.)
struct NrcConstants {
    frame_dimensions: vec2<u32>,
    training_dimensions: vec2<u32>,
    scene_pos_scale: vec3<f32>,
    samples_per_pixel: u32,
    scene_pos_bias: vec3<f32>,
    max_path_vertices: u32,
    learn_irradiance: u32,
    radiance_cache_direct: u32,
    radiance_unpack_multiplier: f32,
    resolve_mode: u32,
    enable_termination_heuristic: u32,
    skip_delta_vertices: u32,
    termination_heuristic_threshold: f32,
    training_termination_heuristic_threshold: f32,
    proportion_unbiased: f32,
    pad0: u32,
    pad1: u32,
    pad2: u32,
}

// struct NrcRadianceParams. As a value only: in a buffer it is 14 tightly
// packed words (see NRC_RADIANCE_PARAMS_* below and nrc_buffers.wgsl).
struct NrcRadianceParams {
    encoded_position: vec3<f32>,
    roughness: f32,
    normal: vec2<f32>,
    view_direction: vec2<f32>,
    albedo: vec3<f32>,
    specular: vec3<f32>,
}

// struct NrcTrainingPathInfo
struct NrcTrainingPathInfo {
    packed_data: u32,
    query_buffer_index: u32,
}

// struct NrcPackedTrainingPathInfo
struct NrcPackedTrainingPathInfo {
    packed_data: u32,
    query_buffer_index: u32,
}

// struct NrcQueryPathInfo
struct NrcQueryPathInfo {
    prefix_throughput: vec3<f32>,
    query_buffer_index: u32,
}

// struct NrcPackedQueryPathInfo
struct NrcPackedQueryPathInfo {
    prefix_throughput: u32,
    query_buffer_index: u32,
}

// struct NrcPathVertex
struct NrcPathVertex {
    radiance: vec3<f32>,
    throughput: vec3<f32>,
    encoded_position: vec3<f32>,
    linear_roughness: f32,
    normal: vec3<f32>,
    view_direction: vec3<f32>,
    albedo: vec3<f32>,
    specular: vec3<f32>,
}

// struct NrcPackedPathVertex. As a value only: in a buffer it is 12 words.
struct NrcPackedPathVertex {
    data: array<u32, 7>,
    pad0: u32,
    encoded_position: vec3<f32>,
    pad1: u32,
}

// struct NrcPathState (NRC_PACK_PATH_16BITS 0)
struct NrcPathState {
    packed_prefix_throughput: u32,
    query_buffer_index: u32,
    primary_spread_radius: f32,
    cumul_spread_radius: f32,
    packed_data: u32,
    brdf_pdf: f32,
}

// Buffer layouts as the library reads them (DX scalar layout), in 32-bit
// words. Record `index` starts at word index * NRC_*_WORDS.
//
// NrcRadianceParams, 56 bytes:
//   0..3 encodedPosition, 3 roughness, 4..6 normal, 6..8 viewDirection,
//   8..11 albedo, 11..14 specular
const NRC_RADIANCE_PARAMS_WORDS: u32 = 14u;
const NRC_RADIANCE_PARAMS_ENCODED_POSITION: u32 = 0u;
const NRC_RADIANCE_PARAMS_ROUGHNESS: u32 = 3u;
const NRC_RADIANCE_PARAMS_NORMAL: u32 = 4u;
const NRC_RADIANCE_PARAMS_VIEW_DIRECTION: u32 = 6u;
const NRC_RADIANCE_PARAMS_ALBEDO: u32 = 8u;
const NRC_RADIANCE_PARAMS_SPECULAR: u32 = 11u;
// NrcPackedPathVertex, 48 bytes: 0..7 data, 7 pad0, 8..11 encodedPosition, 11 pad1
const NRC_PACKED_PATH_VERTEX_WORDS: u32 = 12u;
const NRC_PACKED_PATH_VERTEX_DATA: u32 = 0u;
const NRC_PACKED_PATH_VERTEX_PAD0: u32 = 7u;
const NRC_PACKED_PATH_VERTEX_ENCODED_POSITION: u32 = 8u;
const NRC_PACKED_PATH_VERTEX_PAD1: u32 = 11u;
// NrcPackedQueryPathInfo, 8 bytes: 0 prefixThroughput (LogLuv), 1 queryBufferIndex
const NRC_PACKED_QUERY_PATH_INFO_WORDS: u32 = 2u;
const NRC_PACKED_QUERY_PATH_INFO_PREFIX_THROUGHPUT: u32 = 0u;
const NRC_PACKED_QUERY_PATH_INFO_QUERY_BUFFER_INDEX: u32 = 1u;
// NrcPackedTrainingPathInfo, 8 bytes: 0 packedData, 1 queryBufferIndex
const NRC_PACKED_TRAINING_PATH_INFO_WORDS: u32 = 2u;
const NRC_PACKED_TRAINING_PATH_INFO_PACKED_DATA: u32 = 0u;
const NRC_PACKED_TRAINING_PATH_INFO_QUERY_BUFFER_INDEX: u32 = 1u;
// Query radiance (the library's output): float3, 12 bytes
const NRC_QUERY_RADIANCE_WORDS: u32 = 3u;

// -------------------------------------------------------------------------
// Packing helpers (NrcHelpers.hlsli)
// -------------------------------------------------------------------------

// HLSL f32tof16: the half in the low 16 bits.
fn nrc_f32_to_f16(v: f32) -> u32 {
    return pack2x16float(vec2<f32>(v, 0.0)) & 0xffffu;
}

// HLSL f16tof32: the half in the low 16 bits (the high bits are ignored).
fn nrc_f16_to_f32(v: u32) -> f32 {
    return unpack2x16float(v & 0xffffu).x;
}

// NrcPackUnorm16Unsafe
fn nrc_pack_unorm16_unsafe(v: f32) -> u32 {
    return u32(trunc(v * 65535.0 + 0.5));
}

// NrcPackUnorm16 (isnan(v) through the bit test, see nrc_is_nan)
fn nrc_pack_unorm16(v_in: f32) -> u32 {
    let v = select(saturate(v_in), 0.0, nrc_is_nan(v_in));
    return nrc_pack_unorm16_unsafe(v);
}

// NrcPackUnorm2x16
fn nrc_pack_unorm2x16(v: vec2<f32>) -> u32 {
    return (nrc_pack_unorm16(v.y) << 16u) | nrc_pack_unorm16(v.x);
}

// NrcUnpackUnorm2x16. 1/65535 as the f32 constant HLSL folds (1.f / 65535),
// written as an exact hex float (bits 0x37800080: 2^-16 * (1 + 2^-16)) so no
// rounding of a decimal literal can change it.
const NRC_ONE_OVER_65535: f32 = 0x1.0001p-16f;
fn nrc_unpack_unorm2x16(packed: u32) -> vec2<f32> {
    return vec2<f32>(f32(packed & 0xffffu), f32(packed >> 16u)) * NRC_ONE_OVER_65535;
}

// NrcFloatToSnorm16 (isnan(v) through the bit test)
fn nrc_float_to_snorm16(v_in: f32) -> i32 {
    let v = select(min(max(v_in, -1.0), 1.0), 0.0, nrc_is_nan(v_in));
    return i32(trunc(v * 32767.0 + select(-0.5, 0.5, v >= 0.0)));
}

// NrcPackSnorm2x16. (HLSL marks v precise; WGSL has no equivalent, and the
// function does no arithmetic on v before nrc_float_to_snorm16.)
fn nrc_pack_snorm2x16(v: vec2<f32>) -> u32 {
    return (bitcast<u32>(nrc_float_to_snorm16(v.x)) & 0x0000ffffu)
        | (bitcast<u32>(nrc_float_to_snorm16(v.y)) << 16u);
}

// NrcUnpackSnorm2x16
fn nrc_unpack_snorm2x16(packed: u32) -> vec2<f32> {
    let bits = vec2<i32>(bitcast<i32>(packed << 16u), bitcast<i32>(packed)) >> vec2<u32>(16u);
    let unpacked = max(vec2<f32>(bits) / 32767.0, vec2<f32>(-1.0));
    return unpacked;
}

// NrcOctWrap
fn nrc_oct_wrap(v: vec2<f32>) -> vec2<f32> {
    return vec2<f32>(
        (1.0 - abs(v.y)) * select(-1.0, 1.0, v.x >= 0.0),
        (1.0 - abs(v.x)) * select(-1.0, 1.0, v.y >= 0.0),
    );
}

// NrcOctToDirection
fn nrc_oct_to_direction(p: vec2<f32>) -> vec3<f32> {
    var n = vec3<f32>(p.x, p.y, 1.0 - abs(p.x) - abs(p.y));
    let tmp = select(vec2<f32>(n.x, n.y), nrc_oct_wrap(vec2<f32>(n.x, n.y)), n.z < 0.0);
    n.x = tmp.x;
    n.y = tmp.y;
    return normalize(n);
}

// NrcDirectionToOct
fn nrc_direction_to_oct(n: vec3<f32>) -> vec2<f32> {
    // Project the sphere onto the octahedron (|x|+|y|+|z| = 1) and then onto the xy-plane.
    var p = n.xy * (1.0 / (abs(n.x) + abs(n.y) + abs(n.z)));
    p = select(p, nrc_oct_wrap(p), n.z < 0.0);
    return p;
}

// NrcEncodeNormal2x16
fn nrc_encode_normal2x16(normal: vec3<f32>) -> u32 {
    let oct_normal = nrc_direction_to_oct(normal);
    return nrc_pack_snorm2x16(oct_normal);
}

// NrcDecodeNormal2x16
fn nrc_decode_normal2x16(packed_normal: u32) -> vec3<f32> {
    let oct_normal = nrc_unpack_snorm2x16(packed_normal);
    return nrc_oct_to_direction(oct_normal);
}

// NrcUnpackR11G11B10
fn nrc_unpack_r11g11b10(packed: u32) -> vec3<f32> {
    let r = nrc_f16_to_f32((packed << 4u) & 0x7FF0u);
    let g = nrc_f16_to_f32((packed >> 7u) & 0x7FF0u);
    let b = nrc_f16_to_f32((packed >> 17u) & 0x7FE0u);
    return vec3<f32>(r, g, b);
}

// NrcPackR11G11B10
fn nrc_pack_r11g11b10(v_in: vec3<f32>) -> u32 {
    // Clamp upper bound so that it doesn't accidentally round up to INF
    let v = min(v_in, vec3<f32>(bitcast<f32>(0x477C0000u)));
    // Exponent=15, Mantissa=1.11111
    let r = ((nrc_f32_to_f16(v.x) + 8u) >> 4u) & 0x000007ffu;
    let g = ((nrc_f32_to_f16(v.y) + 8u) << 7u) & 0x003ff800u;
    let b = ((nrc_f32_to_f16(v.z) + 16u) << 17u) & 0xffc00000u;
    return r | g | b;
}

// NrcRgbToXyzRec709: mul(M, c) with M's rows below.
const NRC_RGB_TO_XYZ_REC709_ROW0 = vec3<f32>(0.4123907992659595, 0.3575843393838780, 0.1804807884018343);
const NRC_RGB_TO_XYZ_REC709_ROW1 = vec3<f32>(0.2126390058715104, 0.7151686787677559, 0.0721923153607337);
const NRC_RGB_TO_XYZ_REC709_ROW2 = vec3<f32>(0.0193308187155918, 0.1191947797946259, 0.9505321522496608);
fn nrc_rgb_to_xyz_rec709(c: vec3<f32>) -> vec3<f32> {
    return vec3<f32>(
        dot(NRC_RGB_TO_XYZ_REC709_ROW0, c),
        dot(NRC_RGB_TO_XYZ_REC709_ROW1, c),
        dot(NRC_RGB_TO_XYZ_REC709_ROW2, c),
    );
}

// NrcXyzToRgbRec709: mul(M, c) with M's rows below.
const NRC_XYZ_TO_RGB_REC709_ROW0 = vec3<f32>(3.240969941904522, -1.537383177570094, -0.4986107602930032);
const NRC_XYZ_TO_RGB_REC709_ROW1 = vec3<f32>(-0.9692436362808803, 1.875967501507721, 0.04155505740717569);
const NRC_XYZ_TO_RGB_REC709_ROW2 = vec3<f32>(0.05563007969699373, -0.2039769588889765, 1.056971514242878);
fn nrc_xyz_to_rgb_rec709(c: vec3<f32>) -> vec3<f32> {
    return vec3<f32>(
        dot(NRC_XYZ_TO_RGB_REC709_ROW0, c),
        dot(NRC_XYZ_TO_RGB_REC709_ROW1, c),
        dot(NRC_XYZ_TO_RGB_REC709_ROW2, c),
    );
}

// NrcEncodeLogLuvHdr
// Log-luminance in 14 bits (bits 18..31), chroma u in 9 bits (9..17), v in
// 9 bits (0..8). Black is exactly 0.
fn nrc_encode_log_luv_hdr(color: vec3<f32>) -> u32 {
    // Convert RGB to XYZ.
    let xyz = nrc_rgb_to_xyz_rec709(color);

    // Encode log2(Y) over the range [-20,20) in 14 bits (no sign bit).
    let log_y = 409.6 * (log2(xyz.y) + 20.0); // -inf if Y==0
    let le = u32(clamp(log_y, 0.0, 16383.0));

    // Early out if zero luminance to avoid NaN in chroma computation.
    // Note Le==0 if Y < 9.55e-7. We'll decode that as exactly zero.
    if le == 0u {
        return 0u;
    }

    //  u = 4X / (-2X + 12Y + 3(X + Y + Z))
    //  v = 9Y / (-2X + 12Y + 3(X + Y + Z))
    let inv_denom = 1.0 / (-2.0 * xyz.x + 12.0 * xyz.y + 3.0 * (xyz.x + xyz.y + xyz.z));
    let uv = vec2<f32>(4.0, 9.0) * xyz.xy * inv_denom;

    // Encode chroma (u,v) in 9 bits each.
    let uve = vec2<u32>(clamp(820.0 * uv, vec2<f32>(0.0), vec2<f32>(511.0)));

    return (le << 18u) | (uve.x << 9u) | uve.y;
}

// NrcDecodeLogLuvHdr
fn nrc_decode_log_luv_hdr(packed_color: u32) -> vec3<f32> {
    // Decode luminance Y from encoded log-luminance.
    let le = packed_color >> 18u;
    if le == 0u {
        return vec3<f32>(0.0, 0.0, 0.0);
    }

    let log_y = (f32(le) + 0.5) / 409.6 - 20.0;
    let y = pow(2.0, log_y);

    //  x = 9u / (6u - 16v + 12)
    //  y = 4v / (6u - 16v + 12)
    let uve = vec2<u32>(packed_color >> 9u, packed_color) & vec2<u32>(0x1ffu);
    let uv = (vec2<f32>(uve) + 0.5) / 820.0;

    let inv_denom = 1.0 / (6.0 * uv.x - 16.0 * uv.y + 12.0);
    let xy = vec2<f32>(9.0, 4.0) * uv * inv_denom;

    //  X = Y / y * x
    //  Z = Y / y * (1 - x - y)
    let s = y / xy.y;
    let xyz = vec3<f32>(s * xy.x, y, s * (1.0 - xy.x - xy.y));

    // Convert back to RGB and clamp to avoid out-of-gamut colors.
    return max(nrc_xyz_to_rgb_rec709(xyz), vec3<f32>(0.0));
}

// NrcCartesianToSphericalUnorm. (Like the HLSL, no clamp before acos: a
// normalized z that rounds past 1 gives NaN.)
const NRC_ONE_OVER_PI: f32 = 0.318309886183790671538; // 1/pi
const NRC_ONE_OVER_TWO_PI: f32 = 0.159154943091895335769; // 1/2pi
fn nrc_cartesian_to_spherical_unorm(p_in: vec3<f32>) -> vec2<f32> {
    let p = normalize(p_in);
    var sph: vec2<f32>;
    sph.x = acos(p.z) * NRC_ONE_OVER_PI;
    sph.y = atan2(-p.y, -p.x) * NRC_ONE_OVER_TWO_PI + 0.5;
    return sph;
}

// NrcSafeCartesianToSphericalUnorm (isfinite through the bit test)
fn nrc_safe_cartesian_to_spherical_unorm(p: vec3<f32>) -> vec2<f32> {
    let exponent = bitcast<vec3<u32>>(p) & vec3<u32>(0x7F800000u);
    if any(exponent == vec3<u32>(0x7F800000u)) {
        return vec2<f32>(0.0, 0.0);
    }
    return nrc_cartesian_to_spherical_unorm(p);
}

// NrcIsNan(float)
fn nrc_is_nan(x: f32) -> bool {
    return (bitcast<u32>(x) & 0x7FFFFFFFu) > 0x7F800000u;
}

// NrcIsInf(float)
fn nrc_is_inf(x: f32) -> bool {
    return (bitcast<u32>(x) & 0x7FFFFFFFu) == 0x7F800000u;
}

// NrcIsNan(float3)
fn nrc_is_nan3(v: vec3<f32>) -> bool {
    return nrc_is_nan(v.x) || nrc_is_nan(v.y) || nrc_is_nan(v.z);
}

// NrcIsInf(float3)
fn nrc_is_inf3(v: vec3<f32>) -> bool {
    return nrc_is_inf(v.x) || nrc_is_inf(v.y) || nrc_is_inf(v.z);
}

// NrcSanitizeNansInfs
fn nrc_sanitize_nans_infs(input: vec3<f32>) -> vec3<f32> {
    if nrc_is_inf3(input) || nrc_is_nan3(input) {
        return vec3<f32>(0.0, 0.0, 0.0);
    }
    return input;
}

// NrcCalculateQueryPathIndex
fn nrc_calculate_query_path_index(frame_dimensions: vec2<u32>, pixel: vec2<u32>, sample_index: u32, samples_per_pixel: u32) -> u32 {
    // Index in a linearized Tensor: (height, width, spp)
    var index = sample_index;
    var stride = samples_per_pixel;
    index += pixel.x * stride;
    stride *= frame_dimensions.x;
    index += pixel.y * stride;
    return index;
}

// NrcCalculateTrainingPathIndex
fn nrc_calculate_training_path_index(training_dimensions: vec2<u32>, pixel: vec2<u32>) -> u32 {
    return (training_dimensions.x * pixel.y) + pixel.x;
}

// NrcCalculateTrainingPathVertexIndex
fn nrc_calculate_training_path_vertex_index(training_dimensions: vec2<u32>, pixel: vec2<u32>, vertex_idx: u32, max_path_vertices: u32) -> u32 {
    let training_path_index = nrc_calculate_training_path_index(training_dimensions, pixel);
    return training_path_index * max_path_vertices + vertex_idx;
}

// NrcUpdateTrainingPathVertex
fn nrc_update_training_path_vertex(packed_path_vertex: NrcPackedPathVertex, radiance: vec3<f32>, throughput: vec3<f32>) -> NrcPackedPathVertex {
    var v = packed_path_vertex;
    v.data[0] = nrc_encode_log_luv_hdr(nrc_sanitize_nans_infs(radiance));
    v.data[1] = nrc_encode_log_luv_hdr(nrc_sanitize_nans_infs(throughput));
    return v;
}

// NrcUnpackPathVertex(packed, radiance, throughput)
fn nrc_unpack_path_vertex_with(packed: NrcPackedPathVertex, radiance: vec3<f32>, throughput: vec3<f32>) -> NrcPathVertex {
    var vertex: NrcPathVertex;
    vertex.radiance = nrc_sanitize_nans_infs(radiance);
    vertex.throughput = nrc_sanitize_nans_infs(throughput);
    vertex.linear_roughness = bitcast<f32>(packed.data[2]);
    vertex.normal = nrc_decode_normal2x16(packed.data[3]);
    vertex.view_direction = nrc_decode_normal2x16(packed.data[4]);
    vertex.albedo = nrc_unpack_r11g11b10(packed.data[5]);
    vertex.specular = nrc_unpack_r11g11b10(packed.data[6]);
    vertex.encoded_position = packed.encoded_position;
    return vertex;
}

// NrcUnpackPathVertex(packed)
fn nrc_unpack_path_vertex(packed: NrcPackedPathVertex) -> NrcPathVertex {
    var vertex: NrcPathVertex;
    vertex.radiance = nrc_decode_log_luv_hdr(packed.data[0]);
    vertex.throughput = nrc_decode_log_luv_hdr(packed.data[1]);
    vertex.linear_roughness = bitcast<f32>(packed.data[2]);
    vertex.normal = nrc_decode_normal2x16(packed.data[3]);
    vertex.view_direction = nrc_decode_normal2x16(packed.data[4]);
    vertex.albedo = nrc_unpack_r11g11b10(packed.data[5]);
    vertex.specular = nrc_unpack_r11g11b10(packed.data[6]);
    vertex.encoded_position = packed.encoded_position;
    return vertex;
}

// NrcEncodePosition(worldSpacePosition, nrcConstants)
// (TCNN_USES_FIXED_POINT_POSITIONS 0: HLSL mad becomes fma.)
fn nrc_encode_position(world_space_position: vec3<f32>, nrc_constants: NrcConstants) -> vec3<f32> {
    return fma(world_space_position, nrc_constants.scene_pos_scale, nrc_constants.scene_pos_bias);
}

// NrcEncodePosition(localPositionOffset, localOrigin, nrcConstants)
fn nrc_encode_position_local(local_position_offset: vec3<f32>, local_origin: vec3<f32>, nrc_constants: NrcConstants) -> vec3<f32> {
    return fma(local_origin + local_position_offset, nrc_constants.scene_pos_scale, nrc_constants.scene_pos_bias);
}

// NrcCreateRadianceParams
fn nrc_create_radiance_params(vertex: NrcPathVertex) -> NrcRadianceParams {
    var params: NrcRadianceParams;
    params.encoded_position = vertex.encoded_position;
    params.roughness = vertex.linear_roughness;
    params.normal = nrc_safe_cartesian_to_spherical_unorm(vertex.normal);
    params.view_direction = nrc_safe_cartesian_to_spherical_unorm(vertex.view_direction);
    params.albedo = vertex.albedo;
    params.specular = vertex.specular;
    return params;
}

// NrcInitializePackedPathVertex
fn nrc_initialize_packed_path_vertex(
    linear_roughness: f32,
    shading_normal: vec3<f32>,
    view_direction: vec3<f32>,
    diffuse_albedo: vec3<f32>,
    specular_albedo: vec3<f32>,
    encoded_position: vec3<f32>,
) -> NrcPackedPathVertex {
    var v: NrcPackedPathVertex;
    v.data[0] = nrc_encode_log_luv_hdr(vec3<f32>(0.0, 0.0, 0.0));
    v.data[1] = nrc_encode_log_luv_hdr(vec3<f32>(1.0, 1.0, 1.0));
    v.data[2] = bitcast<u32>(linear_roughness);
    v.data[3] = nrc_encode_normal2x16(shading_normal);
    v.data[4] = nrc_encode_normal2x16(view_direction);
    v.data[5] = nrc_pack_r11g11b10(diffuse_albedo);
    v.data[6] = nrc_pack_r11g11b10(specular_albedo);
    v.encoded_position = encoded_position;
    v.pad0 = 0u;
    v.pad1 = 0u;
    return v;
}

// NrcPackTrainingPathInfo
fn nrc_pack_training_path_info(path_info: NrcTrainingPathInfo) -> NrcPackedTrainingPathInfo {
    var packed_path_info = NrcPackedTrainingPathInfo(0u, 0u);
    packed_path_info.packed_data = path_info.packed_data;
    packed_path_info.query_buffer_index = path_info.query_buffer_index;
    return packed_path_info;
}

// NrcUnpackTrainingPathInfo
fn nrc_unpack_training_path_info(packed_path_info: NrcPackedTrainingPathInfo) -> NrcTrainingPathInfo {
    var path_info = NrcTrainingPathInfo(0u, 0u);
    path_info.packed_data = packed_path_info.packed_data;
    path_info.query_buffer_index = packed_path_info.query_buffer_index;
    return path_info;
}

// NrcUnpackQueryPathInfo
fn nrc_unpack_query_path_info(packed_path_info: NrcPackedQueryPathInfo) -> NrcQueryPathInfo {
    var path_info = NrcQueryPathInfo(vec3<f32>(0.0), 0u);
    path_info.prefix_throughput = nrc_decode_log_luv_hdr(packed_path_info.prefix_throughput);
    path_info.query_buffer_index = packed_path_info.query_buffer_index;
    return path_info;
}

// NrcUnpackQueryRadiance
fn nrc_unpack_query_radiance(nrc_constants: NrcConstants, packed_query_radiance: vec3<f32>) -> vec3<f32> {
    return packed_query_radiance * nrc_constants.radiance_unpack_multiplier;
}

// -------------------------------------------------------------------------
// packedData and the termination heuristic (Nrc.hlsli)
// -------------------------------------------------------------------------
//   +-----------+-------------+--------------+
//   | 15     12 | 11        7 | 6          0 |
//   |   Flags   | Termination |    Vertex    |
//   |           |   Reason    |     Count    |
//   +-----------+-------------+--------------+

const NRC_TERMINATION_REASON_SHIFT: u32 = 7u;
const NRC_PATH_FLAGS_SHIFT: u32 = 12u;
// Training paths that exited the scene should not be "Q-learned"
const NRC_PATH_FLAG_HAS_EXITED_SCENE: u32 = 1u << (NRC_PATH_FLAGS_SHIFT + 0u);
// Some of the training paths are marked as "unbiased" to be extended through the entire scene
const NRC_PATH_FLAG_IS_UNBIASED: u32 = 1u << (NRC_PATH_FLAGS_SHIFT + 1u);
const NRC_PATH_FLAG_PREVIOUS_HIT_WAS_DELTA_LOBE: u32 = 1u << (NRC_PATH_FLAGS_SHIFT + 2u);
const NRC_PATH_FLAG_HEURISTIC_RESET: u32 = 1u << (NRC_PATH_FLAGS_SHIFT + 3u);
const NRC_VERTEX_COUNT_MASK: u32 = (1u << NRC_TERMINATION_REASON_SHIFT) - 1u;
const NRC_TERMINATION_REASON_MASK: u32 = ((1u << NRC_PATH_FLAGS_SHIFT) - 1u) & ~NRC_VERTEX_COUNT_MASK;

// The HLSL setters take packedData inout; these return the new value.

// NrcEvaluateTerminationHeuristic
fn nrc_evaluate_termination_heuristic(path_state: NrcPathState, threshold: f32) -> bool {
    return (path_state.primary_spread_radius > 0.0)
        && (path_state.cumul_spread_radius > (threshold * path_state.primary_spread_radius));
}

// NrcSetFlag(packedData, flag)
fn nrc_set_flag(packed_data: u32, flag: u32) -> u32 {
    return packed_data | flag;
}

// NrcClearFlag
fn nrc_clear_flag(packed_data: u32, flag: u32) -> u32 {
    return packed_data & ~flag;
}

// NrcSetFlag(packedData, flag, value)
fn nrc_set_flag_value(packed_data: u32, flag: u32, value: bool) -> u32 {
    var p = packed_data & ~flag;
    p |= select(0u, flag, value);
    return p;
}

// NrcGetFlag
fn nrc_get_flag(packed_data: u32, flag: u32) -> bool {
    return (packed_data & flag) != 0u;
}

// NrcGetVertexCount
fn nrc_get_vertex_count(packed_data: u32) -> u32 {
    return packed_data & NRC_VERTEX_COUNT_MASK;
}

// NrcSetVertexCount. (Like the HLSL, a count above 127 spills into the
// termination reason bits.)
fn nrc_set_vertex_count(packed_data: u32, vertex_count: u32) -> u32 {
    var p = packed_data & ~NRC_VERTEX_COUNT_MASK;
    p |= vertex_count;
    return p;
}

// NrcSetDebugPathTerminationReason (on pathState.packedData)
fn nrc_set_debug_path_termination_reason(packed_data: u32, reason: u32) -> u32 {
    var p = packed_data & ~NRC_TERMINATION_REASON_MASK;
    p |= reason << NRC_TERMINATION_REASON_SHIFT;
    return p;
}

// NrcGetDebugPathTerminationReason (on pathState.packedData)
fn nrc_get_debug_path_termination_reason(packed_data: u32) -> u32 {
    return (packed_data & NRC_TERMINATION_REASON_MASK) >> NRC_TERMINATION_REASON_SHIFT;
}
