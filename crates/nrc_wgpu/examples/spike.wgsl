// examples/spike.rs: a synthetic scene with a known answer for NRC.
//
// The "world" has outgoing irradiance that depends only on position,
// c(p) = 0.2 + 0.6 * p.x / 10 (function 0) or 0.8 - 0.6 * p.x / 10
// (function 1), equal in r, g, b; every surface has albedo 1, specular 0,
// roughness 1. Scene bounds are 0..10 on each axis.
//
// train_fill writes what NrcUpdateOnHit (update mode) and
// NrcWriteFinalPathInfo write for a path of `params.vertices` vertices, one
// path per thread at the training resolution. Every vertex is finalized with
// radiance c(p_i) and throughput 0, so each vertex's training target is its
// own c(p_i). query_fill writes what NrcUpdateOnHit (query mode, query at
// vertex 0) and NrcWriteFinalPathInfo write, one query per pixel. gather
// picks the library's answers for a fixed sample of 4,096 queries.
//
// Preceded by nrc_records.wgsl and nrc_buffers.wgsl (crates/nrc_records).

struct SpikeParams {
    frame: u32,
    vertices: u32,
    exit_fraction: f32,
    function: u32,
    // 1: the samples are training paths' self-training queries (at the last
    // vertex's position); 0: pixel queries
    sample_from_training: u32,
    sample_stride: u32,
    sample_count: u32,
    // 1: the pixel queries keep the same positions, normals and view
    // directions every frame (to measure frame-to-frame change)
    static_queries: u32,
}

@group(0) @binding(0) var<uniform> constants: NrcConstants;
@group(0) @binding(1) var<uniform> params: SpikeParams;
@group(0) @binding(2) var<storage, read_write> nrc_query_path_info: array<u32>;
@group(0) @binding(3) var<storage, read_write> nrc_training_path_info: array<u32>;
@group(0) @binding(4) var<storage, read_write> nrc_training_path_vertices: array<u32>;
@group(0) @binding(5) var<storage, read_write> nrc_query_radiance_params: array<u32>;
@group(0) @binding(6) var<storage, read_write> nrc_counters: array<atomic<u32>>;
@group(0) @binding(7) var<storage, read> nrc_query_radiance: array<f32>;
// Per sample, 4 words: position x, y, z (f32 bits) and the query index.
@group(0) @binding(8) var<storage, read_write> samples: array<u32>;
// Per sample, 4 words: the packed query radiance r, g, b and position x.
@group(0) @binding(9) var<storage, read_write> results: array<f32>;

const NO_QUERY: u32 = 0xffffffffu;
const SCENE_SIZE: f32 = 10.0;

// PCG hash (Jarzynski and Olano)
fn pcg(v: u32) -> u32 {
    let state = v * 747796405u + 2891336453u;
    let word = ((state >> ((state >> 28u) + 4u)) ^ state) * 277803737u;
    return (word >> 22u) ^ word;
}

var<private> rng_state: u32;

fn seed(thread: u32, frame: u32, stream: u32) {
    rng_state = pcg(thread ^ pcg(frame * 4u + stream + 0x9e3779b9u));
}

// [0, 1)
fn rand() -> f32 {
    rng_state = pcg(rng_state);
    return f32(rng_state >> 8u) * (1.0 / 16777216.0);
}

fn random_position() -> vec3<f32> {
    return vec3<f32>(rand(), rand(), rand()) * SCENE_SIZE;
}

// Uniform on the sphere. z is kept off the poles so that normalize() in
// NrcCartesianToSphericalUnorm cannot round |z| past 1 (acos would give NaN).
fn random_direction() -> vec3<f32> {
    let z = clamp(1.0 - 2.0 * rand(), -0.9999, 0.9999);
    let phi = 6.283185307 * rand();
    let r = sqrt(max(0.0, 1.0 - z * z));
    return vec3<f32>(r * cos(phi), r * sin(phi), z);
}

fn world_radiance(p: vec3<f32>) -> vec3<f32> {
    var c = 0.2 + 0.6 * p.x / SCENE_SIZE;
    if params.function == 1u {
        c = 0.8 - 0.6 * p.x / SCENE_SIZE;
    }
    return vec3<f32>(c);
}

fn write_sample(index: u32, p: vec3<f32>, query_index: u32) {
    if index % params.sample_stride != 0u {
        return;
    }
    let s = index / params.sample_stride;
    if s >= params.sample_count {
        return;
    }
    samples[s * 4u + 0u] = bitcast<u32>(p.x);
    samples[s * 4u + 1u] = bitcast<u32>(p.y);
    samples[s * 4u + 2u] = bitcast<u32>(p.z);
    samples[s * 4u + 3u] = query_index;
}

