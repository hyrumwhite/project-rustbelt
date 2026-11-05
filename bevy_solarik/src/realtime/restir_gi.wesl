import bevy_solarik::filtered_canopy_scene::forest_set_seed;
import bevy_solarik::scene::scene_hit::scene_hit_is_miss;
@if((RAY_MATERIAL_FOOTPRINTS)) import bevy_solarik::scene::raytracing_scene_bindings::primary_ray_cone;
@if((RAY_MATERIAL_FOOTPRINTS)) import bevy_solarik::scene::raytracing_scene_bindings::advance_ray_cone;
@if((RAY_MATERIAL_FOOTPRINTS)) import bevy_solarik::scene::raytracing_scene_bindings::resolve_ray_hit_filtered;
import bevy_core_pipeline::tonemapping::tonemapping_luminance as luminance;
import bevy_pbr::prepass::bindings::PreviousViewUniforms;
import bevy_pbr::render::utils::rand_f;
import bevy_pbr::render::utils::sample_disk;
import bevy_render::maths::PI;
import bevy_render::view::View;
import bevy_solarik::scene::brdf::evaluate_diffuse_brdf;
import bevy_solarik::realtime::surface_path::shade_surface_scattering;
import bevy_solarik::realtime::sky_sampling::sample_sky_mixture;
import bevy_solarik::realtime::gbuffer_utils::gpixel_resolve;
import bevy_solarik::realtime::gbuffer_utils::pixel_dissimilar;
import bevy_solarik::realtime::gbuffer_utils::temporal_pixel_dissimilar;
import bevy_solarik::realtime::gbuffer_utils::permute_pixel;
import bevy_solarik::scene::sampling::sample_random_light_transmitted;
import bevy_solarik::scene::sampling::shade_gi_connection;
import bevy_solarik::scene::sampling::balance_heuristic;
import bevy_solarik::scene::sampling::isnan;
import bevy_solarik::scene::raytracing_scene_bindings::trace_ray;
import bevy_solarik::scene::raytracing_scene_bindings::resolve_ray_hit_full;
import bevy_solarik::scene::raytracing_scene_bindings::sample_sky;
import bevy_solarik::scene::raytracing_scene_bindings::RAY_T_MIN;
import bevy_solarik::scene::raytracing_scene_bindings::ray_max_distance;
import bevy_solarik::realtime::world_cache_query::query_two_sided_world_cache;
import bevy_solarik::realtime::world_cache_query::cell_key_normal;
import bevy_solarik::realtime::world_cache_query::WORLD_CACHE_CELL_LIFETIME;
@if((VISUALIZE_CACHE_AGE)) import bevy_solarik::realtime::world_cache_query::world_cache_debug_samples;
@if((VISUALIZE_CACHE_AGE)) import bevy_solarik::realtime::world_cache_query::WORLD_CACHE_MAX_TEMPORAL_SAMPLES;
import bevy_solarik::realtime::realtime_bindings::view_output;
import bevy_solarik::realtime::realtime_bindings::gi_reservoirs_a;
import bevy_solarik::realtime::realtime_bindings::gi_reservoirs_b;
import bevy_solarik::realtime::realtime_bindings::gbuffer;
import bevy_solarik::realtime::realtime_bindings::depth_buffer;
import bevy_solarik::realtime::realtime_bindings::motion_vectors;
import bevy_solarik::realtime::realtime_bindings::previous_gbuffer;
import bevy_solarik::realtime::realtime_bindings::previous_depth_buffer;
import bevy_solarik::realtime::realtime_bindings::view;
import bevy_solarik::realtime::realtime_bindings::previous_view;
import bevy_solarik::realtime::realtime_bindings::constants;
import bevy_solarik::realtime::realtime_bindings::Reservoir;
import bevy_solarik::realtime::specular_gi::DIFFUSE_GI_REUSE_ROUGHNESS_THRESHOLD;
@if((REGIONAL_HISTORY)) import bevy_solarik::scene::raytracing_scene_bindings::scene_support_is_valid;
@if((REGIONAL_HISTORY)) import bevy_solarik::realtime::world_cache_query::query_two_sided_world_cache_with_support;
@if((NRC)) import bevy_pbr::render::pbr_functions::calculate_diffuse_color;
@if((NRC)) import bevy_pbr::render::pbr_functions::calculate_F0;
@if((NRC)) import bevy_solarik::scene::raytracing_scene_bindings::ResolvedRayHitFull;
@if((NRC)) import bevy_solarik::realtime::nrc::nrc_constants;
@if((NRC)) import bevy_solarik::realtime::nrc::NrcRadianceParams;
@if((NRC)) import bevy_solarik::realtime::nrc::NrcPackedQueryPathInfo;
@if((NRC)) import bevy_solarik::realtime::nrc::NRC_COUNTER_QUERIES;
@if((NRC)) import bevy_solarik::realtime::nrc::nrc_encode_position;
@if((NRC)) import bevy_solarik::realtime::nrc::nrc_safe_cartesian_to_spherical_unorm;
@if((NRC)) import bevy_solarik::realtime::nrc::nrc_encode_log_luv_hdr;
@if((NRC)) import bevy_solarik::realtime::nrc::nrc_decode_log_luv_hdr;
@if((NRC)) import bevy_solarik::realtime::nrc::nrc_unpack_query_radiance;
@if((NRC)) import bevy_solarik::realtime::nrc::nrc_increment_counter;
@if((NRC)) import bevy_solarik::realtime::nrc::nrc_write_radiance_params;
@if((NRC)) import bevy_solarik::realtime::nrc::nrc_write_query_path_info;
@if((NRC)) import bevy_solarik::realtime::nrc::nrc_read_query_path_info;
@if((NRC)) import bevy_solarik::realtime::nrc::nrc_read_query_radiance;

