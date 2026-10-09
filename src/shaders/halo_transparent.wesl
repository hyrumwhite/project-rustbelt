// Halo's transparent chicago and generic shaders: an NV2A register combiner
// interpreter running the programs halo_map::combiner builds from the tags
// (the Xbox preprocessors'), with the semantics of the port's nv2a_psh.c.
//
// Registers: 0 zero, 1 c0, 2 c1, 3 fog, 4 v0, 5 v1, 8-11 t0-t3, 12 r0,
// 13 r1, 14 v1+r0 (final combiner), 15 E*F (final combiner). An input byte
// is [7:5] mapping, [4] alpha channel, [3:0] register. Values are 8-bit gamma
// on the Xbox; the result is taken to linear for Bevy. Unlit, like Halo's.

import bevy_pbr::render::forward_io::VertexOutput;

struct HaloCombiner {
    rgb_inputs: array<vec4<u32>, 2>,
    rgb_outputs: array<vec4<u32>, 2>,
    alpha_inputs: array<vec4<u32>, 2>,
    alpha_outputs: array<vec4<u32>, 2>,
    constant0: array<vec4<f32>, 8>,
    constant1: array<vec4<f32>, 8>,
    // map n's texture transform: scale (xy), offset (zw)
    map_transforms: array<vec4<f32>, 4>,
    // x: stage count, y: combiner flags, z: final ABCD, w: final EFG
    program: vec4<u32>,
    // x: output mode (0 alpha blend, 1 add, 2 multiply, 3 premultiplied),
    // y: maps present (bit per stage)
    modes: vec4<u32>,
}

@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<uniform> combiner: HaloCombiner;
@group(constants::MATERIAL_BIND_GROUP) @binding(1) var map0: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(2) var map0_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(3) var map1: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(4) var map1_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(5) var map2: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(6) var map2_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(7) var map3: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(8) var map3_sampler: sampler;

fn word(words: array<vec4<u32>, 2>, index: u32) -> u32 {
    return words[index / 4u][index % 4u];
}

fn uv_for(map: u32, uv: vec2<f32>) -> vec2<f32> {
    let t = combiner.map_transforms[map];
    return uv * t.xy + t.zw;
}

// sampled as sRGB (linear); the combiners work on the stored 8-bit values
fn to_gamma(c: vec4<f32>) -> vec4<f32> {
    return vec4<f32>(pow(max(c.rgb, vec3<f32>(0.0)), vec3<f32>(1.0 / 2.2)), c.a);
}

fn read_register(registers: ptr<function, array<vec4<f32>, 16>>, index: u32, stage: u32) -> vec4<f32> {
    let unique_c0 = (combiner.program.y & 0x1000u) != 0u;
    let unique_c1 = (combiner.program.y & 0x10000u) != 0u;
    switch index {
        case 1u: { return combiner.constant0[select(0u, stage, unique_c0)]; }
        case 2u: { return combiner.constant1[select(0u, stage, unique_c1)]; }
        default: { return (*registers)[index]; }
    }
}

fn map_input(value: vec4<f32>, mapping: u32) -> vec4<f32> {
    switch mapping {
        case 0x00u: { return max(value, vec4<f32>(0.0)); }
        case 0x20u: { return 1.0 - saturate(value); }
        case 0x40u: { return 2.0 * max(value, vec4<f32>(0.0)) - 1.0; }
        case 0x60u: { return 1.0 - 2.0 * max(value, vec4<f32>(0.0)); }
        case 0x80u: { return max(value, vec4<f32>(0.0)) - 0.5; }
        case 0xa0u: { return 0.5 - max(value, vec4<f32>(0.0)); }
        case 0xc0u: { return value; }
        default: { return -value; }
    }
}

// an input byte as rgb (xyz) or, for the alpha portion, a scalar in all lanes
fn combiner_input(registers: ptr<function, array<vec4<f32>, 16>>, input: u32, alpha_portion: bool, stage: u32) -> vec4<f32> {
    let source = read_register(registers, input & 0x0fu, stage);
    let alpha_channel = (input & 0x10u) != 0u;
    var value: vec4<f32>;
    if alpha_portion {
        value = vec4<f32>(select(source.b, source.a, alpha_channel));
    } else {
        value = select(vec4<f32>(source.rgb, 0.0), vec4<f32>(source.a), alpha_channel);
    }
    return map_input(value, input & 0xe0u);
}

fn output_mapping(value: vec4<f32>, flags: u32) -> vec4<f32> {
    var v = value;
    switch flags & 0x38u {
        case 0x08u: { v = v - 0.5; }
        case 0x10u: { v = v * 2.0; }
        case 0x18u: { v = (v - 0.5) * 2.0; }
        case 0x20u: { v = v * 4.0; }
        case 0x30u: { v = v * 0.5; }
        default: {}
    }
    return clamp(v, vec4<f32>(-1.0), vec4<f32>(1.0));
}

