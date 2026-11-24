// The game's environment fog, fog screen, weather and lens flares
// (src/fog.rs).
//
// The environment fog is rasterizer_xbox_environment_fog.c's pass, whose
// register combiners, decoded, are
//   atmospheric = maximum density * A(view depth from its minimum distance
//                 over its range)
//   planar      = maximum density * mix(L(distance) * L(depth), L(distance),
//                 the camera's own planar density)
//   colour      = atmospheric colour * atmospheric * (1 - (1 - eye) * planar)
//               + planar colour * planar * (1 - atmospheric * eye)
//   coverage    = 1 - (1 - atmospheric) * (1 - planar)
// blended ONE, INVSRCALPHA over the display's picture, where eye is the
// camera's atmospheric density (1 above the fog plane, 0 under it), the
// distance is the view depth over the planar maximum distance, and the depth
// is under the fog plane over its maximum depth. A and L are the game's
// density textures (rasterizer\atmospheric fog density: a ramp; rasterizer\
// planar fog density: a smoothstep), 16 texels sampled with linear filtering,
// clamped (fog_at). The sky (nothing in the depth buffer) is left as it is,
// but for three rules: where the atmospheric fog is opaque (its maximum
// density 1) the game draws nothing past its maximum distance, so the sky is
// fogged as a surface there; where the level's sky has no model (b40's) the
// sky is the game's clear, the fog's colour, added under what was drawn there
// (the camera's skybox dark, crate::follow_camera_sky; HALO_RT_SKY_CLEAR_OVER=1
// paints it over); and under the horizon over the sea it is fogged as the
// sea over a deep floor (HALO_RT_SEA_HORIZON_FOG=0 leaves it bare). Where the
// picture holds the water (atmospheric.w), a surface under the fog plane seen
// from above it was seen through the water's surface: the planar fog covers
// only the light that came through, 1 - the water's Fresnel reflectance
// (Schlick's, F0 0.04) at the view's angle to the plane, and the atmospheric
// fog is to the surface.
//
// `before`, a compute entry point run over the lit picture after Solarik's
// lighting and before the transparent pass, at the render resolution (the
// main pass's viewport), lays that fog on each render pixel by its own depth,
// so the upscaler takes a fogged picture and the transparents are drawn over
// it with their own fog alone, as the game draws its effects after its fog.
// The blend is the game's, in the display's encoding, taken back to the linear
// light the tonemapper follows with: form 2 (fog.lit.w, the default) through
// the tonemapper's curve t along the grey axis, per channel (TONE_CURVE,
// Bevy's TonyMcMapface sampled on its grey axis), so that what is displayed is
// the game's blend of what was displayed,
//   g(P) = t_inv(dec(F + enc(t(P)) (1 - b))),
// form 1 (HALO_RT_FOG_BEFORE=1) taking the tonemapper as straight,
//   g(P) = dec(F + enc(P) (1 - b)).
// The picture there is noisy (the lighting's, before the denoiser), so g is
// taken at a local mean and carried to the pixel as a ratio (affine_about).
// Lit fog's light (Solarik's lit_fog.wesl, at the render resolution, already
// denoised) is laid over in the same pass, screened over what is shown through
// the tonemapper's curve as 1 - exp(-light), taken about the fogged mean the
// same way (HALO_RT_FOG_BEFORE_LIT=add: added as linear light).
//
// `fragment`, the fog pass after tonemapping and the upscaler, lays the rest
// over the display's picture in its encoding. The fog screen is
// _rasterizer_environment_fog_screen_end: up to four layers, each a density,
// the dot product of its frame's colour and its weights (the game moves them
// on and shares them), covering 1 - the product of (1 - each) in the screen's
// colour; added to the picture times the surface's density from its first
// pass (the near over the far density at the near distance, up to 1 at the
// far, along L; 1 for the sky) when its densities differ, from the four render
// pixels round where the jittered view saw this one, filtered by place; else
// blended over all of it. Then the game's weather (src/weather.rs:
// display-encoded, a premultiplied layer, each flake already faded by its own
// fog), laid over, and last the game's lens flares (src/lens_flares.rs),
// added, as the game draws its effects after its fog.

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
    // x lit fog's light laid over by `before` (1 by the shoulder, 2 added; 0
    // none), y 1 to show the light alone, z unused, w the blend's form
    // (src/fog.rs FogBefore: 1 the tonemapper taken as straight, 2 its curve;
    // 0 no fog)
    lit: vec4<f32>,
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
// (`before` alone, src/fog.rs's two layouts: 0 the picture's copy for the
// local mean, 2 to 4, 11, and these two)
// lit fog's light (Solarik's lit_fog_history of the frame): linear, at the
// exposure and its strength, at the render resolution in the corner of the
// texture below DLAA (the main pass's viewport); a black texel without it
@group(0) @binding(14) var lit_light: texture_2d<f32>;
// the lit picture, fogged in place
@group(0) @binding(15) var fogged_picture: texture_storage_2d<rgba16float, read_write>;

