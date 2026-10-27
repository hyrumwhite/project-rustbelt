// The game's screen effect (src/screen_effect.rs), over the tonemapped and
// fogged picture as the game draws it over its world, in the display's
// encoding.
//
// It is rasterizer_xbox_screen_effect.c's last pass, whose register
// combiners, decoded, are, for the filter (a script's cinematic filter, or
// a HUD's: night vision's light enhancement),
//   enhanced = mix(c, 1 - (1 - c)^4, light enhancement)
//   grey     = (enhanced.r + enhanced.g + enhanced.b) / 3
//   result   = mix(enhanced, tint * grey, desaturation), or
//              enhanced + desaturation * tint * grey when additive
// and for the video effect (a helmet recording),
//   m        = ((1 - noise) + noise * N.rgb) * S.rgb
//   coverage = ((1 - noise) + noise * N.a) * S.a
//   result   = mix(saturate(c^4 k), saturate(c k), m) * (1 - coverage)
// where N is the video noise map and S the scanline map (rasterizer\video
// noise and rasterizer\video mask), each a texel to a game pixel, centred on
// the game's view (the mask's 640 by 480 window, dark around it), the noise
// moved on each frame, and k the overbright mode's scale (1, 2 or 4).
//
// A HUD's zoom (the pistol's, the sniper's) comes first: its warp, in each
// of the game's two passes,
//   c' = mix(c, (c + c_in + c_out) / 3, M.a)
// where c_in and c_out are the picture scaled about its centre by
// 1 -+ radius / view (the radius in game pixels), or, with no mask, the
// average of the picture scaled by 1 + r, 1, 1 - r and 1 + 2r; then the
// filter (its light enhancement or desaturation scaled by 1 - 2 M.b where
// the HUD says they fade with the mask); then the last pass's blend darkens
// it by 1 - M.b. M is the HUD's convolution mask, a texel to a game pixel,
// centred on the view.

import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput;

const XBOX_VIEW_WIDTH: f32 = 640.0;

struct ScreenEffect {
    // light enhancement, desaturation, 1 if the desaturation is added, 1 for
    // the video effect
    filtering: vec4<f32>,
    // the desaturation's tint, and the video's overbright scale
    tint: vec4<f32>,
    // the video's noise intensity, the noise map's offset (texels), the
    // game's view's height (pixels)
    video: vec4<f32>,
    // a HUD's zoom: 1 with its mask, its warp's radius (game pixels, 0
    // none), 1 if the light enhancement and 1 if the desaturation fade with
    // the mask
    zoom: vec4<f32>,
}

@group(0) @binding(0) var picture: texture_2d<f32>;
@group(0) @binding(1) var picture_sampler: sampler;
@group(0) @binding(2) var<uniform> effect: ScreenEffect;
@group(0) @binding(3) var scanline_map: texture_2d<f32>;
@group(0) @binding(4) var noise_map: texture_2d<f32>;
@group(0) @binding(5) var map_sampler: sampler;
@group(0) @binding(6) var mask_map: texture_2d<f32>;
@group(0) @binding(7) var linear_sampler: sampler;

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

// A map's texture coordinates at a game pixel: a texel to a pixel, the map
// centred on the view (the game's (size + 1 - view) / 2 offset).
fn map_uv(pixel: vec2<f32>, view: vec2<f32>, size: vec2<f32>, offset: vec2<f32>) -> vec2<f32> {
    return (pixel + (size + 1.0 - view) * 0.5 + offset) / size;
}

// The game's view in game pixels (480 high, as wide as the window's shape).
fn game_view() -> vec2<f32> {
    let output_size = vec2<f32>(textureDimensions(picture));
    return vec2(effect.video.w * output_size.x / output_size.y, effect.video.w);
}

// The zoom mask at a point of the picture.
fn mask(uv: vec2<f32>) -> vec4<f32> {
    let view = game_view();
    return textureSampleLevel(mask_map, linear_sampler, map_uv(uv * view, view, vec2<f32>(textureDimensions(mask_map)), vec2(0.0)), 0.0);
}

