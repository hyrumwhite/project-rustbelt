// The emissive triangles in world space, a table rebuilt every frame
// (`build_light_triangles`, a workgroup a light source), and a light
// sample's resolve from it (`resolve_light_triangle`, for
// bevy_solarik::scene::sampling::resolve_light_sample). A light sample needs
// a point on a triangle, the face's normal and area, the uv there and the
// material's emission; the full hit resolve it took them from also made the
// previous frame's position, the tangent frame, the normal map, the surface
// detail, the ripples and the whole material, from three indexed vertices
// fetched through the bindless slabs and transformed, a chain of a dozen
// dependent reads. At 16 cell candidates a pixel, and four re-resolves a
// pixel in the reuse, that was 0.9 ms of the 2.6 ms direct lighting pass at
// b40's shaft; a record here is one 64-byte read. The point and its values
// are the ones the full resolve gave, so the sampling is unchanged. A light
// past the table's capacity (prepare.rs LIGHT_TRIANGLE_TABLE_CAPACITY, its
// offset NO_LIGHT_TRIANGLES) resolves from its vertices as before.

import bevy_solarik::scene::raytracing_scene_bindings::LightSource;
import bevy_solarik::scene::raytracing_scene_bindings::light_sources;
import bevy_solarik::scene::raytracing_scene_bindings::materials;
import bevy_solarik::scene::raytracing_scene_bindings::material_ids;
import bevy_solarik::scene::raytracing_scene_bindings::transforms;
import bevy_solarik::scene::raytracing_scene_bindings::geometry_ids;
import bevy_solarik::scene::raytracing_scene_bindings::load_vertices;
import bevy_solarik::scene::raytracing_scene_bindings::transform_positions;
import bevy_solarik::scene::raytracing_scene_bindings::sample_texture_filtered;
import bevy_solarik::scene::raytracing_scene_bindings::TEXTURE_MAP_NONE;

// A triangle: its three world positions and uvs, and its material
// (`a` = p0, uv0.x; `b` = p1, uv0.y; `c` = p2, uv1.x; `d` = uv1.y, uv2, the
// material id's bits).
struct LightTriangleRecord {
    a: vec4<f32>,
    b: vec4<f32>,
    c: vec4<f32>,
    d: vec4<f32>,
}

@group(1) @binding(30) var<storage, read_write> light_triangles: array<LightTriangleRecord>;

const NO_LIGHT_TRIANGLES: u32 = 0xFFFFFFFFu;

struct LightTriangle {
    world_position: vec3<f32>,
    normal: vec3<f32>,
    area: f32,
    uv: vec2<f32>,
    emissive: vec3<f32>,
}

fn resolve_light_triangle(light_source: LightSource, triangle_id: u32, barycentrics: vec3<f32>) -> LightTriangle {
    var positions: array<vec3<f32>, 3>;
    var uvs: array<vec2<f32>, 3>;
    var material_id: u32;
    if light_source.triangle_offset != NO_LIGHT_TRIANGLES {
        let record = light_triangles[light_source.triangle_offset + triangle_id];
        positions = array<vec3<f32>, 3>(record.a.xyz, record.b.xyz, record.c.xyz);
        uvs = array<vec2<f32>, 3>(vec2(record.a.w, record.b.w), vec2(record.c.w, record.d.x), record.d.yz);
        material_id = bitcast<u32>(record.d.w);
    } else {
        material_id = material_ids[light_source.id];
        let vertices = load_vertices(geometry_ids[light_source.id], triangle_id);
        positions = transform_positions(transforms[light_source.id], vertices);
        uvs = array<vec2<f32>, 3>(vertices[0].uv, vertices[1].uv, vertices[2].uv);
    }
    let material = materials[material_id];
    let world_position = mat3x3(positions[0], positions[1], positions[2]) * barycentrics;
    let uv = mat3x2(uvs[0], uvs[1], uvs[2]) * barycentrics;
    let face = cross(positions[0] - positions[1], positions[0] - positions[2]);
    var emissive = material.emissive.rgb;
    if material.emissive_texture_id != TEXTURE_MAP_NONE {
        emissive *= sample_texture_filtered(material.emissive_texture_id, uv, vec2f(0.0));
    }
    // An exit aperture emits from the face toward its optical axis only.
    if material.emission_cone.w > 0.0 && dot(face, material.emission_cone.xyz) <= 0.0 {
        emissive = vec3(0.0);
    }
    return LightTriangle(world_position, normalize(face), length(face) / 2.0, uv, emissive);
}

@compute @workgroup_size(64, 1, 1)
fn build_light_triangles(@builtin(workgroup_id) workgroup_id: vec3<u32>, @builtin(local_invocation_index) thread: u32) {
    if workgroup_id.x >= arrayLength(&light_sources) { return; }
    let light = light_sources[workgroup_id.x];
    if (light.kind & 1u) != 0u || light.triangle_offset == NO_LIGHT_TRIANGLES { return; }
    let count = light.kind >> 1u;
    let material_id = bitcast<f32>(material_ids[light.id]);
    let transform = transforms[light.id];
    let ids = geometry_ids[light.id];
    for (var i = thread; i < count; i += 64u) {
        let v = load_vertices(ids, i);
        let p = transform_positions(transform, v);
        light_triangles[light.triangle_offset + i] = LightTriangleRecord(
            vec4(p[0], v[0].uv.x),
            vec4(p[1], v[0].uv.y),
            vec4(p[2], v[1].uv.x),
            vec4(v[1].uv.y, v[2].uv, material_id),
        );
    }
}
