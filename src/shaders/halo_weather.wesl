// The game's weather (src/weather.rs), drawn after the fog as the game draws
// its effects: a premultiplied layer over, in the display's encoding, which
// the fog pass (halo_fog.wesl) lays over the fogged picture before the lens
// flares. The colour is the effect shader's combiners' (halo_sprite.wesl's
// shade_unfogged), with t the bitmap's colour (display-encoded), v the
// vertex's and a its alpha (the fade):
//   c = t v, or with nonlinear tint (1 - v) t^4 + v t
//   alpha blend          c over, by the bitmap's alpha times a
//   alpha multiply add   c a, premultiplied by the bitmap's alpha times a
//   add (max as add)     c a, added
// each times the fog's transmittance at the flake's own depth and place (the
// effect combiners' last stage, as the game fades its effects toward their
// blend's neutral), and nothing raised for what lies behind: the fog over
// that is already in the picture. No scale either: this is after the
// tonemapper, in the display's encoding, as the flares are.
//
// There is no depth attachment at the picture's size, so a flake is hidden
// here where the scene's depth (the prepass's, and the main pass's where the
// first-person weapon's panes are nearer, as the fog pass reads them) is
// nearer than it, filtered over the four render pixels round where the
// jittered view saw the point, as the fog pass filters its fog. Reverse z:
// nearer is greater; the sky (0) hides nothing.

import bevy_render::view::View;

// halo_fog.wesl's (src/fog.rs HaloFog), the fog pass's own buffer
struct HaloFog {
    // rgb, maximum density
    atmospheric_color: vec4<f32>,
    // minimum and maximum distance (metres), the camera's density, 1 when the
    // picture holds the water
    atmospheric: vec4<f32>,
    // normal (out of the fog) and d: n.p = d on the plane (metres)
    plane: vec4<f32>,
    // rgb, maximum density
    planar_color: vec4<f32>,
    // maximum distance and depth (metres), the camera's density, the sky's
    // clear
    planar: vec4<f32>,
    screen_color: vec4<f32>,
    screen: vec4<f32>,
    screen_transforms: array<vec4<f32>, 8>,
    screen_weights: array<vec4<f32>, 4>,
}

@group(0) @binding(0) var<uniform> view: View;
@group(0) @binding(1) var<uniform> fog: HaloFog;
@group(0) @binding(2) var depth: texture_depth_2d;
// the main pass's depth: nearer than the prepass's where the first-person
// weapon's panes are, which the prepass leaves out
@group(0) @binding(3) var scene_depth: texture_depth_2d;
@group(1) @binding(0) var map: texture_2d<f32>;
@group(1) @binding(1) var map_sampler: sampler;

// metres: the level's panes' masks' depth (halo_fog.wesl's), which is none
const GLASS_SKY_MASK_DISTANCE: f32 = 50000.0;

struct Vertex {
    @location(0) position: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) color: vec4<f32>,
    // 0 alpha blend, 1 alpha multiply add, 2 added (src/weather.rs pass_mode)
    @location(3) mode: f32,
    // 1 for nonlinear tint
    @location(4) nonlinear: f32,
}

struct Varyings {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) world_position: vec3<f32>,
    // the clip position again, for the picture's coordinates whatever the
    // viewport
    @location(3) screen: vec4<f32>,
    @location(4) mode: f32,
    @location(5) nonlinear: f32,
}

fn srgb_encode(linear: vec3<f32>) -> vec3<f32> {
    let low = linear * 12.92;
    let high = 1.055 * pow(max(linear, vec3(0.0)), vec3(1.0 / 2.4)) - 0.055;
    return select(high, low, linear <= vec3(0.0031308));
}

// a 16-texel density texture's coordinate as the texel it falls on
fn fog_texel(u: f32) -> f32 {
    return saturate((u * 16.0 - 0.5) / 15.0);
}

