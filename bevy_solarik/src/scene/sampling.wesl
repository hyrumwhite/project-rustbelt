import bevy_solarik::scene::scene_hit::scene_hit_is_miss;
import bevy_solarik::scene::raytracing_scene_bindings::scene_requires_ordered_transport;
@if((PRIMARY_CANOPY_DIRECT_ONLY)) import bevy_solarik::realtime::primary_light_trace::light_trace_set;
import bevy_pbr::render::pbr_lighting::D_GGX;
import bevy_pbr::render::utils::rand_f;
import bevy_pbr::render::utils::rand_vec2f;
import bevy_pbr::render::utils::rand_u;
import bevy_pbr::render::utils::rand_range_u;
import bevy_render::maths::PI;
import bevy_render::maths::PI_2;
import bevy_render::maths::orthonormalize;
import bevy_solarik::scene::raytracing_scene_bindings::sky_light;
import bevy_solarik::scene::raytracing_scene_bindings::directional_light_transmittance;
import bevy_solarik::scene::raytracing_scene_bindings::trace_transmission_ray;
import bevy_solarik::scene::raytracing_scene_bindings::MATERIAL_FLAG_ALPHA_BLEND;
import bevy_solarik::scene::raytracing_scene_bindings::trace_ray;
import bevy_solarik::scene::raytracing_scene_bindings::RAY_T_MIN;
import bevy_solarik::scene::raytracing_scene_bindings::ray_max_distance;
import bevy_solarik::scene::raytracing_scene_bindings::light_sources;
import bevy_solarik::scene::raytracing_scene_bindings::directional_lights;
import bevy_solarik::scene::raytracing_scene_bindings::LIGHT_SOURCE_FLAG_UNSEEN;
import bevy_solarik::scene::raytracing_scene_bindings::local_lights;
import bevy_solarik::scene::raytracing_scene_bindings::LightSource;
import bevy_solarik::scene::raytracing_scene_bindings::LIGHT_SOURCE_KIND_DIRECTIONAL;
import bevy_solarik::scene::raytracing_scene_bindings::light_source_is_emissive_mesh;
import bevy_solarik::scene::raytracing_scene_bindings::resolve_triangle_data_full;
import bevy_solarik::realtime::light_triangles::LightTriangleVertices;
import bevy_solarik::realtime::light_triangles::load_light_triangle;
import bevy_solarik::realtime::light_triangles::light_triangle_at;
import bevy_solarik::scene::raytracing_scene_bindings::materials;
import bevy_solarik::scene::raytracing_scene_bindings::resolve_material_alpha;
import bevy_solarik::scene::raytracing_scene_bindings::MATERIAL_FLAG_DIFFUSE_BLEND;
import bevy_solarik::scene::raytracing_scene_bindings::ResolvedRayHitFull;
import bevy_solarik::scene::raytracing_scene_bindings::MIRROR_ROUGHNESS_THRESHOLD;
import bevy_solarik::scene::collimated::collimated_weight;

enable wgpu_ray_query;




// Whether a float is finite, by its bits (a comparison may be compiled away
// on the assumption that nothing is NaN): the light sampling's guards, so
// that no non-finite value reaches a reservoir or the world cache, where a
// mix keeps it for good.
fn is_finite_f32(x: f32) -> bool {
    return (bitcast<u32>(x) & 0x7F800000u) != 0x7F800000u;
}

fn power_heuristic(f: f32, g: f32) -> f32 {
    return balance_heuristic(f * f, g * g);
}

fn balance_heuristic(f: f32, g: f32) -> f32 {
    // Need to guard against NaNs since ReSTIR reservoirs can have UCW=0
    if f == 0.0 {
        return 0.0;
    }
    return max(0.0, 1.0 / (1.0 + (g / f)));
}

// https://gpuopen.com/download/Bounded_VNDF_Sampling_for_Smith-GGX_Reflections.pdf (Listing 1)
// Result is invalid when output.z <= 0.0, and must be discarded
fn sample_ggx_vndf(wi_tangent: vec3<f32>, roughness: f32, rng: ptr<function, u32>) -> vec3<f32> {
    // Mirror BRDF case
    if roughness <= MIRROR_ROUGHNESS_THRESHOLD {
        return vec3(-wi_tangent.xy, wi_tangent.z);
    }

    let i = wi_tangent;
    let rand = rand_vec2f(rng);
    let i_std = normalize(vec3(i.xy * roughness, i.z));
    let phi = PI_2 * rand.x;
    let a = roughness;
    let s = 1.0 + length(vec2(i.xy));
    let a2 = a * a;
    let s2 = s * s;
    let k = (1.0 - a2) * s2 / (s2 + a2 * i.z * i.z);
    let b = select(i_std.z, k * i_std.z, i.z > 0.0);
    let z = fma(1.0 - rand.y, 1.0 + b, -b);
    let sin_theta = sqrt(saturate(1.0 - z * z));
    let o_std = vec3(sin_theta * cos(phi), sin_theta * sin(phi), z);
    let m_std = i_std + o_std;
    let m = normalize(vec3(m_std.xy * roughness, m_std.z));
    return 2.0 * dot(i, m) * m - i;
}

fn ggx_vndf_sample_invalid(ray_tangent: vec3<f32>) -> bool {
    return !(ray_tangent.z > 0.0);
}

// https://gpuopen.com/download/Bounded_VNDF_Sampling_for_Smith-GGX_Reflections.pdf (Listing 2)
fn ggx_vndf_pdf(wi_tangent: vec3<f32>, wo_tangent: vec3<f32>, roughness: f32) -> f32 {
    // Mirror BRDF case
    if roughness <= MIRROR_ROUGHNESS_THRESHOLD {
        let mirror_wo = vec3(-wi_tangent.xy, wi_tangent.z);
        if all(abs(mirror_wo - wo_tangent) < vec3(0.0001)) {
            return bitcast<f32>(0x7F800000u); // INF
        } else {
            return 0.0;
        }
    }

    let i = wi_tangent;
    let o = wo_tangent;
    let m = normalize(i + o);
    let ndf = D_GGX(roughness, saturate(m.z));
    let ai = roughness * i.xy;
    let len2 = dot(ai, ai);
    let t = sqrt(len2 + i.z * i.z);
    var pdf: f32;
    if i.z >= 0.0 {
        let a = roughness;
        let s = 1.0 + length(i.xy);
        let a2 = a * a;
        let s2 = s * s;
        let k = (1.0 - a2) * s2 / (s2 + a2 * i.z * i.z);
        pdf = ndf / (2.0 * (k * i.z + t));
    } else {
        pdf = ndf * (t - i.z) / (2.0 * len2);
    }

    return select(pdf, 0.0, isnan(pdf));
}

fn isnan(x: f32) -> bool {
    return (bitcast<u32>(x) & 0x7fffffffu) > 0x7f800000u;
}

const NULL_LIGHT_ID = 0xFFFFFFFFu;


struct LightSample {
    light_id: u32,
    seed: u32,
}

