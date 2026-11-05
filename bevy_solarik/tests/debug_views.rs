//! The diagnostic views (`SOLARIK_SHADER_DEFS=VISUALIZE_*`) and the cell
//! lookup variants compose: they are compiled only when asked for, so a
//! slip in one shows nothing until a diagnosis needs it.
#[path = "support/shader_graph.rs"]
mod shader_graph;
use bevy_shader::ShaderDefVal;

#[test]
fn diagnostic_views_compose() {
    let mut graph = shader_graph::Graph::load();
    let base = vec![
        ShaderDefVal::UInt("WORLD_CACHE_SIZE".into(), 1 << 20),
        ShaderDefVal::UInt("RASTER_SUBPIXEL_BITS".into(), 8),
        "BINDLESS_SURFACE_DETAIL".into(),
        "DI_CELL_LIGHTS".into(),
        "DI_SLOT_VISIBILITY_PRIOR".into(),
    ];
    let consumers = [
        "realtime::restir_di",
        "realtime::restir_gi",
        "realtime::specular_gi",
        "realtime::world_cache_update",
        "realtime::primary_glass",
    ];
    let views = [
        "VISUALIZE_DI_PICK",
        "VISUALIZE_CACHE_AGE",
        "VISUALIZE_CELL_LEAK",
        "VISUALIZE_CELL_LIGHTS",
        "VISUALIZE_WORLD_CACHE",
        "VISUALIZE_CELL_KEY_PROBE",
        "VISUALIZE_NORMAL_KEY",
    ];
    for geometric in [false, true] {
        for unjittered in [false, true] {
            for view in views.iter().map(Some).chain(std::iter::once(None)) {
                let mut definitions = base.clone();
                if geometric {
                    definitions.push("CELL_LOOKUP_GEOMETRIC".into());
                    definitions.push("CELL_KEY_PLANE".into());
                }
                if unjittered {
                    definitions.push("CACHE_CELL_UNJITTERED".into());
                }
                if let Some(view) = view {
                    definitions.push((*view).into());
                }
                for consumer in consumers {
                    let mut stage_definitions = definitions.clone();
                    if consumer == "realtime::world_cache_update" {
                        stage_definitions.push("WORLD_CACHE_PRIORITY_ATOMIC_BUFFER".into());
                    }
                    graph.validate(&format!("bevy_solarik::{consumer}"), &stage_definitions);
                }
            }
        }
    }
}
