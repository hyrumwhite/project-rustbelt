//! The AI's path search (`halo_map::path`) for the game: the search of a
//! path state in the game's memory, and its estimated distance to a point
//! (path.c's `path_state_find` and `path_state_estimated_distance`, through
//! halo_sim.c's `halo_sim_path_find` and `halo_sim_path_estimated_distance`).

use halo_map::path::{PathContext, path_state_estimated_distance, path_state_find};

use crate::ai_steps::GameMemory;

/// halo_sim.c's `struct halo_sim_path_call`.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct PathCall {
    pub state: u32,
    pub breakable_surface_flags: u32,
    pub structure_bsp_index: i32,
    /// 0 the search, 1 the estimated distance.
    pub kind: i32,
    pub end_point: [f32; 3],
    pub end_surface_index: i32,
    /// Bit 0: the closest approach wanted; bit 1: the direction.
    pub flags: i32,
    pub result: i32,
    pub distance: f32,
    pub closest_approach: f32,
    pub direction: [f32; 3],
}

const _: () = assert!(std::mem::size_of::<PathCall>() == 60);

/// A path search's call. Returns 0, or -1 for a call it doesn't know.
///
/// # Safety
/// `call` must be valid, and its state the game's.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn halo_sim_path(call: *mut PathCall) -> i32 {
    if call.is_null() {
        return -1;
    }
    // SAFETY: the caller's call
    let call = unsafe { &mut *call };
    let m = GameMemory;
    match call.kind {
        0 => {
            let context = PathContext {
                breakable_surface_flags: call.breakable_surface_flags,
                structure_bsp_index: call.structure_bsp_index as i16,
            };
            call.result = path_state_find(&m, call.state, &context) as i32;
            0
        }
        1 => {
            let (found, distance, closest_approach, direction) = path_state_estimated_distance(
                &m,
                call.state,
                call.end_point,
                call.end_surface_index,
                call.flags & 1 != 0,
                call.flags & 2 != 0,
            );
            call.result = found as i32;
            call.distance = distance;
            call.closest_approach = closest_approach;
            call.direction = direction;
            0
        }
        _ => -1,
    }
}