// What kind of light a resolved sample came from, in world_position.w.
// (Typed: the shader composer only exports typed constants across modules.)
const LIGHT_SAMPLE_DIRECTIONAL: f32 = 0.0;   // xyz is the direction to the light
const LIGHT_SAMPLE_EMISSIVE_MESH: f32 = 1.0; // xyz is a point on a triangle
const LIGHT_SAMPLE_LOCAL: f32 = 2.0;         // xyz is a point on a point/spot light's sphere

struct ResolvedLightSample {
    world_position: vec4<f32>,
    world_normal: vec3<f32>,
    radiance: vec3<f32>,
    inverse_pdf: f32,
    // Point and spot lights: the cone axis, cos(outer) and cos(inner)
    // (-1 = no cone) and the raster path's range. Receiver-dependent, so
    // they are applied in calculate_resolved_light_contribution, not here.
    // An emissive mesh: its emission cone (axis, and its spot_cos.x above 0
    // where it has one); in spot_cos.y LIGHT_NEAR_DISTANCE x the square root
    // of its triangle's area, negated for a light-only emitter under
    // UNSEEN_LIGHT_NEE; and in range the distance from this point to the
    // triangle's farthest corner (both for the near pre-test,
    // light_may_be_near).
    spot_direction: vec3<f32>,
    spot_cos: vec2<f32>,
    range: f32,
}

struct LightContribution {
    radiance: vec3<f32>,
    inverse_pdf: f32,
    wi: vec3<f32>,
    brdf_rays_can_hit: bool,
    // Solid-angle density for MIS only; inverse_pdf remains in area measure.
    solid_angle_pdf: f32,
}

struct LightContributionNoPdf {
    radiance: vec3<f32>,
    wi: vec3<f32>,
}

struct GenerateRandomLightSampleResult {
    light_sample: LightSample,
    resolved_light_sample: ResolvedLightSample,
}

fn sample_random_light(ray_origin: vec3<f32>, origin_world_normal: vec3<f32>, rng: ptr<function, u32>) -> LightContribution {
    let sample = generate_random_light_sample(rng);
    var light_contribution = calculate_resolved_light_contribution(sample.resolved_light_sample, ray_origin, origin_world_normal);
    light_contribution.radiance *= trace_light_visibility(ray_origin, sample.resolved_light_sample.world_position);
    return light_contribution;
}

// A thin diffuse receiver can see lights on either side. Evaluate geometry
// at the true surface; offset only the visibility ray toward the sampled side.
fn sample_random_light_two_sided(ray_origin: vec3<f32>, origin_world_normal: vec3<f32>, geometric_normal: vec3<f32>, rng: ptr<function, u32>) -> LightContribution {
    let sample = generate_random_light_sample(rng);
    var contribution = calculate_resolved_light_contribution(sample.resolved_light_sample, ray_origin, origin_world_normal);
    let side = select(-1.0, 1.0, dot(geometric_normal, contribution.wi) >= 0.0);
    let origin = ray_origin + geometric_normal * (side * RAY_T_MIN);
    contribution.radiance *= trace_light_visibility(origin, sample.resolved_light_sample.world_position);
    return contribution;
}

// Convert area density to solid angle at the previous scattering vertex.
fn area_to_solid_angle_pdf(area_pdf: f32, distance_squared: f32, light_cosine: f32) -> f32 {
    if area_pdf <= 0.0 || distance_squared <= 0.0 { return 0.0; }
    return area_pdf * distance_squared / max(abs(light_cosine), 0.000001);
}

fn random_emissive_light_solid_angle_pdf(hit: ResolvedRayHitFull, previous_position: vec3<f32>) -> f32 {
    let delta = hit.world_position - previous_position;
    let distance_squared = dot(delta, delta);
    if distance_squared <= 0.0 { return 0.0; }
    let cosine = dot(-delta / sqrt(distance_squared), hit.triangle_world_normal);
    return area_to_solid_angle_pdf(random_emissive_light_pdf(hit), distance_squared, cosine);
}

fn random_emissive_light_pdf(hit: ResolvedRayHitFull) -> f32 {
    let area = f32(hit.triangle_count) * hit.triangle_area;
    if area <= 0.0 { return 0.0; }
    return hit.light_probability / area;
}

fn generate_random_light_sample(rng: ptr<function, u32>) -> GenerateRandomLightSampleResult {
    let light_count = arrayLength(&light_sources);
    let bin_id = rand_range_u(light_count, rng);
    let bin = light_sources[bin_id];
    let light_id = select(bin.alias_index, bin_id, rand_f(rng) < bin.alias_threshold);

    let light_source = light_sources[light_id];

    var triangle_id = 0u;
    if light_source_is_emissive_mesh(light_source) {
        let triangle_count = light_source.kind >> 1u;
        triangle_id = rand_range_u(triangle_count, rng);
    }

    let seed = rand_u(rng);
    let light_sample = LightSample((light_id << 16u) | triangle_id, seed);

    var resolved_light_sample = resolve_light_sample(light_sample, light_source);
    resolved_light_sample.inverse_pdf /= light_source.selection_probability;

    return GenerateRandomLightSampleResult(light_sample, resolved_light_sample);
}

fn resolve_light_sample(light_sample: LightSample, light_source: LightSource) -> ResolvedLightSample {
    if light_source.kind == LIGHT_SOURCE_KIND_DIRECTIONAL {
        let directional_light = directional_lights[light_source.id];

@if(!NO_DIRECTIONAL_LIGHT_SOFT_SHADOWS) {
        // Sample a random direction within a cone whose base is the sun approximated as a disk
        // https://www.realtimerendering.com/raytracinggems/unofficial_RayTracingGems_v1.9.pdf#0004286901.INDD%3ASec30%3A305
        var rng = light_sample.seed;
        let random = rand_vec2f(&rng);
        let cos_theta = (1.0 - random.x) + random.x * directional_light.cos_theta_max;
        let sin_theta = sqrt(1.0 - cos_theta * cos_theta);
@if(PRIMARY_CANOPY_DIRECT_ONLY) {
        light_trace_set(19u,vec4f(random,cos_theta,sin_theta));
}
        let phi = random.y * PI_2;
        let x = cos(phi) * sin_theta;
        let y = sin(phi) * sin_theta;
        var direction_to_light = vec3(x, y, cos_theta);

        // Rotate the ray so that the cone it was sampled from is aligned with the light direction
        direction_to_light = orthonormalize(directional_light.direction_to_light) * direction_to_light;
}
@if(!(!NO_DIRECTIONAL_LIGHT_SOFT_SHADOWS)) {
        let direction_to_light = directional_light.direction_to_light;
}

        return ResolvedLightSample(
            vec4(direction_to_light, LIGHT_SAMPLE_DIRECTIONAL),
            -direction_to_light,
            directional_light.luminance,
            directional_light.inverse_pdf,
            vec3(0.0, 0.0, 1.0),
            vec2(-1.0),
            0.0,
        );
    } else if !light_source_is_emissive_mesh(light_source) {
        // A point or spot light: a point uniformly on its sphere, with the
        // sphere's outward normal, surface radiance and area. The sample is
        // resolved without a receiver (the tiles presample it, and the
        // resampling re-resolves it at other pixels), so the whole sphere is
        // sampled rather than the cap a receiver would see; the far half has
        // cos_theta_light <= 0 and falls out of the resampling at no cost.
        // Expectation at a facing receiver: radiance × π r² / d², the light's
        // intensity over d², which is what the raster path computes.
        let light = local_lights[light_source.id];
        let direction = uniform_sphere_direction(light_sample.seed);

        return ResolvedLightSample(
            vec4(light.position + direction * light.radius, LIGHT_SAMPLE_LOCAL),
            direction,
            light.radiance,
            light.inverse_pdf,
            light.direction,
            vec2(light.cos_outer, light.cos_inner),
            light.range,
        );
    } else {
        // (the light's own resolve, not the full hit's: see resolve_light_triangle)
        let triangle = load_light_triangle(light_source, light_sample.light_id & 0xFFFFu);
        return resolve_emissive_light_sample(light_source, triangle, light_sample.seed);
    }
}

