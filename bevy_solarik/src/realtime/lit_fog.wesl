// Lit fog (SolarikRayFog's `lit`): the light the application's fog scatters
// toward the camera from the scene's lights, single scattering along each
// pixel's view ray.
//
// The fog is the application's depth fog (ray_fog.wesl): its coverage c(t)
// along the ray is taken as one minus the medium's transmittance, so the
// scattering coefficient times the transmittance is dc/dt and what the
// medium scatters to the camera is the integral of dc/dt times the light
// arriving at each point (isotropic phase, the fog's colour as its albedo).
// The ray is cut into segments and dc/dt taken as constant in each. The
// local lights are resampled from candidates of the light tiles, each
// candidate a pair of a light's sample and a point on the ray of its own,
// the point drawn half the time by the coverage and half the time by the
// equiangular density about the light (the density of a point seen from
// the light at a uniform angle, which follows the light's 1/d^2 along the
// ray), weighted by how brightly the light would light it unshadowed times
// dc/dt there; the chosen pair is traced for its shadow. A point by the
// coverage alone, the light after it, took a light's 1/d^2 whole wherever
// it happened to fall near one: a few pixels a frame hundreds of times the
// rest, which the filter spread into squares (SOLARIK_SHADER_DEFS=
// LIT_FOG_COVERAGE_POINT the old). The tiles leave the sun out; it has a
// sample of its own when asked for, at a point by the coverage.
//
// The application then fogs the picture by its own fog, which covers what
// lies behind by the coverage at the surface. Where it lays this light over
// that fog itself (lit.w: halo_rt's fog, laid at the render resolution before
// the transparent pass, src/shaders/halo_fog.wesl's `before`, at the same
// pixel), the light is left as it is. Else `lit_fog_composite` adds it before
// tonemapping and over a surface it is divided here by what that fog lets
// through, which a fog taken at other depths (at silhouettes, behind
// first-person panes, on a sky the fog covers) makes outlines and losses of.
// The sky is left unfogged by the application, and here is lit out to the
// fog's maximum distance.
//
// One light a pixel a frame is noisy, and the denoiser (DLSS-RR) takes the
// picture's light as the surfaces' and would texture it with their albedo:
// so it goes to a texture of its own (`lit_fog_raw`, its alpha the pixel's
// view depth), which `lit_fog_filter` prefilters by depth in LIT_FOG_LEVELS
// a-trous levels (a 5x5 binomial at strides 1, 2, 4, ..., taking
// `lit_fog_raw` and `lit_fog_scratch` in turn), `lit_fog_accumulate`
// accumulates over up to LIT_FOG_FRAMES frames, its history clamped to a band
// of LIT_FOG_CLAMP hundredths about the prefiltered light, widened by
// LIT_FOG_CLAMP_NOISE tenths of that light's own noise at the pixel (the last
// a-trous level's difference from the one before it, averaged over the
// history's 3x3), and shortened by what the clamp moved (its frame count
// times the share kept to the power LIT_FOG_CLAMP_POWER), and
// `lit_fog_composite` adds to the picture after the denoiser (or the
// application reads it, SolarikLightingResources::lit_fog_light, lit.w). The estimate
// was a 5x5 inside `lit_fog_accumulate` over 24 frames, unclamped, and its
// grain crawled (SOLARIK_LIT_FOG_LEVELS=0 SOLARIK_LIT_FOG_FRAMES=24
// SOLARIK_LIT_FOG_CLAMP=0 the old; SOLARIK_LIT_FOG_CLAMP_POWER another power
// than the 2 it is built with; SOLARIK_LIT_FOG_CLAMP_NOISE=0 the band without
// the noise, the old; node.rs and tuning.wesl for the knobs).

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
import bevy_solarik::scene::raytracing_scene_bindings::has_sun;
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
import bevy_solarik::realtime::realtime_bindings::lit_fog_scratch;
import bevy_solarik::realtime::tuning::LIT_FOG_LEVELS;
import bevy_solarik::realtime::tuning::LIT_FOG_FRAMES;
import bevy_solarik::realtime::tuning::LIT_FOG_CLAMP;
import bevy_solarik::realtime::tuning::LIT_FOG_CLAMP_POWER;
import bevy_solarik::realtime::tuning::LIT_FOG_CLAMP_NOISE;
import bevy_solarik::realtime::tuning::LIT_FOG_DEPTH;
import bevy_solarik::realtime::tuning::LIT_FOG_REACH;
import bevy_solarik::realtime::tuning::LIT_FOG_CANDIDATES;
import bevy_solarik::realtime::realtime_bindings::depth_buffer;
import bevy_solarik::realtime::realtime_bindings::motion_vectors;
import bevy_solarik::realtime::realtime_bindings::view;
import bevy_solarik::realtime::realtime_bindings::constants;

