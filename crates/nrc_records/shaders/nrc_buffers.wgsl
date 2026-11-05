// Record read/write helpers over the NRC library's buffers (the
// NRC_BUFFER_* accessors of Nrc.hlsli), addressing the DX scalar layout by
// hand. Include after nrc_records.wgsl.
//
// WGSL (naga) does not accept storage-buffer pointers as function arguments,
// so these helpers cannot take the buffer as a parameter the way
// NRC_RW_STRUCTURED_BUFFER does. They read and write module-scope buffers
// that the including shader declares with its own @group/@binding, under
// exactly these names (the NRC_USE_CUSTOM_BUFFER_ACCESSORS pattern):
//
//   var<storage, read_write> nrc_query_path_info: array<u32>;        // NrcPackedQueryPathInfo, 2 words
//   var<storage, read_write> nrc_training_path_info: array<u32>;     // NrcPackedTrainingPathInfo, 2 words
//   var<storage, read_write> nrc_training_path_vertices: array<u32>; // NrcPackedPathVertex, 12 words
//   var<storage, read_write> nrc_query_radiance_params: array<u32>;  // NrcRadianceParams, 14 words
//   var<storage, read_write> nrc_counters: array<atomic<u32>>;       // NrcCounter
//   var<storage, read> nrc_query_radiance: array<f32>;               // float3, 3 words
//
// Every function here is validated, used or not, so a pass that binds only
// some of these buffers (the resolve pass reads query radiance and query
// path info) copies the helpers it needs instead of including this file.

// NRC_BUFFER_QUERY_RADIANCE_PARAMS[index] = params
fn nrc_write_radiance_params(index: u32, params: NrcRadianceParams) {
    let base = index * NRC_RADIANCE_PARAMS_WORDS;
    nrc_query_radiance_params[base + 0u] = bitcast<u32>(params.encoded_position.x);
    nrc_query_radiance_params[base + 1u] = bitcast<u32>(params.encoded_position.y);
    nrc_query_radiance_params[base + 2u] = bitcast<u32>(params.encoded_position.z);
    nrc_query_radiance_params[base + 3u] = bitcast<u32>(params.roughness);
    nrc_query_radiance_params[base + 4u] = bitcast<u32>(params.normal.x);
    nrc_query_radiance_params[base + 5u] = bitcast<u32>(params.normal.y);
    nrc_query_radiance_params[base + 6u] = bitcast<u32>(params.view_direction.x);
    nrc_query_radiance_params[base + 7u] = bitcast<u32>(params.view_direction.y);
    nrc_query_radiance_params[base + 8u] = bitcast<u32>(params.albedo.x);
    nrc_query_radiance_params[base + 9u] = bitcast<u32>(params.albedo.y);
    nrc_query_radiance_params[base + 10u] = bitcast<u32>(params.albedo.z);
    nrc_query_radiance_params[base + 11u] = bitcast<u32>(params.specular.x);
    nrc_query_radiance_params[base + 12u] = bitcast<u32>(params.specular.y);
    nrc_query_radiance_params[base + 13u] = bitcast<u32>(params.specular.z);
}

// NRC_BUFFER_QUERY_RADIANCE_PARAMS[index]
fn nrc_read_radiance_params(index: u32) -> NrcRadianceParams {
    let base = index * NRC_RADIANCE_PARAMS_WORDS;
    var params: NrcRadianceParams;
    params.encoded_position = bitcast<vec3<f32>>(vec3<u32>(
        nrc_query_radiance_params[base + 0u],
        nrc_query_radiance_params[base + 1u],
        nrc_query_radiance_params[base + 2u],
    ));
    params.roughness = bitcast<f32>(nrc_query_radiance_params[base + 3u]);
    params.normal = bitcast<vec2<f32>>(vec2<u32>(
        nrc_query_radiance_params[base + 4u],
        nrc_query_radiance_params[base + 5u],
    ));
    params.view_direction = bitcast<vec2<f32>>(vec2<u32>(
        nrc_query_radiance_params[base + 6u],
        nrc_query_radiance_params[base + 7u],
    ));
    params.albedo = bitcast<vec3<f32>>(vec3<u32>(
        nrc_query_radiance_params[base + 8u],
        nrc_query_radiance_params[base + 9u],
        nrc_query_radiance_params[base + 10u],
    ));
    params.specular = bitcast<vec3<f32>>(vec3<u32>(
        nrc_query_radiance_params[base + 11u],
        nrc_query_radiance_params[base + 12u],
        nrc_query_radiance_params[base + 13u],
    ));
    return params;
}

