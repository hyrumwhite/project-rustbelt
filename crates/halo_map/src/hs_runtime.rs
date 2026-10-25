//! The script runtime: its level start (`hs_runtime.c`'s
//! `hs_runtime_initialize_for_new_map`: the scenario's globals initialized
//! and its scripts' threads), a console command's evaluation
//! (`hs_runtime_evaluate`) and its update (`hs_runtime_update`, with
//! `hs_thread_main`, `hs_evaluate`, the thread stacks, `hs_return`, casting,
//! globals and the built-in functions: `begin`, `begin_random`, `if`, `set`,
//! `and`, `or`, arithmetic, comparisons, `sleep`, `sleep_until`, `wake`,
//! `inspect` and the object cast).
//!
//! Script threads, their stacks (frames linked by pointers), the globals and
//! the random seed are the game's state, saved with its games, so the runtime
//! works on the game's memory in place, through [`HsMemory`]. The library's
//! functions (`hs.c`'s) are the game's, called through
//! [`HsHost::call`] (hs_runtime.c's `hs_runtime_call`, shared with the C): a
//! library function's evaluation works on the same stack with the game's own
//! runtime calls. So do the object lists, the questions the runtime asks, and
//! its messages.

const NONE: i32 = -1;

/// `struct hs_thread_datum`.
const THREAD_TYPE: u32 = 2;
const THREAD_FLAGS: u32 = 3;
const THREAD_SCRIPT_INDEX: u32 = 4;
const THREAD_SLEEP_UNTIL: u32 = 8;
const THREAD_PREVIOUS_SLEEP_UNTIL: u32 = 12;
const THREAD_STACK: u32 = 16;
const THREAD_RESULT: u32 = 20;
const THREAD_STACK_DATA: u32 = 24;
/// `HS_THREAD_STACK_SIZE`.
const STACK_SIZE: u32 = 0x200;

/// `struct hs_stack_frame`.
const FRAME_PREVIOUS: u32 = 0;
const FRAME_EXPRESSION_INDEX: u32 = 4;
const FRAME_RESULT: u32 = 8;
const FRAME_SIZE: u32 = 12;
const FRAME_DATA: u32 = 14;
const FRAME_HEADER: u32 = 16;

/// `struct hs_syntax_node`.
const NODE_INDEX: u32 = 2;
const NODE_TYPE: u32 = 4;
const NODE_FLAGS: u32 = 6;
const NODE_NEXT: u32 = 8;
const NODE_DATA: u32 = 16;

/// `struct hs_function_definition`.
const FUNCTION_RETURN_TYPE: u32 = 0;

/// `struct hs_external_global_definition`.
const EXTERNAL_TYPE: u32 = 4;
const EXTERNAL_ADDRESS: u32 = 8;

/// `struct hs_script` and `struct hs_global` (92 bytes each).
const SCRIPT_SIZE: u32 = 92;
const SCRIPT_TYPE: u32 = 32;
const SCRIPT_RETURN_TYPE: u32 = 34;
const SCRIPT_ROOT_EXPRESSION_INDEX: u32 = 36;
const GLOBAL_SIZE: u32 = 92;
const GLOBAL_TYPE: u32 = 32;
const GLOBAL_INITIALIZATION_EXPRESSION_INDEX: u32 = 40;
/// `DATUM_INDEX_NEW`'s salt for the globals.
const GLOBAL_SALT: i32 = 0xaced;

/// `struct hs_runtime_globals`.
const RUNTIME_INITIALIZED: u32 = 0;
const RUNTIME_EXECUTING_THREAD_INDEX: u32 = 2;

/// Thread types.
const THREAD_TYPE_SCRIPT: u8 = 0;
const THREAD_TYPE_GLOBAL_INITIALIZE: u8 = 1;
const THREAD_TYPE_CONSOLE_COMMAND: u8 = 2;

/// Thread flags.
const THREAD_IN_FUNCTION_CALL: u8 = 1 << 0;
const THREAD_SLEEPING: u8 = 1 << 1;
const THREAD_STACK_OVERFLOW: u8 = 1 << 2;

/// Syntax node flags.
const NODE_PRIMITIVE: i16 = 1 << 0;
const NODE_SCRIPT: i16 = 1 << 1;
const NODE_GLOBAL: i16 = 1 << 2;

/// Script types.
const SCRIPT_STARTUP: i16 = 0;
const SCRIPT_DORMANT: i16 = 1;
const SCRIPT_STATIC: i16 = 3;
const SCRIPT_STUB: i16 = 4;

/// `enum hs_type`.
const HS_UNPARSED: i16 = 0;
const HS_PASSTHROUGH: i16 = 3;
const HS_TYPE_VOID: i16 = 4;
const HS_TYPE_BOOLEAN: i16 = 5;
const HS_TYPE_REAL: i16 = 6;
const HS_TYPE_SHORT_INTEGER: i16 = 7;
const HS_TYPE_LONG_INTEGER: i16 = 8;
const HS_TYPE_STRING: i16 = 9;
const HS_TYPE_SCRIPT: i16 = 10;
const HS_TYPE_AI: i16 = 17;
const HS_TYPE_OBJECT_LIST: i16 = 23;
const HS_TYPE_OBJECT: i16 = 37;
const HS_TYPE_SCENERY: i16 = 42;
const HS_TYPE_OBJECT_NAME: i16 = 43;
const HS_TYPE_SCENERY_NAME: i16 = 48;
const NUMBER_OF_HS_TYPES: i16 = 49;

/// `hs_type_sizes`.
const HS_TYPE_SIZES: [i16; 49] = [
    0, 0, 0, 0, 0, 1, 4, 2, 4, 4, 4, 2, 2, 2, 2, 2, 2, 4, 2, 2, 2, 2, 2, 4, 4, 4, 4, 4, 4, 4, 4, 4, 2, 2, 2, 2, 2, 4,
    4, 4, 4, 4, 4, 2, 2, 2, 2, 2, 2,
];
/// `hs_object_type_masks`.
const HS_OBJECT_TYPE_MASKS: [u16; 6] = [0xFFFF, 0x0003, 0x0002, 0x0004, 0x0380, 0x0040];

/// `enum hs_function_index`'s built-ins.
mod function {
    pub const BEGIN: i16 = 0;
    pub const BEGIN_RANDOM: i16 = 1;
    pub const IF: i16 = 2;
    pub const SET: i16 = 4;
    pub const AND: i16 = 5;
    pub const OR: i16 = 6;
    pub const PLUS: i16 = 7;
    pub const MINUS: i16 = 8;
    pub const TIMES: i16 = 9;
    pub const DIVIDE: i16 = 10;
    pub const MIN: i16 = 11;
    pub const MAX: i16 = 12;
    pub const EQUAL: i16 = 13;
    pub const NOT_EQUAL: i16 = 14;
    pub const GT: i16 = 15;
    pub const LT: i16 = 16;
    pub const GTE: i16 = 17;
    pub const LTE: i16 = 18;
    pub const SLEEP: i16 = 19;
    pub const SLEEP_UNTIL: i16 = 20;
    pub const WAKE: i16 = 21;
    pub const INSPECT: i16 = 22;
    pub const OBJECT_TO_UNIT: i16 = 23;
}