// https://intro-to-restir.cwyman.org/presentations/2023ReSTIR_Course_Notes.pdf
enable wgpu_ray_query;




const SPATIAL_REUSE_RADIUS_PIXELS = 30.0;
const CONFIDENCE_WEIGHT_CAP = 8.0;
// Confidence limits influence, not lifetime. Bright old samples can keep winning.
const MAX_GI_SAMPLE_AGE = 16.0;
// Where a sample that escaped to the sky is said to sit: far enough along its
// ray that the resampling jacobians read as 1 and the neighbours' visibility
// re-trace covers the whole scene, and under ray_max_distance().
const SKY_SAMPLE_DISTANCE = 10000.0;

// NRC (docs/nrc-integration.md): the initial pass writes a cache query for
// its sample point instead of reading the world cache, the library answers
// every query at once between the passes (QueryAndTrain), and
// nrc_resolve_and_temporal puts the answers into the reservoirs before the
// temporal step, which needs the radiance for its target function. These
// hold the query generate_initial_reservoir made for this pixel, if any
// (0xffffffff: none), and its prefix throughput (LogLuv).
@if((NRC))
var<private> nrc_query_index: u32 = 0xffffffffu;
@if((NRC))
var<private> nrc_query_prefix: u32 = 0u;

@compute @workgroup_size(8, 8, 1)
fn initial_and_temporal(@builtin(global_invocation_id) global_id: vec3<u32>) {
    if any(global_id.xy >= vec2u(view.main_pass_viewport.zw)) { return; }

    let pixel_index = global_id.x + global_id.y * u32(view.main_pass_viewport.z);
    var rng = pixel_index + constants.frame_index;
    forest_set_seed(vec2u(pixel_index,constants.frame_index^0x7a78b931u));

    let depth = textureLoad(depth_buffer, global_id.xy, 0);
    if depth == 0.0 {
@if(NRC) {
        write_nrc_query_path_info(pixel_index);
}
        gi_reservoirs_b[pixel_index] = empty_reservoir();
        return;
    }
    let surface = gpixel_resolve(textureLoad(gbuffer, global_id.xy, 0), depth, global_id.xy, view.main_pass_viewport.zw, view.world_from_clip);
    if surface.material.metallic > 0.9999 && surface.material.roughness <= DIFFUSE_GI_REUSE_ROUGHNESS_THRESHOLD {
@if(NRC) {
        write_nrc_query_path_info(pixel_index);
}
        gi_reservoirs_b[pixel_index] = empty_reservoir();
        return;
    }

@if(VISUALIZE_CACHE_AGE) {
    world_cache_debug_samples = -2.0;
}
    let initial_reservoir = generate_initial_reservoir(surface.world_position, surface.world_normal, &rng);
// (SOLARIK_SHADER_DEFS=VISUALIZE_CACHE_AGE, with HALO_RT_NO_DLSS=1: the
// samples in the cache cell this pixel's diffuse bounce read, red at one to
// green at the full history, blue for a cell made just now; magenta where
// the bounce's point found no cell; black for a bounce that read no cell,
// to the sky, an emitter, or none. The later passes leave the view.)
@if(VISUALIZE_CACHE_AGE) {
    var debug_colour = vec3(0.0);
    if world_cache_debug_samples == -1.0 {
        debug_colour = vec3(1.0, 0.0, 1.0);
    } else if world_cache_debug_samples == 0.0 {
        debug_colour = vec3(0.0, 0.0, 1.0);
    } else if world_cache_debug_samples > 0.0 {
        let t = saturate(world_cache_debug_samples / WORLD_CACHE_MAX_TEMPORAL_SAMPLES);
        debug_colour = vec3(1.0 - t, t, 0.0);
    }
    textureStore(view_output, global_id.xy, vec4(debug_colour * 0.5, 1.0));
}
// (SOLARIK_SHADER_DEFS=VISUALIZE_GI_INITIAL, with HALO_RT_NO_DLSS=1: the
// initial sample's radiance from the world cache, times 32 as VISUALIZE_NRC
// with VISUALIZE_NRC_GAIN shows NRC's answers for the same points. The later
// passes leave the view.)
@if(VISUALIZE_GI_INITIAL) {
    textureStore(view_output, global_id.xy, vec4(initial_reservoir.radiance * view.exposure * 32.0, 1.0));
    gi_reservoirs_b[pixel_index] = initial_reservoir;
    return;
}
@if(NRC) {
    // The sample's radiance is not known until QueryAndTrain has run: keep
    // the initial reservoir, nrc_resolve_and_temporal finishes it.
    write_nrc_query_path_info(pixel_index);
    gi_reservoirs_b[pixel_index] = initial_reservoir;
    return;
}
    let temporal = load_temporal_reservoir(global_id.xy, depth, surface.world_position, surface.world_normal);
    let merge_result = merge_reservoirs(initial_reservoir, surface.world_position, surface.world_normal, surface.material.base_color / PI,
        temporal.reservoir, temporal.world_position, temporal.world_normal, temporal.diffuse_brdf, &rng);

    gi_reservoirs_b[pixel_index] = merge_result.merged_reservoir;
}

