//! The AI's ports of an actor's steps, on the running game: each works on
//! the actor's datum, props and definition in the game's memory, in place
//! (`halo_map::ai_actor`), and asks the rest of the game through its step's
//! call function in the C ([`AiStepCallbacks`]).
//!
//! In check mode the game's own step runs first, recording each call with
//! the actor's datum before and after it; the game then puts the datum back
//! as the step began, Rust replays the step on it, and the game compares
//! the datum Rust leaves with its own.

use std::cell::Cell;

use halo_map::actor_combat::actor_combat_update;
use halo_map::actor_attention::{actor_communication_update, actor_conversation_control, actor_look_affect_movement};
use halo_map::actor_dispatch::{actor_action_control, actor_action_update, actor_decision_loop, actor_type_swarm_control};
use halo_map::actor_destination::actor_destination_update;
use halo_map::actor_emotions::actor_emotion_update;
use halo_map::actor_general::actor_general_update;
use halo_map::actor_input::actor_input_update;
use halo_map::actor_looking::actor_look_update;
use halo_map::actor_moving::actor_move_update;
use halo_map::actor_perception::actor_perception_update;
use halo_map::actor_situation::actor_situation_update;
use halo_map::actor_unit_control::actor_unit_control;
use halo_map::actor_upkeep::{actor_clear_orders, actor_clear_output, actor_get_timeslice};
use halo_map::ai_actor::{AiCall, AiHost, AiMemory, AiStepContext, PropIterator};

/// `sizeof(struct actor_datum)`.
pub const ACTOR_DATUM_SIZE: usize = halo_map::layout::actor_datum::SIZEOF;
/// The head of an actor's debug info the checks save and compare
/// (halo_sim.h's `HALO_SIM_ACTOR_DEBUG_HEAD_SIZE`).
pub const ACTOR_DEBUG_HEAD_SIZE: usize = 0x100;
/// `sizeof(struct data_array)`.
pub const DATA_ARRAY_SIZE: usize = 0x38;
/// `sizeof(struct prop_datum)`.
pub const PROP_DATUM_SIZE: usize = halo_map::layout::prop_datum::SIZEOF;

/// A prop a call of a step that writes props changed (or made), as the
/// call left it.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct AiPropDeltaC {
    pub address: u32,
    pub after: [u8; PROP_DATUM_SIZE],
}

/// The game's side of each step (halo_sim.c).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct AiStepCallbacks {
    /// actor_perception.c's `actor_emotion_call`.
    pub emotion: unsafe extern "C" fn(call: *mut AiCall),
    /// actor_perception.c's `actor_situation_call`.
    pub situation: unsafe extern "C" fn(call: *mut AiCall),
    /// actors.c's `actor_upkeep_call` (the timeslice and the clearing of
    /// orders and output).
    pub upkeep: unsafe extern "C" fn(call: *mut AiCall),
    /// actors.c's `actor_input_call`.
    pub input: unsafe extern "C" fn(call: *mut AiCall),
    /// actors.c's `actor_general_call`.
    pub general: unsafe extern "C" fn(call: *mut AiCall),
    /// actor_looking.c's `actor_look_movement_call`.
    pub look_movement: unsafe extern "C" fn(call: *mut AiCall),
    /// ai_communication.c's `actor_communication_call`.
    pub communication: unsafe extern "C" fn(call: *mut AiCall),
    /// action_converse.c's `actor_conversation_call`.
    pub conversation: unsafe extern "C" fn(call: *mut AiCall),
    /// actor_moving.c's `actor_destination_call`.
    pub destination: unsafe extern "C" fn(call: *mut AiCall),
    /// actors.c's `actor_unit_control_call`.
    pub unit_control: unsafe extern "C" fn(call: *mut AiCall),
    /// actor_perception.c's `actor_perception_call`.
    pub perception: unsafe extern "C" fn(call: *mut AiCall),
    /// actor_looking.c's `actor_look_call`.
    pub look: unsafe extern "C" fn(call: *mut AiCall),
    /// actor_moving.c's `actor_move_call`.
    pub moving: unsafe extern "C" fn(call: *mut AiCall),
    /// actor_combat.c's `actor_combat_call`.
    pub combat: unsafe extern "C" fn(call: *mut AiCall),
    /// actors.c's `actor_dispatch_call` (the decision loop, the action's
    /// update and control, a swarm's control).
    pub dispatch: unsafe extern "C" fn(call: *mut AiCall),
}

