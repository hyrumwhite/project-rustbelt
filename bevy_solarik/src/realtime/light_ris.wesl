import bevy_core_pipeline::tonemapping::tonemapping_luminance as luminance;
import bevy_pbr::render::utils::rand_f;
import bevy_pbr::render::utils::rand_range_u;
import bevy_pbr::render::utils::rand_u;
import bevy_solarik::realtime::presample_light_tiles::unpack_resolved_light_sample;
import bevy_solarik::realtime::presample_light_tiles::light_tile_seed;
import bevy_solarik::scene::sampling::calculate_resolved_light_contribution;
import bevy_solarik::scene::sampling::resolve_light_sample;
import bevy_solarik::scene::sampling::LightSample;
import bevy_solarik::scene::sampling::LIGHT_SAMPLE_DIRECTIONAL;
import bevy_solarik::scene::sampling::trace_light_transmission_with_support;
import bevy_solarik::scene::raytracing_scene_bindings::light_sources;
import bevy_solarik::scene::raytracing_scene_bindings::directional_lights;
import bevy_solarik::scene::raytracing_scene_bindings::has_sun;
import bevy_solarik::scene::raytracing_scene_bindings::LIGHT_SOURCE_KIND_DIRECTIONAL;
import bevy_solarik::scene::raytracing_scene_bindings::RAY_T_MIN;
import bevy_solarik::realtime::world_cache_query::WORLD_CACHE_DIRECT_LIGHT_SAMPLE_COUNT;
import bevy_solarik::realtime::world_cache_query::WORLD_CACHE_NO_LIGHT;
import bevy_solarik::realtime::world_cache_query::WORLD_CACHE_SLOT_LIGHT_SAMPLE_COUNT;
import bevy_solarik::realtime::world_cache_query::world_cache_slot_light;
import bevy_solarik::realtime::world_cache_query::world_cache_slot_light_probability;
import bevy_solarik::realtime::world_cache_query::world_cache_draw_slot;
import bevy_solarik::realtime::realtime_bindings::light_tile_resolved_samples;
import bevy_solarik::realtime::realtime_bindings::light_tile_samples;
import bevy_solarik::realtime::realtime_bindings::view;
import bevy_solarik::realtime::realtime_bindings::constants;

// One light sample by resampled importance sampling, as the world cache's
// update takes its direct light (world_cache_update.wesl's sample_di) and the
// NRC training pass its next-event estimation (nrc_training.wesl): the sun
// left out (each takes one sample of its own), everything else from the
// presampled light tiles and a cell's light slots.
enable wgpu_ray_query;

// What sample_light_ris picked. Scene units (not exposed), shadow-tested,
// times the reservoir's unbiased contribution weight: zero when nothing was
// picked or the pick was in shadow.
struct RisLightSample {
    // The irradiance estimate: radiance times the receiver cosine.
    contribution: vec3<f32>,
    // The same without the receiver cosine, for a caller that applies a BRDF
    // of its own (whose cosine is in it).
    radiance: vec3<f32>,
    // Toward the picked point on the light.
    wi: vec3<f32>,
    // The shadow ray's furthest geometry that could have affected the sample.
    support_radius: f32,
    // The light picked, if in sight (else WORLD_CACHE_NO_LIGHT), and its
    // shadow-tested irradiance estimate (exposed), for the slots.
    seen_light: u32,
    seen_weight: f32,
}

// Candidates from the tiles and from the cell's slots (`slots`, drawn by
// `slot_probabilities`), each weighted by the balance heuristic of the two
// (as restir_di's generate_initial_reservoir does); the light picked, if in
// sight, is `seen_light`, and `seen_weight` its shadow-tested irradiance
// estimate (exposed), for the slots. Returns the irradiance contribution and
// the support radius.
fn sample_random_light_ris(world_position: vec3<f32>, world_normal: vec3<f32>, workgroup_id: vec2<u32>, slots: vec4<u32>, slot_probabilities: vec4<f32>, rng: ptr<function, u32>, seen_light: ptr<function, u32>, seen_weight: ptr<function, f32>) -> vec4<f32> {
    let sample = sample_light_ris(world_position, world_normal, workgroup_id, slots, slot_probabilities, WORLD_CACHE_DIRECT_LIGHT_SAMPLE_COUNT, rng);
    if sample.seen_light != WORLD_CACHE_NO_LIGHT {
        *seen_light = sample.seen_light;
        *seen_weight = sample.seen_weight;
    }
    return vec4(sample.contribution, sample.support_radius);
}

