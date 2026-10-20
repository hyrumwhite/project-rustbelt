// Halo's transparent chicago and generic shaders: an NV2A register combiner
// interpreter running the programs halo_map::combiner builds from the tags
// (the Xbox preprocessors'), with the semantics of the port's nv2a_psh.c.
//
// Registers: 0 zero, 1 c0, 2 c1, 3 fog, 4 v0, 5 v1, 8-11 t0-t3, 12 r0,
// 13 r1, 14 v1+r0 (final combiner), 15 E*F (final combiner). An input byte
// is [7:5] mapping, [4] alpha channel, [3:0] register. Values are 8-bit gamma
// on the Xbox; the result is taken to linear for Bevy. Unlit, like Halo's.

import bevy_pbr::render::forward_io::VertexOutput;
import bevy_pbr::render::mesh_view_bindings::view;
@if(DEPTH_PREPASS) import bevy_pbr::prepass::utils::prepass_depth;

struct HaloCombiner {
    rgb_inputs: array<vec4<u32>, 2>,
    rgb_outputs: array<vec4<u32>, 2>,
    alpha_inputs: array<vec4<u32>, 2>,
    alpha_outputs: array<vec4<u32>, 2>,
    constant0: array<vec4<f32>, 8>,
    constant1: array<vec4<f32>, 8>,
    // map n's texture transform: its u row (2n) and v row (2n + 1), applied
    // to (u, v, 0, 1)
    map_rows: array<vec4<f32>, 8>,
    // x: stage count, y: combiner flags, z: final ABCD, w: final EFG
    program: vec4<u32>,
    // x: framebuffer blend function (0 alpha blend, 1 multiply, 2 double
    // multiply, 3 add, 4 subtract, 5 min, 6 max, 7 alpha multiply add),
    // y: maps present (bit per stage), z: the transparent flags (bit 0 alpha
    // tested, 3 first map in screen space, 6 scale first map with
    // distance), w: fade mode (0 none, 1 when perpendicular, 2 when parallel)
    modes: vec4<u32>,
    // x: the fade's scale, y: 1 to show it in magenta (debugging)
    fade: vec4<f32>,
    // the environment fog (halo_fog.wesl's first five)
    atmospheric_color: vec4<f32>,
    atmospheric: vec4<f32>,
    plane: vec4<f32>,
    planar_color: vec4<f32>,
    planar: vec4<f32>,
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
@group(constants::MATERIAL_BIND_GROUP) @binding(9) var noise0: texture_3d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(10) var noise0_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(11) var noise1: texture_3d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(12) var noise1_sampler: sampler;

fn word(words: array<vec4<u32>, 2>, index: u32) -> u32 {
    return words[index / 4u][index % 4u];
}

fn uv_for(map: u32, uv: vec2<f32>) -> vec2<f32> {
    let p = vec4(uv, 0.0, 1.0);
    return vec2(dot(combiner.map_rows[map * 2u], p), dot(combiner.map_rows[map * 2u + 1u], p));
}

// metres in a world unit (halo_map::math::METERS_PER_WORLD_UNIT)
const METERS_PER_WORLD_UNIT: f32 = 3.048;

// the first map's own texture coordinates: the screen's where it's in screen
// space, and scaled by the distance (in world units) where it says
fn first_map_uv(in: VertexOutput) -> vec2<f32> {
    var uv = in.uv;
    if (combiner.modes.z & 8u) != 0u {
        uv = in.position.xy / view.viewport.zw;
    }
    if (combiner.modes.z & 64u) != 0u {
        let forward = -normalize(view.world_from_view[2].xyz);
        uv *= max(dot(in.world_position.xyz - view.world_position, forward), 0.0) / METERS_PER_WORLD_UNIT;
    }
    return uv;
}

// a 16-texel density texture's coordinate as the texel it falls on
fn fog_texel(u: f32) -> f32 {
    return saturate((u * 16.0 - 0.5) / 15.0);
}

// the environment fog's coverage at a view depth and a point (halo_fog.wesl,
// without the water's case)
fn fog_coverage(view_depth: f32, position: vec3<f32>) -> f32 {
    let height = dot(combiner.plane.xyz, position) - combiner.plane.w;
    let range = max(combiner.atmospheric.y - combiner.atmospheric.x, 1e-4);
    let atmospheric = combiner.atmospheric_color.a * fog_texel((view_depth - combiner.atmospheric.x) / range);
    let distance = smoothstep(0.0, 1.0, fog_texel(view_depth / max(combiner.planar.x, 1e-4)));
    let under = smoothstep(0.0, 1.0, fog_texel(-height / max(combiner.planar.y, 1e-4)));
    // (how far the camera is under the plane, from the view: the material's
    // fog leaves it out)
    let eye = saturate(-(dot(combiner.plane.xyz, view.world_position) - combiner.plane.w) / max(combiner.planar.y, 1e-4));
    let planar = combiner.planar_color.a * mix(distance * under, distance, eye);
    return 1.0 - (1.0 - atmospheric) * (1.0 - planar);
}

// The surface's own fog's transmittance, and how much to raise its colour
// by for the fog pass over what lies behind it (as the sprites',
// halo_sprite.wesl): the game draws its transparent surfaces after its fog,
// each faded by the fog at its own depth (the combiners' last stage).
fn surface_fog(in: VertexOutput) -> vec2<f32> {
    if combiner.atmospheric_color.a <= 0.0 && combiner.planar_color.a <= 0.0 {
        return vec2(1.0);
    }
    let forward = -normalize(view.world_from_view[2].xyz);
    let own = 1.0 - fog_coverage(dot(in.world_position.xyz - view.world_position, forward), in.world_position.xyz);
    var behind = 0.0;
@if(DEPTH_PREPASS) {
    let depth = prepass_depth(in.position, 0u);
    if depth > 0.0 {
        let uv = in.position.xy / view.viewport.zw;
        let ndc = vec2(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0);
        let p = view.world_from_clip * vec4(ndc, depth, 1.0);
        let position = p.xyz / p.w;
        behind = fog_coverage(dot(position - view.world_position, forward), position);
    }
}
    return vec2(own, min(1.0 / max(1.0 - behind, 1e-4), 4.0));
}

// the game's fade (its combiners' last stage): by the view's angle to the
// surface, perpendicular (seen head on) or parallel (edge on), times its scale
fn surface_fade(in: VertexOutput) -> f32 {
    let facing = abs(dot(normalize(in.world_normal), normalize(view.world_position - in.world_position.xyz)));
    switch combiner.modes.w {
        case 1u: { return (1.0 - facing) * combiner.fade.x; }
        case 2u: { return facing * combiner.fade.x; }
        default: { return combiner.fade.x; }
    }
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
    if (present & 1u) != 0u { registers[8] = to_gamma(textureSample(map0, map0_sampler, uv_for(0u, first_map_uv(in)))); }
    if (present & 2u) != 0u { registers[9] = to_gamma(textureSample(map1, map1_sampler, uv_for(1u, in.uv))); }
    if (present & 4u) != 0u { registers[10] = to_gamma(textureSample(map2, map2_sampler, uv_for(2u, in.uv))); }
    if (present & 8u) != 0u { registers[11] = to_gamma(textureSample(map3, map3_sampler, uv_for(3u, in.uv))); }
    // vertex colours: no fade and no lighting tint
    registers[4] = vec4<f32>(1.0);
    registers[5] = vec4<f32>(1.0);
    // plasma (modes.z bit 9): its noise maps at the surface's own place (the
    // model's rest position, in the game's units and axes, carried in the
    // vertex colour) times their scale, moved by their direction with time;
    // v1 its colour (and alpha) from parallel to perpendicular by the view's
    // angle; v0 white (the draw's colour constant 0)
    if (combiner.modes.z & 512u) != 0u {
        var position = vec3(0.0);
@if(VERTEX_COLORS) {
        position = in.color.xyz;
}
        let n0 = combiner.map_rows[0];
        let n1 = combiner.map_rows[1];
        registers[8] = textureSample(noise0, noise0_sampler, position * n0.x + n0.yzw);
        registers[9] = textureSample(noise1, noise1_sampler, position * n1.x + n1.yzw);
        let facing = abs(dot(normalize(in.world_normal), normalize(view.world_position - in.world_position.xyz)));
        registers[5] = saturate(combiner.constant0[0] + combiner.constant1[0] * facing);
        registers[12] = vec4<f32>(0.0, 0.0, 0.0, registers[8].a);
    }
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

    // the game's alpha test (reference 0x7f); a meter's alpha kill (its
    // map's zero alpha)
    if (combiner.modes.z & 1u) != 0u && alpha < 127.5 / 255.0 {
        discard;
    }
    if (combiner.modes.z & 256u) != 0u && registers[8].a <= 0.0 {
        discard;
    }
    if combiner.fade.y > 0.5 {
        return vec4(1.0, 0.0, 1.0, 0.5);
    }
    // the fade toward the blend's neutral, then the fog: written as each blend
    // state takes it (material.rs)
    let fog = surface_fog(in);
    let fade = surface_fade(in) * fog.x;
    switch combiner.modes.x {
        // alpha blend: by its coverage
        case 0u: { return vec4(pow(rgb, vec3(2.2)) * fog.y, alpha * fade); }
        // multiply, min: the factor toward 1
        case 1u, 5u: { return vec4(pow(mix(vec3(1.0), rgb, fade), vec3(2.2)), 1.0); }
        // double multiply: twice the factor, toward 0.5
        case 2u: { return vec4(pow(2.0 * mix(vec3(0.5), rgb, fade), vec3(2.2)), 1.0); }
        // alpha multiply add: premultiplied
        case 7u: { return vec4(pow(rgb, vec3(2.2)) * fade * fog.y, alpha * fade); }
        // a meter: its blend's constant colour, the tint (stage 3's c1)
        case 8u: {
            let tint = combiner.constant1[3];
            let keep = dot(pow(tint.rgb, vec3(2.2)), vec3(1.0 / 3.0));
            return vec4(pow(rgb, vec3(2.2)) * tint.a * fade * fog.y, 1.0 - keep);
        }
        case 9u: {
            let tint = combiner.constant1[3];
            return vec4(pow(rgb, vec3(2.2)) * pow(tint.rgb, vec3(2.2)) * fade * fog.y, alpha);
        }
        // plasma: added by its alpha
        case 10u: { return vec4(pow(rgb, vec3(2.2)) * fog.y, alpha * fade); }
        // add, subtract, max
        default: { return vec4(pow(rgb, vec3(2.2)) * fade * fog.y, 1.0); }
    }
}
