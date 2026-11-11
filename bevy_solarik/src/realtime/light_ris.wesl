import bevy_core_pipeline::tonemapping::tonemapping_luminance as luminance;
import bevy_pbr::render::utils::rand_f;
import bevy_pbr::render::utils::rand_range_u;
import bevy_pbr::render::utils::rand_u;
import bevy_solarik::realtime::presample_light_tiles::unpack_resolved_light_sample;
import bevy_solarik::realtime::presample_light_tiles::light_tile_seed;
import bevy_solarik::scene::sampling::calculate_resolved_light_contribution;
import bevy_solarik::scene::sampling::resolve_light_sample;
import bevy_solarik::scene::sampling::ResolvedLightSample;
import bevy_solarik::scene::sampling::LightSample;
import bevy_solarik::scene::sampling::NULL_LIGHT_ID;
import bevy_solarik::scene::sampling::NearLightCandidate;
import bevy_solarik::scene::sampling::light_may_be_near;
import bevy_solarik::scene::sampling::near_light_candidate;
import bevy_solarik::scene::sampling::resolve_near_light_sample;
import bevy_solarik::scene::sampling::is_finite_f32;
import bevy_solarik::scene::sampling::light_triangle_may_be_near;
import bevy_solarik::scene::sampling::resolve_emissive_light_sample;
import bevy_solarik::realtime::light_triangles::load_light_triangle;
import bevy_solarik::scene::raytracing_scene_bindings::LightSource;
import bevy_solarik::scene::raytracing_scene_bindings::light_source_is_emissive_mesh;
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

    var pick = RisPick(0.0, vec3(0.0), vec3(0.0), vec3(0.0), 0.0, vec4(0.0), WORLD_CACHE_NO_LIGHT, 0.0);
    let candidates = RisCandidates(world_position, world_normal, slots, slot_probabilities, slot_count, tile_count, sun_share);
@if((!(LIGHT_NEAR_INLINE || VISUALIZE_LIGHT_NEAR || LIGHT_NEAR_AREA || LIGHT_NEAR_CACHE_AREA))) {
    var deferred_tiles: array<u32, RIS_DEFERRED>;
    var deferred_tile_count = 0u;
    var deferred_slots: array<LightSample, RIS_DEFERRED>;
    var deferred_slot_count = 0u;
}
    for (var i = 0u; i < tile_candidates; i++) {
        let tile_sample = light_tile_start + rand_range_u(1024u, rng);
        let tile_resolved = unpack_resolved_light_sample(light_tile_resolved_samples[tile_sample], view.exposure);
        // (the sun has a sample of its own, sample_di)
        if tile_resolved.world_position.w == LIGHT_SAMPLE_DIRECTIONAL { continue; }
        // (a near triangle's point by solid angle from here: see
        // sampling.wesl's near_light_candidate; put off to the second pass,
        // the list full taken here)
        var candidate = NearLightCandidate(LightSample(NULL_LIGHT_ID, 0u), tile_resolved, tile_resolved.inverse_pdf, false);
        if ris_may_be_near(tile_resolved, world_position) {
@if((!(LIGHT_NEAR_INLINE || VISUALIZE_LIGHT_NEAR || LIGHT_NEAR_AREA || LIGHT_NEAR_CACHE_AREA))) {
            if deferred_tile_count < RIS_DEFERRED {
                deferred_tiles[deferred_tile_count] = tile_sample;
                deferred_tile_count += 1u;
                continue;
            }
}
            let tile_light_sample = light_tile_samples[tile_sample];
            candidate = near_light_candidate(tile_light_sample, light_sources[tile_light_sample.light_id >> 16u], tile_resolved, world_position);
        }
        stream_ris_tile_candidate(candidates, candidate, tile_sample, &pick, rng);
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
        let light_sample = LightSample((light_id << 16u) | triangle_id, rand_u(rng));
@if((!(LIGHT_NEAR_INLINE || VISUALIZE_LIGHT_NEAR || LIGHT_NEAR_AREA || LIGHT_NEAR_CACHE_AREA))) {
        if light_source_is_emissive_mesh(light_source) {
            let triangle = load_light_triangle(light_source, triangle_id);
            if deferred_slot_count < RIS_DEFERRED && light_triangle_may_be_near(triangle, world_position) {
                deferred_slots[deferred_slot_count] = light_sample;
                deferred_slot_count += 1u;
                continue;
            }
            let resolved = resolve_emissive_light_sample(light_source, triangle, light_sample.seed);
            stream_ris_slot_candidate(candidates, NearLightCandidate(light_sample, resolved, resolved.inverse_pdf, false), light_id, light_source, &pick, rng);
            continue;
        }
}
        let candidate = ris_resolve_candidate(light_sample, light_source, world_position);
        stream_ris_slot_candidate(candidates, candidate, light_id, light_source, &pick, rng);
    }
