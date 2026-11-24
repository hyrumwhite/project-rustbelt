// Lit fog's composite (lit_fog_composite.rs): the accumulated scattered
// light, at the render resolution (in the corner of its texture below
// DLAA), over the picture at the output's: added, or in its place
// (LIT_FOG_ONLY: alpha 1) to see it alone. Not drawn where the application
// lays the light over its own fog (SolarikRayFog::lit's w).

import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput;
import bevy_render::view::View;

@group(0) @binding(0) var history: texture_2d<f32>;
@group(0) @binding(1) var history_sampler: sampler;
@group(0) @binding(2) var<uniform> view: View;

@fragment
fn fragment(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let viewport = view.main_pass_viewport;
    // (kept half a texel inside the rendered corner, so the linear filter
    // never takes the texels past it)
    let texel = 1.0 / vec2<f32>(textureDimensions(history));
    let uv = clamp((viewport.xy + in.uv * viewport.zw) * texel, (viewport.xy + 0.5) * texel, (viewport.xy + viewport.zw - 0.5) * texel);
    let light = textureSampleLevel(history, history_sampler, uv, 0.0).rgb;
@if(LIT_FOG_ONLY) {
    return vec4(light, 1.0);
}
@if(!LIT_FOG_ONLY) {
    return vec4(light, 0.0);
}
}
