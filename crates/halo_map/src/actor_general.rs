//! An actor's general update (actors.c's `actor_general_update`, first in
//! every tick of an actor's update): the AI profile's counts, the erasing
//! of a swarm without its cache, the path's refresh flag, the suspicion and
//! command list timers, and whether it wakes or (after two seconds with no
//! reason to stay awake) goes dormant.
//!
//! The actor's datum and props are the game's memory, worked on in place
//! ([`crate::ai_actor`]); the profile, the erasing, its encounter, its
//! dormancy, its action's class and its path are the game's, through
//! [`AiHost::call`] (actors.c's `actor_general_call`, shared with the C).

use crate::ai_actor::{AiCall, AiHost, AiMemoryExt, AiStepContext, NONE, prop_get};
use crate::layout::{actor_datum as a, ai_values as v, prop_datum as p};

/// The general update's calls (halo_sim.h's `_actor_general_call_*`).
pub mod call {
    /// The AI profile's counts of actors and units updated and active.
    pub const PROFILE: i32 = 1;
    /// A swarm without its cache: warned of and erased.
    pub const ERASE: i32 = 2;
    /// The encounter `a`'s `force_active`, the result.
    pub const ENCOUNTER_FORCE_ACTIVE: i32 = 3;
    /// `actor_set_dormant(actor, a)`.
    pub const SET_DORMANT: i32 = 4;
    /// `actor_action_class(actor)`, the result.
    pub const ACTION_CLASS: i32 = 5;
    /// `actor_path_has_path(actor)`, the result.
    pub const HAS_PATH: i32 = 6;
    /// Whether the encounter `a` follows the players, the result.
    pub const ENCOUNTER_FOLLOWS_PLAYERS: i32 = 7;
}

/// The local enum (actors.c's).
const COMBAT_STATUS_NONE: i16 = 0;

fn call_simple(context: &AiStepContext, host: &impl AiHost, kind: i32, a: i32) -> i32 {
    let mut call = AiCall { kind, actor: context.actor_index, a, ..AiCall::default() };
    host.call(&mut call);
    call.result
}

/// `actor_general_update`: false when the actor was erased.
pub fn actor_general_update(context: &AiStepContext, host: &impl AiHost) -> bool {
    let m = host.memory();
    let actor = context.actor;

    call_simple(context, host, call::PROFILE, 0);
    if m.bool(actor + a::META_SWARM as u32) && m.i32(actor + a::META_SWARM_CACHE_INDEX as u32) == NONE {
        call_simple(context, host, call::ERASE, 0);
        return false;
    }

    m.set_bool(actor + a::CONTROL_PATH_REFRESHED_THIS_TICK as u32, false);
    let suspicion_timer = actor + a::STATE_SUSPICION_TIMER as u32;
    if m.i32(suspicion_timer) > 0 {
        m.w32(suspicion_timer, (m.i32(suspicion_timer) - 1) as u32);
        if m.i32(suspicion_timer) == 0 {
            m.set_i16(actor + a::STATE_SUSPICION_COMBAT_STATUS as u32, COMBAT_STATUS_NONE);
        }
    }
    let command_list_delay_timer = actor + a::STATE_COMMAND_LIST_DELAY_TIMER as u32;
    if m.i16(command_list_delay_timer) > 0 {
        m.set_i16(command_list_delay_timer, m.i16(command_list_delay_timer) - 1);
    }

    let encounter_index = m.i32(actor + a::META_ENCOUNTER_INDEX as u32);
    let mut force_active = m.r8(actor + a::META_FORCE_ACTIVE as u32);
    if encounter_index != NONE {
        force_active |= call_simple(context, host, call::ENCOUNTER_FORCE_ACTIVE, encounter_index) as u8;
    }

    if !m.bool(actor + a::META_DORMANT_DESIRE as u32) || force_active != 0 {
        call_simple(context, host, call::SET_DORMANT, 0);
    } else if !m.bool(actor + a::META_DORMANT as u32) {
        let action_class = call_simple(context, host, call::ACTION_CLASS, 0) as i16;
        let mut can_become_dormant = true;

        let target_prop_index = m.i32(actor + a::TARGET_TARGET_PROP_INDEX as u32);
        if action_class == v::_ACTION_CLASS_TRANSITORY as i16 {
            can_become_dormant = false;
        } else if target_prop_index != NONE {
            let prop = prop_get(m, context, target_prop_index);
            if m.bool(prop + p::PLAYER as u32) && m.bool(prop + p::ENEMY as u32) && !m.bool(prop + p::DEAD as u32) {
                let state = m.i16(prop + p::STATE as u32);
                let acknowledged = state >= v::_PROP_STATE_BECOMING_UNACKNOWLEDGED as i16
                    && state <= v::_PROP_STATE_ACKNOWLEDGED as i16;
                let orphaned = state >= v::_PROP_STATE_UNINSPECTED_ORPHAN as i16
                    && state <= v::_PROP_STATE_INSPECTED_ORPHAN as i16;
                if acknowledged || (orphaned && action_class == v::_ACTION_CLASS_PURSUIT as i16) {
                    can_become_dormant = false;
                }
            }
        }

        if can_become_dormant && call_simple(context, host, call::HAS_PATH, 0) != 0 {
            match m.i16(actor + a::CONTROL_PATH_DESTINATION_TYPE as u32) as usize {
                v::_DESTINATION_FIRING_POSITION => {
                    if m.i16(actor + a::STATE_ACTION as u32) == v::_ACTOR_ACTION_GUARD as i16
                        && call_simple(context, host, call::ENCOUNTER_FOLLOWS_PLAYERS, encounter_index) != 0
                    {
                        can_become_dormant = false;
                    }
                }
                v::_DESTINATION_PROP => {
                    let prop =
                        prop_get(m, context, m.i32(actor + a::CONTROL_PATH_DESTINATION_PROP_INDEX as u32));
                    if m.bool(prop + p::PLAYER as u32) {
                        can_become_dormant = false;
                    }
                }
                _ => {}
            }
        }

        if can_become_dormant {
            let timer = actor + a::META_BECOME_DORMANT_TIMER as u32;
            m.set_i16(timer, m.i16(timer).wrapping_add(1));
            if m.i16(timer) >= 60 {
                call_simple(context, host, call::SET_DORMANT, 1);
            }
        }
    }
    true
}
