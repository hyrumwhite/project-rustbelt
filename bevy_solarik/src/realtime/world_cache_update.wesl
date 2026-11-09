import bevy_solarik::realtime::tuning::CACHE_HISTORY_CLAMP;
import bevy_solarik::realtime::tuning::CACHE_DROP_DETECT;
import bevy_solarik::realtime::tuning::CACHE_DROP_THRESHOLD;
import bevy_solarik::realtime::tuning::CACHE_DROP_FLOOR;
import bevy_solarik::filtered_canopy_scene::forest_set_seed;
import bevy_solarik::scene::scene_hit::scene_hit_is_miss;
@if((RAY_MATERIAL_FOOTPRINTS)) import bevy_solarik::scene::raytracing_scene_bindings::primary_ray_cone;
@if((RAY_MATERIAL_FOOTPRINTS)) import bevy_solarik::scene::raytracing_scene_bindings::advance_ray_cone;
@if((RAY_MATERIAL_FOOTPRINTS)) import bevy_solarik::scene::raytracing_scene_bindings::resolve_ray_hit_filtered;
import bevy_core_pipeline::tonemapping::tonemapping_luminance as luminance;
import bevy_pbr::render::utils::sample_cosine_hemisphere;
import bevy_render::maths::PI;
import bevy_solarik::scene::sampling::calculate_resolved_light_contribution;
import bevy_solarik::scene::sampling::resolve_light_sample;
import bevy_solarik::scene::sampling::LightSample;
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
import bevy_solarik::realtime::world_cache_query::WORLD_CACHE_DIRECT_LIGHT_SAMPLES_PER_UPDATE;
import bevy_solarik::realtime::world_cache_query::WORLD_CACHE_HISTORY_FRAMES;
import bevy_solarik::realtime::world_cache_query::WORLD_CACHE_MAX_GI_RAY_DISTANCE;
import bevy_solarik::realtime::world_cache_query::WORLD_CACHE_CELL_UPDATES_SOFT_CAP;
import bevy_solarik::realtime::world_cache_query::query_two_sided_world_cache_with_support;
import bevy_solarik::realtime::world_cache_query::cell_key_normal;
import bevy_solarik::realtime::realtime_bindings::world_cache_light_slots;
import bevy_solarik::realtime::world_cache_query::WORLD_CACHE_NO_LIGHT;
import bevy_solarik::realtime::world_cache_query::WORLD_CACHE_SLOT_FADE;
import bevy_solarik::realtime::world_cache_query::world_cache_slot;
import bevy_solarik::realtime::world_cache_query::world_cache_slot_light;
import bevy_solarik::realtime::world_cache_query::world_cache_slot_weight;
import bevy_solarik::realtime::world_cache_query::world_cache_slot_probabilities;
import bevy_solarik::realtime::world_cache_query::world_cache_slots_found;
import bevy_solarik::realtime::light_ris::sample_random_light_ris;
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

    var pending = world_cache_active_cells_new_radiance[active_cell_id.x];
    var rays = 1u;
@if(CACHE_SHADE_SECOND_RAY) {
    // A cell whose direct sample found nothing this update (the sun behind
    // the canyon wall, no emitter in reach) is lit by what its bounce ray
    // finds alone, sky above all, and one cosine ray an update of a half
    // hidden sky is a noisy estimate of it: such a cell traces two and takes
    // their mean (docs/boiling-plan.md, Phase 4; the cost is a ray on the
    // cells in shade only).
    if all(pending.rgb == vec3(0.0)) { rays = 2u; }
}
    for (var i = 0u; i < rays; i++) {
        let sample = sample_gi_ray(cell_index, geometry_data.world_position, geometry_data.world_normal, &rng);
        pending = vec4(pending.rgb + sample.rgb / f32(rays), max(pending.w, sample.w));
    }
    world_cache_active_cells_new_radiance[active_cell_id.x] = pending;
}

