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
import bevy_solarik::realtime::tuning::{GI_CONFIDENCE_CAP, GI_ADAPTIVE_CONFIDENCE};
import bevy_solarik::realtime::realtime_bindings::Reservoir;
import bevy_solarik::realtime::specular_gi::DIFFUSE_GI_REUSE_ROUGHNESS_THRESHOLD;
@if((REGIONAL_HISTORY)) import bevy_solarik::scene::raytracing_scene_bindings::scene_support_is_valid;
@if((REGIONAL_HISTORY)) import bevy_solarik::realtime::world_cache_query::query_two_sided_world_cache_with_support;
@if((NRC)) import bevy_solarik::scene::raytracing_scene_bindings::ResolvedRayHitFull;
@if((NRC)) import bevy_solarik::realtime::nrc::nrc_constants;
@if((NRC)) import bevy_solarik::realtime::nrc::NrcPackedQueryPathInfo;
@if((NRC)) import bevy_solarik::realtime::nrc::nrc_encode_log_luv_hdr;
@if((NRC)) import bevy_solarik::realtime::nrc::nrc_decode_log_luv_hdr;
@if((NRC)) import bevy_solarik::realtime::nrc::nrc_unpack_query_radiance;
@if((NRC)) import bevy_solarik::realtime::nrc_query::write_nrc_query_record;
@if((NRC)) import bevy_solarik::realtime::nrc::nrc_write_query_path_info;
@if((NRC)) import bevy_solarik::realtime::nrc::nrc_read_query_path_info;
@if((NRC)) import bevy_solarik::realtime::nrc::nrc_read_query_radiance;
@if((NRC && NRC_QUERY_AT_SPREAD)) import bevy_pbr::render::utils::rand_u;
@if((NRC && NRC_QUERY_AT_SPREAD)) import bevy_solarik::scene::brdf::evaluate_brdf;
@if((NRC && NRC_QUERY_AT_SPREAD)) import bevy_solarik::scene::brdf::evaluate_and_sample_brdf;
@if((NRC && NRC_QUERY_AT_SPREAD)) import bevy_solarik::scene::sampling::LightSample;
@if((NRC && NRC_QUERY_AT_SPREAD)) import bevy_solarik::scene::sampling::resolve_light_sample;
@if((NRC && NRC_QUERY_AT_SPREAD)) import bevy_solarik::scene::sampling::calculate_resolved_light_contribution;
@if((NRC && NRC_QUERY_AT_SPREAD)) import bevy_solarik::scene::sampling::trace_light_transmission_with_support;
@if((NRC && NRC_QUERY_AT_SPREAD)) import bevy_solarik::scene::thin_glass::offset_thin_glass_ray;
@if((NRC && NRC_QUERY_AT_SPREAD)) import bevy_solarik::scene::raytracing_scene_bindings::light_sources;
@if((NRC && NRC_QUERY_AT_SPREAD)) import bevy_solarik::scene::raytracing_scene_bindings::directional_lights;
@if((NRC && NRC_QUERY_AT_SPREAD)) import bevy_solarik::scene::raytracing_scene_bindings::MIRROR_ROUGHNESS_THRESHOLD;
@if((NRC && NRC_QUERY_AT_SPREAD)) import bevy_solarik::realtime::realtime_bindings::world_cache_light_slots;
@if((NRC && NRC_QUERY_AT_SPREAD)) import bevy_solarik::realtime::world_cache_query::find_world_cache_cell;
@if((NRC && NRC_QUERY_AT_SPREAD)) import bevy_solarik::realtime::world_cache_query::world_cache_slot_probabilities;
@if((NRC && NRC_QUERY_AT_SPREAD)) import bevy_solarik::realtime::light_ris::sample_light_ris;
@if((NRC && NRC_QUERY_AT_SPREAD)) import bevy_solarik::realtime::nrc::NrcPathState;
@if((NRC && NRC_QUERY_AT_SPREAD)) import bevy_solarik::realtime::nrc::nrc_evaluate_termination_heuristic;

// https://intro-to-restir.cwyman.org/presentations/2023ReSTIR_Course_Notes.pdf
enable wgpu_ray_query;




