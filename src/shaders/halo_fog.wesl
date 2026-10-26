// The game's environment fog and fog screen (src/fog.rs), over the
// tonemapped picture as the game draws them over its world, in the display's
// encoding.
//
// The fog is rasterizer_xbox_environment_fog.c's pass, whose register
// combiners, decoded, are
//   atmospheric = maximum density * A(view depth from its minimum distance
//                 over its range)
//   planar      = maximum density * mix(L(distance) * L(depth), L(distance),
//                 the camera's own planar density)
//   colour      = atmospheric colour * atmospheric * (1 - (1 - eye) * planar)
//               + planar colour * planar * (1 - atmospheric * eye)
//   coverage    = 1 - (1 - atmospheric) * (1 - planar)
// blended ONE, INVSRCALPHA, where eye is the camera's atmospheric density (1
// above the fog plane, 0 under it), the distance is the view depth over the
// planar maximum distance, and the depth is under the fog plane over its
// maximum depth. A and L are the game's density textures
// (rasterizer\atmospheric fog density: a ramp; rasterizer\planar fog
// density: a smoothstep), 16 texels sampled with linear filtering, clamped.
// The sky (nothing in the depth buffer) is left as it is: the game's fog pass
// covers the level's surfaces only. But where the atmospheric fog is opaque
// (its maximum density 1), the game draws nothing past its maximum distance:
// there the sky is fogged as a surface at that distance.
//
// Where the picture holds the water (atmospheric.w), a surface under the fog
// plane seen from above it was seen through the water's surface (the plane,
// in b30): the game fogs it and draws the water's reflection over it, but
// here the reflection is already in the picture with it. So there the
// planar fog covers only the light that came through, 1 - the water's
// Fresnel reflectance (Schlick's, F0 0.04, as its ray-traced pane's) at the
// view's angle to the plane, and the atmospheric fog is to the surface.
//
// The fog screen is its _rasterizer_environment_fog_screen_end: up to four
// layers over the picture, each a density, the dot product of its frame's
// colour and its weights (the game moves them on and shares them), covering
// 1 - the product of (1 - each) in the screen's colour. Added to the
// picture, times the surface's density from its first pass (the near over
// the far density at the near distance, up to 1 at the far, along L; 1 for
// the sky), when its densities differ; else blended over all of it.
//
// After both, the game's lens flares, added up (src/lens_flares.rs,
// display-encoded light): added, as the game adds them over its picture.

import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput;
import bevy_render::view::View;

struct HaloFog {
    // rgb, maximum density
    atmospheric_color: vec4<f32>,
    // minimum and maximum distance (metres), the camera's density, 1 when the
    // picture holds the water
    atmospheric: vec4<f32>,
    // normal (out of the fog) and d: n.p = d on the plane (metres)
    plane: vec4<f32>,
    // rgb, maximum density
    planar_color: vec4<f32>,
    // maximum distance and depth (metres), the camera's density
    planar: vec4<f32>,
    // the fog screen: rgb, and its mode (0 none, 1 added by depth, 2 blended)
    screen_color: vec4<f32>,
    // near and far distance (metres), near over far density, 1 if its
    // depth's density is flat (no texture)
    screen: vec4<f32>,
    // each layer's texture coordinates from the picture's: u's (x, y, 1)
    // then v's
    screen_transforms: array<vec4<f32>, 8>,
    // each layer's frame's weights, and 1 for a layer
    screen_weights: array<vec4<f32>, 4>,
}

@group(0) @binding(0) var source: texture_2d<f32>;
@group(0) @binding(1) var source_sampler: sampler;
@group(0) @binding(2) var depth: texture_depth_2d;
@group(0) @binding(3) var<uniform> view: View;
@group(0) @binding(4) var<uniform> fog: HaloFog;
@group(0) @binding(5) var layer_0: texture_2d<f32>;
@group(0) @binding(6) var layer_1: texture_2d<f32>;
@group(0) @binding(7) var layer_2: texture_2d<f32>;
@group(0) @binding(8) var layer_3: texture_2d<f32>;
@group(0) @binding(9) var layer_sampler: sampler;
@group(0) @binding(10) var flares: texture_2d<f32>;
// the main pass's depth: nearer than the prepass's where the first-person
// weapon's panes (the needler's needles) are, which the prepass leaves out
@group(0) @binding(11) var scene_depth: texture_depth_2d;

// metres: the level's panes' masks' depth (src/material.rs,
// GLASS_SKY_MASK_DISTANCE), which is none
const GLASS_SKY_MASK_DISTANCE: f32 = 50000.0;