// An emissive mesh's sample on its loaded triangle, at the seed's point.
fn resolve_emissive_light_sample(light_source: LightSource, triangle: LightTriangleVertices, seed: u32) -> ResolvedLightSample {
    let triangle_count = light_source.kind >> 1u;
    // (one material read, by the record's id, which is material_ids[the
    // light's instance]: it served the point and the alpha by two routes)
    let raw_material = materials[triangle.material_id];
    let triangle_data = light_triangle_at(triangle, raw_material, triangle_barycentrics(seed));

    var emission = triangle_data.emissive;
    if (raw_material.flags & (MATERIAL_FLAG_DIFFUSE_BLEND | MATERIAL_FLAG_ALPHA_BLEND)) != 0u {
        emission *= clamp(resolve_material_alpha(raw_material, triangle_data.uv), 0.0, 1.0);
    }
    // (spot_cos.y: the near distance, light_may_be_near's, kept off 0 so
    // that its sign holds the unseen mark of a light-only emitter, with
    // UNSEEN_LIGHT_NEE; see calculate_resolved_light_contribution)
    var near_distance = max(LIGHT_NEAR_DISTANCE * sqrt(triangle_data.area), 1e-4);
@if(UNSEEN_LIGHT_NEE) {
    near_distance = select(near_distance, -near_distance, (light_source.flags & LIGHT_SOURCE_FLAG_UNSEEN) != 0u);
}
    return ResolvedLightSample(
        vec4(triangle_data.world_position, LIGHT_SAMPLE_EMISSIVE_MESH),
        triangle_data.normal,
        emission,
        f32(triangle_count) * triangle_data.area,
        raw_material.emission_cone.xyz,
        vec2(raw_material.emission_cone.w, near_distance),
        farthest_corner_distance(triangle, triangle_data.world_position),
    );
}

// The distance from a point of a triangle to its farthest corner: every
// point of the triangle is within it (its distance from the point being
// convex, it peaks at a corner).
fn farthest_corner_distance(triangle: LightTriangleVertices, point: vec3<f32>) -> f32 {
    let d = vec3(dot(triangle.p0 - point, triangle.p0 - point),
        dot(triangle.p1 - point, triangle.p1 - point), dot(triangle.p2 - point, triangle.p2 - point));
    return sqrt(max(d.x, max(d.y, d.z)));
}

// A receiver near an emissive triangle (on its emitting side, nearer the
// triangle than LIGHT_NEAR_DISTANCE x the square root of its area, the
// triangle filling at least LIGHT_NEAR_MIN_SOLID_ANGLE of its sky) takes the
// triangle's point uniformly in solid angle, not area. By area, radiance
// times cos / d^2 over the point's density is unbounded as the receiver
// nears the emitter: a face beside one of b40's invisible cards (long slivers
// wrapping the hall's ledge, coplanar with or a hair off the faces they
// light) drew a point a finger away now and then, a candidate of a huge
// weight that the reuse spread as a broad soft flash (the user's b40 hall,
// 10 November). By solid angle the weight is the emission times the
// triangle's solid angle: bounded. By area, the geometry term times the area
// is at most area / d^2 at the triangle's nearest distance d, so beyond the
// square root of the area it is at most 1, of the order of a solid angle's
// (at most 2 pi): only nearer is it unbounded. (Near by the centroid and
// the radius, first, put every receiver in the hall near every 8 m sliver,
// and cost 1 ms there.) Near is the receiver's and the triangle's alone,
// never the point's, so a
// light's tile and cell candidates keep one density at a receiver and their
// balance heuristic holds. The warp is Arvo's (pbrt-v4's
// SampleSphericalTriangle), its density 1 / the spherical triangle's area,
// Van Oosterom and Strackee's solid angle (exact, and the warp's own area
// term, which spared Arvo's three angles), and f32 keeps the warp only
// between LIGHT_NEAR_MIN_SOLID_ANGLE and LIGHT_NEAR_MAX_SOLID_ANGLE (pbrt's
// bounds; area outside). The warped point is the seed's again
// (triangle_barycentrics_seed), so ReSTIR DI stores it and re-resolves it
// as any other: reuse stays a reconnection to that world point.
// (SOLARIK_SHADER_DEFS=LIGHT_NEAR_AREA: area sampling everywhere, as before.)
// (the near distance, in hundredths from the environment,
// SOLARIK_LIGHT_NEAR_DISTANCE, 100 by default; 1 where a pipeline passes
// none, the path tracer's)
@if((LIGHT_NEAR_DISTANCE))
const LIGHT_NEAR_DISTANCE: f32 = f32(constants::LIGHT_NEAR_DISTANCE) / 100.0;
@if((!(LIGHT_NEAR_DISTANCE)))
const LIGHT_NEAR_DISTANCE: f32 = 1.0;
// A receiver nearer the triangle's plane than this share of the square root
// of its area is not near: there the warped direction meets the plane at
// the receiver itself, the point can be the receiver to the last bit, and a
// zero distance made a NaN that the world cache's blend kept and its rays
// spread to every cell (b40's shaft went black after a few thousand frames:
// its spotlight cones cross the floors).
const LIGHT_NEAR_MIN_PLANE_DISTANCE: f32 = 1e-3;
const LIGHT_NEAR_MIN_SOLID_ANGLE: f32 = 3e-4;
const LIGHT_NEAR_MAX_SOLID_ANGLE: f32 = 6.22;

