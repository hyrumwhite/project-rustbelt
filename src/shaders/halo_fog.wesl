// The game's environment fog and fog screen (src/fog.rs), over the
// tonemapped picture as the game draws them over its world, in the display's
// encoding.
//
// The fog is rasterizer_xbox_environment_fog.c's pass, whose register
// combiners, decoded, are
//   atmospheric = maximum density * A(view depth from its minimum distance
//                 over its range)
//   planar      = maximum density * mix(L(distance) * L(depth), L(distance),
//                 the camera's own planar density)
//   colour      = atmospheric colour * atmospheric * (1 - (1 - eye) * planar)
//               + planar colour * planar * (1 - atmospheric * eye)
//   coverage    = 1 - (1 - atmospheric) * (1 - planar)
// blended ONE, INVSRCALPHA, where eye is the camera's atmospheric density (1
// above the fog plane, 0 under it), the distance is the view depth over the
// planar maximum distance, and the depth is under the fog plane over its
// maximum depth. A and L are the game's density textures
// (rasterizer\atmospheric fog density: a ramp; rasterizer\planar fog
// density: a smoothstep), 16 texels sampled with linear filtering, clamped.
// The sky (nothing in the depth buffer) is left as it is: the game's fog pass
// covers the level's surfaces only. But where the atmospheric fog is opaque
// (its maximum density 1), the game draws nothing past its maximum distance:
// there the sky is fogged as a surface at that distance. And where the
// level's sky has no model (b40's), the sky is the game's clear: the fog's
// colour, under what was drawn there (the camera's skybox dark,
// crate::follow_camera_sky), added in the display's encoding as the game
// adds its additive surfaces over the clear. b40's dome: its panes and the
// energy field behind them, with nothing opaque beyond, were painted over
// with the fog's colour (HALO_RT_SKY_CLEAR_OVER=1 the old, painted over).
// An alpha-blended or tinting surface there cannot darken the clear, added
// after it: only the additive ones are exact.
//
// Where the picture holds the water (atmospheric.w), a surface under the fog
// plane seen from above it was seen through the water's surface (the plane,
// in b30): the game fogs it and draws the water's reflection over it, but
// here the reflection is already in the picture with it. So there the
// planar fog covers only the light that came through, 1 - the water's
// Fresnel reflectance (Schlick's, F0 0.04, as its ray-traced pane's) at the
// view's angle to the plane, and the atmospheric fog is to the surface.
//
// The fog screen is its _rasterizer_environment_fog_screen_end: up to four
// layers over the picture, each a density, the dot product of its frame's
// colour and its weights (the game moves them on and shares them), covering
// 1 - the product of (1 - each) in the screen's colour. Added to the
// picture, times the surface's density from its first pass (the near over
// the far density at the near distance, up to 1 at the far, along L; 1 for
// the sky), when its densities differ; else blended over all of it.
//
// After both, the game's weather (src/weather.rs: display-encoded, a
// premultiplied layer, each flake already faded by its own fog), laid over,
// as the game draws its effects after its fog. Then the game's lens flares,
// added up (src/lens_flares.rs, display-encoded light): added, as the game
// adds them over its picture.

import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput;
import bevy_render::view::View;

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
    // maximum distance and depth (metres), the camera's density, 1 when the
    // level's sky has no model (the fog's colour under the picture), 2 the
    // same painted over it (HALO_RT_SKY_CLEAR_OVER)
    planar: vec4<f32>,
    // the fog screen: rgb, and its mode (0 none, 1 added by depth, 2 blended)
    screen_color: vec4<f32>,
    // near and far distance (metres), near over far density, 1 if its
    // depth's density is flat (no texture)
    screen: vec4<f32>,
    // each layer's texture coordinates from the picture's: u's (x, y, 1)
    // then v's
    screen_transforms: array<vec4<f32>, 8>,
    // each layer's frame's weights, and 1 for a layer
    screen_weights: array<vec4<f32>, 4>,
}