// After QueryAndTrain: the NRC answer for each pixel's sample point becomes
// the reservoir's radiance, then the temporal step initial_and_temporal
// leaves out with NRC.
@compute @workgroup_size(8, 8, 1)
@if((NRC))
fn nrc_resolve_and_temporal(@builtin(global_invocation_id) global_id: vec3<u32>) {
    if any(global_id.xy >= vec2u(view.main_pass_viewport.zw)) { return; }

    let pixel_index = global_id.x + global_id.y * u32(view.main_pass_viewport.z);
    // (Not initial_and_temporal's stream: its first numbers drew the sample.)
    var rng = (pixel_index + constants.frame_index) ^ 0x3c6ef372u;

    // The same early returns as initial_and_temporal, which has already
    // stored their empty reservoirs.
    let depth = textureLoad(depth_buffer, global_id.xy, 0);
    if depth == 0.0 { return; }
    let surface = gpixel_resolve(textureLoad(gbuffer, global_id.xy, 0), depth, global_id.xy, view.main_pass_viewport.zw, view.world_from_clip);
    if surface.material.metallic > 0.9999 && surface.material.roughness <= DIFFUSE_GI_REUSE_ROUGHNESS_THRESHOLD { return; }

    var reservoir = gi_reservoirs_b[pixel_index];
    let info = nrc_read_query_path_info(pixel_index);
    var answer = vec3(0.0);
    if info.query_buffer_index != 0xffffffffu {
        // The library learns radiance divided by the query's albedo + F0
        // (learn_irradiance), in its own scale, from exposed radiance; the
        // prefix throughput and the unpack multiplier undo the first two and
        // the exposure the last. The result is the outgoing radiance toward
        // this pixel's surface, the albedo included.
        answer = max(vec3(0.0), nrc_decode_log_luv_hdr(info.prefix_throughput)
            * nrc_unpack_query_radiance(nrc_constants, nrc_read_query_radiance(info.query_buffer_index)) / view.exposure);
// (SOLARIK_SHADER_DEFS=NRC_CONSTANT_ANSWER, a diagnostic: the answer replaced
// by a flat exposed radiance of 0.1 times the sample point's albedo, to tell
// the answers' own variation from the rest of the NRC path.)
@if(NRC_CONSTANT_ANSWER) {
        answer = nrc_decode_log_luv_hdr(info.prefix_throughput) * 0.1 / view.exposure;
}
        reservoir.radiance = answer;
    }
// (SOLARIK_SHADER_DEFS=VISUALIZE_NRC: the cache's answers at the diffuse
// bounce's sample points, black where the pixel made no query: the sky, an
// emitter, the world cache during NRC's warm-up. spatial_and_shade leaves
// the view.)
@if(VISUALIZE_NRC) {
    var shown = answer * view.exposure;
@if(VISUALIZE_NRC_GAIN) { shown *= 32.0; } // (dim interiors: the answers are a few percent)
    textureStore(view_output, global_id.xy, vec4(shown, 1.0));
    gi_reservoirs_b[pixel_index] = reservoir;
    return;
}

    let temporal = load_temporal_reservoir(global_id.xy, depth, surface.world_position, surface.world_normal);
    let merge_result = merge_reservoirs(reservoir, surface.world_position, surface.world_normal, surface.material.base_color / PI,
        temporal.reservoir, temporal.world_position, temporal.world_normal, temporal.diffuse_brdf, &rng);

    gi_reservoirs_b[pixel_index] = merge_result.merged_reservoir;
}

