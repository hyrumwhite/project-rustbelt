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
import bevy_solarik::realtime::gbuffer_utils::permute_pixel;
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
import bevy_solarik::scene::sampling::trace_light_transmission;
import bevy_solarik::scene::sampling::balance_heuristic;
import bevy_solarik::scene::raytracing_scene_bindings::light_sources;
import bevy_solarik::scene::raytracing_scene_bindings::directional_lights;
import bevy_solarik::scene::raytracing_scene_bindings::LIGHT_SOURCE_KIND_DIRECTIONAL;
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
import bevy_solarik::realtime::realtime_bindings::ResolvedLightSamplePacked;

// https://intro-to-restir.cwyman.org/presentations/2023ReSTIR_Course_Notes.pdf
// https://d1qx31qr3h6wln.cloudfront.net/publications/ReSTIR%20GI.pdf

enable wgpu_ray_query;


// (32 and a confidence cap of 8, from 8 and 20: a sample reused for 20
// frames over 30 pixels made patches of light that came and went, which
// DLSS-RR keeps as detail; interiors boiled)
const INITIAL_SAMPLES = 32u;
// Candidates from the lights in sight of the pixel's world cache cell, beside
// the tiles' (which draw from the whole level by power), with
// on by default (SOLARIK_CELL_LIGHTS=0 leaves them out): a narrow corridor's recessed fixtures, among hundreds
// of the level's lights, got a sample now and then. In a10's corridors, in
// motion, 8% less boiling for 0.6 ms (the tiles alone, since the sun has its
// own sample, mostly find them). (With the slots weighted by what each light
// gives the cell, and half the screen finding its cell again, in a10's
// Jefferies tubes a wall beside a dim red strip went from lit and dark by
// turns to steady, its three-shot spread 0.35 to 0.01; 24 did no better.)
const CELL_SAMPLES = 8u;
const SPATIAL_REUSE_RADIUS_PIXELS = 30.0;
const CONFIDENCE_WEIGHT_CAP = 8.0;
@if((RASTER_SUBPIXEL_BITS))
const RASTER_SUBPIXEL_STEP = 1.0 / f32(1u << constants::RASTER_SUBPIXEL_BITS);
// Vulkan's guaranteed minimum, used by tools without adapter specialization.
@if((!(RASTER_SUBPIXEL_BITS)))
const RASTER_SUBPIXEL_STEP = 1.0 / 16.0;

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

    let diffuse_brdf = target_brdf(surface.material);
    let initial_reservoir = generate_initial_reservoir(surface.world_position, surface.world_normal, diffuse_brdf, workgroup_id.xy, &rng);
    let temporal = load_temporal_reservoir(global_id.xy, depth, surface.world_position, surface.world_normal);
    let merge_result = merge_reservoirs(initial_reservoir, surface.world_position, surface.world_normal, diffuse_brdf,
        temporal.reservoir, temporal.world_position, temporal.world_normal, temporal.diffuse_brdf, &rng);

    store_reservoir_b(global_id.xy, merge_result.merged_reservoir);
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

    let diffuse_brdf = target_brdf(surface.material);
    let input_reservoir = load_reservoir_b(global_id.xy);
    let spatial = load_spatial_reservoir(global_id.xy, depth, surface.world_position, surface.world_normal, &rng);
    let merge_result = merge_reservoirs(input_reservoir, surface.world_position, surface.world_normal, diffuse_brdf,
        spatial.reservoir, spatial.world_position, spatial.world_normal, spatial.diffuse_brdf, &rng);
    let combined_reservoir = merge_result.merged_reservoir;

    // Reusable weights use an unoccluded target in both resampling modes.
    // RGB connection transport belongs only to this receiver's shading.
    store_reservoir_a(global_id.xy, combined_reservoir);

    let wo = normalize(view.world_position - surface.world_position);
    var brdf = evaluate_diffuse_brdf(wo, merge_result.wi, surface.world_normal, surface.material);
    // Only consider a specular lobe if it is not smooth, else leave it for the specular GI pass to handle
    if surface.material.roughness > SPECULAR_GI_FOR_DI_ROUGHNESS_THRESHOLD {
        brdf += evaluate_base_specular_brdf(wo, merge_result.wi, surface.world_normal, surface.material);
    }
    if clearcoat_roughness(surface.material) > SPECULAR_GI_FOR_DI_ROUGHNESS_THRESHOLD {
        brdf += evaluate_clearcoat_brdf(wo, merge_result.wi, surface.world_normal, surface.material);
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
    let transmission = trace_light_transmission(offset_surface_ray(world_position, world_normal, view.world_position, raster_error),
        result.selected_light_world_position);
    return result.selected_sample_radiance * result.merged_reservoir.unbiased_contribution_weight * transmission.rgb;
}

