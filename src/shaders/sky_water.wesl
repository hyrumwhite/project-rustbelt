// A sky's water (src/material.rs, SkyWaterMaterial): the game draws a
// shader_transparent_water on a sky model as its reflection cube map alone,
// opaque (rasterizer_xbox_water.c: no blending for the sky, and a model's
// group has no plane, so its tint is white). The cube map is in the world's
// axes (Halo's: x, y, z up), looked up along the view's reflection about
// the surface, bent by the ripples.
//
// The ripples (rasterizer_water_build_bumpmap): up to four bump maps, each
// tiled its repeats times, drifting and offset, summed by their weights into
// one bump map, which the surface tiles at the ripple scale, itself
// drifting.

import bevy_pbr::render::forward_io::VertexOutput;
import bevy_pbr::render::mesh_view_bindings::{view, globals};

struct SkyWaterParams {
    ripple: vec4<f32>,
    ripples: array<vec4<f32>, 4>,
    drifts: array<vec4<f32>, 4>,
    mipmaps: vec4<f32>,
}

@group(constants::MATERIAL_BIND_GROUP) @binding(0) var reflection: texture_cube<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(1) var reflection_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(2) var<uniform> params: SkyWaterParams;
@group(constants::MATERIAL_BIND_GROUP) @binding(3) var ripple0: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(4) var ripple0_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(5) var ripple1: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(6) var ripple1_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(7) var ripple2: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(8) var ripple2_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(9) var ripple3: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(10) var ripple3_sampler: sampler;

// a ripple's texture coordinate at the bump map's
fn ripple_uv(index: u32, uv: vec2<f32>) -> vec2<f32> {
    let r = params.ripples[index];
    return uv * r.x + globals.time * params.drifts[index].xy + r.yz;
}

// the game's bump map's size (WATER_BUMPMAP_RESOLUTION)
const BUMPMAP_SIZE: f32 = 128.0;

// a bump map's texel as a tangent-space normal
fn bump(texel: vec4<f32>) -> vec3<f32> {
    return texel.rgb * 2.0 - 1.0;
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let incident = normalize(in.world_position.xyz - view.world_position);
    var normal = normalize(in.world_normal);
    // (seen from either side: the side facing the camera)
    if dot(normal, incident) > 0.0 {
        normal = -normal;
    }

    // the ripples' sum (sampled before anything branches, for derivatives)
    let uv = in.uv * params.ripple.x + globals.time * params.ripple.yz;
    let w = vec4(params.ripples[0].w, params.ripples[1].w, params.ripples[2].w, params.ripples[3].w);
    let tangent_normal = bump(textureSample(ripple0, ripple0_sampler, ripple_uv(0u, uv))) * w.x
        + bump(textureSample(ripple1, ripple1_sampler, ripple_uv(1u, uv))) * w.y
        + bump(textureSample(ripple2, ripple2_sampler, ripple_uv(2u, uv))) * w.z
        + bump(textureSample(ripple3, ripple3_sampler, ripple_uv(3u, uv))) * w.w;
    // the surface's tangent frame from its texture coordinates' derivatives
    let dp_dx = dpdx(in.world_position.xyz);
    let dp_dy = dpdy(in.world_position.xyz);
    let duv_dx = dpdx(in.uv);
    let duv_dy = dpdy(in.uv);
    let dp_dy_perp = cross(dp_dy, normal);
    let dp_dx_perp = cross(normal, dp_dx);
    let tangent = dp_dy_perp * duv_dx.x + dp_dx_perp * duv_dy.x;
    let bitangent = dp_dy_perp * duv_dx.y + dp_dx_perp * duv_dy.y;
    let scale = inverseSqrt(max(max(dot(tangent, tangent), dot(bitangent, bitangent)), 1e-20));
    // (each of the sum's mip levels faded toward flat by its level over the
    // last, times the fade factor; sampled the game's bias below its level)
    let levels = params.mipmaps.x;
    let level = clamp(log2(max(length(duv_dx * params.ripple.x), length(duv_dy * params.ripple.x)) * BUMPMAP_SIZE) - params.mipmaps.z, 0.0, levels - 1.0);
    let fade = select(0.0, level / (levels - 1.0) * params.mipmaps.y, levels > 1.0);
    let faded = mix(tangent_normal, vec3(0.0, 0.0, 1.0), fade);
    if params.ripple.w > 0.5 && length(faded) > 1e-4 {
        let n = normalize(faded);
        normal = normalize(tangent * scale * n.x + bitangent * scale * n.y + normal * n.z);
    }

    let r = reflect(incident, normal);
    // Bevy (x, y, z) is Halo (x, -z, y)
    let halo = vec3(r.x, -r.z, r.y);
    return vec4(textureSample(reflection, reflection_sampler, halo).rgb, 1.0);
}
