// A glass or water pane's sky mask (src/material.rs, GlassSkyMask): its
// depth only, written at a fixed distance far beyond the level but in front
// of the sky dome, so that the sky's layers (drawn in the transparent pass,
// after Solarik has composited the pane over what was behind it) don't draw
// over the pane where only the sky is behind it, while the level's own
// transparent surfaces and effects behind it still do.

import bevy_pbr::render::{
    mesh_functions,
    skinning,
    forward_io::{Vertex, decompress_vertex},
    view_transformations::position_world_to_clip,
    mesh_view_bindings::view,
};

// metres: beyond any level, nearer than any sky part (100 m per sky unit)
const MASK_DISTANCE: f32 = 50000.0;

@vertex
fn vertex(vertex_in: Vertex) -> @builtin(position) vec4<f32> {
    let vertex = decompress_vertex(vertex_in, vertex_in.instance_index);
@if(SKINNED)
    let world_from_local = skinning::skin_model(vertex.joint_indices, vertex.joint_weights, vertex_in.instance_index);
@else
    let world_from_local = mesh_functions::get_world_from_local(vertex_in.instance_index);
    let world = mesh_functions::mesh_position_local_to_world(world_from_local, vec4(vertex.position, 1.0));
    var clip = position_world_to_clip(world.xyz);
    // (reverse Z, infinite: depth = near / distance)
    clip.z = view.clip_from_view[3][2] / MASK_DISTANCE * clip.w;
    return clip;
}

@fragment
fn fragment() -> @location(0) vec4<f32> {
    return vec4(0.0);
}