/// The runtime's calls of the rest of the game (halo_sim.h's
/// `_hs_runtime_call_*`).
pub mod call {
    /// A library function's evaluation: `a` the function, `b` the thread,
    /// `c` whether it is initializing.
    pub const EXTERNAL: i32 = 1;
    /// `hs_scenario_script_disabled(a)`.
    pub const SCRIPT_DISABLED: i32 = 2;
    /// A library function's body, its arguments evaluated by the runtime:
    /// `a` the function, `b` the thread, `c` the arguments' address.
    pub const LIBRARY: i32 = 17;
    /// `game_in_progress()`.
    pub const GAME_IN_PROGRESS: i32 = 3;
    /// `object_index_from_name_index(a)`.
    pub const OBJECT_FROM_NAME: i32 = 4;
    /// `object_list_from_ai_reference(a)`.
    pub const OBJECT_LIST_FROM_AI: i32 = 5;
    /// A new object list holding the object `a`.
    pub const OBJECT_LIST_OF_OBJECT: i32 = 6;
    /// `object_list_add_reference(a)`.
    pub const OBJECT_LIST_ADD_REFERENCE: i32 = 7;
    /// `object_list_remove_reference(a)`.
    pub const OBJECT_LIST_REMOVE_REFERENCE: i32 = 8;
    /// The object `a`'s type.
    pub const OBJECT_TYPE: i32 = 9;
    /// A message: `a` 1 a stack overflow, 2 a corrupt syntax tree, 3
    /// unexpected parameters, 4 an object (`c`) that can't be the type `b`;
    /// the thread in `b` for the first three.
    pub const ERROR: i32 = 10;
    /// `inspect`'s printing of the value `b` of the type `a`.
    pub const INSPECT: i32 = 11;
    /// `object_list_gc()`.
    pub const OBJECT_LIST_GC: i32 = 13;
    /// `hs_node_gc()`.
    pub const NODE_GC: i32 = 14;
    // (15, a console command's thread deleted, is the runtime's own now)
    /// `hs_scenario_global_initializer_disabled(a)`.
    pub const GLOBAL_INITIALIZER_DISABLED: i32 = 16;
}

/// The messages' kinds.
const ERROR_STACK_OVERFLOW: i32 = 1;
const ERROR_SYNTAX: i32 = 2;
const ERROR_UNEXPECTED_PARAMETERS: i32 = 3;
const ERROR_OBJECT_CAST: i32 = 4;
/// "ran out of script threads."
const ERROR_OUT_OF_SCRIPT_THREADS: i32 = 5;
/// "there are not enough threads to execute that command."
const ERROR_OUT_OF_COMMAND_THREADS: i32 = 6;
/// A global's initialization slept (the game's assertion; `b` its thread).
const ERROR_GLOBAL_INITIALIZATION_SLEPT: i32 = 7;

/// `struct hs_function_definition`'s parse procedure, parameter count and
/// parameter types.
const FUNCTION_PARSE: u32 = 8;
const FUNCTION_PARAMETER_COUNT: u32 = 24;
const FUNCTION_PARAMETER_TYPES: u32 = 26;

/// halo_sim.h's `struct halo_sim_hs_call`.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct HsCall {
    pub kind: i32,
    pub a: i32,
    pub b: i32,
    pub c: i32,
    pub result: i32,
}

/// The game's memory, where the runtime's state lives.
pub trait HsMemory {
    fn r8(&self, address: u32) -> u8;
    fn r16(&self, address: u32) -> u16;
    fn r32(&self, address: u32) -> u32;
    fn w8(&self, address: u32, value: u8);
    fn w16(&self, address: u32, value: u16);
    fn w32(&self, address: u32, value: u32);
}

/// The runtime's memory as the data arrays' ([`crate::data_array`]).
struct AsAiMemory<'a>(&'a dyn HsMemory);

impl crate::ai_actor::AiMemory for AsAiMemory<'_> {
    fn r8(&self, address: u32) -> u8 {
        self.0.r8(address)
    }
    fn r16(&self, address: u32) -> u16 {
        self.0.r16(address)
    }
    fn r32(&self, address: u32) -> u32 {
        self.0.r32(address)
    }
    fn w8(&self, address: u32, value: u8) {
        self.0.w8(address, value)
    }
    fn w16(&self, address: u32, value: u16) {
        self.0.w16(address, value)
    }
    fn w32(&self, address: u32, value: u32) {
        self.0.w32(address, value)
    }
}

/// The game's side of the runtime.
pub trait HsHost {
    fn memory(&self) -> &dyn HsMemory;
    fn call(&self, call: &mut HsCall);
}

/// Where the runtime's state and tables are in the game's memory.
#[derive(Debug, Clone, Copy, Default)]
pub struct HsContext {
    /// `hs_thread_data`, `hs_syntax_data`, `hs_global_data` (their
    /// `struct data_array`s).
    pub thread_data: u32,
    pub syntax_data: u32,
    pub global_data: u32,
    /// `hs_function_table.functions` and its count.
    pub functions: u32,
    pub function_count: i32,
    /// `hs_external_globals` and its count.
    pub external_globals: u32,
    pub external_global_count: i32,
    /// The scenario's scripts and globals (their elements) and counts.
    pub scripts: u32,
    pub script_count: i32,
    pub globals: u32,
    pub global_count: i32,
    /// `hs_runtime_globals`.
    pub runtime_globals: u32,
    /// Whether a scenario is loaded (`global_scenario_index != NONE`).
    pub scenario_loaded: bool,
    /// `get_global_random_seed_address()`.
    pub random_seed: u32,
    /// `game_time_get()`.
    pub game_time: i32,
    /// `_hs_type_string_default` (the empty string's address).
    pub string_default: u32,
    /// `hs_macro_function_parse`: a library function it parses has its
    /// arguments evaluated by the runtime, then its body ([`call::LIBRARY`]).
    pub macro_parse: u32,
}

/// `(long)value` as x86 truncates it.
fn c_long(value: f32) -> i32 {
    if (-2_147_483_648.0..2_147_483_648.0).contains(&value) { value as i32 } else { i32::MIN }
}

struct Runtime<'a, H: HsHost> {
    context: &'a HsContext,
    host: &'a H,
}

