import bevy_solarik::filtered_canopy_scene::forest_set_seed;
import bevy_solarik::scene::scene_hit::scene_hit_is_miss;
@if((RAY_MATERIAL_FOOTPRINTS)) import bevy_solarik::scene::raytracing_scene_bindings::primary_ray_cone;
@if((RAY_MATERIAL_FOOTPRINTS)) import bevy_solarik::scene::raytracing_scene_bindings::advance_ray_cone;
@if((RAY_MATERIAL_FOOTPRINTS)) import bevy_solarik::scene::raytracing_scene_bindings::resolve_ray_hit_filtered;
import bevy_core_pipeline::tonemapping::tonemapping_luminance as luminance;
import bevy_pbr::render::utils::rand_f;
import bevy_pbr::render::utils::rand_range_u;
import bevy_pbr::render::utils::sample_cosine_hemisphere;
import bevy_render::maths::PI;
import bevy_render::view::View;
import bevy_solarik::realtime::presample_light_tiles::ResolvedLightSamplePacked;
import bevy_solarik::realtime::presample_light_tiles::unpack_resolved_light_sample;
import bevy_solarik::realtime::presample_light_tiles::light_tile_seed;
import bevy_solarik::scene::sampling::calculate_resolved_light_contribution;
import bevy_solarik::scene::sampling::resolve_light_sample;
import bevy_solarik::scene::sampling::LightSample;
import bevy_solarik::scene::sampling::LIGHT_SAMPLE_DIRECTIONAL;
import bevy_solarik::scene::raytracing_scene_bindings::light_sources;
import bevy_solarik::scene::raytracing_scene_bindings::directional_lights;
import bevy_pbr::render::utils::rand_u;
import bevy_solarik::scene::sampling::trace_light_transmission_with_support;
import bevy_solarik::scene::sampling::trace_shadow_transmission_impl;
import bevy_solarik::scene::raytracing_scene_bindings::trace_ray;
import bevy_solarik::scene::raytracing_scene_bindings::resolve_ray_hit_full;
import bevy_solarik::scene::raytracing_scene_bindings::sample_sky;
import bevy_solarik::scene::raytracing_scene_bindings::RAY_T_MIN;
import bevy_solarik::scene::raytracing_scene_bindings::ray_max_distance;
import bevy_solarik::realtime::world_cache_priority::selected_world_cache_cell_count;
import bevy_solarik::realtime::world_cache_priority::PRIORITY_UPDATED_COUNT;
import bevy_solarik::realtime::world_cache_query::WORLD_CACHE_MAX_TEMPORAL_SAMPLES;
import bevy_solarik::realtime::world_cache_query::WORLD_CACHE_DIRECT_LIGHT_SAMPLE_COUNT;
import bevy_solarik::realtime::world_cache_query::WORLD_CACHE_DIRECT_LIGHT_SAMPLES_PER_UPDATE;
import bevy_solarik::realtime::world_cache_query::WORLD_CACHE_HISTORY_FRAMES;
import bevy_solarik::realtime::world_cache_query::WORLD_CACHE_MAX_GI_RAY_DISTANCE;
import bevy_solarik::realtime::world_cache_query::WORLD_CACHE_CELL_UPDATES_SOFT_CAP;
import bevy_solarik::realtime::world_cache_query::query_two_sided_world_cache_with_support;
import bevy_solarik::realtime::realtime_bindings::light_tile_resolved_samples;
import bevy_solarik::realtime::realtime_bindings::light_tile_samples;
import bevy_solarik::realtime::realtime_bindings::world_cache_light_slots;
import bevy_solarik::realtime::world_cache_query::WORLD_CACHE_NO_LIGHT;
import bevy_solarik::realtime::world_cache_query::WORLD_CACHE_SLOT_FADE;
import bevy_solarik::realtime::world_cache_query::WORLD_CACHE_SLOT_LIGHT_SAMPLE_COUNT;
import bevy_solarik::realtime::world_cache_query::world_cache_slot;
import bevy_solarik::realtime::world_cache_query::world_cache_slot_light;
import bevy_solarik::realtime::world_cache_query::world_cache_slot_weight;
import bevy_solarik::realtime::world_cache_query::world_cache_slot_probabilities;
import bevy_solarik::realtime::world_cache_query::world_cache_slot_light_probability;
import bevy_solarik::realtime::world_cache_query::world_cache_draw_slot;
import bevy_solarik::realtime::world_cache_query::world_cache_slots_found;
import bevy_solarik::scene::raytracing_scene_bindings::LIGHT_SOURCE_KIND_DIRECTIONAL;
import bevy_solarik::realtime::realtime_bindings::view;
import bevy_solarik::realtime::realtime_bindings::constants;
import bevy_solarik::realtime::realtime_bindings::world_cache_active_cells_count;
import bevy_solarik::realtime::realtime_bindings::world_cache_a;
import bevy_solarik::realtime::realtime_bindings::world_cache_b;
import bevy_solarik::realtime::realtime_bindings::world_cache_life;
import bevy_solarik::realtime::realtime_bindings::world_cache_geometry_data;
import bevy_solarik::realtime::realtime_bindings::world_cache_radiance;
import bevy_solarik::realtime::realtime_bindings::world_cache_luminance_deltas;
import bevy_solarik::realtime::realtime_bindings::world_cache_active_cells_new_radiance;

