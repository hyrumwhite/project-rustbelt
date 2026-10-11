//! An actor's upkeep in its update (actors.c): whether it gets this tick's
//! timeslice of thinking time (`actor_get_timeslice`), and the clearing of
//! its orders before its decision loop (`actor_clear_orders`) and of its
//! output before its control (`actor_clear_output`).
//!
//! The actor's datum is the game's memory, worked on in place
//! ([`crate::ai_actor`]); the AI's globals (the service timers) are the
//! game's, through [`AiHost::call`] (actors.c's `actor_upkeep_call`, shared
//! with the C).

use crate::ai_actor::{AiCall, AiHost, AiMemoryExt, AiStepContext, NONE};
use crate::layout::{actor_datum as a, actor_orders, ai_values as v};

/// The upkeep's calls (halo_sim.h's `_actor_upkeep_call_*`).
pub mod call {
    /// `ai_globals->time_given_this_frame`, the result.
    pub const TIME_GIVEN: i32 = 1;
    /// `ai_globals->last_highest_service_timer`, the result.
    pub const LAST_HIGHEST_SERVICE_TIMER: i32 = 2;
    /// `ai_globals->current_highest_service_timer`, the result.
    pub const CURRENT_HIGHEST_SERVICE_TIMER: i32 = 3;
    /// `ai_globals->time_given_this_frame = TRUE`.
    pub const GIVE_TIME: i32 = 4;
    /// `ai_globals->current_highest_service_timer = a`.
    pub const SET_CURRENT_HIGHEST_SERVICE_TIMER: i32 = 5;
}

fn call_simple(context: &AiStepContext, host: &impl AiHost, kind: i32, a: i32) -> i32 {
    let mut call = AiCall { kind, actor: context.actor_index, a, ..AiCall::default() };
    host.call(&mut call);
    call.result
}

/// `actor_get_timeslice`: whether the actor thinks this tick.
pub fn actor_get_timeslice(context: &AiStepContext, host: &impl AiHost) -> bool {
    let m = host.memory();
    let actor = context.actor;
    let service_timer = actor + a::META_SERVICE_TIMER as u32;
    let mut jumping = false;
    let mut result = false;

    if m.i16(actor + a::STATE_ACTION as u32) == v::_ACTOR_ACTION_CHARGE as i16 {
        let goal = m.i16(actor + a::STATE_CHARGE_GOAL as u32) as usize;
        jumping = matches!(
            goal,
            v::_CHARGE_GOAL_MELEE
                | v::_CHARGE_GOAL_MELEE_LEAPING
                | v::_CHARGE_GOAL_VEHICLE_STRAFING
                | v::_CHARGE_GOAL_VEHICLE_RAMMING
        );
    }
    m.set_i16(service_timer, m.i16(service_timer).wrapping_add(if jumping { 3 } else { 1 }));

    let timer = m.i16(service_timer) as i32;
    if call_simple(context, host, call::TIME_GIVEN, 0) == 0
        && timer > call_simple(context, host, call::LAST_HIGHEST_SERVICE_TIMER, 0)
        && timer > 15
    {
        m.set_i16(service_timer, 0);
        call_simple(context, host, call::GIVE_TIME, 0);
        result = true;
    } else if timer > call_simple(context, host, call::CURRENT_HIGHEST_SERVICE_TIMER, 0) {
        call_simple(context, host, call::SET_CURRENT_HIGHEST_SERVICE_TIMER, timer);
    }
    m.set_bool(actor + a::META_TIMESLICE as u32, result);
    result
}

/// `actor_clear_orders`.
pub fn actor_clear_orders(context: &AiStepContext, host: &impl AiHost) {
    let m = host.memory();
    let actor = context.actor;
    for offset in 0..actor_orders::SIZEOF as u32 {
        m.w8(actor + a::ORDERS as u32 + offset, 0);
    }
    m.set_i16(actor + a::ORDERS_MOVE_ANIMATION_IMPULSE as u32, NONE as i16);
    m.set_i16(actor + a::ORDERS_MOVE_OVERRIDE_MOVEMENT_TYPE as u32, NONE as i16);
    m.set_i16(actor + a::ORDERS_MOVE_OVERRIDE_MOVEMENT_FACING as u32, NONE as i16);
}

/// `actor_clear_output`.
pub fn actor_clear_output(context: &AiStepContext, host: &impl AiHost) {
    let m = host.memory();
    let actor = context.actor;
    for (output, input) in [
        (a::OUTPUT_FACING_VECTOR, a::INPUT_FACING_VECTOR),
        (a::OUTPUT_AIMING_VECTOR, a::INPUT_AIMING_VECTOR),
        (a::OUTPUT_LOOKING_VECTOR, a::INPUT_LOOKING_VECTOR),
    ] {
        for word in 0..3 {
            m.w32(actor + output as u32 + word * 4, m.r32(actor + input as u32 + word * 4));
        }
    }
    m.w32(actor + a::OUTPUT_CONTROL_FLAGS as u32, 0);
    m.set_f32(actor + a::OUTPUT_ANALOG_PRIMARY_TRIGGER as u32, 0.0);
    for word in 0..3 {
        m.set_f32(actor + a::OUTPUT_THROTTLE as u32 + word * 4, 0.0);
    }
    m.set_i16(actor + a::OUTPUT_ANIMATION_IMPULSE as u32, NONE as i16);
}