@group(0) @binding(0) var source: texture_2d<f32>;
@group(0) @binding(1) var source_sampler: sampler;
@group(0) @binding(2) var depth: texture_depth_2d;
@group(0) @binding(3) var<uniform> view: View;
@group(0) @binding(4) var<uniform> fog: HaloFog;
@group(0) @binding(5) var layer_0: texture_2d<f32>;
@group(0) @binding(6) var layer_1: texture_2d<f32>;
@group(0) @binding(7) var layer_2: texture_2d<f32>;
@group(0) @binding(8) var layer_3: texture_2d<f32>;
@group(0) @binding(9) var layer_sampler: sampler;
@group(0) @binding(10) var flares: texture_2d<f32>;
// the main pass's depth: nearer than the prepass's where the first-person
// weapon's panes (the needler's needles) are, which the prepass leaves out
@group(0) @binding(11) var scene_depth: texture_depth_2d;
// the weather's layer (src/weather.rs): premultiplied, display-encoded
@group(0) @binding(12) var weather: texture_2d<f32>;
// the picture read between its pixels, filtered: the guide at a render
// pixel's centre (fog_edge_guide)
@group(0) @binding(13) var guide_sampler: sampler;

// metres: the level's panes' masks' depth (src/material.rs,
// GLASS_SKY_MASK_DISTANCE), which is none
const GLASS_SKY_MASK_DISTANCE: f32 = 50000.0;

// the sky under the horizon fogged as the sea (fog_at);
// HALO_RT_SEA_HORIZON_FOG=0 leaves it bare, the old
@if(SEA_HORIZON_UNFOGGED) const SEA_HORIZON_FOG: bool = false;
@if(!SEA_HORIZON_UNFOGGED) const SEA_HORIZON_FOG: bool = true;

// Silhouettes (HALO_RT_FOG_EDGES): where the four render pixels' fogs differ
// by more than the first (the largest difference of any channel, light or
// coverage, in the display's encoding), the edge's rule takes over from the
// filter, wholly by the second; between, the two mixed, so that a pixel
// does not switch from one to the other as the jitter moves the depths
// under it. A surface's neighbours mostly differ by less than the first
// (where they do not, a surface seen at a grazing angle, the guide finds
// their pictures alike and keeps the filter's weights); b30's leaves against
// the sea under its fog (its density 0.5) by tenths.
const FOG_EDGE_START: f32 = 0.01;
const FOG_EDGE_FULL: f32 = 0.04;
// The guide's sharpness, in the display's encoding: how far a render
// pixel's picture (its luma, and its chroma, blue and red less the luma) may
// be from this pixel's before its fog counts for less, a Gaussian's width.
// The picture is before the fog, so the far side is the bare surface, not
// the pale fogged one, and may be nearer the near side's colour than what is
// seen: kept sharp, but meant to hold a surface's own texture (a few
// hundredths) inside it (chosen by reasoning, not yet tuned on captures).
const FOG_GUIDE_LUMA: f32 = 0.06;
const FOG_GUIDE_CHROMA: f32 = 0.04;
// The weight every render pixel keeps whatever its picture: where none is
// like this pixel's, or all are alike, they fall back to the filter's
// weights (and a sample's lead over another is at most 1 / this)
const FOG_GUIDE_FLOOR: f32 = 0.01;

// a 16-texel texture's coordinate as the texel it falls on (0 to 1 over the
// texels' centres, clamped)
fn texel(u: f32) -> f32 {
    return saturate((u * 16.0 - 0.5) / 15.0);
}

fn srgb_encode(linear: vec3<f32>) -> vec3<f32> {
    let low = linear * 12.92;
    let high = 1.055 * pow(linear, vec3(1.0 / 2.4)) - 0.055;
    return select(high, low, linear <= vec3(0.0031308));
}

