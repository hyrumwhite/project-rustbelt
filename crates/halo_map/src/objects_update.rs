//! Halo's object update loop (ROADMAP phase 7, objects and physics):
//! objects.c's `objects_update` and `object_update`, ported as they are,
//! with the two smallest type updates, garbage.c's `garbage_update` and
//! scenery.c's `scenery_update`.
//!
//! Each tick: the objects the players' view reaches are activated (and
//! those it left deactivated); every active top-level object is updated
//! (only players' units on a double-speed game's odd ticks); then each
//! object's do-not-update flag is cleared, the objects created this tick
//! get their first update, and those deleted are deleted; and garbage is
//! collected. An object's update: counting garbage, its animation's
//! interpolation, its type's update and its damage over time, its exported
//! and computed function values, its node matrices, change colours and
//! lights, then its children and (a child's) next sibling, and its node
//! matrices' postprocess.
//!
//! Everything an object update does to an object or the rest of the game
//! goes through an [`ObjectsHost`] ([`call`]'s kinds), reads of the object
//! headers and objects included: a system already handed over runs in Rust
//! behind its call.

use crate::unit_animation::{AnimationGraph, animation_update, update_result};

const NONE: i32 = -1;

// object header flags
const HEADER_ACTIVE: u8 = 1 << 0;
const HEADER_BEING_CREATED: u8 = 1 << 2;
const HEADER_BEING_DELETED: u8 = 1 << 3;
const HEADER_DO_NOT_UPDATE: u8 = 1 << 4;
// object flags
const OBJECT_INVISIBLE: u32 = 1 << 0;
const OBJECT_DYNAMIC_LIGHTING_RECOMPUTE: u32 = 1 << 13;
const OBJECT_GARBAGE: u32 = 1 << 16;
const OBJECT_DO_NOT_RECOMPUTE_NODE_MATRICES: u32 = 1 << 23;
// object types (bipeds and vehicles are units)
const OBJECT_MASK_UNIT: u32 = (1 << 0) | (1 << 1);

/// An object header, as the loop reads it.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ObjectHeader {
    /// The object header data's count (the loop's bound, read each time).
    pub count: i32,
    pub identifier: i16,
    pub flags: u8,
    pub object_type: u8,
    /// A unit's player (NONE for none, or for another object).
    pub unit_player_index: i32,
}

/// An object, as its update reads it.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ObjectUpdateState {
    /// Whether it is there.
    pub exists: u32,
    pub header_flags: u8,
    pub pad0: u8,
    pub interpolation_frame_index: i16,
    pub interpolation_frame_count: i16,
    pub pad1: i16,
    pub object_flags: u32,
    pub first_child_object_index: i32,
    pub next_object_index: i32,
    pub parent_object_index: i32,
    /// Whether its definition has a collision model, and a model.
    pub has_collision_model: u32,
    pub has_model: u32,
}

/// What the loop asks of the game (`ObjectsCall::kind`).
pub mod call {
    /// The header at absolute index `a` (and the headers' count).
    pub const HEADER: i32 = 1;
    /// The tick begins: the active garbage count reset.
    pub const BEGIN: i32 = 2;
    /// Activating and deactivating by the players' combined view, and the
    /// structure's decals.
    pub const ACTIVATION: i32 = 3;
    /// An object's state.
    pub const OBJECT: i32 = 4;
    /// Another active piece of garbage counted.
    pub const COUNT_GARBAGE: i32 = 5;
    /// Its animation's interpolation: `a` the frame index, `b` the count.
    pub const SET_INTERPOLATION: i32 = 6;
    /// `object_type_update`.
    pub const TYPE_UPDATE: i32 = 7;
    /// `object_damage_update`.
    pub const DAMAGE_UPDATE: i32 = 8;
    /// `object_type_export_function_values`.
    pub const EXPORT_FUNCTION_VALUES: i32 = 9;
    /// `object_compute_node_matrices`.
    pub const COMPUTE_NODE_MATRICES: i32 = 10;
    /// `object_compute_function_values`.
    pub const COMPUTE_FUNCTION_VALUES: i32 = 11;
    /// `object_compute_change_colors`.
    pub const COMPUTE_CHANGE_COLORS: i32 = 12;
    /// `object_connect_lights(object, TRUE, TRUE)`.
    pub const CONNECT_LIGHTS: i32 = 13;
    /// `object_postprocess_node_matrices`.
    pub const POSTPROCESS_NODE_MATRICES: i32 = 14;
    /// The header `a`'s do-not-update flag cleared.
    pub const CLEAR_DO_NOT_UPDATE: i32 = 15;
    /// The header `a`'s being-created flag cleared.
    pub const CLEAR_BEING_CREATED: i32 = 16;
    /// `object_delete_recursive(object, FALSE)`.
    pub const DELETE: i32 = 17;
    /// `objects_garbage_collection`.
    pub const GARBAGE_COLLECTION: i32 = 18;
}

/// One call of the game, and what it read.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ObjectsCall {
    pub kind: i32,
    pub object_index: i32,
    pub a: i32,
    pub b: i32,
    pub header: ObjectHeader,
    pub state: ObjectUpdateState,
}

/// A running game, as the loop asks it.
pub trait ObjectsHost {
    fn call(&self, call: &mut ObjectsCall);
}

