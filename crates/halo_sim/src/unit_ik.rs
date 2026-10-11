//! A unit's inverse kinematics (`unit_postprocess_node_matrices`) and an
//! object's choice of lights to reconnect (`object_connect_lights`) in Rust,
//! on the running game: `halo_map::unit_ik`'s, on the unit's node matrices in
//! place, from its tags (its graph's points, its model's markers and nodes)
//! and the grip objects' markers and poses as the game holds them. Whether a
//! grenade throw releases a vehicle's grip is the game's, which keeps its own
//! record ([`IkCallbacks`]); reconnecting the lights is the game's.
//!
//! In check mode the game's own runs first, its grenade answer recorded;
//! Rust's from the matrices it began with is compared with what it left.

use std::sync::Arc;

use halo_map::markers::ModelMarkers;
use halo_map::model::Matrix4x3;
use halo_map::unit_ik::{GripObject, HandModel, IkHost, IkInputs, object_lights, unit_postprocess_node_matrices};

use crate::TAGS;
use crate::world::{GameObjects, purpose};

/// `_unit_simple_creature_bit`.
const UNIT_SIMPLE_CREATURE: u32 = 1 << 11;
/// `_object_mirrored_bit`.
const OBJECT_MIRRORED: u32 = 1 << 12;

/// The game's side of a unit's inverse kinematics (halo_sim.c).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct IkCallbacks {
    /// `unit_grenade_throw_releases_vehicle_ik`: 1 if so.
    pub grenade_throw_releases_vehicle_ik: unsafe extern "C" fn(unit_index: u32) -> i32,
}

/// What a unit's inverse kinematics read, and the matrices they adjust.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct IkCallC {
    pub unit_index: u32,
    pub definition_index: u32,
    pub parent_object_index: i32,
    pub current_weapon_object_index: i32,
    pub seat_index: i16,
    pub weapon_class_index: i16,
    pub soft_ping_animation_index: i16,
    pub current_weapon_slot: i16,
    pub animation_state: i8,
    pub action: i8,
    pub pad: i16,
    pub object_flags: u32,
    pub region_permutations: [u8; 8],
    /// Its node matrices (13 floats each), adjusted in place.
    pub node_matrices: *mut f32,
    pub node_count: u32,
    /// In check mode: the game's answer to whether a grenade throw releases
    /// the vehicle's grip (-1 if it didn't ask).
    pub grenade_releases: i32,
    /// Whether its weapon's inverse kinematics ran.
    pub weapon_ik: u32,
}

/// An object's lights: its flags and attachments in, the lights to
/// reconnect out.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct LightsCallC {
    pub object_flags: u32,
    pub attachment_count: u32,
    pub attachment_types: [i8; 8],
    pub attachment_indices: [i32; 8],
    pub light_count: u32,
    pub lights: [i32; 8],
}

impl crate::Tags {
    fn model_markers(&mut self, model: u32) -> Option<Arc<ModelMarkers>> {
        let cache = &self.cache;
        self.model_markers.entry(model).or_insert_with(|| ModelMarkers::load(cache, model).ok().map(Arc::new)).clone()
    }
}

fn matrix(f: &[f32]) -> Matrix4x3 {
    Matrix4x3 { scale: f[0], forward: [f[1], f[2], f[3]], left: [f[4], f[5], f[6]], up: [f[7], f[8], f[9]], position: [f[10], f[11], f[12]] }
}

fn floats(m: &Matrix4x3) -> [f32; 13] {
    [
        m.scale,
        m.forward[0],
        m.forward[1],
        m.forward[2],
        m.left[0],
        m.left[1],
        m.left[2],
        m.up[0],
        m.up[1],
        m.up[2],
        m.position[0],
        m.position[1],
        m.position[2],
    ]
}

struct Host {
    source: GameObjects,
    unit_index: u32,
    callbacks: IkCallbacks,
    /// In check mode, the game's recorded answer.
    recorded_grenade: Option<bool>,
}

impl IkHost for Host {
    fn grenade_throw_releases_vehicle_ik(&self) -> bool {
        match self.recorded_grenade {
            Some(answer) => answer,
            // SAFETY: the game's callback
            None => (unsafe { (self.callbacks.grenade_throw_releases_vehicle_ik)(self.unit_index) }) != 0,
        }
    }