// (Timing switches, SOLARIK_SHADER_DEFS=LIGHT_NEAR_TIME_NO_...: each leaves
// a part of the near sampling out so the passes' spans say what it costs;
// the picture is wrong with any of them. NO_TILE: the tile candidates never
// near (light_may_be_near false); NO_CELL: the cell and slot candidates
// never tested; both: the area sampling and the non-finite guards alone.
// NO_WARP: every test runs and its verdict is kept (a near candidate's
// weight a hair off, near set), the warp and the second resolve skipped.
// All three hold in both forms below. LIGHT_NEAR_CACHE_AREA, a diagnostic:
// the world cache's light pick (light_ris.wesl) area-sampled as under
// LIGHT_NEAR_AREA, nothing put off there, ReSTIR DI's near sampling kept, to
// see whether the flashes need the cache's half.)
//
// The candidate loops of restir_di and light_ris run in two passes. The
// first weighs every candidate that cannot be near at once, as an area
// sample, and puts off one that may be (a tile's by light_may_be_near, a
// cell's or slot's by the cheap rejects, light_triangle_may_be_near) on a
// short list; the second runs the whole near path over the list. In one
// loop a wave paid for the near path at every candidate where any one lane
// had a near one, which beside an emitter is nearly every candidate;
// deferred, it pays for as many as the lane with the most (0.09 ms less at
// the hall and the strips, 10 November). The weights and densities are the
// one loop's, only the order the candidates stream into the reservoir
// changes, so the distribution is the same; the random stream differs.
// Under LIGHT_NEAR_AREA nothing is put off and there is no second pass.
// (SOLARIK_SHADER_DEFS=LIGHT_NEAR_INLINE: the one loop, as a cost A/B;
// VISUALIZE_LIGHT_NEAR takes it too.)
@if(LIGHT_NEAR_TIME_NO_WARP)
const LIGHT_NEAR_WARP: bool = false;
@if((!(LIGHT_NEAR_TIME_NO_WARP)))
const LIGHT_NEAR_WARP: bool = true;

// A light candidate after the near test: its sample (the warped seed if
// near), its point resolved as any sample is (its inverse_pdf the area
// form, which the glossy sharing's density and a re-resolve elsewhere see),
// and the point's own inverse density in the same measure and form (its
// light's selection kept), which the resampling weighs it by.
struct NearLightCandidate {
    light_sample: LightSample,
    resolved: ResolvedLightSample,
    inverse_pdf: f32,
    near: bool,
}

// Whether a presampled point's triangle may be near, without reading its
// record: far if the receiver is behind the triangle's plane, or the plane
// (no nearer than the triangle) is beyond the near distance, or the point is
// beyond the near distance and its distance to the triangle's farthest
// corner (every point of it is within that of this one). The normal is
// packed in 16 bits a component, so the plane tests leave a margin of its
// error.
fn light_may_be_near(resolved: ResolvedLightSample, receiver: vec3<f32>) -> bool {
@if((LIGHT_NEAR_AREA || LIGHT_NEAR_TIME_NO_TILE)) {
    return false;
}
@if((!(LIGHT_NEAR_AREA || LIGHT_NEAR_TIME_NO_TILE))) {
    if resolved.world_position.w != LIGHT_SAMPLE_EMISSIVE_MESH { return false; }
    let near_distance = abs(resolved.spot_cos.y);
    let offset = receiver - resolved.world_position.xyz;
    let point_distance = length(offset);
    let plane_distance = dot(resolved.world_normal, offset);
    let margin = 1e-3 * point_distance + 1e-6;
    return plane_distance >= -margin
        && abs(plane_distance) < near_distance + margin
        && point_distance < near_distance + resolved.range;
}
}

// A presampled (tile) candidate, as it is or warped for this receiver: its
// sample (`light_sample`, from light_tile_samples) and its unpacked point.
fn near_light_candidate(light_sample: LightSample, light_source: LightSource, resolved: ResolvedLightSample, receiver: vec3<f32>) -> NearLightCandidate {
    let triangle = load_light_triangle(light_source, light_sample.light_id & 0xFFFFu);
    let warp = near_triangle_warp(triangle, receiver, light_sample.seed);
    if warp.near {
        if !LIGHT_NEAR_WARP {
            return NearLightCandidate(light_sample, resolved, resolved.inverse_pdf * 1.0000002, true);
        }
        let warped = warped_light_candidate(light_sample, light_source, triangle, warp, resolved.inverse_pdf, receiver);
        if warped.near { return warped; }
    }
    return NearLightCandidate(light_sample, resolved, resolved.inverse_pdf, false);
}

// A candidate resolved for this receiver (a cell's or slot's), its triangle
// read once for the test and the point.
fn resolve_near_light_sample(light_sample: LightSample, light_source: LightSource, receiver: vec3<f32>) -> NearLightCandidate {
    if !light_source_is_emissive_mesh(light_source) {
        let resolved = resolve_light_sample(light_sample, light_source);
        return NearLightCandidate(light_sample, resolved, resolved.inverse_pdf, false);
    }
    let triangle = load_light_triangle(light_source, light_sample.light_id & 0xFFFFu);
    // (LIGHT_NEAR_TIME_NO_WARP's verdict, kept)
    var tested_near = false;
@if((!(LIGHT_NEAR_AREA || LIGHT_NEAR_TIME_NO_CELL))) {
    let warp = near_triangle_warp(triangle, receiver, light_sample.seed);
    if warp.near {
        if LIGHT_NEAR_WARP {
            let area_inverse_pdf = f32(light_source.kind >> 1u) * warp.triangle_area;
            let warped = warped_light_candidate(light_sample, light_source, triangle, warp, area_inverse_pdf, receiver);
            if warped.near { return warped; }
        } else {
            tested_near = true;
        }
    }
}
    let resolved = resolve_emissive_light_sample(light_source, triangle, light_sample.seed);
    return NearLightCandidate(light_sample, resolved, resolved.inverse_pdf * select(1.0, 1.0000002, tested_near), tested_near);
}

// The warped candidate: resolved at the stored seed's point (encoded, then
// decoded: what is weighed is what is stored), its inverse density the area
// form's (`area_inverse_pdf`, with whatever selection it carries) times
// solid angle x d^2 / cos over the triangle's area. Not near (the caller
// takes the area sample) where the point is at the receiver or the density
// isn't finite and positive.
fn warped_light_candidate(light_sample: LightSample, light_source: LightSource, triangle: LightTriangleVertices, warp: NearTriangleWarp, area_inverse_pdf: f32, receiver: vec3<f32>) -> NearLightCandidate {
    let warped_sample = LightSample(light_sample.light_id, warp.seed);
    var resolved = resolve_emissive_light_sample(light_source, triangle, warp.seed);
    resolved.inverse_pdf = area_inverse_pdf;
    let to_receiver = receiver - resolved.world_position.xyz;
    let distance_squared = dot(to_receiver, to_receiver);
    let cos_light = abs(dot(resolved.world_normal, to_receiver)) * inverseSqrt(max(distance_squared, 1e-30));
    let inverse_pdf = area_inverse_pdf * warp.solid_angle * distance_squared
        / (max(cos_light, 1e-6) * max(warp.triangle_area, 1e-30));
    let valid = distance_squared > 1e-12 && is_finite_f32(inverse_pdf) && inverse_pdf > 0.0;
    return NearLightCandidate(warped_sample, resolved, inverse_pdf, valid);
}

