import bevy_solarik::filtered_canopy_scene::forest_set_seed;
import bevy_core_pipeline::tonemapping::tonemapping_luminance as luminance;
import bevy_pbr::prepass::bindings::PreviousViewUniforms;
import bevy_pbr::render::utils::rand_f;
import bevy_pbr::render::utils::rand_range_u;
import bevy_pbr::render::utils::sample_disk;
import bevy_render::maths::PI;
import bevy_render::view::View;
import bevy_solarik::scene::brdf::evaluate_diffuse_brdf;
import bevy_solarik::scene::brdf::evaluate_base_specular_brdf;
import bevy_solarik::scene::brdf::evaluate_clearcoat_brdf;
import bevy_solarik::scene::brdf::clearcoat_roughness;
import bevy_solarik::realtime::gbuffer_utils::gpixel_resolve;
import bevy_solarik::realtime::gbuffer_utils::pixel_dissimilar;
import bevy_solarik::realtime::gbuffer_utils::temporal_pixel_dissimilar;
import bevy_solarik::realtime::gbuffer_utils::permute_pixel;
@if((CELL_LOOKUP_GEOMETRIC)) import bevy_solarik::realtime::gbuffer_utils::pixel_geometric_normal;
import bevy_solarik::realtime::gbuffer_utils::raster_reconstruction_error;
import bevy_solarik::scene::raytracing_scene_bindings::ResolvedMaterial;
import bevy_solarik::realtime::presample_light_tiles::unpack_resolved_light_sample;
import bevy_solarik::realtime::presample_light_tiles::light_tile_seed;
import bevy_solarik::scene::sampling::LightSample;
import bevy_solarik::scene::sampling::ResolvedLightSample;
import bevy_solarik::scene::sampling::NULL_LIGHT_ID;
import bevy_solarik::scene::sampling::calculate_resolved_light_contribution;
import bevy_solarik::scene::sampling::resolve_and_calculate_light_contribution;
import bevy_solarik::scene::sampling::resolve_light_sample;
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
import bevy_solarik::scene::sampling::trace_light_transmission;
import bevy_solarik::scene::sampling::balance_heuristic;
import bevy_solarik::scene::sampling::power_heuristic;
import bevy_solarik::scene::sampling::ggx_vndf_pdf;
import bevy_render::maths::orthonormalize;
import bevy_solarik::scene::raytracing_scene_bindings::light_sources;
import bevy_solarik::scene::raytracing_scene_bindings::directional_lights;
import bevy_solarik::scene::raytracing_scene_bindings::has_sun;
import bevy_solarik::scene::raytracing_scene_bindings::LIGHT_SOURCE_KIND_DIRECTIONAL;
import bevy_solarik::scene::raytracing_scene_bindings::LIGHT_SOURCE_FLAG_UNSEEN;
import bevy_solarik::realtime::realtime_bindings::world_cache_light_slots;
import bevy_solarik::realtime::world_cache_query::find_world_cache_cell;
import bevy_solarik::realtime::world_cache_query::WORLD_CACHE_NO_LIGHT;
import bevy_solarik::realtime::world_cache_query::world_cache_slot_light;
import bevy_solarik::realtime::world_cache_query::world_cache_slot_probabilities;
import bevy_solarik::realtime::world_cache_query::world_cache_slot_light_probability;
import bevy_solarik::realtime::world_cache_query::world_cache_draw_slot;
import bevy_solarik::scene::sampling::LIGHT_SAMPLE_DIRECTIONAL;
import bevy_pbr::render::utils::rand_u;
import bevy_solarik::scene::raytracing_scene_bindings::previous_frame_light_id_translations;
import bevy_solarik::scene::raytracing_scene_bindings::LIGHT_NOT_PRESENT_THIS_FRAME;
import bevy_solarik::scene::raytracing_scene_bindings::RAY_T_MIN;
import bevy_solarik::scene::raytracing_scene_bindings::offset_surface_ray;
import bevy_solarik::realtime::specular_gi::SPECULAR_GI_FOR_DI_ROUGHNESS_THRESHOLD;
import bevy_solarik::realtime::specular_gi::DIFFUSE_GI_REUSE_ROUGHNESS_THRESHOLD;
import bevy_solarik::realtime::realtime_bindings::view_output;
import bevy_solarik::realtime::realtime_bindings::light_tile_samples;
import bevy_solarik::realtime::realtime_bindings::light_tile_resolved_samples;
import bevy_solarik::realtime::realtime_bindings::di_reservoirs_a;
import bevy_solarik::realtime::realtime_bindings::di_reservoirs_b;
import bevy_solarik::realtime::realtime_bindings::gbuffer;
import bevy_solarik::realtime::realtime_bindings::depth_buffer;
import bevy_solarik::realtime::realtime_bindings::motion_vectors;
import bevy_solarik::realtime::realtime_bindings::previous_gbuffer;
import bevy_solarik::realtime::realtime_bindings::previous_depth_buffer;
import bevy_solarik::realtime::realtime_bindings::view;
import bevy_solarik::realtime::realtime_bindings::previous_view;
import bevy_solarik::realtime::realtime_bindings::constants as lighting_constants;
import bevy_solarik::realtime::tuning::DI_TILE_CANDIDATES;
import bevy_solarik::realtime::tuning::DI_CELL_CANDIDATES;
import bevy_solarik::realtime::tuning::DI_CONFIDENCE_CAP;
import bevy_solarik::realtime::realtime_bindings::ResolvedLightSamplePacked;

// https://intro-to-restir.cwyman.org/presentations/2023ReSTIR_Course_Notes.pdf
// https://d1qx31qr3h6wln.cloudfront.net/publications/ReSTIR%20GI.pdf

enable wgpu_ray_query;


