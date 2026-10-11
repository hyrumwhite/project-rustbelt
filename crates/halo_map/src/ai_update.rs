//! The AI's update (`ai.c`'s `ai_update`, `actors.c`'s `actors_update` and
//! `actor_update`): what runs each tick (the conversations, the encounters
//! and every active actor, or the actors moved at random, or frozen when the
//! AI is switched off), the order of an actor's update, and the walk over
//! the actors (each active encounter's, then those of no encounter).
//!
//! Each step, and each question the walk asks of the AI's state, is the
//! game's, through [`AiUpdateHost::call`] (ai.c's `ai_update_call`, shared
//! with the C).

/// The update's steps and questions (halo_sim.h's `_ai_update_call_*`).
pub mod call {
    /// The AI's state (the result's bits: [`super::STATE_INITIALIZED`] and
    /// the rest).
    pub const STATE: i32 = 1;
    /// `ai_has_control_data` set to `a`.
    pub const SET_CONTROL_DATA: i32 = 2;
    pub const PROFILE_ENTER: i32 = 3;
    pub const PROFILE_EXIT: i32 = 4;
    pub const DEBUG_UPDATE: i32 = 5;
    pub const PROFILE_UPDATE: i32 = 6;
    pub const PLACE_PENDING_MOUNTED_WEAPONS: i32 = 7;
    pub const MOVE_RANDOMLY: i32 = 8;
    pub const CONVERSATION_UPDATE: i32 = 9;
    pub const ENCOUNTERS_UPDATE: i32 = 10;
    pub const FREEZE: i32 = 11;
    /// The service timers moved on and the frame's time not yet given.
    pub const ACTORS_BEGIN: i32 = 12;
    /// The next encounter from the absolute index `a` (`data_iterator_next`):
    /// whether there is one (`result`), the index the walk goes on from
    /// (`result2`), whether it is active (`result3`) and its first actor
    /// (`result4`).
    pub const ENCOUNTER_NEXT: i32 = 13;
    /// `ai_globals->first_encounterless_actor_index`.
    pub const FIRST_ENCOUNTERLESS: i32 = 14;
    /// The actor `a`: its next actor, and its bits ([`super::ACTOR_ACTIVE`]
    /// and the rest).
    pub const ACTOR: i32 = 15;
    pub const ACTOR_ERASE: i32 = 16;
    /// The actor `a` after its decisions: whether it is dormant (bit 0) or a
    /// swarm (bit 1).
    pub const ACTOR_DORMANCY: i32 = 17;
    pub const UPDATE_BEGIN: i32 = 18;
    /// `actor_general_update`: whether the actor goes on.
    pub const GENERAL_UPDATE: i32 = 19;
    pub const TIMESLICE: i32 = 20;
    pub const INPUT: i32 = 21;
    pub const PERCEPTION: i32 = 22;
    pub const SITUATION: i32 = 23;
    pub const EMOTION: i32 = 24;
    pub const CLEAR_ORDERS: i32 = 25;
    pub const DECISION_LOOP: i32 = 26;
    pub const ACTION_UPDATE: i32 = 27;
    pub const SWARM_CONTROL: i32 = 28;
    pub const CLEAR_OUTPUT: i32 = 29;
    pub const ACTION_CONTROL: i32 = 30;
    pub const COMMUNICATION: i32 = 31;
    pub const CONVERSATION_CONTROL: i32 = 32;
    pub const DESTINATION: i32 = 33;
    pub const LOOK_AFFECT_MOVEMENT: i32 = 34;
    pub const MOVE_UPDATE: i32 = 35;
    pub const LOOK_UPDATE: i32 = 36;
    pub const COMBAT_UPDATE: i32 = 37;
    pub const UNIT_CONTROL: i32 = 38;
    pub const UPDATE_END: i32 = 39;
}

/// [`call::STATE`]'s bits.
pub const STATE_INITIALIZED: i32 = 1 << 0;
pub const STATE_PROFILE_DISABLED: i32 = 1 << 1;
pub const STATE_MOVE_RANDOMLY: i32 = 1 << 2;
pub const STATE_ACTIVE: i32 = 1 << 3;
pub const STATE_HAS_CONTROL_DATA: i32 = 1 << 4;

/// [`call::ACTOR`]'s bits.
pub const ACTOR_ACTIVE: i32 = 1 << 0;
pub const ACTOR_ERASE_NEXT_TICK: i32 = 1 << 1;
/// `state.mode > _actor_mode_braindead`.
pub const ACTOR_THINKING: i32 = 1 << 2;

const NONE: i32 = -1;

/// halo_sim.h's `struct halo_sim_ai_update_call`.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AiUpdateCall {
    pub kind: i32,
    pub a: i32,
    pub result: i32,
    pub result2: i32,
    pub result3: i32,
    pub result4: i32,
}

/// The game's side of the update.
pub trait AiUpdateHost {
    fn call(&self, call: &mut AiUpdateCall);
}

fn ask(host: &impl AiUpdateHost, kind: i32, a: i32) -> AiUpdateCall {
    let mut call = AiUpdateCall { kind, a, ..Default::default() };
    host.call(&mut call);
    call
}