// the fog's transmittance at a point (halo_sprite.wesl's fog_coverage,
// without the water's case; the camera's planar density the fog pass's)
fn own_fog(position: vec3<f32>) -> f32 {
    if fog.atmospheric_color.a <= 0.0 && fog.planar_color.a <= 0.0 {
        return 1.0;
    }
    let forward = -normalize(view.world_from_view[2].xyz);
    let view_depth = dot(position - view.world_position, forward);
    let height = dot(fog.plane.xyz, position) - fog.plane.w;
    let range = max(fog.atmospheric.y - fog.atmospheric.x, 1e-4);
    let atmospheric = fog.atmospheric_color.a * fog_texel((view_depth - fog.atmospheric.x) / range);
    let distance = smoothstep(0.0, 1.0, fog_texel(view_depth / max(fog.planar.x, 1e-4)));
    let under = smoothstep(0.0, 1.0, fog_texel(-height / max(fog.planar.y, 1e-4)));
    let planar = fog.planar_color.a * mix(distance * under, distance, fog.planar.z);
    return (1.0 - atmospheric) * (1.0 - planar);
}

// whether a flake at a depth shows at one of the render resolution's pixels:
// the scene's depth there as halo_fog.wesl's fog_at reads it
fn shows(pixel: vec2<i32>, flake_z: f32) -> f32 {
    var z = textureLoad(depth, pixel, 0);
    let pane_z = textureLoad(scene_depth, pixel, 0);
    let mask_z = view.clip_from_view[3][2] / GLASS_SKY_MASK_DISTANCE;
    if pane_z > z * 1.0001 && abs(pane_z - mask_z) > mask_z * 0.01 {
        z = pane_z;
    }
    return select(0.0, 1.0, z <= 0.0 || flake_z >= z);
}

@vertex
fn vertex(in: Vertex) -> Varyings {
    var out: Varyings;
    // (the picture is upscaled by now: no jitter)
    out.clip = view.unjittered_clip_from_world * vec4(in.position, 1.0);
    out.uv = in.uv;
    out.color = in.color;
    out.world_position = in.position;
    out.screen = out.clip;
    out.mode = in.mode;
    out.nonlinear = in.nonlinear;
    return out;
}

@fragment
fn fragment(in: Varyings) -> @location(0) vec4<f32> {
    // (sampled before anything is discarded, for its derivatives)
    let sample = textureSample(map, map_sampler, in.uv);

    // the picture's coordinates, then the depth's texels as the fog pass
    // finds them (halo_fog.wesl's fragment: the render resolution, jittered,
    // below DLAA in the corner of a texture the picture's size)
    let ndc = in.screen.xy / in.screen.w;
    let uv = vec2(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5);
    let anchor = view.world_position - normalize(view.world_from_view[2].xyz);
    let jittered = view.clip_from_world * vec4(anchor, 1.0);
    let steady = view.unjittered_clip_from_world * vec4(anchor, 1.0);
    let jitter = jittered.xy / jittered.w - steady.xy / steady.w;
    let viewport = view.main_pass_viewport;
    let at = viewport.xy + (uv + vec2(jitter.x, -jitter.y) * 0.5) * viewport.zw - 0.5;
    let base = floor(at);
    let weight = at - base;
    let low = vec2<i32>(viewport.xy);
    let high = vec2<i32>(viewport.xy + viewport.zw) - 1;
    let p00 = clamp(vec2<i32>(base), low, high);
    let p11 = clamp(vec2<i32>(base) + 1, low, high);
    // (the flake's own depth: the jitter moves x and y alone)
    let flake_z = in.clip.z;
    let visible = shows(p00, flake_z) * (1.0 - weight.x) * (1.0 - weight.y)
        + shows(vec2(p11.x, p00.y), flake_z) * weight.x * (1.0 - weight.y)
        + shows(vec2(p00.x, p11.y), flake_z) * (1.0 - weight.x) * weight.y
        + shows(p11, flake_z) * weight.x * weight.y;
    if visible <= 0.0 {
        discard;
    }

    let t = srgb_encode(sample.rgb);
    let v = in.color;
    var c = t * v.rgb;
    if in.nonlinear > 0.5 {
        let t2 = t * t;
        c = (1.0 - v.rgb) * t2 * t2 + v.rgb * t;
    }
    let fade = v.a * own_fog(in.world_position) * visible;
    // alpha blend: over, by its coverage
    if in.mode < 0.5 {
        let a = sample.a * fade;
        return vec4(c * a, a);
    }
    // alpha multiply add: premultiplied
    if in.mode < 1.5 {
        return vec4(c * fade, sample.a * fade);
    }
    // added
    return vec4(c * fade, 0.0);
}