@compute @workgroup_size(8, 8, 1)
fn spatial_and_shade(@builtin(global_invocation_id) global_id: vec3<u32>) {
    if any(global_id.xy >= vec2u(view.main_pass_viewport.zw)) { return; }

    let pixel_index = global_id.x + global_id.y * u32(view.main_pass_viewport.z);
    var rng = pixel_index + constants.frame_index;
    forest_set_seed(vec2u(pixel_index,constants.frame_index^0x17917811u));

    let depth = textureLoad(depth_buffer, global_id.xy, 0);
    if depth == 0.0 {
        gi_reservoirs_a[pixel_index] = empty_reservoir();
        return;
    }
    let surface = gpixel_resolve(textureLoad(gbuffer, global_id.xy, 0), depth, global_id.xy, view.main_pass_viewport.zw, view.world_from_clip);
    if surface.material.metallic > 0.9999 && surface.material.roughness <= DIFFUSE_GI_REUSE_ROUGHNESS_THRESHOLD {
        gi_reservoirs_a[pixel_index] = empty_reservoir();
        return;
    }

    let input_reservoir = gi_reservoirs_b[pixel_index];
    let spatial = load_spatial_reservoir(global_id.xy, depth, surface.world_position, surface.world_normal, &rng);
    let merge_result = merge_reservoirs(input_reservoir, surface.world_position, surface.world_normal, surface.material.base_color / PI,
        spatial.reservoir, spatial.world_position, spatial.world_normal, spatial.diffuse_brdf, &rng);
    var combined_reservoir = merge_result.merged_reservoir;

    // Endpoint radiance, age and unoccluded weights survive spatial reuse.
    gi_reservoirs_a[pixel_index] = combined_reservoir;
@if(VISUALIZE_DI_PICK) {
    return;
}
@if(VISUALIZE_CACHE_AGE) {
    return;
}
@if((NRC && VISUALIZE_NRC) || VISUALIZE_GI_INITIAL) {
    return;
}

    let wo = normalize(view.world_position - surface.world_position);
    let brdf = evaluate_diffuse_brdf(wo, merge_result.wi, surface.world_normal, surface.material);

    var pixel_color = textureLoad(view_output, global_id.xy);
    pixel_color += vec4(shade_gi_connection(surface.world_position, surface.world_normal, combined_reservoir.sample_point_world_position, combined_reservoir.radiance, combined_reservoir.unbiased_contribution_weight) * view.exposure * brdf, 0.0);
    textureStore(view_output, global_id.xy, pixel_color);
}