enable wgpu_ray_query;

const SEGMENTS: u32 = 8u;
// Candidates from the light tiles a point's light is resampled from
// (SOLARIK_LIT_FOG_CANDIDATES, held to 1 to 64; 16 as the constant was).
const CANDIDATES: u32 = min(max(LIT_FOG_CANDIDATES, 1u), 64u);
// (the least of the fog's transmittance a surface's light is divided by)
const MINIMUM_TRANSMITTANCE: f32 = 0.05;
// The least distance (metres) of a light from the ray that the equiangular
// density is drawn about: the density's width is kept from collapsing where
// the ray passes through a light's point. The estimate stays unbiased, the
// density is only wider than the light's own there.
const EQUIANGULAR_NEAREST: f32 = 0.05;
// Frames the accumulation averages over at most (SOLARIK_LIT_FOG_FRAMES,
// held to 1 to 1024; 24 the old), and the depth filter's radius (pixels) and
// tolerance (a share of the view depth: SOLARIK_LIT_FOG_DEPTH in thousandths,
// at least 1; 0.05 as the constant was).
const HISTORY_FRAMES: f32 = f32(min(max(LIT_FOG_FRAMES, 1u), 1024u));
const FILTER_RADIUS: i32 = 2;
const DEPTH_TOLERANCE: f32 = f32(max(LIT_FOG_DEPTH, 1u)) / 1000.0;
// (the filter's taps: the 5x5 of FILTER_RADIUS 2)
const FILTER_TAPS: u32 = 25u;
// A tap's light is bounded to this many times the depth-weighted mean of
// the filter's taps before it is summed.
const FIREFLY_BOUND: f32 = 8.0;
// The spacing (pixels) of the 3x3 the history is read through.
const HISTORY_BLUR_STRIDE: i32 = 2;
// The a-trous prefilter's levels (SOLARIK_LIT_FOG_LEVELS, held to 6; 0 none,
// the old 5x5 inside lit_fog_accumulate). The diagnostics LIT_FOG_RAW and
// LIT_FOG_BOX take the path of no prefilter whatever the levels:
// lit_fog_filter does nothing and lit_fog_accumulate reads lit_fog_raw as it
// did.
@if(LIT_FOG_RAW || LIT_FOG_BOX)
const PREFILTER_LEVELS: u32 = 0u;
@if(!(LIT_FOG_RAW || LIT_FOG_BOX))
const PREFILTER_LEVELS: u32 = min(LIT_FOG_LEVELS, 6u);
// The history's clamp: its band about the prefiltered light
// (SOLARIK_LIT_FOG_CLAMP in hundredths; 0 none, the old), used only with the
// prefilter, whose light is steady enough to clamp to; and a floor in the
// stored light's units, so that a light going fully out leaves no tail of
// the old level.
const CLAMP_BAND: f32 = f32(LIT_FOG_CLAMP) / 100.0;
const CLAMP_FLOOR: f32 = 0.0005;
// The power of the share of its luminance a clamped history kept by which
// its frame count is scaled (SOLARIK_LIT_FOG_CLAMP_POWER, held to 1 to 64; 2
// by default): 8 and 24 were tried against a muzzle flash in thick fog and
// followed it no sooner than 2, within three frames each way, and at 8 two
// still patches of four read a little grainier, so 2 stayed.
const CLAMP_POWER: f32 = f32(min(max(LIT_FOG_CLAMP_POWER, 1u), 64u));
// The clamp's band widened either way by this many times the prefiltered
// light's own noise at the pixel, per channel (SOLARIK_LIT_FOG_CLAMP_NOISE in
// tenths, held to 200; 40 by default; 0 the old band): in dark far fog one
// frame's prefiltered light is off by more than CLAMP_BAND, so the band alone
// chased each frame's noise, the history never grew long and soft blobs tens
// of pixels wide crawled (a50's corridor at strength 3: 1.50 of 255 against
// 0.12 unclamped). The noise is the last a-trous level's difference from the
// level before it, which the other texture of the pair still holds, so it
// needs two levels at least and is nothing with fewer.
const CLAMP_NOISE_ON: bool = PREFILTER_LEVELS >= 2u && LIT_FOG_CLAMP_NOISE > 0u;
const CLAMP_NOISE: f32 = f32(min(LIT_FOG_CLAMP_NOISE, 200u)) / 10.0;
// The view depth as lit_fog_raw's alpha keeps it (an f16): a surface's held
// under 60000, the sky's 65000.
const STORED_DEPTH_MAX: f32 = 60000.0;
const STORED_SKY: f32 = 65000.0;