// a 16-texel texture's coordinate as the texel it falls on (0 to 1 over the
// texels' centres, clamped)
fn texel(u: f32) -> f32 {
    return saturate((u * 16.0 - 0.5) / 15.0);
}

fn srgb_encode(linear: vec3<f32>) -> vec3<f32> {
    let low = linear * 12.92;
    let high = 1.055 * pow(linear, vec3(1.0 / 2.4)) - 0.055;
    return select(high, low, linear <= vec3(0.0031308));
}

fn srgb_decode(encoded: vec3<f32>) -> vec3<f32> {
    let low = encoded / 12.92;
    let high = pow((encoded + 0.055) / 1.055, vec3(2.4));
    return select(high, low, encoded <= vec3(0.04045));
}

// a fog screen layer's density at the picture's point
fn layer_density(index: u32, colour: vec4<f32>) -> f32 {
    let weights = fog.screen_weights[index];
    return saturate(dot(colour.rgb, weights.rgb)) * weights.w;
}

// The environment fog at one of the render resolution's pixels: the light
// it adds (rgb) and its coverage (a), and the fog screen's density there
// (1 for the sky).
struct Fogged {
    fogged: vec4<f32>,
    screen_density: f32,
}

fn fog_at(pixel: vec2<i32>) -> Fogged {
    var z = textureLoad(depth, pixel, 0);
    // (a first-person pane in front: fogged at its own depth)
    let pane_z = textureLoad(scene_depth, pixel, 0);
    let mask_z = view.clip_from_view[3][2] / GLASS_SKY_MASK_DISTANCE;
    if pane_z > z * 1.0001 && abs(pane_z - mask_z) > mask_z * 0.01 {
        z = pane_z;
    }
    var result = Fogged(vec4(0.0), 1.0);
    // (the sky, where the atmospheric fog is opaque: the game brings its far
    // plane in to the fog's maximum distance (render.c), and draws no sky
    // past it, only fog; as a surface there. c10's swamps: the treetops in
    // the fog, against the sky, stood out bare)
    let opaque = fog.atmospheric_color.a >= 1.0 && fog.atmospheric.y > 0.0;
    if z <= 0.0 && !opaque {
        return result;
    }
    // (the pixel's centre, where the jittered view sampled it)
    let viewport = view.main_pass_viewport;
    let uv = (vec2<f32>(pixel) + 0.5 - viewport.xy) / viewport.zw;
    let ndc = vec2(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0);
    let forward = -normalize(view.world_from_view[2].xyz);
    var position: vec3<f32>;
    if z <= 0.0 {
        let near = view.world_from_clip * vec4(ndc, 1.0, 1.0);
        let ray = near.xyz / near.w - view.world_position;
        position = view.world_position + ray * (fog.atmospheric.y / max(dot(ray, forward), 1e-6));
    } else {
        let p = view.world_from_clip * vec4(ndc, z, 1.0);
        position = p.xyz / p.w;
    }
    let view_depth = dot(position - view.world_position, forward);

    // (through the water: the camera above the plane, the surface under it)
    let camera_height = dot(fog.plane.xyz, view.world_position) - fog.plane.w;
    let surface_height = dot(fog.plane.xyz, position) - fog.plane.w;
    var transmitted = 1.0;
    var atmospheric_depth = view_depth;
    if fog.atmospheric.w > 0.5 && camera_height > 0.0 && surface_height < 0.0 {
        let ray = normalize(position - view.world_position);
        let cosine = abs(dot(ray, fog.plane.xyz));
        transmitted = 1.0 - (0.04 + 0.96 * pow(1.0 - cosine, 5.0));
        atmospheric_depth = view_depth * camera_height / (camera_height - surface_height);
    }

    let range = max(fog.atmospheric.y - fog.atmospheric.x, 1e-4);
    let atmospheric = fog.atmospheric_color.a * texel((atmospheric_depth - fog.atmospheric.x) / range);
    let distance = smoothstep(0.0, 1.0, texel(view_depth / max(fog.planar.x, 1e-4)));
    let under = smoothstep(0.0, 1.0, texel(-surface_height / max(fog.planar.y, 1e-4)));
    let planar = fog.planar_color.a * mix(distance * under, distance, fog.planar.z) * transmitted;

    let eye = fog.atmospheric.z;
    let fogged = fog.atmospheric_color.rgb * atmospheric * (1.0 - (1.0 - eye) * planar)
        + fog.planar_color.rgb * planar * (1.0 - atmospheric * eye);
    let coverage = 1.0 - (1.0 - atmospheric) * (1.0 - planar);
    result.fogged = vec4(fogged, coverage);

    if fog.screen.w < 0.5 {
        let range = max(fog.screen.y - fog.screen.x, 1e-4);
        result.screen_density = mix(fog.screen.z, 1.0, smoothstep(0.0, 1.0, texel((view_depth - fog.screen.x) / range)));
    }
    return result;
}

