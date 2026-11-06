// The NRC query record both query passes write (docs/nrc-integration.md):
// restir_gi's initial pass at its diffuse sample point (the pixel's first
// query path slot) and, with NRC_SPECULAR, specular_gi where its glossy path
// would end in the world cache (the second slot). Only modules compiled with
// `NRC` import it, so nothing in it is conditional.

import bevy_pbr::render::pbr_functions::calculate_diffuse_color;
import bevy_pbr::render::pbr_functions::calculate_F0;
import bevy_solarik::scene::raytracing_scene_bindings::ResolvedRayHitFull;
import bevy_solarik::realtime::nrc::nrc_constants;
import bevy_solarik::realtime::nrc::NrcRadianceParams;
import bevy_solarik::realtime::nrc::NRC_COUNTER_QUERIES;
import bevy_solarik::realtime::nrc::nrc_encode_position;
import bevy_solarik::realtime::nrc::nrc_safe_cartesian_to_spherical_unorm;
import bevy_solarik::realtime::nrc::nrc_increment_counter;
import bevy_solarik::realtime::nrc::nrc_write_radiance_params;

// A query written: its index in the library's buffers, and the prefix
// throughput the resolve multiplies the answer by (linear; the caller folds
// in any factor the path applies later, then LogLuv-encodes it into the
// query path info).
struct NrcQueryRecord {
    query_index: u32,
    prefix_throughput: vec3<f32>,
}

// NrcUpdateOnHit in query mode for a surface seen from `wo`, at the end of a
// path prefix whose throughput is `throughput`: takes a query index and
// writes the radiance params. The surface is described as
// nrc_training.wesl describes its training vertices, or the network is
// asked about inputs it never saw: the shading normal (a leaf's side facing
// `wo`), `wo` toward where the path came from, linear roughness, bevy_pbr's
// albedo and F0.
fn write_nrc_query_record(sample_point: ResolvedRayHitFull, wo: vec3<f32>, throughput: vec3<f32>) -> NrcQueryRecord {
    let material = sample_point.material;
    var normal = sample_point.world_normal;
    if material.diffuse_transmission > 0.0 && dot(normal, wo) < 0.0 {
        normal = -normal;
    }
    let albedo = calculate_diffuse_color(material.base_color, material.metallic, 0.0, 0.0);
    let specular = calculate_F0(material.base_color, material.metallic, vec3(material.reflectance));

    var params: NrcRadianceParams;
    params.encoded_position = nrc_encode_position(sample_point.world_position, nrc_constants);
    params.roughness = material.roughness;
    params.normal = nrc_safe_cartesian_to_spherical_unorm(normal);
    params.view_direction = nrc_safe_cartesian_to_spherical_unorm(wo);
    params.albedo = albedo;
    params.specular = specular;
    let query_index = nrc_increment_counter(NRC_COUNTER_QUERIES);
    nrc_write_radiance_params(query_index, params);
    // learn_irradiance: the answer is radiance / (albedo + F0), so that times
    // the path's throughput is the prefix throughput.
    return NrcQueryRecord(query_index, max(throughput * (albedo + specular), vec3(0.0)));
}