// One cosine-weighted bounce ray from a cell: its radiance (sky or the cache
// at the hit, through any thin panes) and the support distance it saw.
fn sample_gi_ray(cell_index: u32, world_position: vec3<f32>, world_normal: vec3<f32>, rng: ptr<function, u32>) -> vec4<f32> {
    let ray_direction = sample_cosine_hemisphere(world_normal, rng);
    // Traced to the end of the world so that geometry beyond the GI ray
    // distance is not mistaken for sky; such far hits contribute nothing, as
    // upstream's shorter ray had them.
    let ray = trace_ray(world_position + (world_normal * RAY_T_MIN), ray_direction, RAY_T_MIN, ray_max_distance(), RAY_FLAG_NONE);
    // trace_ray skips thin panes when finding the endpoint. Apply their energy
    // to this new cache sample once, excluding the endpoint itself.
    let connection_distance = select(ray.t - RAY_T_MIN, ray_max_distance(), scene_hit_is_miss(ray));
    var sample = vec4(0.0, 0.0, 0.0, connection_distance + 2.0 * RAY_T_MIN);
    let transmission = trace_shadow_transmission_impl(world_position + world_normal * RAY_T_MIN, ray_direction, connection_distance, false).rgb;
    if scene_hit_is_miss(ray) {
        // Escaped to the sky: cosine sampling turns radiance into irradiance
        // with a factor of pi, the same units the cached direct light is in.
        sample = vec4(transmission * PI * sample_sky(ray_direction), sample.w);
    } else if ray.t <= WORLD_CACHE_MAX_GI_RAY_DISTANCE {
@if(RAY_MATERIAL_FOOTPRINTS) {
        let cone = primary_ray_cone(view.clip_from_view, view.main_pass_viewport.zw, distance(world_position, view.world_position) + ray.t);
        let ray_hit = resolve_ray_hit_filtered(ray, ray_direction, cone);
}
@if(!(RAY_MATERIAL_FOOTPRINTS)) {
        let ray_hit = resolve_ray_hit_full(ray);
}
        let cell_life = atomicLoad(&world_cache_life[cell_index]);
        let radiance = query_two_sided_world_cache_with_support(ray_hit.world_position, cell_key_normal(ray_hit.geometric_world_normal, ray_hit.triangle_world_normal), ray_hit.material.diffuse_transmission, view.world_position, ray.t, cell_life, rng);
        sample = vec4(transmission * ray_hit.material.base_color * radiance.rgb,
            max(sample.w, distance(world_position, ray_hit.world_position) + radiance.w));
    }
    return sample;
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
    var sample_count = min(old_radiance.a + 1.0, WORLD_CACHE_MAX_TEMPORAL_SAMPLES);
    let update_probability = min(1.0, f32(WORLD_CACHE_CELL_UPDATES_SOFT_CAP) / f32(world_cache_active_cells_count));
    let history = cache_history_length(sample_count, luminance(old_radiance.rgb), luminance(new_radiance), luminance_delta, update_probability);
    var blend_amount = 1.0 / history;
    // (the history the blend used is the history kept: a cell that followed
    // a change, a grenade's flash, keeps a short one, and lets the flash go
    // as fast as it took it; with the count climbing regardless, the flash
    // sat in a 32-sample history and left over a second: the change
    // detector's delta is smoothed and was slow to see the drop)
    if CACHE_HISTORY_CLAMP != 0u {
        // (never below the floor: a cell rebuilding from one sample boiled)
        sample_count = min(sample_count, max(history, f32(CACHE_DROP_FLOOR)));
    }
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
fn cache_history_length(sample_count: f32, old_luminance: f32, new_luminance: f32, luminance_delta: f32, update_probability: f32) -> f32 {
    // The history shortens as the cell's light changes: in proportion to the
    // change (an eighth root made every single-sample update's noise read as
    // a change, keeping a few samples where 32 were meant).
    let alpha = abs(luminance_delta) / max(old_luminance, 0.001);
    var max_sample_count = mix(WORLD_CACHE_MAX_TEMPORAL_SAMPLES, 1.0, saturate(alpha));
    if CACHE_DROP_DETECT != 0u {
        // (a drop seen the update it comes: the smoothed delta takes updates
        // to see a flash end, and the cell held the flash meanwhile, for
        // seconds in a closed room. Only a drop past the threshold counts,
        // a bolt passing or a sample's dip not at all, and what it leaves
        // is the floor at most: cut to a sample by every passing light, a
        // hallway after a firefight boiled)
        let threshold = f32(CACHE_DROP_THRESHOLD) / 100.0;
        let drop = max(old_luminance - new_luminance, 0.0) / max(old_luminance, 0.001);
        let past = saturate((drop - threshold) / max(1.0 - threshold, 0.01));
        max_sample_count = min(max_sample_count, mix(WORLD_CACHE_MAX_TEMPORAL_SAMPLES, f32(CACHE_DROP_FLOOR), past));
    }
    // (and at most WORLD_CACHE_HISTORY_FRAMES frames' updates: at 16, a big
    // cache, a cell updated one frame in four, kept four samples)
    let frame_limited_samples = max(1.0, WORLD_CACHE_HISTORY_FRAMES * clamp(update_probability, 0.0, 1.0));
    return max(1.0, min(sample_count, min(max_sample_count, frame_limited_samples)));
}
