// The game's screen flash (src/hud.rs): one of its layers over the
// picture, the HUD included, a colour that its blend multiplies the picture
// by, adds to it or takes from it.

import bevy_ui_render::ui_vertex_output::UiVertexOutput;

@group(1) @binding(0) var<uniform> colour: vec4<f32>;

@fragment
fn fragment(in: UiVertexOutput) -> @location(0) vec4<f32> {
    return colour;
}