struct NearTriangleWarp {
    near: bool,
    // The warped point's seed (triangle_barycentrics_seed).
    seed: u32,
    // Its density's inverse: the spherical triangle's area.
    solid_angle: f32,
    // The triangle's own area.
    triangle_area: f32,
}

// The receiver's distance outside a triangle's edges in its plane, the most
// of the three (0 inside): the triangle is the three edges' half-planes'
// meeting, so its distance in the plane is at least this. `face` is
// cross(p0 - p1, p0 - p2); cross(edge, face) points out of the triangle
// across that edge whatever the winding.
fn light_triangle_edge_distance(triangle: LightTriangleVertices, face: vec3<f32>, receiver: vec3<f32>) -> f32 {
    let m01 = cross(triangle.p1 - triangle.p0, face);
    let m12 = cross(triangle.p2 - triangle.p1, face);
    let m20 = cross(triangle.p0 - triangle.p2, face);
    let s01 = dot(receiver - triangle.p0, m01) * inverseSqrt(max(dot(m01, m01), 1e-30));
    let s12 = dot(receiver - triangle.p1, m12) * inverseSqrt(max(dot(m12, m12), 1e-30));
    let s20 = dot(receiver - triangle.p2, m20) * inverseSqrt(max(dot(m20, m20), 1e-30));
    return max(0.0, max(s01, max(s12, s20)));
}

// The near test's cheap rejects alone, as one verdict without early outs
// (the deferred loops' first pass): false where the triangle cannot be
// near (the emitting side, the least plane distance, the plane distance and
// the distance outside its edges beyond the near distance, the centroid's
// sphere, as near_triangle_warp's); true where it may be. A degenerate
// triangle (no face) is false.
fn light_triangle_may_be_near(triangle: LightTriangleVertices, receiver: vec3<f32>) -> bool {
@if((LIGHT_NEAR_AREA || LIGHT_NEAR_TIME_NO_CELL)) {
    return false;
}
@if((!(LIGHT_NEAR_AREA || LIGHT_NEAR_TIME_NO_CELL))) {
    let face = cross(triangle.p0 - triangle.p1, triangle.p0 - triangle.p2);
    let face_dot = dot(face, receiver - triangle.p0);
    let face_dot_squared = face_dot * face_dot;
    let face_squared = dot(face, face);
    let triangle_area = sqrt(face_squared) / 2.0;
    let near_squared = LIGHT_NEAR_DISTANCE * LIGHT_NEAR_DISTANCE * triangle_area;
    let centroid = (triangle.p0 + triangle.p1 + triangle.p2) / 3.0;
    let oc = centroid - receiver;
    let radius_squared = max(dot(triangle.p0 - centroid, triangle.p0 - centroid),
        max(dot(triangle.p1 - centroid, triangle.p1 - centroid), dot(triangle.p2 - centroid, triangle.p2 - centroid)));
    let edge_distance = light_triangle_edge_distance(triangle, face, receiver);
    // (h^2 + e^2 below the near distance's square, h the plane distance
    // face_dot / |face|, e the distance outside the edges, both scaled by
    // |face|^2; it holds the plane test too)
    return (face_dot > 0.0)
        & (face_dot_squared > LIGHT_NEAR_MIN_PLANE_DISTANCE * LIGHT_NEAR_MIN_PLANE_DISTANCE * triangle_area * face_squared)
        & (face_dot_squared + edge_distance * edge_distance * face_squared < near_squared * face_squared)
        & (sqrt(dot(oc, oc)) < sqrt(near_squared) + sqrt(radius_squared));
}
}

// The near test and, if near, the point in solid angle from the receiver,
// the seed's two halves (unfolded) its uniform numbers.
fn near_triangle_warp(triangle: LightTriangleVertices, receiver: vec3<f32>, seed: u32) -> NearTriangleWarp {
    var warp = NearTriangleWarp(false, seed, 0.0, 0.0);
    // (the emitting side, at least LIGHT_NEAR_MIN_PLANE_DISTANCE x the root
    // of the area off the plane: the plane distance h is dot(face, offset) /
    // |face|, compared squared)
    let face = cross(triangle.p0 - triangle.p1, triangle.p0 - triangle.p2);
    let face_dot = dot(face, receiver - triangle.p0);
    if face_dot <= 0.0 { return warp; }
    let face_squared = dot(face, face);
    let triangle_area = sqrt(face_squared) / 2.0;
    if face_dot * face_dot <= LIGHT_NEAR_MIN_PLANE_DISTANCE * LIGHT_NEAR_MIN_PLANE_DISTANCE * triangle_area * face_squared { return warp; }
    // (two exact rejects before the closest point: the plane beyond the near
    // distance, the triangle no nearer than its plane; the receiver beyond
    // the near distance plus the radius from the centroid, every point of
    // the triangle within that radius of it)
    let near_squared = LIGHT_NEAR_DISTANCE * LIGHT_NEAR_DISTANCE * triangle_area;
    if face_dot * face_dot >= near_squared * face_squared { return warp; }
    // (the corners from the receiver, shared by the centroid's sphere, the
    // corner shortcut and the solid angle's directions)
    let o0 = triangle.p0 - receiver;
    let o1 = triangle.p1 - receiver;
    let o2 = triangle.p2 - receiver;
    let corner_squared = vec3(dot(o0, o0), dot(o1, o1), dot(o2, o2));
    let oc = (o0 + o1 + o2) / 3.0;
    let centroid = receiver + oc;
    let radius_squared = max(dot(triangle.p0 - centroid, triangle.p0 - centroid),
        max(dot(triangle.p1 - centroid, triangle.p1 - centroid), dot(triangle.p2 - centroid, triangle.p2 - centroid)));
    if sqrt(dot(oc, oc)) >= sqrt(near_squared) + sqrt(radius_squared) { return warp; }
    // (and the distance outside the edges with the plane distance, as
    // light_triangle_may_be_near: a receiver off the side of a long or big
    // triangle leaves here, before the closest point)
    let edge_distance = light_triangle_edge_distance(triangle, face, receiver);
    if face_dot * face_dot + edge_distance * edge_distance * face_squared >= near_squared * face_squared { return warp; }
    // (a corner within the near distance is a near verdict: the closest
    // point is no farther; only otherwise the closest point itself)
    if min(corner_squared.x, min(corner_squared.y, corner_squared.z)) >= near_squared {
        let to_closest = receiver - closest_point_on_triangle(receiver, triangle.p0, triangle.p1, triangle.p2);
        if dot(to_closest, to_closest) >= near_squared { return warp; }
    }

    let a = o0 * inverseSqrt(corner_squared.x);
    let b = o1 * inverseSqrt(corner_squared.y);
    let c = o2 * inverseSqrt(corner_squared.z);
    let ab_dot = dot(a, b);
    let ca_dot = dot(c, a);
    let solid_angle = abs(2.0 * atan2(dot(a, cross(b, c)), 1.0 + ab_dot + dot(b, c) + ca_dot));
    if !is_finite_f32(solid_angle) || !(solid_angle >= LIGHT_NEAR_MIN_SOLID_ANGLE && solid_angle <= LIGHT_NEAR_MAX_SOLID_ANGLE) { return warp; }
    // (LIGHT_NEAR_TIME_NO_WARP: the verdict alone)
    if !LIGHT_NEAR_WARP {
        warp.near = true;
        warp.solid_angle = solid_angle;
        warp.triangle_area = triangle_area;
        return warp;
    }

    // (pbrt-v4's SampleSphericalTriangle, its spherical area the solid angle
    // above: it needs only the angle at a, from the two planes through a)
    var n_ab = cross(a, b);
    var n_ca = cross(c, a);
    if dot(n_ab, n_ab) == 0.0 || dot(n_ca, n_ca) == 0.0 { return warp; }
    n_ab = normalize(n_ab);
    n_ca = normalize(n_ca);
    let area_pi = solid_angle + PI;

    let u = (vec2(f32(seed & 0xFFFFu), f32(seed >> 16u)) + 0.5) / 65536.0;
    let area_pi_sampled = mix(PI, area_pi, u.x);
    let cos_alpha = clamp(dot(n_ab, -n_ca), -1.0, 1.0);
    let sin_alpha = sqrt(max(0.0, 1.0 - cos_alpha * cos_alpha));
    let sin_sampled = sin(area_pi_sampled);
    let cos_sampled = cos(area_pi_sampled);
    let sin_phi = sin_sampled * cos_alpha - cos_sampled * sin_alpha;
    let cos_phi = cos_sampled * cos_alpha + sin_sampled * sin_alpha;
    let k1 = cos_phi + cos_alpha;
    let k2 = sin_phi - sin_alpha * ab_dot;
    let cos_bp_divisor = (k2 * sin_phi + k1 * cos_phi) * sin_alpha;
    var cos_bp = 1.0;
    if cos_bp_divisor != 0.0 {
        cos_bp = clamp((k2 + (k2 * cos_phi - k1 * sin_phi) * cos_alpha) / cos_bp_divisor, -1.0, 1.0);
    }
    let sin_bp = sqrt(max(0.0, 1.0 - cos_bp * cos_bp));
    let cp = cos_bp * a + sin_bp * safe_normalize(c - ca_dot * a);
    let cos_theta = 1.0 - u.y * (1.0 - dot(cp, b));
    let sin_theta = sqrt(max(0.0, 1.0 - cos_theta * cos_theta));
    let w = cos_theta * b + sin_theta * safe_normalize(cp - dot(cp, b) * b);

    // (the barycentrics where w meets the triangle, clamped)
    let e1 = triangle.p1 - triangle.p0;
    let e2 = triangle.p2 - triangle.p0;
    let s1 = cross(w, e2);
    let divisor = dot(s1, e1);
    var b1 = 1.0 / 3.0;
    var b2 = 1.0 / 3.0;
    if divisor != 0.0 {
        let s = receiver - triangle.p0;
        b1 = clamp(dot(s, s1) / divisor, 0.0, 1.0);
        b2 = clamp(dot(w, cross(s, e1)) / divisor, 0.0, 1.0);
        let sum = b1 + b2;
        if sum > 1.0 {
            b1 /= sum;
            b2 /= sum;
        }
    }
    warp.near = true;
    warp.seed = triangle_barycentrics_seed(vec3(1.0 - b1 - b2, b1, b2));
    warp.solid_angle = solid_angle;
    warp.triangle_area = triangle_area;
    return warp;
}