@compute @workgroup_size(8, 8)
fn train_fill(@builtin(global_invocation_id) id: vec3<u32>) {
    let dims = constants.training_dimensions;
    if id.x >= dims.x || id.y >= dims.y {
        return;
    }
    let pixel = id.xy;
    let path = nrc_calculate_training_path_index(dims, pixel);
    seed(path, params.frame, 1u);
    let exited = rand() < params.exit_fraction;

    // NrcUpdateOnHit per vertex: NrcInitializePackedPathVertex, then (at the
    // next hit, or in NrcWriteFinalPathInfo for the last one)
    // NrcUpdateTrainingPathVertex with the radiance and throughput gathered
    // after it. Throughput 0: the target is the vertex's own radiance.
    var last_vertex: NrcPackedPathVertex;
    var last_position = vec3<f32>(0.0);
    for (var i = 0u; i < params.vertices; i++) {
        let p = random_position();
        let normal = random_direction();
        let view = random_direction();
        var vertex = nrc_initialize_packed_path_vertex(
            1.0, normal, view, vec3<f32>(1.0), vec3<f32>(0.0), nrc_encode_position(p, constants));
        vertex = nrc_update_training_path_vertex(vertex, world_radiance(p), vec3<f32>(0.0));
        nrc_write_packed_path_vertex(
            nrc_calculate_training_path_vertex_index(dims, pixel, i, constants.max_path_vertices), vertex);
        last_vertex = vertex;
        last_position = p;
    }

    // NrcWriteFinalPathInfo, update mode
    var packed_data = nrc_set_vertex_count(0u, params.vertices);
    var query_index = NO_QUERY;
    if exited {
        // (NrcUpdateOnMiss sets the flag; no self-training query then)
        packed_data = nrc_set_flag(packed_data, NRC_PATH_FLAG_HAS_EXITED_SCENE);
    } else if params.vertices > 0u && constants.max_path_vertices > 1u {
        let vertex = nrc_unpack_path_vertex_with(last_vertex, world_radiance(last_position), vec3<f32>(0.0));
        query_index = nrc_increment_counter(NRC_COUNTER_QUERIES);
        nrc_write_radiance_params(query_index, nrc_create_radiance_params(vertex));
    }
    nrc_write_training_path_info(path, nrc_pack_training_path_info(NrcTrainingPathInfo(packed_data, query_index)));

    if params.sample_from_training != 0u {
        write_sample(path, last_position, query_index);
    }
}

// One pixel's query: NrcUpdateOnHit (query mode, querying at vertex 0, the
// termination heuristic off) and NrcWriteFinalPathInfo (query mode), with the
// query index already taken from the Queries counter.
fn write_query(pixel: vec2<u32>, query_index: u32) {
    let dims = constants.frame_dimensions;
    let pixel_index = pixel.y * dims.x + pixel.x;
    seed(pixel_index, select(params.frame, 0u, params.static_queries != 0u), 2u);
    let p = random_position();
    let normal = random_direction();
    let view = random_direction();

    var radiance_params: NrcRadianceParams;
    radiance_params.encoded_position = nrc_encode_position(p, constants);
    radiance_params.roughness = 1.0;
    radiance_params.normal = nrc_safe_cartesian_to_spherical_unorm(normal);
    radiance_params.view_direction = nrc_safe_cartesian_to_spherical_unorm(view);
    radiance_params.albedo = vec3<f32>(1.0);
    radiance_params.specular = vec3<f32>(0.0);
    nrc_write_radiance_params(query_index, radiance_params);

    // learnIrradiance: the prefix throughput is throughput * (specular + albedo) = 1.
    let path_index = nrc_calculate_query_path_index(dims, pixel, 0u, constants.samples_per_pixel);
    nrc_write_query_path_info(path_index, NrcPackedQueryPathInfo(nrc_encode_log_luv_hdr(vec3<f32>(1.0)), query_index));

    if params.sample_from_training == 0u {
        write_sample(pixel_index, p, query_index);
    }
}

// One atomicAdd on the global counter per thread (nrc_increment_counter, the
// HLSL's reference #else branch).
@compute @workgroup_size(8, 8)
fn query_fill(@builtin(global_invocation_id) id: vec3<u32>) {
    let dims = constants.frame_dimensions;
    if id.x >= dims.x || id.y >= dims.y {
        return;
    }
    write_query(id.xy, nrc_increment_counter(NRC_COUNTER_QUERIES));
}

// One atomicAdd on the global counter per workgroup, as the HLSL's default
// NrcIncrementCounter does per wave (same set of indices, different order).
var<workgroup> group_count: atomic<u32>;
var<workgroup> group_base: u32;

@compute @workgroup_size(8, 8)
fn query_fill_grouped(@builtin(global_invocation_id) id: vec3<u32>, @builtin(local_invocation_index) local: u32) {
    let dims = constants.frame_dimensions;
    if local == 0u {
        atomicStore(&group_count, 0u);
    }
    workgroupBarrier();
    let in_frame = id.x < dims.x && id.y < dims.y;
    var offset = 0u;
    if in_frame {
        offset = atomicAdd(&group_count, 1u);
    }
    workgroupBarrier();
    if local == 0u {
        group_base = atomicAdd(&nrc_counters[NRC_COUNTER_QUERIES], atomicLoad(&group_count));
    }
    workgroupBarrier();
    if in_frame {
        write_query(id.xy, group_base + offset);
    }
}

@compute @workgroup_size(64)
fn gather(@builtin(global_invocation_id) id: vec3<u32>) {
    let s = id.x;
    if s >= params.sample_count {
        return;
    }
    let query_index = samples[s * 4u + 3u];
    var radiance = vec3<f32>(-1.0);
    if query_index != NO_QUERY {
        radiance = nrc_read_query_radiance(query_index);
    }
    results[s * 4u + 0u] = radiance.x;
    results[s * 4u + 1u] = radiance.y;
    results[s * 4u + 2u] = radiance.z;
    results[s * 4u + 3u] = bitcast<f32>(samples[s * 4u + 0u]);
}
