// The game's environment fog (src/fog.rs), over the tonemapped picture as the
// game draws it over its world: rasterizer_xbox_environment_fog.c's pass,
// whose register combiners, decoded, are
//   atmospheric = maximum density * A(view depth from its minimum distance
//                 over its range)
//   planar      = maximum density * mix(L(distance) * L(depth), L(distance),
//                 the camera's own planar density)
//   colour      = atmospheric colour * atmospheric * (1 - (1 - eye) * planar)
//               + planar colour * planar * (1 - atmospheric * eye)
//   coverage    = 1 - (1 - atmospheric) * (1 - planar)
// blended ONE, INVSRCALPHA in the display's encoding, where eye is the
// camera's atmospheric density (1 above the fog plane, 0 under it), the
// distance is the view depth over the planar maximum distance, and the depth
// is under the fog plane over its maximum depth. A and L are the game's
// density textures (rasterizer\atmospheric fog density: a ramp;
// rasterizer\planar fog density: a smoothstep), 16 texels sampled with
// linear filtering, clamped. The sky (nothing in the depth buffer) is left
// as it is: the game's fog pass covers the level's surfaces only.

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
    // maximum distance and depth (metres), the camera's density
    planar: vec4<f32>,
}

@group(0) @binding(0) var source: texture_2d<f32>;
@group(0) @binding(1) var source_sampler: sampler;
@group(0) @binding(2) var depth: texture_depth_2d;
@group(0) @binding(3) var<uniform> view: View;
@group(0) @binding(4) var<uniform> fog: HaloFog;

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

@fragment
fn fragment(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let colour = textureSample(source, source_sampler, in.uv);
    // (the depth is at the render resolution, the picture perhaps upscaled)
    let size = vec2<f32>(textureDimensions(depth));
    let pixel = vec2<i32>(min(in.uv * size, size - 1.0));
    let z = textureLoad(depth, pixel, 0);
    if z <= 0.0 {
        return colour;
    }
    let ndc = vec2(in.uv.x * 2.0 - 1.0, 1.0 - in.uv.y * 2.0);
    let p = view.world_from_clip * vec4(ndc, z, 1.0);
    let position = p.xyz / p.w;
    let forward = -normalize(view.world_from_view[2].xyz);
    let view_depth = dot(position - view.world_position, forward);

    let range = max(fog.atmospheric.y - fog.atmospheric.x, 1e-4);
    let atmospheric = fog.atmospheric_color.a * texel((view_depth - fog.atmospheric.x) / range);
    let distance = smoothstep(0.0, 1.0, texel(view_depth / max(fog.planar.x, 1e-4)));
    let under = smoothstep(0.0, 1.0, texel((fog.plane.w - dot(fog.plane.xyz, position)) / max(fog.planar.y, 1e-4)));
    let planar = fog.planar_color.a * mix(distance * under, distance, fog.planar.z);

    let eye = fog.atmospheric.z;
    let fogged = fog.atmospheric_color.rgb * atmospheric * (1.0 - (1.0 - eye) * planar)
        + fog.planar_color.rgb * planar * (1.0 - atmospheric * eye);
    let coverage = 1.0 - (1.0 - atmospheric) * (1.0 - planar);
    if coverage <= 0.0 {
        return colour;
    }
    let encoded = srgb_encode(saturate(colour.rgb));
    return vec4(srgb_decode(fogged + encoded * (1.0 - coverage)), colour.a);
}
