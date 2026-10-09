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

import bevy_pbr::render::{
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::alpha_discard,
};

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

struct HaloEnvironment {
    // primary detail scale (xy), secondary detail scale (zw)
    detail_scales: vec4<f32>,
    // micro detail scale (xy)
    micro_scale: vec4<f32>,
    // x: blend detail by base alpha, y: detail function, z: micro detail function
    modes: vec4<u32>,
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

    let uv = in.uv;
    let base = textureSample(base_map, base_sampler, uv);
    let primary = textureSample(primary_detail_map, primary_detail_sampler, uv * halo.detail_scales.xy);
    let secondary = textureSample(secondary_detail_map, secondary_detail_sampler, uv * halo.detail_scales.zw);
    let micro = textureSample(micro_detail_map, micro_detail_sampler, uv * halo.micro_scale.xy);

    let w = select(secondary.a, base.a, halo.modes.x != 0u);
    let detail = mix(to_gamma(secondary.rgb), to_gamma(primary.rgb), w);
    var colour = saturate(detail_combine(to_gamma(base.rgb), detail, halo.modes.y));
    colour = saturate(detail_combine(colour, to_gamma(micro.rgb), halo.modes.z));
    pbr_input.material.base_color = vec4<f32>(to_linear(colour), pbr_input.material.base_color.a);

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
