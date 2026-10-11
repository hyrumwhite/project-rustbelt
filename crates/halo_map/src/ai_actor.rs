//! What the AI's ports of an actor's steps share: the actor's datum, its
//! props and its definition are the game's state, so a step works on the
//! game's memory in place, through [`AiMemory`] (with the layouts measured
//! from the game's own C, `layout.rs`), and what it asks of the rest of the
//! game goes through [`AiHost::call`] (each step's own call function in the
//! C, shared with it).

use crate::layout::prop_datum;

pub const NONE: i32 = -1;

/// `struct data_array`'s fields.
const DATA_SIZE: u32 = 0x22;
const DATA_COUNT: u32 = 0x2E;
const DATA_DATA: u32 = 0x34;

/// The game's memory.
pub trait AiMemory {
    fn r8(&self, address: u32) -> u8;
    fn r16(&self, address: u32) -> u16;
    fn r32(&self, address: u32) -> u32;
    fn w8(&self, address: u32, value: u8);
    fn w16(&self, address: u32, value: u16);
    fn w32(&self, address: u32, value: u32);
}

/// Typed reads and writes.
pub trait AiMemoryExt: AiMemory {
    fn i8(&self, address: u32) -> i8 {
        self.r8(address) as i8
    }
    fn bool(&self, address: u32) -> bool {
        self.r8(address) != 0
    }
    fn i16(&self, address: u32) -> i16 {
        self.r16(address) as i16
    }
    fn i32(&self, address: u32) -> i32 {
        self.r32(address) as i32
    }
    fn f32(&self, address: u32) -> f32 {
        f32::from_bits(self.r32(address))
    }
    fn vector(&self, address: u32) -> [f32; 3] {
        [self.f32(address), self.f32(address + 4), self.f32(address + 8)]
    }
    fn set_bool(&self, address: u32, value: bool) {
        self.w8(address, value as u8);
    }
    fn set_i16(&self, address: u32, value: i16) {
        self.w16(address, value as u16);
    }
    fn set_f32(&self, address: u32, value: f32) {
        self.w32(address, value.to_bits());
    }
}

impl<T: AiMemory + ?Sized> AiMemoryExt for T {}

/// halo_sim.h's `struct halo_sim_ai_call`: what a step asks of the game.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct AiCall {
    pub kind: i32,
    pub actor: i32,
    pub a: i32,
    pub b: i32,
    pub input: [f32; 9],
    pub output: [f32; 3],
    pub result: i32,
}

/// The game's side of a step.
pub trait AiHost {
    fn memory(&self) -> &dyn AiMemory;
    fn call(&self, call: &mut AiCall);
}

/// Where an actor's step finds what it works on.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct AiStepContext {
    pub actor_index: i32,
    /// The actor's datum.
    pub actor: u32,
    /// Its definition (`actor_definition_get`).
    pub definition: u32,
    /// `prop_data` (its `struct data_array`).
    pub prop_data: u32,
    pub game_time: i32,
}

/// `datum_get`: the datum's address (None for one unused or changed).
pub fn datum_get(m: &dyn AiMemory, data: u32, index: i32) -> Option<u32> {
    let identifier = (index >> 16) as i16;
    let absolute_index = index as i16;
    let count = m.r16(data + DATA_COUNT) as i16;
    if absolute_index >= 0 && absolute_index < count {
        let size = m.r16(data + DATA_SIZE) as i16 as u32;
        let header = m.r32(data + DATA_DATA) + size * absolute_index as u32;
        let header_identifier = m.r16(header) as i16;
        if header_identifier != 0 && (identifier == 0 || identifier == header_identifier) {
            return Some(header);
        }
    }
    None
}

/// `prop_get`.
pub fn prop_get(m: &dyn AiMemory, context: &AiStepContext, prop_index: i32) -> u32 {
    datum_get(m, context.prop_data, prop_index).unwrap_or(0)
}

/// `struct prop_iterator` over an actor's props.
pub struct PropIterator {
    pub index: i32,
    pub next_index: i32,
}

impl PropIterator {
    /// `prop_iterator_new`.
    pub fn new(m: &dyn AiMemory, context: &AiStepContext) -> Self {
        let next_index = m.i32(context.actor + crate::layout::actor_datum::META_FIRST_PROP_INDEX as u32);
        Self { index: NONE, next_index }
    }

    /// `prop_iterator_next`: the next prop's address.
    pub fn next(&mut self, m: &dyn AiMemory, context: &AiStepContext) -> Option<u32> {
        self.index = self.next_index;
        if self.index == NONE {
            return None;
        }
        let prop = prop_get(m, context, self.index);
        self.next_index = m.i32(prop + prop_datum::NEXT_PROP_INDEX as u32);
        Some(prop)
    }
}