/// A call of the game's step, recorded in check mode, with the actor's
/// datum before and after it.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct AiEventC {
    pub call: AiCall,
    pub before: [u8; ACTOR_DATUM_SIZE],
    pub after: [u8; ACTOR_DATUM_SIZE],
    /// For a step that writes props: their checksum before the call
    /// ([`props_checksum`]) and the ones it changed (`deltas`).
    pub prop_checksum: u32,
    pub delta_first: u32,
    pub delta_count: u32,
    /// The props' data array as the call left it (its count and free list
    /// change as props are made and deleted).
    pub prop_data_after: [u8; DATA_ARRAY_SIZE],
    /// The head of the actor's debug info before and after the call.
    pub debug_before: [u8; ACTOR_DEBUG_HEAD_SIZE],
    pub debug_after: [u8; ACTOR_DEBUG_HEAD_SIZE],
}

/// A step: where it works, what it returns (for the steps that return
/// something: the timeslice and the general update), and in check mode the
/// game's calls and the first that differs (-1 for none) and why (1 its
/// kind, 2 its arguments, 3 the actor before it, 4 more calls than the
/// game's, 5 fewer, 6 the actor's props before it).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct AiStepC {
    pub context: AiStepContext,
    pub events: *const AiEventC,
    pub event_count: u32,
    pub difference_event: i32,
    pub difference_reason: i32,
    pub result: i32,
    /// Whether the step writes props (each call's prop checksum is then
    /// compared, reason 6, and its deltas applied).
    pub props: i32,
    pub deltas: *const AiPropDeltaC,
    pub delta_count: u32,
}

/// The game's memory, in this process.
struct GameMemory;

impl AiMemory for GameMemory {
    fn r8(&self, address: u32) -> u8 {
        // SAFETY: the game's memory, at an address its own state gives
        unsafe { (address as usize as *const u8).read_unaligned() }
    }
    fn r16(&self, address: u32) -> u16 {
        // SAFETY: as above
        unsafe { (address as usize as *const u16).read_unaligned() }
    }
    fn r32(&self, address: u32) -> u32 {
        // SAFETY: as above
        unsafe { (address as usize as *const u32).read_unaligned() }
    }
    fn w8(&self, address: u32, value: u8) {
        // SAFETY: as above
        unsafe { (address as usize as *mut u8).write_unaligned(value) }
    }
    fn w16(&self, address: u32, value: u16) {
        // SAFETY: as above
        unsafe { (address as usize as *mut u16).write_unaligned(value) }
    }
    fn w32(&self, address: u32, value: u32) {
        // SAFETY: as above
        unsafe { (address as usize as *mut u32).write_unaligned(value) }
    }
}

struct LiveHost {
    call: unsafe extern "C" fn(call: *mut AiCall),
}

impl AiHost for LiveHost {
    fn memory(&self) -> &dyn AiMemory {
        &GameMemory
    }
    fn call(&self, call: &mut AiCall) {
        // SAFETY: the game's callback
        unsafe { (self.call)(call) };
    }
}

struct CheckHost<'a> {
    context: AiStepContext,
    props: bool,
    deltas: &'a [AiPropDeltaC],
    actor: u32,
    events: &'a [AiEventC],
    next: Cell<usize>,
    difference: Cell<Option<(usize, i32)>>,
}

impl CheckHost<'_> {
    fn differ(&self, event: usize, reason: i32) {
        if self.difference.get().is_none() {
            self.difference.set(Some((event, reason)));
        }
    }
}