// NRC_BUFFER_TRAINING_PATH_VERTICES[index] = vertex
fn nrc_write_packed_path_vertex(index: u32, vertex: NrcPackedPathVertex) {
    let base = index * NRC_PACKED_PATH_VERTEX_WORDS;
    for (var i = 0u; i < 7u; i++) {
        nrc_training_path_vertices[base + NRC_PACKED_PATH_VERTEX_DATA + i] = vertex.data[i];
    }
    nrc_training_path_vertices[base + NRC_PACKED_PATH_VERTEX_PAD0] = vertex.pad0;
    nrc_training_path_vertices[base + 8u] = bitcast<u32>(vertex.encoded_position.x);
    nrc_training_path_vertices[base + 9u] = bitcast<u32>(vertex.encoded_position.y);
    nrc_training_path_vertices[base + 10u] = bitcast<u32>(vertex.encoded_position.z);
    nrc_training_path_vertices[base + NRC_PACKED_PATH_VERTEX_PAD1] = vertex.pad1;
}

// NRC_BUFFER_TRAINING_PATH_VERTICES[index]
fn nrc_read_packed_path_vertex(index: u32) -> NrcPackedPathVertex {
    let base = index * NRC_PACKED_PATH_VERTEX_WORDS;
    var vertex: NrcPackedPathVertex;
    for (var i = 0u; i < 7u; i++) {
        vertex.data[i] = nrc_training_path_vertices[base + NRC_PACKED_PATH_VERTEX_DATA + i];
    }
    vertex.pad0 = nrc_training_path_vertices[base + NRC_PACKED_PATH_VERTEX_PAD0];
    vertex.encoded_position = bitcast<vec3<f32>>(vec3<u32>(
        nrc_training_path_vertices[base + 8u],
        nrc_training_path_vertices[base + 9u],
        nrc_training_path_vertices[base + 10u],
    ));
    vertex.pad1 = nrc_training_path_vertices[base + NRC_PACKED_PATH_VERTEX_PAD1];
    return vertex;
}

// NRC_BUFFER_QUERY_PATH_INFO[index] = info
fn nrc_write_query_path_info(index: u32, info: NrcPackedQueryPathInfo) {
    let base = index * NRC_PACKED_QUERY_PATH_INFO_WORDS;
    nrc_query_path_info[base + NRC_PACKED_QUERY_PATH_INFO_PREFIX_THROUGHPUT] = info.prefix_throughput;
    nrc_query_path_info[base + NRC_PACKED_QUERY_PATH_INFO_QUERY_BUFFER_INDEX] = info.query_buffer_index;
}

// NRC_BUFFER_QUERY_PATH_INFO[index]
fn nrc_read_query_path_info(index: u32) -> NrcPackedQueryPathInfo {
    let base = index * NRC_PACKED_QUERY_PATH_INFO_WORDS;
    return NrcPackedQueryPathInfo(
        nrc_query_path_info[base + NRC_PACKED_QUERY_PATH_INFO_PREFIX_THROUGHPUT],
        nrc_query_path_info[base + NRC_PACKED_QUERY_PATH_INFO_QUERY_BUFFER_INDEX],
    );
}

// NRC_BUFFER_TRAINING_PATH_INFO[index] = info
fn nrc_write_training_path_info(index: u32, info: NrcPackedTrainingPathInfo) {
    let base = index * NRC_PACKED_TRAINING_PATH_INFO_WORDS;
    nrc_training_path_info[base + NRC_PACKED_TRAINING_PATH_INFO_PACKED_DATA] = info.packed_data;
    nrc_training_path_info[base + NRC_PACKED_TRAINING_PATH_INFO_QUERY_BUFFER_INDEX] = info.query_buffer_index;
}

// NRC_BUFFER_TRAINING_PATH_INFO[index]
fn nrc_read_training_path_info(index: u32) -> NrcPackedTrainingPathInfo {
    let base = index * NRC_PACKED_TRAINING_PATH_INFO_WORDS;
    return NrcPackedTrainingPathInfo(
        nrc_training_path_info[base + NRC_PACKED_TRAINING_PATH_INFO_PACKED_DATA],
        nrc_training_path_info[base + NRC_PACKED_TRAINING_PATH_INFO_QUERY_BUFFER_INDEX],
    );
}

// The library's query result for record `index`: float3 at a 12-byte stride.
// (Pass it to nrc_unpack_query_radiance.)
fn nrc_read_query_radiance(index: u32) -> vec3<f32> {
    let base = index * NRC_QUERY_RADIANCE_WORDS;
    return vec3<f32>(
        nrc_query_radiance[base + 0u],
        nrc_query_radiance[base + 1u],
        nrc_query_radiance[base + 2u],
    );
}

// NrcIncrementCounter: the reference implementation the HLSL keeps under
// #else, one atomicAdd per thread. The HLSL default does one InterlockedAdd
// per wave (WaveActiveCountBits/WavePrefixCountBits) and hands out the same
// set of indices in a different order; that version needs WGSL subgroups.
// Returns the counter's value before the increment.
fn nrc_increment_counter(counter: u32) -> u32 {
    return atomicAdd(&nrc_counters[counter], 1u);
}
