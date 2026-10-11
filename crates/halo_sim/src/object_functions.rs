//! An object's function values and change colours
//! (`object_compute_function_values`, `object_compute_change_colors`) in
//! Rust, on the running game: `halo_map::object_functions`', from the
//! object's values, its definition (the game's tags) and the game's
//! function tables. Neither calls the rest of the game. In check mode the
//! game's own runs first, and Rust's from the values it began with is
//! compared with what the game's left.

use std::sync::Arc;

use halo_map::object_functions::{
    FunctionTables, ObjectFunctionsDefinition, ObjectFunctionsState, object_compute_change_colors, object_compute_function_values,
};

use crate::TAGS;

/// What an object's function values (`part` 0) or change colours (1) read,
/// and where they leave them.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct ObjectFunctionsCallC {
    pub state: ObjectFunctionsState,
    pub part: u32,
    pub game_time: i32,
    /// The game's periodic functions' tables (12) and transition functions'
    /// (6), 1024 bytes each, or null before it builds them.
    pub periodic_tables: [*const u8; 12],
    pub transition_tables: [*const u8; 6],
}

impl crate::Tags {
    fn object_functions_definition(&mut self, datum: u32) -> Option<Arc<ObjectFunctionsDefinition>> {
        let cache = &self.cache;
        self.object_functions_definitions
            .entry(datum)
            .or_insert_with(|| ObjectFunctionsDefinition::load(cache, datum).ok().map(Arc::new))
            .clone()
    }
}

/// # Safety
/// `pointer` must be null or point to 1024 bytes.
unsafe fn table<'a>(pointer: *const u8) -> Option<&'a [u8; 1024]> {
    // SAFETY: as above
    (!pointer.is_null()).then(|| unsafe { &*(pointer as *const [u8; 1024]) })
}

/// An object's function values or change colours, computed in place.
/// Returns 0, or -1 if the tags aren't there (nothing was done).
///
/// # Safety
/// `call` must be valid; its tables hold 1024 bytes each.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn halo_sim_object_functions(call: *mut ObjectFunctionsCallC) -> i32 {
    if call.is_null() {
        return -1;
    }
    // SAFETY: the caller's call
    let call = unsafe { &mut *call };
    let definition = {
        let mut tags = TAGS.lock().unwrap_or_else(|e| e.into_inner());
        let Some(tags) = tags.as_mut() else { return -1 };
        tags.object_functions_definition(call.state.definition_index)
    };
    let Some(definition) = definition else { return -1 };
    let mut tables = FunctionTables::default();
    for (slot, &pointer) in tables.periodic.iter_mut().zip(call.periodic_tables.iter()) {
        // SAFETY: the game's tables
        *slot = unsafe { table(pointer) };
    }
    for (slot, &pointer) in tables.transition.iter_mut().zip(call.transition_tables.iter()) {
        // SAFETY: as above
        *slot = unsafe { table(pointer) };
    }
    if call.part == 0 {
        object_compute_function_values(&definition, &tables, call.game_time, &mut call.state);
    } else {
        object_compute_change_colors(&definition, &mut call.state);
    }
    0
}
