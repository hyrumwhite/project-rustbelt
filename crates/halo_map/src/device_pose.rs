//! Halo's device poses (ROADMAP phase 7, objects and physics): what a
//! device's type (a machine, a control, a light fixture) does to its pose
//! (`device_preprocess_node_orientations` in OpenCE's
//! `source/devices/devices.c`), ported as it is: the graph's position
//! overlay at the device's position (a door's opening, an elevator's
//! travel) and its power overlay at its power.

use crate::pose::{Orientation, PoseAnimation, overlay_animation_apply, overlay_animation_apply_continuous};
use crate::{CacheFile, Result, layout};

const DEVICE_POSITION_LOOPS: u32 = 1 << 0;
const DEVICE_POSITION_ANIMATION_NOT_INTERPOLATED: u32 = 1 << 1;
/// The device's own (runtime) flag.
const DEVICE_POSITION_REVERSED: u32 = 1 << 0;

/// What a device's preprocess reads of its definition and graph.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DevicePoseDefinition {
    /// The device definition's flags.
    pub flags: u32,
    /// The graph's first device animations (position, power), if it has
    /// any.
    pub animations: Option<Vec<i16>>,
}

impl DevicePoseDefinition {
    pub fn load(cache: &CacheFile, datum: u32) -> Result<Self> {
        use layout::animation_graph_device_animations as a;
        let root = cache.tag_root(datum, crate::groups::OBJECT)?;
        let mut animations = None;
        if let Some(g) = root.reference(layout::_object_definition::ANIMATION_GRAPH)?.datum {
            let graph = cache.tag_root(g, crate::groups::ANIMATION_GRAPH)?;
            if let Some(first) = graph.block(layout::animation_graph::DEVICE_ANIMATIONS, a::SIZEOF)?.iter().next() {
                animations = Some(first.block(a::ANIMATIONS, 2)?.iter().map(|p| p.i16(0)).collect::<Result<_>>()?);
            }
        }
        Ok(DevicePoseDefinition { flags: root.u32(layout::device_definition::DEVICE + layout::_device_definition::FLAGS)?, animations })
    }
}

/// `device_preprocess_node_orientations`. `animation` gives the graph's
/// animation of an index (`None` for one it doesn't have; these aren't
/// held to the model's node count).
pub fn device_preprocess_node_orientations<'a>(
    definition: &DevicePoseDefinition,
    animation: &dyn Fn(i16) -> Option<&'a PoseAnimation>,
    device_flags: u32,
    position: f32,
    power: f32,
    orientations: &mut [Orientation],
) {
    let Some(animations) = &definition.animations else { return };
    if let Some(a) = animations.first().and_then(|&index| animation(index)) {
        let position = if device_flags & DEVICE_POSITION_REVERSED != 0 { 1.0 - position } else { position };
        let frames = if definition.flags & DEVICE_POSITION_LOOPS != 0 { a.frame_count as f32 } else { (a.frame_count as i32 - 1) as f32 };
        let frame_index = position * frames;
        if definition.flags & DEVICE_POSITION_ANIMATION_NOT_INTERPOLATED != 0 {
            overlay_animation_apply(a, frame_index as i32 as i16, orientations);
        } else {
            overlay_animation_apply_continuous(a, frame_index, orientations);
        }
    }
    if let Some(a) = animations.get(1).and_then(|&index| animation(index)) {
        overlay_animation_apply_continuous(a, a.frame_count as f32 * power, orientations);
    }
}