// The fog's coverage at a point (its view depth and height over the plane).
fn coverage_at(point: vec3<f32>, forward: vec3<f32>) -> vec4<f32> {
    let plane = sky_light.fog.plane;
    return ray_fog_layers(dot(point - view.world_position, forward), dot(plane.xyz, point) - plane.w);
}

// The fog's colour at a point as its albedo (its hue, at full brightness).
fn fog_albedo(point: vec3<f32>, forward: vec3<f32>) -> vec3<f32> {
    let layers = coverage_at(point, forward);
    let colour = fog_decode(layers.rgb / max(layers.a, 1e-4));
    return colour / max(max(colour.r, max(colour.g, colour.b)), 1e-4);
}

// A distance along the ray by the coverage: a segment by its share of the
// coverage's rise, a uniform place in it.
fn coverage_distance(rises: array<f32, SEGMENTS>, total: f32, ray_length: f32, rng: ptr<function, u32>) -> f32 {
    // (a local copy, to be indexed by a variable)
    var shares = rises;
    var pick = rand_f(rng) * total;
    var segment = SEGMENTS - 1u;
    for (var i = 0u; i < SEGMENTS; i += 1u) {
        if pick < shares[i] { segment = i; break; }
        pick -= shares[i];
    }
    return ray_length * (f32(segment) + rand_f(rng)) / f32(SEGMENTS);
}

// The segment a distance along the ray falls in.
fn segment_at(t: f32, ray_length: f32) -> u32 {
    return min(u32(max(t, 0.0) / ray_length * f32(SEGMENTS)), SEGMENTS - 1u);
}

@compute @workgroup_size(8, 8, 1)
fn lit_fog(@builtin(global_invocation_id) global_id: vec3<u32>) {
    if any(global_id.xy >= vec2u(view.main_pass_viewport.zw)) { return; }
    // (the pixel's view depth in alpha, for the prefilter's and the
    // history's depth weights, whatever light the pixel has)
    let depth = store_depth(view_depth(vec2<i32>(global_id.xy)));
    textureStore(lit_fog_raw, global_id.xy, vec4(scattered_light(global_id.xy), depth));
}