const SPATIAL_REUSE_RADIUS_PIXELS = 30.0;
// (SOLARIK_GI_CONFIDENCE_CAP, tuning.wesl)
const CONFIDENCE_WEIGHT_CAP = GI_CONFIDENCE_CAP;
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
// (NRC_QUERY_AT_SPREAD: the light tile the extra bounce's resampled NEE
// draws from, this pixel's 8x8 workgroup, as the training pass's tile; and
// whether this pixel took the extra bounce, for VISUALIZE_NRC_BOUNCED.)
@if((NRC && NRC_QUERY_AT_SPREAD))
var<private> nrc_light_tile: vec2<u32> = vec2(0u);
@if((NRC && NRC_QUERY_AT_SPREAD))
var<private> nrc_bounced: bool = false;

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
@if(NRC && NRC_QUERY_AT_SPREAD) {
    nrc_light_tile = global_id.xy / 8u;
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
// (SOLARIK_SHADER_DEFS=NRC_QUERY_AT_SPREAD,VISUALIZE_NRC_BOUNCED, with
// HALO_RT_NO_DLSS=1: magenta where the sample took the extra bounce, green
// where it queried at the sample point, black for no query (the sky, an
// emitter, a mirror receiver). The later passes leave the view.)
@if(NRC_QUERY_AT_SPREAD && VISUALIZE_NRC_BOUNCED) {
    var bounce_colour = vec3(0.0);
    if nrc_bounced {
        bounce_colour = vec3(1.0, 0.0, 1.0);
    } else if nrc_query_index != 0xffffffffu {
        bounce_colour = vec3(0.0, 1.0, 0.0);
    }
    textureStore(view_output, global_id.xy, vec4(bounce_colour, 1.0));
}
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

@if(NRC_SPECULAR) {
    // specular_gi's query, in the pixel's second slot (empty unless the main
    // lobe's glossy path asked): its answer times the prefix throughput into
    // the view, before the early returns below, since mirrors are the pixels
    // with specular paths. Exposed radiance as the view holds it: the
    // network learnt exposed radiance and the prefix holds no exposure (the
    // specular pass multiplied its own radiance by it before adding; the
    // GI's answer below is divided by it only for its unexposed reservoir).
    let specular_info = nrc_read_query_path_info(u32(view.main_pass_viewport.z) * u32(view.main_pass_viewport.w) + pixel_index);
    var specular_answer = vec3(0.0);
    if specular_info.query_buffer_index != 0xffffffffu {
        specular_answer = max(vec3(0.0), nrc_decode_log_luv_hdr(specular_info.prefix_throughput)
            * nrc_unpack_query_radiance(nrc_constants, nrc_read_query_radiance(specular_info.query_buffer_index)));
    }
// (SOLARIK_SHADER_DEFS=VISUALIZE_NRC_SPECULAR, with HALO_RT_NO_DLSS=1: the
// specular slot's answer alone, as it is added to the view; black where the
// glossy path ended elsewhere or the pixel traced none. The GI's temporal
// step is left out, and spatial_and_shade leaves the view.)
@if(VISUALIZE_NRC_SPECULAR) {
    textureStore(view_output, global_id.xy, vec4(specular_answer, 1.0));
    return;
}
    if specular_info.query_buffer_index != 0xffffffffu {
        textureStore(view_output, global_id.xy, textureLoad(view_output, global_id.xy) + vec4(specular_answer, 0.0));
    }
}

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
        // Added, not replacing: a query at the sample point itself leaves
        // the initial radiance at zero (the answer is all of it), while
        // NRC_QUERY_AT_SPREAD's extra bounce leaves the sample point's direct
        // light there (and the answer is what the bounce's hit sends back).
        reservoir.radiance += answer;
    }