fn generate_initial_reservoir(world_position: vec3<f32>, world_normal: vec3<f32>, rng: ptr<function, u32>) -> Reservoir {
    var reservoir = empty_reservoir();
@if(REGIONAL_HISTORY) {
    // Unknown path transport fails closed for bounded regional reuse.
    reservoir.support_radius = ray_max_distance();
}

    let direction_sample = sample_sky_mixture(world_normal, rng);
    let ray_direction = direction_sample.direction;
    // A full-sphere sky draw below the receiver has zero contribution.
    // Do not resample it: conditioning would change the mixture PDF.
    if dot(world_normal, ray_direction) <= 0.0 {
        reservoir.confidence_weight = 1.0;
        reservoir.sample_point_world_position = world_position + ray_direction * SKY_SAMPLE_DISTANCE;
        reservoir.sample_point_world_normal = -ray_direction;
        return reservoir;
    }
    let ray = trace_ray(world_position + (world_normal * RAY_T_MIN), ray_direction, RAY_T_MIN, ray_max_distance(), RAY_FLAG_NONE);

    if scene_hit_is_miss(ray) {
        // The ray left the scene: it sees the sky. The sample point is a far
        // point on the ray facing back at us, so reuse, the jacobian and the
        // neighbours' visibility test work as for any other sample; its
        // radiance is the sky's, already the outgoing radiance toward us.
        reservoir.sample_point_world_position = world_position + ray_direction * SKY_SAMPLE_DISTANCE;
        reservoir.sample_point_world_normal = -ray_direction;
        reservoir.confidence_weight = 1.0;
        reservoir.radiance = sample_sky(ray_direction);
        reservoir.unbiased_contribution_weight = direction_sample.inverse_pdf;
        return reservoir;
    }

@if(RAY_MATERIAL_FOOTPRINTS) {
    let cone = primary_ray_cone(view.clip_from_view, view.main_pass_viewport.zw, distance(world_position, view.world_position) + ray.t);
    let sample_point = resolve_ray_hit_filtered(ray, ray_direction, cone);
}
@if(!(RAY_MATERIAL_FOOTPRINTS)) {
    let sample_point = resolve_ray_hit_full(ray);
}

    if any(sample_point.material.emissive != vec3(0.0)) {
        return reservoir;
    }

    reservoir.sample_point_world_position = sample_point.world_position;
    reservoir.sample_point_world_normal = sample_point.world_normal;
    if sample_point.material.diffuse_transmission > 0.0 {
        // One reservoir holds outgoing radiance for the sampled leaf side.
        // Use the physical plane for its Jacobian support: a tilted shading
        // normal must not let that radiance be reused across the leaf.
        let triangle_normal = sample_point.triangle_world_normal;
        reservoir.sample_point_world_normal = select(-triangle_normal, triangle_normal,
            dot(triangle_normal, world_position - sample_point.world_position) >= 0.0);
    }
    reservoir.confidence_weight = 1.0;

    // Particulate radiance depends on emission angle. It cannot be recovered
    // by multiplying cached hemisphere irradiance by a Lambert coefficient.
    if sample_point.material.lommel {
        reservoir.radiance = shade_surface_scattering(sample_point,
            normalize(world_position-sample_point.world_position), rng);
        reservoir.unbiased_contribution_weight = direction_sample.inverse_pdf;
        return reservoir;
    }

@if(NO_WORLD_CACHE) {
    let direct_lighting = sample_random_light_transmitted(sample_point.world_position, sample_point.world_normal, sample_point.geometric_world_normal, rng).light;
    let light_cosine = dot(direct_lighting.wi, sample_point.world_normal);
    var diffuse_cosine = saturate(light_cosine);
    if sample_point.material.diffuse_transmission > 0.0 {
        let t = sample_point.material.diffuse_transmission;
        diffuse_cosine = abs(light_cosine) * select(1.0 - t, t, light_cosine < 0.0);
    }
    reservoir.radiance = direct_lighting.radiance * diffuse_cosine;
    reservoir.unbiased_contribution_weight = direct_lighting.inverse_pdf * direction_sample.inverse_pdf;
}
@if(!(NO_WORLD_CACHE)) {
@if(NRC) {
    // NRC answers once it has warmed up (pad0, set by the renderer); until
    // then the world cache below. The query's answer is the full outgoing
    // radiance toward us, albedo included, so the base_color / PI factor
    // below is not applied; the resolve fills reservoir.radiance. (With
    // REGIONAL_HISTORY the support stays the whole scene, as set above: the
    // network's answer depends on everything it has seen.)
    if nrc_constants.pad0 != 0u {
        write_nrc_query(sample_point, -ray_direction);
        reservoir.unbiased_contribution_weight = direction_sample.inverse_pdf;
        return reservoir;
    }
}
    // Cache entries are incident hemisphere irradiance; only the endpoint
    // consumes material weights, before the existing albedo / PI below.
@if(REGIONAL_HISTORY) {
    let cached = query_two_sided_world_cache_with_support(sample_point.world_position,
        cell_key_normal(sample_point.geometric_world_normal, sample_point.triangle_world_normal), sample_point.material.diffuse_transmission,
        view.world_position, ray.t, WORLD_CACHE_CELL_LIFETIME, rng);
    reservoir.radiance = cached.rgb;
    reservoir.support_radius = max(ray.t + RAY_T_MIN, cached.w) * 1.00001 + 0.01;
}
@if(!(REGIONAL_HISTORY)) {
    reservoir.radiance = query_two_sided_world_cache(sample_point.world_position,
        cell_key_normal(sample_point.geometric_world_normal, sample_point.triangle_world_normal), sample_point.material.diffuse_transmission,
        view.world_position, ray.t, WORLD_CACHE_CELL_LIFETIME, rng);
}
    reservoir.unbiased_contribution_weight = direction_sample.inverse_pdf;
}

    let sample_point_diffuse_brdf = sample_point.material.base_color / PI;
    reservoir.radiance *= sample_point_diffuse_brdf;

    return reservoir;
}

