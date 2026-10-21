// The game's overlay (src/overlay.rs): its picture without the opaque world,
// premultiplied by alpha, drawn over the ray-traced picture.

import bevy_ui_render::ui_vertex_output::UiVertexOutput;

@group(1) @binding(0) var picture: texture_2d<f32>;
@group(1) @binding(1) var picture_sampler: sampler;
@group(1) @binding(2) var<uniform> mode: u32;

@fragment
fn fragment(in: UiVertexOutput) -> @location(0) vec4<f32> {
    let colour = textureSample(picture, picture_sampler, in.uv);
    if mode == 1u {
        return vec4(0.5 * colour.a, 0.0, 0.0, 0.5 * colour.a);
    }
    if mode == 2u {
        return colour * 0.5;
    }
    return colour;
}