// (32 and a confidence cap of 8, from 8 and 20: a sample reused for 20
// frames over 30 pixels made patches of light that came and went, which
// DLSS-RR keeps as detail; interiors boiled)
// (SOLARIK_DI_TILE_CANDIDATES, tuning.wesl)
const INITIAL_SAMPLES = DI_TILE_CANDIDATES;
// Candidates from the lights in sight of the pixel's world cache cell, beside
// the tiles' (which draw from the whole level by power), with
// on by default (SOLARIK_CELL_LIGHTS=0 leaves them out): a narrow corridor's recessed fixtures, among hundreds
// of the level's lights, got a sample now and then. In a10's corridors, in
// motion, 8% less boiling for 0.6 ms (the tiles alone, since the sun has its
// own sample, mostly find them). (With the slots weighted by what each light
// gives the cell, and half the screen finding its cell again, in a10's
// Jefferies tubes a wall beside a dim red strip went from lit and dark by
// turns to steady, its three-shot spread 0.35 to 0.01; 24 did no better.)
// (SOLARIK_DI_CELL_CANDIDATES)
const CELL_SAMPLES = DI_CELL_CANDIDATES;
// The share of its unshadowed contribution a tile candidate is weighed by
// when the pixel's cell has light slots and this light is in none of them
// (DI_SLOT_VISIBILITY_PRIOR). The resampling weighs candidates by their
// unshadowed contribution, so the brightest light wins whether or not the
// pixel can see it, and the shadow ray then drops it: in b40's elevator
// shaft 92% of the pixels' picks were dropped, in c20's halls 63%, and
// those pixels shaded from reuse alone. The slots hold the lights that
// shadow-tested light reached the cell: a light in none of them is weighed
// as mostly hidden. The target function may be anything positive where the
// light contributes, so this changes variance alone: the pick's weight
// divides by this target and the shading multiplies by the real, traced
// contribution.
const SLOT_VISIBILITY_PRIOR: f32 = 0.1;
const SPATIAL_REUSE_RADIUS_PIXELS = 30.0;
// (SOLARIK_DI_CONFIDENCE_CAP)
const CONFIDENCE_WEIGHT_CAP = DI_CONFIDENCE_CAP;
@if((RASTER_SUBPIXEL_BITS))
const RASTER_SUBPIXEL_STEP = 1.0 / f32(1u << constants::RASTER_SUBPIXEL_BITS);
// Vulkan's guaranteed minimum, used by tools without adapter specialization.
@if((!(RASTER_SUBPIXEL_BITS)))
const RASTER_SUBPIXEL_STEP = 1.0 / 16.0;

// (SOLARIK_SHADER_DEFS=VISUALIZE_DI_PICK, with HALO_RT_NO_DLSS=1: what the
// initial candidates found. Red: the share of the tile candidates with any
// contribution; green: the share of the cell candidates with any; blue: the
// pick was shadow-tested dark and dropped. The later passes leave the view.)
@if(VISUALIZE_DI_PICK) {
var<private> debug_di_pick: vec3<f32>;
}

// (SOLARIK_SHADER_DEFS=VISUALIZE_LIGHT_NEAR, with HALO_RT_NO_DLSS=1: the near
// sampling's verdicts at the initial candidates (sampling.wesl,
// near_light_candidate). Red: the share of the tile candidates that passed
// light_may_be_near and were not warped; green: the share of the tile
// candidates warped plus the share of the cell candidates warped; blue: the
// share of the cell candidates that passed the cheap rejects and were not
// warped (the closest point or the solid angle said far). It takes the one-loop form
// (LIGHT_NEAR_INLINE) itself. The later passes leave the view.)
@if(VISUALIZE_LIGHT_NEAR) {
var<private> debug_light_near: vec3<f32>;
}

// (Timing switches, SOLARIK_SHADER_DEFS=DI_TIME_NO_...: each leaves a stage
// out so the pass's two spans say what it costs; the picture is wrong with
// any of them. NO_PICK_RAY: the initial pick's shadow test; NO_SUN_RAY: the
// sun candidate's; NO_FINAL_RAY: the shading's; NO_TEMPORAL and NO_SPATIAL:
// the reuse, the canonical reservoir kept.)

@compute @workgroup_size(8, 8, 1)
fn initial_and_temporal(@builtin(workgroup_id) workgroup_id: vec3<u32>, @builtin(global_invocation_id) global_id: vec3<u32>) {
    if any(global_id.xy >= vec2u(view.main_pass_viewport.zw)) { return; }

    let pixel_index = global_id.x + global_id.y * u32(view.main_pass_viewport.z);
    var rng = pixel_index + lighting_constants.frame_index;
    forest_set_seed(vec2u(pixel_index,lighting_constants.frame_index));

    let depth = textureLoad(depth_buffer, global_id.xy, 0);
    if depth == 0.0 {
        store_reservoir_b(global_id.xy, empty_reservoir());
        return;
    }
    let surface = gpixel_resolve(textureLoad(gbuffer, global_id.xy, 0), depth, global_id.xy, view.main_pass_viewport.zw, view.world_from_clip);

    let lobe = target_lobe(surface.material, surface.world_position);
    let initial_reservoir = generate_initial_reservoir(surface.world_position, surface.world_normal, lobe, workgroup_id.xy, global_id.xy, depth, &rng);
@if(DI_TIME_NO_TEMPORAL) {
    store_reservoir_b(global_id.xy, initial_reservoir);
}
@if((!(DI_TIME_NO_TEMPORAL))) {
    let temporal = load_temporal_reservoir(global_id.xy, depth, surface.world_position, surface.world_normal);
    let merge_result = merge_reservoirs(initial_reservoir, surface.world_position, surface.world_normal, lobe,
        temporal.reservoir, temporal.world_position, temporal.world_normal, temporal.lobe, &rng);

    store_reservoir_b(global_id.xy, merge_result.merged_reservoir);
}
@if(VISUALIZE_DI_PICK) {
    textureStore(view_output, global_id.xy, vec4(debug_di_pick, 1.0));
}
@if(VISUALIZE_LIGHT_NEAR) {
    textureStore(view_output, global_id.xy, vec4(debug_light_near, 1.0));
}
}