// The point of a triangle nearest p (Ericson, Real-Time Collision Detection,
// 5.1.5: by the Voronoi regions of its corners, edges and face).
fn closest_point_on_triangle(p: vec3<f32>, a: vec3<f32>, b: vec3<f32>, c: vec3<f32>) -> vec3<f32> {
    let ab = b - a;
    let ac = c - a;
    let ap = p - a;
    let d1 = dot(ab, ap);
    let d2 = dot(ac, ap);
    if d1 <= 0.0 && d2 <= 0.0 { return a; }
    let bp = p - b;
    let d3 = dot(ab, bp);
    let d4 = dot(ac, bp);
    if d3 >= 0.0 && d4 <= d3 { return b; }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 { return a + ab * (d1 / (d1 - d3)); }
    let cp = p - c;
    let d5 = dot(ab, cp);
    let d6 = dot(ac, cp);
    if d6 >= 0.0 && d5 <= d6 { return c; }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 { return a + ac * (d2 / (d2 - d6)); }
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 { return b + (c - b) * ((d4 - d3) / ((d4 - d3) + (d5 - d6))); }
    let denominator = va + vb + vc;
    if denominator == 0.0 { return a; }
    let v = vb / denominator;
    let w = vc / denominator;
    return a + ab * v + ac * w;
}

fn safe_normalize(v: vec3<f32>) -> vec3<f32> {
    let length_squared = dot(v, v);
    return select(vec3(0.0), v * inverseSqrt(length_squared), length_squared > 0.0);
}

fn calculate_resolved_light_contribution(resolved_light_sample: ResolvedLightSample, ray_origin: vec3<f32>, origin_world_normal: vec3<f32>) -> LightContribution {
    let positional = resolved_light_sample.world_position.w != LIGHT_SAMPLE_DIRECTIONAL;
    let ray = resolved_light_sample.world_position.xyz - select(vec3(0.0), ray_origin, positional);
    let light_distance = length(ray);
    let light_distance_squared = light_distance * light_distance;
    // (a point at the receiver lights nothing: 0 / 0 was a NaN)
    let reachable = light_distance_squared > 1e-12 && is_finite_f32(light_distance);
    let wi = select(vec3(0.0, 0.0, 1.0), ray / light_distance, reachable);

    let cos_theta_light = saturate(dot(-wi, resolved_light_sample.world_normal));

    // (SOLARIK_SHADER_DEFS=LIGHT_NEAR_CLAMP, a diagnostic: an emissive mesh's
    // point lights as if at least half a metre away, biased dark near
    // emitters; it says only whether the near field is a boiling's cause)
    var geometry_distance_squared = light_distance_squared;
@if(LIGHT_NEAR_CLAMP) {
    if resolved_light_sample.world_position.w == LIGHT_SAMPLE_EMISSIVE_MESH {
        geometry_distance_squared = max(geometry_distance_squared, 0.25);
    }
}
    var radiance = resolved_light_sample.radiance * (cos_theta_light / geometry_distance_squared);
    if resolved_light_sample.world_position.w == LIGHT_SAMPLE_LOCAL {
        radiance *= local_light_attenuation(resolved_light_sample, wi, light_distance_squared);
    }

    if resolved_light_sample.world_position.w == LIGHT_SAMPLE_EMISSIVE_MESH && resolved_light_sample.spot_cos.x > 0.0 {
        radiance *= collimated_weight(vec4(resolved_light_sample.spot_direction, resolved_light_sample.spot_cos.x), -wi);
    }

    // Only emissive meshes are geometry a BRDF ray can hit; the others are
    // reached by next-event estimation alone, so their MIS weight is 1.
    var brdf_rays_can_hit = resolved_light_sample.world_position.w == LIGHT_SAMPLE_EMISSIVE_MESH;
    // (SOLARIK_SHADER_DEFS=UNSEEN_LIGHT_NEE: a light-only emitter too, which
    // no ray meets, marked in its sample's spot_cos.y by resolve_light_sample;
    // its NEE weight was cut for a BRDF hit that never comes, its light lost
    // round every invisible card)
@if(UNSEEN_LIGHT_NEE) {
    brdf_rays_can_hit = brdf_rays_can_hit && resolved_light_sample.spot_cos.y >= 0.0;
}
    var solid_angle_pdf = 0.0;
    if brdf_rays_can_hit && reachable && resolved_light_sample.inverse_pdf > 0.0 {
        solid_angle_pdf = area_to_solid_angle_pdf(1.0 / resolved_light_sample.inverse_pdf,
            light_distance_squared, dot(-wi, resolved_light_sample.world_normal));
    }
    return LightContribution(select(vec3(0.0), radiance, reachable), resolved_light_sample.inverse_pdf, wi, brdf_rays_can_hit, solid_angle_pdf);
}