// metres: the level's panes' masks' depth (src/material.rs,
// GLASS_SKY_MASK_DISTANCE), which is none
const GLASS_SKY_MASK_DISTANCE: f32 = 50000.0;

// the sky under the horizon fogged as the sea (fog_at);
// HALO_RT_SEA_HORIZON_FOG=0 leaves it bare, the old
@if(SEA_HORIZON_UNFOGGED) const SEA_HORIZON_FOG: bool = false;
@if(!SEA_HORIZON_UNFOGGED) const SEA_HORIZON_FOG: bool = true;

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

// Lit fog's light as shown over the tonemapped picture, in linear display
// space: the light is the scene's (at the exposure, unbounded), so it goes
// through a soft shoulder of its own (1 - exp(-light): about the light where
// it is faint, never 1)
fn lit_fog_shown(light: vec3<f32>) -> vec3<f32> {
    return 1.0 - exp(-light);
}

// ... and screened over the picture (linear): brightens what is dark the
// most, never over 1
fn lit_fog_over(picture: vec3<f32>, light: vec3<f32>) -> vec3<f32> {
    return picture + lit_fog_shown(light) * (1.0 - picture);
}

// a fog screen layer's density at the picture's point
fn layer_density(index: u32, colour: vec4<f32>) -> f32 {
    let weights = fog.screen_weights[index];
    return saturate(dot(colour.rgb, weights.rgb)) * weights.w;
}

// The environment fog at one of the render resolution's pixels: the light
// it adds (rgb) and its coverage (a), and the fog screen's density there (1
// for the sky).
struct Fogged {
    fogged: vec4<f32>,
    screen_density: f32,
}

