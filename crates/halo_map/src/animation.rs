//! Halo's animation graphs (`antr` tags), what movement reads of them: an
//! animation's per-frame root motion (`animation_get_frame_info` in OpenCE's
//! `source/models/model_animation_definitions.c`).

use crate::{CacheFile, Result, layout};

/// A frame's root motion: how far the animation moves its object forward
/// (`dx`), left (`dy`) and up (`dz`), and turns it (`dyaw`), in world units
/// and radians for the frame. `kind` is the animation's `frame_info_type`
/// (0 none, 1 dx dy, 2 dx dy dyaw, 3 dx dy dz dyaw).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct FrameInfo {
    pub kind: i16,
    pub dx: f32,
    pub dy: f32,
    pub dz: f32,
    pub dyaw: f32,
}

/// The root motion of an animation's frame, as `animation_get_frame_info`
/// gives it: zero for a frame past the animation's frame info. `None` for an
/// animation the graph doesn't have.
pub fn frame_info(cache: &CacheFile, graph: u32, animation_index: i16, frame_index: i16) -> Result<Option<FrameInfo>> {
    use layout::animation as a;
    let root = cache.tag_root(graph, crate::groups::ANIMATION_GRAPH)?;
    let animations = root.block(layout::animation_graph::ANIMATIONS, a::SIZEOF)?;
    let Some(animation) = (animation_index >= 0).then(|| animations.get(animation_index as usize)).flatten() else {
        return Ok(None);
    };
    let kind = animation.i16(a::FRAME_INFO_TYPE)?;
    let size: usize = match kind {
        1 => 8,
        2 => 12,
        3 => 16,
        _ => return Ok(Some(FrameInfo { kind, ..Default::default() })),
    };
    let data = animation.data_bytes(a::FRAME_INFO)?;
    let mut values = [0.0f32; 4];
    if frame_index >= 0 && (frame_index as usize + 1) * size <= data.len() {
        let frame = &data[frame_index as usize * size..][..size];
        for (i, value) in values.iter_mut().enumerate().take(size / 4) {
            *value = f32::from_le_bytes(frame[i * 4..i * 4 + 4].try_into().unwrap());
        }
    }
    Ok(Some(match kind {
        1 => FrameInfo { kind, dx: values[0], dy: values[1], ..Default::default() },
        2 => FrameInfo { kind, dx: values[0], dy: values[1], dz: 0.0, dyaw: values[2] },
        _ => FrameInfo { kind, dx: values[0], dy: values[1], dz: values[2], dyaw: values[3] },
    }))
}