// The raster path's cone and range on a point or spot light sample
// (bevy_pbr's pbr_lighting.wgsl: Filament's smooth cone and range window),
// so both paths compute the same irradiance from the same lamp. `wi` points
// from the receiver to the sample.
fn local_light_attenuation(sample: ResolvedLightSample, wi: vec3<f32>, light_distance_squared: f32) -> f32 {
    var attenuation = 1.0;
    let cos_outer = sample.spot_cos.x;
    if cos_outer > -1.0 {
        let cd = dot(sample.spot_direction, -wi);
        let t = saturate((cd - cos_outer) / max(sample.spot_cos.y - cos_outer, 1e-4));
        attenuation = t * t;
    }
    let factor = light_distance_squared / (sample.range * sample.range);
    let window = saturate(1.0 - factor * factor);
    return attenuation * window * window;
}

// Virtual emitters are additive and do not occlude geometry or one another.
// Only an estimator excluded from NEE may collect these hits. Ordinary NEE
// keeps full weight; straight panes retain ownership until a scattering event.
fn analytic_light_radiance(origin: vec3<f32>, wi: vec3<f32>, limit: f32, owned: bool, scatter_position: vec3<f32>) -> vec3<f32> {
    if !owned || limit <= RAY_T_MIN { return vec3(0.0); }
    var radiance = vec3(0.0);
    for (var i = 0u; i < arrayLength(&local_lights); i += 1u) {
        let light = local_lights[i];
        // (one whose source the application draws itself: not seen here)
        if light.unreflected > 0.5 { continue; }
        let offset = origin - light.position;
        let b = dot(offset, wi);
        let c = dot(offset, offset) - light.radius * light.radius;
        let discriminant = b * b - c;
        if discriminant <= 0.0 { continue; }
        // Near side only: the sphere emits outwards, including for inside rays.
        let t = -b - sqrt(discriminant);
        if t <= RAY_T_MIN || t >= limit { continue; }
        let sample = ResolvedLightSample(vec4(origin + t * wi, LIGHT_SAMPLE_LOCAL),
            normalize(origin + t * wi - light.position), light.radiance,
            light.inverse_pdf, light.direction, vec2(light.cos_outer, light.cos_inner), light.range);
        // Straight panes move the traversal origin, not the lighting receiver.
        let light_ray = sample.world_position.xyz - scatter_position;
        let distance_squared = dot(light_ray, light_ray);
        if distance_squared > 0.0 {
            radiance += light.radiance * local_light_attenuation(sample,
                light_ray / sqrt(distance_squared), distance_squared);
        }
    }
    if limit == ray_max_distance() {
        for (var i = 0u; i < arrayLength(&directional_lights); i += 1u) {
            let light = directional_lights[i];
            if light.cos_theta_max < 1.0 && dot(wi, light.direction_to_light) >= light.cos_theta_max {
                radiance += light.luminance;
            }
        }
    }
    return radiance;
}

// A uniformly distributed direction on the unit sphere from a seed.
fn uniform_sphere_direction(seed: u32) -> vec3<f32> {
    var rng = seed;
    let random = rand_vec2f(&rng);
    let z = 1.0 - 2.0 * random.x;
    let r = sqrt(max(1.0 - z * z, 0.0));
    let phi = PI_2 * random.y;
    return vec3(r * cos(phi), r * sin(phi), z);
}

fn resolve_and_calculate_light_contribution(light_sample: LightSample, ray_origin: vec3<f32>, origin_world_normal: vec3<f32>) -> LightContributionNoPdf {
    let resolved_light_sample = resolve_light_sample(light_sample, light_sources[light_sample.light_id >> 16u]);
    let light_contribution = calculate_resolved_light_contribution(resolved_light_sample, ray_origin, origin_world_normal);
    return LightContributionNoPdf(light_contribution.radiance, light_contribution.wi);
}

struct TransmittedLightContribution {
    light: LightContribution,
    continuation_probability: f32,
}

// RGB transport is intentionally separate from scalar ReSTIR visibility.
fn sample_random_light_transmitted(ray_origin: vec3<f32>, world_normal: vec3<f32>, geometric_normal: vec3<f32>, rng: ptr<function, u32>) -> TransmittedLightContribution {
    let sample = generate_random_light_sample(rng).resolved_light_sample;
    var light = calculate_resolved_light_contribution(sample, ray_origin, world_normal);
    let side = select(-1.0, 1.0, dot(geometric_normal, light.wi) >= 0.0);
    let origin = ray_origin + geometric_normal * (side * RAY_T_MIN);
    let transmission = trace_light_transmission(origin, sample.world_position);
@if(PRIMARY_CANOPY_DIRECT_ONLY) {
    light_trace_set(10u,sample.world_position);
    light_trace_set(11u,vec4f(sample.radiance,sample.inverse_pdf));
    light_trace_set(12u,vec4f(light.wi,light.solid_angle_pdf));
    light_trace_set(13u,vec4f(light.radiance,light.inverse_pdf));
    light_trace_set(14u,transmission);
}
    light.radiance *= transmission.rgb;
    return TransmittedLightContribution(light, transmission.a);
}

// A GI reservoir stores outgoing endpoint radiance, never receiver transmission.
// Both diffuse and rough-specular consumers evaluate the current connection.
fn shade_gi_connection(world_position: vec3<f32>, world_normal: vec3<f32>, endpoint: vec3<f32>, radiance: vec3<f32>, weight: f32) -> vec3<f32> {
    if weight <= 0.0 || all(radiance <= vec3(0.0)) { return vec3(0.0); }
    let origin = world_position + world_normal * RAY_T_MIN;
    if trace_point_visibility(origin, endpoint) == 0.0 { return vec3(0.0); }
    let delta = endpoint - origin;
    let distance = length(delta);
    if distance <= RAY_T_MIN { return vec3(0.0); }
    let transmission = trace_shadow_transmission_impl(origin, delta / distance, distance - RAY_T_MIN, false);
    return radiance * weight * transmission.rgb;
}