fn srgb_decode(encoded: vec3<f32>) -> vec3<f32> {
    let low = encoded / 12.92;
    let high = pow((encoded + 0.055) / 1.055, vec3(2.4));
    return select(high, low, encoded <= vec3(0.04045));
}

// the picture's colour as the guide compares it: luma, and blue and red
// less the luma, in the display's encoding, each over its width
fn guide_of(colour: vec3<f32>) -> vec3<f32> {
    let encoded = srgb_encode(saturate(colour));
    let luma = dot(encoded, vec3(0.2126, 0.7152, 0.0722));
    return vec3(luma / FOG_GUIDE_LUMA, (encoded.b - luma) / FOG_GUIDE_CHROMA, (encoded.r - luma) / FOG_GUIDE_CHROMA);
}

// How like this pixel's guide the picture is at a render pixel's centre
// (where the jittered view saw it, on the picture: to_uv the jitter, there
// in the picture's coordinates), filtered between the picture's pixels so
// that it moves smoothly with the jitter; never less than the floor.
fn fog_edge_guide(guide: vec3<f32>, pixel: vec2<i32>, viewport: vec4<f32>, to_uv: vec2<f32>) -> f32 {
    let uv = (vec2<f32>(pixel) + 0.5 - viewport.xy) / viewport.zw - to_uv;
    let difference = guide_of(textureSampleLevel(source, guide_sampler, uv, 0.0).rgb) - guide;
    return exp(-0.5 * dot(difference, difference)) + FOG_GUIDE_FLOOR;
}

// a fog screen layer's density at the picture's point
fn layer_density(index: u32, colour: vec4<f32>) -> f32 {
    let weights = fog.screen_weights[index];
    return saturate(dot(colour.rgb, weights.rgb)) * weights.w;
}

// The environment fog at one of the render resolution's pixels: the light
// it adds (rgb) and its coverage (a), the fog screen's density there (1 for
// the sky), and the depth it was fogged at (reversed: larger nearer, 0 for
// the sky).
struct Fogged {
    fogged: vec4<f32>,
    screen_density: f32,
    z: f32,
}