@compute @workgroup_size(8, 8, 1)
fn spatial_and_shade(@builtin(global_invocation_id) global_id: vec3<u32>) {
    if any(global_id.xy >= vec2u(view.main_pass_viewport.zw)) { return; }

    let pixel_index = global_id.x + global_id.y * u32(view.main_pass_viewport.z);
    var rng = pixel_index + lighting_constants.frame_index;
    forest_set_seed(vec2u(pixel_index,lighting_constants.frame_index));

    let depth = textureLoad(depth_buffer, global_id.xy, 0);
    if depth == 0.0 {
        store_reservoir_a(global_id.xy, empty_reservoir());
        return;
    }
    let surface = gpixel_resolve(textureLoad(gbuffer, global_id.xy, 0), depth, global_id.xy, view.main_pass_viewport.zw, view.world_from_clip);

    let lobe = target_lobe(surface.material, surface.world_position);
    let input_reservoir = load_reservoir_b(global_id.xy);
    var spatial = NeighborInfo(empty_reservoir(), surface.world_position, surface.world_normal, TargetLobe());
@if((!(DI_TIME_NO_SPATIAL))) {
    spatial = load_spatial_reservoir(global_id.xy, depth, surface.world_position, surface.world_normal, &rng);
}
    let merge_result = merge_reservoirs(input_reservoir, surface.world_position, surface.world_normal, lobe,
        spatial.reservoir, spatial.world_position, spatial.world_normal, spatial.lobe, &rng);
    let combined_reservoir = merge_result.merged_reservoir;

    // Reusable weights use an unoccluded target in both resampling modes.
    // RGB connection transport belongs only to this receiver's shading.
    store_reservoir_a(global_id.xy, combined_reservoir);
@if(VISUALIZE_DI_PICK) {
    return;
}
@if(VISUALIZE_LIGHT_NEAR) {
    return;
}

    let wo = normalize(view.world_position - surface.world_position);
    var brdf = evaluate_diffuse_brdf(wo, merge_result.wi, surface.world_normal, surface.material);
    // Only consider a specular lobe if it is not smooth, else leave it for the specular GI pass to handle
    // (SOLARIK_SHADER_DEFS=DI_NO_SPECULAR, a diagnostic: both glossy lobes
    // left out of the shading, to see what share of the picture they are)
@if((!(DI_NO_SPECULAR))) {
    if surface.material.roughness > SPECULAR_GI_FOR_DI_ROUGHNESS_THRESHOLD {
        var specular = evaluate_base_specular_brdf(wo, merge_result.wi, surface.world_normal, surface.material);
@if((!(DI_GLOSSY_UNSHARED))) {
        specular *= glossy_light_weight(surface.material.roughness, wo, merge_result.wi, surface.world_normal, merge_result.light);
}
        brdf += specular;
    }
    if clearcoat_roughness(surface.material) > SPECULAR_GI_FOR_DI_ROUGHNESS_THRESHOLD {
        var coat = evaluate_clearcoat_brdf(wo, merge_result.wi, surface.world_normal, surface.material);
@if((!(DI_GLOSSY_UNSHARED))) {
        coat *= glossy_light_weight(clearcoat_roughness(surface.material), wo, merge_result.wi, surface.world_normal, merge_result.light);
}
        brdf += coat;
    }
}

    var raster_error = 0.0;
    // Finite-canopy and Gaussian primary surfaces write analytic intersection
    // depth. Ordinary raster meshes (including VG) retain the snapping bound.
    if (textureLoad(gbuffer, global_id.xy, 0).a & 0x28000000u) == 0u {
        raster_error = raster_reconstruction_error(global_id.xy, depth, surface.world_normal,
            view.main_pass_viewport.zw, view.world_from_clip, RASTER_SUBPIXEL_STEP);
    }
    var pixel_color = shade_di_reservoir(merge_result, surface.world_position, surface.world_normal, raster_error);
    pixel_color *= brdf;
    pixel_color += surface.material.emissive;
    pixel_color *= view.exposure;
    textureStore(view_output, global_id.xy, vec4(pixel_color, 1.0));
}

fn shade_di_reservoir(result: ReservoirMergeResult, world_position: vec3<f32>, world_normal: vec3<f32>, raster_error: f32) -> vec3<f32> {
    if !reservoir_valid(result.merged_reservoir) { return vec3(0.0); }
    var transmission = vec4(1.0);
@if((!(DI_TIME_NO_FINAL_RAY))) {
    transmission = trace_light_transmission(offset_surface_ray(world_position, world_normal, view.world_position, raster_error),
        result.selected_light_world_position);
}
    return result.selected_sample_radiance * result.merged_reservoir.unbiased_contribution_weight * transmission.rgb;
}