// Connection energy and competing straight-through BSDF probability.
struct ShadowTransmission {
    energy: vec4<f32>,
    // Furthest geometry position that could have affected this sample.
    distance: f32,
}
fn trace_light_transmission(origin: vec3<f32>, light_position: vec4<f32>) -> vec4<f32> {
    return trace_light_transport(origin, light_position, false).energy;
}
fn trace_light_transmission_with_support(origin: vec3<f32>, light_position: vec4<f32>) -> ShadowTransmission {
    return trace_light_transport(origin, light_position, true);
}
// (`closest`: the distance must be the nearest blocker's, see trace_shadow_transport)
fn trace_light_transport(origin: vec3<f32>, light_position: vec4<f32>, closest: bool) -> ShadowTransmission {
    var direction = light_position.xyz;
    var distance = ray_max_distance();
    if light_position.w != LIGHT_SAMPLE_DIRECTIONAL {
        let delta = light_position.xyz - origin;
        distance = length(delta);
        if distance <= RAY_T_MIN { return ShadowTransmission(vec4(0.0), 0.0); }
        direction = delta / distance;
        return trace_shadow_transport(origin, direction, distance - RAY_T_MIN, true, closest);
    }
    // A directional light comes from beyond the air.
    var shadow = trace_shadow_transport(origin, direction, distance - RAY_T_MIN, true, closest);
    shadow.energy = vec4(shadow.energy.rgb * directional_light_transmittance(origin, direction), shadow.energy.a);
    return shadow;
}

// RGB straight-through energy plus probability of the competing BSDF path.
// Preserve one ray origin and advance t_min, so panes cannot extend past the light.
fn trace_shadow_transmission(origin: vec3<f32>, direction: vec3<f32>, ray_t_max: f32) -> vec4<f32> {
    return trace_shadow_transmission_impl(origin, direction, ray_t_max, true);
}

// GI endpoint/visibility tracing already samples diffuse alpha coverage.
fn trace_shadow_transmission_impl(origin: vec3<f32>, direction: vec3<f32>, ray_t_max: f32, include_coverage: bool) -> vec4<f32> {
    return trace_shadow_transport(origin, direction, ray_t_max, include_coverage, false).energy;
}
fn trace_shadow_transmission_with_support(origin: vec3<f32>, direction: vec3<f32>, ray_t_max: f32, include_coverage: bool) -> ShadowTransmission {
    return trace_shadow_transport(origin, direction, ray_t_max, include_coverage, true);
}
// (`closest`: through panes, the distance is the nearest blocker's, as the
// ordered walk found it, for the world cache's support radius: a traversal
// stopping at its first hit finds any blocker, often a further one, whose
// cells changes further away would reset)
fn trace_shadow_transport(origin: vec3<f32>, direction: vec3<f32>, ray_t_max: f32, include_coverage: bool, closest: bool) -> ShadowTransmission {
    // Only actual admitted TLAS materials decide this shortcut. Mixed panes
    // and diffuse coverage retain the complete ordered transport below.
    if !scene_requires_ordered_transport() {
        // Both false callers have already traced visibility/the GI endpoint.
        if !include_coverage || ray_t_max < RAY_T_MIN { return ShadowTransmission(vec4(1.0), max(ray_t_max, 0.0)); }
        let hit = trace_ray(origin, direction, RAY_T_MIN, ray_t_max, RAY_FLAG_TERMINATE_ON_FIRST_HIT);
        return ShadowTransmission(vec4(select(0.0, 1.0, scene_hit_is_miss(hit))),
            select(hit.t, ray_t_max, scene_hit_is_miss(hit)));
    }
    if ray_t_max < RAY_T_MIN { return ShadowTransmission(vec4(1.0), max(ray_t_max, 0.0)); }
    let ray = trace_transmission_ray(origin, direction, ray_t_max, include_coverage, closest);
    return ShadowTransmission(ray.energy, ray.distance);
}
// End shadow transport

fn trace_light_visibility(ray_origin: vec3<f32>, light_sample_world_position: vec4<f32>) -> f32 {
    var ray_direction = light_sample_world_position.xyz;
    var ray_t_max = ray_max_distance();

    if light_sample_world_position.w != LIGHT_SAMPLE_DIRECTIONAL {
        let ray = ray_direction - ray_origin;
        let dist = length(ray);
        ray_direction = ray / dist;
        ray_t_max = dist - RAY_T_MIN;
    }

    if ray_t_max < RAY_T_MIN { return 0.0; }

    let ray_hit = trace_ray(ray_origin, ray_direction, RAY_T_MIN, ray_t_max, RAY_FLAG_TERMINATE_ON_FIRST_HIT);
    return f32(scene_hit_is_miss(ray_hit));
}

fn trace_point_visibility(ray_origin: vec3<f32>, point: vec3<f32>) -> f32 {
    let ray = point - ray_origin;
    let dist = length(ray);
    let ray_direction = ray / dist;

    let ray_t_max = dist - RAY_T_MIN;
    if ray_t_max < RAY_T_MIN { return 0.0; }

    let ray_hit = trace_ray(ray_origin, ray_direction, RAY_T_MIN, ray_t_max, RAY_FLAG_TERMINATE_ON_FIRST_HIT);
    return f32(scene_hit_is_miss(ray_hit));
}

// https://www.realtimerendering.com/raytracinggems/unofficial_RayTracingGems_v1.9.pdf#0004286901.INDD%3ASec22%3A297
// A triangle's barycentrics from a sample's seed: its two 16-bit halves a
// point on the unit square, folded onto the triangle (uniform by area, as the
// hashed pair was); a point maps back to a seed (triangle_barycentrics_seed),
// so a warped point is stored as any other (near_triangle_warp).
fn triangle_barycentrics(seed: u32) -> vec3<f32> {
    var barycentrics = (vec2(f32(seed & 0xFFFFu), f32(seed >> 16u)) + 0.5) / 65536.0;
    if barycentrics.x + barycentrics.y > 1.0 { barycentrics = 1.0 - barycentrics; }
    return vec3(1.0 - barycentrics.x - barycentrics.y, barycentrics);
}

// The seed whose point is these barycentrics, to 1/65536: unfolded, its
// decoded sum kept at most one (a sum past it would fold the point to the
// hypotenuse's far end).
fn triangle_barycentrics_seed(barycentrics: vec3<f32>) -> u32 {
    var lo = u32(clamp(floor(barycentrics.y * 65536.0), 0.0, 65535.0));
    var hi = u32(clamp(floor(barycentrics.z * 65536.0), 0.0, 65535.0));
    if lo + hi + 1u > 65536u {
        let excess = lo + hi + 1u - 65536u;
        if lo >= hi { lo -= min(excess, lo); } else { hi -= min(excess, hi); }
    }
    return lo | (hi << 16u);
}
