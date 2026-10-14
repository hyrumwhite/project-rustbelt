// Halo's shader_environment diffuse layers as a StandardMaterial extension.
//
// The Xbox pixel shader (rasterizer_xbox_environment.c) combines, in 8-bit
// gamma space:
//   detail  = mix(secondary, primary, w)   w: secondary alpha (normal type)
//                                             or base alpha (blended types)
//   colour  = f(base, detail)               f: the detail map function
//   colour  = g(colour, micro detail)       g: the micro detail map function
// with each function one of 2*a*b, a*b or a + 2*b - 1. Textures are sampled
// as sRGB (linear), taken back to gamma for the combine, and the result
// returned to linear as the material's base colour. Lighting is Solari's.
//
// The bump map (a tangent-space normal map, decoded from the Xbox's vector
// palette) tiles at its own scale; its alpha is also what alpha-tested
// shaders cut away (the Xbox alpha-tests the bump map's pass, at 0x7F).
//
// Animated as the game animates them (rasterizer_xbox_environment.c's
// lightmap pass), each by the game's periodic functions (their tables:
// periodic_tables, a row a function) of the time: the textures' coordinates
// offset (shader_environment_texture_animation_evaluate); the
// self-illumination's primary and secondary colours between "off" and "on",
// and its plasma, the "off" colour with the "on" where the map's alpha is
// near the plasma function's value (the band, from the game's own combiner
// program). The material's emissive (a bake of the "on" colours) is scaled
// by the animated self-illumination over the baked.

import bevy_pbr::render::{
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::alpha_discard,
};
// the time (the prepass binds the globals at 1, which nothing of bevy_pbr's
// declares for it)
@if(PREPASS_PIPELINE)
import bevy_render::globals::Globals;
@if(!PREPASS_PIPELINE)
import bevy_pbr::render::mesh_view_bindings::globals;

@if(PREPASS_PIPELINE)
import bevy_pbr::{
    prepass::io::{VertexOutput, FragmentOutput},
    deferred::functions::deferred_output,
};
@else
import bevy_pbr::render::{
    forward_io::{VertexOutput, FragmentOutput},
    pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing},
};

@if(PREPASS_PIPELINE)
@group(0) @binding(1) var<uniform> globals: Globals;

struct HaloEnvironment {
    // primary detail scale (xy), secondary detail scale (zw)
    detail_scales: vec4<f32>,
    // micro detail scale (xy), bump map scale (zw)
    micro_scale: vec4<f32>,
    // x: blend detail by base alpha, y: detail function, z: micro detail
    // function, w: flags (1 bump map, 2 alpha tested, 4 animated
    // self-illumination, 8 texture animation)
    modes: vec4<u32>,
    illumination_on: array<vec4<f32>, 3>,
    illumination_off: array<vec4<f32>, 3>,
    illumination_functions: vec4<u32>,
    illumination_periods: vec4<f32>,
    illumination_phases: vec4<f32>,
    texture_functions: vec4<u32>,
    // u and v's periods (xy) and scales (zw)
    texture_animation: vec4<f32>,
    // 64 samples, 0 to 0.1 from the plasma's value
    plasma_band: array<vec4<f32>, 16>,
}

@group(constants::MATERIAL_BIND_GROUP) @binding(100) var<uniform> halo: HaloEnvironment;
@group(constants::MATERIAL_BIND_GROUP) @binding(101) var base_map: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(102) var base_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(103) var primary_detail_map: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(104) var primary_detail_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(105) var secondary_detail_map: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(106) var secondary_detail_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(107) var micro_detail_map: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(108) var micro_detail_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(109) var bump_map: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(110) var bump_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(111) var self_illumination_map: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(112) var self_illumination_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(113) var periodic_tables: texture_2d<f32>;

// periodic_functions.c's periodic_function_evaluate: the function's table at
// time * 25.6, between its samples (the slides wrapping round)
fn periodic(function: u32, time: f32) -> f32 {
    if function == 0u || function >= 12u {
        return 1.0;
    }
    let t = time * 25.6;
    let fraction = t - trunc(t);
    let index = i32(round(t - fraction)) & 1023;
    let first = textureLoad(periodic_tables, vec2<i32>(index, i32(function)), 0).r;
    var second = textureLoad(periodic_tables, vec2<i32>((index + 1) & 1023, i32(function)), 0).r;
    // (the slides: 6 and 7)
    if function == 6u || function == 7u {
        if first > 0.75 && second < 0.25 {
            second += 1.0;
        }
        let result = (1.0 - fraction) * first + second * fraction;
        return select(result, result - 1.0, result > 1.0);
    }
    return (1.0 - fraction) * first + second * fraction;
}