fn generate_initial_reservoir(world_position: vec3<f32>, world_normal: vec3<f32>, lobe: TargetLobe, workgroup_id: vec2<u32>, pixel_id: vec2<u32>, depth: f32, rng: ptr<function, u32>) -> Reservoir {
    var workgroup_rng = light_tile_seed((workgroup_id.x * 5782582u) + workgroup_id.y, lighting_constants.frame_index);
    let light_tile_start = rand_range_u(128u, &workgroup_rng) * 1024u;

    var reservoir = empty_reservoir();

    // Two strategies pick a light: the tiles, by the table's selection
    // probability (over what isn't the sun's, presample_light_tiles), and the
    // cell's slots, by the light's weight in them; a point on the light is
    // picked alike after. Each candidate is weighted by the balance heuristic
    // of the two, so a light the cell missed still comes by the tiles.
    var sun_share = 1.0;
    if has_sun() {
        let sun_probability = light_sources[directional_lights[0].light_source_index].selection_probability;
        if sun_probability < 0.999 { sun_share = 1.0 - sun_probability; }
    }
    var light_slots = vec4(WORLD_CACHE_NO_LIGHT);
@if(DI_CELL_LIGHTS) {
    // (CELL_LOOKUP_GEOMETRIC: the cell by the surface's geometric normal, as
    // the cells were keyed, see pixel_geometric_normal)
    var lookup_normal = world_normal;
@if(CELL_LOOKUP_GEOMETRIC) {
    lookup_normal = pixel_geometric_normal(pixel_id, depth, world_normal, view.main_pass_viewport.zw, view.world_from_clip, depth_buffer);
}
    let cell = find_world_cache_cell(world_position, lookup_normal, view.world_position, rng);
    if cell >= 0 {
        light_slots = world_cache_light_slots[cell];
    }
}
    // (the slots drawn by how much each light lights the cell)
    let slot_probabilities = world_cache_slot_probabilities(light_slots);
    var cell_samples = 0u;
    if any(slot_probabilities > vec4(0.0)) {
        cell_samples = CELL_SAMPLES;
    }

    var pick = InitialPick(0.0, 0.0, LightSample(0u, 0u), 0xFFFFFFFFu, vec4(0.0));
@if(VISUALIZE_DI_PICK) {
    debug_di_pick = vec3(0.0);
}
    let candidates = InitialCandidates(world_position, world_normal, lobe, cell_samples, light_slots, slot_probabilities, sun_share);
@if(VISUALIZE_LIGHT_NEAR) {
    debug_light_near = vec3(0.0);
}
    // (a candidate that may be near put off to a second pass, but with
    // LIGHT_NEAR_INLINE: see sampling.wesl)
@if((!(LIGHT_NEAR_INLINE || VISUALIZE_LIGHT_NEAR || LIGHT_NEAR_AREA))) {
    var deferred_tiles: array<u32, INITIAL_DEFERRED>;
    var deferred_tile_count = 0u;
    var deferred_cells: array<LightSample, INITIAL_DEFERRED>;
    var deferred_cell_count = 0u;
}
    for (var i = 0u; i < INITIAL_SAMPLES; i++) {
        let tile_sample = light_tile_start + rand_range_u(1024u, rng);
        let tile_resolved = unpack_resolved_light_sample(light_tile_resolved_samples[tile_sample], view.exposure);
        // (the sun has a sample of its own, below)
        if tile_resolved.world_position.w == LIGHT_SAMPLE_DIRECTIONAL { continue; }
        // (a near triangle's point taken by solid angle from here, its
        // record read only where the presampled point says it may be near:
        // see near_light_candidate)
        var candidate = NearLightCandidate(LightSample(NULL_LIGHT_ID, 0u), tile_resolved, tile_resolved.inverse_pdf, false);
        if light_may_be_near(tile_resolved, world_position) {
@if((!(LIGHT_NEAR_INLINE || VISUALIZE_LIGHT_NEAR || LIGHT_NEAR_AREA))) {
            if deferred_tile_count < INITIAL_DEFERRED {
                deferred_tiles[deferred_tile_count] = tile_sample;
                deferred_tile_count += 1u;
                continue;
            }
}
            let tile_light_sample = light_tile_samples[tile_sample];
            candidate = near_light_candidate(tile_light_sample, light_sources[tile_light_sample.light_id >> 16u], tile_resolved, world_position);
@if(VISUALIZE_LIGHT_NEAR) {
            debug_light_near.r += select(1.0, 0.0, candidate.near) / f32(INITIAL_SAMPLES);
}
        }
@if(VISUALIZE_LIGHT_NEAR) {
        debug_light_near.g += select(0.0, 1.0, candidate.near) / f32(INITIAL_SAMPLES);
}
        stream_tile_candidate(candidates, candidate, tile_sample, &pick, rng);
    }
    for (var i = 0u; i < cell_samples; i++) {
        let light_id = world_cache_slot_light(light_slots[world_cache_draw_slot(slot_probabilities, rand_f(rng))]);
        if light_id >= arrayLength(&light_sources) { continue; }
        let light_source = light_sources[light_id];
        if light_source.kind == LIGHT_SOURCE_KIND_DIRECTIONAL { continue; }
        var triangle_id = 0u;
        if (light_source.kind & 1u) == 0u {
            triangle_id = rand_range_u(light_source.kind >> 1u, rng);
        }
        let light_sample = LightSample((light_id << 16u) | triangle_id, rand_u(rng));
@if((!(LIGHT_NEAR_INLINE || VISUALIZE_LIGHT_NEAR || LIGHT_NEAR_AREA))) {
        // (the cheap rejects alone here; one that passes waits)
        if light_source_is_emissive_mesh(light_source) {
            let triangle = load_light_triangle(light_source, triangle_id);
            if deferred_cell_count < INITIAL_DEFERRED && light_triangle_may_be_near(triangle, world_position) {
                deferred_cells[deferred_cell_count] = light_sample;
                deferred_cell_count += 1u;
                continue;
            }
            let resolved = resolve_emissive_light_sample(light_source, triangle, light_sample.seed);
            stream_cell_candidate(candidates, NearLightCandidate(light_sample, resolved, resolved.inverse_pdf, false), light_id, light_source, &pick, rng);
            continue;
        }
}
        // (a near triangle's point by solid angle, as above)
        let candidate = resolve_near_light_sample(light_sample, light_source, world_position);
@if(VISUALIZE_LIGHT_NEAR) {
        debug_light_near.g += select(0.0, 1.0, candidate.near) / f32(cell_samples);
        if !candidate.near && light_source_is_emissive_mesh(light_source)
            && light_triangle_may_be_near(load_light_triangle(light_source, triangle_id), world_position) {
            debug_light_near.b += 1.0 / f32(cell_samples);
        }
}
        stream_cell_candidate(candidates, candidate, light_id, light_source, &pick, rng);
    }
@if((!(LIGHT_NEAR_INLINE || VISUALIZE_LIGHT_NEAR || LIGHT_NEAR_AREA))) {
    // (the deferred candidates: the near path, run by the lanes that have
    // any together)
    for (var i = 0u; i < deferred_tile_count; i++) {
        let tile_sample = deferred_tiles[i];
        let tile_resolved = unpack_resolved_light_sample(light_tile_resolved_samples[tile_sample], view.exposure);
        let tile_light_sample = light_tile_samples[tile_sample];
        let candidate = near_light_candidate(tile_light_sample, light_sources[tile_light_sample.light_id >> 16u], tile_resolved, world_position);
        stream_tile_candidate(candidates, candidate, tile_sample, &pick, rng);
    }
    for (var i = 0u; i < deferred_cell_count; i++) {
        let light_sample = deferred_cells[i];
        let light_id = light_sample.light_id >> 16u;
        let light_source = light_sources[light_id];
        stream_cell_candidate(candidates, resolve_near_light_sample(light_sample, light_source, world_position), light_id, light_source, &pick, rng);
    }
}

    var sun_selected = false;
    // The sun, one sample of its own in every pixel, shadow-tested before
    // it competes: indoors, out of sight behind the hull, it was the
    // brightest candidate yet lit nothing, and won most pixels (a fixed
    // share of the tiles' samples was the sun's); outdoors, it reaches
    // every pixel instead of half.
    if has_sun() {
        let light_id = directional_lights[0].light_source_index;
        let candidate = LightSample(light_id << 16u, rand_u(rng));
        let resolved = resolve_light_sample(candidate, light_sources[light_id]);
        let contribution = calculate_resolved_light_contribution(resolved, world_position, world_normal);
        let target_function = light_target_function(lobe, contribution.radiance, contribution.wi, world_normal,
            GlossyMisLight(light_id, contribution.brdf_rays_can_hit, contribution.solid_angle_pdf));
        if target_function > 0.0 {
            var transmission = vec4(1.0);
@if((!(DI_TIME_NO_SUN_RAY))) {
            transmission = trace_light_transmission(offset_surface_ray(world_position, world_normal, view.world_position, 0.0), resolved.world_position);
}
            if max(transmission.r, max(transmission.g, transmission.b)) > 0.0 {
                let resampling_weight = target_function * contribution.inverse_pdf;
                pick.weight_sum += resampling_weight;
                if rand_f(rng) < resampling_weight / pick.weight_sum {
                    pick.target_function = target_function;
                    sun_selected = true;
                    pick.sample = candidate;
                    pick.light_position = resolved.world_position;
                }
            }
        }
    }

    if pick.target_function != 0.0 {
        reservoir.sample = pick.sample;
        if pick.tile_sample != 0xFFFFFFFFu && !sun_selected {
            reservoir.sample = light_tile_samples[pick.tile_sample];
        }
    }

    if reservoir_valid(reservoir) {
        let inverse_target_function = select(0.0, 1.0 / pick.target_function, pick.target_function > 0.0);
        reservoir.unbiased_contribution_weight = pick.weight_sum * inverse_target_function;
        // A light out of sight lights nothing: it is dropped before reuse
        // spreads it (over 30 pixels and 8 frames, the patches of a corridor
        // lit or dark as one hidden light came and went). The sun was tested
        // above.
        if !sun_selected {
            var transmission = vec4(1.0);
@if((!(DI_TIME_NO_PICK_RAY))) {
            transmission = trace_light_transmission(offset_surface_ray(world_position, world_normal, view.world_position, 0.0), pick.light_position);
}
            if max(transmission.r, max(transmission.g, transmission.b)) <= 0.0 {
                reservoir.unbiased_contribution_weight = 0.0;
@if(VISUALIZE_DI_PICK) {
                debug_di_pick.b = 1.0;
}
            }
        }
    }

    reservoir.confidence_weight = 1.0;
    return reservoir;
}

