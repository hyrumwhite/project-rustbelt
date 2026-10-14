// The game's multiplying blends (src/overlay.rs): the bottom half of the
// overlay's texture holds half the factor they multiply the world by, in the
// display's encoding (0.5: nothing). The picture under this node is
// multiplied by it (the blend: destination times source).

import bevy_ui_render::ui_vertex_output::UiVertexOutput;

@group(1) @binding(0) var picture: texture_2d<f32>;
@group(1) @binding(1) var picture_sampler: sampler;

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

@fragment
fn fragment(in: UiVertexOutput) -> @location(0) vec4<f32> {
    // (the texture is sRGB: the sample is decoded, the game's factor isn't)
    let half_factor = srgb_encode(textureSample(picture, picture_sampler, vec2(in.uv.x, 0.5 + in.uv.y * 0.5)).rgb);
    // the game multiplies display-encoded colours; this blend multiplies
    // linear ones: the factor that does the same to them
    return vec4(srgb_decode(2.0 * half_factor), 1.0);
}