@fragment
fn fragment(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let colour = textureSample(source, source_sampler, in.uv);
    let ndc = vec2(in.uv.x * 2.0 - 1.0, 1.0 - in.uv.y * 2.0);

    // the fog screen's layers (sampled before anything returns, for their
    // derivatives)
    let s = vec3(ndc, 1.0);
    let t = fog.screen_transforms;
    let d0 = layer_density(0u, textureSample(layer_0, layer_sampler, vec2(dot(t[0].xyz, s), dot(t[1].xyz, s))));
    let d1 = layer_density(1u, textureSample(layer_1, layer_sampler, vec2(dot(t[2].xyz, s), dot(t[3].xyz, s))));
    let d2 = layer_density(2u, textureSample(layer_2, layer_sampler, vec2(dot(t[4].xyz, s), dot(t[5].xyz, s))));
    let d3 = layer_density(3u, textureSample(layer_3, layer_sampler, vec2(dot(t[6].xyz, s), dot(t[7].xyz, s))));
    let screen_coverage = 1.0 - (1.0 - d0) * (1.0 - d1) * (1.0 - d2) * (1.0 - d3);

    // (sampled before anything returns, as the layers)
    let flare = textureSample(flares, source_sampler, in.uv).rgb;

    // The depth is at the render resolution, jittered each frame, while the
    // picture is upscaled and steady: the fog of the four render pixels round
    // where the jittered view saw this point, filtered. Below DLAA the depth
    // texture is the picture's size, the view drawn in its corner (the main
    // pass's viewport): read over the whole texture, the fog was from depths
    // elsewhere in the picture (b30's far sea a white band, streaked, and
    // outlines round models against it); from the nearest pixel alone, a
    // silhouette's edge was fogged as the surface beside it now and then.
    let anchor = view.world_position - normalize(view.world_from_view[2].xyz);
    let jittered = view.clip_from_world * vec4(anchor, 1.0);
    let steady = view.unjittered_clip_from_world * vec4(anchor, 1.0);
    let jitter = jittered.xy / jittered.w - steady.xy / steady.w;
    let viewport = view.main_pass_viewport;
    let at = viewport.xy + (in.uv + vec2(jitter.x, -jitter.y) * 0.5) * viewport.zw - 0.5;
    let base = floor(at);
    let weight = at - base;
    let low = vec2<i32>(viewport.xy);
    let high = vec2<i32>(viewport.xy + viewport.zw) - 1;
    let p00 = clamp(vec2<i32>(base), low, high);
    let p11 = clamp(vec2<i32>(base) + 1, low, high);
    let f00 = fog_at(p00);
    let f10 = fog_at(vec2(p11.x, p00.y));
    let f01 = fog_at(vec2(p00.x, p11.y));
    let f11 = fog_at(p11);
    let w00 = (1.0 - weight.x) * (1.0 - weight.y);
    let w10 = weight.x * (1.0 - weight.y);
    let w01 = (1.0 - weight.x) * weight.y;
    let w11 = weight.x * weight.y;
    let fogged = f00.fogged * w00 + f10.fogged * w10 + f01.fogged * w01 + f11.fogged * w11;
    let screen_density = f00.screen_density * w00 + f10.screen_density * w10 + f01.screen_density * w01 + f11.screen_density * w11;

    var encoded = srgb_encode(saturate(colour.rgb));
    var changed = false;
    if fogged.a > 0.0 {
        encoded = fogged.rgb + encoded * (1.0 - fogged.a);
        changed = true;
    }

    let mode = fog.screen_color.a;
    if mode > 0.5 && screen_coverage > 0.0 {
        if mode < 1.5 {
            // added, by the surface's density (the sky's: 1)
            encoded += fog.screen_color.rgb * screen_coverage * screen_density;
        } else {
            encoded = mix(encoded, fog.screen_color.rgb, screen_coverage);
        }
        changed = true;
    }
    // (added as the game's framebuffer adds them: up to 1)
    if any(flare > vec3(0.0)) {
        encoded = max(encoded, min(encoded + flare, vec3(1.0)));
        changed = true;
    }
    if !changed {
        return colour;
    }
    return vec4(srgb_decode(saturate(encoded)), colour.a);
}