// The initial pick, which each candidate streams into.
struct InitialPick {
    weight_sum: f32,
    target_function: f32,
    sample: LightSample,
    // The tile sample picked, or 0xFFFFFFFF for the pick's own `sample`.
    tile_sample: u32,
    light_position: vec4<f32>,
}

// What every initial candidate at a pixel is weighed by.
struct InitialCandidates {
    world_position: vec3<f32>,
    world_normal: vec3<f32>,
    lobe: TargetLobe,
    cell_samples: u32,
    light_slots: vec4<u32>,
    slot_probabilities: vec4<f32>,
    sun_share: f32,
}

// The most initial candidates of a kind the first pass puts off (a
// candidate past it is taken inline).
const INITIAL_DEFERRED: u32 = 16u;

// A tile candidate weighed and streamed into the pick (one place for the
// first pass and the second).
fn stream_tile_candidate(c: InitialCandidates, candidate: NearLightCandidate, tile_sample: u32, pick: ptr<function, InitialPick>, rng: ptr<function, u32>) {
    let resolved_light_sample = candidate.resolved;
    let light_contribution = calculate_resolved_light_contribution(resolved_light_sample, c.world_position, c.world_normal);
    let target_function_unseen = light_target_function(c.lobe, light_contribution.radiance, light_contribution.wi, c.world_normal,
        GlossyMisLight(light_tile_samples[tile_sample].light_id >> 16u, light_contribution.brdf_rays_can_hit, light_contribution.solid_angle_pdf));
    if !(target_function_unseen > 0.0) || !is_finite_f32(target_function_unseen) { return; }
@if(VISUALIZE_DI_PICK) {
    debug_di_pick.r += 1.0 / f32(INITIAL_SAMPLES);
}

    var target_function = target_function_unseen;
    var resampling_weight = target_function * candidate.inverse_pdf / f32(INITIAL_SAMPLES);
    if c.cell_samples > 0u {
        let light_id = light_tile_samples[tile_sample].light_id >> 16u;
        let tile_probability = light_sources[light_id].selection_probability / c.sun_share;
        let cell_probability = world_cache_slot_light_probability(c.light_slots, c.slot_probabilities, light_id);
        // (DI_SLOT_VISIBILITY_PRIOR: a light the cell's slots don't know
        // is weighed as if mostly hidden; see SLOT_VISIBILITY_PRIOR)
@if(DI_SLOT_VISIBILITY_PRIOR) {
        if cell_probability <= 0.0 { target_function *= SLOT_VISIBILITY_PRIOR; }
}
        resampling_weight = target_function * candidate.inverse_pdf * tile_probability
            / (f32(INITIAL_SAMPLES) * tile_probability + f32(c.cell_samples) * cell_probability);
    }
    // (no non-finite weight into the reservoir: see is_finite_f32)
    if !is_finite_f32(resampling_weight) || resampling_weight < 0.0 { return; }

    (*pick).weight_sum += resampling_weight;
    if rand_f(rng) < resampling_weight / (*pick).weight_sum {
        (*pick).target_function = target_function;
        // (a warped point is its own sample, not the tile's)
        (*pick).tile_sample = select(tile_sample, 0xFFFFFFFFu, candidate.near);
        (*pick).sample = candidate.light_sample;
        (*pick).light_position = resolved_light_sample.world_position;
    }
}

// A cell candidate weighed and streamed into the pick (as above).
fn stream_cell_candidate(c: InitialCandidates, candidate: NearLightCandidate, light_id: u32, light_source: LightSource, pick: ptr<function, InitialPick>, rng: ptr<function, u32>) {
    let resolved_light_sample = candidate.resolved;
    let light_contribution = calculate_resolved_light_contribution(resolved_light_sample, c.world_position, c.world_normal);
    let target_function = light_target_function(c.lobe, light_contribution.radiance, light_contribution.wi, c.world_normal,
        GlossyMisLight(light_id, light_contribution.brdf_rays_can_hit, light_contribution.solid_angle_pdf));
    if !(target_function > 0.0) || !is_finite_f32(target_function) { return; }
@if(VISUALIZE_DI_PICK) {
    debug_di_pick.g += 1.0 / f32(c.cell_samples);
}

    let tile_probability = light_source.selection_probability / c.sun_share;
    let cell_probability = world_cache_slot_light_probability(c.light_slots, c.slot_probabilities, light_id);
    let resampling_weight = target_function * candidate.inverse_pdf
        / (f32(INITIAL_SAMPLES) * tile_probability + f32(c.cell_samples) * cell_probability);
    if !is_finite_f32(resampling_weight) || resampling_weight < 0.0 { return; }

    (*pick).weight_sum += resampling_weight;
    if rand_f(rng) < resampling_weight / (*pick).weight_sum {
        (*pick).target_function = target_function;
        (*pick).tile_sample = 0xFFFFFFFFu;
        (*pick).sample = candidate.light_sample;
        (*pick).light_position = resolved_light_sample.world_position;
    }
}