enable wgpu_ray_query;




@compute @workgroup_size(64, 1, 1)
fn sample_di(@builtin(workgroup_id) workgroup_id: vec3<u32>, @builtin(global_invocation_id) active_cell_id: vec3<u32>) {
    if active_cell_id.x >= selected_world_cache_cell_count() { return; }

    let cell_index = world_cache_a[active_cell_id.x];
    let geometry_data = world_cache_geometry_data[cell_index];
    var rng = cell_index + constants.frame_index;
    forest_set_seed(vec2u(cell_index,constants.frame_index^0x4173bda1u));

    // Several shadow-tested lights an update, each from a tile of its own: a
    // cell's single light sample (one of the level's hundreds of emitters)
    // was noisy enough to set the whole cache, which feeds itself, drifting.
    // (each light found in sight goes in the cell's slots by how much it
    // lights the cell, world_cache_slots_found; the search draws candidates
    // from them too, as ReSTIR DI does: the lights that reach this place)
    var new_radiance = vec4(0.0);
    let found_slots = world_cache_light_slots[cell_index];
    let slot_probabilities = world_cache_slot_probabilities(found_slots);
    var light_slots = found_slots;
    for (var j = 0u; j < 4u; j++) {
        if world_cache_slot_light(light_slots[j]) != WORLD_CACHE_NO_LIGHT {
            light_slots[j] = world_cache_slot(light_slots[j], world_cache_slot_weight(light_slots[j]) * WORLD_CACHE_SLOT_FADE);
        }
    }
    for (var i = 0u; i < WORLD_CACHE_DIRECT_LIGHT_SAMPLES_PER_UPDATE; i++) {
        var seen_light = WORLD_CACHE_NO_LIGHT;
        var seen_weight = 0.0;
        let sample = sample_random_light_ris(geometry_data.world_position, geometry_data.world_normal, workgroup_id.xy + vec2(0u, i * 7919u), found_slots, slot_probabilities, &rng, &seen_light, &seen_weight);
        new_radiance = vec4(new_radiance.rgb + sample.rgb, max(new_radiance.w, sample.w));
        if seen_light != WORLD_CACHE_NO_LIGHT {
            light_slots = world_cache_slots_found(light_slots, seen_light, seen_weight);
        }
    }
    world_cache_light_slots[cell_index] = light_slots;
    new_radiance = vec4(new_radiance.rgb / f32(WORLD_CACHE_DIRECT_LIGHT_SAMPLES_PER_UPDATE), new_radiance.w);
    // The sun by itself, one shadow-tested sample an update (the tiles hold
    // none, presample_light_tiles: indoors, behind the hull, the sun's
    // fixed share of them took half the candidates and lit nothing)
    if arrayLength(&directional_lights) > 0u {
        let light_id = directional_lights[0].light_source_index;
        let resolved = resolve_light_sample(LightSample(light_id << 16u, rand_u(&rng)), light_sources[light_id]);
        let sun = calculate_resolved_light_contribution(resolved, geometry_data.world_position, geometry_data.world_normal);
        let contribution = sun.radiance * saturate(dot(sun.wi, geometry_data.world_normal)) * sun.inverse_pdf;
        if any(contribution > vec3(0.0)) {
            let shadow = trace_light_transmission_with_support(geometry_data.world_position + (geometry_data.world_normal * RAY_T_MIN), resolved.world_position);
            new_radiance = vec4(new_radiance.rgb + contribution * shadow.energy.rgb, new_radiance.w);
        }
    }

    world_cache_active_cells_new_radiance[active_cell_id.x] = new_radiance;
}