fn final_input(registers: ptr<function, array<vec4<f32>, 16>>, input: u32, alpha_portion: bool) -> vec4<f32> {
    let index = input & 0x0fu;
    var source = (*registers)[index];
    if index == 1u { source = combiner.constant0[0]; }
    if index == 2u { source = combiner.constant1[0]; }
    let alpha_channel = (input & 0x10u) != 0u;
    var value: vec4<f32>;
    if alpha_portion {
        value = vec4<f32>(select(source.b, source.a, alpha_channel));
    } else {
        value = select(vec4<f32>(source.rgb, 0.0), vec4<f32>(source.a), alpha_channel);
    }
    value = saturate(value);
    return select(value, 1.0 - value, (input & 0x20u) != 0u);
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    var registers: array<vec4<f32>, 16>;
    for (var i = 0u; i < 16u; i++) {
        registers[i] = vec4<f32>(0.0);
    }
    let present = combiner.modes.y;
    if (present & 1u) != 0u { registers[8] = to_gamma(textureSample(map0, map0_sampler, uv_for(0u, in.uv))); }
    if (present & 2u) != 0u { registers[9] = to_gamma(textureSample(map1, map1_sampler, uv_for(1u, in.uv))); }
    if (present & 4u) != 0u { registers[10] = to_gamma(textureSample(map2, map2_sampler, uv_for(2u, in.uv))); }
    if (present & 8u) != 0u { registers[11] = to_gamma(textureSample(map3, map3_sampler, uv_for(3u, in.uv))); }
    // vertex colours: no fade and no lighting tint
    registers[4] = vec4<f32>(1.0);
    registers[5] = vec4<f32>(1.0);
    // r0's alpha starts as t0's
    registers[12] = vec4<f32>(0.0, 0.0, 0.0, registers[8].a);

    let mux_msb = (combiner.program.y & 0x100u) != 0u;
    for (var stage = 0u; stage < min(combiner.program.x, 8u); stage++) {
        var results: array<vec4<f32>, 6>; // rgb AB, CD, SUM; alpha AB, CD, SUM
        for (var portion = 0u; portion < 2u; portion++) {
            let alpha = portion == 1u;
            let inputs = select(word(combiner.rgb_inputs, stage), word(combiner.alpha_inputs, stage), alpha);
            let outputs = select(word(combiner.rgb_outputs, stage), word(combiner.alpha_outputs, stage), alpha);
            let flags = outputs >> 12u;
            let a = combiner_input(&registers, (inputs >> 24u) & 0xffu, alpha, stage);
            let b = combiner_input(&registers, (inputs >> 16u) & 0xffu, alpha, stage);
            let c = combiner_input(&registers, (inputs >> 8u) & 0xffu, alpha, stage);
            let d = combiner_input(&registers, inputs & 0xffu, alpha, stage);
            var ab = a * b;
            var cd = c * d;
            if !alpha && (flags & 0x02u) != 0u { ab = vec4<f32>(dot(a.rgb, b.rgb)); }
            if !alpha && (flags & 0x01u) != 0u { cd = vec4<f32>(dot(c.rgb, d.rgb)); }
            var sum = ab + cd;
            if (flags & 0x04u) != 0u {
                let r0a = registers[12].a;
                let pick_cd = select((u32(r0a * 255.0 + 0.5) & 1u) != 0u, r0a >= 0.5, mux_msb);
                sum = select(ab, cd, pick_cd);
            }
            results[portion * 3u + 0u] = output_mapping(ab, flags);
            results[portion * 3u + 1u] = output_mapping(cd, flags);
            results[portion * 3u + 2u] = output_mapping(sum, flags);
        }
        // write back once both portions have read their inputs
        for (var portion = 0u; portion < 2u; portion++) {
            let alpha = portion == 1u;
            let outputs = select(word(combiner.rgb_outputs, stage), word(combiner.alpha_outputs, stage), alpha);
            let destinations = array<u32, 3>((outputs >> 4u) & 0x0fu, outputs & 0x0fu, (outputs >> 8u) & 0x0fu);
            for (var k = 0u; k < 3u; k++) {
                let destination = destinations[k];
                // only v0, v1, t0-t3, r0, r1 are writable
                if destination != 4u && destination != 5u && !(destination >= 8u && destination <= 13u) {
                    continue;
                }
                let value = results[portion * 3u + k];
                if alpha {
                    registers[destination].a = value.a;
                } else {
                    registers[destination] = vec4<f32>(value.rgb, registers[destination].a);
                }
            }
        }
    }

    // final combiner: rgb = A*B + (1-A)*C + D, alpha = G
    let abcd = combiner.program.z;
    let efg = combiner.program.w;
    let e = final_input(&registers, (efg >> 24u) & 0xffu, false);
    let f = final_input(&registers, (efg >> 16u) & 0xffu, false);
    registers[15] = vec4<f32>(e.rgb * f.rgb, 0.0);
    registers[14] = vec4<f32>(saturate(registers[5].rgb + registers[12].rgb), 0.0);
    let fa = final_input(&registers, (abcd >> 24u) & 0xffu, false);
    let fb = final_input(&registers, (abcd >> 16u) & 0xffu, false);
    let fc = final_input(&registers, (abcd >> 8u) & 0xffu, false);
    let fd = final_input(&registers, abcd & 0xffu, false);
    let rgb = saturate(fa.rgb * fb.rgb + (1.0 - fa.rgb) * fc.rgb + fd.rgb);
    let alpha = final_input(&registers, (efg >> 8u) & 0xffu, true).a;

    let linear = pow(rgb, vec3<f32>(2.2));
    switch combiner.modes.x {
        // additive: Bevy's premultiplied blend state with zero alpha
        case 1u: { return vec4<f32>(linear, 0.0); }
        case 2u: { return vec4<f32>(linear, 1.0); }
        default: { return vec4<f32>(linear, alpha); }
    }
}
