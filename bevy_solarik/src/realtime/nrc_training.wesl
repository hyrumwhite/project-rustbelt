import bevy_solarik::filtered_canopy_scene::forest_set_seed;
import bevy_solarik::scene::scene_hit::scene_hit_is_miss;
import bevy_pbr::render::utils::rand_f;
import bevy_pbr::render::utils::rand_u;
import bevy_pbr::render::utils::rand_vec2f;
import bevy_pbr::render::pbr_functions::calculate_diffuse_color;
import bevy_pbr::render::pbr_functions::calculate_F0;
import bevy_solarik::scene::brdf::evaluate_brdf;
import bevy_solarik::scene::brdf::evaluate_and_sample_brdf;
import bevy_solarik::scene::brdf::evaluate_brdf_pdf;
import bevy_solarik::scene::sampling::analytic_light_radiance;
import bevy_solarik::scene::sampling::sample_random_light_transmitted;
import bevy_solarik::scene::sampling::random_emissive_light_solid_angle_pdf;
import bevy_solarik::scene::sampling::power_heuristic;
import bevy_solarik::scene::sampling::LightSample;
import bevy_solarik::scene::sampling::resolve_light_sample;
import bevy_solarik::scene::sampling::calculate_resolved_light_contribution;
import bevy_solarik::scene::sampling::trace_light_transmission_with_support;
import bevy_solarik::scene::thin_glass::offset_thin_glass_ray;
import bevy_solarik::scene::raytracing_scene_bindings::trace_ray;
import bevy_solarik::scene::raytracing_scene_bindings::resolve_ray_hit_full;
import bevy_solarik::scene::raytracing_scene_bindings::sample_sky;
import bevy_solarik::scene::raytracing_scene_bindings::emitted_radiance;
import bevy_solarik::scene::raytracing_scene_bindings::light_sources;
import bevy_solarik::scene::raytracing_scene_bindings::directional_lights;
import bevy_solarik::scene::raytracing_scene_bindings::ResolvedMaterial;
import bevy_solarik::scene::raytracing_scene_bindings::RAY_T_MIN;
import bevy_solarik::scene::raytracing_scene_bindings::ray_max_distance;
import bevy_solarik::scene::raytracing_scene_bindings::MIRROR_ROUGHNESS_THRESHOLD;
import bevy_solarik::realtime::gbuffer_utils::gpixel_resolve;
import bevy_solarik::realtime::realtime_bindings::gbuffer;
import bevy_solarik::realtime::realtime_bindings::depth_buffer;
import bevy_solarik::realtime::realtime_bindings::view;
import bevy_solarik::realtime::realtime_bindings::constants;
import bevy_solarik::realtime::realtime_bindings::world_cache_light_slots;
import bevy_solarik::realtime::world_cache_query::find_world_cache_cell;
import bevy_solarik::realtime::world_cache_query::world_cache_slot_probabilities;
import bevy_solarik::realtime::light_ris::sample_light_ris;
import bevy_solarik::realtime::nrc::nrc_constants;
import bevy_solarik::realtime::nrc::NrcPathState;
import bevy_solarik::realtime::nrc::NrcPackedPathVertex;
import bevy_solarik::realtime::nrc::NrcTrainingPathInfo;
import bevy_solarik::realtime::nrc::NRC_COUNTER_QUERIES;
import bevy_solarik::realtime::nrc::NRC_PATH_FLAG_HAS_EXITED_SCENE;
import bevy_solarik::realtime::nrc::NRC_PATH_FLAG_IS_UNBIASED;
import bevy_solarik::realtime::nrc::NRC_PATH_FLAG_PREVIOUS_HIT_WAS_DELTA_LOBE;
import bevy_solarik::realtime::nrc::NRC_PATH_FLAG_HEURISTIC_RESET;
import bevy_solarik::realtime::nrc::NRC_DEBUG_PATH_TERMINATION_REASON_PATH_MISS_EXIT;
import bevy_solarik::realtime::nrc::NRC_DEBUG_PATH_TERMINATION_REASON_BRDF_ABSORPTION;
import bevy_solarik::realtime::nrc::nrc_set_flag;
import bevy_solarik::realtime::nrc::nrc_set_flag_value;
import bevy_solarik::realtime::nrc::nrc_get_flag;
import bevy_solarik::realtime::nrc::nrc_set_vertex_count;
import bevy_solarik::realtime::nrc::nrc_set_debug_path_termination_reason;
import bevy_solarik::realtime::nrc::nrc_evaluate_termination_heuristic;
import bevy_solarik::realtime::nrc::nrc_encode_position;
import bevy_solarik::realtime::nrc::nrc_initialize_packed_path_vertex;
import bevy_solarik::realtime::nrc::nrc_update_training_path_vertex;
import bevy_solarik::realtime::nrc::nrc_unpack_path_vertex_with;
import bevy_solarik::realtime::nrc::nrc_create_radiance_params;
import bevy_solarik::realtime::nrc::nrc_calculate_training_path_index;
import bevy_solarik::realtime::nrc::nrc_calculate_training_path_vertex_index;
import bevy_solarik::realtime::nrc::nrc_pack_training_path_info;
import bevy_solarik::realtime::nrc::nrc_write_packed_path_vertex;
import bevy_solarik::realtime::nrc::nrc_write_radiance_params;
import bevy_solarik::realtime::nrc::nrc_write_training_path_info;
import bevy_solarik::realtime::nrc::nrc_increment_counter;

