//! The steps of an actor's update that hand it to its action and its type:
//! its decision loop (actors.c's `actor_decision_loop`: its type decides its
//! action, and the action performs, until it settles), its action's update
//! and control (actions.c's `actor_action_update`, `actor_action_control`)
//! and a swarm's control (actor_types.c's `actor_type_swarm_control`).
//!
//! The actions ported to Rust ([`crate::actions`]) and the types' decisions
//! ([`crate::actor_types`]) run here; the rest are still the game's: each
//! goes through [`AiHost::call`] (actors.c's `actor_dispatch_call`, shared
//! with the C).

use crate::actions::{self, Function};
use crate::actor_types;
use crate::ai_actor::{AiCall, AiHost, AiMemoryExt, AiStepContext, NONE};
use crate::layout::actor_datum as a;

/// The dispatch's calls (halo_sim.h's `_actor_dispatch_call_*`).
pub mod call {
    /// `actor_type_decide_action(actor)` (now Rust's, [`crate::actor_types`]).
    pub const DECIDE_ACTION: i32 = 1;
    /// `actor_stimulus_clear(actor)`.
    pub const STIMULUS_CLEAR: i32 = 2;
    /// `actor_action_perform(actor)`, the result.
    pub const ACTION_PERFORM: i32 = 3;
    /// The decision loop's failure (`a`: the history's next entry, and in bit
    /// 8 whether it ran out of iterations; the history in `input`): its
    /// warning, and the action reset to none.
    pub const LOOP_FAILED: i32 = 4;
    /// The game's `actor_action_update(actor)`.
    pub const ACTION_UPDATE: i32 = 5;
    /// The game's `actor_action_control(actor)`.
    pub const ACTION_CONTROL: i32 = 6;
    /// The game's `actor_type_swarm_control(actor)`.
    pub const SWARM_CONTROL: i32 = 7;
}

/// The local constants (actors.c's).
const MAXIMUM_DECISION_LOOP_ITERATIONS: i32 = 10;
const NUMBER_OF_DECISION_LOOP_HISTORY_ENTRIES: usize = 5;

fn call_simple(context: &AiStepContext, host: &impl AiHost, kind: i32) -> i32 {
    let mut call = AiCall { kind, actor: context.actor_index, ..AiCall::default() };
    host.call(&mut call);
    call.result
}

/// `actor_decision_loop`.
pub fn actor_decision_loop(context: &AiStepContext, host: &impl AiHost) {
    let m = host.memory();
    let action = context.actor + a::STATE_ACTION as u32;
    let action_changed = context.actor + a::STATE_ACTION_CHANGED as u32;
    let mut history = [NONE as i16; NUMBER_OF_DECISION_LOOP_HISTORY_ENTRIES];
    let mut action_performed = false;
    let mut iteration = 0;
    let mut history_index = 0;

    loop {
        history[history_index] = m.i16(action);
        iteration += 1;
        history_index = (history_index + 1) % NUMBER_OF_DECISION_LOOP_HISTORY_ENTRIES;
        m.set_bool(action_changed, false);
        actor_types::actor_type_decide_action(context, host);
        call_simple(context, host, call::STIMULUS_CLEAR);
        if (action_performed && !m.bool(action_changed)) || iteration >= MAXIMUM_DECISION_LOOP_ITERATIONS {
            break;
        }
        action_performed = match actions::run(m.i16(action), Function::Perform, context, host) {
            Some(performed) => performed,
            None => call_simple(context, host, call::ACTION_PERFORM) != 0,
        };
        if !action_performed && !m.bool(action_changed) {
            return;
        }
    }

    let mut call = AiCall {
        kind: call::LOOP_FAILED,
        actor: context.actor_index,
        a: history_index as i32 | ((iteration >= MAXIMUM_DECISION_LOOP_ITERATIONS) as i32) << 8,
        ..AiCall::default()
    };
    for (input, entry) in call.input.iter_mut().zip(history) {
        *input = entry as f32;
    }
    host.call(&mut call);
}

/// `actor_action_update`.
pub fn actor_action_update(context: &AiStepContext, host: &impl AiHost) {
    let action = host.memory().i16(context.actor + a::STATE_ACTION as u32);
    if actions::run(action, Function::Update, context, host).is_none() {
        call_simple(context, host, call::ACTION_UPDATE);
    }
}

/// `actor_action_control`.
pub fn actor_action_control(context: &AiStepContext, host: &impl AiHost) {
    let action = host.memory().i16(context.actor + a::STATE_ACTION as u32);
    if actions::run(action, Function::Control, context, host).is_none() {
        call_simple(context, host, call::ACTION_CONTROL);
    }
}

/// `actor_type_swarm_control`.
pub fn actor_type_swarm_control(context: &AiStepContext, host: &impl AiHost) {
    call_simple(context, host, call::SWARM_CONTROL);
}
