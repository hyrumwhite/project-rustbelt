//! The reference path tracer's shader against its pipeline's layout: group 0
//! the scene's bindings, group 1 its own three (pathtracer/node.rs). The
//! shared modules it reaches (scene/sampling.wesl, the light triangles) also
//! serve the realtime passes, whose group 1 is another layout; a realtime
//! binding reached from the path tracer validated as WGSL here and failed
//! only at pipeline creation in the viewer, so the mode rotted unseen.
//!
//! What it catches: a global the `pathtrace` entry point uses (as naga's
//! analysis sees it, which is what wgpu matches against the layout) in a
//! group other than 0 and 1, in group 1 outside the path tracer's bindings,
//! in group 0 past the scene layout's entries, or a group 0 storage buffer
//! written to (the scene's are all read-only). What it does not: a binding
//! of the right number and the wrong type, and defs the pipeline sets that
//! this list lacks (it checks that each def named here is in node.rs, not
//! the reverse).
#[path = "support/shader_graph.rs"]
mod shader_graph;
use bevy_shader::ShaderDefVal;
use wgpu::naga;

// (the defs pathtracer/node.rs gives its pipeline, and those it adds from
// the environment)
const PIPELINE_DEFS: [&str; 4] = ["BINDLESS_SURFACE_DETAIL", "FOLIAGE_TRANSMISSION", "PATHTRACER", "UNSEEN_LIGHT_NEE"];
const OPTIONAL_DEFS: [&str; 2] = ["RAY_MATERIAL_FOOTPRINTS", "PATHTRACER_DEBUG_SAMPLE_COUNT"];
// (the path tracer's group 1: accumulation, output, view)
const GROUP_1_BINDINGS: [u32; 3] = [0, 1, 2];
// (raytracing_scene_bind_group_layout's entries, scene/binder.rs: bindings
// 0 to 28)
const SCENE_BINDING_COUNT: u32 = 29;

#[test]
fn pathtracer_shader_matches_its_layout() {
    let node = include_str!("../src/pathtracer/node.rs");
    for name in PIPELINE_DEFS.iter().chain(OPTIONAL_DEFS.iter()) {
        assert!(node.contains(&format!("\"{name}\".into()")), "pathtracer/node.rs no longer sets {name}");
    }
    let mut graph = shader_graph::Graph::load();
    for optional in 0..1 << OPTIONAL_DEFS.len() {
        let mut defs: Vec<ShaderDefVal> = PIPELINE_DEFS.iter().map(|name| (*name).into()).collect();
        for (index, name) in OPTIONAL_DEFS.iter().enumerate() {
            if optional & (1 << index) != 0 {
                defs.push((*name).into());
            }
        }
        graph.validate("bevy_solarik::pathtracer::pathtracer", &defs);
        let source = graph.compile("bevy_solarik::pathtracer::pathtracer", &defs).unwrap();
        let module = naga::front::wgsl::parse_str(&source).unwrap();
        let info = naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::all())
            .validate(&module)
            .unwrap();
        let entry = module
            .entry_points
            .iter()
            .position(|entry| entry.name == "pathtrace")
            .expect("the pathtrace entry point");
        let used = info.get_entry_point(entry);
        let mut misplaced = Vec::new();
        for (handle, global) in module.global_variables.iter() {
            let Some(binding) = &global.binding else { continue };
            if used[handle].is_empty() {
                continue;
            }
            let name = global.name.clone().unwrap_or_default();
            let fits = match binding.group {
                0 => {
                    binding.binding < SCENE_BINDING_COUNT
                        && !matches!(global.space, naga::AddressSpace::Storage { access } if access.contains(naga::StorageAccess::STORE))
                }
                1 => GROUP_1_BINDINGS.contains(&binding.binding),
                _ => false,
            };
            if !fits {
                misplaced.push(format!("{name} (group {}, binding {})", binding.group, binding.binding));
            }
        }
        assert!(
            misplaced.is_empty(),
            "the path tracer's shader with {:?} uses bindings outside its pipeline's layout: {}",
            defs,
            misplaced.join(", ")
        );
    }
}