    fn grip_object(&self, index: i32) -> Option<GripObject> {
        let o = self.source.host_object(index as u32, purpose::OTHER)?;
        let markers = {
            let mut tags = TAGS.lock().unwrap_or_else(|e| e.into_inner());
            let tags = tags.as_mut()?;
            tags.definition(o.definition_index).and_then(|d| d.model).and_then(|model| tags.model_markers(model))
        };
        let node_matrices = (0..o.node_count as usize)
            .map(|n| {
                // SAFETY: the host's matrices, node_count of them
                let f = unsafe { std::slice::from_raw_parts(o.matrices.add(n * 13), 13) };
                matrix(f)
            })
            .collect();
        Some(GripObject { markers, node_matrices, region_permutations: o.region_permutations, mirrored: o.flags & OBJECT_MIRRORED != 0 })
    }
}

fn run(call: &mut IkCallC, check: bool) -> i32 {
    let Some(host) = crate::world::host() else { return -1 };
    let (graph, model, markers, unit_flags) = {
        let mut tags = TAGS.lock().unwrap_or_else(|e| e.into_inner());
        let Some(tags) = tags.as_mut() else { return -1 };
        let Some(definition) = tags.definition(call.definition_index) else { return -1 };
        let graph = definition.animation_graph.and_then(|g| tags.animation_graph(g));
        let model = definition.model.and_then(|m| tags.model(m));
        let markers = definition.model.and_then(|m| tags.model_markers(m));
        (graph, model, markers, tags.unit_flags(call.definition_index))
    };
    let node_parents: Vec<i16> = model.as_ref().map_or_else(Vec::new, |m| m.nodes.iter().map(|n| n.parent).collect());
    let hand = HandModel { markers: markers.as_deref(), node_parents: &node_parents };
    let inputs = IkInputs {
        simple_creature: unit_flags & UNIT_SIMPLE_CREATURE != 0,
        seat_index: call.seat_index,
        weapon_class_index: call.weapon_class_index,
        parent_object_index: call.parent_object_index,
        animation_state: call.animation_state,
        action: call.action,
        soft_ping_animation_index: call.soft_ping_animation_index,
        current_weapon_slot: call.current_weapon_slot,
        current_weapon_object_index: call.current_weapon_object_index,
        region_permutations: call.region_permutations,
        mirrored: call.object_flags & OBJECT_MIRRORED != 0,
    };
    let count = call.node_count as usize;
    // SAFETY: the game's matrices, node_count of them
    let raw = unsafe { std::slice::from_raw_parts_mut(call.node_matrices, count * 13) };
    let mut matrices: Vec<Matrix4x3> = raw.chunks_exact(13).map(matrix).collect();
    let ik_host = Host {
        source: GameObjects { host },
        unit_index: call.unit_index,
        callbacks: host.ik,
        recorded_grenade: check.then_some(call.grenade_releases > 0),
    };
    let weapon_ik = unit_postprocess_node_matrices(graph.as_deref(), &hand, &inputs, &ik_host, &mut matrices);
    for (out, m) in raw.chunks_exact_mut(13).zip(&matrices) {
        out.copy_from_slice(&floats(m));
    }
    call.weapon_ik = weapon_ik as u32;
    0
}

/// A unit's inverse kinematics on the running game, its node matrices
/// adjusted in place. Returns 0, or -1 if it can't run (nothing was done).
///
/// # Safety
/// `call` must be valid; its matrices hold `node_count` matrices.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn halo_sim_unit_ik(call: *mut IkCallC) -> i32 {
    if call.is_null() {
        return -1;
    }
    // SAFETY: the caller's call
    run(unsafe { &mut *call }, false)
}

/// The check of a unit's inverse kinematics the game ran: Rust's on the
/// matrices it began with (a copy), the game's grenade answer replayed.
/// Returns 0, or -1.
///
/// # Safety
/// As [`halo_sim_unit_ik`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn halo_sim_unit_ik_check(call: *mut IkCallC) -> i32 {
    if call.is_null() {
        return -1;
    }
    // SAFETY: the caller's call
    run(unsafe { &mut *call }, true)
}

/// An object's lights to reconnect (`object_connect_lights`'). Returns 0.
///
/// # Safety
/// `call` must be valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn halo_sim_object_lights(call: *mut LightsCallC) -> i32 {
    if call.is_null() {
        return -1;
    }
    // SAFETY: the caller's call
    let call = unsafe { &mut *call };
    let lights = object_lights(call.object_flags, call.attachment_count as usize, &call.attachment_types, &call.attachment_indices);
    call.light_count = lights.len() as u32;
    call.lights = [-1; 8];
    call.lights[..lights.len()].copy_from_slice(&lights);
    0
}
