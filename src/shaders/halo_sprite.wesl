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
//
// Fog: the game draws its effects after its fog, each faded toward its
// blend's neutral by the fog's transmittance at its own depth (the effect
// combiners' last stage). Here the fog pass comes after, over the picture by
// the depth of what lies behind (src/fog.rs). So a sprite fades by its own
// fog, and its colour is raised by what that pass will take from the surface
// behind (1 over 1 - its coverage, up to 4 times; none over the sky, which
// the pass leaves): an effect in front of a far fogged wall is no longer
// fogged as much as the wall, nor one against the sky not at all.

import bevy_pbr::render::forward_io::VertexOutput;
import bevy_pbr::render::mesh_view_bindings::view;
@if(DEPTH_PREPASS) import bevy_pbr::prepass::utils::prepass_depth;

struct HaloSprite {
    // x: framebuffer blend function, y: what a displayed 1 is written as, z: 1 for
    // nonlinear tint, w: 1 to show it in cyan (debugging)
    mode: vec4<f32>,
    // x: a zsprite's radius (metres)
    zsprite: vec4<f32>,
}

// the environment fog (halo_fog.wesl's first five; crate::fog::fog_layers),
// one buffer every material shares, so that the fog's changes (its distance
// follows the camera on some levels) rewrite no material
struct HaloFogLayers {
    atmospheric_color: vec4<f32>,
    atmospheric: vec4<f32>,
    plane: vec4<f32>,
    planar_color: vec4<f32>,
    planar: vec4<f32>,
}

@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<uniform> sprite: HaloSprite;
@group(constants::MATERIAL_BIND_GROUP) @binding(1) var map: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(2) var map_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(3) var zmap: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(4) var zmap_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(5) var<storage, read> fog_layers: HaloFogLayers;

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

// a 16-texel density texture's coordinate as the texel it falls on
fn fog_texel(u: f32) -> f32 {
    return saturate((u * 16.0 - 0.5) / 15.0);
}

// the environment fog's coverage at a view depth and a point (halo_fog.wesl,
// without the water's case)
fn fog_coverage(view_depth: f32, position: vec3<f32>) -> f32 {
    let height = dot(fog_layers.plane.xyz, position) - fog_layers.plane.w;
    let range = max(fog_layers.atmospheric.y - fog_layers.atmospheric.x, 1e-4);
    let atmospheric = fog_layers.atmospheric_color.a * fog_texel((view_depth - fog_layers.atmospheric.x) / range);
    let distance = smoothstep(0.0, 1.0, fog_texel(view_depth / max(fog_layers.planar.x, 1e-4)));
    let under = smoothstep(0.0, 1.0, fog_texel(-height / max(fog_layers.planar.y, 1e-4)));
    // (how far the camera is under the plane, from the view: the material's
    // fog leaves it out)
    let eye = saturate(-(dot(fog_layers.plane.xyz, view.world_position) - fog_layers.plane.w) / max(fog_layers.planar.y, 1e-4));
    let planar = fog_layers.planar_color.a * mix(distance * under, distance, eye);
    return 1.0 - (1.0 - atmospheric) * (1.0 - planar);
}

// the sprite's own fog's transmittance, and how much to raise its colour by
// for the fog pass over what lies behind it
fn sprite_fog(in: VertexOutput) -> vec2<f32> {
    if fog_layers.atmospheric_color.a <= 0.0 && fog_layers.planar_color.a <= 0.0 {
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

fn shade(in: VertexOutput) -> vec4<f32> {
    let fogged = sprite_fog(in);
    let shaded = shade_unfogged(in);
    let mode = u32(sprite.mode.x);
    if sprite.mode.w > 2.5 {
        return vec4(1.0 - fogged.x, (fogged.y - 1.0) / 3.0, 0.0, 1.0);
    }
    if sprite.mode.w > 0.5 { return shaded; }
    switch mode {
        // multiply, min, double multiply: the factor toward its neutral
        case 1u, 5u: {
            return vec4(mix(vec3(1.0), shaded.rgb, fogged.x), shaded.a);
        }
        case 2u: {
            return vec4(mix(vec3(1.0), shaded.rgb, fogged.x), shaded.a);
        }
        // alpha blend: its coverage
        case 0u: {
            return vec4(shaded.rgb * fogged.y, shaded.a * fogged.x);
        }
        // alpha multiply add: premultiplied
        case 7u: {
            return vec4(shaded.rgb * fogged.x * fogged.y, shaded.a * fogged.x);
        }
        // add, subtract, max
        default: {
            return vec4(shaded.rgb * fogged.x * fogged.y, shaded.a);
        }
    }
}

fn shade_unfogged(in: VertexOutput) -> vec4<f32> {
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
