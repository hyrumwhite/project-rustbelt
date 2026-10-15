// A game decal (src/decals.rs), drawn over the lit, traced picture before
// its fog, as the game draws its decals before its fog. The colour each
// framebuffer blend function takes is the Xbox combiners'
// (rasterizer_xbox_decals.c), in the display's encoding, with t the bitmap's
// colour, c the decal's and i its intensity:
//   multiply         f = i (t + (1 - t)(1 - c)) + 1 - i         dst * f
//   double multiply  f = i (t + (0.5 - t)(1 - c)) + (1 - i) 0.5  dst * 2f
//   add, max, ...    e = i t c                                  dst + e, ...
//   alpha blend      t c over, by i times the bitmap's alpha
// Factors become linear ones (what multiplying the encoded colour does to
// the linear); added colours become radiance, at what shows as that colour
// at the camera's exposure. An alpha blend, which would need the light on
// it, multiplies toward its colour instead.

import bevy_pbr::render::forward_io::VertexOutput;

struct HaloDecal {
    // the decal's colour (display-encoded), its intensity
    color: vec4<f32>,
    // x: framebuffer blend function, y: radiance a displayed 1 is, z: 1 to
    // cut away below half the bitmap's alpha (the alpha-tested layer), w: 1
    // to show it in magenta (debugging)
    mode: vec4<f32>,
}

@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<uniform> decal: HaloDecal;
@group(constants::MATERIAL_BIND_GROUP) @binding(1) var map: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(2) var map_sampler: sampler;

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

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let sample = textureSample(map, map_sampler, in.uv);
    if decal.mode.z > 0.5 && sample.a < 0.5 {
        discard;
    }
    let t = srgb_encode(sample.rgb);
    let c = decal.color.rgb;
    let i = decal.color.a;
    let scale = decal.mode.y;
    // (debugging: magenta, multiplied in or added)
    if decal.mode.w > 0.5 {
        let mode = u32(decal.mode.x);
        if mode == 0u || mode == 1u || mode == 2u {
            return vec4(1.0, 0.0, 1.0, 1.0);
        }
        return vec4(0.5 * scale, 0.0, 0.5 * scale, 1.0);
    }
    switch u32(decal.mode.x) {
        // multiply, min
        case 1u, 5u: {
            let f = i * (t + (1.0 - t) * (1.0 - c)) + 1.0 - i;
            if u32(decal.mode.x) == 5u {
                return vec4(srgb_decode(f) * scale, 1.0);
            }
            return vec4(srgb_decode(f), 1.0);
        }
        // double multiply
        case 2u: {
            let f = i * (t + (0.5 - t) * (1.0 - c)) + (1.0 - i) * 0.5;
            return vec4(srgb_decode(2.0 * f), 1.0);
        }
        // alpha blend (multiplying toward its colour)
        case 0u: {
            let f = mix(vec3(1.0), t * c, i * sample.a);
            return vec4(srgb_decode(f), 1.0);
        }
        // alpha multiply add: premultiplied
        case 7u: {
            return vec4(srgb_decode(i * t * c) * scale, i * sample.a);
        }
        // add, reverse subtract, max
        default: {
            return vec4(srgb_decode(i * t * c) * scale, 1.0);
        }
    }
}