// The NRC training pass: one short path per thread over the training
// resolution, started at a random camera pixel, written as the training
// records NVIDIA's update-mode shader API writes (NrcCreatePathState,
// NrcUpdateOnHit, NrcSetBrdfPdf, NrcUpdateOnMiss and NrcWriteFinalPathInfo of
// vendor/NRC/Include/Nrc.hlsli, ported here by hand around a bounce loop
// built from surface_path.wesl's pieces).
//
// How the library uses what this writes: each recorded vertex i holds the
// radiance gathered on the segment after it (radiance_i: direct light by NEE
// at i, emission met by i's scatter ray, sky on a miss) and the throughput
// of that segment (throughput_i: the BRDF weights from i up to the next
// recorded vertex). The library reconstructs vertex i's training target, its
// outgoing radiance toward the previous vertex, as
// radiance_i + throughput_i * target_{i+1}, and the last vertex's from the
// self-training query written at the end (the network's own answer there).
// That is why the last vertex is never trained on (only anchored), and why a
// path must record at least two vertices to teach anything.
//
// Radiance handed to NRC is exposed (times view.exposure); the GI resolve
// divides the answers by it. Exposed scene radiance averages 0.1 to 1, which
// fits max_expected_average_radiance_value = 1 and the LogLuv range.
//
// Thin panes: trace_ray passes through them, as the GI sample rays do, and
// their transmission is not applied (a pane would cost a transmission ray per
// segment; GI applies pane energy at its own connection).
enable wgpu_ray_query;

// No query (NrcCreatePathState's queryBufferIndex).
const NRC_NO_QUERY: u32 = 0xffffffffu;
const ONE_OVER_FOUR_PI: f32 = 0.079577471545947667884;

// A surface the path stands on, from the G-buffer (the first vertex) or a ray
// hit (the rest).
struct TrainingSurface {
    world_position: vec3<f32>,
    world_normal: vec3<f32>,
    geometric_world_normal: vec3<f32>,
    material: ResolvedMaterial,
}

// A delta lobe: Solarik's perfect mirror, as surface_path.wesl and the
// specular passes tell it.
fn is_delta_lobe(material: ResolvedMaterial) -> bool {
    return material.roughness <= MIRROR_ROUGHNESS_THRESHOLD && material.metallic > 0.9999;
}

