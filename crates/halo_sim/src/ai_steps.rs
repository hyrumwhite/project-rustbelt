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

use halo_map::actor_emotions::actor_emotion_update;
use halo_map::actor_input::actor_input_update;
use halo_map::actor_situation::actor_situation_update;
use halo_map::actor_upkeep::{actor_clear_orders, actor_clear_output, actor_get_timeslice};
use halo_map::ai_actor::{AiCall, AiHost, AiMemory, AiStepContext};

/// `sizeof(struct actor_datum)`.
pub const ACTOR_DATUM_SIZE: usize = halo_map::layout::actor_datum::SIZEOF;

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
}

/// A call of the game's step, recorded in check mode, with the actor's
/// datum before and after it.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct AiEventC {
    pub call: AiCall,
    pub before: [u8; ACTOR_DATUM_SIZE],
    pub after: [u8; ACTOR_DATUM_SIZE],
}

/// A step: where it works, and in check mode the game's calls and the first
/// that differs (-1 for none) and why (1 its kind, 2 its arguments, 3 the
/// actor before it, 4 more calls than the game's, 5 fewer).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct AiStepC {
    pub context: AiStepContext,
    pub events: *const AiEventC,
    pub event_count: u32,
    pub difference_event: i32,
    pub difference_reason: i32,
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
        // SAFETY: the actor's datum in the game's memory
        let actor = unsafe { std::slice::from_raw_parts_mut(self.actor as usize as *mut u8, ACTOR_DATUM_SIZE) };
        if actor != &event.before[..] {
            self.differ(index, 3);
        }
        actor.copy_from_slice(&event.after);
        call.output = e.output;
        call.result = e.result;
    }
}

/// The steps the port runs (halo_sim.h's `_halo_sim_ai_step_*`).
const STEP_EMOTION: i32 = 0;
const STEP_SITUATION: i32 = 1;
const STEP_TIMESLICE: i32 = 2;
const STEP_CLEAR_ORDERS: i32 = 3;
const STEP_CLEAR_OUTPUT: i32 = 4;
const STEP_INPUT: i32 = 5;

fn run(step: i32, context: &AiStepContext, host: &impl AiHost) -> bool {
    match step {
        STEP_EMOTION => actor_emotion_update(context, host),
        STEP_SITUATION => actor_situation_update(context, host),
        STEP_TIMESLICE => {
            actor_get_timeslice(context, host);
        }
        STEP_CLEAR_ORDERS => actor_clear_orders(context, host),
        STEP_CLEAR_OUTPUT => actor_clear_output(context, host),
        STEP_INPUT => actor_input_update(context, host),
        _ => return false,
    }
    true
}

fn callback(callbacks: &AiStepCallbacks, step: i32) -> Option<unsafe extern "C" fn(*mut AiCall)> {
    match step {
        STEP_EMOTION => Some(callbacks.emotion),
        STEP_SITUATION => Some(callbacks.situation),
        STEP_TIMESLICE | STEP_CLEAR_ORDERS | STEP_CLEAR_OUTPUT => Some(callbacks.upkeep),
        STEP_INPUT => Some(callbacks.input),
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
    if run(step, &step_call.context, &LiveHost { call }) { 0 } else { -1 }
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
    let checker =
        CheckHost { actor: step_call.context.actor, events, next: Cell::new(0), difference: Cell::new(None) };
    if !run(step, &step_call.context, &checker) {
        return -1;
    }
    if checker.difference.get().is_none() && checker.next.get() < events.len() {
        checker.differ(checker.next.get(), 5);
    }
    (step_call.difference_event, step_call.difference_reason) =
        checker.difference.get().map_or((-1, 0), |(e, r)| (e as i32, r));
    0
}
