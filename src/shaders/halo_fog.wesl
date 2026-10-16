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
// covers the level's surfaces only.
//
// The fog screen is its _rasterizer_environment_fog_screen_end: up to four
// layers over the picture, each a density, the dot product of its frame's
// colour and its weights (the game moves them on and shares them), covering
// 1 - the product of (1 - each) in the screen's colour. Added to the
// picture, times the surface's density from its first pass (the near over
// the far density at the near distance, up to 1 at the far, along L; 1 for
// the sky), when its densities differ; else blended over all of it.
//
// Before both, the game's multiplying blends (decals darkening or
// brightening what's under them), which it draws before its fog: the bottom
// half of the overlay's picture holds half their factor, display-encoded
// (0.5: nothing), as for the overlay's multiply node (game_multiply.wesl).
//
// After both, the game's lens flares, added up (src/lens_flares.rs,
// display-encoded light): added, as the game adds them over its picture.

import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput;
import bevy_render::view::View;

struct HaloFog {
    // rgb, maximum density
    atmospheric_color: vec4<f32>,
    // minimum and maximum distance (metres), the camera's density
    atmospheric: vec4<f32>,
    // normal (out of the fog) and d: n.p = d on the plane (metres)
    plane: vec4<f32>,
    // rgb, maximum density
    planar_color: vec4<f32>,
    // maximum distance and depth (metres), the camera's density, 1 with the
    // overlay's multiply
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
@group(0) @binding(10) var overlay: texture_2d<f32>;
@group(0) @binding(11) var flares: texture_2d<f32>;

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

@fragment
fn fragment(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let colour = textureSample(source, source_sampler, in.uv);
    // (the depth is at the render resolution, the picture perhaps upscaled)
    let size = vec2<f32>(textureDimensions(depth));
    let pixel = vec2<i32>(min(in.uv * size, size - 1.0));
    let z = textureLoad(depth, pixel, 0);
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
    let half_factor = srgb_encode(textureSample(overlay, source_sampler, vec2(in.uv.x, 0.5 + in.uv.y * 0.5)).rgb);
    let flare = textureSample(flares, source_sampler, in.uv).rgb;

    var encoded = srgb_encode(saturate(colour.rgb));
    var changed = false;
    if fog.planar.w > 0.5 && any(abs(half_factor - 0.5) > vec3(0.5 / 255.0)) {
        encoded = saturate(encoded * 2.0 * half_factor);
        changed = true;
    }
    var view_depth = 0.0;
    if z > 0.0 {
        let p = view.world_from_clip * vec4(ndc, z, 1.0);
        let position = p.xyz / p.w;
        let forward = -normalize(view.world_from_view[2].xyz);
        view_depth = dot(position - view.world_position, forward);

        let range = max(fog.atmospheric.y - fog.atmospheric.x, 1e-4);
        let atmospheric = fog.atmospheric_color.a * texel((view_depth - fog.atmospheric.x) / range);
        let distance = smoothstep(0.0, 1.0, texel(view_depth / max(fog.planar.x, 1e-4)));
        let under = smoothstep(0.0, 1.0, texel((fog.plane.w - dot(fog.plane.xyz, position)) / max(fog.planar.y, 1e-4)));
        let planar = fog.planar_color.a * mix(distance * under, distance, fog.planar.z);

        let eye = fog.atmospheric.z;
        let fogged = fog.atmospheric_color.rgb * atmospheric * (1.0 - (1.0 - eye) * planar)
            + fog.planar_color.rgb * planar * (1.0 - atmospheric * eye);
        let coverage = 1.0 - (1.0 - atmospheric) * (1.0 - planar);
        if coverage > 0.0 {
            encoded = fogged + encoded * (1.0 - coverage);
            changed = true;
        }
    }

    let mode = fog.screen_color.a;
    if mode > 0.5 && screen_coverage > 0.0 {
        if mode < 1.5 {
            // added, by the surface's density (the sky's: 1)
            var density = 1.0;
            if z > 0.0 && fog.screen.w < 0.5 {
                let range = max(fog.screen.y - fog.screen.x, 1e-4);
                density = mix(fog.screen.z, 1.0, smoothstep(0.0, 1.0, texel((view_depth - fog.screen.x) / range)));
            }
            encoded += fog.screen_color.rgb * screen_coverage * density;
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
