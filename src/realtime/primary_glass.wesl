import bevy_solarik::filtered_canopy_scene::forest_set_seed;
import bevy_solarik::scene::scene_hit::scene_hit_is_miss;
import bevy_solarik::scene::raytracing_scene_bindings::scene_hit_material;
import bevy_solarik::scene::raytracing_scene_bindings::scene_hit_alpha;
import bevy_solarik::scene::raytracing_scene_bindings::primary_ray_cone;
import bevy_solarik::scene::raytracing_scene_bindings::advance_ray_cone;
import bevy_solarik::scene::raytracing_scene_bindings::resolve_ray_hit_filtered;
import bevy_solarik::scene::raytracing_scene_bindings::scene_hit_ripples;
import bevy_solarik::scene::thin_glass::thin_glass_weights;
import bevy_solarik::scene::thin_glass::offset_thin_glass_ray;
import bevy_solarik::realtime::gbuffer_utils::reconstruct_world_position;
import bevy_solarik::realtime::gbuffer_utils::ResolvedGPixel;
import bevy_solarik::scene::raytracing_scene_bindings::emitted_radiance;
import bevy_solarik::scene::raytracing_scene_bindings::trace_glass_ray;
import bevy_solarik::scene::raytracing_scene_bindings::materials;
import bevy_solarik::scene::raytracing_scene_bindings::material_ids;
import bevy_solarik::scene::raytracing_scene_bindings::MATERIAL_FLAG_ALPHA_BLEND;
import bevy_solarik::scene::raytracing_scene_bindings::MATERIAL_FLAG_DIFFUSE_BLEND;
import bevy_solarik::scene::raytracing_scene_bindings::MATERIAL_FLAG_REFRACT;
import bevy_solarik::scene::raytracing_scene_bindings::resolve_material_alpha;
import bevy_solarik::scene::raytracing_scene_bindings::resolve_ray_hit_full;
import bevy_solarik::scene::raytracing_scene_bindings::RAY_T_MIN;
import bevy_solarik::scene::raytracing_scene_bindings::ray_max_distance;
import bevy_solarik::scene::raytracing_scene_bindings::sky_light;
import bevy_solarik::scene::ray_fog::fog_ray_segment;
import bevy_solarik::scene::ray_fog::ray_fog_active;
import bevy_solarik::realtime::specular_gi::trace_glossy_path;
import bevy_solarik::realtime::surface_path::shade_surface_path;
import bevy_solarik::realtime::realtime_bindings::view_output;
import bevy_solarik::realtime::realtime_bindings::depth_buffer;
import bevy_solarik::realtime::realtime_bindings::view;
import bevy_solarik::realtime::realtime_bindings::constants;
import bevy_solarik::realtime::realtime_bindings::primary_background;
@if((DLSS_RR_GUIDE_BUFFERS)) import bevy_solarik::realtime::resolve_dlss_rr_textures::resolve_background_guides;
import bevy_pbr::render::utils::rand_f;

enable wgpu_ray_query;

// One reflected path a pixel, however many panes it crosses: each pane
// offers its reflection (`offer_reflection`), one is kept by weighted
// reservoir sampling, and its path stands for them all, weighted by their
// sum over its own share. Unbiased; panes that reflect alike (a slab's two
// faces, stacked sheets) give almost the same path, so almost no noise. On
// a10's bridge monitor (two sheets across the screen) the pass took 8.2 ms
// with a path for every pane, 6.5 ms with one.
var<private> reflection_sum: f32;
var<private> reflection_luminance: f32;
var<private> reflection_weight: vec3f;
var<private> reflection_surface: ResolvedGPixel;
var<private> reflection_distance: f32;
var<private> reflection_wi: vec3f;

fn offer_reflection(weight: vec3f, surface: ResolvedGPixel, distance: f32, wi: vec3f, rng: ptr<function, u32>) {
    let luminance = dot(weight, vec3(0.2126, 0.7152, 0.0722));
    if luminance <= 0.0 { return; }
    reflection_sum += luminance;
    if rand_f(rng) * reflection_sum < luminance {
        reflection_luminance = luminance;
        reflection_weight = weight;
        reflection_surface = surface;
        reflection_distance = distance;
        reflection_wi = wi;
    }
}




