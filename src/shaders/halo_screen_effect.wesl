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
}

@group(0) @binding(0) var picture: texture_2d<f32>;
@group(0) @binding(1) var picture_sampler: sampler;
@group(0) @binding(2) var<uniform> effect: ScreenEffect;
@group(0) @binding(3) var scanline_map: texture_2d<f32>;
@group(0) @binding(4) var noise_map: texture_2d<f32>;
@group(0) @binding(5) var map_sampler: sampler;

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

@fragment
fn fragment(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let colour = textureSample(picture, picture_sampler, in.uv);
    let c = srgb_encode(saturate(colour.rgb));
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
        let enhanced = mix(c, 1.0 - squared * squared, effect.filtering.x);
        let grey = (enhanced.r + enhanced.g + enhanced.b) / 3.0;
        if effect.filtering.z > 0.5 {
            result = enhanced + effect.filtering.y * effect.tint.rgb * grey;
        } else {
            result = mix(enhanced, effect.tint.rgb * grey, effect.filtering.y);
        }
    }
    return vec4(srgb_decode(saturate(result)), colour.a);
}