impl<H: HsHost> Runtime<'_, H> {
    fn m(&self) -> &dyn HsMemory {
        self.host.memory()
    }
    fn call(&self, kind: i32, a: i32, b: i32, c: i32) -> i32 {
        let mut call = HsCall { kind, a, b, c, result: 0 };
        self.host.call(&mut call);
        call.result
    }

    /// `datum_get`: the datum's address (None for one unused or changed).
    fn datum_get(&self, data: u32, index: i32) -> Option<u32> {
        crate::data_array::datum_get(&AsAiMemory(self.m()), data, index)
    }

    /// `data_next_index`.
    fn data_next_index(&self, data: u32, index: i32) -> i32 {
        crate::data_array::data_next_index(&AsAiMemory(self.m()), data, index)
    }

    /// `datum_new`.
    fn datum_new(&self, data: u32) -> i32 {
        crate::data_array::datum_new(&AsAiMemory(self.m()), data)
    }

    /// `datum_new_at_index`.
    fn datum_new_at_index(&self, data: u32, index: i32) -> i32 {
        crate::data_array::datum_new_at_index(&AsAiMemory(self.m()), data, index)
    }

    /// `datum_delete`.
    fn datum_delete(&self, data: u32, index: i32) {
        crate::data_array::datum_delete(&AsAiMemory(self.m()), data, index)
    }

    /// `data_make_valid` (with `data_delete_all`).
    fn data_make_valid(&self, data: u32) {
        crate::data_array::data_make_valid(&AsAiMemory(self.m()), data)
    }

    /// `hs_thread_new`.
    fn thread_new(&self, thread_type: u8, script_index: i32) -> i32 {
        let m = self.m();
        let thread_index = self.datum_new(self.context.thread_data);
        if thread_index != NONE {
            let thread = self.thread(thread_index);
            let base = thread + THREAD_STACK_DATA;
            m.w32(thread + THREAD_STACK, base);
            m.w32(base + FRAME_PREVIOUS, 0);
            m.w16(base + FRAME_SIZE, 0);
            m.w32(base + FRAME_EXPRESSION_INDEX, NONE as u32);
            m.w8(thread + THREAD_TYPE, thread_type);
            m.w32(thread + THREAD_SCRIPT_INDEX, script_index as u32);
            m.w8(thread + THREAD_FLAGS, 0);
            let dormant =
                script_index != NONE && m.r16(self.script(script_index) + SCRIPT_TYPE) as i16 == SCRIPT_DORMANT;
            m.w32(thread + THREAD_SLEEP_UNTIL, if dormant { (NONE - 1) as u32 } else { 0 });
        }
        thread_index
    }

    /// `hs_thread_delete`.
    fn thread_delete(&self, thread_index: i32) {
        self.datum_delete(self.context.thread_data, thread_index);
    }

    fn thread(&self, thread_index: i32) -> u32 {
        self.datum_get(self.context.thread_data, thread_index).unwrap_or(0)
    }
    fn syntax(&self, expression_index: i32) -> Option<u32> {
        self.datum_get(self.context.syntax_data, expression_index)
    }
    fn node_index(&self, expression_index: i32) -> i16 {
        self.syntax(expression_index).map_or(0, |n| self.m().r16(n + NODE_INDEX) as i16)
    }
    fn node_type(&self, expression_index: i32) -> i16 {
        self.syntax(expression_index).map_or(0, |n| self.m().r16(n + NODE_TYPE) as i16)
    }
    fn node_next(&self, expression_index: i32) -> i32 {
        self.syntax(expression_index).map_or(NONE, |n| self.m().r32(n + NODE_NEXT) as i32)
    }
    fn node_data(&self, expression_index: i32) -> i32 {
        self.syntax(expression_index).map_or(0, |n| self.m().r32(n + NODE_DATA) as i32)
    }

    fn stack(&self, thread: u32) -> u32 {
        self.m().r32(thread + THREAD_STACK)
    }
    fn frame_expression(&self, thread: u32) -> i32 {
        self.m().r32(self.stack(thread) + FRAME_EXPRESSION_INDEX) as i32
    }
    /// The first argument's node of the frame's expression (the function's
    /// name node's next).
    fn first_argument(&self, thread: u32) -> i32 {
        self.node_next(self.node_data(self.frame_expression(thread)))
    }
    fn flags(&self, thread: u32) -> u8 {
        self.m().r8(thread + THREAD_FLAGS)
    }
    fn set_flag(&self, thread: u32, flag: u8, value: bool) {
        let flags = self.flags(thread);
        self.m().w8(thread + THREAD_FLAGS, if value { flags | flag } else { flags & !flag });
    }

    fn script(&self, script_index: i32) -> u32 {
        self.context.scripts + SCRIPT_SIZE * script_index as u32
    }
    fn function_definition(&self, function_index: i16) -> u32 {
        self.m().r32(self.context.functions + 4 * function_index as u32)
    }

    /// `hs_stack_overflow`.
    fn stack_overflow(&self, thread_index: i32) {
        let thread = self.thread(thread_index);
        if self.flags(thread) & THREAD_STACK_OVERFLOW == 0 {
            self.call(call::ERROR, ERROR_STACK_OVERFLOW, thread_index, 0);
            self.set_flag(thread, THREAD_STACK_OVERFLOW, true);
        }
    }

    /// `hs_syntax_error`.
    fn syntax_error(&self, thread_index: i32) {
        let thread = self.thread(thread_index);
        if self.flags(thread) & THREAD_STACK_OVERFLOW == 0 {
            self.call(call::ERROR, ERROR_SYNTAX, thread_index, 0);
            self.set_flag(thread, THREAD_STACK_OVERFLOW, true);
        }
    }

    /// `hs_stack_push`.
    fn stack_push(&self, thread_index: i32) -> bool {
        let m = self.m();
        let thread = self.thread(thread_index);
        let stack = self.stack(thread);
        let size = m.r16(stack + FRAME_SIZE) as i16 as i32 as u32;
        let new_frame = stack.wrapping_add(size).wrapping_add(FRAME_HEADER);
        if new_frame.wrapping_add(FRAME_HEADER) >= thread + THREAD_STACK_DATA + STACK_SIZE
            || self.flags(thread) & THREAD_STACK_OVERFLOW != 0
        {
            self.stack_overflow(thread_index);
            return false;
        }
        m.w32(new_frame + FRAME_PREVIOUS, stack);
        m.w32(thread + THREAD_STACK, new_frame);
        m.w16(new_frame + FRAME_SIZE, 0);
        true
    }

    /// `hs_stack_allocate`: the space's address, or None (a stack overflow).
    fn stack_allocate(&self, thread_index: i32, size: i32) -> Option<u32> {
        let m = self.m();
        let thread = self.thread(thread_index);
        let frame = self.stack(thread);
        let frame_size = m.r16(frame + FRAME_SIZE) as i16 as i32;
        let end = (thread + THREAD_STACK_DATA + STACK_SIZE) as i64;
        let used = (frame + FRAME_DATA) as i64 + frame_size as i64;
        if size < 0 || size as i64 > end - used || self.flags(thread) & THREAD_STACK_OVERFLOW != 0 {
            self.stack_overflow(thread_index);
            return None;
        }
        let result = (frame + FRAME_DATA).wrapping_add(frame_size as u32);
        m.w16(frame + FRAME_SIZE, (frame_size + size) as i16 as u16);
        Some(result)
    }

    /// `hs_stack_pop`.
    fn stack_pop(&self, thread_index: i32) {
        let thread = self.thread(thread_index);
        let previous = self.m().r32(self.stack(thread) + FRAME_PREVIOUS);
        self.m().w32(thread + THREAD_STACK, previous);
    }

    /// `hs_global_get_type`.
    fn global_type(&self, designator: i16) -> i16 {
        let index = (designator as i32) & 0x7FFF;
        if designator as u16 & 0x8000 != 0 {
            if index >= self.context.external_global_count {
                return HS_UNPARSED;
            }
            let external = self.m().r32(self.context.external_globals + 4 * index as u32);
            return self.m().r16(external + EXTERNAL_TYPE) as i16;
        }
        if index >= self.context.global_count {
            return HS_UNPARSED;
        }
        self.m().r16(self.context.globals + GLOBAL_SIZE * index as u32 + GLOBAL_TYPE) as i16
    }

    /// The global's datum (its value at +4).
    fn global_datum(&self, designator: i16) -> u32 {
        let index = if designator as u16 & 0x8000 != 0 {
            (designator as i32) & 0x7FFF
        } else {
            ((designator as i32) & 0x7FFF) + self.context.external_global_count
        };
        self.datum_get(self.context.global_data, index).unwrap_or(0)
    }

    /// `hs_global_reconcile_read`.
    fn global_reconcile_read(&self, designator: i16) {
        if designator as u16 & 0x8000 == 0 {
            return;
        }
        let m = self.m();
        let global = self.global_datum(designator);
        let index = (designator as i32) & 0x7FFF;
        let external = m.r32(self.context.external_globals + 4 * index as u32);
        let address = m.r32(external + EXTERNAL_ADDRESS);
        let value = global + 4;
        let ty = self.global_type(designator);
        match external_width(ty) {
            1 => m.w8(value, if address != 0 { m.r8(address) } else { 0 }),
            2 => {
                let default = if ty == HS_TYPE_SHORT_INTEGER { 0 } else { NONE as u16 };
                m.w16(value, if address != 0 { m.r16(address) } else { default })
            }
            _ => {
                let default = match ty {
                    HS_TYPE_REAL | HS_TYPE_LONG_INTEGER => 0,
                    HS_TYPE_STRING => self.context.string_default,
                    _ => NONE as u32,
                };
                m.w32(value, if address != 0 { m.r32(address) } else { default })
            }
        }
    }

    /// `hs_global_reconcile_write`.
    fn global_reconcile_write(&self, designator: i16) {
        if designator as u16 & 0x8000 == 0 {
            return;
        }
        let m = self.m();
        let global = self.global_datum(designator);
        let index = (designator as i32) & 0x7FFF;
        let external = m.r32(self.context.external_globals + 4 * index as u32);
        let address = m.r32(external + EXTERNAL_ADDRESS);
        if address == 0 {
            return;
        }
        let value = m.r32(global + 4);
        match external_width(self.global_type(designator)) {
            1 => m.w8(address, value as u8),
            2 => m.w16(address, value as u16),
            _ => m.w32(address, value),
        }
    }

    /// `hs_global_evaluate`.
    fn global_evaluate(&self, designator: i16) -> i32 {
        self.global_reconcile_read(designator);
        self.m().r32(self.global_datum(designator) + 4) as i32
    }

    /// The `typecasting_procedures` table, as the game's lays it out (its
    /// rows past the integers sit two types early against `enum hs_type`:
    /// the real casts are from types 30 to 33, the object list's from types
    /// 16, 34 to 38 and 39 to 44), and the cast's result.
    fn typecast(&self, desired: i16, actual: i16, value: i32) -> i32 {
        let n = value as u32;
        match (desired, actual) {
            (HS_TYPE_VOID, 5..=48) => 0,
            // (the casts to a boolean or a short write only their low byte
            // or word: the rest is the argument's, as January leaves it)
            (HS_TYPE_BOOLEAN, HS_TYPE_REAL) | (HS_TYPE_BOOLEAN, HS_TYPE_LONG_INTEGER) => {
                ((n & 0xFFFF_FF00) | (n == 0) as u32) as i32
            }
            (HS_TYPE_BOOLEAN, HS_TYPE_SHORT_INTEGER) => ((n & 0xFFFF_FF00) | ((n as i16) == 0) as u32) as i32,
            // (inlined, January's upper bytes are zero)
            (HS_TYPE_BOOLEAN, HS_TYPE_STRING) => (self.string_length(n) == 0) as i32,
            (HS_TYPE_REAL, HS_TYPE_SHORT_INTEGER) => ((value as i16) as f32).to_bits() as i32,
            (HS_TYPE_REAL, HS_TYPE_LONG_INTEGER) => (value as f32).to_bits() as i32,
            (HS_TYPE_REAL, 30..=33) => (((value as i16) as i32 + 1) as f32).to_bits() as i32,
            (HS_TYPE_SHORT_INTEGER, HS_TYPE_REAL) => {
                ((n & 0xFFFF_0000) | (c_long(f32::from_bits(n)) as u16 as u32)) as i32
            }
            (HS_TYPE_SHORT_INTEGER, HS_TYPE_LONG_INTEGER) => value,
            (HS_TYPE_LONG_INTEGER, HS_TYPE_REAL) => c_long(f32::from_bits(n)),
            (HS_TYPE_OBJECT_LIST, 16) => self.call(call::OBJECT_LIST_FROM_AI, value, 0, 0),
            (HS_TYPE_OBJECT_LIST, 34..=38) => {
                if value != NONE {
                    self.call(call::OBJECT_LIST_OF_OBJECT, value, 0, 0)
                } else {
                    NONE
                }
            }
            (HS_TYPE_OBJECT_LIST, 39..=44) => {
                let object_index = self.call(call::OBJECT_FROM_NAME, value as i16 as i32, 0, 0);
                if object_index != NONE {
                    self.call(call::OBJECT_LIST_OF_OBJECT, object_index, 0, 0)
                } else {
                    NONE
                }
            }
            // (no cast: the game's table has none, and hs_can_cast keeps the
            // compiled scripts from asking)
            _ => value,
        }
    }

    fn string_length(&self, address: u32) -> u32 {
        let mut length = 0;
        while self.m().r8(address.wrapping_add(length)) != 0 {
            length += 1;
        }
        length
    }

    /// `hs_cast`.
    fn cast(&self, actual: i16, desired: i16, value: i32) -> i32 {
        let is_object = |t: i16| (HS_TYPE_OBJECT..=HS_TYPE_SCENERY).contains(&t);
        let is_object_name = |t: i16| (HS_TYPE_OBJECT_NAME..=HS_TYPE_SCENERY_NAME).contains(&t);
        if actual != desired && actual != HS_PASSTHROUGH && !is_object_name(desired) {
            if !is_object(desired) {
                return self.typecast(desired, actual, value);
            } else if is_object_name(actual) {
                return self.call(call::OBJECT_FROM_NAME, value as i16 as i32, 0, 0);
            }
        }
        value
    }

    /// `hs_return`.
    fn hs_return(&self, thread_index: i32, value: i32) {
        let m = self.m();
        let thread = self.thread(thread_index);
        let expression = self.frame_expression(thread);
        let node = self.syntax(expression).unwrap_or(0);
        let flags = m.r16(node + NODE_FLAGS) as i16;
        let index = m.r16(node + NODE_INDEX) as i16;
        let return_type = if flags & NODE_SCRIPT == 0 {
            m.r16(self.function_definition(index) + FUNCTION_RETURN_TYPE) as i16
        } else {
            m.r16(self.script(index as i32) + SCRIPT_RETURN_TYPE) as i16
        };
        let node_type = m.r16(node + NODE_TYPE) as i16;
        let previous = m.r32(self.stack(thread) + FRAME_PREVIOUS);
        let destination = m.r32(previous + FRAME_RESULT);
        let cast = self.cast(return_type, node_type, value);
        m.w32(destination, cast as u32);
        let thread = self.thread(thread_index);
        let previous = m.r32(self.stack(thread) + FRAME_PREVIOUS);
        m.w32(thread + THREAD_STACK, previous);
    }

    /// `hs_evaluate`.
    fn evaluate(&self, thread_index: i32, expression_index: i32, destination: u32) {
        let m = self.m();
        let thread = self.thread(thread_index);
        let Some(node) = self.syntax(expression_index) else {
            self.syntax_error(thread_index);
            return;
        };
        let node_type = m.r16(node + NODE_TYPE) as i16;
        if !(HS_TYPE_VOID..NUMBER_OF_HS_TYPES).contains(&node_type) {
            self.syntax_error(thread_index);
            return;
        }
        let flags = m.r16(node + NODE_FLAGS) as i16;
        if flags & NODE_PRIMITIVE != 0 {
            let data = m.r32(node + NODE_DATA) as i32;
            let value = if flags & NODE_GLOBAL != 0 {
                let designator = data as i16;
                let global_type = self.global_type(designator);
                let value = self.global_evaluate(designator);
                self.cast(global_type, node_type, value)
            } else {
                self.cast(m.r16(node + NODE_INDEX) as i16, node_type, data)
            };
            m.w32(destination, value as u32);
        } else {
            m.w32(self.stack(thread) + FRAME_RESULT, destination);
            if self.stack_push(thread_index) {
                self.set_flag(thread, THREAD_IN_FUNCTION_CALL, true);
                m.w32(self.stack(thread) + FRAME_EXPRESSION_INDEX, expression_index as u32);
            }
        }
    }

    /// `hs_arguments_evaluate`: the arguments' values' address once all are
    /// evaluated, or None.
    fn arguments_evaluate(&self, thread_index: i32, parameter_types: &[i16], initialize: bool) -> Option<u32> {
        let m = self.m();
        let thread = self.thread(thread_index);
        let values = self.stack_allocate(thread_index, 4 * parameter_types.len() as i32);
        let argument_index = self.stack_allocate(thread_index, 2);
        let expression_index = self.stack_allocate(thread_index, 4);
        let (Some(values), Some(argument_index), Some(expression_index)) = (values, argument_index, expression_index)
        else {
            return None;
        };
        if initialize {
            m.w16(argument_index, 0);
            m.w32(expression_index, self.first_argument(thread) as u32);
        }
        let argument = m.r16(argument_index) as i16;
        if (argument as usize) < parameter_types.len() {
            let expression = m.r32(expression_index) as i32;
            if self.node_type(expression) != parameter_types[argument as usize] {
                self.call(call::ERROR, ERROR_UNEXPECTED_PARAMETERS, thread_index, 0);
                return Some(values);
            }
            self.evaluate(thread_index, expression, values + 4 * argument as u32);
            m.w32(expression_index, self.node_next(expression) as u32);
            m.w16(argument_index, (argument + 1) as u16);
            return None;
        }
        Some(values)
    }

    /// `hs_find_thread_by_script`.
    fn find_thread_by_script(&self, script_index: i16) -> i32 {
        let mut thread_index = self.data_next_index(self.context.thread_data, NONE);
        while thread_index != NONE {
            let thread = self.thread(thread_index);
            if self.m().r32(thread + THREAD_SCRIPT_INDEX) as i32 == script_index as i32 {
                return thread_index;
            }
            thread_index = self.data_next_index(self.context.thread_data, thread_index);
        }
        NONE
    }

    /// `hs_wake`.
    fn wake(&self, thread_index: i32) {
        let m = self.m();
        let thread = self.thread(thread_index);
        if m.r32(thread + THREAD_SLEEP_UNTIL) as i32 != NONE {
            m.w32(thread + THREAD_SLEEP_UNTIL, 0);
            if self.flags(thread) & THREAD_SLEEPING != 0 {
                m.w32(thread + THREAD_SLEEP_UNTIL, m.r32(thread + THREAD_PREVIOUS_SLEEP_UNTIL));
                self.set_flag(thread, THREAD_SLEEPING, false);
            } else {
                let stack = self.stack(thread);
                let expression = m.r32(stack + FRAME_EXPRESSION_INDEX) as i32;
                if expression != NONE && self.node_index(expression) == function::SLEEP_UNTIL {
                    self.stack_pop(thread_index);
                    return;
                }
                let previous = m.r32(stack + FRAME_PREVIOUS);
                if previous != 0 {
                    let previous_expression = m.r32(previous + FRAME_EXPRESSION_INDEX) as i32;
                    if previous_expression != NONE && self.node_index(previous_expression) == function::SLEEP_UNTIL {
                        self.stack_pop(thread_index);
                        self.stack_pop(thread_index);
                        self.set_flag(thread, THREAD_IN_FUNCTION_CALL, false);
                    }
                }
            }
        }
    }

    // ---------- the built-in functions

    fn evaluate_begin(&self, thread_index: i32, initialize: bool) {
        let m = self.m();
        let thread = self.thread(thread_index);
        let expression_index = self.stack_allocate(thread_index, 4);
        let result = self.stack_allocate(thread_index, 4);
        let (Some(expression_index), Some(result)) = (expression_index, result) else { return };
        if initialize {
            m.w32(expression_index, self.first_argument(thread) as u32);
            m.w32(result, 0);
        }
        let expression = m.r32(expression_index) as i32;
        if expression != NONE {
            self.evaluate(thread_index, expression, result);
            m.w32(expression_index, self.node_next(expression) as u32);
        } else {
            self.hs_return(thread_index, m.r32(result) as i32);
        }
    }

    fn evaluate_begin_random(&self, thread_index: i32, initialize: bool) {
        let m = self.m();
        let thread = self.thread(thread_index);
        let argument_count = self.stack_allocate(thread_index, 2);
        let evaluated = self.stack_allocate(thread_index, 4);
        let result = self.stack_allocate(thread_index, 4);
        let (Some(argument_count), Some(evaluated), Some(result)) = (argument_count, evaluated, result) else { return };
        if initialize {
            let mut expression = self.first_argument(thread);
            let mut count: i16 = 0;
            while expression != NONE {
                expression = self.node_next(expression);
                count += 1;
            }
            m.w16(argument_count, count as u16);
            // BIT_VECTOR_SIZE_IN_BYTES
            let bytes = ((count as i32 + 31) >> 5) << 2;
            for i in 0..bytes.max(0) as u32 {
                m.w8(evaluated + i, 0);
            }
        }
        let count = m.r16(argument_count) as i16;
        // random_range(0, count): seed_random_range on the global seed
        let seed = m.r32(self.context.random_seed).wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        m.w32(self.context.random_seed, seed);
        let random = ((count as i32 as u32).wrapping_mul((seed >> 16) as u16 as u32) >> 16) as i16;
        let mut index: i16 = 0;
        while index < count {
            let choice = ((random as i32 + index as i32) % count as i32) as i16;
            let word = evaluated + 4 * (choice as u32 >> 5);
            let bit = 1u32 << (choice as u32 & 31);
            if m.r32(word) & bit == 0 {
                let mut expression = self.first_argument(thread);
                for _ in 0..choice {
                    expression = self.node_next(expression);
                }
                self.evaluate(thread_index, expression, result);
                m.w32(word, m.r32(word) | bit);
                break;
            }
            index += 1;
        }
        if index == count {
            self.hs_return(thread_index, m.r32(result) as i32);
        }
    }

    fn evaluate_if(&self, thread_index: i32, initialize: bool) {
        let m = self.m();
        let thread = self.thread(thread_index);
        let condition = self.stack_allocate(thread_index, 4);
        let expression_index = self.stack_allocate(thread_index, 4);
        let result = self.stack_allocate(thread_index, 4);
        let (Some(condition), Some(expression_index), Some(result)) = (condition, expression_index, result) else {
            return;
        };
        if initialize {
            m.w32(condition, 0);
            m.w32(expression_index, NONE as u32);
            self.evaluate(thread_index, self.first_argument(thread), condition);
        } else if m.r32(expression_index) as i32 == NONE {
            let then = self.node_next(self.first_argument(thread));
            if m.r8(condition) != 0 {
                m.w32(expression_index, then as u32);
            } else {
                let otherwise = self.node_next(then);
                m.w32(expression_index, otherwise as u32);
                if otherwise == NONE {
                    self.hs_return(thread_index, 0);
                    return;
                }
            }
            self.evaluate(thread_index, m.r32(expression_index) as i32, result);
        } else {
            self.hs_return(thread_index, m.r32(result) as i32);
        }
    }

    fn evaluate_set(&self, thread_index: i32, initialize: bool) {
        let m = self.m();
        let thread = self.thread(thread_index);
        let variable_expression = self.first_argument(thread);
        let variable = if variable_expression != NONE { self.syntax(variable_expression) } else { None };
        let Some(variable) = variable else {
            self.syntax_error(thread_index);
            return;
        };
        let flags = m.r16(variable + NODE_FLAGS) as i16;
        let designator = m.r32(variable + NODE_DATA) as i16;
        if flags & NODE_PRIMITIVE == 0 || flags & NODE_GLOBAL == 0 || self.global_type(designator) == HS_UNPARSED {
            self.syntax_error(thread_index);
            return;
        }
        if self.stack_allocate(thread_index, 4).is_none() {
            return;
        }
        let ty = self.global_type(designator);
        if initialize {
            if ty == HS_TYPE_OBJECT_LIST {
                let list = self.global_evaluate(designator);
                self.call(call::OBJECT_LIST_REMOVE_REFERENCE, list, 0, 0);
            }
            let value = self.global_datum(designator) + 4;
            self.evaluate(thread_index, self.node_next(variable_expression), value);
        } else {
            self.global_reconcile_write(designator);
            if ty == HS_TYPE_OBJECT_LIST {
                let list = self.global_evaluate(designator);
                self.call(call::OBJECT_LIST_ADD_REFERENCE, list, 0, 0);
            }
            let value = self.global_evaluate(designator);
            self.hs_return(thread_index, value);
        }
    }

    fn evaluate_logical(&self, function_index: i16, thread_index: i32, initialize: bool) {
        let m = self.m();
        let thread = self.thread(thread_index);
        let expression_index = self.stack_allocate(thread_index, 4);
        let value = self.stack_allocate(thread_index, 4);
        let result = self.stack_allocate(thread_index, 1);
        let and = function_index == function::AND;
        let (Some(expression_index), Some(value), Some(result)) = (expression_index, value, result) else { return };
        if initialize {
            m.w32(expression_index, self.first_argument(thread) as u32);
            m.w8(result, and as u8);
        } else {
            let argument = m.r8(value) != 0;
            let current = m.r8(result) != 0;
            m.w8(result, if and { current && argument } else { current || argument } as u8);
        }
        let expression = m.r32(expression_index) as i32;
        if expression != NONE && (m.r8(result) != 0) == and {
            self.evaluate(thread_index, expression, value);
            m.w32(expression_index, self.node_next(expression) as u32);
        } else {
            // (the upper bytes zero: January's were the initialize argument's
            // residue)
            self.hs_return(thread_index, m.r8(result) as i32);
        }
    }

    fn evaluate_arithmetic(&self, function_index: i16, thread_index: i32, initialize: bool) {
        let m = self.m();
        let thread = self.thread(thread_index);
        let argument_index = self.stack_allocate(thread_index, 2);
        let expression_index = self.stack_allocate(thread_index, 4);
        let value = self.stack_allocate(thread_index, 4);
        let result = self.stack_allocate(thread_index, 4);
        let (Some(argument_index), Some(expression_index), Some(value), Some(result)) =
            (argument_index, expression_index, value, result)
        else {
            return;
        };
        if initialize {
            m.w16(argument_index, 0);
            m.w32(expression_index, self.first_argument(thread) as u32);
        } else {
            let argument = f32::from_bits(m.r32(value));
            if m.r16(argument_index) == 0 {
                m.w32(result, argument.to_bits());
            } else {
                let current = f32::from_bits(m.r32(result));
                let combined = match function_index {
                    function::PLUS => current + argument,
                    function::MINUS => current - argument,
                    function::TIMES => current * argument,
                    function::DIVIDE => current / argument,
                    // MIN and MAX as the macros compare
                    function::MIN => {
                        if current < argument {
                            current
                        } else {
                            argument
                        }
                    }
                    function::MAX => {
                        if current > argument {
                            current
                        } else {
                            argument
                        }
                    }
                    _ => current,
                };
                m.w32(result, combined.to_bits());
            }
            m.w16(argument_index, m.r16(argument_index).wrapping_add(1));
        }
        let expression = m.r32(expression_index) as i32;
        if expression != NONE {
            self.evaluate(thread_index, expression, value);
            m.w32(expression_index, self.node_next(expression) as u32);
        } else {
            self.hs_return(thread_index, m.r32(result) as i32);
        }
    }

    fn evaluate_equality(&self, function_index: i16, thread_index: i32, initialize: bool) {
        let m = self.m();
        let thread = self.thread(thread_index);
        let ty = self.node_type(self.first_argument(thread));
        if let Some(arguments) = self.arguments_evaluate(thread_index, &[ty, ty], initialize) {
            let size = HS_TYPE_SIZES.get(ty as usize).copied().unwrap_or(0) as u32;
            let mut equal = (0..size).all(|i| m.r8(arguments + i) == m.r8(arguments + 4 + i));
            if function_index == function::NOT_EQUAL {
                equal = !equal;
            }
            // (the upper bytes as January leaves them: the type, twice in the
            // argument slot)
            self.hs_return(thread_index, ((ty as i32) << 16) | equal as i32);
        }
    }

    fn evaluate_inequality(&self, function_index: i16, thread_index: i32, initialize: bool) {
        let m = self.m();
        let thread = self.thread(thread_index);
        let ty = self.node_type(self.first_argument(thread));
        if let Some(arguments) = self.arguments_evaluate(thread_index, &[ty, ty], initialize) {
            let (a, b) = (m.r32(arguments), m.r32(arguments + 4));
            let (value0, value1) = match ty {
                HS_TYPE_REAL => (f32::from_bits(a), f32::from_bits(b)),
                HS_TYPE_LONG_INTEGER => (a as i32 as f32, b as i32 as f32),
                _ => ((a as i16) as f32, (b as i16) as f32),
            };
            let comparison = match function_index {
                function::GT => value0 > value1,
                function::LT => value0 < value1,
                function::GTE => value0 >= value1,
                function::LTE => value0 <= value1,
                _ => false,
            };
            // (the upper bytes as January leaves them: value1's)
            self.hs_return(thread_index, ((value1.to_bits() & 0xFFFF_FF00) | comparison as u32) as i32);
        }
    }

    fn evaluate_sleep(&self, thread_index: i32, initialize: bool) {
        let m = self.m();
        let thread = self.thread(thread_index);
        let ticks = self.stack_allocate(thread_index, 4);
        let script_index = self.stack_allocate(thread_index, 4);
        let argument_index = self.stack_allocate(thread_index, 2);
        let (Some(ticks), Some(script_index), Some(argument_index)) = (ticks, script_index, argument_index) else {
            return;
        };
        if initialize {
            self.evaluate(thread_index, self.first_argument(thread), ticks);
            m.w16(argument_index, 0);
            return;
        }
        if m.r16(argument_index) == 0 {
            let expression = self.node_next(self.first_argument(thread));
            m.w16(argument_index, 1);
            if expression != NONE {
                self.evaluate(thread_index, expression, script_index);
                return;
            }
            m.w32(script_index, NONE as u32);
        }
        if m.r16(argument_index) != 0 {
            let sleep_ticks = m.r32(ticks) as i16;
            if sleep_ticks != 0 {
                let mut sleep_thread_index = thread_index;
                if m.r32(script_index) as i16 != NONE as i16 {
                    sleep_thread_index = self.find_thread_by_script(m.r32(script_index) as i16);
                }
                if sleep_thread_index != NONE {
                    let sleep_thread = self.thread(sleep_thread_index);
                    let sleep_until =
                        if sleep_ticks < 0 { NONE - 1 } else { self.context.game_time.wrapping_add(sleep_ticks as i32) };
                    if m.r32(sleep_thread + THREAD_SLEEP_UNTIL) as i32 != NONE {
                        if sleep_thread_index != thread_index && self.flags(sleep_thread) & THREAD_SLEEPING == 0 {
                            self.set_flag(sleep_thread, THREAD_SLEEPING, true);
                            m.w32(sleep_thread + THREAD_PREVIOUS_SLEEP_UNTIL, m.r32(sleep_thread + THREAD_SLEEP_UNTIL));
                        }
                        m.w32(self.thread(sleep_thread_index) + THREAD_SLEEP_UNTIL, sleep_until as u32);
                    }
                }
            }
            self.hs_return(thread_index, 0);
        }
    }

    fn evaluate_sleep_until(&self, thread_index: i32, initialize: bool) {
        let m = self.m();
        let thread = self.thread(thread_index);
        let condition = self.stack_allocate(thread_index, 4);
        let ticks = self.stack_allocate(thread_index, 4);
        let timeout = self.stack_allocate(thread_index, 4);
        let start_time = self.stack_allocate(thread_index, 4);
        let argument_index = self.stack_allocate(thread_index, 2);
        let expression_index = self.node_next(self.first_argument(thread));
        let (Some(condition), Some(ticks), Some(timeout), Some(start_time), Some(argument_index)) =
            (condition, ticks, timeout, start_time, argument_index)
        else {
            return;
        };
        let game_time = self.context.game_time;
        if initialize {
            m.w8(condition, 0);
            m.w32(start_time, game_time as u32);
            m.w16(argument_index, 0);
            m.w16(ticks, 30);
            m.w32(timeout, NONE as u32);
            if expression_index != NONE {
                self.evaluate(thread_index, expression_index, ticks);
                return;
            }
        }
        if m.r16(argument_index) == 0 {
            m.w16(argument_index, 1);
            if expression_index != NONE {
                let timeout_expression = self.node_next(expression_index);
                if timeout_expression != NONE {
                    self.evaluate(thread_index, timeout_expression, timeout);
                    return;
                }
            }
        }
        if m.r16(argument_index) == 1 {
            let timeout_value = m.r32(timeout) as i32;
            let start = m.r32(start_time) as i32;
            if m.r8(condition) != 0 || (timeout_value != NONE && game_time >= timeout_value.wrapping_add(start)) {
                self.hs_return(thread_index, 0);
            } else {
                self.evaluate(thread_index, self.first_argument(thread), condition);
                let thread = self.thread(thread_index);
                let ticks = m.r16(ticks) as i16;
                let mut sleep_until = game_time.wrapping_add(if 1 > ticks { 1 } else { ticks as i32 });
                if timeout_value != NONE {
                    let limit = timeout_value.wrapping_add(start);
                    sleep_until = if limit < sleep_until { limit } else { sleep_until };
                }
                m.w32(thread + THREAD_SLEEP_UNTIL, sleep_until as u32);
            }
        }
    }

    fn evaluate_wake(&self, thread_index: i32) {
        let thread = self.thread(thread_index);
        let script_name_node = self.node_next(self.node_data(self.frame_expression(thread)));
        let wake_thread_index = self.find_thread_by_script(self.node_data(script_name_node) as i16);
        if wake_thread_index != NONE {
            self.wake(wake_thread_index);
        }
        self.hs_return(thread_index, 0);
    }

    fn evaluate_inspect(&self, thread_index: i32, initialize: bool) {
        let m = self.m();
        let thread = self.thread(thread_index);
        let Some(value) = self.stack_allocate(thread_index, 4) else { return };
        if initialize {
            self.evaluate(thread_index, self.first_argument(thread), value);
        } else {
            let ty = self.node_type(self.first_argument(thread));
            self.call(call::INSPECT, ty as i32, m.r32(value) as i32, 0);
            self.hs_return(thread_index, 0);
        }
    }

    fn evaluate_object_cast_up(&self, function_index: i16, thread_index: i32, initialize: bool) {
        let m = self.m();
        let thread = self.thread(thread_index);
        let Some(object_index) = self.stack_allocate(thread_index, 4) else { return };
        if initialize {
            self.evaluate(thread_index, self.first_argument(thread), object_index);
        } else if m.r32(object_index) as i32 != NONE {
            let object = m.r32(object_index) as i32;
            let object_type = function_index - function::INSPECT;
            let ty = self.call(call::OBJECT_TYPE, object, 0, 0);
            let mask = HS_OBJECT_TYPE_MASKS.get(object_type as usize).copied().unwrap_or(0) as i16;
            if mask as i32 & (1 << (ty & 31)) != 0 {
                self.hs_return(thread_index, object);
            } else {
                self.call(call::ERROR, ERROR_OBJECT_CAST, object_type as i32 + HS_TYPE_OBJECT as i32, object);
                self.hs_return(thread_index, NONE);
            }
        } else {
            self.hs_return(thread_index, NONE);
        }
    }

    /// A built-in function's evaluation, or the game's for a library
    /// function.
    fn evaluate_function(&self, function_index: i16, thread_index: i32, initialize: bool) {
        use function::*;
        match function_index {
            BEGIN => self.evaluate_begin(thread_index, initialize),
            BEGIN_RANDOM => self.evaluate_begin_random(thread_index, initialize),
            IF => self.evaluate_if(thread_index, initialize),
            SET => self.evaluate_set(thread_index, initialize),
            AND | OR => self.evaluate_logical(function_index, thread_index, initialize),
            PLUS..=MAX => self.evaluate_arithmetic(function_index, thread_index, initialize),
            EQUAL | NOT_EQUAL => self.evaluate_equality(function_index, thread_index, initialize),
            GT..=LTE => self.evaluate_inequality(function_index, thread_index, initialize),
            SLEEP => self.evaluate_sleep(thread_index, initialize),
            SLEEP_UNTIL => self.evaluate_sleep_until(thread_index, initialize),
            WAKE => self.evaluate_wake(thread_index),
            INSPECT => self.evaluate_inspect(thread_index, initialize),
            OBJECT_TO_UNIT => self.evaluate_object_cast_up(function_index, thread_index, initialize),
            // the library: a function its macro parses has its arguments
            // evaluated here, as hs_macro_function_evaluate would, then its
            // body (the game's); cond and the debug strings are the game's
            _ => {
                let definition = self.function_definition(function_index);
                let m = self.m();
                if self.context.macro_parse != 0 && m.r32(definition + FUNCTION_PARSE) == self.context.macro_parse {
                    let count = (m.r16(definition + FUNCTION_PARAMETER_COUNT) as i16).max(0) as u32;
                    let types: Vec<i16> = (0..count).map(|i| m.r16(definition + FUNCTION_PARAMETER_TYPES + 2 * i) as i16).collect();
                    // (none to evaluate for a function without parameters,
                    // whose evaluation doesn't ask for them)
                    if types.is_empty() {
                        self.call(call::LIBRARY, function_index as i32, thread_index, 0);
                    } else if let Some(arguments) = self.arguments_evaluate(thread_index, &types, initialize) {
                        self.call(call::LIBRARY, function_index as i32, thread_index, arguments as i32);
                    }
                } else {
                    self.call(call::EXTERNAL, function_index as i32, thread_index, initialize as i32);
                }
            }
        }
    }

    /// `hs_script_evaluate`.
    fn script_evaluate(&self, script_index: i16, thread_index: i32, initialize: bool) {
        let m = self.m();
        let script = self.script(script_index as i32);
        let Some(result) = self.stack_allocate(thread_index, 4) else { return };
        if self.call(call::SCRIPT_DISABLED, script_index as i32, 0, 0) != 0 {
            let return_type = m.r16(script + SCRIPT_RETURN_TYPE) as i16;
            self.hs_return(thread_index, type_default_value(return_type, self.context.string_default));
        } else if initialize {
            self.evaluate(thread_index, m.r32(script + SCRIPT_ROOT_EXPRESSION_INDEX) as i32, result);
        } else {
            self.hs_return(thread_index, m.r32(result) as i32);
        }
    }

    fn initialized(&self) -> bool {
        self.m().r8(self.context.runtime_globals + RUNTIME_INITIALIZED) != 0
    }

    /// `hs_thread_main`.
    fn thread_main(&self, thread_index: i32) {
        let m = self.m();
        let thread = self.thread(thread_index);
        m.w16(self.context.runtime_globals + RUNTIME_EXECUTING_THREAD_INDEX, thread_index as i16 as u16);
        let thread_type = m.r8(thread + THREAD_TYPE);
        let mut script = None;
        if thread_type == THREAD_TYPE_SCRIPT {
            let script_index = m.r32(thread + THREAD_SCRIPT_INDEX) as i32;
            script = Some(self.script(script_index));
            if self.call(call::SCRIPT_DISABLED, script_index as i16 as i32, 0, 0) != 0 {
                m.w32(thread + THREAD_SLEEP_UNTIL, NONE as u32);
                m.w16(self.context.runtime_globals + RUNTIME_EXECUTING_THREAD_INDEX, NONE as u16);
                return;
            }
        }

        m.w32(thread + THREAD_SLEEP_UNTIL, 0);
        let base = thread + THREAD_STACK_DATA;
        if self.stack(thread) == base {
            m.w16(base + FRAME_SIZE, 0);
            if let Some(root_result) = self.stack_allocate(thread_index, 4) {
                let root = script.map_or(NONE, |s| m.r32(s + SCRIPT_ROOT_EXPRESSION_INDEX) as i32);
                self.evaluate(thread_index, root, root_result);
            }
        }

        loop {
            if self.stack(thread) == base {
                break;
            }
            let sleep_until = m.r32(thread + THREAD_SLEEP_UNTIL) as i32;
            if sleep_until < 0 {
                break;
            }
            if self.call(call::GAME_IN_PROGRESS, 0, 0, 0) != 0 && sleep_until > self.context.game_time {
                break;
            }
            if !self.initialized() {
                break;
            }
            let stack = self.stack(thread);
            let expression = m.r32(stack + FRAME_EXPRESSION_INDEX) as i32;
            let node = self.syntax(expression).unwrap_or(0);
            let initialize = self.flags(thread) & THREAD_IN_FUNCTION_CALL != 0;
            m.w16(stack + FRAME_SIZE, 0);
            self.set_flag(thread, THREAD_IN_FUNCTION_CALL, false);

            let flags = m.r16(node + NODE_FLAGS) as i16;
            let index = m.r16(node + NODE_INDEX) as i16;
            let index_count = if flags & NODE_SCRIPT != 0 { self.context.script_count } else { self.context.function_count };
            if index < 0 || index as i32 >= index_count {
                self.syntax_error(thread_index);
            } else if flags & NODE_SCRIPT == 0 {
                self.evaluate_function(index, thread_index, initialize);
            } else {
                self.script_evaluate(index, thread_index, initialize);
            }

            if self.flags(thread) & THREAD_STACK_OVERFLOW != 0 {
                m.w32(thread + THREAD_STACK, base);
                m.w8(thread + THREAD_FLAGS, 0);
                m.w32(
                    thread + THREAD_SLEEP_UNTIL,
                    if m.r8(thread + THREAD_TYPE) == THREAD_TYPE_SCRIPT { NONE as u32 } else { 0 },
                );
            }
        }

        if self.stack(thread) == base {
            if thread_type == THREAD_TYPE_SCRIPT {
                if let Some(script) = script {
                    let script_type = m.r16(script + SCRIPT_TYPE) as i16;
                    if script_type == SCRIPT_STARTUP || script_type == SCRIPT_DORMANT {
                        m.w32(thread + THREAD_SLEEP_UNTIL, NONE as u32);
                    }
                }
            } else if thread_type == THREAD_TYPE_CONSOLE_COMMAND {
                self.thread_delete(thread_index);
            }
        }
        m.w16(self.context.runtime_globals + RUNTIME_EXECUTING_THREAD_INDEX, NONE as u16);
    }

    /// `hs_runtime_initialize_for_new_map`: the scenario's globals
    /// initialized (each in an internal thread of its own), and a thread for
    /// each of its scripts that runs.
    fn initialize_for_new_map(&self) {
        let m = self.m();
        self.data_make_valid(self.context.thread_data);
        m.w8(self.context.runtime_globals + RUNTIME_INITIALIZED, 1);
        m.w16(self.context.runtime_globals + RUNTIME_EXECUTING_THREAD_INDEX, NONE as u16);
        let internal_thread_index = self.thread_new(THREAD_TYPE_GLOBAL_INITIALIZE, NONE);
        if !self.context.scenario_loaded {
            return;
        }
        let internal_thread = self.thread(internal_thread_index);
        for global_index in 0..self.context.global_count as i16 {
            let global = self.context.globals + GLOBAL_SIZE * global_index as u32;
            let global_datum_index = global_index as i32 + self.context.external_global_count;
            self.datum_new_at_index(self.context.global_data, (GLOBAL_SALT << 16) | global_datum_index);
            let global_datum = self.datum_get(self.context.global_data, global_datum_index).unwrap_or(0);
            m.w32(internal_thread + THREAD_SCRIPT_INDEX, NONE as u32);
            m.w16(self.stack(internal_thread) + FRAME_SIZE, 0);
            let global_type = m.r16(global + GLOBAL_TYPE) as i16;
            if self.call(call::GLOBAL_INITIALIZER_DISABLED, global_index as i32, 0, 0) != 0 {
                m.w32(global_datum + 4, type_default_value(global_type, self.context.string_default) as u32);
            } else {
                let expression = m.r32(global + GLOBAL_INITIALIZATION_EXPRESSION_INDEX) as i32;
                self.evaluate(internal_thread_index, expression, global_datum + 4);
            }
            if self.flags(internal_thread) & THREAD_IN_FUNCTION_CALL != 0 {
                self.thread_main(internal_thread_index);
                if global_type == HS_TYPE_OBJECT_LIST {
                    self.call(call::OBJECT_LIST_ADD_REFERENCE, self.global_evaluate(global_index), 0, 0);
                }
                if m.r32(internal_thread + THREAD_SLEEP_UNTIL) != 0 {
                    self.call(call::ERROR, ERROR_GLOBAL_INITIALIZATION_SLEPT, internal_thread_index, 0);
                }
            }
            self.global_reconcile_write(global_index);
        }
        self.thread_delete(internal_thread_index);
        for script_index in 0..self.context.script_count {
            let script_type = m.r16(self.script(script_index) + SCRIPT_TYPE) as i16;
            if script_type != SCRIPT_STATIC
                && script_type != SCRIPT_STUB
                && self.call(call::SCRIPT_DISABLED, script_index as i16 as i32, 0, 0) == 0
                && self.thread_new(THREAD_TYPE_SCRIPT, script_index) == NONE
            {
                self.call(call::ERROR, ERROR_OUT_OF_SCRIPT_THREADS, 0, 0);
            }
        }
    }

    /// `hs_runtime_evaluate`: a console command's expression, in a thread of
    /// its own; its value, or NONE if it is left running.
    fn evaluate_command(&self, expression_index: i32) -> i32 {
        let m = self.m();
        if !self.initialized() || expression_index == NONE {
            return NONE;
        }
        let thread_index = self.datum_new(self.context.thread_data);
        if thread_index == NONE {
            self.call(call::ERROR, ERROR_OUT_OF_COMMAND_THREADS, 0, 0);
            return NONE;
        }
        let thread = self.thread(thread_index);
        let base = thread + THREAD_STACK_DATA;
        m.w32(thread + THREAD_STACK, base);
        m.w32(base + FRAME_PREVIOUS, 0);
        m.w16(base + FRAME_SIZE, 0);
        m.w32(base + FRAME_EXPRESSION_INDEX, NONE as u32);
        m.w8(thread + THREAD_TYPE, THREAD_TYPE_CONSOLE_COMMAND);
        m.w32(thread + THREAD_SCRIPT_INDEX, NONE as u32);
        m.w8(thread + THREAD_FLAGS, 0);
        m.w32(thread + THREAD_SLEEP_UNTIL, 0);
        self.evaluate(thread_index, expression_index, thread + THREAD_RESULT);
        if self.flags(thread) & THREAD_IN_FUNCTION_CALL != 0 {
            self.thread_main(thread_index);
            return NONE;
        }
        m.r32(thread + THREAD_RESULT) as i32
    }

    /// `hs_runtime_update`.
    fn update(&self) {
        if !self.initialized() {
            return;
        }
        let m = self.m();
        let time = self.context.game_time;
        let mut console_command_running = false;
        let mut thread_index = self.data_next_index(self.context.thread_data, NONE);
        while self.initialized() && thread_index != NONE {
            let thread = self.thread(thread_index);
            if m.r8(thread + THREAD_TYPE) == THREAD_TYPE_CONSOLE_COMMAND {
                console_command_running = true;
            }
            let sleep_until = m.r32(thread + THREAD_SLEEP_UNTIL) as i32;
            if sleep_until >= 0 && sleep_until <= time {
                self.thread_main(thread_index);
            }
            thread_index = self.data_next_index(self.context.thread_data, thread_index);
        }
        self.call(call::OBJECT_LIST_GC, 0, 0, 0);
        if !console_command_running && time % 16 == 0 {
            self.call(call::NODE_GC, 0, 0, 0);
        }
    }
}

