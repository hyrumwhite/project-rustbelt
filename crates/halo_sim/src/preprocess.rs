//! The object types' preprocess of a pose (`object_type_preprocess_
//! node_orientations`): a unit's (its actions, overlays, aiming and looking
//! screens, and the port's grenade throw on the move), then a vehicle's
//! (steering, speed, wheel and suspension overlays), or a device's
//! (position and power overlays). A weapon's does nothing.

use std::sync::Arc;

use halo_map::pose::{self, Orientation};
use halo_map::unit_animation::AnimationGraph;
use halo_map::unit_pose::{GrenadeThrowLegs, UnitPoseInputs, unit_preprocess_node_orientations};

use crate::{TAGS, Tags};

const OBJECT_TYPE_BIPED: i16 = 0;
const OBJECT_TYPE_VEHICLE: i16 = 1;
const OBJECT_TYPE_MACHINE: i16 = 7;
const OBJECT_TYPE_LIGHT_FIXTURE: i16 = 9;
/// `_unit_has_no_aiming_bit`, `_unit_simple_creature_bit`.
const UNIT_HAS_NO_AIMING: u32 = 1 << 10;
const UNIT_SIMPLE_CREATURE: u32 = 1 << 11;

/// What a type's preprocess reads, and (for a unit) what it sets.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct PosePreprocessCall {
    pub object_index: u32,
    pub definition_index: u32,
    pub object_type: i16,
    pub node_count: i16,
    /// The orientations (8 floats each), changed in place.
    pub orientations: *mut f32,
    // a unit's: as bevy_bridge's pose record gathers them
    pub enhanced: u32,
    pub airborne: u32,
    pub parented: u32,
    pub has_weapon: u32,
    pub player: u32,
    pub game_time: i32,
    pub unit_animation_flags: u16,
    pub aiming_screen_index: i16,
    pub looking_screen_index: i16,
    pub override_emotion_animation_index: i16,
    pub seat_index: i8,
    pub weapon_index: i8,
    pub state: i8,
    pub action: i8,
    pub base_seat_index: i8,
    pub emotion_index: i8,
    pub pad: i16,
    pub action_animation: [i16; 2],
    pub overlay_action_animation: [i16; 2],
    pub soft_ping_animation: [i16; 2],
    pub mouth_aperture: f32,
    pub seat_acceleration: [f32; 3],
    pub aiming_vector: [f32; 3],
    pub looking_vector: [f32; 3],
    pub orientation_forward: [f32; 3],
    pub orientation_up: [f32; 3],
    pub forward: [f32; 3],
    pub up: [f32; 3],
    pub translational_velocity: [f32; 3],
    /// The grenade throw's leg weights and their times (read and written).
    pub leg_weight: f32,
    pub leg_time: i32,
    pub air_weight: f32,
    pub air_time: i32,
    /// Written: whether it aims and looks with euler screens, and (where
    /// set) their bounds.
    pub aiming_with_euler_screen: u32,
    pub looking_with_euler_screen: u32,
    pub aiming_bounds_set: u32,
    pub looking_bounds_set: u32,
    pub aiming_screen_bounds: [f32; 4],
    pub looking_screen_bounds: [f32; 4],
    // a vehicle's
    pub vehicle_turn: f32,
    pub vehicle_speed: f32,
    pub vehicle_wheel: f32,
    pub vehicle_suspension: [u8; 8],
    // a device's
    pub device_flags: u32,
    pub device_position: f32,
    pub device_power: f32,
}

impl Tags {
    fn graph(&mut self, datum: u32) -> Option<Arc<AnimationGraph>> {
        let cache = &self.cache;
        self.graphs.entry(datum).or_insert_with(|| AnimationGraph::load(cache, datum).ok().map(Arc::new)).clone()
    }

    /// Every animation of a graph, read once.
    fn load_graph_animations(&mut self, graph: u32) {
        if !self.graphs_loaded.insert(graph) {
            return;
        }
        for index in 0..i16::MAX {
            if self.animation(graph, index).is_none() {
                break;
            }
        }
    }

    pub(crate) fn unit_flags(&mut self, definition: u32) -> u32 {
        let cache = &self.cache;
        *self.unit_flags.entry(definition).or_insert_with(|| {
            cache
                .tag_root(definition, halo_map::groups::OBJECT)
                .and_then(|root| root.u32(halo_map::layout::biped_definition::UNIT + halo_map::layout::_unit_definition::FLAGS))
                .unwrap_or(0)
        })
    }
}

