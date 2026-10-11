//! The script runtime's update in Rust, on the running game: threads,
//! stacks, evaluation, globals and the built-in functions are
//! `halo_map::hs_runtime`'s, working on the game's memory in place; the
//! library's functions and what the runtime asks of the rest of the game are
//! the game's, through hs_runtime.c's `hs_runtime_call` ([`HsCallbacks`]).
//!
//! In check mode the game's own update runs first, recording each call (a
//! library function's with its thread's memory before and after, and where
//! its value went); the game then puts its script memory back as the update
//! began, Rust replays the update on it, and the game compares what Rust left
//! with what its own did.

use std::cell::Cell;

use halo_map::hs_runtime::{HsCall, HsContext, HsHost, HsMemory, call, hs_runtime_update};

/// `sizeof(struct hs_thread_datum)`.
pub const THREAD_DATUM_SIZE: usize = 0x218;

/// The game's side of the runtime (halo_sim.c).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct HsCallbacks {
    pub call: unsafe extern "C" fn(call: *mut HsCall),
}

/// A call of the game's update, recorded in check mode: for a library
/// function, its thread's memory before and after, and the value at the
/// place its value returns to.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct HsEventC {
    pub call: HsCall,
    pub thread: u32,
    pub before: [u8; THREAD_DATUM_SIZE],
    pub after: [u8; THREAD_DATUM_SIZE],
    pub destination: u32,
    pub destination_value: u32,
    pub random_seed_after: u32,
    pub initialized_after: u32,
}

/// The update: where its state and tables are, and in check mode the game's
/// calls and the first that differs (-1 for none) and why (1 its kind, 2 its
/// arguments, 3 the thread's memory before it, 4 more calls than the game's,
/// 5 fewer).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct HsUpdateC {
    pub context: HsContextC,
    pub events: *const HsEventC,
    pub event_count: u32,
    pub difference_event: i32,
    pub difference_reason: i32,
}

/// `halo_map::hs_runtime::HsContext`, as the game hands it over.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct HsContextC {
    pub thread_data: u32,
    pub syntax_data: u32,
    pub global_data: u32,
    pub functions: u32,
    pub function_count: i32,
    pub external_globals: u32,
    pub external_global_count: i32,
    pub scripts: u32,
    pub script_count: i32,
    pub globals: u32,
    pub global_count: i32,
    pub runtime_globals: u32,
    pub random_seed: u32,
    pub game_time: i32,
    pub string_default: u32,
}

impl HsContextC {
    fn context(&self) -> HsContext {
        HsContext {
            thread_data: self.thread_data,
            syntax_data: self.syntax_data,
            global_data: self.global_data,
            functions: self.functions,
            function_count: self.function_count,
            external_globals: self.external_globals,
            external_global_count: self.external_global_count,
            scripts: self.scripts,
            script_count: self.script_count,
            globals: self.globals,
            global_count: self.global_count,
            runtime_globals: self.runtime_globals,
            random_seed: self.random_seed,
            game_time: self.game_time,
            string_default: self.string_default,
        }
    }
}

/// The game's memory, in this process.
struct GameMemory;

impl HsMemory for GameMemory {
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
    callbacks: HsCallbacks,
}

impl HsHost for LiveHost {
    fn memory(&self) -> &dyn HsMemory {
        &GameMemory
    }
    fn call(&self, call: &mut HsCall) {
        // SAFETY: the game's callback
        unsafe { (self.callbacks.call)(call) };
    }
}

struct CheckHost<'a> {
    context: HsContext,
    events: &'a [HsEventC],
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

impl HsHost for CheckHost<'_> {
    fn memory(&self) -> &dyn HsMemory {
        &GameMemory
    }
    fn call(&self, call: &mut HsCall) {
        let index = self.next.get();
        let Some(event) = self.events.get(index) else {
            self.differ(index, 4);
            return;
        };
        self.next.set(index + 1);
        let e = &event.call;
        if e.kind != call.kind {
            self.differ(index, 1);
        } else if e.a != call.a || e.b != call.b || e.c != call.c {
            self.differ(index, 2);
        }
        if call.kind == call::EXTERNAL && e.kind == call::EXTERNAL {
            // SAFETY: the thread's datum in the game's memory
            let thread = unsafe { std::slice::from_raw_parts_mut(event.thread as usize as *mut u8, THREAD_DATUM_SIZE) };
            if thread != &event.before[..] {
                self.differ(index, 3);
            }
            thread.copy_from_slice(&event.after);
            if event.destination != 0 {
                GameMemory.w32(event.destination, event.destination_value);
            }
            GameMemory.w32(self.context.random_seed, event.random_seed_after);
            GameMemory.w8(self.context.runtime_globals, event.initialized_after as u8);
        }
        call.result = e.result;
    }
}

/// The script runtime's update on the running game, in place of the game's.
/// Returns 0, or -1 if it can't run (the game then runs its own; nothing was
/// done).
///
/// # Safety
/// `update` must be valid, describing the game's script memory.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn halo_sim_hs_update(update: *mut HsUpdateC) -> i32 {
    if update.is_null() {
        return -1;
    }
    let Some(host) = crate::world::host() else { return -1 };
    // SAFETY: the caller's update
    let update = unsafe { &mut *update };
    hs_runtime_update(&update.context.context(), &LiveHost { callbacks: host.hs });
    0
}

/// The check of an update the game ran: Rust's replay of it on the script
/// memory the game has put back as the update began. Returns 0, or -1.
///
/// # Safety
/// As [`halo_sim_hs_update`]; the events hold their count.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn halo_sim_hs_update_check(update: *mut HsUpdateC) -> i32 {
    if update.is_null() {
        return -1;
    }
    // SAFETY: the caller's update
    let update = unsafe { &mut *update };
    // SAFETY: the game's recording, as described
    let events = if update.events.is_null() || update.event_count == 0 {
        &[][..]
    } else {
        unsafe { std::slice::from_raw_parts(update.events, update.event_count as usize) }
    };
    let context = update.context.context();
    let checker = CheckHost { context, events, next: Cell::new(0), difference: Cell::new(None) };
    hs_runtime_update(&context, &checker);
    if checker.difference.get().is_none() && checker.next.get() < events.len() {
        checker.differ(checker.next.get(), 5);
    }
    (update.difference_event, update.difference_reason) = checker.difference.get().map_or((-1, 0), |(e, r)| (e as i32, r));
    0
}