fn generate_initial_reservoir(world_position: vec3<f32>, world_normal: vec3<f32>, diffuse_brdf: vec3<f32>, workgroup_id: vec2<u32>, rng: ptr<function, u32>) -> Reservoir {
    var workgroup_rng = light_tile_seed((workgroup_id.x * 5782582u) + workgroup_id.y, lighting_constants.frame_index);
    let light_tile_start = rand_range_u(128u, &workgroup_rng) * 1024u;

    var reservoir = empty_reservoir();
    var weight_sum = 0.0;

    // Two strategies pick a light: the tiles, by the table's selection
    // probability (over what isn't the sun's, presample_light_tiles), and the
    // cell's slots, by the light's weight in them; a point on the light is
    // picked alike after. Each candidate is weighted by the balance heuristic
    // of the two, so a light the cell missed still comes by the tiles.
    var sun_share = 1.0;
    if arrayLength(&directional_lights) > 0u {
        let sun_probability = light_sources[directional_lights[0].light_source_index].selection_probability;
        if sun_probability < 0.999 { sun_share = 1.0 - sun_probability; }
    }
    var light_slots = vec4(WORLD_CACHE_NO_LIGHT);
@if(DI_CELL_LIGHTS) {
    let cell = find_world_cache_cell(world_position, world_normal, view.world_position, rng);
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

    var reservoir_target_function = 0.0;
    var selected_sample = LightSample(0u, 0u);
    var selected_tile_sample = 0xFFFFFFFFu;
    var selected_light_position = vec4(0.0);
    for (var i = 0u; i < INITIAL_SAMPLES; i++) {
        let tile_sample = light_tile_start + rand_range_u(1024u, rng);
        let resolved_light_sample = unpack_resolved_light_sample(light_tile_resolved_samples[tile_sample], view.exposure);
        // (the sun has a sample of its own, below)
        if resolved_light_sample.world_position.w == LIGHT_SAMPLE_DIRECTIONAL { continue; }
        let light_contribution = calculate_resolved_light_contribution(resolved_light_sample, world_position, world_normal);
        let target_function = luminance(light_contribution.radiance * diffuse_brdf * saturate(dot(light_contribution.wi, world_normal)));
        if target_function <= 0.0 { continue; }

        var resampling_weight = target_function * resolved_light_sample.inverse_pdf / f32(INITIAL_SAMPLES);
        if cell_samples > 0u {
            let light_id = light_tile_samples[tile_sample].light_id >> 16u;
            let tile_probability = light_sources[light_id].selection_probability / sun_share;
            let cell_probability = world_cache_slot_light_probability(light_slots, slot_probabilities, light_id);
            resampling_weight = target_function * resolved_light_sample.inverse_pdf * tile_probability
                / (f32(INITIAL_SAMPLES) * tile_probability + f32(cell_samples) * cell_probability);
        }

        weight_sum += resampling_weight;
        if rand_f(rng) < resampling_weight / weight_sum {
            reservoir_target_function = target_function;
            selected_tile_sample = tile_sample;
            selected_light_position = resolved_light_sample.world_position;
        }
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
        let resolved_light_sample = resolve_light_sample(light_sample, light_source);
        let light_contribution = calculate_resolved_light_contribution(resolved_light_sample, world_position, world_normal);
        let target_function = luminance(light_contribution.radiance * diffuse_brdf * saturate(dot(light_contribution.wi, world_normal)));
        if target_function <= 0.0 { continue; }

        let tile_probability = light_source.selection_probability / sun_share;
        let cell_probability = world_cache_slot_light_probability(light_slots, slot_probabilities, light_id);
        let resampling_weight = target_function * resolved_light_sample.inverse_pdf
            / (f32(INITIAL_SAMPLES) * tile_probability + f32(cell_samples) * cell_probability);

        weight_sum += resampling_weight;
        if rand_f(rng) < resampling_weight / weight_sum {
            reservoir_target_function = target_function;
            selected_tile_sample = 0xFFFFFFFFu;
            selected_sample = light_sample;
            selected_light_position = resolved_light_sample.world_position;
        }
    }

    var sun_selected = false;
    // The sun, one sample of its own in every pixel, shadow-tested before
    // it competes: indoors, out of sight behind the hull, it was the
    // brightest candidate yet lit nothing, and won most pixels (a fixed
    // share of the tiles' samples was the sun's); outdoors, it reaches
    // every pixel instead of half.
    if arrayLength(&directional_lights) > 0u {
        let light_id = directional_lights[0].light_source_index;
        let candidate = LightSample(light_id << 16u, rand_u(rng));
        let resolved = resolve_light_sample(candidate, light_sources[light_id]);
        let contribution = calculate_resolved_light_contribution(resolved, world_position, world_normal);
        let target_function = luminance(contribution.radiance * diffuse_brdf * saturate(dot(contribution.wi, world_normal)));
        if target_function > 0.0 {
            let transmission = trace_light_transmission(offset_surface_ray(world_position, world_normal, view.world_position, 0.0), resolved.world_position);
            if max(transmission.r, max(transmission.g, transmission.b)) > 0.0 {
                let resampling_weight = target_function * contribution.inverse_pdf;
                weight_sum += resampling_weight;
                if rand_f(rng) < resampling_weight / weight_sum {
                    reservoir_target_function = target_function;
                    sun_selected = true;
                    selected_sample = candidate;
                    selected_light_position = resolved.world_position;
                }
            }
        }
    }

    if reservoir_target_function != 0.0 {
        reservoir.sample = selected_sample;
        if selected_tile_sample != 0xFFFFFFFFu && !sun_selected {
            reservoir.sample = light_tile_samples[selected_tile_sample];
        }
    }

    if reservoir_valid(reservoir) {
        let inverse_target_function = select(0.0, 1.0 / reservoir_target_function, reservoir_target_function > 0.0);
        reservoir.unbiased_contribution_weight = weight_sum * inverse_target_function;
        // A light out of sight lights nothing: it is dropped before reuse
        // spreads it (over 30 pixels and 8 frames, the patches of a corridor
        // lit or dark as one hidden light came and went). The sun was tested
        // above.
        if !sun_selected {
            let transmission = trace_light_transmission(offset_surface_ray(world_position, world_normal, view.world_position, 0.0), selected_light_position);
            if max(transmission.r, max(transmission.g, transmission.b)) <= 0.0 {
                reservoir.unbiased_contribution_weight = 0.0;
            }
        }
    }

    reservoir.confidence_weight = 1.0;
    return reservoir;
}

fn load_temporal_reservoir(pixel_id: vec2<u32>, depth: f32, world_position: vec3<f32>, world_normal: vec3<f32>) -> NeighborInfo {
    let motion_vector = textureLoad(motion_vectors, pixel_id, 0).xy;
    let temporal_pixel_id_float = round(vec2<f32>(pixel_id) - (motion_vector * view.main_pass_viewport.zw));
    var point_temporal_pixel_id = vec2<u32>(temporal_pixel_id_float);

    // DI reservoirs retain unoccluded weights; final visibility is traced now.
    // Global events and the legacy radius path keep their full fallback.
    if (lighting_constants.reset & 1u) != 0u || (lighting_constants.reset & 6u) == 2u {
        return NeighborInfo(empty_reservoir(), vec3(0.0), vec3(0.0), vec3(0.0));
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
        return NeighborInfo(empty_reservoir(), vec3(0.0), vec3(0.0), vec3(0.0));
    }
    temporal.reservoir.sample.light_id = (light_id << 16u) | triangle_id;

    temporal.reservoir.confidence_weight = min(temporal.reservoir.confidence_weight, CONFIDENCE_WEIGHT_CAP);

    return temporal;
}

fn load_temporal_reservoir_inner(temporal_pixel_id: vec2<u32>, depth: f32, world_position: vec3<f32>, world_normal: vec3<f32>) -> NeighborInfo {
    // Check if the pixel features have changed heavily between the current and previous frame
    let temporal_depth = textureLoad(previous_depth_buffer, temporal_pixel_id, 0);
    let temporal_surface = gpixel_resolve(textureLoad(previous_gbuffer, temporal_pixel_id, 0), temporal_depth, temporal_pixel_id, view.main_pass_viewport.zw, previous_view.world_from_clip);
    let temporal_diffuse_brdf = target_brdf(temporal_surface.material);
    if pixel_dissimilar(depth, world_position, temporal_surface.world_position, world_normal, temporal_surface.world_normal, view) {
        return NeighborInfo(empty_reservoir(), vec3(0.0), vec3(0.0), vec3(0.0));
    }

    let temporal_reservoir = load_reservoir_a(temporal_pixel_id);
    return NeighborInfo(temporal_reservoir, temporal_surface.world_position, temporal_surface.world_normal, temporal_diffuse_brdf);
}

fn load_spatial_reservoir(pixel_id: vec2<u32>, depth: f32, world_position: vec3<f32>, world_normal: vec3<f32>, rng: ptr<function, u32>) -> NeighborInfo {
    for (var i = 0u; i < 5u; i++) {
        let spatial_pixel_id = get_neighbor_pixel_id(pixel_id, SPATIAL_REUSE_RADIUS_PIXELS, rng);

        let spatial_depth = textureLoad(depth_buffer, spatial_pixel_id, 0);
        let spatial_surface = gpixel_resolve(textureLoad(gbuffer, spatial_pixel_id, 0), spatial_depth, spatial_pixel_id, view.main_pass_viewport.zw, view.world_from_clip);
        let spatial_diffuse_brdf = target_brdf(spatial_surface.material);
        if pixel_dissimilar(depth, world_position, spatial_surface.world_position, world_normal, spatial_surface.world_normal, view) {
            continue;
        }

        let spatial_reservoir = load_reservoir_b(spatial_pixel_id);
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

fn reservoir_valid(reservoir: Reservoir) -> bool {
    return reservoir.sample.light_id != NULL_LIGHT_ID;
}

fn pack_reservoir(reservoir: Reservoir) -> vec4<u32> {
    let weights = bitcast<vec2<u32>>(vec2<f32>(reservoir.confidence_weight, reservoir.unbiased_contribution_weight));
    return vec4<u32>(reservoir.sample.light_id, reservoir.sample.seed, weights);
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
    let canonical_contribution_canonical_sample = resolved_reservoir_contribution(canonical_reservoir, canonical_resolved, canonical_world_position, canonical_world_normal, canonical_diffuse_brdf);
    let canonical_contribution_other_sample = resolved_reservoir_contribution(other_reservoir, other_resolved, canonical_world_position, canonical_world_normal, canonical_diffuse_brdf);

    // Extra contributions for MIS (evaluate resolved samples at other position)
    let other_contribution_canonical_sample = resolved_reservoir_contribution(canonical_reservoir, canonical_resolved, other_world_position, other_world_normal, other_diffuse_brdf);
    let other_contribution_other_sample = resolved_reservoir_contribution(other_reservoir, other_resolved, other_world_position, other_world_normal, other_diffuse_brdf);

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

        return ReservoirMergeResult(combined_reservoir, canonical_contribution_other_sample.radiance, canonical_contribution_other_sample.wi, other_resolved.world_position);
    } else {
        combined_reservoir.sample = canonical_reservoir.sample;

        let inverse_target_function = select(0.0, 1.0 / canonical_contribution_canonical_sample.target_function, canonical_contribution_canonical_sample.target_function > 0.0);
        combined_reservoir.unbiased_contribution_weight = weight_sum * inverse_target_function;

        return ReservoirMergeResult(combined_reservoir, canonical_contribution_canonical_sample.radiance, canonical_contribution_canonical_sample.wi, canonical_resolved.world_position);
    }
}

struct ReservoirContribution {
    radiance: vec3<f32>,
    target_function: f32,
    wi: vec3<f32>,
}

fn resolved_reservoir_contribution(reservoir: Reservoir, resolved: ResolvedLightSample, world_position: vec3<f32>, world_normal: vec3<f32>, diffuse_brdf: vec3<f32>) -> ReservoirContribution {
    if !reservoir_valid(reservoir) { return ReservoirContribution(vec3(0.0), 0.0, vec3(0.0)); }
    let light_contribution = calculate_resolved_light_contribution(resolved, world_position, world_normal);
    let target_function = luminance(light_contribution.radiance * diffuse_brdf * saturate(dot(light_contribution.wi, world_normal)));
    return ReservoirContribution(light_contribution.radiance, target_function, light_contribution.wi);
}