@compute @workgroup_size(8, 8, 1)
fn primary_glass(@builtin(global_invocation_id) id: vec3<u32>) {
    if any(id.xy >= vec2u(view.main_pass_viewport.zw)) { return; }
    let origin = reconstruct_world_position(id.xy, 1.0, view.main_pass_viewport.zw, view.world_from_clip);
    let along_ray = reconstruct_world_position(id.xy, 0.5, view.main_pass_viewport.zw, view.world_from_clip);
    let direction = normalize(along_ray - origin);
    let depth = textureLoad(depth_buffer, id.xy, 0);
    var distance = ray_max_distance();
    // (where the background is a surface, w 1; the sky, 0)
    var background_point = vec4(0.0);
    if depth > 0.0 {
        let background_position = reconstruct_world_position(id.xy, depth, view.main_pass_viewport.zw, view.world_from_clip);
        distance = max(0.0, dot(background_position - origin, direction) - RAY_T_MIN);
        background_point = vec4(background_position, 1.0);
    }
    var rng = id.x + id.y * u32(view.main_pass_viewport.z) + constants.frame_index;
    forest_set_seed(vec2u(id.x+id.y*u32(view.main_pass_viewport.z),constants.frame_index^0x71478d15u));
    let background = textureLoad(view_output, id.xy);
    reflection_sum = 0.0;
    var radiance = composite_primary_glass(id.xy, origin, direction, distance, background.rgb / view.exposure, background_point, &rng);
    if reflection_sum > 0.0 {
        let reflected = trace_glossy_path(id.xy, reflection_surface, reflection_distance, reflection_wi, 1.0, true, &rng);
        radiance = vec4(radiance.rgb + reflection_weight * (reflection_sum / reflection_luminance) * reflected, radiance.a);
    }
    textureStore(view_output, id.xy, vec4(radiance.rgb * view.exposure, background.a));
@if(DLSS_RR_GUIDE_BUFFERS) {
    if radiance.a > 0.0 { resolve_background_guides(id.xy); }
}
}

// How near the background surface (a fraction of its distance) a surface the
// rays meet is taken for it.
const HIDDEN_SURFACE_TOLERANCE = 0.02;