fn fog_at(pixel: vec2<i32>) -> Fogged {
    var z = textureLoad(depth, pixel, 0);
    // (a first-person pane in front: fogged at its own depth)
    let pane_z = textureLoad(scene_depth, pixel, 0);
    let mask_z = view.clip_from_view[3][2] / GLASS_SKY_MASK_DISTANCE;
    if pane_z > z * 1.0001 && abs(pane_z - mask_z) > mask_z * 0.01 {
        z = pane_z;
    }
    var result = Fogged(vec4(0.0), 1.0);
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

// The tonemapper's response along the grey axis (Bevy 0.20's TonyMcMapface,
// the camera's, sampled by its LUT as its shader samples it), for the
// blend's second form: TONE_CURVE at u = x / (x + 1) in 63
// steps, TONE_INVERSE the u for a response in 63 steps up to TONE_MAX.
// Made by tmp/fb/tony.py from the LUT; within 0.003 of it.
const TONE_STEPS: f32 = 63.0;
const TONE_MAX: f32 = 0.998047;
// the most linear light the inverse gives (its display about 0.985): the
// curve's top is flat, and a white fog would ask for thousands
const TONE_INVERSE_CAP: f32 = 64.0;
var<private> TONE_CURVE: array<f32, 64> = array<f32, 64>(0.000137, 0.017894, 0.035537, 0.053229, 0.071143, 0.088571, 0.106132, 0.123951, 0.142217, 0.159205, 0.176699, 0.194626, 0.213016, 0.229819, 0.247230, 0.265318, 0.283392, 0.300650, 0.317816, 0.336015, 0.354124, 0.370937, 0.388221, 0.406681, 0.424808, 0.441271, 0.458455, 0.477351, 0.495358, 0.511293, 0.528513, 0.547867, 0.565567, 0.580169, 0.597254, 0.616102, 0.633706, 0.648604, 0.665246, 0.684012, 0.700954, 0.714487, 0.730710, 0.749884, 0.764771, 0.777520, 0.792672, 0.811565, 0.824803, 0.836224, 0.850483, 0.868666, 0.879309, 0.889580, 0.901987, 0.917692, 0.926783, 0.935894, 0.947527, 0.962643, 0.968540, 0.977005, 0.990908, 0.998047);
var<private> TONE_INVERSE: array<f32, 64> = array<f32, 64>(0.000000, 0.014039, 0.028280, 0.042503, 0.056603, 0.070839, 0.085223, 0.099480, 0.113531, 0.127322, 0.142124, 0.156519, 0.170599, 0.184374, 0.198763, 0.213470, 0.227700, 0.241605, 0.255591, 0.270163, 0.284812, 0.298680, 0.312552, 0.327130, 0.341848, 0.355938, 0.369643, 0.383774, 0.398955, 0.413508, 0.426815, 0.440693, 0.455986, 0.470912, 0.484486, 0.497985, 0.513095, 0.529371, 0.543676, 0.557121, 0.571405, 0.588178, 0.603275, 0.616675, 0.631262, 0.648921, 0.664732, 0.678144, 0.693770, 0.712715, 0.729560, 0.742988, 0.760684, 0.781566, 0.798003, 0.813468, 0.837518, 0.858161, 0.875015, 0.902645, 0.923779, 0.946520, 0.974190, 1.000000);

fn tone_table_curve(at: f32) -> f32 {
    let p = clamp(at, 0.0, 1.0) * TONE_STEPS;
    let i = min(u32(p), 62u);
    return mix(TONE_CURVE[i], TONE_CURVE[i + 1u], p - f32(i));
}

fn tone_table_inverse(at: f32) -> f32 {
    let p = clamp(at, 0.0, 1.0) * TONE_STEPS;
    let i = min(u32(p), 62u);
    return mix(TONE_INVERSE[i], TONE_INVERSE[i + 1u], p - f32(i));
}

// the tonemapper's response to linear light, per channel
fn tone_curve(light: vec3<f32>) -> vec3<f32> {
    let x = max(light, vec3(0.0));
    let u = x / (x + 1.0);
    return vec3(tone_table_curve(u.r), tone_table_curve(u.g), tone_table_curve(u.b));
}

// the linear light the tonemapper takes to a response, per channel (capped)
fn tone_inverse(response: vec3<f32>) -> vec3<f32> {
    let y = clamp(response, vec3(0.0), vec3(TONE_MAX)) / TONE_MAX;
    let u = vec3(tone_table_inverse(y.r), tone_table_inverse(y.g), tone_table_inverse(y.b));
    return min(u / max(1.0 - u, vec3(1e-6)), vec3(TONE_INVERSE_CAP));
}

// The game's fog blend (light f.rgb added and coverage f.a, in the display's
// encoding) over linear light, in linear light again for the tonemapper that
// follows: through its curve (form 2) or taking it as straight (form 1). The
// encoding is run on past 1 for light over it (form 1).
fn fog_blend_linear(light: vec3<f32>, f: vec4<f32>, form: f32) -> vec3<f32> {
    let p = max(light, vec3(0.0));
    if form > 1.5 {
        let shown = srgb_encode(min(tone_curve(p), vec3(1.0)));
        return tone_inverse(srgb_decode(f.rgb + shown * (1.0 - f.a)));
    }
    return srgb_decode(f.rgb + srgb_encode(p) * (1.0 - f.a));
}

// Applied to the picture as it is here, the lighting's before the denoiser
// (one noisy estimate a pixel), the blend g above is curved, so blended pixel
// by pixel and then averaged by the denoiser it comes out darker than the
// blend of the average, the old (3 to 5 of 255 in c10's mid fog). So it is
// taken at a local mean m and carried to the pixel affinely, as a ratio:
//   P' = g(0) + s P,   s = (g(m) - g(0)) / m,   while s <= 1;
//   P' = g(m) + (P - m)                         where it would be over 1
// (per channel; the second only where a fog's light lifts dark surfaces, the
// encoding's toe). The denoiser's average of P' is g of the average m, so the
// darkening goes; P' is continuous and monotone in P, never under g(0) (never
// negative), never steeper than 1, and the pixel's own value never passes
// through the tonemapper's inverse (a bright sample whose blended value neared
// the top of the curve ran there to tens of linear units, in one channel more
// than another). And a noisy sample keeps to its own pixel: taken about m with
// g's slope at m, as first built, a firefly in the window raised m and with it
// every pixel round it by the curve's offset g(m) - g'(m) m, a square of the
// window's size each frame (d20's first corridor in its black fog, pink, cyan
// and white, flickering); as a ratio from g(0) there is no offset, and a
// firefly moves its neighbours' ratio only by g's curvature. The mean itself
// counts the brightest sample of the window at no more than FOG_MEAN_CLIP times
// the others' mean, so that a firefly hardly moves it at all.
//
// m is the picture's local mean (HALO_RT_FOG_BEFORE_MEAN=<radius>,
// FOG_BEFORE_MEAN: the pixels within the radius at about this one's depth,
// from the copy bound as `source`), or the pixel itself without it; either
// held under FOG_MEAN_CAP, where the tables are well conditioned. With m the
// pixel itself and P under the cap it is g(P) exactly.
@if(FOG_BEFORE_MEAN) const FOG_MEAN_RADIUS: i32 = i32(constants::FOG_BEFORE_MEAN);
// (a neighbour counts where its view depth is within this share of the pixel's)
const FOG_MEAN_DEPTH: f32 = 0.05;
// (linear light: the tonemapper's response 0.93 there, well inside the tables)
const FOG_MEAN_CAP: f32 = 8.0;
// (the window's brightest sample counted at most this many times the mean of
// the rest, by luminance)
const FOG_MEAN_CLIP: f32 = 4.0;

// the picture's local mean round a pixel (the mean form), else the pixel
fn fog_local_mean(p: vec3<f32>, pixel: vec2<i32>, viewport: vec4<f32>) -> vec3<f32> {
    var mean = p;
@if(FOG_BEFORE_MEAN) {
    let near = view.clip_from_view[3][2];
    let z = textureLoad(depth, pixel, 0);
    let d = near / max(z, 1e-12);
    let low = vec2<i32>(viewport.xy);
    let high = vec2<i32>(viewport.xy + viewport.zw) - 1;
    var total = vec3(0.0);
    var count = 0.0;
    var brightest = vec3(0.0);
    for (var y = -FOG_MEAN_RADIUS; y <= FOG_MEAN_RADIUS; y++) {
        for (var x = -FOG_MEAN_RADIUS; x <= FOG_MEAN_RADIUS; x++) {
            let at = clamp(pixel + vec2(x, y), low, high);
            let zn = textureLoad(depth, at, 0);
            var same = false;
            if z <= 0.0 || zn <= 0.0 {
                same = z <= 0.0 && zn <= 0.0;
            } else {
                same = abs(near / zn - d) <= d * FOG_MEAN_DEPTH;
            }
            if same {
                let sample = max(textureLoad(source, at, 0).rgb, vec3(0.0));
                total += sample;
                count += 1.0;
                if luminance(sample) > luminance(brightest) {
                    brightest = sample;
                }
            }
        }
    }
    // (the brightest at most FOG_MEAN_CLIP times the others' mean)
    if count > 1.5 {
        let others = (total - brightest) / (count - 1.0);
        let limit = FOG_MEAN_CLIP * luminance(others);
        let bright = luminance(brightest);
        let kept = select(brightest, brightest * (limit / bright), bright > limit);
        total = total - brightest + kept;
    }
    mean = total / max(count, 1.0);
}
    return min(mean, vec3(FOG_MEAN_CAP));
}

fn luminance(c: vec3<f32>) -> f32 {
    return dot(c, vec3(0.2126, 0.7152, 0.0722));
}

// P' = g(m) + s (P - m), s = (g(m) - g(0)) / m held to 0..1, per channel:
// g(0) + s P while the secant is under 1 (above)
fn affine_about(p: vec3<f32>, m: vec3<f32>, at_mean: vec3<f32>, at_zero: vec3<f32>) -> vec3<f32> {
    let s = clamp((at_mean - at_zero) / max(m, vec3(1e-6)), vec3(0.0), vec3(1.0));
    return max(at_mean + (p - m) * s, vec3(0.0));
}

// lit fog's shoulder over linear light (the form's): 1 - exp(-light) screened
// over what is shown, through the tonemapper's curve and back (form 2), or
// over the light under 1 taken as shown (form 1)
fn lit_shoulder(light: vec3<f32>, lit: vec3<f32>, form: f32) -> vec3<f32> {
    let p = max(light, vec3(0.0));
    if form > 1.5 {
        return tone_inverse(lit_fog_over(min(tone_curve(p), vec3(1.0)), lit));
    }
    return p + lit_fog_shown(lit) * (1.0 - min(p, vec3(1.0)));
}

// The fog before the upscaler (src/fog.rs draw_fog_before; fog.lit.w its
// form): the lit picture, at the render resolution (the main pass's
// viewport), before the transparent pass, each pixel by its own fog_at, then
// lit fog's light laid over (fog.lit.x; y the light alone), both taken about
// the mean (above). A pixel with no coverage, no fog light and no lit fog's
// light is left as it was.
@compute @workgroup_size(8, 8, 1)
fn before(@builtin(global_invocation_id) id: vec3<u32>) {
    let form = fog.lit.w;
    let viewport = view.main_pass_viewport;
    if form <= 0.5 || any(vec2<f32>(id.xy) >= viewport.zw) {
        return;
    }
    let pixel = vec2<i32>(id.xy) + vec2<i32>(viewport.xy);
    let f = fog_at(pixel).fogged;
    // (a modelless sky's clear adds its colour with no coverage)
    let fogged = f.a > 0.0 || any(f.rgb > vec3(0.0));
    var lit = vec3(0.0);
    if fog.lit.x > 0.5 {
        let last = vec2<i32>(textureDimensions(lit_light)) - 1;
        lit = max(textureLoad(lit_light, min(pixel, last), 0).rgb, vec3(0.0));
    }
    let only = fog.lit.x > 0.5 && fog.lit.y > 0.5;
    let lit_on = fog.lit.x > 0.5 && !only && any(lit > vec3(0.0));
    if !fogged && !only && !lit_on {
        return;
    }
    let picture = textureLoad(fogged_picture, pixel);
    if only {
        textureStore(fogged_picture, pixel, vec4(lit, picture.a));
        return;
    }
    let p = max(picture.rgb, vec3(0.0));
    let m = fog_local_mean(p, pixel, viewport);
    var light = p;
    // (the mean after the fog, for the light's shoulder)
    var fogged_mean = m;
    if fogged {
        let at_mean = fog_blend_linear(m, f, form);
        light = affine_about(p, m, at_mean, fog_blend_linear(vec3(0.0), f, form));
        fogged_mean = at_mean;
    }
    if lit_on {
        if fog.lit.x > 1.5 {
            // (HALO_RT_FOG_BEFORE_LIT=add: linear light, the tonemapper
            // following)
            light += lit;
        } else {
            // (the shoulder, as the fog pass laid it over what was shown,
            // taken about the fogged mean as the fog is)
            let mf = min(fogged_mean, vec3(FOG_MEAN_CAP));
            let at_mean = lit_shoulder(mf, lit, form);
            light = affine_about(light, mf, at_mean, lit_shoulder(vec3(0.0), lit, form));
        }
    }
    textureStore(fogged_picture, pixel, vec4(light, picture.a));
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

    var encoded = srgb_encode(saturate(colour.rgb));
    var changed = false;
    let mode = fog.screen_color.a;
    if mode > 0.5 && screen_coverage > 0.0 {
        if mode < 1.5 {
            // Added, by the surface's density (the sky's: 1). The depth is at
            // the render resolution, jittered each frame, while the picture is
            // upscaled and steady: the density of the four render pixels round
            // where the jittered view saw this point, filtered by place (below
            // DLAA the depth texture is the picture's size, the view drawn in
            // its corner, the main pass's viewport)
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
            let screen_density = mix(
                mix(fog_at(p00).screen_density, fog_at(vec2(p11.x, p00.y)).screen_density, weight.x),
                mix(fog_at(vec2(p00.x, p11.y)).screen_density, fog_at(p11).screen_density, weight.x),
                weight.y,
            );
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
