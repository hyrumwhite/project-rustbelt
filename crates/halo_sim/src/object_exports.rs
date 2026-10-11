//! Each object type's export of its function values
//! (`object_type_export_function_values`) in Rust, on the running game:
//! `halo_map::object_exports`', from the object's state as the game hands
//! it over, its definitions (the game's tags) and the random seed. It calls
//! nothing of the rest of the game. In check mode the game's own runs first,
//! and Rust's from the state it began with is compared with what the game's
//! left.

use std::sync::Arc;

use halo_map::object_exports::{ExportDefinition, ExportState, object_type_export_function_values};

use crate::TAGS;

impl crate::Tags {
    fn export_definition(&mut self, datum: u32, object_type: i16) -> Option<Arc<ExportDefinition>> {
        let cache = &self.cache;
        self.export_definitions
            .entry((datum, object_type))
            .or_insert_with(|| ExportDefinition::load(cache, datum, object_type).ok().map(Arc::new))
            .clone()
    }
}

/// An object's export of its function values, computed in place (its
/// state, the random seed in and out). Returns 0, or -1 if the tags aren't
/// there (nothing was done).
///
/// # Safety
/// `state` must be valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn halo_sim_object_exports(state: *mut ExportState) -> i32 {
    if state.is_null() {
        return -1;
    }
    // SAFETY: the caller's state
    let state = unsafe { &mut *state };
    let (definition, graph) = {
        let mut tags = TAGS.lock().unwrap_or_else(|e| e.into_inner());
        let Some(tags) = tags.as_mut() else { return -1 };
        let definition = tags.export_definition(state.definition_index, state.object_type);
        let unit = state.object_type == 0 || state.object_type == 1;
        let graph = if unit && state.animation_graph_index != u32::MAX { tags.animation_graph(state.animation_graph_index) } else { None };
        (definition, graph)
    };
    let Some(definition) = definition else { return -1 };
    object_type_export_function_values(&definition, graph.as_deref(), state);
    0
}
