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
    pub output: [f32; 6],
    pub result: i32,
    pub result2: i32,
}

impl AiCall {
    /// The first three of `output`: a vector.
    pub fn vector(&self) -> [f32; 3] {
        [self.output[0], self.output[1], self.output[2]]
    }
}

/// The game's side of a step.
pub trait AiHost {
    fn memory(&self) -> &dyn AiMemory;
    fn call(&self, call: &mut AiCall);
    /// `collision_bsp_test_vector` in the step's structure (its collision
    /// BSP): the hit's t, if any. With `breakable_surfaces`, broken ones are
    /// passed through; without, every one stands (the game passes none).
    fn structure_test_vector(
        &self,
        context: &AiStepContext,
        breakable_surfaces: bool,
        flags: u32,
        point: [f32; 3],
        vector: [f32; 3],
        maximum_t: f32,
    ) -> Option<f32>;
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
    /// `actor_data` (its `struct data_array`).
    pub actor_data: u32,
    /// `object_header_data` (its `struct data_array`).
    pub object_header_data: u32,
    /// Its debug info (ai_debug.h's `actor_debug_array`).
    pub debug_info: u32,
    /// Its variant's definition (`actor_variant_definition_get`).
    pub variant_definition: u32,
    /// The scenario (`global_scenario_get`).
    pub scenario: u32,
    /// The structure (`global_structure_bsp_get`).
    pub structure: u32,
    /// Which breakable surfaces still stand (`breakable_surface_flags_get`).
    pub breakable_surface_flags: u32,
    /// `global_structure_bsp_index`.
    pub structure_bsp_index: i32,
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

/// `object_get`: the object's datum (None for one unused or changed).
pub fn object_get(m: &dyn AiMemory, context: &AiStepContext, object_index: i32) -> Option<u32> {
    datum_get(m, context.object_header_data, object_index)
        .map(|header| m.r32(header + crate::layout::object_header_datum::DATUM as u32))
}

/// `TAG_BLOCK_GET_ELEMENT`: the element's address.
pub fn tag_block_element(m: &dyn AiMemory, block: u32, index: i32, size: usize) -> u32 {
    m.r32(block + crate::layout::tag_block::ADDRESS as u32).wrapping_add((index as u32).wrapping_mul(size as u32))
}

/// A tag block's count.
pub fn tag_block_count(m: &dyn AiMemory, block: u32) -> i32 {
    m.i32(block + crate::layout::tag_block::COUNT as u32)
}

/// The definition of an encounter's squad (the scenario's).
pub fn squad_definition(m: &dyn AiMemory, context: &AiStepContext, encounter_index: i32, squad_index: i16) -> u32 {
    use crate::layout::{encounter_definition, scenario, squad_definition};
    let encounter = tag_block_element(
        m,
        context.scenario + scenario::AI_ENCOUNTERS as u32,
        encounter_index & 0xFFFF,
        encounter_definition::SIZEOF,
    );
    tag_block_element(m, encounter + encounter_definition::SQUADS as u32, squad_index as i32, squad_definition::SIZEOF)
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
        if prop == 0 {
            // (the C's prop_get asserts; a list through a freed prop ends)
            return None;
        }
        self.next_index = m.i32(prop + prop_datum::NEXT_PROP_INDEX as u32);
        Some(prop)
    }
}