fn load_temporal_reservoir(pixel_id: vec2<u32>, depth: f32, world_position: vec3<f32>, world_normal: vec3<f32>) -> NeighborInfo {
    let motion_vector = textureLoad(motion_vectors, pixel_id, 0).xy;
    let temporal_pixel_id_float = round(vec2<f32>(pixel_id) - (motion_vector * view.main_pass_viewport.zw));
    var point_temporal_pixel_id = vec2<u32>(temporal_pixel_id_float);

    // DI reservoirs retain unoccluded weights; final visibility is traced now.
    // Global events and the legacy radius path keep their full fallback.
    if (lighting_constants.reset & 1u) != 0u || (lighting_constants.reset & 6u) == 2u {
        return NeighborInfo(empty_reservoir(), vec3(0.0), vec3(0.0), TargetLobe());
    }

    if any(temporal_pixel_id_float < vec2(0.0)) || any(temporal_pixel_id_float >= view.main_pass_viewport.zw) {
        point_temporal_pixel_id = pixel_id;
    }

    let permuted_temporal_pixel_id = permute_pixel(point_temporal_pixel_id, lighting_constants.frame_index, view.main_pass_viewport.zw);
    var temporal = load_temporal_reservoir_inner(permuted_temporal_pixel_id, depth, world_position, world_normal);

    // Check if the light selected in the previous frame no longer exists in the current frame (e.g. entity despawned)
    let previous_light_id = temporal.reservoir.sample.light_id >> 16u;
    let triangle_id = temporal.reservoir.sample.light_id & 0xFFFFu;
    let light_id = previous_frame_light_id_translations[previous_light_id];
    if light_id == LIGHT_NOT_PRESENT_THIS_FRAME {
        return NeighborInfo(empty_reservoir(), vec3(0.0), vec3(0.0), TargetLobe());
    }
    temporal.reservoir.sample.light_id = (light_id << 16u) | triangle_id;

    temporal.reservoir.confidence_weight = min(temporal.reservoir.confidence_weight, CONFIDENCE_WEIGHT_CAP);

    return temporal;
}

fn load_temporal_reservoir_inner(temporal_pixel_id: vec2<u32>, depth: f32, world_position: vec3<f32>, world_normal: vec3<f32>) -> NeighborInfo {
    // Check if the pixel features have changed heavily between the current and previous frame
    let temporal_depth = textureLoad(previous_depth_buffer, temporal_pixel_id, 0);
    let temporal_surface = gpixel_resolve(textureLoad(previous_gbuffer, temporal_pixel_id, 0), temporal_depth, temporal_pixel_id, view.main_pass_viewport.zw, previous_view.world_from_clip);
    let temporal_lobe = target_lobe(temporal_surface.material, temporal_surface.world_position);
    if temporal_pixel_dissimilar(depth, world_position, temporal_surface.world_position, world_normal, temporal_surface.world_normal, view, previous_view.view_from_world) {
        return NeighborInfo(empty_reservoir(), vec3(0.0), vec3(0.0), TargetLobe());
    }

    let temporal_reservoir = load_reservoir_a(temporal_pixel_id);
    return NeighborInfo(temporal_reservoir, temporal_surface.world_position, temporal_surface.world_normal, temporal_lobe);
}

fn load_spatial_reservoir(pixel_id: vec2<u32>, depth: f32, world_position: vec3<f32>, world_normal: vec3<f32>, rng: ptr<function, u32>) -> NeighborInfo {
    for (var i = 0u; i < 5u; i++) {
        let spatial_pixel_id = get_neighbor_pixel_id(pixel_id, SPATIAL_REUSE_RADIUS_PIXELS, rng);

        let spatial_depth = textureLoad(depth_buffer, spatial_pixel_id, 0);
        let spatial_surface = gpixel_resolve(textureLoad(gbuffer, spatial_pixel_id, 0), spatial_depth, spatial_pixel_id, view.main_pass_viewport.zw, view.world_from_clip);
        let spatial_lobe = target_lobe(spatial_surface.material, spatial_surface.world_position);
        if pixel_dissimilar(depth, world_position, spatial_surface.world_position, world_normal, spatial_surface.world_normal, view) {
            continue;
        }

        let spatial_reservoir = load_reservoir_b(spatial_pixel_id);
        return NeighborInfo(spatial_reservoir, spatial_surface.world_position, spatial_surface.world_normal, spatial_lobe);
    }

    return NeighborInfo(empty_reservoir(), world_position, world_normal, TargetLobe());
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
    lobe: TargetLobe,
}

struct Reservoir {
    sample: LightSample,
    confidence_weight: f32,
    unbiased_contribution_weight: f32,
}

fn empty_reservoir() -> Reservoir {
    return Reservoir(
        LightSample(NULL_LIGHT_ID, 0u),
        0.0,
        0.0,
    );
}

// The least albedo a surface's lights are weighed by (a dielectric's
// reflectance): weighed by the diffuse colour alone, a black surface found
// no light, and so took no highlight either (the shading's BRDF has its
// specular lobe): a Covenant crate's bevels, mapped on its texture's black
// outlines, went black beside its glossy body's bright highlight.
const TARGET_ALBEDO_FLOOR: f32 = 0.04;

// The diffuse BRDF lights are weighed by when one is picked (see
// TARGET_ALBEDO_FLOOR); the shading evaluates the surface's own.
fn target_brdf(material: ResolvedMaterial) -> vec3<f32> {
    return max(material.base_color, vec3(TARGET_ALBEDO_FLOOR)) / PI;
}