fn call(host: &dyn ObjectsHost, kind: i32, object_index: i32, a: i32, b: i32) -> ObjectsCall {
    let mut c = ObjectsCall { kind, object_index, a, b, ..Default::default() };
    host.call(&mut c);
    c
}

fn header(host: &dyn ObjectsHost, index: i32) -> ObjectHeader {
    call(host, call::HEADER, NONE, index, 0).header
}

fn object(host: &dyn ObjectsHost, object_index: i32) -> ObjectUpdateState {
    call(host, call::OBJECT, object_index, 0, 0).state
}

/// `DATUM_INDEX_NEW`.
fn datum_index(absolute_index: i32, identifier: i16) -> i32 {
    (((identifier as i32 as u32) << 16) | (absolute_index as u32 & 0xffff)) as i32
}

/// `objects_update`. `dont_update_object`: a double-speed game's odd tick
/// (only players' units are updated).
pub fn objects_update(host: &dyn ObjectsHost, dont_update_object: bool) {
    call(host, call::BEGIN, NONE, 0, 0);
    call(host, call::ACTIVATION, NONE, 0, 0);

    let mut i = 0;
    loop {
        let h = header(host, i);
        if i >= h.count {
            break;
        }
        if h.identifier != 0 && h.flags & HEADER_ACTIVE != 0 && h.flags & HEADER_BEING_CREATED == 0 {
            let object_index = datum_index(i, h.identifier);
            let unit = OBJECT_MASK_UNIT & (1 << (h.object_type as u32 & 31)) != 0;
            if !dont_update_object || (unit && h.unit_player_index != NONE) {
                object_update(host, object_index);
            }
        }
        i += 1;
    }

    let mut i = 0;
    loop {
        let h = header(host, i);
        if i >= h.count {
            break;
        }
        if h.identifier != 0 {
            call(host, call::CLEAR_DO_NOT_UPDATE, NONE, i, 0);
            let mut flags = h.flags;
            let mut identifier = h.identifier;
            if flags & HEADER_BEING_CREATED != 0 {
                call(host, call::CLEAR_BEING_CREATED, NONE, i, 0);
                object_update(host, datum_index(i, identifier));
                // (the header as the update left it)
                let after = header(host, i);
                flags = after.flags;
                identifier = after.identifier;
            }
            if flags & HEADER_BEING_DELETED != 0 {
                call(host, call::DELETE, datum_index(i, identifier), 0, 0);
            }
        }
        i += 1;
    }

    call(host, call::GARBAGE_COLLECTION, NONE, 0, 0);
}

/// `object_update`.
pub fn object_update(host: &dyn ObjectsHost, object_index: i32) {
    let state = object(host, object_index);
    if state.exists == 0 || state.header_flags & HEADER_DO_NOT_UPDATE != 0 {
        return;
    }
    if state.object_flags & OBJECT_GARBAGE != 0 {
        call(host, call::COUNT_GARBAGE, object_index, 0, 0);
    }
    if state.interpolation_frame_count != 0 {
        let frame_index = state.interpolation_frame_index.wrapping_add(1);
        let count = if frame_index >= state.interpolation_frame_count { 0 } else { state.interpolation_frame_count };
        call(host, call::SET_INTERPOLATION, object_index, frame_index as i32, count as i32);
    }

    call(host, call::TYPE_UPDATE, object_index, 0, 0);
    if state.has_collision_model != 0 {
        call(host, call::DAMAGE_UPDATE, object_index, 0, 0);
    }
    call(host, call::EXPORT_FUNCTION_VALUES, object_index, 0, 0);

    if object(host, object_index).object_flags & OBJECT_DO_NOT_RECOMPUTE_NODE_MATRICES == 0 {
        call(host, call::COMPUTE_NODE_MATRICES, object_index, 0, 0);
    }
    call(host, call::COMPUTE_FUNCTION_VALUES, object_index, 0, 0);
    call(host, call::COMPUTE_CHANGE_COLORS, object_index, 0, 0);

    let flags = object(host, object_index).object_flags;
    if flags & OBJECT_DYNAMIC_LIGHTING_RECOMPUTE != 0 && (flags & OBJECT_INVISIBLE == 0 || state.has_model == 0) {
        call(host, call::CONNECT_LIGHTS, object_index, 0, 0);
    }

    let children = object(host, object_index).first_child_object_index;
    if children != NONE {
        object_update(host, children);
    }
    let after = object(host, object_index);
    if after.parent_object_index != NONE && after.next_object_index != NONE {
        object_update(host, after.next_object_index);
    }

    call(host, call::POSTPROCESS_NODE_MATRICES, object_index, 0, 0);
}

/// `garbage_update`: its lifetime a tick shorter, and whether it is still
/// active (the caller deletes it if not).
pub fn garbage_update(lifetime_ticks: &mut i16) -> bool {
    *lifetime_ticks = lifetime_ticks.wrapping_sub(1);
    *lifetime_ticks > 0
}

/// `scenery_update`: a scenery that animates by itself (its flags' bit 0)
/// a frame further, drawing from the game's random seed.
pub fn scenery_update(graph: Option<&AnimationGraph>, flags: u32, seed: &mut u32, index: &mut i16, frame_index: &mut i16) {
    if flags & 1 != 0 && animation_update(graph, seed, index, frame_index) == update_result::WILL_RESTART_ON_NEXT_FRAME {
        *frame_index = frame_index.wrapping_sub(1);
    }
}