fn fog_at(pixel: vec2<i32>) -> Fogged {
    var z = textureLoad(depth, pixel, 0);
    // (a first-person pane in front: fogged at its own depth)
    let pane_z = textureLoad(scene_depth, pixel, 0);
    let mask_z = view.clip_from_view[3][2] / GLASS_SKY_MASK_DISTANCE;
    if pane_z > z * 1.0001 && abs(pane_z - mask_z) > mask_z * 0.01 {
        z = pane_z;
    }
    var result = Fogged(vec4(0.0), 1.0, max(z, 0.0));
    // (the sky, where the atmospheric fog is opaque: the game brings its far
    // plane in to the fog's maximum distance (render.c), and draws no sky
    // past it, only fog; as a surface there. c10's swamps: the treetops in
    // the fog, against the sky, stood out bare)
    let opaque = fog.atmospheric_color.a >= 1.0 && fog.atmospheric.y > 0.0;
    // (the sky, where the level's has no model: the game clears its picture
    // to the fog's colour, and draws no sky over it: b40's snowy white; its
    // panes and additive surfaces over the clear, so the colour is added
    // under them, no coverage: b40's dome, its panes and the energy field
    // behind them were painted over. 2: painted over, the old)
    if z <= 0.0 && !opaque && fog.planar.w > 0.5 {
        let over = select(0.0, 1.0, fog.planar.w > 1.5);
        result.fogged = vec4(fog.atmospheric_color.rgb, over);
        return result;
    }
    // (the sky under the horizon, where the picture holds the sea and the
    // camera is over it (HALO_RT_SEA_HORIZON_FOG): past the level's sea, a
    // thousand world units out on b30, the sky's own water showed bare, a
    // white band between the fogged sea and the sky (in the game too); now
    // fogged as the sea over a deep floor, through the water below. The
    // modelless sky and the opaque fog keep their rules)
    let camera_height = dot(fog.plane.xyz, view.world_position) - fog.plane.w;
    let sea_sky = SEA_HORIZON_FOG && z <= 0.0 && !opaque && fog.atmospheric.w > 0.5 && camera_height > 0.0;
    if z <= 0.0 && !opaque && !sea_sky {
        return result;
    }
    // (the pixel's centre, where the jittered view sampled it)
    let viewport = view.main_pass_viewport;
    let uv = (vec2<f32>(pixel) + 0.5 - viewport.xy) / viewport.zw;
    let ndc = vec2(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0);
    let forward = -normalize(view.world_from_view[2].xyz);
    var position: vec3<f32>;
    // (the sea's floor under the plane, for the sky under the horizon: none
    // else)
    var sea_floor = 0.0;
    if sea_sky {
        let near = view.world_from_clip * vec4(ndc, 1.0, 1.0);
        let ray = normalize(near.xyz / near.w - view.world_position);
        let down = -dot(ray, fog.plane.xyz);
        // (over the horizon: the sky, unfogged)
        if down <= 0.0 {
            return result;
        }
        // twice the planar fog's maximum depth under the plane, along the
        // ray: its depth's density full. Its height is set, not found from
        // the point, which near the horizon is a great way off, past the
        // float's precision for a few metres
        sea_floor = max(fog.planar.y, 1e-4) * 2.0;
        position = view.world_position + ray * ((camera_height + sea_floor) / max(down, 1e-7));
    } else if z <= 0.0 {
        let near = view.world_from_clip * vec4(ndc, 1.0, 1.0);
        let ray = near.xyz / near.w - view.world_position;
        position = view.world_position + ray * (fog.atmospheric.y / max(dot(ray, forward), 1e-6));
    } else {
        let p = view.world_from_clip * vec4(ndc, z, 1.0);
        position = p.xyz / p.w;
    }
    let view_depth = dot(position - view.world_position, forward);

    // (through the water: the camera above the plane, the surface under it)
    var surface_height = dot(fog.plane.xyz, position) - fog.plane.w;
    if sea_floor > 0.0 {
        surface_height = -sea_floor;
    }
    var transmitted = 1.0;
    var atmospheric_depth = view_depth;
    if fog.atmospheric.w > 0.5 && camera_height > 0.0 && surface_height < 0.0 {
        let ray = normalize(position - view.world_position);
        let cosine = abs(dot(ray, fog.plane.xyz));
        transmitted = 1.0 - (0.04 + 0.96 * pow(1.0 - cosine, 5.0));
        atmospheric_depth = view_depth * camera_height / (camera_height - surface_height);
    }

    let range = max(fog.atmospheric.y - fog.atmospheric.x, 1e-4);
    let atmospheric = fog.atmospheric_color.a * texel((atmospheric_depth - fog.atmospheric.x) / range);
    let distance = smoothstep(0.0, 1.0, texel(view_depth / max(fog.planar.x, 1e-4)));
    let under = smoothstep(0.0, 1.0, texel(-surface_height / max(fog.planar.y, 1e-4)));
    let planar = fog.planar_color.a * mix(distance * under, distance, fog.planar.z) * transmitted;

    let eye = fog.atmospheric.z;
    let fogged = fog.atmospheric_color.rgb * atmospheric * (1.0 - (1.0 - eye) * planar)
        + fog.planar_color.rgb * planar * (1.0 - atmospheric * eye);
    let coverage = 1.0 - (1.0 - atmospheric) * (1.0 - planar);
    result.fogged = vec4(fogged, coverage);

    if fog.screen.w < 0.5 {
        let range = max(fog.screen.y - fog.screen.x, 1e-4);
        result.screen_density = mix(fog.screen.z, 1.0, smoothstep(0.0, 1.0, texel((view_depth - fog.screen.x) / range)));
    }
    return result;
}