impl AiHost for CheckHost<'_> {
    fn memory(&self) -> &dyn AiMemory {
        &GameMemory
    }
    fn call(&self, call: &mut AiCall) {
        let index = self.next.get();
        let Some(event) = self.events.get(index) else {
            self.differ(index, 4);
            return;
        };
        self.next.set(index + 1);
        let e = &event.call;
        if e.kind != call.kind {
            self.differ(index, 1);
        } else if e.actor != call.actor
            || e.a != call.a
            || e.b != call.b
            || e.input.iter().zip(call.input.iter()).any(|(x, y)| x.to_bits() != y.to_bits())
        {
            self.differ(index, 2);
        }
        if self.props && props_checksum(&GameMemory, &self.context) != event.prop_checksum {
            self.differ(index, 6);
        }
        // SAFETY: the actor's datum in the game's memory
        let actor = unsafe { std::slice::from_raw_parts_mut(self.actor as usize as *mut u8, ACTOR_DATUM_SIZE) };
        if actor != &event.before[..] {
            self.differ(index, 3);
        }
        actor.copy_from_slice(&event.after);
        // SAFETY: the head of the actor's debug info in the game's memory
        let debug =
            unsafe { std::slice::from_raw_parts_mut(self.context.debug_info as usize as *mut u8, ACTOR_DEBUG_HEAD_SIZE) };
        if debug != &event.debug_before[..] {
            self.differ(index, 3);
        }
        debug.copy_from_slice(&event.debug_after);
        if self.props {
            // SAFETY: the props' data array in the game's memory
            unsafe {
                std::ptr::copy_nonoverlapping(
                    event.prop_data_after.as_ptr(),
                    self.context.prop_data as usize as *mut u8,
                    DATA_ARRAY_SIZE,
                )
            };
        }
        let first = event.delta_first as usize;
        for delta in self.deltas.get(first..first + event.delta_count as usize).unwrap_or(&[]) {
            // SAFETY: a prop's datum in the game's memory, as the game's call left it
            unsafe {
                std::ptr::copy_nonoverlapping(delta.after.as_ptr(), delta.address as usize as *mut u8, PROP_DATUM_SIZE)
            };
        }
        call.output = e.output;
        call.result = e.result;
        call.result2 = e.result2;
    }
}

/// The actor's props' checksum: FNV-1a over their datums in the actor's
/// order, then their count (halo_sim.c's `halo_sim_ai_props_checksum_of`).
fn props_checksum(m: &dyn AiMemory, context: &AiStepContext) -> u32 {
    let mut hash: u32 = 2_166_136_261;
    let mut count: u32 = 0;
    let mut iterator = PropIterator::new(m, context);
    while let Some(prop) = iterator.next(m, context) {
        for offset in 0..PROP_DATUM_SIZE as u32 {
            hash = (hash ^ m.r8(prop + offset) as u32).wrapping_mul(16_777_619);
        }
        count += 1;
    }
    (hash ^ count).wrapping_mul(16_777_619)
}

/// The steps the port runs (halo_sim.h's `_halo_sim_ai_step_*`).
const STEP_EMOTION: i32 = 0;
const STEP_SITUATION: i32 = 1;
const STEP_TIMESLICE: i32 = 2;
const STEP_CLEAR_ORDERS: i32 = 3;
const STEP_CLEAR_OUTPUT: i32 = 4;
const STEP_INPUT: i32 = 5;
const STEP_GENERAL: i32 = 6;
const STEP_LOOK_MOVEMENT: i32 = 7;
const STEP_COMMUNICATION: i32 = 8;
const STEP_CONVERSATION: i32 = 9;
const STEP_DESTINATION: i32 = 10;
const STEP_UNIT_CONTROL: i32 = 11;
const STEP_PERCEPTION: i32 = 12;
const STEP_LOOK: i32 = 13;
const STEP_MOVE: i32 = 14;
const STEP_COMBAT: i32 = 15;
const STEP_DECISION_LOOP: i32 = 16;
const STEP_ACTION_UPDATE: i32 = 17;
const STEP_ACTION_CONTROL: i32 = 18;
const STEP_SWARM_CONTROL: i32 = 19;

/// The step run: its result, or None for a step the port doesn't have.
fn run(step: i32, context: &AiStepContext, host: &impl AiHost) -> Option<i32> {
    match step {
        STEP_EMOTION => actor_emotion_update(context, host),
        STEP_SITUATION => actor_situation_update(context, host),
        STEP_TIMESLICE => return Some(actor_get_timeslice(context, host) as i32),
        STEP_CLEAR_ORDERS => actor_clear_orders(context, host),
        STEP_CLEAR_OUTPUT => actor_clear_output(context, host),
        STEP_INPUT => actor_input_update(context, host),
        STEP_GENERAL => return Some(actor_general_update(context, host) as i32),
        STEP_LOOK_MOVEMENT => actor_look_affect_movement(context, host),
        STEP_COMMUNICATION => actor_communication_update(context, host),
        STEP_CONVERSATION => actor_conversation_control(context, host),
        STEP_DESTINATION => actor_destination_update(context, host),
        STEP_UNIT_CONTROL => actor_unit_control(context, host),
        STEP_PERCEPTION => actor_perception_update(context, host),
        STEP_LOOK => actor_look_update(context, host),
        STEP_MOVE => actor_move_update(context, host),
        STEP_COMBAT => actor_combat_update(context, host),
        STEP_DECISION_LOOP => actor_decision_loop(context, host),
        STEP_ACTION_UPDATE => actor_action_update(context, host),
        STEP_ACTION_CONTROL => actor_action_control(context, host),
        STEP_SWARM_CONTROL => actor_type_swarm_control(context, host),
        _ => return None,
    }
    Some(0)
}

