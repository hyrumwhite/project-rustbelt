//! The AI's path search (`halo_map::path`) for the game: the search of a
//! path state in the game's memory and its estimated distance to a point
//! (path.c's `path_state_find` and `path_state_estimated_distance`), the
//! structure's 2D line and pill tests (path_structure_bsp.c's
//! `structure_test_line2d` and `structure_test_pill2d`), the smoothing
//! (path_smoothing.c's `path_smooth`) and the approach point (path.c's
//! `path_state_approach_point`), through halo_sim.c's `halo_sim_path_*`.

use halo_map::ai_actor::AiMemory;
use halo_map::path::{
    PathCollisionResult, PathContext, PathStep, Structure, path_smooth, path_state_approach_point, path_state_estimated_distance,
    path_state_find,
};

use crate::ai_steps::GameMemory;

/// halo_sim.c's `struct halo_sim_path_call`.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct PathCall {
    pub state: u32,
    pub breakable_surface_flags: u32,
    pub structure_bsp_index: i32,
    /// 0 the search, 1 the estimated distance, 2 the line test, 3 the pill
    /// test, 4 the smoothing, 5 the approach point.
    pub kind: i32,
    pub end_point: [f32; 3],
    pub end_surface_index: i32,
    /// Bit 0: the closest approach wanted; bit 1: the direction.
    pub flags: i32,
    pub result: i32,
    pub distance: f32,
    pub closest_approach: f32,
    pub direction: [f32; 3],
    pub structure: u32,
    pub ignore_broken_surfaces: i32,
    pub p0: [f32; 2],
    pub p0_surface_index: i32,
    pub p1: [f32; 2],
    pub p1_surface_index: i32,
    pub radius: f32,
    pub test_flags: u32,
    pub collision: PathCollisionResult,
    pub raw_steps: u32,
    pub raw_step_count: i32,
    pub smoothed_steps: u32,
    pub smoothed_step_count: i32,
    /// The smoothing's `steps_finish_path`; the approach point's straight
    /// line.
    pub steps_finish_path: i32,
}

const _: () = assert!(std::mem::size_of::<PathCall>() == 148);

fn read_step(m: &dyn AiMemory, address: u32) -> PathStep {
    PathStep { surface_index: m.r32(address) as i32, point: std::array::from_fn(|k| f32::from_bits(m.r32(address + 4 + 4 * k as u32))) }
}

fn write_step(m: &dyn AiMemory, address: u32, step: &PathStep) {
    m.w32(address, step.surface_index as u32);
    for (k, value) in step.point.iter().enumerate() {
        m.w32(address + 4 + 4 * k as u32, value.to_bits());
    }
}

/// A path call. Returns 0, or -1 for a call it doesn't know.
///
/// # Safety
/// `call` must be valid, and its state, structure and steps the game's.
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
        }
        2 => {
            let structure = Structure::new(&m, call.structure, call.breakable_surface_flags);
            let (blocked, result) =
                structure.test_line2d(call.ignore_broken_surfaces != 0, call.p0, call.p0_surface_index, call.p1, call.p1_surface_index);
            call.result = blocked as i32;
            call.collision = result;
        }
        3 => {
            let structure = Structure::new(&m, call.structure, call.breakable_surface_flags);
            let (blocked, result) = structure.test_pill2d(
                call.ignore_broken_surfaces != 0,
                call.p0,
                call.p0_surface_index,
                call.p1,
                call.p1_surface_index,
                call.radius,
                call.test_flags,
                call.collision,
            );
            call.result = blocked as i32;
            call.collision = result;
        }
        4 => {
            let raw_steps: Vec<PathStep> =
                (0..call.raw_step_count.max(0) as u32).map(|index| read_step(&m, call.raw_steps + 16 * index)).collect();
            let mut finish = call.steps_finish_path != 0;
            let smoothed = path_smooth(&m, call.state, call.breakable_surface_flags, &raw_steps, &mut finish);
            for (index, step) in smoothed.iter().enumerate() {
                write_step(&m, call.smoothed_steps + 16 * index as u32, step);
            }
            call.smoothed_step_count = smoothed.len() as i32;
            call.steps_finish_path = finish as i32;
        }
        5 => {
            match path_state_approach_point(&m, call.state, call.breakable_surface_flags, call.p0, call.p0_surface_index) {
                Some((straight_line, point)) => {
                    call.result = 1;
                    call.steps_finish_path = straight_line as i32;
                    call.collision.point = point;
                }
                None => call.result = 0,
            }
        }
        _ => return -1,
    }
    0
}