// The training vertex for a surface seen from `wo`, with the attributes
// restir_gi.wesl gives its NRC queries (the two must describe a surface the
// same way, or the network answers queries for inputs it never saw):
// the shading normal, for a leaf the side facing `wo`; albedo and F0 as
// bevy_pbr computes them; linear roughness.
fn training_vertex(surface: TrainingSurface, wo: vec3<f32>) -> NrcPackedPathVertex {
    let material = surface.material;
    var normal = surface.world_normal;
    if material.diffuse_transmission > 0.0 && dot(normal, wo) < 0.0 {
        normal = -normal;
    }
    let albedo = calculate_diffuse_color(material.base_color, material.metallic, 0.0, 0.0);
    let specular = calculate_F0(material.base_color, material.metallic, vec3(material.reflectance));
    return nrc_initialize_packed_path_vertex(material.roughness, normal, wo, albedo, specular,
        nrc_encode_position(surface.world_position, nrc_constants));
}

@compute @workgroup_size(8, 8, 1)
fn nrc_train(@builtin(workgroup_id) workgroup_id: vec3<u32>, @builtin(global_invocation_id) global_id: vec3<u32>) {
    let training_dimensions = nrc_constants.training_dimensions;
    if any(global_id.xy >= training_dimensions) { return; }
    let path_index = nrc_calculate_training_path_index(training_dimensions, global_id.xy);

    // A stream of its own: the training paths start on the same pixels the
    // lighting passes shade, and must not repeat their random numbers.
    var rng = (path_index + constants.frame_index) ^ 0x5bd1e995u;
    let viewport = vec2u(view.main_pass_viewport.zw);
    let jitter = rand_vec2f(&rng);
    let pixel = min(vec2u((vec2f(global_id.xy) + jitter) * vec2f(viewport) / vec2f(training_dimensions)), viewport - 1u);
    let pixel_index = pixel.x + pixel.y * viewport.x;
    forest_set_seed(vec2u(pixel_index, constants.frame_index ^ 0x5bd1e995u));

    // NrcCreatePathState: a share of the paths is "unbiased", traced to the
    // full length without the termination heuristic.
    var state = NrcPathState(0u, NRC_NO_QUERY, 0.0, 0.0, 0u, 0.0);
    state.packed_data = nrc_set_flag_value(state.packed_data, NRC_PATH_FLAG_IS_UNBIASED,
        rand_f(&rng) < nrc_constants.proportion_unbiased);

    let depth = textureLoad(depth_buffer, pixel, 0);
    if depth == 0.0 {
        // The pixel sees the sky: an empty path that left the scene.
        state.packed_data = nrc_set_flag(state.packed_data, NRC_PATH_FLAG_HAS_EXITED_SCENE);
        nrc_write_training_path_info(path_index, nrc_pack_training_path_info(NrcTrainingPathInfo(state.packed_data, NRC_NO_QUERY)));
        return;
    }
    let gpixel = gpixel_resolve(textureLoad(gbuffer, pixel, 0), depth, pixel, view.main_pass_viewport.zw, view.world_from_clip);
    // The G-buffer has no geometric normal; the shading normal stands in for
    // it, as restir_gi.wesl's sample rays do.
    var surface = TrainingSurface(gpixel.world_position, gpixel.world_normal, gpixel.world_normal, gpixel.material);
    var wo = normalize(view.world_position - surface.world_position);
    var hit_distance = distance(view.world_position, surface.world_position);

    // NVIDIA's segment accumulators: reset when a vertex is recorded, so they
    // hold what the path gathered since the last recorded vertex. Radiance is
    // in scene units here and exposed when handed to NRC.
    var radiance = vec3(0.0);
    var throughput = vec3(1.0);
    var vertex_count = 0u;
    // The last recorded vertex, written once its segment is known.
    var pending: NrcPackedPathVertex;

    let max_path_vertices = nrc_constants.max_path_vertices;
    for (var bounce = 0u; bounce < max_path_vertices; bounce += 1u) {
        let delta = is_delta_lobe(surface.material);
        // This vertex's NEE was the cell's resampled light sample (below),
        // which leaves the emission its scatter ray meets to the NEE.
        var ris_nee = false;

        // NrcUpdateOnHit, update mode. The path's spread (a ray cone's
        // footprint) drives the termination heuristic: the primary footprint
        // from the camera distance, then the growth at each later hit from
        // the previous scatter's pdf (a mirror does not widen it).
        let cos_gamma = abs(dot(wo, surface.world_normal));
        if state.primary_spread_radius == 0.0 {
            state.primary_spread_radius = hit_distance / sqrt(cos_gamma * ONE_OVER_FOUR_PI);
        } else if !nrc_get_flag(state.packed_data, NRC_PATH_FLAG_PREVIOUS_HIT_WAS_DELTA_LOBE) {
            state.cumul_spread_radius += hit_distance / sqrt(cos_gamma * state.brdf_pdf);
        }
        state.packed_data = nrc_set_flag_value(state.packed_data, NRC_PATH_FLAG_PREVIOUS_HIT_WAS_DELTA_LOBE, delta);

        if delta {
            // A mirror is not recorded (NVIDIA skips delta vertices whenever
            // the heuristic is on): its outgoing radiance is a sharp image of
            // what it reflects, which a smooth function of position and
            // direction cannot learn, and it would cost a vertex that a
            // surface seen in the mirror needs. The path bounces on and the
            // segment of the vertex before it continues through it. No NEE
            // either: a light sample has no chance of lying on a delta lobe.
            if bounce + 1u == max_path_vertices {
                // Nothing after this hit could be recorded any more.
                break;
            }
        } else {
            if vertex_count > 0u {
                // The previous vertex's segment ends here.
                finish_vertex(pending, vertex_count - 1u, global_id.xy, radiance, throughput);
            }
            vertex_count += 1u;
            state.packed_data = nrc_set_vertex_count(state.packed_data, vertex_count);
            radiance = vec3(0.0);
            throughput = vec3(1.0);
            pending = training_vertex(surface, wo);

            var terminate = bounce + 1u == max_path_vertices;
            if !nrc_get_flag(state.packed_data, NRC_PATH_FLAG_IS_UNBIASED)
                && nrc_evaluate_termination_heuristic(state, nrc_constants.training_termination_heuristic_threshold) {
                // Run to the heuristic twice: reset the spread and go on the
                // first time, stop the second.
                terminate = terminate || nrc_get_flag(state.packed_data, NRC_PATH_FLAG_HEURISTIC_RESET);
                state.packed_data = nrc_set_flag(state.packed_data, NRC_PATH_FLAG_HEURISTIC_RESET);
                state.cumul_spread_radius = 0.0;
            }
            // TerminateImmediately (the cache includes direct light,
            // include_direct_lighting): the last vertex gets no NEE. Its
            // target is the self-training query, whose answer already holds
            // the vertex's direct light; adding NEE would count it twice.
            if terminate { break; }

            // NEE as the world cache's update takes its direct light
            // (world_cache_update.wesl's sample_di): where the vertex finds a
            // cache cell, pad1 samples resampled from 128 candidates of the
            // presampled light tiles and 16 of the cell's light slots (the
            // lights that reach this place), each shadow-tested, averaged,
            // and the sun's own shadow-tested sample. A light picked
            // uniformly from b40's 464 emitters is almost always one that
            // does not light the vertex, so almost every training vertex
            // recorded no direct light and a rare one a huge value, which
            // the network under-fitted (its answers 3.6 times darker than
            // the cache's in the shaft). The slots are only read: the
            // cache's update owns them. The full BRDF is applied to the
            // pick (the sampler's target is the receiver's irradiance, so
            // its weight is the radiance without the cosine).
            //
            // A leaf (diffuse transmission) keeps the uniform sampler: the
            // resampling's target is zero for a light behind the surface,
            // which a leaf transmits; so does a vertex without a cell.
            if arrayLength(&light_sources) > 0u {
                let light_samples = max(nrc_constants.pad1, 1u);
                var direct = vec3(0.0);
                var cell = -1;
                if surface.material.diffuse_transmission <= 0.0 {
                    cell = find_world_cache_cell(surface.world_position, surface.world_normal, view.world_position, &rng);
                }
                if cell >= 0 {
                    // MIS: a resampled pick has no density of its own to
                    // weigh against the BRDF's, so the NEE takes weight 1
                    // and the emission met by this vertex's scatter ray
                    // weight 0 (below): every emissive mesh is among the
                    // tiles' candidates, so the NEE alone covers them.
                    ris_nee = true;
                    let slots = world_cache_light_slots[cell];
                    let slot_probabilities = world_cache_slot_probabilities(slots);
                    for (var s = 0u; s < light_samples; s++) {
                        // (each sample from a tile of its own, as sample_di)
                        let light = sample_light_ris(surface.world_position, surface.world_normal,
                            workgroup_id.xy + vec2(0u, s * 7919u), slots, slot_probabilities, &rng);
                        if any(light.radiance > vec3(0.0)) {
                            direct += light.radiance * evaluate_brdf(wo, light.wi, surface.world_normal, surface.material);
                        }
                    }
                    radiance += direct / f32(light_samples);
                    // The sun by itself (the tiles hold none), as sample_di:
                    // reached by NEE alone, weight 1 (a scatter ray that
                    // leaves the scene meets the sky texture, not the sun).
                    if arrayLength(&directional_lights) > 0u {
                        let light_id = directional_lights[0].light_source_index;
                        let resolved = resolve_light_sample(LightSample(light_id << 16u, rand_u(&rng)), light_sources[light_id]);
                        let sun = calculate_resolved_light_contribution(resolved, surface.world_position, surface.world_normal);
                        let sun_radiance = sun.radiance * sun.inverse_pdf;
                        if any(sun_radiance > vec3(0.0)) && dot(sun.wi, surface.world_normal) > 0.0 {
                            let shadow = trace_light_transmission_with_support(surface.world_position + (surface.world_normal * RAY_T_MIN), resolved.world_position);
                            radiance += sun_radiance * shadow.energy.rgb
                                * evaluate_brdf(wo, sun.wi, surface.world_normal, surface.material);
                        }
                    }
                } else {
                    // The uniform sampler, as surface_path.wesl does it: MIS
                    // by the power heuristic against the BRDF sample that
                    // follows (throughput is 1 here, just reset). pad1 light
                    // samples, averaged: each one pairs with the single BRDF
                    // sample as an estimator of its own, so the average
                    // stays unbiased and the emission weight below is
                    // unchanged.
                    for (var s = 0u; s < light_samples; s++) {
                        let shadow = sample_random_light_transmitted(surface.world_position,
                            surface.world_normal, surface.geometric_world_normal, &rng);
                        let light = shadow.light;
                        var weight = 1.0;
                        if light.brdf_rays_can_hit {
                            weight = power_heuristic(light.solid_angle_pdf,
                                evaluate_brdf_pdf(wo, light.wi, surface.world_normal, surface.material) * shadow.continuation_probability);
                        }
                        direct += light.radiance * light.inverse_pdf * weight
                            * evaluate_brdf(wo, light.wi, surface.world_normal, surface.material);
                    }
                    radiance += direct / f32(light_samples);
                }
            }
        }

        // Scatter (NrcSetBrdfPdf keeps the pdf for the next hit's spread).
        let next = evaluate_and_sample_brdf(wo, surface.world_normal, surface.material, &rng);
        if next.pdf <= 0.0 {
            throughput = vec3(0.0);
            state.packed_data = nrc_set_debug_path_termination_reason(state.packed_data, NRC_DEBUG_PATH_TERMINATION_REASON_BRDF_ABSORPTION);
            break;
        }
        throughput *= next.throughput;
        state.brdf_pdf = next.pdf;

        let wi = next.wi;
        let origin = offset_thin_glass_ray(surface.world_position, surface.geometric_world_normal, wi, RAY_T_MIN);
        let ray = trace_ray(origin, wi, RAY_T_MIN, ray_max_distance(), RAY_FLAG_NONE);
        // Point and spot lights are spheres no ray hits; NEE samples them,
        // except behind a mirror, where only this ray can find them
        // (surface_path.wesl's ownership rule).
        radiance += throughput * analytic_light_radiance(origin, wi,
            select(ray.t, ray_max_distance(), scene_hit_is_miss(ray)), delta, surface.world_position);
        if scene_hit_is_miss(ray) {
            // NrcUpdateOnMiss: the sky ends the segment, and the path is not
            // continued by a self-training query (nothing lies beyond it).
            radiance += throughput * sample_sky(wi);
            state.packed_data = nrc_set_debug_path_termination_reason(state.packed_data, NRC_DEBUG_PATH_TERMINATION_REASON_PATH_MISS_EXIT);
            state.packed_data = nrc_set_flag(state.packed_data, NRC_PATH_FLAG_HAS_EXITED_SCENE);
            break;
        }
        let candidate = resolve_ray_hit_full(ray);
        // Emission met by the scatter ray belongs to the segment of the
        // vertex it left, MIS-weighted against that vertex's NEE: weight 1
        // after a mirror, where there was no NEE; 0 after a resampled NEE,
        // which has no single light density to weigh against and takes
        // weight 1 itself (counting the emission here too would count the
        // light twice); the power heuristic after the uniform sampler's NEE.
        var emission_weight = 1.0;
        if ris_nee {
            emission_weight = 0.0;
        } else if !delta {
            emission_weight = power_heuristic(next.pdf,
                random_emissive_light_solid_angle_pdf(candidate, surface.world_position));
        }
        radiance += throughput * emitted_radiance(candidate.material, -wi) * emission_weight;

        surface = TrainingSurface(candidate.world_position, candidate.world_normal, candidate.geometric_world_normal, candidate.material);
        wo = -wi;
        hit_distance = ray.t;
    }

    // NrcWriteFinalPathInfo, update mode.
    if vertex_count > 0u {
        finish_vertex(pending, vertex_count - 1u, global_id.xy, radiance, throughput);
        // The self-training query at the last vertex, for every path that
        // did not leave the scene (unbiased ones too): without it a vertex
        // near the end of a path would learn less indirect light than one
        // near its start.
        if !nrc_get_flag(state.packed_data, NRC_PATH_FLAG_HAS_EXITED_SCENE) && max_path_vertices > 1u {
            let vertex = nrc_unpack_path_vertex_with(pending, radiance * view.exposure, throughput);
            state.query_buffer_index = nrc_increment_counter(NRC_COUNTER_QUERIES);
            nrc_write_radiance_params(state.query_buffer_index, nrc_create_radiance_params(vertex));
        }
    }
    nrc_write_training_path_info(path_index, nrc_pack_training_path_info(NrcTrainingPathInfo(state.packed_data, state.query_buffer_index)));
}

// NrcUpdateTrainingPathVertex and the write: a vertex's record with the
// segment gathered after it (radiance exposed).
fn finish_vertex(vertex: NrcPackedPathVertex, vertex_index: u32, path: vec2<u32>, radiance: vec3<f32>, throughput: vec3<f32>) {
    let index = nrc_calculate_training_path_vertex_index(nrc_constants.training_dimensions, path, vertex_index, nrc_constants.max_path_vertices);
    nrc_write_packed_path_vertex(index, nrc_update_training_path_vertex(vertex, radiance * view.exposure, throughput));
}
