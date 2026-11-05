//! The WGSL compiles and validates under naga (the workspace's, from wgpu
//! 30), and NrcConstants has the C layout.

fn validate(source: &str) -> naga::Module {
    let module = match naga::front::wgsl::parse_str(source) {
        Ok(m) => m,
        Err(e) => panic!("{}", e.emit_to_string(source)),
    };
    let mut validator = naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(), // pack2x16float/unpack2x16float (f32tof16/f16tof32) need this; wgpu grants
        // it on every Vulkan adapter (DownlevelFlags::SHADER_F16_IN_F32).
        naga::valid::Capabilities::SHADER_FLOAT16_IN_FLOAT32,
    );
    if let Err(e) = validator.validate(&module) {
        panic!("{}", e.emit_to_string(source));
    }
    module
}

/// Declarations nrc_buffers.wgsl expects from the including shader, and an
/// entry point so nothing is dead.
const BUFFER_DECLARATIONS: &str = "
@group(0) @binding(0) var<storage, read_write> nrc_query_path_info: array<u32>;
@group(0) @binding(1) var<storage, read_write> nrc_training_path_info: array<u32>;
@group(0) @binding(2) var<storage, read_write> nrc_training_path_vertices: array<u32>;
@group(0) @binding(3) var<storage, read_write> nrc_query_radiance_params: array<u32>;
@group(0) @binding(4) var<storage, read_write> nrc_counters: array<atomic<u32>>;
@group(0) @binding(5) var<storage, read> nrc_query_radiance: array<f32>;
@group(0) @binding(6) var<uniform> constants: NrcConstants;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    let v = nrc_read_packed_path_vertex(id.x);
    nrc_write_packed_path_vertex(id.x, nrc_update_training_path_vertex(v, vec3(1.0), vec3(0.5)));
    let index = nrc_increment_counter(NRC_COUNTER_QUERIES);
    nrc_write_radiance_params(index, nrc_create_radiance_params(nrc_unpack_path_vertex(v)));
    let q = nrc_unpack_query_path_info(nrc_read_query_path_info(id.x));
    nrc_write_query_path_info(id.x, NrcPackedQueryPathInfo(nrc_encode_log_luv_hdr(q.prefix_throughput * nrc_unpack_query_radiance(constants, nrc_read_query_radiance(q.query_buffer_index))), index));
    nrc_write_training_path_info(id.x, nrc_read_training_path_info(id.x));
    let p = nrc_read_radiance_params(id.x);
    _ = p;
}
";

#[test]
fn records_wgsl_validates_alone() {
    validate(nrc_records::WGSL);
}

#[test]
fn buffers_wgsl_validates_with_the_declarations() {
    validate(&format!(
        "{}\n{}\n{}",
        nrc_records::WGSL,
        nrc_records::WGSL_BUFFERS,
        BUFFER_DECLARATIONS
    ));
}

#[test]
fn nrc_constants_wgsl_layout_matches_c() {
    let module = validate(nrc_records::WGSL);
    let (_, ty) = module
        .types
        .iter()
        .find(|(_, t)| t.name.as_deref() == Some("NrcConstants"))
        .expect("NrcConstants");
    let naga::TypeInner::Struct { members, span } = &ty.inner else {
        panic!("not a struct")
    };
    let offsets: Vec<(String, u32)> = members
        .iter()
        .map(|m| (m.name.clone().unwrap(), m.offset))
        .collect();
    let expected = [
        ("frame_dimensions", 0),
        ("training_dimensions", 8),
        ("scene_pos_scale", 16),
        ("samples_per_pixel", 28),
        ("scene_pos_bias", 32),
        ("max_path_vertices", 44),
        ("learn_irradiance", 48),
        ("radiance_cache_direct", 52),
        ("radiance_unpack_multiplier", 56),
        ("resolve_mode", 60),
        ("enable_termination_heuristic", 64),
        ("skip_delta_vertices", 68),
        ("termination_heuristic_threshold", 72),
        ("training_termination_heuristic_threshold", 76),
        ("proportion_unbiased", 80),
        ("pad0", 84),
        ("pad1", 88),
        ("pad2", 92),
    ];
    assert_eq!(offsets, expected.map(|(n, o)| (n.to_string(), o)).to_vec());
    assert_eq!(*span, 96);
    assert_eq!(size_of::<nrc_records::NrcConstants>(), 96);
}