// sample_random_light_ris with everything it learned: also the pick's
// direction and its radiance without the receiver cosine. The slots are only
// read. `tile_candidates` is how many candidates come from the tile (the
// cache update takes WORLD_CACHE_DIRECT_LIGHT_SAMPLE_COUNT; a caller with
// many samples to take, as the NRC training pass, fewer: the estimate stays
// unbiased, its variance grows).
fn sample_light_ris(world_position: vec3<f32>, world_normal: vec3<f32>, workgroup_id: vec2<u32>, slots: vec4<u32>, slot_probabilities: vec4<f32>, tile_candidates: u32, rng: ptr<function, u32>) -> RisLightSample {
    var workgroup_rng = light_tile_seed((workgroup_id.x * 5782582u) + workgroup_id.y, constants.frame_index);
    let light_tile_start = rand_range_u(128u, &workgroup_rng) * 1024u;

    var sun_share = 1.0;
    if has_sun() {
        let sun_probability = light_sources[directional_lights[0].light_source_index].selection_probability;
        if sun_probability < 0.999 { sun_share = 1.0 - sun_probability; }
    }
    let slot_count = select(0u, WORLD_CACHE_SLOT_LIGHT_SAMPLE_COUNT, any(slot_probabilities > vec4(0.0)));
    let tile_count = f32(tile_candidates);

    var weight_sum = 0.0;
    var selected_sample_radiance = vec3(0.0);
    var selected_light_radiance = vec3(0.0);
    var selected_wi = vec3(0.0);
    var selected_sample_target_function = 0.0;
    var selected_sample_world_position = vec4(0.0);
    var selected_light = WORLD_CACHE_NO_LIGHT;
    var selected_light_inverse_pdf = 0.0;
    for (var i = 0u; i < tile_candidates; i++) {
        let tile_sample = light_tile_start + rand_range_u(1024u, rng);
        let resolved_light_sample = unpack_resolved_light_sample(light_tile_resolved_samples[tile_sample], view.exposure);
        // (the sun has a sample of its own, sample_di)
        if resolved_light_sample.world_position.w == LIGHT_SAMPLE_DIRECTIONAL { continue; }
        let light_contribution = calculate_resolved_light_contribution(resolved_light_sample, world_position, world_normal);

        let contribution = light_contribution.radiance * saturate(dot(light_contribution.wi, world_normal));
        let target_function = luminance(contribution);
        if target_function <= 0.0 { continue; }
        let light_id = light_tile_samples[tile_sample].light_id >> 16u;
        let tile_probability = light_sources[light_id].selection_probability / sun_share;
        // (the point's density on its light: the tile's, without the light's
        // selection)
        let light_inverse_pdf = light_contribution.inverse_pdf * tile_probability;
        let slot_probability = world_cache_slot_light_probability(slots, slot_probabilities, light_id);
        // (DI_SLOT_VISIBILITY_PRIOR: a light the slots don't know, weighed
        // as mostly hidden, as restir_di's candidates; the slots are updated
        // by what the shadow ray finds, so an unknown light still enters)
        var prior = 1.0;
@if(DI_SLOT_VISIBILITY_PRIOR) {
        if slot_count > 0u && slot_probability <= 0.0 { prior = 0.1; }
}
        let resampling_weight = prior * target_function * light_inverse_pdf / (tile_count * tile_probability + f32(slot_count) * slot_probability);

        weight_sum += resampling_weight;

        if rand_f(rng) < resampling_weight / weight_sum {
            selected_sample_radiance = contribution;
            selected_light_radiance = light_contribution.radiance;
            selected_wi = light_contribution.wi;
            selected_sample_target_function = prior * target_function;
            selected_sample_world_position = resolved_light_sample.world_position;
            selected_light = light_id;
            selected_light_inverse_pdf = light_inverse_pdf;
        }
    }
    for (var i = 0u; i < slot_count; i++) {
        let light_id = world_cache_slot_light(slots[world_cache_draw_slot(slot_probabilities, rand_f(rng))]);
        if light_id >= arrayLength(&light_sources) { continue; }
        let light_source = light_sources[light_id];
        if light_source.kind == LIGHT_SOURCE_KIND_DIRECTIONAL { continue; }
        var triangle_id = 0u;
        if (light_source.kind & 1u) == 0u {
            triangle_id = rand_range_u(light_source.kind >> 1u, rng);
        }
        let resolved_light_sample = resolve_light_sample(LightSample((light_id << 16u) | triangle_id, rand_u(rng)), light_source);
        let light_contribution = calculate_resolved_light_contribution(resolved_light_sample, world_position, world_normal);

        let contribution = light_contribution.radiance * saturate(dot(light_contribution.wi, world_normal));
        let target_function = luminance(contribution);
        if target_function <= 0.0 { continue; }
        let tile_probability = light_source.selection_probability / sun_share;
        let slot_probability = world_cache_slot_light_probability(slots, slot_probabilities, light_id);
        let resampling_weight = target_function * light_contribution.inverse_pdf / (tile_count * tile_probability + f32(slot_count) * slot_probability);

        weight_sum += resampling_weight;

        if rand_f(rng) < resampling_weight / weight_sum {
            selected_sample_radiance = contribution;
            selected_light_radiance = light_contribution.radiance;
            selected_wi = light_contribution.wi;
            selected_sample_target_function = target_function;
            selected_sample_world_position = resolved_light_sample.world_position;
            selected_light = light_id;
            selected_light_inverse_pdf = light_contribution.inverse_pdf;
        }
    }

    var support_radius = 0.0;
    var unbiased_contribution_weight = 0.0;
    var seen_light = WORLD_CACHE_NO_LIGHT;
    var seen_weight = 0.0;
    if any(selected_sample_radiance > vec3(0.0)) {
        let inverse_target_function = select(0.0, 1.0 / selected_sample_target_function, selected_sample_target_function > 0.0);
        unbiased_contribution_weight = weight_sum * inverse_target_function;

        let shadow = trace_light_transmission_with_support(world_position + (world_normal * RAY_T_MIN), selected_sample_world_position);
        selected_sample_radiance *= shadow.energy.rgb;
        selected_light_radiance *= shadow.energy.rgb;
        support_radius = shadow.distance + RAY_T_MIN;
        if any(selected_sample_radiance > vec3(0.0)) {
            seen_light = selected_light;
            // (the light's irradiance here, estimated from this point on it)
            seen_weight = luminance(selected_sample_radiance) * selected_light_inverse_pdf * view.exposure;
        }
    }

    return RisLightSample(selected_sample_radiance * unbiased_contribution_weight,
        selected_light_radiance * unbiased_contribution_weight, selected_wi, support_radius,
        seen_light, seen_weight);
}