// the plasma's band at a distance from its value (wrapping round 0 and 1)
fn plasma_band(alpha: f32, value: f32) -> f32 {
    let difference = abs(alpha - value);
    let distance = min(difference, 1.0 - difference);
    let position = distance / 0.1 * 63.0;
    if position >= 63.0 {
        return 0.0;
    }
    let index = u32(position);
    let low = halo.plasma_band[index / 4u][index % 4u];
    let high = halo.plasma_band[(index + 1u) / 4u][(index + 1u) % 4u];
    return mix(low, high, position - f32(index));
}

fn to_gamma(c: vec3<f32>) -> vec3<f32> {
    return pow(max(c, vec3<f32>(0.0)), vec3<f32>(1.0 / 2.2));
}

fn to_linear(c: vec3<f32>) -> vec3<f32> {
    return pow(max(c, vec3<f32>(0.0)), vec3<f32>(2.2));
}

// _detail_map_function_*: double biased multiply, multiply, double biased add
fn detail_combine(a: vec3<f32>, b: vec3<f32>, function: u32) -> vec3<f32> {
    switch function {
        case 1u: { return a * b; }
        case 2u: { return a + 2.0 * b - 1.0; }
        default: { return 2.0 * a * b; }
    }
}

@fragment
fn fragment(
    in: VertexOutput,
    @builtin(front_facing) is_front: bool,
) -> FragmentOutput {
    var pbr_input = pbr_input_from_standard_material(in, is_front);

    let time = globals.time;
    var uv = in.uv;
    if (halo.modes.w & 8u) != 0u {
        uv += vec2<f32>(
            periodic(halo.texture_functions.x, time / halo.texture_animation.x) * halo.texture_animation.z,
            periodic(halo.texture_functions.y, time / halo.texture_animation.y) * halo.texture_animation.w,
        );
    }
    if (halo.modes.w & 4u) != 0u {
        let map = textureSample(self_illumination_map, self_illumination_sampler, uv);
        let periods = halo.illumination_periods;
        let phases = halo.illumination_phases;
        let primary_value = periodic(halo.illumination_functions.x, (time + phases.x) / periods.x);
        let secondary_value = periodic(halo.illumination_functions.y, (time + phases.y) / periods.y);
        let plasma_value = periodic(halo.illumination_functions.z, (time + phases.z) / periods.z);
        let on = halo.illumination_on;
        let off = halo.illumination_off;
        let primary = mix(off[0].rgb, on[0].rgb, primary_value);
        let secondary = mix(off[1].rgb, on[1].rgb, secondary_value);
        let plasma = off[2].rgb + on[2].rgb * plasma_band(map.a, plasma_value);
        let animated = map.r * primary + map.g * secondary + map.b * plasma;
        let baked = map.r * on[0].rgb + map.g * on[1].rgb + map.b * on[2].rgb;
        let ratio = select(animated / max(baked, vec3<f32>(1e-4)), vec3<f32>(0.0), baked < vec3<f32>(1e-4));
        pbr_input.material.emissive = vec4<f32>(pbr_input.material.emissive.rgb * min(ratio, vec3<f32>(16.0)), pbr_input.material.emissive.a);
    }
    let base = textureSample(base_map, base_sampler, uv);
    let primary = textureSample(primary_detail_map, primary_detail_sampler, uv * halo.detail_scales.xy);
    let secondary = textureSample(secondary_detail_map, secondary_detail_sampler, uv * halo.detail_scales.zw);
    let micro = textureSample(micro_detail_map, micro_detail_sampler, uv * halo.micro_scale.xy);

    let w = select(secondary.a, base.a, halo.modes.x != 0u);
    let detail = mix(to_gamma(secondary.rgb), to_gamma(primary.rgb), w);
    var colour = saturate(detail_combine(to_gamma(base.rgb), detail, halo.modes.y));
    colour = saturate(detail_combine(colour, to_gamma(micro.rgb), halo.modes.z));
    pbr_input.material.base_color = vec4<f32>(to_linear(colour), pbr_input.material.base_color.a);

    let bump = textureSample(bump_map, bump_sampler, uv * halo.micro_scale.zw);
    if (halo.modes.w & 2u) != 0u {
        pbr_input.material.base_color.a = bump.a;
    }
@if(VERTEX_TANGENTS) {
    if (halo.modes.w & 1u) != 0u {
        let n = normalize(in.world_normal);
        let t = normalize(in.world_tangent.xyz - n * dot(in.world_tangent.xyz, n));
        let b = cross(n, t) * in.world_tangent.w;
        let local = bump.rgb * 2.0 - 1.0;
        var normal = normalize(t * local.x + b * local.y + n * local.z);
        if !is_front {
            normal = -normal;
        }
        pbr_input.N = normal;
    }
}

    pbr_input.material.base_color = alpha_discard(
        pbr_input.material.flags,
        pbr_input.material.alpha_cutoff,
        pbr_input.material.base_color
    );

    var out: FragmentOutput;
@if(PREPASS_PIPELINE) {
    out = deferred_output(in, pbr_input);
} @else {
    out.color = apply_pbr_lighting(pbr_input);
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
}
    return out;
}
