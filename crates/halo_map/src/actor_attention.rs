//! Three small steps of an actor's update: whether its looking forces how it
//! moves (actor_looking.c's `actor_look_affect_movement`), its idle
//! vocalizations (ai_communication.c's `actor_communication_update`), and
//! its looking at whoever it is in conversation with (action_converse.c's
//! `actor_conversation_control`).
//!
//! The actor's datum is the game's memory, worked on in place
//! ([`crate::ai_actor`]); the path, the direction's decoding, the dialogue,
//! the speech, a unit's prop and head and the secondary look are the
//! game's, through [`AiHost::call`] (each step's call function in its C
//! file, shared with the C).

use crate::ai_actor::{AiCall, AiHost, AiMemoryExt, AiStepContext, NONE};
use crate::layout::{actor_datum as a, ai_values as v};

/// The look's effect on movement's calls (halo_sim.h's
/// `_actor_look_movement_call_*`).
pub mod look_movement_call {
    /// `actor_path_has_path(actor)`, the result.
    pub const HAS_PATH: i32 = 1;
    /// `actor_look_decode_direction(actor, &orders.look.primary_direction,
    /// &control.moving_forced_aim_direction)`, the result.
    pub const DECODE_DIRECTION: i32 = 2;
}

/// The idle vocalizations' calls (halo_sim.h's `_actor_communication_call_*`).
pub mod communication_call {
    /// `ai_globals->dialogue_triggers_enabled`, the result.
    pub const DIALOGUE_TRIGGERS_ENABLED: i32 = 1;
    /// `actor_in_combat(actor)`, the result.
    pub const IN_COMBAT: i32 = 2;
    /// `actor_reset_idle_vocalization_timer(actor)`.
    pub const RESET_IDLE_VOCALIZATION_TIMER: i32 = 3;
    /// The idle speech, in combat or not (`a`).
    pub const IDLE_SPEECH: i32 = 4;
}

/// The conversation's calls (halo_sim.h's `_actor_conversation_call_*`).
pub mod conversation_call {
    /// `prop_get_active_by_unit_index(actor, a)`, the result.
    pub const ACTIVE_PROP: i32 = 1;
    /// `unit_get_head_position(a)`, in `output`.
    pub const HEAD_POSITION: i32 = 2;
    /// `actor_look_secondary(actor, communicating prop, turn and aim,
    /// direction)`: the direction's type `a`, its prop `b` or its point in
    /// `input`.
    pub const LOOK: i32 = 3;
}

/// The local enum (ai_communication.c's).
const MODE_ALERT: i16 = 2;

fn call_full(context: &AiStepContext, host: &impl AiHost, kind: i32, a: i32, b: i32, input: [f32; 3]) -> AiCall {
    let mut call = AiCall { kind, actor: context.actor_index, a, b, ..AiCall::default() };
    call.input[..3].copy_from_slice(&input);
    host.call(&mut call);
    call
}

fn call_simple(context: &AiStepContext, host: &impl AiHost, kind: i32, a: i32) -> i32 {
    call_full(context, host, kind, a, 0, [0.0; 3]).result
}

/// `actor_look_affect_movement`.
pub fn actor_look_affect_movement(context: &AiStepContext, host: &impl AiHost) {
    let m = host.memory();
    let actor = context.actor;
    let direction_type = m.i16(actor + a::ORDERS_LOOK_PRIMARY_DIRECTION_TYPE as u32);
    let priority = actor + a::ORDERS_LOOK_PRIMARY_PRIORITY as u32;
    let movement = v::_DIRECTION_SPECIFICATION_MOVEMENT as i16;

    if direction_type == movement && call_simple(context, host, look_movement_call::HAS_PATH, 0) == 0 {
        m.set_i16(priority, v::_PRIMARY_PRIORITY_NONE as i16);
    }
    let forced = m.i16(priority) >= v::_PRIMARY_PRIORITY_FACING as i16
        && direction_type != movement
        && call_simple(context, host, look_movement_call::DECODE_DIRECTION, 0) != 0;
    m.set_bool(actor + a::CONTROL_MOVING_FORCED_BY_AIMING as u32, forced);
}

/// `actor_communication_update`.
pub fn actor_communication_update(context: &AiStepContext, host: &impl AiHost) {
    let m = host.memory();
    let actor = context.actor;

    if m.i16(actor + a::STATE_MODE as u32) >= MODE_ALERT
        && call_simple(context, host, communication_call::DIALOGUE_TRIGGERS_ENABLED, 0) != 0
    {
        let in_combat = call_simple(context, host, communication_call::IN_COMBAT, 0) as u8;
        let timer = actor + a::CONTROL_IDLE_VOCALIZATION_TIMER as u32;
        if m.i16(timer) == 0 || m.r8(actor + a::CONTROL_IDLE_VOCALIZATION_COMBAT as u32) != in_combat {
            call_simple(context, host, communication_call::RESET_IDLE_VOCALIZATION_TIMER, 0);
        }
        if m.i16(timer) > 0 {
            m.set_i16(timer, m.i16(timer) - 1);
            if m.i16(timer) == 0 {
                call_simple(context, host, communication_call::IDLE_SPEECH, in_combat as i32);
            }
        }
    }
}

/// `actor_conversation_control`.
pub fn actor_conversation_control(context: &AiStepContext, host: &impl AiHost) {
    let m = host.memory();
    let actor = context.actor;
    let attention_unit_index = m.i32(actor + a::EXTERNAL_ORDERS_CONVERSATION_ATTENTION_UNIT_INDEX as u32);

    if m.i32(actor + a::EXTERNAL_ORDERS_CONVERSATION_INDEX as u32) != NONE && attention_unit_index != NONE {
        let prop_index = call_simple(context, host, conversation_call::ACTIVE_PROP, attention_unit_index);
        let (direction_type, point) = if prop_index != NONE {
            (v::_DIRECTION_SPECIFICATION_PROP, [0.0; 3])
        } else {
            let head =
                call_full(context, host, conversation_call::HEAD_POSITION, attention_unit_index, 0, [0.0; 3]).vector();
            (v::_DIRECTION_SPECIFICATION_POINT, head)
        };
        let prop = if prop_index != NONE { prop_index } else { 0 };
        call_full(context, host, conversation_call::LOOK, direction_type as i32, prop, point);
    }
}
