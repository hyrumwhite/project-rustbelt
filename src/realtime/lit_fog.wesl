// Lit fog (SolarikRayFog's `lit`): the light the application's fog scatters
// toward the camera from the scene's lights, single scattering along each
// pixel's view ray.
//
// The fog is the application's depth fog (ray_fog.wesl): its coverage c(t)
// along the ray is taken as one minus the medium's transmittance, so the
// scattering coefficient times the transmittance is dc/dt and what the
// medium scatters to the camera is the integral of dc/dt times the light
// arriving at each point (isotropic phase, the fog's colour as its albedo).
// The ray is cut into segments, one is picked by its share of the coverage
// and a point in it is lit by one light: resampled from candidates of the
// light tiles by how brightly each would light it unshadowed, then traced
// for its shadow (the tiles leave the sun out; it has a sample of its own
// when asked for).
//
// The application then fogs the picture by its own fog, which covers what
// lies behind by the coverage at the surface: so over a surface the light
// here is divided by what that fog lets through. The sky is left unfogged
// by it, and here is lit out to the fog's maximum distance.
//
// One light a pixel a frame is noisy, and the denoiser (DLSS-RR) takes the
// picture's light as the surfaces' and would texture it with their albedo:
// so it goes to a texture of its own (`lit_fog_raw`), which
// `lit_fog_accumulate` filters by depth and accumulates over frames, and
// `lit_fog_composite` adds to the picture after the denoiser.

import bevy_core_pipeline::tonemapping::tonemapping_luminance as luminance;
import bevy_pbr::render::utils::rand_f;
import bevy_pbr::render::utils::rand_u;
import bevy_pbr::render::utils::rand_range_u;
import bevy_render::maths::PI;
import bevy_solarik::scene::ray_fog::ray_fog_layers;
import bevy_solarik::scene::ray_fog::ray_fog_active;
import bevy_solarik::scene::ray_fog::fog_decode;
import bevy_solarik::scene::raytracing_scene_bindings::sky_light;
import bevy_solarik::scene::raytracing_scene_bindings::light_sources;
import bevy_solarik::scene::raytracing_scene_bindings::directional_lights;
import bevy_solarik::scene::sampling::LightSample;
import bevy_solarik::scene::sampling::ResolvedLightSample;
import bevy_solarik::scene::sampling::resolve_light_sample;
import bevy_solarik::scene::sampling::calculate_resolved_light_contribution;
import bevy_solarik::scene::sampling::trace_light_visibility;
import bevy_solarik::scene::sampling::LIGHT_SAMPLE_DIRECTIONAL;
import bevy_solarik::realtime::presample_light_tiles::unpack_resolved_light_sample;
import bevy_solarik::realtime::gbuffer_utils::reconstruct_world_position;
import bevy_solarik::realtime::realtime_bindings::light_tile_resolved_samples;
import bevy_solarik::realtime::realtime_bindings::lit_fog_raw;
import bevy_solarik::realtime::realtime_bindings::lit_fog_history_a;
import bevy_solarik::realtime::realtime_bindings::lit_fog_history_b;
import bevy_solarik::realtime::realtime_bindings::depth_buffer;
import bevy_solarik::realtime::realtime_bindings::motion_vectors;
import bevy_solarik::realtime::realtime_bindings::view;
import bevy_solarik::realtime::realtime_bindings::constants;

enable wgpu_ray_query;

const SEGMENTS: u32 = 8u;
// Candidates from the light tiles a point's light is resampled from.
const CANDIDATES: u32 = 16u;
// (the least of the fog's transmittance a surface's light is divided by)
const MINIMUM_TRANSMITTANCE: f32 = 0.05;
// Frames the accumulation averages over at most, and the depth filter's
// radius (pixels) and tolerance (a share of the view depth).
const HISTORY_FRAMES: f32 = 24.0;
const FILTER_RADIUS: i32 = 2;
const DEPTH_TOLERANCE: f32 = 0.05;

// The fog's coverage at a point (its view depth and height over the plane).
fn coverage_at(point: vec3<f32>, forward: vec3<f32>) -> vec4<f32> {
    let plane = sky_light.fog.plane;
    return ray_fog_layers(dot(point - view.world_position, forward), dot(plane.xyz, point) - plane.w);
}

