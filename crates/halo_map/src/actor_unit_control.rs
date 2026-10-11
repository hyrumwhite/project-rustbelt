//! An actor's control of its unit (actors.c's `actor_unit_control`, last in
//! every tick of an actor's update that controls it): unless a player
//! drives the unit, the actor's output goes to the unit (thawing a frozen
//! one first), with its animation impulse and persistent control.
//!
//! The actor's datum and its unit are the game's memory, read in place
//! ([`crate::ai_actor`]); the players' input, the unit's control, its
//! animation and its persistent control are the game's, through
//! [`AiHost::call`] (actors.c's `actor_unit_control_call`, shared with the
//! C).

use crate::ai_actor::{AiCall, AiHost, AiMemoryExt, AiStepContext, NONE, object_get};
use crate::layout::{actor_datum as a, unit_datum as u};

/// The unit control's calls (halo_sim.h's `_actor_unit_control_call_*`).
pub mod call {
    /// `player_input_enabled()`, the result.
    pub const PLAYER_INPUT_ENABLED: i32 = 1;
    /// `unit_set_actively_controlled(unit, TRUE)`.
    pub const SET_ACTIVELY_CONTROLLED: i32 = 2;
    /// `unit_control(unit, ...)` with the actor's output.
    pub const CONTROL: i32 = 3;
    /// `unit_start_animation_impulse(unit, ...)` with the output's.
    pub const START_ANIMATION_IMPULSE: i32 = 4;
    /// `unit_persistent_control(unit, ...)` with the output's.
    pub const PERSISTENT_CONTROL: i32 = 5;
}

fn call_simple(context: &AiStepContext, host: &impl AiHost, kind: i32) -> i32 {
    let mut call = AiCall { kind, actor: context.actor_index, ..AiCall::default() };
    host.call(&mut call);
    call.result
}

/// `actor_unit_control`.
pub fn actor_unit_control(context: &AiStepContext, host: &impl AiHost) {
    let m = host.memory();
    let actor = context.actor;
    let unit = object_get(m, context, m.i32(actor + a::META_UNIT_INDEX as u32)).unwrap_or(0);

    if m.i32(unit + u::UNIT_PLAYER_INDEX as u32) == NONE
        || call_simple(context, host, call::PLAYER_INPUT_ENABLED) == 0
    {
        let frozen = actor + a::META_FROZEN as u32;
        if m.bool(frozen) {
            call_simple(context, host, call::SET_ACTIVELY_CONTROLLED);
            m.set_bool(frozen, false);
        }
        call_simple(context, host, call::CONTROL);
        if m.i16(actor + a::OUTPUT_ANIMATION_IMPULSE as u32) != NONE as i16 {
            call_simple(context, host, call::START_ANIMATION_IMPULSE);
        }
        if m.i16(actor + a::OUTPUT_PERSISTENT_CONTROL_TICKS as u32) > 0 {
            call_simple(context, host, call::PERSISTENT_CONTROL);
        }
    }
}
