// tests/gpu_roundtrip.rs: applies every function of nrc_records.wgsl and
// nrc_buffers.wgsl to one input case per invocation and writes the results in
// a fixed order; the host reads them back in the same order (see `check_case`)
// and compares with the Rust reference.

@group(0) @binding(0) var<uniform> constants: NrcConstants;
@group(0) @binding(1) var<storage, read> inputs: array<u32>;
@group(0) @binding(2) var<storage, read_write> outputs: array<u32>;
@group(0) @binding(3) var<storage, read_write> nrc_query_path_info: array<u32>;
@group(0) @binding(4) var<storage, read_write> nrc_training_path_info: array<u32>;
@group(0) @binding(5) var<storage, read_write> nrc_training_path_vertices: array<u32>;
@group(0) @binding(6) var<storage, read_write> nrc_query_radiance_params: array<u32>;
@group(0) @binding(7) var<storage, read_write> nrc_counters: array<atomic<u32>>;
@group(0) @binding(8) var<storage, read> nrc_query_radiance: array<f32>;

const IN_WORDS: u32 = 40u;
const OUT_WORDS: u32 = 160u;

var<private> cursor: u32;
var<private> in_base: u32;

fn put_u(x: u32) {
    outputs[cursor] = x;
    cursor += 1u;
}
fn put_b(x: bool) {
    put_u(select(0u, 1u, x));
}
fn put_f(x: f32) {
    put_u(bitcast<u32>(x));
}
fn put_v2(x: vec2<f32>) {
    put_f(x.x);
    put_f(x.y);
}
fn put_v3(x: vec3<f32>) {
    put_f(x.x);
    put_f(x.y);
    put_f(x.z);
}
fn in_u(k: u32) -> u32 {
    return inputs[in_base + k];
}
fn in_f(k: u32) -> f32 {
    return bitcast<f32>(in_u(k));
}
fn in_v2(k: u32) -> vec2<f32> {
    return vec2<f32>(in_f(k), in_f(k + 1u));
}
fn in_v3(k: u32) -> vec3<f32> {
    return vec3<f32>(in_f(k), in_f(k + 1u), in_f(k + 2u));
}

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    let count = arrayLength(&inputs) / IN_WORDS;
    let i = id.x;
    if i >= count {
        return;
    }
    in_base = i * IN_WORDS;
    cursor = i * OUT_WORDS;

    let color = in_v3(0u);
    let s = in_f(3u);
    let n = in_v3(4u);
    let v = in_v3(7u);
    let world = in_v3(10u);
    let rough = in_f(13u);
    let a = in_u(14u);
    let b = in_u(15u);
    let albedo = in_v3(16u);
    let specular = in_v3(19u);
    let t = in_f(22u);
    let s2 = in_f(23u);
    let dirty = in_v3(24u);
    let primary = in_f(27u);
    let cumul = in_f(28u);
    let threshold = in_f(29u);
    let c = in_u(30u);
    let d = in_u(31u);
    let p = in_v2(32u);
    let local = in_v3(34u);
    let e = in_u(37u);

    // unorm / snorm
    put_u(nrc_pack_unorm16(s));
    put_u(nrc_pack_unorm16_unsafe(t));
    put_u(nrc_pack_unorm2x16(vec2<f32>(s, s2)));
    put_v2(nrc_unpack_unorm2x16(a));
    put_u(bitcast<u32>(nrc_float_to_snorm16(s)));
    put_u(nrc_pack_snorm2x16(vec2<f32>(s, s2)));
    put_v2(nrc_unpack_snorm2x16(a));
    // octahedral
    put_v2(nrc_oct_wrap(p));
    put_v3(nrc_oct_to_direction(p));
    put_v2(nrc_direction_to_oct(n));
    put_u(nrc_encode_normal2x16(n));
    put_v3(nrc_decode_normal2x16(a));
    // f16 and R11G11B10
    put_u(nrc_f32_to_f16(min(albedo.x, 65504.0))); // pack2x16float is indeterminate past the f16 range
    put_u(nrc_f32_to_f16(min(albedo.y, 65504.0))); // pack2x16float is indeterminate past the f16 range
    put_u(nrc_f32_to_f16(min(albedo.z, 65504.0))); // pack2x16float is indeterminate past the f16 range
    put_u(nrc_pack_r11g11b10(albedo));
    put_v3(nrc_unpack_r11g11b10(a));
    // color spaces and LogLuv
    put_v3(nrc_rgb_to_xyz_rec709(color));
    put_v3(nrc_xyz_to_rgb_rec709(color));
    put_u(nrc_encode_log_luv_hdr(color));
    // nrc_encode_log_luv_hdr's own intermediates, the same expressions
    let xyz = nrc_rgb_to_xyz_rec709(color);
    let log_y = 409.6 * (log2(xyz.y) + 20.0);
    let inv_denom = 1.0 / (-2.0 * xyz.x + 12.0 * xyz.y + 3.0 * (xyz.x + xyz.y + xyz.z));
    let uv = vec2<f32>(4.0, 9.0) * xyz.xy * inv_denom;
    put_f(log_y);
    put_v2(uv);
    put_v3(nrc_decode_log_luv_hdr(a));
    // spherical
    put_v2(nrc_cartesian_to_spherical_unorm(n));
    put_v2(nrc_safe_cartesian_to_spherical_unorm(dirty));
    // NaN / Inf
    put_u(select(0u, 1u, nrc_is_nan(s)) | select(0u, 2u, nrc_is_inf(s))
        | select(0u, 4u, nrc_is_nan3(dirty)) | select(0u, 8u, nrc_is_inf3(dirty)));
    put_v3(nrc_sanitize_nans_infs(dirty));
    // indices: dimensions in c, pixel in d, sample/spp/vertex/max in e
    let dims = vec2<u32>(c & 0xffffu, c >> 16u);
    let pixel = vec2<u32>(d & 0xffffu, d >> 16u);
    put_u(nrc_calculate_query_path_index(dims, pixel, e & 3u, (e >> 2u) & 7u));
    put_u(nrc_calculate_training_path_index(dims, pixel));
    put_u(nrc_calculate_training_path_vertex_index(dims, pixel, (e >> 5u) & 7u, (e >> 8u) & 15u));
    // positions
    put_v3(nrc_encode_position(world, constants));
    put_v3(nrc_encode_position_local(local, world, constants));
    // packedData
    let flag = NRC_PATH_FLAG_HAS_EXITED_SCENE << ((e >> 12u) & 3u);
    let p1 = nrc_set_vertex_count(a & ~NRC_VERTEX_COUNT_MASK, (e >> 16u) & 0x7fu);
    let p2 = nrc_set_debug_path_termination_reason(p1, ((e >> 23u) & 0xffu) % 7u);
    let p3 = nrc_set_flag(p2, flag);
    let p4 = nrc_clear_flag(p3, NRC_PATH_FLAG_HAS_EXITED_SCENE << ((e >> 14u) & 3u));
    let p5 = nrc_set_flag_value(p4, flag, (b & 1u) == 1u);
    put_u(p1);
    put_u(p2);
    put_u(p3);
    put_u(p4);
    put_u(p5);
    put_u(nrc_get_vertex_count(a) | (nrc_get_debug_path_termination_reason(a) << 8u)
        | (select(0u, 1u, nrc_get_flag(a, NRC_PATH_FLAG_HAS_EXITED_SCENE)) << 16u)
        | (select(0u, 1u, nrc_get_flag(a, NRC_PATH_FLAG_IS_UNBIASED)) << 17u)
        | (select(0u, 1u, nrc_get_flag(a, NRC_PATH_FLAG_PREVIOUS_HIT_WAS_DELTA_LOBE)) << 18u)
        | (select(0u, 1u, nrc_get_flag(a, NRC_PATH_FLAG_HEURISTIC_RESET)) << 19u));
    var state: NrcPathState;
    state.primary_spread_radius = primary;
    state.cumul_spread_radius = cumul;
    put_b(nrc_evaluate_termination_heuristic(state, threshold));

    // ---- records through the buffers ----
    put_u(nrc_increment_counter(NRC_COUNTER_QUERIES));
    if (i & 1u) == 0u {
        _ = nrc_increment_counter(NRC_COUNTER_TRAINING_RECORDS);
    }

    let encoded = nrc_encode_position(world, constants);
    var vtx = nrc_initialize_packed_path_vertex(rough, n, v, albedo, specular, encoded);
    if (i & 1u) == 1u {
        vtx = nrc_update_training_path_vertex(vtx, color, dirty);
    }
    nrc_write_packed_path_vertex(i, vtx);
    let back = nrc_read_packed_path_vertex(i);
    for (var k = 0u; k < 7u; k++) {
        put_u(back.data[k]);
    }
    put_u(back.pad0);
    put_v3(back.encoded_position);
    put_u(back.pad1);
    // the direct encodings the record must hold
    put_u(nrc_encode_log_luv_hdr(nrc_sanitize_nans_infs(dirty)));
    put_u(nrc_encode_normal2x16(v));
    put_v2(nrc_direction_to_oct(v));

    var vertex: NrcPathVertex;
    if ((i >> 1u) & 1u) == 0u {
        vertex = nrc_unpack_path_vertex(back);
    } else {
        vertex = nrc_unpack_path_vertex_with(back, color, dirty);
    }
    put_v3(vertex.radiance);
    put_v3(vertex.throughput);
    put_v3(vertex.encoded_position);
    put_f(vertex.linear_roughness);
    put_v3(vertex.normal);
    put_v3(vertex.view_direction);
    put_v3(vertex.albedo);
    put_v3(vertex.specular);

    nrc_write_radiance_params(i, nrc_create_radiance_params(vertex));
    let params = nrc_read_radiance_params(i);
    put_v3(params.encoded_position);
    put_f(params.roughness);
    put_v2(params.normal);
    put_v2(params.view_direction);
    put_v3(params.albedo);
    put_v3(params.specular);

    nrc_write_query_path_info(i, NrcPackedQueryPathInfo(nrc_encode_log_luv_hdr(color), b));
    let query = nrc_read_query_path_info(i);
    put_u(query.prefix_throughput);
    put_u(query.query_buffer_index);
    let unpacked_query = nrc_unpack_query_path_info(query);
    put_v3(unpacked_query.prefix_throughput);
    put_u(unpacked_query.query_buffer_index);

    nrc_write_training_path_info(i, nrc_pack_training_path_info(NrcTrainingPathInfo(p5, a)));
    let training = nrc_unpack_training_path_info(nrc_read_training_path_info(i));
    put_u(training.packed_data);
    put_u(training.query_buffer_index);

    put_v3(nrc_unpack_query_radiance(constants, nrc_read_query_radiance(i)));

    // The constants as the uniform holds them, after the cases' outputs
    if i == 0u {
        cursor = count * OUT_WORDS;
        put_u(constants.frame_dimensions.x);
        put_u(constants.frame_dimensions.y);
        put_u(constants.training_dimensions.x);
        put_u(constants.training_dimensions.y);
        put_v3(constants.scene_pos_scale);
        put_u(constants.samples_per_pixel);
        put_v3(constants.scene_pos_bias);
        put_u(constants.max_path_vertices);
        put_u(constants.learn_irradiance);
        put_u(constants.radiance_cache_direct);
        put_f(constants.radiance_unpack_multiplier);
        put_u(constants.resolve_mode);
        put_u(constants.enable_termination_heuristic);
        put_u(constants.skip_delta_vertices);
        put_f(constants.termination_heuristic_threshold);
        put_f(constants.training_termination_heuristic_threshold);
        put_f(constants.proportion_unbiased);
        put_u(constants.pad0);
        put_u(constants.pad1);
        put_u(constants.pad2);
    }
}