@compute @workgroup_size(64, 1, 1)
fn sample_gi(@builtin(workgroup_id) workgroup_id: vec3<u32>, @builtin(global_invocation_id) active_cell_id: vec3<u32>) {
    if active_cell_id.x >= selected_world_cache_cell_count() { return; }

    let cell_index = world_cache_a[active_cell_id.x];
    let geometry_data = world_cache_geometry_data[cell_index];
    var rng = cell_index + constants.frame_index;
    forest_set_seed(vec2u(cell_index,constants.frame_index^0x79b4da17u));

    let ray_direction = sample_cosine_hemisphere(geometry_data.world_normal, &rng);
    // Traced to the end of the world so that geometry beyond the GI ray
    // distance is not mistaken for sky; such far hits contribute nothing, as
    // upstream's shorter ray had them.
    let ray = trace_ray(geometry_data.world_position + (geometry_data.world_normal * RAY_T_MIN), ray_direction, RAY_T_MIN, ray_max_distance(), RAY_FLAG_NONE);
    // trace_ray skips thin panes when finding the endpoint. Apply their energy
    // to this new cache sample once, excluding the endpoint itself.
    let connection_distance = select(ray.t - RAY_T_MIN, ray_max_distance(), scene_hit_is_miss(ray));
    var pending = world_cache_active_cells_new_radiance[active_cell_id.x];
    pending.w = max(pending.w, connection_distance + 2.0 * RAY_T_MIN);
    let transmission = trace_shadow_transmission_impl(geometry_data.world_position + geometry_data.world_normal * RAY_T_MIN, ray_direction, connection_distance, false).rgb;
    if scene_hit_is_miss(ray) {
        // Escaped to the sky: cosine sampling turns radiance into irradiance
        // with a factor of pi, the same units the cached direct light is in.
        pending = vec4(pending.rgb + transmission * PI * sample_sky(ray_direction), pending.w);
    } else if ray.t <= WORLD_CACHE_MAX_GI_RAY_DISTANCE {
@if(RAY_MATERIAL_FOOTPRINTS) {
        let cone = primary_ray_cone(view.clip_from_view, view.main_pass_viewport.zw, distance(geometry_data.world_position, view.world_position) + ray.t);
        let ray_hit = resolve_ray_hit_filtered(ray, ray_direction, cone);
}
@if(!(RAY_MATERIAL_FOOTPRINTS)) {
        let ray_hit = resolve_ray_hit_full(ray);
}
        let cell_life = atomicLoad(&world_cache_life[cell_index]);
        let radiance = query_two_sided_world_cache_with_support(ray_hit.world_position, ray_hit.geometric_world_normal, ray_hit.material.diffuse_transmission, view.world_position, ray.t, cell_life, &rng);
        pending = vec4(pending.rgb + transmission * ray_hit.material.base_color * radiance.rgb,
            max(pending.w, distance(geometry_data.world_position, ray_hit.world_position) + radiance.w));
    }
    world_cache_active_cells_new_radiance[active_cell_id.x] = pending;
}

@compute @workgroup_size(64, 1, 1)
fn blend_new_samples(@builtin(global_invocation_id) active_cell_id: vec3<u32>) {
    let selected_count = selected_world_cache_cell_count();
    if active_cell_id.x >= selected_count { return; }

    let cell_index = world_cache_a[active_cell_id.x];

    let old_radiance = world_cache_radiance[cell_index];
    let pending = world_cache_active_cells_new_radiance[active_cell_id.x];
    let new_radiance = pending.rgb;
    let luminance_delta = world_cache_luminance_deltas[cell_index];

    // https://bsky.app/profile/gboisse.bsky.social/post/3m5blga3ftk2a
    let sample_count = min(old_radiance.a + 1.0, WORLD_CACHE_MAX_TEMPORAL_SAMPLES);
    let update_probability = min(1.0, f32(WORLD_CACHE_CELL_UPDATES_SOFT_CAP) / f32(world_cache_active_cells_count));
    var blend_amount = cache_blend_amount(sample_count, luminance(old_radiance.rgb), luminance_delta, update_probability);
    if (constants.reset & 1u) != 0u {
        blend_amount = 1.0;
    }

    let blended_radiance = mix(old_radiance.rgb, new_radiance, blend_amount);
    let blended_luminance_delta = select(mix(luminance_delta, luminance(blended_radiance) - luminance(old_radiance.rgb), 1.0 / 8.0), 0.0, (constants.reset & 1u) != 0u);

    // Exponential history retains old contributions unless fully replaced.
    // Round support outward; nonfinite support deliberately fails local reuse.
    let new_support = pending.w * 1.00001 + 0.01;
    let old_support = world_cache_geometry_data[cell_index].support_radius;
    world_cache_geometry_data[cell_index].support_radius = select(
        max(old_support, new_support), new_support, blend_amount == 1.0);
    world_cache_radiance[cell_index] = vec4(blended_radiance, sample_count);
    world_cache_luminance_deltas[cell_index] = blended_luminance_delta;
    world_cache_geometry_data[cell_index].last_traced_frame = constants.render_frame;
    if active_cell_id.x % 64u == 0u {
        atomicAdd(&world_cache_b[PRIORITY_UPDATED_COUNT], min(64u, selected_count - active_cell_id.x));
    }
}