/// An object type's preprocess of a pose. Returns 0, or -1 if the tags
/// aren't there (the game then runs its own).
///
/// # Safety
/// `call` must be valid, its `orientations` hold `node_count` orientations.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn halo_sim_pose_preprocess(call: *mut PosePreprocessCall) -> i32 {
    if call.is_null() {
        return -1;
    }
    // SAFETY: the caller's call
    let call = unsafe { &mut *call };
    if call.orientations.is_null() || call.node_count <= 0 || call.node_count as usize > pose::MAXIMUM_NODES {
        return -1;
    }
    let node_count = call.node_count as usize;
    let mut tags = TAGS.lock().unwrap_or_else(|e| e.into_inner());
    let Some(tags) = tags.as_mut() else { return -1 };
    let Some(definition) = tags.definition(call.definition_index) else { return -1 };
    let Some(graph_index) = definition.animation_graph else { return 0 };
    let Some(model) = definition.model.and_then(|m| tags.model(m)) else { return -1 };
    tags.load_graph_animations(graph_index);
    let mut orientations: Vec<Orientation> = (0..node_count)
        .map(|index| {
            let mut floats = [0.0f32; 8];
            // SAFETY: the caller's buffer holds `node_count` orientations
            unsafe { std::ptr::copy_nonoverlapping(call.orientations.add(index * 8), floats.as_mut_ptr(), 8) };
            Orientation::from_floats(floats)
        })
        .collect();

    if call.object_type == OBJECT_TYPE_BIPED || call.object_type == OBJECT_TYPE_VEHICLE {
        let Some(graph) = tags.graph(graph_index) else { return -1 };
        let unit_flags = tags.unit_flags(call.definition_index);
        let inputs = UnitPoseInputs {
            has_no_aiming: unit_flags & UNIT_HAS_NO_AIMING != 0,
            simple_creature: unit_flags & UNIT_SIMPLE_CREATURE != 0,
            enhanced: call.enhanced != 0,
            airborne: call.airborne != 0,
            parented: call.parented != 0,
            has_weapon: call.has_weapon != 0,
            player: call.player != 0,
            game_time: call.game_time,
            animation_flags: call.unit_animation_flags,
            aiming_screen_index: call.aiming_screen_index,
            looking_screen_index: call.looking_screen_index,
            override_emotion_animation_index: call.override_emotion_animation_index,
            seat_index: call.seat_index,
            weapon_index: call.weapon_index,
            state: call.state,
            action: call.action,
            base_seat_index: call.base_seat_index,
            emotion_index: call.emotion_index,
            action_animation: (call.action_animation[0], call.action_animation[1]),
            overlay_action_animation: (call.overlay_action_animation[0], call.overlay_action_animation[1]),
            soft_ping_animation: (call.soft_ping_animation[0], call.soft_ping_animation[1]),
            mouth_aperture: call.mouth_aperture,
            seat_acceleration: call.seat_acceleration,
            aiming_vector: call.aiming_vector,
            looking_vector: call.looking_vector,
            orientation_forward: call.orientation_forward,
            orientation_up: call.orientation_up,
            forward: call.forward,
            up: call.up,
            translational_velocity: call.translational_velocity,
        };
        let mut legs = GrenadeThrowLegs { leg_weight: call.leg_weight, leg_time: call.leg_time, air_weight: call.air_weight, air_time: call.air_time };
        let animations = &tags.animations;
        let animation = |index: i16| animations.get(&(graph_index, index)).and_then(|a| a.as_deref());
        let outputs = unit_preprocess_node_orientations(&model, &graph, &animation, &inputs, &mut legs, &mut orientations);
        call.leg_weight = legs.leg_weight;
        call.leg_time = legs.leg_time;
        call.air_weight = legs.air_weight;
        call.air_time = legs.air_time;
        call.aiming_with_euler_screen = outputs.aiming_with_euler_screen as u32;
        call.looking_with_euler_screen = outputs.looking_with_euler_screen as u32;
        call.aiming_bounds_set = outputs.aiming_screen_bounds.is_some() as u32;
        call.looking_bounds_set = outputs.looking_screen_bounds.is_some() as u32;
        call.aiming_screen_bounds = outputs.aiming_screen_bounds.unwrap_or_default();
        call.looking_screen_bounds = outputs.looking_screen_bounds.unwrap_or_default();
    }
    if call.object_type == OBJECT_TYPE_VEHICLE {
        let cache = &tags.cache;
        let Some(vehicle) = tags
            .vehicles
            .entry(call.definition_index)
            .or_insert_with(|| halo_map::vehicle_pose::VehiclePoseDefinition::load(cache, call.definition_index).ok().map(Arc::new))
            .clone()
        else {
            return -1;
        };
        let inputs = halo_map::vehicle_pose::VehiclePoseInputs {
            turn: call.vehicle_turn,
            speed: call.vehicle_speed,
            wheel: call.vehicle_wheel,
            suspension: call.vehicle_suspension,
            forward: call.forward,
            up: call.up,
            translational_velocity: call.translational_velocity,
        };
        let animations = &tags.animations;
        let animation = |index: i16| animations.get(&(graph_index, index)).and_then(|a| a.as_deref());
        halo_map::vehicle_pose::vehicle_preprocess_node_orientations(&vehicle, &animation, node_count, &inputs, &mut orientations);
    }
    if (OBJECT_TYPE_MACHINE..=OBJECT_TYPE_LIGHT_FIXTURE).contains(&call.object_type) {
        let cache = &tags.cache;
        let Some(device) = tags
            .devices
            .entry(call.definition_index)
            .or_insert_with(|| halo_map::device_pose::DevicePoseDefinition::load(cache, call.definition_index).ok().map(Arc::new))
            .clone()
        else {
            return -1;
        };
        let animations = &tags.animations;
        let animation = |index: i16| animations.get(&(graph_index, index)).and_then(|a| a.as_deref());
        halo_map::device_pose::device_preprocess_node_orientations(
            &device,
            &animation,
            call.device_flags,
            call.device_position,
            call.device_power,
            &mut orientations,
        );
    }

    for (index, orientation) in orientations.iter().take(node_count).enumerate() {
        let floats = orientation.to_floats();
        // SAFETY: as above
        unsafe { std::ptr::copy_nonoverlapping(floats.as_ptr(), call.orientations.add(index * 8), 8) };
    }
    0
}