// Front-to-back camera panes split both lobes deterministically. Reflected
// paths retain the existing bounded glossy estimator; transmission reuses
// the already shaded opaque/sky pixel. Inputs and output are scene radiance.
fn composite_primary_glass(pixel_id: vec2u, initial_origin: vec3f, direction: vec3f, max_distance: f32, unbent_background: vec3f, background_point: vec4f, rng: ptr<function, u32>) -> vec4f {
    var origin = initial_origin;
    // (the first pane, where it is within the near rays' reach of the camera
    // (SolarikNearRays: a first-person weapon's), w 1)
    var near_pane = vec4(0.0);
    var background = unbent_background;
    var refracted = false;
    // (the ray's width: the panes' texture filtering with footprints, and
    // always a water pane's ripples' level of detail)
    var cone = primary_ray_cone(view.clip_from_view, view.main_pass_viewport.zw, distance(initial_origin, view.world_position));
    var transmission = vec3(1.0);
    var radiance = vec3(0.0);
    var has_glass = 0.0;
    for (var i = 0u; i <= 32u; i += 1u) {
        let remaining = max_distance - dot(origin - initial_origin, direction);
        if remaining <= RAY_T_MIN { return vec4(radiance + transmission * behind_near_pane(background, near_pane, background_point), has_glass); }
        let ray = trace_glass_ray(origin, direction, RAY_T_MIN, remaining);
        if scene_hit_is_miss(ray) { return vec4(radiance + transmission * behind_near_pane(background, near_pane, background_point), has_glass); }
        let material = scene_hit_material(ray);
        cone = advance_ray_cone(cone, ray.t);
        if (material.flags & (MATERIAL_FLAG_ALPHA_BLEND | MATERIAL_FLAG_DIFFUSE_BLEND)) == 0u {
            // Raster depth bounds this query. An earlier opaque hit was omitted
            // by raster (e.g. a building's back-facing wall), so sky/background
            // cannot stand in for its radiance. Preserve nonglass pixels.
            if has_glass == 0.0 { return vec4(background, 0.0); }
            // (the background's surface itself, met short of its depth, the
            // depth buffer's and the rays' geometry apart: in stripes across
            // all a pane let through, each shaded by one noisy path, hatched
            // c10's tinted structures and cost a third of the pass)
            if background_point.w > 0.0 && distance(origin + direction * ray.t, background_point.xyz)
                <= HIDDEN_SURFACE_TOLERANCE * distance(background_point.xyz, initial_origin) {
                return vec4(radiance + transmission * behind_near_pane(background, near_pane, background_point), has_glass);
            }
@if(RAY_MATERIAL_FOOTPRINTS) {
            let hidden = resolve_ray_hit_filtered(ray, direction, cone);
}
@if(!(RAY_MATERIAL_FOOTPRINTS)) {
            let hidden = resolve_ray_hit_full(ray);
}
            // (one path: the denoiser takes this pixel's light as any other's.
            // A water pane's floor is such a surface, under all of it: c10's
            // swamp took 13.5 ms with four, 5.0 with one)
            let opaque_radiance = shade_surface_path(hidden, -direction, rng);
            return vec4(radiance + transmission * opaque_radiance, has_glass);
        }
        if i == 32u { break; }
        let hit = resolve_ray_hit_filtered(ray, direction, cone);
        let near = sky_light.near_rays;
        if has_glass == 0.0 && near.w > 0.0 && distance(hit.world_position, near.xyz) < near.w {
            near_pane = vec4(hit.world_position, 1.0);
        }
        has_glass = 1.0;
        let alpha = clamp(scene_hit_alpha(ray, hit.uv), 0.0, 1.0);
        if (material.flags & MATERIAL_FLAG_REFRACT) != 0u {
            // A thick pane: its front reflects; what lies behind it is the
            // picture there, bent. (Once: its far side, or another body
            // behind it, would bend a picture that is already bent.)
            if !refracted {
                refracted = true;
                let normal = liquid_normal(hit.world_normal, hit.world_position, direction);
                let weights = thin_glass_weights(-direction, normal, hit.material.base_color, alpha, hit.material.reflectance);
                radiance += transmission * alpha * emitted_radiance(hit.material, -direction);
                if weights.a > 0.0 {
                    var surface = ResolvedGPixel(hit.world_position, normal, hit.material);
                    surface.material.roughness = 0.0;
                    surface.material.metallic = 0.0;
                    offer_reflection(transmission * weights.a, surface, length(hit.world_position - initial_origin), reflect(direction, normal), rng);
                }
                background = refracted_background(hit.world_position, direction, normal, material.alpha_cutoff, hit.material.reflectance, background);
                transmission *= weights.rgb;
                if all(transmission <= vec3(0.0)) { return vec4(radiance, has_glass); }
            }
            origin = offset_thin_glass_ray(hit.world_position, hit.geometric_world_normal, direction, RAY_T_MIN);
            continue;
        }
        if (material.flags & MATERIAL_FLAG_DIFFUSE_BLEND) != 0u {
            var surface_radiance = vec3(0.0);
            if alpha > 0.0 {
                for (var sample = 0u; sample < 4u; sample += 1u) {
                    surface_radiance += shade_surface_path(hit, -direction, rng);
                }
            }
            radiance += transmission * alpha * (surface_radiance / 4.0);
            transmission *= 1.0 - alpha;
            if all(transmission <= vec3(0.0)) { return vec4(radiance, has_glass); }
            origin = offset_thin_glass_ray(hit.world_position, hit.geometric_world_normal, direction, RAY_T_MIN);
            continue;
        }
        // (a water pane reflects about its rippled normal, toward the
        // camera; a ray it would send into the water reflects about the
        // flat surface instead)
        let facing = faceForward(hit.geometric_world_normal, direction, hit.geometric_world_normal);
        var pane_normal = hit.geometric_world_normal;
        if scene_hit_ripples(ray) {
            let rippled = faceForward(hit.world_normal, direction, hit.world_normal);
            if dot(reflect(direction, rippled), facing) > 0.0 {
                pane_normal = rippled;
            }
        }
        let weights = thin_glass_weights(-direction, pane_normal, hit.material.base_color, alpha, hit.material.reflectance);
        radiance += transmission * alpha * emitted_radiance(hit.material, -direction);
        if weights.a > 0.0 {
            let wi = reflect(direction, pane_normal);
            let normal = faceForward(pane_normal, direction, pane_normal);
            var surface = ResolvedGPixel(hit.world_position, normal, hit.material);
            // Smooth dielectric delta: owns reflected emission and never PSR.
            surface.material.roughness = 0.0;
            surface.material.metallic = 0.0;
            offer_reflection(transmission * weights.a, surface, length(hit.world_position - initial_origin), wi, rng);
        }
        transmission *= weights.rgb;
        if all(transmission <= vec3(0.0)) { return vec4(radiance, has_glass); }
        origin = offset_thin_glass_ray(hit.world_position, hit.geometric_world_normal, direction, RAY_T_MIN);
    }
    // A bounded chain cannot assume unvisited panes are transparent.
    return vec4(radiance, has_glass);
}