// NrcUpdateOnHit in query mode, querying at the first vertex (the sample
// point is already one bounce in, so the termination heuristic has nothing to
// wait for). The surface is described as nrc_training.wesl describes its
// training vertices, or the network is asked about inputs it never saw: the
// shading normal (a leaf's side facing `wo`), `wo` toward the receiver,
// linear roughness, bevy_pbr's albedo and F0.
@if((NRC))
fn write_nrc_query(sample_point: ResolvedRayHitFull, wo: vec3<f32>) {
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
    nrc_query_index = nrc_increment_counter(NRC_COUNTER_QUERIES);
    nrc_write_radiance_params(nrc_query_index, params);
    // learn_irradiance: the answer is radiance / (albedo + F0), so that is the
    // prefix throughput (the path's own throughput is 1: the reservoir
    // weights the connection to the receiver itself).
    nrc_query_prefix = nrc_encode_log_luv_hdr(max(albedo + specular, vec3(0.0)));
}

// NrcWriteFinalPathInfo in query mode: one query path per pixel (one sample
// per pixel, so the query path index is the pixel index).
@if((NRC))
fn write_nrc_query_path_info(pixel_index: u32) {
    nrc_write_query_path_info(pixel_index, NrcPackedQueryPathInfo(nrc_query_prefix, nrc_query_index));
}

fn load_temporal_reservoir(pixel_id: vec2<u32>, depth: f32, world_position: vec3<f32>, world_normal: vec3<f32>) -> NeighborInfo {
    let motion_vector = textureLoad(motion_vectors, pixel_id, 0).xy;
    let temporal_pixel_id_float = round(vec2<f32>(pixel_id) - (motion_vector * view.main_pass_viewport.zw));
    var point_temporal_pixel_id = vec2<u32>(temporal_pixel_id_float);

    if (constants.reset & 1u) != 0u || (constants.reset & 6u) == 2u {
        return NeighborInfo(empty_reservoir(), vec3(0.0), vec3(0.0), vec3(0.0));
    }

    if any(temporal_pixel_id_float < vec2(0.0)) || any(temporal_pixel_id_float >= view.main_pass_viewport.zw) {
        point_temporal_pixel_id = pixel_id;
    }

    let permuted_temporal_pixel_id = permute_pixel(point_temporal_pixel_id, constants.frame_index, view.main_pass_viewport.zw);
    var temporal = load_temporal_reservoir_inner(permuted_temporal_pixel_id, depth, world_position, world_normal);

    temporal.reservoir = age_temporal_reservoir(temporal.reservoir);

    return temporal;
}

fn age_temporal_reservoir(input: Reservoir) -> Reservoir {
    var reservoir = input;
    reservoir.sample_age += 1.0;
    if reservoir.sample_age >= MAX_GI_SAMPLE_AGE {
        return empty_reservoir();
    }
    reservoir.confidence_weight = min(reservoir.confidence_weight, CONFIDENCE_WEIGHT_CAP);
    return reservoir;
}

fn load_temporal_reservoir_inner(temporal_pixel_id: vec2<u32>, depth: f32, world_position: vec3<f32>, world_normal: vec3<f32>) -> NeighborInfo {
    // Check if the pixel features have changed heavily between the current and previous frame
    let temporal_depth = textureLoad(previous_depth_buffer, temporal_pixel_id, 0);
    let temporal_surface = gpixel_resolve(textureLoad(previous_gbuffer, temporal_pixel_id, 0), temporal_depth, temporal_pixel_id, view.main_pass_viewport.zw, previous_view.world_from_clip);
    let temporal_diffuse_brdf = temporal_surface.material.base_color / PI;
    if temporal_pixel_dissimilar(depth, world_position, temporal_surface.world_position, world_normal, temporal_surface.world_normal, view, previous_view.view_from_world) {
        return NeighborInfo(empty_reservoir(), vec3(0.0), vec3(0.0), vec3(0.0));
    }

    let temporal_pixel_index = temporal_pixel_id.x + temporal_pixel_id.y * u32(view.main_pass_viewport.z);
    let temporal_reservoir = gi_reservoirs_a[temporal_pixel_index];
@if(REGIONAL_HISTORY) {
    if (constants.reset & 4u) != 0u && !scene_support_is_valid(temporal_reservoir.sample_point_world_position, temporal_reservoir.support_radius) {
        return NeighborInfo(empty_reservoir(), vec3f(0.0), vec3f(0.0), vec3f(0.0));
    }
}

    return NeighborInfo(temporal_reservoir, temporal_surface.world_position, temporal_surface.world_normal, temporal_diffuse_brdf);
}