/// How much of a global the reconciliation with an external variable
/// copies (its switch's widths).
fn external_width(ty: i16) -> u32 {
    match ty {
        HS_TYPE_BOOLEAN => 1,
        HS_TYPE_REAL | HS_TYPE_LONG_INTEGER | HS_TYPE_STRING | HS_TYPE_AI | HS_TYPE_OBJECT_LIST => 4,
        24..=31 | HS_TYPE_OBJECT..=HS_TYPE_SCENERY => 4,
        _ => 2,
    }
}

/// `hs_type_default_value`.
fn type_default_value(ty: i16, string_default: u32) -> i32 {
    match ty {
        HS_TYPE_VOID | HS_TYPE_BOOLEAN | HS_TYPE_REAL | HS_TYPE_SHORT_INTEGER | HS_TYPE_LONG_INTEGER => 0,
        HS_TYPE_STRING => string_default as i32,
        HS_TYPE_SCRIPT => NONE,
        _ => NONE,
    }
}

/// `hs_runtime_initialize_for_new_map`, on the game's memory.
pub fn hs_runtime_initialize_for_new_map(context: &HsContext, host: &impl HsHost) {
    Runtime { context, host }.initialize_for_new_map();
}

/// `hs_runtime_evaluate`, on the game's memory: the command's value, or NONE.
pub fn hs_runtime_evaluate(context: &HsContext, host: &impl HsHost, expression_index: i32) -> i32 {
    Runtime { context, host }.evaluate_command(expression_index)
}

/// `hs_runtime_update`: every thread due runs, on the game's memory.
pub fn hs_runtime_update(context: &HsContext, host: &impl HsHost) {
    Runtime { context, host }.update();
}
