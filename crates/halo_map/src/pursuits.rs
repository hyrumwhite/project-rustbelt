//! The encounters' examined pursuit positions (encounters.c's
//! `encounter_find_pursuit`, `encounter_mark_examined_pursuit_position` and
//! `encounter_pursuit_position_already_examined`): each encounter keeps a
//! list of the firing positions its actors have searched, when they last
//! did and which of them did, forgotten when the target was seen again
//! since. The pursuits are the game's data array, worked on in place.

use crate::ai_actor::{AiMemory, AiMemoryExt, AiStepContext, NONE, datum_get};
use crate::layout::{actor_datum as a, encounter_datum as e};

/// encounters.c's `struct pursuit_datum` (local to it).
mod pursuit {
    pub const FIRING_POSITION_INDEX: u32 = 2;
    pub const LAST_EXAMINED_TIME: u32 = 4;
    pub const ACTOR_COUNT: u32 = 8;
    pub const NEXT_ACTOR_INDEX_INDEX: u32 = 10;
    pub const ACTOR_INDICES: u32 = 12;
    pub const NEXT_PURSUIT_INDEX: u32 = 36;
}

/// encounters.h's `NUMBER_OF_ACTOR_INDICES_PER_EXAMINED_PURSUIT_POSITION`.
const ACTOR_INDICES_PER_PURSUIT: i16 = 6;

fn encounter_get(m: &dyn AiMemory, context: &AiStepContext, encounter_index: i32) -> u32 {
    datum_get(m, context.encounter_data, encounter_index).unwrap_or(0)
}

fn pursuit_get(m: &dyn AiMemory, context: &AiStepContext, pursuit_index: i32) -> u32 {
    datum_get(m, context.pursuit_data, pursuit_index).unwrap_or(0)
}

/// `encounter_find_pursuit`: the encounter's pursuit of the firing
/// position, made if it has none and `force_create`; one examined before
/// `history_start_time` is reset (and then, unless made, none is found).
fn find_pursuit(
    m: &dyn AiMemory,
    context: &AiStepContext,
    encounter_index: i32,
    firing_position_index: i16,
    history_start_time: i32,
    force_create: bool,
) -> i32 {
    let encounter = encounter_get(m, context, encounter_index);
    let mut pursuit_index = m.i32(encounter + e::FIRST_PURSUIT_INDEX as u32);
    let mut reset = false;
    while pursuit_index != NONE {
        let pursuit = pursuit_get(m, context, pursuit_index);
        if m.i16(pursuit + pursuit::FIRING_POSITION_INDEX) == firing_position_index {
            if m.i32(pursuit + pursuit::LAST_EXAMINED_TIME) < history_start_time {
                reset = true;
            }
            break;
        }
        pursuit_index = m.i32(pursuit + pursuit::NEXT_PURSUIT_INDEX);
    }
    if pursuit_index == NONE && force_create {
        pursuit_index = crate::data_array::datum_new(m, context.pursuit_data);
        if pursuit_index != NONE {
            let pursuit = pursuit_get(m, context, pursuit_index);
            m.set_i16(pursuit + pursuit::FIRING_POSITION_INDEX, firing_position_index);
            m.w32(pursuit + pursuit::NEXT_PURSUIT_INDEX, m.i32(encounter + e::FIRST_PURSUIT_INDEX as u32) as u32);
            m.w32(encounter + e::FIRST_PURSUIT_INDEX as u32, pursuit_index as u32);
            reset = true;
        }
        // (the game's warning of too many pursuits is a silent error)
    }
    if reset {
        let pursuit = pursuit_get(m, context, pursuit_index);
        m.w32(pursuit + pursuit::LAST_EXAMINED_TIME, NONE as u32);
        m.set_i16(pursuit + pursuit::ACTOR_COUNT, 0);
        m.set_i16(pursuit + pursuit::NEXT_ACTOR_INDEX_INDEX, 0);
        for i in 0..ACTOR_INDICES_PER_PURSUIT as u32 {
            m.w32(pursuit + pursuit::ACTOR_INDICES + 4 * i, NONE as u32);
        }
        if !force_create {
            return NONE;
        }
    }
    pursuit_index
}

/// `encounter_mark_examined_pursuit_position` for the step's actor (of its
/// encounter): whether it is newly among the position's examiners.
pub fn mark_examined(m: &dyn AiMemory, context: &AiStepContext, firing_position_index: i16, history_start_time: i32) -> bool {
    let encounter_index = m.i32(context.actor + a::META_ENCOUNTER_INDEX as u32);
    let mut marked = false;
    let pursuit_index = find_pursuit(m, context, encounter_index, firing_position_index, history_start_time, true);
    if pursuit_index != NONE {
        let pursuit = pursuit_get(m, context, pursuit_index);
        let known = (0..ACTOR_INDICES_PER_PURSUIT as u32)
            .any(|i| m.i32(pursuit + pursuit::ACTOR_INDICES + 4 * i) == context.actor_index);
        if !known {
            let next = m.i16(pursuit + pursuit::NEXT_ACTOR_INDEX_INDEX);
            m.w32(pursuit + pursuit::ACTOR_INDICES + 4 * next as u32, context.actor_index as u32);
            m.set_i16(pursuit + pursuit::NEXT_ACTOR_INDEX_INDEX, (next + 1) % ACTOR_INDICES_PER_PURSUIT);
            m.set_i16(pursuit + pursuit::ACTOR_COUNT, m.i16(pursuit + pursuit::ACTOR_COUNT).wrapping_add(1));
            marked = true;
        }
        m.w32(pursuit + pursuit::LAST_EXAMINED_TIME, context.game_time as u32);
    }
    marked
}

/// `encounter_pursuit_position_already_examined` for the step's actor:
/// whether it (or more actors than a pursuit remembers) examined the
/// position since `start_time`, with how many examined it and when last.
pub fn already_examined(m: &dyn AiMemory, context: &AiStepContext, firing_position_index: i16, start_time: i32) -> (bool, i16, i32) {
    let encounter_index = m.i32(context.actor + a::META_ENCOUNTER_INDEX as u32);
    let pursuit_index = find_pursuit(m, context, encounter_index, firing_position_index, start_time, false);
    let mut last_examined_time = NONE;
    let mut examined = false;
    let mut actor_count = 0;
    if pursuit_index != NONE {
        let pursuit = pursuit_get(m, context, pursuit_index);
        actor_count = m.i16(pursuit + pursuit::ACTOR_COUNT);
        last_examined_time = m.i32(pursuit + pursuit::LAST_EXAMINED_TIME);
        examined = actor_count >= ACTOR_INDICES_PER_PURSUIT + 1
            || (0..ACTOR_INDICES_PER_PURSUIT as u32).any(|i| m.i32(pursuit + pursuit::ACTOR_INDICES + 4 * i) == context.actor_index);
    }
    (examined, actor_count, last_examined_time)
}