fn load_spatial_reservoir(pixel_id: vec2<u32>, depth: f32, world_position: vec3<f32>, world_normal: vec3<f32>, rng: ptr<function, u32>) -> NeighborInfo {
    for (var i = 0u; i < 5u; i++) {
        let spatial_pixel_id = get_neighbor_pixel_id(pixel_id, SPATIAL_REUSE_RADIUS_PIXELS, rng);

        let spatial_depth = textureLoad(depth_buffer, spatial_pixel_id, 0);
        let spatial_surface = gpixel_resolve(textureLoad(gbuffer, spatial_pixel_id, 0), spatial_depth, spatial_pixel_id, view.main_pass_viewport.zw, view.world_from_clip);
        let spatial_diffuse_brdf = spatial_surface.material.base_color / PI;
        if pixel_dissimilar(depth, world_position, spatial_surface.world_position, world_normal, spatial_surface.world_normal, view) {
            continue;
        }

        let spatial_pixel_index = spatial_pixel_id.x + spatial_pixel_id.y * u32(view.main_pass_viewport.z);
        let spatial_reservoir = gi_reservoirs_b[spatial_pixel_index];
        return NeighborInfo(spatial_reservoir, spatial_surface.world_position, spatial_surface.world_normal, spatial_diffuse_brdf);
    }

    return NeighborInfo(empty_reservoir(), world_position, world_normal, vec3(0.0));
}

fn get_neighbor_pixel_id(center_pixel_id: vec2<u32>, search_radius: f32, rng: ptr<function, u32>) -> vec2<u32> {
    var spatial_id = vec2<f32>(center_pixel_id) + sample_disk(search_radius, rng);
    spatial_id = clamp(spatial_id, vec2(0.0), view.main_pass_viewport.zw - 1.0);
    return vec2<u32>(spatial_id);
}

struct NeighborInfo {
    reservoir: Reservoir,
    world_position: vec3<f32>,
    world_normal: vec3<f32>,
    diffuse_brdf: vec3<f32>,
}

fn jacobian(
    new_world_position: vec3<f32>,
    original_world_position: vec3<f32>,
    sample_point_world_position: vec3<f32>,
    sample_point_world_normal: vec3<f32>,
) -> f32 {
    let r = new_world_position - sample_point_world_position;
    let q = original_world_position - sample_point_world_position;
    let rl = length(r);
    let ql = length(q);
    let phi_r = saturate(dot(r / rl, sample_point_world_normal));
    let phi_q = saturate(dot(q / ql, sample_point_world_normal));
    let jacobian = (phi_r * ql * ql) / (phi_q * rl * rl);
    return select(jacobian, 0.0, isinf(jacobian) || isnan(jacobian));
}

fn isinf(x: f32) -> bool {
    return (bitcast<u32>(x) & 0x7fffffffu) == 0x7f800000u;
}

fn empty_reservoir() -> Reservoir {
    return Reservoir();
}

struct ReservoirMergeResult {
    merged_reservoir: Reservoir,
    selected_sample_radiance: vec3<f32>,
    wi: vec3<f32>,
}