@compute @workgroup_size(8, 8, 1)
fn lit_fog(@builtin(global_invocation_id) global_id: vec3<u32>) {
    if any(global_id.xy >= vec2u(view.main_pass_viewport.zw)) { return; }
    textureStore(lit_fog_raw, global_id.xy, vec4(scattered_light(global_id.xy), 1.0));
}

// The light the fog scatters toward the camera along the pixel's ray (the
// picture's units: at the exposure), divided by what the fog over its
// surface lets through.
fn scattered_light(pixel: vec2<u32>) -> vec3<f32> {
    let lit = sky_light.fog.lit;
    if lit.x <= 0.0 || !ray_fog_active() { return vec3(0.0); }

    let pixel_index = pixel.x + pixel.y * u32(view.main_pass_viewport.z);
    var rng = (pixel_index * 0x9e3779b9u) ^ (constants.frame_index * 0x85ebca6bu) ^ 0x1f0e3dbu;

    let size = view.main_pass_viewport.zw;
    let depth = textureLoad(depth_buffer, pixel, 0);
    let forward = -normalize(view.world_from_view[2].xyz);
    let sky = depth == 0.0;
    var end: vec3<f32>;
    if sky {
        // (the sky: out to where the fog is densest)
        let far = reconstruct_world_position(pixel, 1e-7, size, view.world_from_clip);
        let direction = normalize(far - view.world_position);
        let reach = max(sky_light.fog.atmospheric.y, sky_light.fog.planar.x);
        end = view.world_position + direction * reach / max(dot(direction, forward), 0.05);
    } else {
        end = reconstruct_world_position(pixel, depth, size, view.world_from_clip);
    }
    let ray = end - view.world_position;

    // the coverage at the segments' ends, and its rise over each
    var rises: array<f32, SEGMENTS>;
    var previous = 0.0;
    var total = 0.0;
    for (var i = 0u; i < SEGMENTS; i += 1u) {
        let c = coverage_at(view.world_position + ray * (f32(i + 1u) / f32(SEGMENTS)), forward).a;
        rises[i] = max(c - previous, 0.0);
        total += rises[i];
        previous = max(previous, c);
    }
    if total <= 1e-4 { return vec3(0.0); }

    // a segment by its share, a point in it
    var pick = rand_f(&rng) * total;
    var segment = SEGMENTS - 1u;
    for (var i = 0u; i < SEGMENTS; i += 1u) {
        if pick < rises[i] { segment = i; break; }
        pick -= rises[i];
    }
    let point = view.world_position + ray * ((f32(segment) + rand_f(&rng)) / f32(SEGMENTS));

    // the light arriving there (each sample's radiance over its density)
    var arriving = vec3(0.0);
    let tile_start = rand_range_u(128u, &rng) * 1024u;
    var weight_sum = 0.0;
    var chosen_target = 0.0;
    var chosen_radiance = vec3(0.0);
    var chosen_position = vec4(0.0);
    for (var i = 0u; i < CANDIDATES; i += 1u) {
        let sample = unpack_resolved_light_sample(light_tile_resolved_samples[tile_start + rand_range_u(1024u, &rng)], view.exposure);
        if sample.world_position.w == LIGHT_SAMPLE_DIRECTIONAL { continue; }
        let contribution = calculate_resolved_light_contribution(sample, point, forward);
        let target_function = luminance(contribution.radiance);
        if target_function <= 0.0 { continue; }
        let weight = target_function * contribution.inverse_pdf / f32(CANDIDATES);
        weight_sum += weight;
        if rand_f(&rng) * weight_sum < weight {
            chosen_target = target_function;
            chosen_radiance = contribution.radiance;
            chosen_position = sample.world_position;
        }
    }
    if weight_sum > 0.0 {
        arriving += chosen_radiance * (weight_sum / chosen_target) * trace_light_visibility(point, chosen_position);
    }
    if lit.y > 0.5 && arrayLength(&directional_lights) > 0u {
        let light_id = directional_lights[0].light_source_index;
        let sun = resolve_light_sample(LightSample(light_id << 16u, rand_u(&rng)), light_sources[light_id]);
        let contribution = calculate_resolved_light_contribution(sun, point, forward);
        arriving += contribution.radiance * contribution.inverse_pdf * trace_light_visibility(point, sun.world_position);
    }

    // the fog's colour there as its albedo (its hue, at full brightness)
    let layers = coverage_at(point, forward);
    let colour = fog_decode(layers.rgb / max(layers.a, 1e-4));
    let albedo = colour / max(max(colour.r, max(colour.g, colour.b)), 1e-4);

    var scattered = total * albedo * arriving / (4.0 * PI);
    if !sky {
        scattered /= max(1.0 - previous, MINIMUM_TRANSMITTANCE);
    }
    scattered *= lit.x * view.exposure;
    if !finite_light(vec4(scattered, 0.0)) { return vec3(0.0); }
    return scattered;
}

