// The skinned characters' ray-traced copies, skinned on the GPU
// (src/rt_skinning.rs): a thread a vertex reads the camera's skinned mesh in
// its vertex slab (the rest pose with its joints and weights, Bevy's
// interleaved layout, as 32-bit words) and writes the rays' copy in its own
// slab (positions, normals and tangents; its uvs and indices never change),
// blended between the vertex's nodes by their weights, as the CPU did
// (`TracedSkin::mesh`): the copy sits a little inside the camera's surface
// along its normal, so that rays leaving that surface don't meet it.

struct Job {
    // the first word of each mesh's first vertex in its slab, the vertices
    src_base: u32,
    dst_base: u32,
    count: u32,
    // the object's node matrices in `joints`: the first and how many
    joints_first: u32,
    joint_count: u32,
    // the source layout, in words: its stride and each attribute's offset
    src_stride: u32,
    src_pos: u32,
    src_nrm: u32,
    src_tan: u32,
    src_jnt: u32,
    src_wgt: u32,
    // the copy's layout
    dst_stride: u32,
    dst_pos: u32,
    dst_nrm: u32,
    dst_tan: u32,
    pad: u32,
}

@group(0) @binding(0) var<storage, read> src: array<u32>;
@group(0) @binding(1) var<storage, read_write> dst: array<u32>;
@group(0) @binding(2) var<storage, read> joints: array<mat4x4<f32>>;
// (the frame's jobs on this pair of slabs: a dispatch's y is the job, its x
// the vertex; one dispatch a pair of slabs rather than one a job, whose
// hundred dispatches a frame cost more than their vertices)
@group(0) @binding(3) var<storage, read> jobs: array<Job>;

// metres (`TRACED_SKIN_INSET`)
const INSET: f32 = 0.005;

fn read_vec3(base: u32) -> vec3<f32> {
    return vec3<f32>(bitcast<f32>(src[base]), bitcast<f32>(src[base + 1u]), bitcast<f32>(src[base + 2u]));
}

fn read_vec4(base: u32) -> vec4<f32> {
    return vec4<f32>(bitcast<f32>(src[base]), bitcast<f32>(src[base + 1u]), bitcast<f32>(src[base + 2u]), bitcast<f32>(src[base + 3u]));
}

fn write_vec3(base: u32, v: vec3<f32>) {
    dst[base] = bitcast<u32>(v.x);
    dst[base + 1u] = bitcast<u32>(v.y);
    dst[base + 2u] = bitcast<u32>(v.z);
}

fn write_vec4(base: u32, v: vec4<f32>) {
    dst[base] = bitcast<u32>(v.x);
    dst[base + 1u] = bitcast<u32>(v.y);
    dst[base + 2u] = bitcast<u32>(v.z);
    dst[base + 3u] = bitcast<u32>(v.w);
}

fn normalize_or_zero(v: vec3<f32>) -> vec3<f32> {
    let length = length(v);
    if length > 1e-12 {
        return v / length;
    }
    return vec3<f32>(0.0);
}

@compute @workgroup_size(64)
fn skin(@builtin(global_invocation_id) id: vec3<u32>) {
    let job = jobs[id.y];
    let i = id.x;
    if i >= job.count {
        return;
    }
    let s = job.src_base + i * job.src_stride;
    let p = read_vec3(s + job.src_pos);
    let n = read_vec3(s + job.src_nrm);
    let t = read_vec4(s + job.src_tan);
    // (Uint16x4: two words)
    let jw0 = src[s + job.src_jnt];
    let jw1 = src[s + job.src_jnt + 1u];
    let j = vec4<u32>(jw0 & 0xffffu, jw0 >> 16u, jw1 & 0xffffu, jw1 >> 16u);
    let w = read_vec4(s + job.src_wgt);
    var position = vec3<f32>(0.0);
    var normal = vec3<f32>(0.0);
    var tangent = vec3<f32>(0.0);
    for (var k = 0u; k < 4u; k += 1u) {
        if w[k] == 0.0 {
            continue;
        }
        // (a node past the object's: left where it is, as the CPU did)
        var m = mat4x4<f32>(vec4<f32>(1.0, 0.0, 0.0, 0.0), vec4<f32>(0.0, 1.0, 0.0, 0.0), vec4<f32>(0.0, 0.0, 1.0, 0.0), vec4<f32>(0.0, 0.0, 0.0, 1.0));
        if j[k] < job.joint_count {
            m = joints[job.joints_first + j[k]];
        }
        position += w[k] * (m * vec4<f32>(p, 1.0)).xyz;
        normal += w[k] * (m * vec4<f32>(n, 0.0)).xyz;
        tangent += w[k] * (m * vec4<f32>(t.xyz, 0.0)).xyz;
    }
    normal = normalize_or_zero(normal);
    tangent = normalize_or_zero(tangent);
    position -= normal * INSET;
    let d = job.dst_base + i * job.dst_stride;
    write_vec3(d + job.dst_pos, position);
    write_vec3(d + job.dst_nrm, normal);
    write_vec4(d + job.dst_tan, vec4<f32>(tangent, t.w));
}