// A surface as its lights are weighed: the diffuse BRDF above, and for
// DI_SPECULAR_TARGET its glossy lobes, seen from this frame's camera (the
// temporal neighbour's too: a frame's move is nothing to the lobe).
struct TargetLobe {
    diffuse_brdf: vec3<f32>,
    wo: vec3<f32>,
    material: ResolvedMaterial,
}

fn target_lobe(material: ResolvedMaterial, world_position: vec3<f32>) -> TargetLobe {
    return TargetLobe(target_brdf(material), normalize(view.world_position - world_position), material);
}

// The target function a light is picked and merged by. Weighed by the
// diffuse response alone, the highlight of a small light on a glossy surface
// was whatever the pick for the diffuse gave the lobe, and wandered (c20's
// strip-lit hall's rounded door, 10 November). (SOLARIK_SHADER_DEFS=
// DI_SPECULAR_TARGET: the glossy lobes the shading applies, under the same
// conditions, weighed in too; they carry their own cosine, and their share
// (glossy_light_weight), as the shading takes them. Not finite, a target is
// nothing.)
fn light_target_function(lobe: TargetLobe, radiance: vec3<f32>, wi: vec3<f32>, world_normal: vec3<f32>, light: GlossyMisLight) -> f32 {
@if((!(DI_SPECULAR_TARGET))) {
    return luminance(radiance * lobe.diffuse_brdf * saturate(dot(wi, world_normal)));
}
@if(DI_SPECULAR_TARGET) {
    var brdf = lobe.diffuse_brdf * saturate(dot(wi, world_normal));
    if lobe.material.roughness > SPECULAR_GI_FOR_DI_ROUGHNESS_THRESHOLD {
        var specular = evaluate_base_specular_brdf(lobe.wo, wi, world_normal, lobe.material);
@if((!(DI_GLOSSY_UNSHARED))) {
        specular *= glossy_light_weight(lobe.material.roughness, lobe.wo, wi, world_normal, light);
}
        brdf += specular;
    }
    if clearcoat_roughness(lobe.material) > SPECULAR_GI_FOR_DI_ROUGHNESS_THRESHOLD {
        var coat = evaluate_clearcoat_brdf(lobe.wo, wi, world_normal, lobe.material);
@if((!(DI_GLOSSY_UNSHARED))) {
        coat *= glossy_light_weight(clearcoat_roughness(lobe.material), lobe.wo, wi, world_normal, light);
}
        brdf += coat;
    }
    let target_function = luminance(radiance * brdf);
    return select(0.0, target_function, target_function <= 3.0e38);
}
}

// A picked light as its glossy share needs it (glossy_light_weight).
struct GlossyMisLight {
    light_id: u32,
    brdf_rays_can_hit: bool,
    // Its sample's solid-angle density from the receiver, the light's
    // selection left out (calculate_resolved_light_contribution).
    solid_angle_pdf: f32,
}

// A glossy lobe's share of a picked light, against the specular GI's glossy
// ray, which traces this lobe each frame where its roughness is in
// (SPECULAR_GI_FOR_DI_ROUGHNESS_THRESHOLD, DIFFUSE_GI_REUSE_ROUGHNESS_THRESHOLD]
// and takes the rest of an emitter it meets (specular_gi's
// emissive_mis_weight): the power heuristic of the same two densities, so
// that the two shares are one in every direction. Light-sampled alone, the
// highlight of a near emitter on a glossy surface flashed, its f/p unbounded
// as the emitter nears or the lobe narrows, where a glossy ray's sample of
// it is bounded by the lobe's albedo and the emitter's radiance (c20's
// strip-lit hall, the door's rim, 10 November). A light no ray can meet
// (analytic, the sun, a light-only emitter) is all this side's.
// (SOLARIK_SHADER_DEFS=DI_GLOSSY_UNSHARED: the lobe all this side's, the
// ray's first emitter weighed 0, as before. DI_GLOSSY_RAY_ONLY, a
// diagnostic: every light both sides reach left to the ray, which alone is
// unbiased, so its picture's mean over time should be the shared one's.)
fn glossy_light_weight(roughness: f32, wo: vec3<f32>, wi: vec3<f32>, world_normal: vec3<f32>, light: GlossyMisLight) -> f32 {
    if roughness <= SPECULAR_GI_FOR_DI_ROUGHNESS_THRESHOLD || roughness > DIFFUSE_GI_REUSE_ROUGHNESS_THRESHOLD { return 1.0; }
    if !light.brdf_rays_can_hit || light.light_id >= arrayLength(&light_sources) { return 1.0; }
    let light_source = light_sources[light.light_id];
    if (light_source.flags & LIGHT_SOURCE_FLAG_UNSEEN) != 0u { return 1.0; }
    // (the frame and density glossy_lobe samples the lobe by; a direction it
    // can't sample is this side's alone)
    let TBN = orthonormalize(world_normal);
    let wo_tangent = vec3(dot(wo, TBN[0]), dot(wo, TBN[1]), dot(wo, TBN[2]));
    let wi_tangent = vec3(dot(wi, TBN[0]), dot(wi, TBN[1]), dot(wi, TBN[2]));
    if wi_tangent.z <= 0.0 { return 1.0; }
    let p_bounce = ggx_vndf_pdf(wo_tangent, wi_tangent, roughness);
    if !(p_bounce > 0.0) { return 1.0; }
    // (as random_emissive_light_solid_angle_pdf finds it at the glossy
    // ray's hit: the light's selection over its triangles' area, to solid
    // angle by the distance and the triangle's face)
    let p_light = light.solid_angle_pdf * light_source.selection_probability;
@if(DI_GLOSSY_RAY_ONLY) {
    return 0.0;
}
@if((!(DI_GLOSSY_RAY_ONLY))) {
    return power_heuristic(p_light, p_bounce);
}
}

fn reservoir_valid(reservoir: Reservoir) -> bool {
    return reservoir.sample.light_id != NULL_LIGHT_ID;
}

fn pack_reservoir(reservoir: Reservoir) -> vec4<u32> {
    // (a non-finite weight is dropped here, not carried by the reuse from
    // frame to frame)
    var stored = reservoir;
    if !is_finite_f32(stored.unbiased_contribution_weight) || !is_finite_f32(stored.confidence_weight) {
        stored = empty_reservoir();
    }
    let weights = bitcast<vec2<u32>>(vec2<f32>(stored.confidence_weight, stored.unbiased_contribution_weight));
    return vec4<u32>(stored.sample.light_id, stored.sample.seed, weights);
}