// Convert the history horizon from rendered frames to successful cell updates.
// Otherwise the update budget silently stretches history as the cache grows.
fn cache_blend_amount(sample_count: f32, old_luminance: f32, luminance_delta: f32, update_probability: f32) -> f32 {
    // The history shortens as the cell's light changes: in proportion to the
    // change (an eighth root made every single-sample update's noise read as
    // a change, keeping a few samples where 32 were meant).
    let alpha = abs(luminance_delta) / max(old_luminance, 0.001);
    let max_sample_count = mix(WORLD_CACHE_MAX_TEMPORAL_SAMPLES, 1.0, saturate(alpha));
    // (and at most WORLD_CACHE_HISTORY_FRAMES frames' updates: at 16, a big
    // cache, a cell updated one frame in four, kept four samples)
    let frame_limited_samples = max(1.0, WORLD_CACHE_HISTORY_FRAMES * clamp(update_probability, 0.0, 1.0));
    return 1.0 / min(sample_count, min(max_sample_count, frame_limited_samples));
}

// Candidates from the tiles and from the cell's slots (`slots`, drawn by
// `slot_probabilities`), each weighted by the balance heuristic of the two
// (as restir_di's generate_initial_reservoir does); the light picked, if in
// sight, is `seen_light`, and `seen_weight` its shadow-tested irradiance
// estimate (exposed), for the slots.
fn sample_random_light_ris(world_position: vec3<f32>, world_normal: vec3<f32>, workgroup_id: vec2<u32>, slots: vec4<u32>, slot_probabilities: vec4<f32>, rng: ptr<function, u32>, seen_light: ptr<function, u32>, seen_weight: ptr<function, f32>) -> vec4<f32> {
    var workgroup_rng = light_tile_seed((workgroup_id.x * 5782582u) + workgroup_id.y, constants.frame_index);
    let light_tile_start = rand_range_u(128u, &workgroup_rng) * 1024u;

    var sun_share = 1.0;
    if arrayLength(&directional_lights) > 0u {
        let sun_probability = light_sources[directional_lights[0].light_source_index].selection_probability;
        if sun_probability < 0.999 { sun_share = 1.0 - sun_probability; }
    }
    let slot_count = select(0u, WORLD_CACHE_SLOT_LIGHT_SAMPLE_COUNT, any(slot_probabilities > vec4(0.0)));
    let tile_count = f32(WORLD_CACHE_DIRECT_LIGHT_SAMPLE_COUNT);

    var weight_sum = 0.0;
    var selected_sample_radiance = vec3(0.0);
    var selected_sample_target_function = 0.0;
    var selected_sample_world_position = vec4(0.0);
    var selected_light = WORLD_CACHE_NO_LIGHT;
    var selected_light_inverse_pdf = 0.0;
    for (var i = 0u; i < WORLD_CACHE_DIRECT_LIGHT_SAMPLE_COUNT; i++) {
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
        let resampling_weight = target_function * light_inverse_pdf / (tile_count * tile_probability + f32(slot_count) * slot_probability);

        weight_sum += resampling_weight;

        if rand_f(rng) < resampling_weight / weight_sum {
            selected_sample_radiance = contribution;
            selected_sample_target_function = target_function;
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
            selected_sample_target_function = target_function;
            selected_sample_world_position = resolved_light_sample.world_position;
            selected_light = light_id;
            selected_light_inverse_pdf = light_contribution.inverse_pdf;
        }
    }

    var support_radius = 0.0;
    var unbiased_contribution_weight = 0.0;
    if any(selected_sample_radiance > vec3(0.0)) {
        let inverse_target_function = select(0.0, 1.0 / selected_sample_target_function, selected_sample_target_function > 0.0);
        unbiased_contribution_weight = weight_sum * inverse_target_function;

        let shadow = trace_light_transmission_with_support(world_position + (world_normal * RAY_T_MIN), selected_sample_world_position);
        selected_sample_radiance *= shadow.energy.rgb;
        support_radius = shadow.distance + RAY_T_MIN;
        if any(selected_sample_radiance > vec3(0.0)) {
            *seen_light = selected_light;
            // (the light's irradiance here, estimated from this point on it)
            *seen_weight = luminance(selected_sample_radiance) * selected_light_inverse_pdf * view.exposure;
        }
    }

    return vec4(selected_sample_radiance * unbiased_contribution_weight, support_radius);
}