fn merge_reservoirs(
    canonical_reservoir: Reservoir,
    canonical_world_position: vec3<f32>,
    canonical_world_normal: vec3<f32>,
    canonical_diffuse_brdf: vec3<f32>,
    other_reservoir: Reservoir,
    other_world_position: vec3<f32>,
    other_world_normal: vec3<f32>,
    other_diffuse_brdf: vec3<f32>,
    rng: ptr<function, u32>,
) -> ReservoirMergeResult {
    // Radiances for resampling
    let canonical_sample_wi = normalize(canonical_reservoir.sample_point_world_position - canonical_world_position);
    let other_sample_wi = normalize(other_reservoir.sample_point_world_position - canonical_world_position);

    // Target functions for resampling and MIS
    let canonical_target_function_canonical_sample = luminance(
        canonical_reservoir.radiance * saturate(dot(canonical_sample_wi, canonical_world_normal)) * canonical_diffuse_brdf
    );
    let canonical_target_function_other_sample = luminance(
        other_reservoir.radiance * saturate(dot(other_sample_wi, canonical_world_normal)) * canonical_diffuse_brdf
    );

    // Extra target functions for MIS
    let other_target_function_canonical_sample = luminance(
        canonical_reservoir.radiance * saturate(dot(normalize(canonical_reservoir.sample_point_world_position - other_world_position), other_world_normal)) * other_diffuse_brdf
    );
    let other_target_function_other_sample = luminance(
        other_reservoir.radiance * saturate(dot(normalize(other_reservoir.sample_point_world_position - other_world_position), other_world_normal)) * other_diffuse_brdf
    );

    // Jacobians for resampling and MIS
    let canonical_target_function_other_sample_jacobian = jacobian(
        canonical_world_position,
        other_world_position,
        other_reservoir.sample_point_world_position,
        other_reservoir.sample_point_world_normal
    );
    let other_target_function_canonical_sample_jacobian = jacobian(
        other_world_position,
        canonical_world_position,
        canonical_reservoir.sample_point_world_position,
        canonical_reservoir.sample_point_world_normal
    );

    // Don't merge samples with huge jacobians, as it explodes the variance
    if canonical_target_function_other_sample_jacobian > 1.2 || other_target_function_canonical_sample_jacobian > 1.2 {
        return ReservoirMergeResult(canonical_reservoir, canonical_reservoir.radiance, canonical_sample_wi);
    }

    // Resampling weight for canonical sample
    let canonical_sample_mis_weight = balance_heuristic(
        canonical_reservoir.confidence_weight * canonical_target_function_canonical_sample,
        other_reservoir.confidence_weight * other_target_function_canonical_sample * other_target_function_canonical_sample_jacobian,
    );
    let canonical_sample_resampling_weight = canonical_sample_mis_weight * canonical_target_function_canonical_sample * canonical_reservoir.unbiased_contribution_weight;

    // Resampling weight for other sample
    let other_sample_mis_weight = balance_heuristic(
        other_reservoir.confidence_weight * other_target_function_other_sample,
        canonical_reservoir.confidence_weight * canonical_target_function_other_sample * canonical_target_function_other_sample_jacobian,
    );
    let other_sample_resampling_weight = other_sample_mis_weight * canonical_target_function_other_sample * other_reservoir.unbiased_contribution_weight * canonical_target_function_other_sample_jacobian;

    // Perform resampling
    var combined_reservoir = empty_reservoir();
    combined_reservoir.confidence_weight = canonical_reservoir.confidence_weight + other_reservoir.confidence_weight;
    let weight_sum = canonical_sample_resampling_weight + other_sample_resampling_weight;

    if rand_f(rng) < other_sample_resampling_weight / weight_sum {
        combined_reservoir.sample_point_world_position = other_reservoir.sample_point_world_position;
        combined_reservoir.sample_point_world_normal = other_reservoir.sample_point_world_normal;
        combined_reservoir.radiance = other_reservoir.radiance;
        combined_reservoir.sample_age = other_reservoir.sample_age;
@if(REGIONAL_HISTORY) {
        combined_reservoir.support_radius = max(other_reservoir.support_radius,
            canonical_reservoir.support_radius + distance(canonical_reservoir.sample_point_world_position, other_reservoir.sample_point_world_position));
}

        let inverse_target_function = select(0.0, 1.0 / canonical_target_function_other_sample, canonical_target_function_other_sample > 0.0);
        combined_reservoir.unbiased_contribution_weight = weight_sum * inverse_target_function;

        return ReservoirMergeResult(combined_reservoir, other_reservoir.radiance, other_sample_wi);
    } else {
        combined_reservoir.sample_point_world_position = canonical_reservoir.sample_point_world_position;
        combined_reservoir.sample_point_world_normal = canonical_reservoir.sample_point_world_normal;
        combined_reservoir.radiance = canonical_reservoir.radiance;
        combined_reservoir.sample_age = canonical_reservoir.sample_age;
@if(REGIONAL_HISTORY) {
        combined_reservoir.support_radius = max(canonical_reservoir.support_radius,
            other_reservoir.support_radius + distance(canonical_reservoir.sample_point_world_position, other_reservoir.sample_point_world_position));
}

        let inverse_target_function = select(0.0, 1.0 / canonical_target_function_canonical_sample, canonical_target_function_canonical_sample > 0.0);
        combined_reservoir.unbiased_contribution_weight = weight_sum * inverse_target_function;

        return ReservoirMergeResult(combined_reservoir, canonical_reservoir.radiance, canonical_sample_wi);
    }
}