// (SOLARIK_SHADER_DEFS=VISUALIZE_NRC: the diffuse bounce's sample points'
// radiance as the reservoir now holds it, the total: the cache's answer plus
// what the initial pass gathered itself (nothing for a query at the sample
// point; the direct light there, or the sky, after NRC_QUERY_AT_SPREAD's
// extra bounce; a lommel surface's own shading). Black where there was
// nothing: the sky, an emitter. During NRC's warm-up the world cache's
// value. spatial_and_shade leaves the view.)
@if(VISUALIZE_NRC && !(NRC_QUERY_AT_SPREAD && VISUALIZE_NRC_BOUNCED)) {
    var shown = reservoir.radiance * view.exposure;
@if(VISUALIZE_NRC_GAIN) { shown *= 32.0; } // (dim interiors: the answers are a few percent)
    textureStore(view_output, global_id.xy, vec4(shown, 1.0));
    gi_reservoirs_b[pixel_index] = reservoir;
    return;
}
@if(NRC_QUERY_AT_SPREAD && VISUALIZE_NRC_BOUNCED) {
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
    var merge_result = merge_reservoirs(input_reservoir, surface.world_position, surface.world_normal, surface.material.base_color / PI,
        spatial.reservoir, spatial.world_position, spatial.world_normal, spatial.diffuse_brdf, &rng);
    var combined_reservoir = merge_result.merged_reservoir;
@if(GI_ADAPTIVE_SPATIAL) {
    // A pixel with a short history (its temporal sample refused by a
    // disocclusion or a fast camera, or a frame or two old) merges a second
    // spatial neighbour: one more sample where the estimate is roughest,
    // nothing where the history carries it (docs/boiling-plan.md, Phase 3;
    // SOLARIK_GI_ADAPTIVE_CONFIDENCE is the threshold, tuning.wesl).
    if input_reservoir.confidence_weight < GI_ADAPTIVE_CONFIDENCE {
        let second = load_spatial_reservoir(global_id.xy, depth, surface.world_position, surface.world_normal, &rng);
        merge_result = merge_reservoirs(combined_reservoir, surface.world_position, surface.world_normal, surface.material.base_color / PI,
            second.reservoir, second.world_position, second.world_normal, second.diffuse_brdf, &rng);
        combined_reservoir = merge_result.merged_reservoir;
    }
}

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
@if(NRC && NRC_QUERY_AT_SPREAD && VISUALIZE_NRC_BOUNCED) {
    return;
}
@if(NRC_SPECULAR && VISUALIZE_NRC_SPECULAR) {
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
        // The cell at the sample point is made or renewed as the cache read
        // below would (the value unused): the cells live by being queried,
        // and ReSTIR DI's cell light candidates and the training pass's light
        // sample look them up without making them. Without this the cells at
        // the camera's surfaces died out under NRC (docs/nrc-integration.md,
        // "Built (6 November)").
        _ = query_two_sided_world_cache(sample_point.world_position,
            cell_key_normal(sample_point.geometric_world_normal, sample_point.triangle_world_normal), sample_point.material.diffuse_transmission,
            view.world_position, ray.t, WORLD_CACHE_CELL_LIFETIME, rng);
@if(NRC_QUERY_AT_SPREAD) {
        // NVIDIA's query rule: at the sample point only once the path's
        // footprint there is wide enough, else one bounce more; the radiance
        // then holds what that bounce gathered itself (scene units).
        reservoir.radiance = nrc_query_at_spread(world_position, world_normal, sample_point,
            ray_direction, ray.t, direction_sample.inverse_pdf, rng);
}
@if(!(NRC_QUERY_AT_SPREAD)) {
        write_nrc_query(sample_point, -ray_direction);
}
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
// wait for), `wo` toward the receiver. (The record itself is nrc_query.wesl's,
// shared with specular_gi's query.)
@if((NRC))
fn write_nrc_query(sample_point: ResolvedRayHitFull, wo: vec3<f32>) {
    // (the path's own throughput is 1: the reservoir weights the connection
    // to the receiver itself)
    write_nrc_query_with_throughput(sample_point, wo, vec3(1.0));
}

// The query record for a surface seen from `wo`, at the end of a path prefix
// whose throughput (from the sample point on) is `throughput`.
@if((NRC))
fn write_nrc_query_with_throughput(sample_point: ResolvedRayHitFull, wo: vec3<f32>, throughput: vec3<f32>) {
    let record = write_nrc_query_record(sample_point, wo, throughput);
    nrc_query_index = record.query_index;
    nrc_query_prefix = nrc_encode_log_luv_hdr(record.prefix_throughput);
}

// NRC_QUERY_AT_SPREAD: NrcUpdateOnHit in query mode with its termination
// heuristic (vendor/NRC/Include/Nrc.hlsli). The cache's answer at a point is
// a blur over about the cache's resolution there; asked with a footprint
// smaller than that (a sample point a metre or two from the receiver) it
// shows as blotches. So the path's spread is computed as the training pass
// computes it (the primary vertex is the receiver seen from the camera, the
// first bounce the sample point), and only once it has passed
// termination_heuristic_threshold times the primary spread is the query made
// at the sample point. Otherwise one more bounce, and at most one: the
// sample point's direct light by the cache's resampled light sampler and the
// sun's own sample (as the training pass takes its NEE), a BRDF-sampled ray,
// and the query at its hit (no emission added there: the resampled NEE
// covers every emissive light, as the training's ris_nee). Returns the
// radiance the bounce gathered itself (scene units), zero when the query is
// made at the sample point. The extra bounce is not taken (the query made at
// the sample point) for a mirror or a leaf (the training records no mirror
// vertex, and takes a leaf's NEE with the uniform sampler, whose MIS this has
// no pair for), without a cache cell (no light slots), or when the BRDF
// sample fails.
@if((NRC && NRC_QUERY_AT_SPREAD))
fn nrc_query_at_spread(world_position: vec3<f32>, world_normal: vec3<f32>, sample_point: ResolvedRayHitFull,
    ray_direction: vec3<f32>, sample_distance: f32, inverse_pdf: f32, rng: ptr<function, u32>) -> vec3<f32> {
    let wo = -ray_direction;
    let material = sample_point.material;
    let normal = sample_point.world_normal;

    // The spread: the primary footprint from the camera distance and cos
    // gamma, then the growth to the sample point from the sample's pdf.
    var state = NrcPathState(0u, 0xffffffffu, 0.0, 0.0, 0u, 0.0);
    let primary_distance = distance(view.world_position, world_position);
    let primary_cos_gamma = abs(dot(normalize(view.world_position - world_position), world_normal));
    state.primary_spread_radius = primary_distance / sqrt(primary_cos_gamma / (4.0 * PI));
    let cos_gamma = abs(dot(wo, normal));
    // (cos_gamma times the pdf, the inverse of the sample's inverse pdf)
    state.cumul_spread_radius = sample_distance / sqrt(cos_gamma / inverse_pdf);

    let delta = material.roughness <= MIRROR_ROUGHNESS_THRESHOLD && material.metallic > 0.9999;
    if nrc_evaluate_termination_heuristic(state, nrc_constants.termination_heuristic_threshold)
        || delta || material.diffuse_transmission > 0.0 {
        write_nrc_query(sample_point, wo);
        return vec3(0.0);
    }
    let cell = find_world_cache_cell(sample_point.world_position, normal, view.world_position, rng);
    if cell < 0 {
        write_nrc_query(sample_point, wo);
        return vec3(0.0);
    }
    let next = evaluate_and_sample_brdf(wo, normal, material, rng);
    if next.pdf <= 0.0 {
        write_nrc_query(sample_point, wo);
        return vec3(0.0);
    }
    nrc_bounced = true;

    // NEE at the sample point, as the training pass's (the slots only read).
    var radiance = vec3(0.0);
    if arrayLength(&light_sources) > 0u {
        let slots = world_cache_light_slots[cell];
        let slot_probabilities = world_cache_slot_probabilities(slots);
        let light = sample_light_ris(sample_point.world_position, normal, nrc_light_tile,
            slots, slot_probabilities, max(nrc_constants.pad2, 1u), rng);
        if any(light.radiance > vec3(0.0)) {
            radiance += light.radiance * evaluate_brdf(wo, light.wi, normal, material);
        }
        // The sun by itself (the tiles hold none).
        if arrayLength(&directional_lights) > 0u {
            let light_id = directional_lights[0].light_source_index;
            let resolved = resolve_light_sample(LightSample(light_id << 16u, rand_u(rng)), light_sources[light_id]);
            let sun = calculate_resolved_light_contribution(resolved, sample_point.world_position, normal);
            let sun_radiance = sun.radiance * sun.inverse_pdf;
            if any(sun_radiance > vec3(0.0)) && dot(sun.wi, normal) > 0.0 {
                let shadow = trace_light_transmission_with_support(sample_point.world_position + (normal * RAY_T_MIN), resolved.world_position);
                radiance += sun_radiance * shadow.energy.rgb * evaluate_brdf(wo, sun.wi, normal, material);
            }
        }
    }

    // The bounce.
    let wi = next.wi;
    let origin = offset_thin_glass_ray(sample_point.world_position, sample_point.geometric_world_normal, wi, RAY_T_MIN);
    let bounce = trace_ray(origin, wi, RAY_T_MIN, ray_max_distance(), RAY_FLAG_NONE);
    if scene_hit_is_miss(bounce) {
        // The sky ends the path: no query.
        return radiance + sample_sky(wi) * next.throughput;
    }
    let hit = resolve_ray_hit_full(bounce);
    write_nrc_query_with_throughput(hit, -wi, next.throughput);
    return radiance;
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