fn step(host: &impl AiUpdateHost, kind: i32, a: i32) -> i32 {
    ask(host, kind, a).result
}

/// `struct actor_iterator`, active actors only (`actor_iterator_new(...,
/// TRUE)`).
struct ActorIterator {
    encounter_absolute_index: i32,
    iterated_encounterless_list: bool,
    next_index: i32,
}

impl ActorIterator {
    /// `actor_iterator_next`: the next active actor and its bits.
    fn next(&mut self, host: &impl AiUpdateHost) -> Option<(i32, i32)> {
        if step(host, call::STATE, 0) & STATE_INITIALIZED == 0 {
            return None;
        }
        if self.next_index == NONE {
            let mut encounter_found;
            loop {
                let encounter = ask(host, call::ENCOUNTER_NEXT, self.encounter_absolute_index);
                self.encounter_absolute_index = encounter.result2;
                encounter_found = encounter.result != 0;
                if !encounter_found {
                    break;
                }
                if encounter.result3 != 0 {
                    self.next_index = encounter.result4;
                }
                if self.next_index != NONE {
                    break;
                }
            }
            if !encounter_found && !self.iterated_encounterless_list {
                self.next_index = step(host, call::FIRST_ENCOUNTERLESS, 0);
                self.iterated_encounterless_list = true;
            }
        }
        loop {
            let index = self.next_index;
            if index == NONE {
                return None;
            }
            let actor = ask(host, call::ACTOR, index);
            self.next_index = actor.result;
            if actor.result2 & ACTOR_ACTIVE != 0 {
                return Some((index, actor.result2));
            }
        }
    }
}

/// `actor_update`.
fn actor_update(host: &impl AiUpdateHost, actor_index: i32) {
    let s = |kind| step(host, kind, actor_index);
    s(call::UPDATE_BEGIN);
    if s(call::GENERAL_UPDATE) != 0 {
        s(call::TIMESLICE);
        s(call::INPUT);
        s(call::PERCEPTION);
        s(call::SITUATION);
        s(call::EMOTION);
        s(call::CLEAR_ORDERS);
        s(call::DECISION_LOOP);
        s(call::ACTION_UPDATE);
        let dormancy = s(call::ACTOR_DORMANCY);
        if dormancy & 1 == 0 {
            if dormancy & 2 != 0 {
                s(call::SWARM_CONTROL);
            } else {
                s(call::CLEAR_OUTPUT);
                s(call::ACTION_CONTROL);
                s(call::COMMUNICATION);
                s(call::CONVERSATION_CONTROL);
                s(call::DESTINATION);
                s(call::LOOK_AFFECT_MOVEMENT);
                s(call::MOVE_UPDATE);
                s(call::LOOK_UPDATE);
                s(call::COMBAT_UPDATE);
                s(call::UNIT_CONTROL);
            }
        }
    }
    s(call::UPDATE_END);
}

/// `actors_update`.
fn actors_update(host: &impl AiUpdateHost) {
    step(host, call::ACTORS_BEGIN, 0);
    // (`actor_iterator_new` leaves the iterator as it was when the AI isn't
    // initialized for the map, and `actor_iterator_next` then gives none)
    let mut iterator = ActorIterator { encounter_absolute_index: 0, iterated_encounterless_list: false, next_index: NONE };
    while let Some((actor_index, bits)) = iterator.next(host) {
        if bits & ACTOR_ERASE_NEXT_TICK != 0 {
            step(host, call::ACTOR_ERASE, actor_index);
        } else if bits & ACTOR_THINKING != 0 {
            actor_update(host, actor_index);
        }
    }
}

/// `ai_update`.
pub fn ai_update(host: &impl AiUpdateHost) {
    let state = step(host, call::STATE, 0);
    let update_ai = state & STATE_INITIALIZED != 0 && state & STATE_PROFILE_DISABLED == 0;
    let move_actors_randomly = state & STATE_MOVE_RANDOMLY != 0;
    step(host, call::PROFILE_ENTER, 0);
    if update_ai {
        step(host, call::DEBUG_UPDATE, 0);
        step(host, call::PROFILE_UPDATE, 0);
        step(host, call::PLACE_PENDING_MOUNTED_WEAPONS, 0);
        if move_actors_randomly {
            step(host, call::MOVE_RANDOMLY, 0);
            step(host, call::SET_CONTROL_DATA, 1);
        } else {
            // (asked again: the steps before may change it)
            let state = step(host, call::STATE, 0);
            if state & STATE_ACTIVE != 0 {
                step(host, call::CONVERSATION_UPDATE, 0);
                step(host, call::ENCOUNTERS_UPDATE, 0);
                actors_update(host);
                step(host, call::SET_CONTROL_DATA, 1);
            } else if state & STATE_HAS_CONTROL_DATA != 0 {
                step(host, call::FREEZE, 0);
                step(host, call::SET_CONTROL_DATA, 0);
            }
        }
    }
    step(host, call::PROFILE_EXIT, 0);
}