fn store_reservoir_a(pixel: vec2<u32>, reservoir: Reservoir) {
    textureStore(di_reservoirs_a, pixel, pack_reservoir(reservoir));
}

fn store_reservoir_b(pixel: vec2<u32>, reservoir: Reservoir) {
    textureStore(di_reservoirs_b, pixel, pack_reservoir(reservoir));
}

fn unpack_reservoir(packed: vec4<u32>) -> Reservoir {
    let weights = bitcast<vec2<f32>>(packed.zw);
    return Reservoir(LightSample(packed.x, packed.y), weights.x, weights.y);
}

fn load_reservoir_a(pixel: vec2<u32>) -> Reservoir {
    return unpack_reservoir(textureLoad(di_reservoirs_a, pixel));
}

fn load_reservoir_b(pixel: vec2<u32>) -> Reservoir {
    return unpack_reservoir(textureLoad(di_reservoirs_b, pixel));
}

struct ReservoirMergeResult {
    merged_reservoir: Reservoir,
    selected_sample_radiance: vec3<f32>,
    wi: vec3<f32>,
    selected_light_world_position: vec4<f32>,
    light: GlossyMisLight,
}

fn merge_reservoirs(
    canonical_reservoir: Reservoir,
    canonical_world_position: vec3<f32>,
    canonical_world_normal: vec3<f32>,
    canonical_lobe: TargetLobe,
    other_reservoir: Reservoir,
    other_world_position: vec3<f32>,
    other_world_normal: vec3<f32>,
    other_lobe: TargetLobe,
    rng: ptr<function, u32>,
) -> ReservoirMergeResult {
    // Resolve each light sample once, then evaluate at both positions
    var canonical_resolved: ResolvedLightSample;
    if reservoir_valid(canonical_reservoir) {
        canonical_resolved = resolve_light_sample(canonical_reservoir.sample, light_sources[canonical_reservoir.sample.light_id >> 16u]);
    }
    var other_resolved: ResolvedLightSample;
    if reservoir_valid(other_reservoir) {
        other_resolved = resolve_light_sample(other_reservoir.sample, light_sources[other_reservoir.sample.light_id >> 16u]);
    }

    // Contributions for resampling (evaluate resolved samples at canonical position)
    let canonical_contribution_canonical_sample = resolved_reservoir_contribution(canonical_reservoir, canonical_resolved, canonical_world_position, canonical_world_normal, canonical_lobe);
    let canonical_contribution_other_sample = resolved_reservoir_contribution(other_reservoir, other_resolved, canonical_world_position, canonical_world_normal, canonical_lobe);

    // Extra contributions for MIS (evaluate resolved samples at other position)
    let other_contribution_canonical_sample = resolved_reservoir_contribution(canonical_reservoir, canonical_resolved, other_world_position, other_world_normal, other_lobe);
    let other_contribution_other_sample = resolved_reservoir_contribution(other_reservoir, other_resolved, other_world_position, other_world_normal, other_lobe);

    // Resampling weight for canonical sample
    let canonical_sample_mis_weight = balance_heuristic(
        canonical_reservoir.confidence_weight * canonical_contribution_canonical_sample.target_function,
        other_reservoir.confidence_weight * other_contribution_canonical_sample.target_function,
    );
    let canonical_sample_resampling_weight = canonical_sample_mis_weight * canonical_contribution_canonical_sample.target_function * canonical_reservoir.unbiased_contribution_weight;

    // Resampling weight for other sample
    let other_sample_mis_weight = balance_heuristic(
        other_reservoir.confidence_weight * other_contribution_other_sample.target_function,
        canonical_reservoir.confidence_weight * canonical_contribution_other_sample.target_function,
    );
    let other_sample_resampling_weight = other_sample_mis_weight * canonical_contribution_other_sample.target_function * other_reservoir.unbiased_contribution_weight;

    // Perform resampling
    var combined_reservoir = empty_reservoir();
    combined_reservoir.confidence_weight = canonical_reservoir.confidence_weight + other_reservoir.confidence_weight;
    let weight_sum = canonical_sample_resampling_weight + other_sample_resampling_weight;

    if rand_f(rng) < other_sample_resampling_weight / weight_sum {
        combined_reservoir.sample = other_reservoir.sample;

        let inverse_target_function = select(0.0, 1.0 / canonical_contribution_other_sample.target_function, canonical_contribution_other_sample.target_function > 0.0);
        combined_reservoir.unbiased_contribution_weight = weight_sum * inverse_target_function;

        return ReservoirMergeResult(combined_reservoir, canonical_contribution_other_sample.radiance, canonical_contribution_other_sample.wi, other_resolved.world_position, canonical_contribution_other_sample.light);
    } else {
        combined_reservoir.sample = canonical_reservoir.sample;

        let inverse_target_function = select(0.0, 1.0 / canonical_contribution_canonical_sample.target_function, canonical_contribution_canonical_sample.target_function > 0.0);
        combined_reservoir.unbiased_contribution_weight = weight_sum * inverse_target_function;

        return ReservoirMergeResult(combined_reservoir, canonical_contribution_canonical_sample.radiance, canonical_contribution_canonical_sample.wi, canonical_resolved.world_position, canonical_contribution_canonical_sample.light);
    }
}

struct ReservoirContribution {
    radiance: vec3<f32>,
    target_function: f32,
    wi: vec3<f32>,
    light: GlossyMisLight,
}

fn resolved_reservoir_contribution(reservoir: Reservoir, resolved: ResolvedLightSample, world_position: vec3<f32>, world_normal: vec3<f32>, lobe: TargetLobe) -> ReservoirContribution {
    if !reservoir_valid(reservoir) { return ReservoirContribution(vec3(0.0), 0.0, vec3(0.0), GlossyMisLight()); }
    let light_contribution = calculate_resolved_light_contribution(resolved, world_position, world_normal);
    let light = GlossyMisLight(reservoir.sample.light_id >> 16u, light_contribution.brdf_rays_can_hit, light_contribution.solid_angle_pdf);
    let target_function = light_target_function(lobe, light_contribution.radiance, light_contribution.wi, world_normal, light);
    return ReservoirContribution(light_contribution.radiance, target_function, light_contribution.wi, light);
}