// What a first-person pane lets through, fogged (SolarikRayFog) from the
// pane to the surface behind it. The application fogs such a pane's pixel at
// the pane's own depth (the needler's needles: the game draws the weapon
// after its fog, so the needles stay clear); the surface behind them would
// otherwise be left unfogged, the needles a clear window through the fog.
// Elsewhere, and in front of the sky, the background is left as it is.
fn behind_near_pane(background: vec3f, near_pane: vec4f, background_point: vec4f) -> vec3f {
    if near_pane.w <= 0.0 || background_point.w <= 0.0 || !ray_fog_active() { return background; }
    let forward = -normalize(view.world_from_view[2].xyz);
    let pane_depth = dot(near_pane.xyz - view.world_position, forward);
    let background_depth = dot(background_point.xyz - view.world_position, forward);
    let plane = sky_light.fog.plane;
    return fog_ray_segment(background, pane_depth, dot(plane.xyz, near_pane.xyz) - plane.w,
        max(background_depth - pane_depth, 0.0), dot(plane.xyz, background_point.xyz) - plane.w, view.exposure);
}

// How far a refracting pane's normal sways (a liquid's surface), and how
// finely and fast it ripples (per metre, per second at 60 frames a second).
const LIQUID_SWAY = 0.12;
const LIQUID_RIPPLES = 7.0;
const LIQUID_FLOW = 2.0;

// A refracting pane's normal, toward the camera, rippling as it flows.
fn liquid_normal(normal: vec3f, position: vec3f, direction: vec3f) -> vec3f {
    // (render_frame counts frames: there is no clock here)
    let t = f32(constants.render_frame % 1000000u) / 60.0 * LIQUID_FLOW;
    let p = position * LIQUID_RIPPLES;
    let ripple = vec3(
        sin(p.y + t + sin(p.z * 1.3 + t * 0.7)),
        sin(p.z + t * 0.8 + sin(p.x * 1.1 - t * 0.6)),
        sin(p.x - t * 0.9 + sin(p.y * 0.9 + t * 0.5)),
    );
    let swayed = normalize(normal + LIQUID_SWAY * ripple);
    return faceForward(swayed, direction, swayed);
}

// The picture behind a pane `depth` thick at `position`, seen along
// `direction` through its `normal`: where the ray bent into it would come
// out, projected (Bevy's screen-space transmission). Parts of the picture
// nearer than the pane don't show through it; with none, the unbent one.
fn refracted_background(position: vec3f, direction: vec3f, normal: vec3f, depth: f32, reflectance: f32, unbent: vec3f) -> vec3f {
    // (its index from its reflectance: f0 = 0.16 reflectance^2)
    let root_f0 = clamp(0.4 * reflectance, 0.0, 0.9);
    let bent = refract(direction, normal, (1.0 - root_f0) / (1.0 + root_f0));
    let pane_clip = view.clip_from_world * vec4(position, 1.0);
    let clip = view.clip_from_world * vec4(position + bent * depth, 1.0);
    if clip.w <= 0.0 || pane_clip.w <= 0.0 { return unbent; }
    let pane_depth = pane_clip.z / pane_clip.w;
    let size = view.main_pass_viewport.zw;
    let pixel = ((clip.xy / clip.w) * vec2(0.5, -0.5) + 0.5) * size - 0.5;
    if any(pixel < vec2(0.0)) || any(pixel > size - 1.0) { return unbent; }
    let corner = vec2u(floor(pixel));
    let f = fract(pixel);
    var sum = vec3(0.0);
    var weight = 0.0;
    for (var tap = 0u; tap < 4u; tap += 1u) {
        let offset = vec2u(tap & 1u, tap >> 1u);
        let at = min(corner + offset, vec2u(size) - 1u);
        // (reversed depth: nearer is greater)
        if textureLoad(depth_buffer, at, 0) > pane_depth { continue; }
        let w = select(1.0 - f.x, f.x, offset.x == 1u) * select(1.0 - f.y, f.y, offset.y == 1u);
        sum += w * textureLoad(primary_background, at, 0).rgb;
        weight += w;
    }
    if weight <= 0.0001 { return unbent; }
    return sum / weight / view.exposure;
}