@if((!(LIGHT_NEAR_INLINE || VISUALIZE_LIGHT_NEAR || LIGHT_NEAR_AREA || LIGHT_NEAR_CACHE_AREA))) {
    // (the deferred candidates: the near path, run by the lanes that have
    // any together)
    for (var i = 0u; i < deferred_tile_count; i++) {
        let tile_sample = deferred_tiles[i];
        let tile_resolved = unpack_resolved_light_sample(light_tile_resolved_samples[tile_sample], view.exposure);
        let tile_light_sample = light_tile_samples[tile_sample];
        let candidate = near_light_candidate(tile_light_sample, light_sources[tile_light_sample.light_id >> 16u], tile_resolved, world_position);
        stream_ris_tile_candidate(candidates, candidate, tile_sample, &pick, rng);
    }
    for (var i = 0u; i < deferred_slot_count; i++) {
        let light_sample = deferred_slots[i];
        let light_id = light_sample.light_id >> 16u;
        let light_source = light_sources[light_id];
        stream_ris_slot_candidate(candidates, resolve_near_light_sample(light_sample, light_source, world_position), light_id, light_source, &pick, rng);
    }
}

    var support_radius = 0.0;
    var unbiased_contribution_weight = 0.0;
    var seen_light = WORLD_CACHE_NO_LIGHT;
    var seen_weight = 0.0;
    if any(pick.sample_radiance > vec3(0.0)) {
        let inverse_target_function = select(0.0, 1.0 / pick.target_function, pick.target_function > 0.0);
        unbiased_contribution_weight = pick.weight_sum * inverse_target_function;
        // (1 / a denormal target overflows)
        if !is_finite_f32(unbiased_contribution_weight) { unbiased_contribution_weight = 0.0; }

        let shadow = trace_light_transmission_with_support(world_position + (world_normal * RAY_T_MIN), pick.world_position);
        pick.sample_radiance *= shadow.energy.rgb;
        pick.light_radiance *= shadow.energy.rgb;
        support_radius = shadow.distance + RAY_T_MIN;
        if any(pick.sample_radiance > vec3(0.0)) {
            seen_light = pick.light;
            // (the light's irradiance here, estimated from this point on it)
            seen_weight = luminance(pick.sample_radiance) * pick.light_inverse_pdf * view.exposure;
            // (and none for the cell's slots that isn't finite)
            if !is_finite_f32(seen_weight) {
                seen_light = WORLD_CACHE_NO_LIGHT;
                seen_weight = 0.0;
            }
        }
    }

    return RisLightSample(pick.sample_radiance * unbiased_contribution_weight,
        pick.light_radiance * unbiased_contribution_weight, pick.wi, support_radius,
        seen_light, seen_weight);
}

// What sample_light_ris has picked so far, which each candidate streams into.
struct RisPick {
    weight_sum: f32,
    sample_radiance: vec3<f32>,
    light_radiance: vec3<f32>,
    wi: vec3<f32>,
    target_function: f32,
    world_position: vec4<f32>,
    light: u32,
    light_inverse_pdf: f32,
}

// What every candidate at the receiver is weighed by.
struct RisCandidates {
    world_position: vec3<f32>,
    world_normal: vec3<f32>,
    slots: vec4<u32>,
    slot_probabilities: vec4<f32>,
    slot_count: u32,
    tile_count: f32,
    sun_share: f32,
}

// The near sampling's tile pre-test and cell resolve for the cache's pick,
// or the area sampling's with LIGHT_NEAR_CACHE_AREA (a diagnostic: see
// sampling.wesl's switches).
fn ris_may_be_near(resolved: ResolvedLightSample, receiver: vec3<f32>) -> bool {
@if(LIGHT_NEAR_CACHE_AREA) {
    return false;
}
@if((!(LIGHT_NEAR_CACHE_AREA))) {
    return light_may_be_near(resolved, receiver);
}
}

