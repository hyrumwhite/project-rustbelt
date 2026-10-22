// The sky, for DLSS-RR's transparency overlay (src/sky_overlay.rs): where
// nothing in the level is behind the pixel (the depth prepass's and the main
// pass's depth both clear: the skybox, the sky model's layers and whatever
// transparent surface the game draws over them), the picture as it is, at an
// alpha of 1; elsewhere nothing. RR upscales the overlay without denoising
// it: the sky is drawn by the rasterizer, without noise, and as noisy light
// RR smoothed a starfield away.

import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput;

@group(0) @binding(0) var colour: texture_2d<f32>;
@group(0) @binding(1) var depth: texture_depth_2d;
// the main pass's: the glass's sky masks are in it (src/material.rs)
@group(0) @binding(2) var scene_depth: texture_depth_2d;

@fragment
fn fragment(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let pixel = vec2<i32>(in.position.xy);
    if textureLoad(depth, pixel, 0) > 0.0 || textureLoad(scene_depth, pixel, 0) > 0.0 {
        return vec4(0.0);
    }
    return vec4(textureLoad(colour, pixel, 0).rgb, 1.0);
}
