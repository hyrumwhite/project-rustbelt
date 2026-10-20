// A batch of the game's sprites (src/effects.rs: particles, particle
// systems, weather, glows), drawn over the lit, traced picture before its
// fog. The colour is the effect shader's combiners'
// (rasterizer_xbox_transparent_geometry.c), in the display's encoding, with
// t the bitmap's colour, v the vertex's and a its alpha (the fade):
//   c = t v, or with nonlinear tint (1 - v) t^4 + v t
//   alpha blend          c over, by the bitmap's alpha times a
//   multiply             dst * mix(1, c, a)
//   double multiply      dst * 2 mix(0.5, c, a)
//   add, max, subtract   c a
//   alpha multiply add   c a, premultiplied by the bitmap's alpha times a
// Factors become linear ones; colours are written as they are, so they show
// as that colour (Bevy's materials apply the camera's exposure in their own
// shaders; these write past them), unlit, as the game's effects are.
//
// A zsprite (ZSPRITE) writes each pixel's depth from its z-map, as the game's
// zsprite constants and texture stages do: the view depth z less the radius
// R (no nearer than 1 cm past the near plane) times the z-map's height
// (green over blue), so a flash or burst bulges toward the camera instead of
// being cut flat by the surface it meets. The z-map spans each quad.

import bevy_pbr::render::forward_io::VertexOutput;
import bevy_pbr::render::mesh_view_bindings::view;

struct HaloSprite {
    // x: framebuffer blend function, y: what a displayed 1 is written as, z: 1 for
    // nonlinear tint, w: 1 to show it in cyan (debugging)
    mode: vec4<f32>,
    // x: a zsprite's radius (metres)
    zsprite: vec4<f32>,
}

@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<uniform> sprite: HaloSprite;
@group(constants::MATERIAL_BIND_GROUP) @binding(1) var map: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(2) var map_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(3) var zmap: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(4) var zmap_sampler: sampler;

struct ZspriteOutput {
    @location(0) colour: vec4<f32>,
    @builtin(frag_depth) depth: f32,
}

@if(ZSPRITE)
@fragment
fn fragment(in: VertexOutput) -> ZspriteOutput {
    let colour = shade(in);
    var depth = in.position.z;
@if(VERTEX_UVS_B) {
    let texel = textureSample(zmap, zmap_sampler, in.uv_b);
    let height = saturate(texel.g / max(texel.b, 1.0 / 255.0));
    let forward = -normalize(view.world_from_view[2].xyz);
    let z = dot(in.world_position.xyz - view.world_position, forward);
    let near = view.clip_from_view[3][2];
    let lifted = z - min(sprite.zsprite.x, max(z - near - 0.01, 0.0)) * height;
    let clip = view.clip_from_view * vec4(0.0, 0.0, -lifted, 1.0);
    depth = clip.z / clip.w;
}
    return ZspriteOutput(colour, depth);
}

@if(!ZSPRITE)
@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    return shade(in);
}

fn srgb_encode(linear: vec3<f32>) -> vec3<f32> {
    let low = linear * 12.92;
    let high = 1.055 * pow(max(linear, vec3(0.0)), vec3(1.0 / 2.4)) - 0.055;
    return select(high, low, linear <= vec3(0.0031308));
}

fn srgb_decode(encoded: vec3<f32>) -> vec3<f32> {
    let low = encoded / 12.92;
    let high = pow((max(encoded, vec3(0.0)) + 0.055) / 1.055, vec3(2.4));
    return select(high, low, encoded <= vec3(0.04045));
}

fn shade(in: VertexOutput) -> vec4<f32> {
    let sample = textureSample(map, map_sampler, in.uv);
    var v = vec4(1.0);
@if(VERTEX_COLORS) {
    v = in.color;
}
    let t = srgb_encode(sample.rgb);
    var c = t * v.rgb;
    if sprite.mode.z > 0.5 {
        let t2 = t * t;
        c = (1.0 - v.rgb) * t2 * t2 + v.rgb * t;
    }
    let a = v.a;
    let scale = sprite.mode.y;
    let mode = u32(sprite.mode.x);
    if sprite.mode.w > 0.5 {
        if mode == 1u || mode == 2u || mode == 5u {
            return vec4(0.0, 1.0, 1.0, 1.0);
        }
        // (alpha blended: by the coverage the game gives it)
        if mode == 0u {
            return vec4(0.0, 1.0, 1.0, sample.a * a);
        }
        return vec4(0.0, 0.5 * scale, 0.5 * scale, 0.5);
    }
    switch mode {
        // alpha blend
        case 0u: {
            return vec4(srgb_decode(c) * scale, sample.a * a);
        }
        // multiply, min
        case 1u, 5u: {
            let f = mix(vec3(1.0), c, a);
            if mode == 5u {
                return vec4(srgb_decode(f) * scale, 1.0);
            }
            return vec4(srgb_decode(f), 1.0);
        }
        // double multiply
        case 2u: {
            return vec4(srgb_decode(2.0 * mix(vec3(0.5), c, a)), 1.0);
        }
        // alpha multiply add: premultiplied
        case 7u: {
            return vec4(srgb_decode(c * a) * scale, sample.a * a);
        }
        // add, subtract, max
        default: {
            return vec4(srgb_decode(c * a) * scale, 1.0);
        }
    }
}