// The picture, filtered, in the display's encoding.
fn tap(uv: vec2<f32>) -> vec3<f32> {
    return srgb_encode(saturate(textureSampleLevel(picture, linear_sampler, uv, 0.0).rgb));
}

// The picture scaled about its centre by 1 + k * radius / view.
fn scaled(uv: vec2<f32>, k: f32) -> vec2<f32> {
    return 0.5 + (uv - 0.5) * (1.0 + k * effect.zoom.y / game_view());
}

// The warp's first pass.
fn warp_first(uv: vec2<f32>) -> vec3<f32> {
    let c = tap(uv);
    if effect.zoom.x > 0.5 {
        return mix(c, (c + tap(scaled(uv, -1.0)) + tap(scaled(uv, 1.0))) / 3.0, mask(uv).a);
    }
    return (tap(scaled(uv, 1.0)) + c + tap(scaled(uv, -1.0)) + tap(scaled(uv, 2.0))) * 0.25;
}

// The warp's second pass, of the first's.
fn warp(uv: vec2<f32>) -> vec3<f32> {
    let c = warp_first(uv);
    if effect.zoom.x > 0.5 {
        return saturate(mix(c, (c + warp_first(scaled(uv, -1.0)) + warp_first(scaled(uv, 1.0))) / 3.0, mask(uv).a));
    }
    return saturate((warp_first(scaled(uv, 1.0)) + c + warp_first(scaled(uv, -1.0)) + warp_first(scaled(uv, 2.0))) * 0.25);
}

@fragment
fn fragment(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let colour = textureSample(picture, picture_sampler, in.uv);
    var c = srgb_encode(saturate(colour.rgb));
    if effect.zoom.y > 0.0 {
        c = warp(in.uv);
    }
    let m = select(vec4(0.0), mask(in.uv), effect.zoom.x > 0.5);
    let fade = saturate(1.0 - 2.0 * m.b);
    let light_enhancement = effect.filtering.x * select(1.0, fade, effect.zoom.z > 0.5);
    let desaturation = effect.filtering.y * select(1.0, fade, effect.zoom.w > 0.5);
    var result: vec3<f32>;
    if effect.filtering.w > 0.5 {
        let output_size = vec2<f32>(textureDimensions(picture));
        let view = vec2(effect.video.w * output_size.x / output_size.y, effect.video.w);
        let pixel = in.uv * view;
        // (the mask's window is the Xbox's 640 by 480: widened to the view's
        // width, where it left bars of black either side)
        let window = vec2(XBOX_VIEW_WIDTH, view.y);
        let scanline = textureSample(scanline_map, map_sampler, map_uv(pixel * window / view, window, vec2<f32>(textureDimensions(scanline_map)), vec2(0.0)));
        let noise = textureSample(noise_map, map_sampler, map_uv(pixel, view, vec2<f32>(textureDimensions(noise_map)), effect.video.yz));
        let intensity = effect.video.x;
        let m = ((1.0 - intensity) + intensity * noise.rgb) * scanline.rgb;
        let coverage = ((1.0 - intensity) + intensity * noise.a) * scanline.a;
        let k = effect.tint.w;
        let squared = c * c;
        result = mix(saturate(squared * squared * k), saturate(c * k), m) * (1.0 - coverage);
    } else {
        let inverse = 1.0 - c;
        let squared = inverse * inverse;
        let enhanced = mix(c, 1.0 - squared * squared, light_enhancement);
        let grey = (enhanced.r + enhanced.g + enhanced.b) / 3.0;
        if effect.filtering.z > 0.5 {
            result = enhanced + desaturation * effect.tint.rgb * grey;
        } else {
            result = mix(enhanced, effect.tint.rgb * grey, desaturation);
        }
        // (the zoom's darkened edge)
        result = saturate(result) * (1.0 - m.b);
    }
    return vec4(srgb_decode(saturate(result)), colour.a);
}
