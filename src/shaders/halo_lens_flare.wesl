// The game's lens flares' reflections (src/lens_flares.rs), added up in the
// display's encoding for the fog pass to add to the picture. Each is
// rasterizer_xbox_widgets.c's internal sprite, whose register combiners,
// decoded, are
//   colour = (1 - v) k t^6 + v t, added by the vertex's alpha
// with t the bitmap's colour, v the vertex's (the light's or the
// reflection's tint) and k the reflection's tint factor.

import bevy_render::view::View;

@group(0) @binding(0) var<uniform> view: View;
@group(1) @binding(0) var map: texture_2d<f32>;
@group(1) @binding(1) var map_sampler: sampler;

struct Vertex {
    @location(0) position: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) color: vec4<f32>,
    @location(3) tint: f32,
}

struct Varyings {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) tint: f32,
}

fn srgb_encode(linear: vec3<f32>) -> vec3<f32> {
    let low = linear * 12.92;
    let high = 1.055 * pow(max(linear, vec3(0.0)), vec3(1.0 / 2.4)) - 0.055;
    return select(high, low, linear <= vec3(0.0031308));
}

@vertex
fn vertex(in: Vertex) -> Varyings {
    var out: Varyings;
    // (the picture is upscaled by now: no jitter)
    out.clip = view.unjittered_clip_from_world * vec4(in.position, 1.0);
    out.uv = in.uv;
    out.color = in.color;
    out.tint = in.tint;
    return out;
}

@fragment
fn fragment(in: Varyings) -> @location(0) vec4<f32> {
    let t = srgb_encode(textureSample(map, map_sampler, in.uv).rgb);
    let v = in.color.rgb;
    let t2 = t * t;
    let c = (1.0 - v) * in.tint * t2 * t2 * t2 + v * t;
    return vec4(c * in.color.a, 0.0);
}