// The light the fog scatters toward the camera along the pixel's ray (the
// picture's units: at the exposure), divided by what the fog over its
// surface lets through unless the application lays it over its fog (lit.w).
fn scattered_light(pixel: vec2<u32>) -> vec3<f32> {
    let lit = sky_light.fog.lit;
    if lit.x <= 0.0 || !ray_fog_active() { return vec3(0.0); }

    let pixel_index = pixel.x + pixel.y * u32(view.main_pass_viewport.z);
    var rng = (pixel_index * 0x9e3779b9u) ^ (constants.frame_index * 0x85ebca6bu) ^ 0x1f0e3dbu;

    let size = view.main_pass_viewport.zw;
    let depth = textureLoad(depth_buffer, pixel, 0);
    let forward = -normalize(view.world_from_view[2].xyz);
    // (a surface past an opaque fog's reach as the sky, its ray the sky's:
    // past_reach)
    var sky = depth == 0.0;
    if !sky && LIT_FOG_REACH > 0u {
        let surface = reconstruct_world_position(pixel, depth, size, view.world_from_clip);
        sky = past_reach(dot(surface - view.world_position, forward));
    }
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

@if(LIT_FOG_COVERAGE_POINT) {
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
    if lit.y > 0.5 && has_sun() {
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
}
@if(!LIT_FOG_COVERAGE_POINT) {
    let ray_length = length(ray);
    if ray_length <= 1e-4 { return vec3(0.0); }
    let direction = ray / ray_length;

    // the local lights: each candidate a tile's sample and a point of its
    // own, drawn by the mixture of the coverage and the equiangular density
    // about the light (both densities taken at the point however it was
    // drawn), with the fog's dc/dt there in the target (rises[segment] *
    // SEGMENTS / ray_length; the coverage's density of a point is that over
    // total)
    let tile_start = rand_range_u(128u, &rng) * 1024u;
    var weight_sum = 0.0;
    var chosen_radiance = vec3(0.0);
    var chosen_position = vec4(0.0);
    var chosen_point = vec3(0.0);
    for (var i = 0u; i < CANDIDATES; i += 1u) {
        let sample = unpack_resolved_light_sample(light_tile_resolved_samples[tile_start + rand_range_u(1024u, &rng)], view.exposure);
        if sample.world_position.w == LIGHT_SAMPLE_DIRECTIONAL { continue; }
        let light_position = sample.world_position.xyz;
        let along = dot(light_position - view.world_position, direction);
        let nearest = max(length(light_position - (view.world_position + direction * along)), EQUIANGULAR_NEAREST);
        let angle_start = atan2(-along, nearest);
        let angle_end = atan2(ray_length - along, nearest);
        let span = angle_end - angle_start;
        // (the equiangular half only where its span is more than nothing: a
        // light far off a short ray is left to the coverage)
        let equiangular_share = select(0.0, 0.5, span > 1e-6);
        var t: f32;
        if rand_f(&rng) < equiangular_share {
            t = along + nearest * tan(mix(angle_start, angle_end, rand_f(&rng)));
        } else {
            t = coverage_distance(rises, total, ray_length, &rng);
        }
        t = clamp(t, 0.0, ray_length);
        let density = rises[segment_at(t, ray_length)] * f32(SEGMENTS) / ray_length;
        let offset = t - along;
        let equiangular_pdf = nearest / (max(span, 1e-6) * (nearest * nearest + offset * offset));
        let pdf = (1.0 - equiangular_share) * density / total + equiangular_share * equiangular_pdf;
        if pdf <= 0.0 { continue; }
        let point = view.world_position + direction * t;
        let contribution = calculate_resolved_light_contribution(sample, point, forward);
        let target_function = luminance(contribution.radiance) * density;
        if target_function <= 0.0 { continue; }
        let weight = target_function * contribution.inverse_pdf / (pdf * f32(CANDIDATES));
        weight_sum += weight;
        if rand_f(&rng) * weight_sum < weight {
            chosen_radiance = contribution.radiance;
            chosen_position = sample.world_position;
            chosen_point = point;
        }
    }
    var scattered = vec3(0.0);
    let chosen_luminance = luminance(chosen_radiance);
    if weight_sum > 0.0 && chosen_luminance > 0.0 {
        let visibility = trace_light_visibility(chosen_point, chosen_position);
        scattered += fog_albedo(chosen_point, forward) * chosen_radiance * (weight_sum / chosen_luminance) * visibility / (4.0 * PI);
    }
    // the sun, at a point of its own by the coverage
    if lit.y > 0.5 && has_sun() {
        let sun_point = view.world_position + direction * coverage_distance(rises, total, ray_length, &rng);
        let light_id = directional_lights[0].light_source_index;
        let sun = resolve_light_sample(LightSample(light_id << 16u, rand_u(&rng)), light_sources[light_id]);
        let sun_contribution = calculate_resolved_light_contribution(sun, sun_point, forward);
        let sun_visibility = trace_light_visibility(sun_point, sun.world_position);
        scattered += total * fog_albedo(sun_point, forward) * sun_contribution.radiance * sun_contribution.inverse_pdf * sun_visibility / (4.0 * PI);
    }
}

    // (laid over by the application's own fog pass, lit.w: not divided, as
    // the sky)
    if !sky && lit.w <= 0.5 {
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

// Whether a surface at this view depth is past an opaque fog's reach (its
// atmospheric maximum density 1): where the fog's atmospheric coverage is
// whole (its density texture's last texel's centre, as ray_fog_layers and the
// application's fog pass sample it), which fogs it wholly, as the sky beside
// it. Such a surface is taken as the sky (SOLARIK_LIT_FOG_REACH, 0 the old):
// its ray ends where the sky's does, and to the filter and the history it and
// the sky are at the reach (reach_depth): the sky's own depth, against which
// a surface weighs nothing, held c10's far trees among the fogged sky apart,
// a few pixels each of their own light, blocky streaks through the lit fog.
fn past_reach(view_depth: f32) -> bool {
    let fog = sky_light.fog;
    if LIT_FOG_REACH == 0u || fog.atmospheric_color.a < 1.0 || fog.atmospheric.y <= 0.0 { return false; }
    let range = max(fog.atmospheric.y - fog.atmospheric.x, 1e-4);
    return (view_depth - fog.atmospheric.x) / range >= 15.5 / 16.0;
}

// The view depth of the opaque fog's reach, where the filter and the history
// take the sky and every surface past the reach to be (the application's fog
// pass fogs the sky there too, as a surface at the fog's maximum distance):
// one region with the surfaces just short of it, weighed against them by
// depth as any surfaces are, in place of the sky's depth, against which a
// surface weighs nothing (a thin far vine just short of the reach among the
// sky was a few pixels of its own still); 0 where the rule is off or the fog
// isn't opaque.
fn reach_depth() -> f32 {
    let fog = sky_light.fog;
    if LIT_FOG_REACH == 0u || fog.atmospheric_color.a < 1.0 || fog.atmospheric.y <= 0.0 { return 0.0; }
    return fog.atmospheric.y;
}

fn view_depth(pixel: vec2<i32>) -> f32 {
    let depth = textureLoad(depth_buffer, pixel, 0);
    let reach = reach_depth();
    if depth == 0.0 {
        // (the sky under an opaque fog: at its reach)
        if reach > 0.0 { return reach; }
        return 1e9;
    }
    let position = reconstruct_world_position(vec2<u32>(pixel), depth, view.main_pass_viewport.zw, view.world_from_clip);
    let d = dot(position - view.world_position, -normalize(view.world_from_view[2].xyz));
    // (past an opaque fog's reach: at it, as the sky)
    if reach > 0.0 { return min(d, reach); }
    return d;
}

// How much a neighbour at view depth d counts for a pixel at view depth
// centre (the sky against the sky in full).
fn depth_weight(d: f32, centre: f32) -> f32 {
    return select(exp(-abs(d - centre) / (DEPTH_TOLERANCE * centre + 0.1)), 1.0, centre >= 1e9 && d >= 1e9);
}

// A view depth (view_depth's) as lit_fog_raw's alpha keeps it.
fn store_depth(depth: f32) -> f32 {
    return select(min(depth, STORED_DEPTH_MAX), STORED_SKY, depth >= 1e9);
}

fn stored_is_sky(stored: f32) -> bool {
    return stored > 0.5 * (STORED_DEPTH_MAX + STORED_SKY);
}

// depth_weight of two stored depths: the sky against the sky in full, the sky
// against a surface not at all (depth_weight's exp(-20) and less), surfaces
// as depth_weight weighs them.
fn stored_depth_weight(d: f32, centre: f32) -> f32 {
    let d_sky = stored_is_sky(d);
    let centre_sky = stored_is_sky(centre);
    let surfaces = exp(-abs(d - centre) / (DEPTH_TOLERANCE * centre + 0.1));
    return select(select(surfaces, 0.0, d_sky != centre_sky), 1.0, d_sky && centre_sky);
}

// A pixel of a prefilter level's source: lit_fog_raw for an even level,
// lit_fog_scratch for an odd one.
fn load_level_source(pixel: vec2<i32>, odd: bool) -> vec4<f32> {
    if odd { return textureLoad(lit_fog_scratch, pixel); }
    return textureLoad(lit_fog_raw, pixel);
}

// A pixel of the prefilter's result: the last level wrote lit_fog_scratch
// when the level count is odd, lit_fog_raw when it is even (a constant
// condition).
fn load_prefiltered(pixel: vec2<i32>) -> vec4<f32> {
    if (PREFILTER_LEVELS & 1u) == 1u { return textureLoad(lit_fog_scratch, pixel); }
    return textureLoad(lit_fog_raw, pixel);
}

// A pixel of the prefilter's level before its last, in the other texture of
// the pair from load_prefiltered's: lit_fog_raw when the level count is odd,
// lit_fog_scratch when it is even (a constant condition; with fewer than two
// levels not a level, and not read).
fn load_before_last(pixel: vec2<i32>) -> vec4<f32> {
    if (PREFILTER_LEVELS & 1u) == 1u { return textureLoad(lit_fog_raw, pixel); }
    return textureLoad(lit_fog_scratch, pixel);
}

// The prefiltered light's noise at a pixel, per channel: the absolute
// difference of the last a-trous level from the one before it, averaged over
// the 3x3 the history is read through (HISTORY_BLUR_STRIDE about the pixel,
// weights (1, 2, 1) each way times the tap's stored depth weight against
// centre's); taps out of the viewport, or not finite, keep a weight of 0.
fn prefilter_noise(pixel: vec2<i32>, size: vec2<i32>, centre: f32) -> vec3<f32> {
    var sum = vec3(0.0);
    var weights = 0.0;
    for (var y = -1; y <= 1; y += 1) {
        for (var x = -1; x <= 1; x += 1) {
            let tap = pixel + vec2(x, y) * HISTORY_BLUR_STRIDE;
            if any(tap < vec2(0)) || any(tap >= size) { continue; }
            let last = load_prefiltered(tap);
            let before = load_before_last(tap);
            if !finite_light(vec4(last.rgb, 0.0)) || !finite_light(vec4(before.rgb, 0.0)) { continue; }
            let weight = stored_depth_weight(last.a, centre) * f32((2 - abs(x)) * (2 - abs(y)));
            sum += abs(last.rgb - before.rgb) * weight;
            weights += weight;
        }
    }
    if weights > 1e-6 { return sum / weights; }
    return vec3(0.0);
}

// The binomial kernel (1, 4, 6, 4, 1) at an offset of the filter (its
// radius 2).
fn binomial(offset: i32) -> f32 {
    return select(select(1.0, 4.0, abs(offset) == 1), 6.0, offset == 0);
}

// One level of the a-trous prefilter: the 5x5 binomial (1, 4, 6, 4, 1 each
// way) at a stride of 2^level pixels, each tap weighted by its stored depth
// against the pixel's, from lit_fog_raw into lit_fog_scratch at an even level
// and back at an odd one (so a level never reads the texture it writes); the
// first level bounds each tap's light to FIREFLY_BOUND times the taps' mean
// by depth first (not under LIT_FOG_UNBOUNDED). The level comes in the fourth
// immediate as its bits (node.rs; stable_radius's place, which this pass
// doesn't read). The stored depth is carried through unchanged.
@compute @workgroup_size(8, 8, 1)
fn lit_fog_filter(@builtin(global_invocation_id) global_id: vec3<u32>) {
    let size = vec2<i32>(view.main_pass_viewport.zw);
    let pixel = vec2<i32>(global_id.xy);
    if any(pixel >= size) { return; }
    let level = bitcast<u32>(constants.stable_radius);
    // (nothing under LIT_FOG_RAW or LIT_FOG_BOX, nor past the levels the
    // shader was built for)
    if level >= PREFILTER_LEVELS { return; }
    let stride = 1i << level;
    let odd = (level & 1u) == 1u;

    let centre = load_level_source(pixel, odd);
    // (each tap loaded once; taps out of the viewport, and lights not
    // finite, keep a weight of 0)
    var lights: array<vec3<f32>, FILTER_TAPS>;
    var depth_weights: array<f32, FILTER_TAPS>;
    var mean_sum = 0.0;
    var mean_weights = 0.0;
    for (var y = -FILTER_RADIUS; y <= FILTER_RADIUS; y += 1) {
        for (var x = -FILTER_RADIUS; x <= FILTER_RADIUS; x += 1) {
            let neighbour = pixel + vec2(x, y) * stride;
            if any(neighbour < vec2(0)) || any(neighbour >= size) { continue; }
            let value = load_level_source(neighbour, odd);
            if !finite_light(vec4(value.rgb, 0.0)) { continue; }
            let tap = u32((y + FILTER_RADIUS) * (2 * FILTER_RADIUS + 1) + x + FILTER_RADIUS);
            let weight = stored_depth_weight(value.a, centre.a);
            lights[tap] = value.rgb;
            depth_weights[tap] = weight;
            mean_sum += luminance(value.rgb) * weight;
            mean_weights += weight;
        }
    }
    let bound = FIREFLY_BOUND * mean_sum / max(mean_weights, 1e-4);
    var sum = vec3(0.0);
    var weights = 0.0;
    for (var y = -FILTER_RADIUS; y <= FILTER_RADIUS; y += 1) {
        for (var x = -FILTER_RADIUS; x <= FILTER_RADIUS; x += 1) {
            let tap = u32((y + FILTER_RADIUS) * (2 * FILTER_RADIUS + 1) + x + FILTER_RADIUS);
            var light = lights[tap];
@if(!LIT_FOG_UNBOUNDED) {
            if level == 0u {
                let brightness = luminance(light);
                if brightness > bound { light *= bound / brightness; }
            }
}
            let weight = depth_weights[tap] * binomial(x) * binomial(y);
            sum += light * weight;
            weights += weight;
        }
    }
    // (a pixel whose taps weigh nothing keeps its own light)
    var filtered = centre.rgb;
    if weights > 0.0 { filtered = sum / weights; }
    if odd {
        textureStore(lit_fog_raw, pixel, vec4(filtered, centre.a));
    } else {
        textureStore(lit_fog_scratch, pixel, vec4(filtered, centre.a));
    }
}

// The frame's scattered light, prefiltered (lit_fog_filter) or filtered here
// by depth over its neighbours, then averaged with the last frames' where the
// pixel was (by the motion vectors), into the parity's history.
@compute @workgroup_size(8, 8, 1)
fn lit_fog_accumulate(@builtin(global_invocation_id) global_id: vec3<u32>) {
    let size = vec2<i32>(view.main_pass_viewport.zw);
    let pixel = vec2<i32>(global_id.xy);
    if any(pixel >= size) { return; }
    if sky_light.fog.lit.x <= 0.0 { return; }

    // (with the prefilter, its result and the depth stored with it, and the
    // history's depth weights by the stored depths; without it, the depths
    // from the depth buffer and the filter here, as before)
    var current: vec3<f32>;
    var centre: f32;
    if PREFILTER_LEVELS > 0u {
        let filtered = load_prefiltered(pixel);
        current = filtered.rgb;
        centre = filtered.a;
    } else {
    centre = view_depth(pixel);
@if(LIT_FOG_RAW) {
    // (a diagnostic, to measure the raw estimate: the frame's own pixel, no
    // filter and no history, so a frame count of 1)
    current = textureLoad(lit_fog_raw, pixel).rgb;
}
@if(!LIT_FOG_RAW && LIT_FOG_BOX) {
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
    current = sum / max(weights, 1e-4);
}
@if(!LIT_FOG_RAW && !(LIT_FOG_BOX)) {
    // A binomial kernel over the neighbours by depth, each tap's light first
    // bounded to FIREFLY_BOUND times their mean by depth: a box held each
    // bright estimate whole in a square of its own (SOLARIK_SHADER_DEFS=
    // LIT_FOG_BOX the old, LIT_FOG_UNBOUNDED the kernel without the bound).
    // (each tap loaded once; taps out of the picture keep a weight of 0)
    var lights: array<vec3<f32>, FILTER_TAPS>;
    var depth_weights: array<f32, FILTER_TAPS>;
    var mean_sum = 0.0;
    var mean_weights = 0.0;
    for (var y = -FILTER_RADIUS; y <= FILTER_RADIUS; y += 1) {
        for (var x = -FILTER_RADIUS; x <= FILTER_RADIUS; x += 1) {
            let neighbour = pixel + vec2(x, y);
            if any(neighbour < vec2(0)) || any(neighbour >= size) { continue; }
            let tap = u32((y + FILTER_RADIUS) * (2 * FILTER_RADIUS + 1) + x + FILTER_RADIUS);
            let weight = depth_weight(view_depth(neighbour), centre);
            let light = textureLoad(lit_fog_raw, neighbour).rgb;
            lights[tap] = light;
            depth_weights[tap] = weight;
            mean_sum += luminance(light) * weight;
            mean_weights += weight;
        }
    }
    let bound = FIREFLY_BOUND * mean_sum / max(mean_weights, 1e-4);
    var sum = vec3(0.0);
    var weights = 0.0;
    for (var y = -FILTER_RADIUS; y <= FILTER_RADIUS; y += 1) {
        for (var x = -FILTER_RADIUS; x <= FILTER_RADIUS; x += 1) {
            let tap = u32((y + FILTER_RADIUS) * (2 * FILTER_RADIUS + 1) + x + FILTER_RADIUS);
            var light = lights[tap];
@if(!LIT_FOG_UNBOUNDED) {
            let brightness = luminance(light);
            if brightness > bound { light *= bound / brightness; }
}
            let weight = depth_weights[tap] * binomial(x) * binomial(y);
            sum += light * weight;
            weights += weight;
        }
    }
    current = sum / max(weights, 1e-4);
}
    }

    let uv = (vec2<f32>(pixel) + 0.5) / vec2<f32>(size);
    let previous_pixel = vec2<i32>(floor((uv - textureLoad(motion_vectors, pixel, 0).xy) * vec2<f32>(size)));
    let parity = constants.render_frame & 1u;
    var history = vec4(0.0);
@if(!LIT_FOG_RAW && LIT_FOG_BOX) {
    if (constants.reset & 1u) == 0u && all(previous_pixel >= vec2(0)) && all(previous_pixel < size) {
        history = select(textureLoad(lit_fog_history_a, previous_pixel), textureLoad(lit_fog_history_b, previous_pixel), parity == 0u);
    }
}
@if(!LIT_FOG_RAW && !(LIT_FOG_BOX)) {
    // The history read through a 3x3 about where the pixel was, at a stride
    // of HISTORY_BLUR_STRIDE, weights (1, 2, 1) each way times the depth
    // weight of the current frame's depth where the tap stands for (the
    // previous frame's depths are not kept: the neighbour of where the pixel
    // was is taken as the neighbour of where it is), the light and the frame
    // count blended alike. Fed back through a small blur, each frame's light
    // spreads a little further each frame (about the stride times the square
    // root of half the frames either way in the steady state), held by the
    // depth weights to surfaces of like depth; a 5x5 box over 24 frames was
    // 600 estimates in a square, and showed as squares (SOLARIK_SHADER_DEFS=
    // LIT_FOG_BOX the old).
    if (constants.reset & 1u) == 0u {
        var history_sum = vec4(0.0);
        var history_weights = 0.0;
        for (var y = -1; y <= 1; y += 1) {
            for (var x = -1; x <= 1; x += 1) {
                let offset = vec2(x, y) * HISTORY_BLUR_STRIDE;
                let tap = previous_pixel + offset;
                let stands_for = pixel + offset;
                if any(tap < vec2(0)) || any(tap >= size) || any(stands_for < vec2(0)) || any(stands_for >= size) { continue; }
                let light = select(textureLoad(lit_fog_history_a, tap), textureLoad(lit_fog_history_b, tap), parity == 0u);
                if !finite_light(light) { continue; }
                var depth_match: f32;
                if PREFILTER_LEVELS > 0u {
                    depth_match = stored_depth_weight(load_prefiltered(stands_for).a, centre);
                } else {
                    depth_match = depth_weight(view_depth(stands_for), centre);
                }
                let weight = depth_match * f32((2 - abs(x)) * (2 - abs(y)));
                history_sum += light * weight;
                history_weights += weight;
            }
        }
        if history_weights > 1e-6 { history = history_sum / history_weights; }
    }
}
    // (nothing that isn't a finite light is kept: one would spread through
    // the filter and stay in the history)
    if !finite_light(history) { history = vec4(0.0); }
    var current_light = current;
    if !finite_light(vec4(current, 0.0)) { current_light = vec3(0.0); }
    // The history clamped per channel to a band of CLAMP_BAND about the
    // prefiltered light (and CLAMP_FLOOR), and its frame count scaled by the
    // share of its luminance kept to the power CLAMP_POWER: a history that
    // fits the band averages on over all its frames, one the clamp had to
    // move is moved and shortened, and follows a light switched on or off in
    // a few frames. (a power of 2 as the product, a constant condition, so
    // that it is the first form's arithmetic exactly)
    // Only with the prefilter: one frame's light over a 5x5 is too noisy a
    // band to clamp to (SOLARIK_LIT_FOG_CLAMP=0 none, the old). The band is
    // widened either way by CLAMP_NOISE times the prefiltered light's own
    // noise about the pixel (prefilter_noise), so that a history inside what
    // one frame's light may be off by is left to average; with the noise off
    // (a constant condition) the arithmetic is the band's of before exactly.
    if PREFILTER_LEVELS > 0u && LIT_FOG_CLAMP > 0u {
        var low = current_light * (1.0 - CLAMP_BAND) - CLAMP_FLOOR;
        var high = current_light * (1.0 + CLAMP_BAND) + CLAMP_FLOOR;
        if CLAMP_NOISE_ON {
            let noise = CLAMP_NOISE * prefilter_noise(pixel, size, centre);
            low -= noise;
            high += noise;
        }
        low = max(low, vec3(0.0));
        let clamped = clamp(history.rgb, low, high);
        let before = luminance(history.rgb);
        let after = luminance(clamped);
        let most = max(before, after);
        let kept = select(min(before, after) / most, 1.0, most <= 1e-8);
        var shortening: f32;
        if CLAMP_POWER == 2.0 {
            shortening = kept * kept;
        } else {
            shortening = pow(kept, CLAMP_POWER);
        }
        history = vec4(clamped, history.a * shortening);
    }
    let frames = min(history.a + 1.0, HISTORY_FRAMES);
    let accumulated = vec4(mix(history.rgb, current_light, 1.0 / frames), frames);
    if parity == 0u {
        textureStore(lit_fog_history_a, global_id.xy, accumulated);
    } else {
        textureStore(lit_fog_history_b, global_id.xy, accumulated);
    }
}