// (by the bits: a compiler may take x != x as never true)
fn finite_light(light: vec4<f32>) -> bool {
    let finite = all((bitcast<vec4<u32>>(light) & vec4(0x7fffffffu)) < vec4(0x7f800000u));
    return finite && all(abs(light) < vec4(65000.0)) && all(light.rgb >= vec3(0.0));
}

fn view_depth(pixel: vec2<i32>) -> f32 {
    let depth = textureLoad(depth_buffer, pixel, 0);
    if depth == 0.0 { return 1e9; }
    let position = reconstruct_world_position(vec2<u32>(pixel), depth, view.main_pass_viewport.zw, view.world_from_clip);
    return dot(position - view.world_position, -normalize(view.world_from_view[2].xyz));
}

// The frame's scattered light filtered by depth over its neighbours, then
// averaged with the last frames' where the pixel was (by the motion
// vectors), into the parity's history.
@compute @workgroup_size(8, 8, 1)
fn lit_fog_accumulate(@builtin(global_invocation_id) global_id: vec3<u32>) {
    let size = vec2<i32>(view.main_pass_viewport.zw);
    let pixel = vec2<i32>(global_id.xy);
    if any(pixel >= size) { return; }
    if sky_light.fog.lit.x <= 0.0 { return; }

    let centre = view_depth(pixel);
    var sum = vec3(0.0);
    var weights = 0.0;
    for (var y = -FILTER_RADIUS; y <= FILTER_RADIUS; y += 1) {
        for (var x = -FILTER_RADIUS; x <= FILTER_RADIUS; x += 1) {
            let neighbour = pixel + vec2(x, y);
            if any(neighbour < vec2(0)) || any(neighbour >= size) { continue; }
            let d = view_depth(neighbour);
            let weight = select(exp(-abs(d - centre) / (DEPTH_TOLERANCE * centre + 0.1)), 1.0, centre >= 1e9 && d >= 1e9);
            sum += textureLoad(lit_fog_raw, neighbour).rgb * weight;
            weights += weight;
        }
    }
    let current = sum / max(weights, 1e-4);

    let uv = (vec2<f32>(pixel) + 0.5) / vec2<f32>(size);
    let previous_pixel = vec2<i32>(floor((uv - textureLoad(motion_vectors, pixel, 0).xy) * vec2<f32>(size)));
    let parity = constants.render_frame & 1u;
    var history = vec4(0.0);
    if (constants.reset & 1u) == 0u && all(previous_pixel >= vec2(0)) && all(previous_pixel < size) {
        history = select(textureLoad(lit_fog_history_a, previous_pixel), textureLoad(lit_fog_history_b, previous_pixel), parity == 0u);
    }
    // (nothing that isn't a finite light is kept: one would spread through
    // the filter and stay in the history)
    if !finite_light(history) { history = vec4(0.0); }
    var current_light = current;
    if !finite_light(vec4(current, 0.0)) { current_light = vec3(0.0); }
    let frames = min(history.a + 1.0, HISTORY_FRAMES);
    let accumulated = vec4(mix(history.rgb, current_light, 1.0 / frames), frames);
    if parity == 0u {
        textureStore(lit_fog_history_a, global_id.xy, accumulated);
    } else {
        textureStore(lit_fog_history_b, global_id.xy, accumulated);
    }
}