fn ris_resolve_candidate(light_sample: LightSample, light_source: LightSource, receiver: vec3<f32>) -> NearLightCandidate {
@if(LIGHT_NEAR_CACHE_AREA) {
    let resolved = resolve_light_sample(light_sample, light_source);
    return NearLightCandidate(light_sample, resolved, resolved.inverse_pdf, false);
}
@if((!(LIGHT_NEAR_CACHE_AREA))) {
    return resolve_near_light_sample(light_sample, light_source, receiver);
}
}

// The most candidates of a kind the first pass puts off (the cache's 64
// tile candidates past it taken inline).
const RIS_DEFERRED: u32 = 16u;

// A tile candidate weighed and streamed into the pick (one place for the
// first pass and the second).
fn stream_ris_tile_candidate(c: RisCandidates, candidate: NearLightCandidate, tile_sample: u32, pick: ptr<function, RisPick>, rng: ptr<function, u32>) {
    let resolved_light_sample = candidate.resolved;
    let light_contribution = calculate_resolved_light_contribution(resolved_light_sample, c.world_position, c.world_normal);

    let contribution = light_contribution.radiance * saturate(dot(light_contribution.wi, c.world_normal));
    let target_function = luminance(contribution);
    if !(target_function > 0.0) || !is_finite_f32(target_function) { return; }
    let light_id = light_tile_samples[tile_sample].light_id >> 16u;
    let tile_probability = light_sources[light_id].selection_probability / c.sun_share;
    // (the point's density on its light: the tile's, without the light's
    // selection)
    let light_inverse_pdf = candidate.inverse_pdf * tile_probability;
    let slot_probability = world_cache_slot_light_probability(c.slots, c.slot_probabilities, light_id);
    // (DI_SLOT_VISIBILITY_PRIOR: a light the slots don't know, weighed
    // as mostly hidden, as restir_di's candidates; the slots are updated
    // by what the shadow ray finds, so an unknown light still enters)
    var prior = 1.0;
@if(DI_SLOT_VISIBILITY_PRIOR) {
    if c.slot_count > 0u && slot_probability <= 0.0 { prior = 0.1; }
}
    let resampling_weight = prior * target_function * light_inverse_pdf / (c.tile_count * tile_probability + f32(c.slot_count) * slot_probability);
    // (no non-finite weight into the sum: see is_finite_f32)
    if !is_finite_f32(resampling_weight) || resampling_weight < 0.0 { return; }

    (*pick).weight_sum += resampling_weight;

    if rand_f(rng) < resampling_weight / (*pick).weight_sum {
        (*pick).sample_radiance = contribution;
        (*pick).light_radiance = light_contribution.radiance;
        (*pick).wi = light_contribution.wi;
        (*pick).target_function = prior * target_function;
        (*pick).world_position = resolved_light_sample.world_position;
        (*pick).light = light_id;
        (*pick).light_inverse_pdf = light_inverse_pdf;
    }
}

// A slot candidate weighed and streamed into the pick (as above).
fn stream_ris_slot_candidate(c: RisCandidates, candidate: NearLightCandidate, light_id: u32, light_source: LightSource, pick: ptr<function, RisPick>, rng: ptr<function, u32>) {
    let resolved_light_sample = candidate.resolved;
    let light_contribution = calculate_resolved_light_contribution(resolved_light_sample, c.world_position, c.world_normal);

    let contribution = light_contribution.radiance * saturate(dot(light_contribution.wi, c.world_normal));
    let target_function = luminance(contribution);
    if !(target_function > 0.0) || !is_finite_f32(target_function) { return; }
    let tile_probability = light_source.selection_probability / c.sun_share;
    let slot_probability = world_cache_slot_light_probability(c.slots, c.slot_probabilities, light_id);
    let resampling_weight = target_function * candidate.inverse_pdf / (c.tile_count * tile_probability + f32(c.slot_count) * slot_probability);
    if !is_finite_f32(resampling_weight) || resampling_weight < 0.0 { return; }

    (*pick).weight_sum += resampling_weight;

    if rand_f(rng) < resampling_weight / (*pick).weight_sum {
        (*pick).sample_radiance = contribution;
        (*pick).light_radiance = light_contribution.radiance;
        (*pick).wi = light_contribution.wi;
        (*pick).target_function = target_function;
        (*pick).world_position = resolved_light_sample.world_position;
        (*pick).light = light_id;
        (*pick).light_inverse_pdf = candidate.inverse_pdf;
    }
}
