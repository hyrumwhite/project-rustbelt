//! Halo's vehicle poses (ROADMAP phase 7, objects and physics): what a
//! vehicle's type does to its pose after the unit's
//! (`vehicle_preprocess_node_orientations` in OpenCE's
//! `source/units/vehicles.c`), ported as it is, in the same
//! single-precision arithmetic: the graph's steering screen at the
//! vehicle's turn, overlays for its sideways velocity, speed, forward
//! velocity and wheel rotation, and one per suspension at how far it is
//! compressed.

use crate::features::{cross3, dot3};
use crate::pose::{Orientation, PoseAnimation, ScreenBounds, aiming_screen_apply, overlay_animation_apply_continuous};
use crate::{CacheFile, Result, layout};

/// The suspensions a vehicle keeps.
const MAXIMUM_SUSPENSIONS: usize = 8;

/// What a vehicle's preprocess reads of its definition and graph.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct VehiclePoseDefinition {
    /// The vehicle definition's forward and reverse speeds
    /// (`unknown2f8`, `unknown2fc`) and wheel circumference.
    pub maximum_forward_speed: f32,
    pub maximum_reverse_speed: f32,
    pub wheel_circumference: f32,
    /// The graph's first vehicle animations, if it has any: the steering
    /// screen, the overlays (steering, sideways, speed, forward, unused,
    /// wheel) and each suspension's animation.
    pub animations: Option<VehicleAnimations>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct VehicleAnimations {
    pub steering_screen_bounds: ScreenBounds,
    pub animations: Vec<i16>,
    pub suspension_animations: Vec<i16>,
}

impl VehiclePoseDefinition {
    pub fn load(cache: &CacheFile, datum: u32) -> Result<Self> {
        use layout::{vehicle_animation as a, vehicle_definition as v, vehicle_suspension as s};
        let root = cache.tag_root(datum, crate::groups::OBJECT)?;
        let mut animations = None;
        if let Some(g) = root.reference(layout::_object_definition::ANIMATION_GRAPH)?.datum {
            let graph = cache.tag_root(g, crate::groups::ANIMATION_GRAPH)?;
            if let Some(first) = graph.block(layout::animation_graph::VEHICLE_ANIMATIONS, a::SIZEOF)?.iter().next() {
                animations = Some(VehicleAnimations {
                    steering_screen_bounds: ScreenBounds::read(first.add(a::STEERING_SCREEN_BOUNDS))?,
                    animations: first.block(a::ANIMATIONS, 2)?.iter().map(|p| p.i16(0)).collect::<Result<_>>()?,
                    suspension_animations: first.block(a::SUSPENSIONS, s::SIZEOF)?.iter().map(|p| p.i16(s::ANIMATION_INDEX)).collect::<Result<_>>()?,
                });
            }
        }
        Ok(VehiclePoseDefinition {
            maximum_forward_speed: root.f32(v::UNKNOWN2F8)?,
            maximum_reverse_speed: root.f32(v::UNKNOWN2FC)?,
            wheel_circumference: root.f32(v::WHEEL_CIRCUMFERENCE)?,
            animations,
        })
    }
}

/// What a vehicle's preprocess reads of it.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct VehiclePoseInputs {
    pub turn: f32,
    pub speed: f32,
    pub wheel: f32,
    pub suspension: [u8; MAXIMUM_SUSPENSIONS],
    pub forward: [f32; 3],
    pub up: [f32; 3],
    pub translational_velocity: [f32; 3],
}

/// `PIN(n, 0, 1)`.
fn pin01(n: f32) -> f32 {
    if n < 0.0 {
        0.0
    } else if n > 1.0 {
        1.0
    } else {
        n
    }
}

/// `vehicle_preprocess_node_orientations`. `animation` gives the graph's
/// animation of an index (`None` for one it doesn't have); those with more
/// nodes than the model are skipped (`unit_animation_get_fitting`).
pub fn vehicle_preprocess_node_orientations<'a>(
    definition: &VehiclePoseDefinition,
    animation: &dyn Fn(i16) -> Option<&'a PoseAnimation>,
    model_node_count: usize,
    inputs: &VehiclePoseInputs,
    orientations: &mut [Orientation],
) {
    let Some(vehicle_animations) = &definition.animations else { return };
    let fitting = |index: i16| animation(index).filter(|a| a.node_count as i32 <= model_node_count as i32);
    let overlay = |slot: usize| vehicle_animations.animations.get(slot).and_then(|&index| fitting(index));
    let frames = |a: &PoseAnimation| (a.frame_count as i32 - 1) as f32;

    if let Some(a) = overlay(0) {
        aiming_screen_apply(a, &vehicle_animations.steering_screen_bounds, inputs.turn, 0.0, orientations);
    }
    if let Some(a) = overlay(1) {
        // (triple_product3d: (up × forward) · velocity)
        let sideways = dot3(cross3(inputs.up, inputs.forward), inputs.translational_velocity);
        let value = (sideways / definition.maximum_forward_speed + 1.0) * 0.5;
        overlay_animation_apply_continuous(a, pin01(value) * frames(a), orientations);
    }
    if let Some(a) = overlay(2) {
        let value = if inputs.speed < 0.0 {
            0.5 - inputs.speed / definition.maximum_reverse_speed * 0.5
        } else {
            (inputs.speed / definition.maximum_forward_speed + 1.0) * 0.5
        };
        overlay_animation_apply_continuous(a, value * frames(a), orientations);
    }
    if let Some(a) = overlay(3) {
        let value = dot3(inputs.translational_velocity, inputs.forward);
        let value = pin01(value) / (definition.maximum_forward_speed as f64).abs() as f32;
        overlay_animation_apply_continuous(a, pin01(value) * frames(a), orientations);
    }
    if let Some(a) = overlay(5) {
        let value = if definition.wheel_circumference > 0.0 { inputs.wheel / definition.wheel_circumference } else { 0.0 };
        overlay_animation_apply_continuous(a, value * a.frame_count as f32, orientations);
    }
    for (suspension_index, &animation_index) in vehicle_animations.suspension_animations.iter().take(MAXIMUM_SUSPENSIONS).enumerate() {
        if let Some(a) = fitting(animation_index) {
            let compression = inputs.suspension[suspension_index];
            let value = if compression == 0xff { 1.0 } else { compression as f32 * (1.0 / 255.0) };
            overlay_animation_apply_continuous(a, value * frames(a), orientations);
        }
    }
}