fn callback(callbacks: &AiStepCallbacks, step: i32) -> Option<unsafe extern "C" fn(*mut AiCall)> {
    match step {
        STEP_EMOTION => Some(callbacks.emotion),
        STEP_SITUATION => Some(callbacks.situation),
        STEP_TIMESLICE | STEP_CLEAR_ORDERS | STEP_CLEAR_OUTPUT => Some(callbacks.upkeep),
        STEP_INPUT => Some(callbacks.input),
        STEP_GENERAL => Some(callbacks.general),
        STEP_LOOK_MOVEMENT => Some(callbacks.look_movement),
        STEP_COMMUNICATION => Some(callbacks.communication),
        STEP_CONVERSATION => Some(callbacks.conversation),
        STEP_DESTINATION => Some(callbacks.destination),
        STEP_UNIT_CONTROL => Some(callbacks.unit_control),
        STEP_PERCEPTION => Some(callbacks.perception),
        STEP_LOOK => Some(callbacks.look),
        STEP_MOVE => Some(callbacks.moving),
        STEP_COMBAT => Some(callbacks.combat),
        STEP_DECISION_LOOP | STEP_ACTION_UPDATE | STEP_ACTION_CONTROL | STEP_SWARM_CONTROL => Some(callbacks.dispatch),
        _ => None,
    }
}

/// An actor's step on the running game, in place of the game's. Returns 0,
/// or -1 if it can't run (the game then runs its own; nothing was done).
///
/// # Safety
/// `step_call` must be valid, describing the game's actor.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn halo_sim_ai_step(step: i32, step_call: *mut AiStepC) -> i32 {
    if step_call.is_null() {
        return -1;
    }
    let Some(host) = crate::world::host() else { return -1 };
    let Some(call) = callback(&host.ai_steps, step) else { return -1 };
    // SAFETY: the caller's step
    let step_call = unsafe { &mut *step_call };
    let Some(result) = run(step, &step_call.context, &LiveHost { call }) else { return -1 };
    step_call.result = result;
    0
}

/// The check of a step the game ran: Rust's replay of it on the actor the
/// game has put back as the step began. Returns 0, or -1.
///
/// # Safety
/// As [`halo_sim_ai_step`]; the events hold their count.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn halo_sim_ai_step_check(step: i32, step_call: *mut AiStepC) -> i32 {
    if step_call.is_null() {
        return -1;
    }
    // SAFETY: the caller's step
    let step_call = unsafe { &mut *step_call };
    // SAFETY: the game's recording, as described
    let events = if step_call.events.is_null() || step_call.event_count == 0 {
        &[][..]
    } else {
        unsafe { std::slice::from_raw_parts(step_call.events, step_call.event_count as usize) }
    };
    // SAFETY: the game's recording, as described
    let deltas = if step_call.deltas.is_null() || step_call.delta_count == 0 {
        &[][..]
    } else {
        unsafe { std::slice::from_raw_parts(step_call.deltas, step_call.delta_count as usize) }
    };
    let checker = CheckHost {
        context: step_call.context,
        props: step_call.props != 0,
        deltas,
        actor: step_call.context.actor,
        events,
        next: Cell::new(0),
        difference: Cell::new(None),
    };
    let Some(result) = run(step, &step_call.context, &checker) else { return -1 };
    step_call.result = result;
    if checker.difference.get().is_none() && checker.next.get() < events.len() {
        checker.differ(checker.next.get(), 5);
    }
    (step_call.difference_event, step_call.difference_reason) =
        checker.difference.get().map_or((-1, 0), |(e, r)| (e as i32, r));
    0
}