@fragment
fn fragment(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let colour = textureSample(source, source_sampler, in.uv);
    let ndc = vec2(in.uv.x * 2.0 - 1.0, 1.0 - in.uv.y * 2.0);

    // the fog screen's layers (sampled before anything returns, for their
    // derivatives)
    let s = vec3(ndc, 1.0);
    let t = fog.screen_transforms;
    let d0 = layer_density(0u, textureSample(layer_0, layer_sampler, vec2(dot(t[0].xyz, s), dot(t[1].xyz, s))));
    let d1 = layer_density(1u, textureSample(layer_1, layer_sampler, vec2(dot(t[2].xyz, s), dot(t[3].xyz, s))));
    let d2 = layer_density(2u, textureSample(layer_2, layer_sampler, vec2(dot(t[4].xyz, s), dot(t[5].xyz, s))));
    let d3 = layer_density(3u, textureSample(layer_3, layer_sampler, vec2(dot(t[6].xyz, s), dot(t[7].xyz, s))));
    let screen_coverage = 1.0 - (1.0 - d0) * (1.0 - d1) * (1.0 - d2) * (1.0 - d3);

    // (sampled before anything returns, as the layers)
    let flare = textureSample(flares, source_sampler, in.uv).rgb;
    let weather_over = textureSample(weather, source_sampler, in.uv);

    // The depth is at the render resolution, jittered each frame, while the
    // picture is upscaled and steady: the fog of the four render pixels round
    // where the jittered view saw this point, filtered. Below DLAA the depth
    // texture is the picture's size, the view drawn in its corner (the main
    // pass's viewport): read over the whole texture, the fog was from depths
    // elsewhere in the picture (b30's far sea a white band, streaked, and
    // outlines round models against it); from the nearest pixel alone, a
    // silhouette's edge was fogged as the surface beside it now and then.
    let anchor = view.world_position - normalize(view.world_from_view[2].xyz);
    let jittered = view.clip_from_world * vec4(anchor, 1.0);
    let steady = view.unjittered_clip_from_world * vec4(anchor, 1.0);
    let jitter = jittered.xy / jittered.w - steady.xy / steady.w;
    let viewport = view.main_pass_viewport;
    let at = viewport.xy + (in.uv + vec2(jitter.x, -jitter.y) * 0.5) * viewport.zw - 0.5;
    let base = floor(at);
    let weight = at - base;
    let low = vec2<i32>(viewport.xy);
    let high = vec2<i32>(viewport.xy + viewport.zw) - 1;
    let p00 = clamp(vec2<i32>(base), low, high);
    let p11 = clamp(vec2<i32>(base) + 1, low, high);
    let f00 = fog_at(p00);
    let f10 = fog_at(vec2(p11.x, p00.y));
    let f01 = fog_at(vec2(p00.x, p11.y));
    let f11 = fog_at(p11);
    let w00 = (1.0 - weight.x) * (1.0 - weight.y);
    let w10 = weight.x * (1.0 - weight.y);
    let w01 = (1.0 - weight.x) * weight.y;
    let w11 = weight.x * weight.y;
    var fogged = f00.fogged * w00 + f10.fogged * w10 + f01.fogged * w01 + f11.fogged * w11;
    var screen_density = f00.screen_density * w00 + f10.screen_density * w10 + f01.screen_density * w01 + f11.screen_density * w11;

    // A silhouette among the four (HALO_RT_FOG_EDGES): the picture is
    // upscaled to the display's pixels, each edge where the upscaler put it,
    // while the four are a render pixel apart, so filtered by place a near
    // leaf's pixels within a render pixel of the far sea took part of the
    // sea's fog (b30's trees against the water, a pale rim, wider the lower
    // the render resolution; none against the sky, which takes no fog, the
    // leaves' own little). Where they differ, each sample is taken whole, its
    // light, coverage and screen density together. 0: filtered by place, the
    // old.
    @if(!FOG_EDGES_BILINEAR) {
        let spread_low = min(min(f00.fogged, f10.fogged), min(f01.fogged, f11.fogged));
        let spread_high = max(max(f00.fogged, f10.fogged), max(f01.fogged, f11.fogged));
        let spread_fog = spread_high - spread_low;
        var spread = max(max(spread_fog.r, spread_fog.g), max(spread_fog.b, spread_fog.a));
        // (the screen's density counts only where it is added by depth)
        if fog.screen_color.a > 0.5 && fog.screen_color.a < 1.5 {
            let screen_low = min(min(f00.screen_density, f10.screen_density), min(f01.screen_density, f11.screen_density));
            let screen_high = max(max(f00.screen_density, f10.screen_density), max(f01.screen_density, f11.screen_density));
            spread = max(spread, (screen_high - screen_low) * max(max(fog.screen_color.r, fog.screen_color.g), fog.screen_color.b));
        }
        let edge = smoothstep(FOG_EDGE_START, FOG_EDGE_FULL, spread);
        if edge > 0.0 {
            var chosen: Fogged;
            @if(FOG_EDGES_NEAR) {
                // near: the nearest sample's fog, a near object's pixels
                // none of the far side's (and the far side's within a render
                // pixel of it the near object's: a thin rim less fogged)
                chosen = f00;
                if f10.z > chosen.z { chosen = f10; }
                if f01.z > chosen.z { chosen = f01; }
                if f11.z > chosen.z { chosen = f11; }
            }
            @if(!FOG_EDGES_NEAR) {
                // guided (the default): each sample's filter weight times
                // how like this pixel's picture the picture is where that
                // sample lies, so that a pixel takes the fog of the side the
                // upscaler drew it on
                let guide = guide_of(colour.rgb);
                let to_uv = vec2(jitter.x, -jitter.y) * 0.5;
                let g00 = fog_edge_guide(guide, p00, viewport, to_uv) * w00;
                let g10 = fog_edge_guide(guide, vec2(p11.x, p00.y), viewport, to_uv) * w10;
                let g01 = fog_edge_guide(guide, vec2(p00.x, p11.y), viewport, to_uv) * w01;
                let g11 = fog_edge_guide(guide, p11, viewport, to_uv) * w11;
                let total = max(g00 + g10 + g01 + g11, 1e-6);
                chosen.fogged = (f00.fogged * g00 + f10.fogged * g10 + f01.fogged * g01 + f11.fogged * g11) / total;
                chosen.screen_density = (f00.screen_density * g00 + f10.screen_density * g10 + f01.screen_density * g01 + f11.screen_density * g11) / total;
            }
            fogged = mix(fogged, chosen.fogged, edge);
            screen_density = mix(screen_density, chosen.screen_density, edge);
        }
    }

    var encoded = srgb_encode(saturate(colour.rgb));
    var changed = false;
    // (a modelless sky's clear adds its colour with no coverage)
    if fogged.a > 0.0 || any(fogged.rgb > vec3(0.0)) {
        encoded = fogged.rgb + encoded * (1.0 - fogged.a);
        changed = true;
    }

    let mode = fog.screen_color.a;
    if mode > 0.5 && screen_coverage > 0.0 {
        if mode < 1.5 {
            // added, by the surface's density (the sky's: 1)
            encoded += fog.screen_color.rgb * screen_coverage * screen_density;
        } else {
            encoded = mix(encoded, fog.screen_color.rgb, screen_coverage);
        }
        changed = true;
    }
    // (the weather over the fogged picture: its flakes faded by their own fog)
    if weather_over.a > 0.0 || any(weather_over.rgb > vec3(0.0)) {
        encoded = encoded * (1.0 - weather_over.a) + weather_over.rgb;
        changed = true;
    }
    // (added as the game's framebuffer adds them: up to 1)
    if any(flare > vec3(0.0)) {
        encoded = max(encoded, min(encoded + flare, vec3(1.0)));
        changed = true;
    }
    if !changed {
        return colour;
    }
    return vec4(srgb_decode(saturate(encoded)), colour.a);
}
