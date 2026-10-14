//! An actor's senses: its line of sight to a point (ai.c's
//! `ai_test_line_of_sight`: the clusters' PVS, then the game's whole ray
//! test, widened about a player target), how well it sees a point
//! (actor_perception.c's `actor_visibility_at_point`, with
//! `actor_get_vision_distances`) and how well it hears one
//! (`actor_audibility_at_point`, by the structure's encoded sound distances
//! and its deafening background sounds).
//!
//! The structure and tags are read in the game's memory: its PVS
//! (`structure_bsp_get_cluster_pvs`), its sound distances
//! (`structure_bsp_get_cluster_encoded_sound_distance`) and a tag's data by
//! its index as `tag_get` finds it. The ray test is the game's in Rust
//! ([`AiHost::world_test_vector`]). Fog is none (`scenario_fog_at_point`
//! is zero in this build). The AI profile's meters, the collision user
//! stack and the debug switches (`ai_debug.fast_los`, the rendering of
//! lines of sight) are the game's alone.

use crate::ai_actor::{AiHost, AiMemory, AiMemoryExt, AiStepContext, NONE, object_get, tag_block_count, tag_block_element};
use crate::layout::{actor_datum as a, actor_debug_info as debug, actor_definition as d, actor_position_data as pos, structure_bsp as s};

/// ai.h's line of sight results.
pub mod line_of_sight {
    pub const CLEAR: i16 = 0;
    pub const OCCLUDED: i16 = 1;
    pub const FROM_COVER: i16 = 2;
    pub const TO_COVER: i16 = 3;
    pub const OBSTRUCTED: i16 = 4;
}

/// ai.h's line of sight modes.
pub mod mode {
    pub const NORMAL: i16 = 0;
    pub const EXPAND_SOURCE: i16 = 1;
    pub const EXPAND_TARGET: i16 = 2;
}

/// collisions.h's test flags the line of sight uses.
mod test {
    pub const FRONT_FACING_SURFACES: u32 = 1 << 0;
    pub const BACK_FACING_SURFACES: u32 = 1 << 1;
    pub const IGNORE_TWO_SIDED_SURFACES: u32 = 1 << 2;
    pub const IGNORE_BREAKABLE_SURFACES: u32 = 1 << 4;
    pub const STRUCTURE: u32 = 1 << 5;
    pub const OBJECTS: u32 = 1 << 7;
    pub const OBJECTS_VEHICLES: u32 = 1 << 9;
    pub const OBJECTS_SCENERY: u32 = 1 << 14;
    pub const OBJECTS_MACHINES: u32 = 1 << 15;
    /// `_collision_test_objects_sight_blocking_flags`.
    pub const OBJECTS_SIGHT_BLOCKING: u32 = OBJECTS_VEHICLES | OBJECTS_SCENERY | OBJECTS_MACHINES;
}

/// The tag groups the senses read.
const UNIT_TAG: u32 = crate::fourcc(b"unit");
const LOOPING_SOUND_TAG: u32 = crate::fourcc(b"lsnd");

/// cache_files.c's `struct cache_file_tag_instance`.
mod tag_instance {
    pub const SIZEOF: u32 = 0x20;
    pub const GROUP_TAG: u32 = 0x0;
    pub const PARENT_GROUP_TAGS: u32 = 0x4;
    pub const TAG_INDEX: u32 = 0xc;
    pub const BASE_ADDRESS: u32 = 0x14;
}

/// tag_groups.h's `struct tag_data`: its size, and its address.
mod tag_data {
    pub const SIZE: u32 = 0x0;
    pub const ADDRESS: u32 = 0xc;
}

/// `MAXIMUM_CLUSTERS_PER_STRUCTURE`.
const MAXIMUM_CLUSTERS_PER_STRUCTURE: i32 = 512;

/// An actor's sense position (actors.h's `struct actor_position_data`).
#[derive(Debug, Clone, Copy, Default)]
pub struct SensePosition {
    pub head: [f32; 3],
    pub body: [f32; 3],
    pub body_facing: [f32; 3],
    pub leaf: i32,
    pub cluster: i16,
    pub velocity: [f32; 3],
}

impl SensePosition {
    /// A copy of the position at `address` (as the game copies an actor's
    /// input position).
    pub fn read(m: &dyn AiMemory, address: u32) -> Self {
        Self {
            head: m.vector(address + pos::HEAD_POSITION as u32),
            body: m.vector(address + pos::BODY_POSITION as u32),
            body_facing: m.vector(address + pos::BODY_FACING as u32),
            leaf: m.i32(address + pos::BODY_LOCATION as u32),
            cluster: m.i16(address + pos::BODY_LOCATION_CLUSTER_INDEX as u32),
            velocity: m.vector(address + pos::VELOCITY as u32),
        }
    }
}

/// `tag_get`: the tag's data, as the game finds it by its index (None for
/// the empty tag's, all zeros: an index that is not a tag, or a tag of
/// another group or with no data).
pub fn tag_get(m: &dyn AiMemory, context: &AiStepContext, group_tag: u32, tag_index: i32) -> Option<u32> {
    let absolute_index = tag_index as i16;
    if context.tag_instances == 0 || absolute_index < 0 || absolute_index as i32 >= context.tag_count {
        return None;
    }
    let instance = context.tag_instances + absolute_index as u32 * tag_instance::SIZEOF;
    if tag_index as u32 & 0xFFFF_0000 != 0 && m.i32(instance + tag_instance::TAG_INDEX) != tag_index {
        return None;
    }
    let group = m.r32(instance + tag_instance::GROUP_TAG);
    let of_group = group == group_tag
        || m.r32(instance + tag_instance::PARENT_GROUP_TAGS) == group_tag
        || m.r32(instance + tag_instance::PARENT_GROUP_TAGS + 4) == group_tag;
    let base_address = m.r32(instance + tag_instance::BASE_ADDRESS);
    (of_group && base_address != 0).then_some(base_address)
}

/// network_distributed.c's `distributed_tag_of_group`: whether the index is
/// a loaded tag's of the group (its own or a parent's; `tag_iterator_next`'s
/// test). Tag indices are unique, so only the instance at its absolute
/// index can be it.
pub fn tag_of_group(m: &dyn AiMemory, context: &AiStepContext, group_tag: u32, tag_index: i32) -> bool {
    let absolute_index = tag_index as i16;
    if context.tag_instances == 0 || absolute_index < 0 || absolute_index as i32 >= context.tag_count {
        return false;
    }
    let instance = context.tag_instances + absolute_index as u32 * tag_instance::SIZEOF;
    m.i32(instance + tag_instance::TAG_INDEX) == tag_index
        && (m.r32(instance + tag_instance::GROUP_TAG) == group_tag
            || m.r32(instance + tag_instance::PARENT_GROUP_TAGS) == group_tag
            || m.r32(instance + tag_instance::PARENT_GROUP_TAGS + 4) == group_tag)
}

/// `BIT_VECTOR_SIZE_IN_LONGS`.
fn bit_vector_longs(bit_count: i32) -> i32 {
    (bit_count + 31) >> 5
}

/// `scenario_test_pvs`: whether the second cluster is in the first's PVS
/// (`structure_bsp_get_cluster_pvs`: none for a cluster or data the
/// structure doesn't have).
pub fn test_pvs(m: &dyn AiMemory, structure: u32, cluster_index0: i16, cluster_index1: i16) -> bool {
    let count = tag_block_count(m, structure + s::CLUSTERS as u32);
    let longs = bit_vector_longs(count);
    let size = m.i32(structure + s::CLUSTER_DATA as u32 + tag_data::SIZE);
    if cluster_index0 < 0
        || cluster_index0 as i32 >= count
        || count > MAXIMUM_CLUSTERS_PER_STRUCTURE
        || (cluster_index0 as i32 + 1) * 4 * longs > size
    {
        return false;
    }
    let pvs = m.r32(structure + s::CLUSTER_DATA as u32 + tag_data::ADDRESS) + 4 * cluster_index0 as u32 * longs as u32;
    let bit = cluster_index1 as i32;
    m.r32(pvs + 4 * (bit >> 5) as u32) & (1 << (bit & 31)) != 0
}

/// `structure_bsp_get_cluster_encoded_sound_distance`: 0 within a
/// cluster, the greatest (255) between clusters the data doesn't have.
fn encoded_sound_distance(m: &dyn AiMemory, structure: u32, from_cluster_index: i16, to_cluster_index: i16) -> u8 {
    if from_cluster_index == to_cluster_index {
        return 0;
    }
    let (row, column) = if from_cluster_index > to_cluster_index {
        (to_cluster_index as i32, from_cluster_index as i32)
    } else {
        (from_cluster_index as i32, to_cluster_index as i32)
    };
    let count = tag_block_count(m, structure + s::CLUSTERS as u32);
    let size = m.i32(structure + s::SOUND_CLUSTER_DATA as u32 + tag_data::SIZE);
    let offset = row * (count - 1) - row * (row + 1) / 2 + column - 1;
    if row < 0 || row >= column || column >= count || offset < 0 || offset >= size {
        return u8::MAX;
    }
    m.r8(m.r32(structure + s::SOUND_CLUSTER_DATA as u32 + tag_data::ADDRESS) + offset as u32)
}

/// `scenario_location_deafening`: whether the cluster's background sound
/// (one in the palette) is a looping sound that deafens.
fn location_deafening(m: &dyn AiMemory, context: &AiStepContext, cluster_index: i16) -> bool {
    use crate::layout::{structure_background_sound_palette_entry as entry, structure_cluster as cluster};
    let structure = context.structure;
    let cluster = tag_block_element(m, structure + s::CLUSTERS as u32, cluster_index as i32, cluster::SIZEOF);
    let palette_index = m.i16(cluster + cluster::BACKGROUND_SOUND_PALETTE_INDEX as u32);
    let palette = structure + s::BACKGROUND_SOUND_PALETTE as u32;
    if palette_index < 0 || palette_index as i32 >= tag_block_count(m, palette) {
        return false;
    }
    let background_sound = tag_block_element(m, palette, palette_index as i32, entry::SIZEOF);
    // (the tag reference's index)
    let sound_index = m.i32(background_sound + entry::BACKGROUND_SOUND as u32 + 0xc);
    if sound_index == NONE {
        return false;
    }
    tag_get(m, context, LOOPING_SOUND_TAG, sound_index).is_some_and(|sound| m.r32(sound) & 1 != 0)
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn magnitude_squared(v: [f32; 3]) -> f32 {
    v[0] * v[0] + v[1] * v[1] + v[2] * v[2]
}

/// `point_from_line3d`.
fn point_from_line(p: [f32; 3], v: [f32; 3], t: f32) -> [f32; 3] {
    [v[0] * t + p[0], v[1] * t + p[1], v[2] * t + p[2]]
}

/// `arctangent`: atan2 in double, as the game's.
fn arctangent(y: f32, x: f32) -> f32 {
    (y as f64).atan2(x as f64) as f32
}

/// `collision_test_line`: whether the ray between the points hits.
fn test_line(context: &AiStepContext, host: &impl AiHost, flags: u32, p0: [f32; 3], p1: [f32; 3], ignore: i32) -> bool {
    host.world_test_vector(context, flags, p0, sub(p1, p0), ignore).is_some()
}

/// `ai_test_line_of_sight`: clear, occluded, from or to cover, or
/// obstructed ([`line_of_sight`]).
#[allow(clippy::too_many_arguments)]
pub fn ai_test_line_of_sight(
    context: &AiStepContext,
    host: &impl AiHost,
    p0: [f32; 3],
    p0_cluster_index: i16,
    p1: [f32; 3],
    p1_cluster_index: i16,
    line_mode: i16,
    test_line_of_fire: bool,
    ignore_object_index: i32,
    ignore_vehicles: bool,
) -> i16 {
    use line_of_sight::*;
    let m = host.memory();
    if p0_cluster_index != NONE as i16
        && p1_cluster_index != NONE as i16
        && !test_pvs(m, context.structure, p0_cluster_index, p1_cluster_index)
    {
        return OBSTRUCTED;
    }
    let mut flags = test::FRONT_FACING_SURFACES
        | test::BACK_FACING_SURFACES
        | test::STRUCTURE
        | test::OBJECTS
        | test::OBJECTS_SIGHT_BLOCKING;
    if test_line_of_fire {
        flags |= test::IGNORE_BREAKABLE_SURFACES;
    } else {
        flags |= test::IGNORE_TWO_SIDED_SURFACES;
    }
    if ignore_vehicles {
        flags &= !test::OBJECTS_VEHICLES;
    }
    let hit = host.world_test_vector(context, flags, p0, sub(p1, p0), ignore_object_index);
    let clear_line_of_sight = hit.is_none();
    let collision_t = hit.unwrap_or(1.0);

    // (the fog between them is none: never more than 0.6)
    let mut classify_distance = !clear_line_of_sight;
    if line_mode != mode::NORMAL {
        let mut perpendicular = [p0[1] - p1[1], p1[0] - p0[0], 0.0];
        if crate::actor_transitions::normalize3(&mut perpendicular) == 0.0 {
            perpendicular = [1.0, 0.0, 0.0];
        }
        let blocked;
        if line_mode == mode::EXPAND_SOURCE {
            let p0a = [perpendicular[0] * 0.25 + p0[0], perpendicular[1] * 0.25 + p0[1], perpendicular[2] * 0.25 + p0[2]];
            let p0b = [p0[0] - perpendicular[0] * 0.25, p0[1] - perpendicular[1] * 0.25, p0[2] - perpendicular[2] * 0.25];
            if clear_line_of_sight {
                blocked = test_line(context, host, flags, p0a, p1, ignore_object_index)
                    || test_line(context, host, flags, p0b, p1, ignore_object_index);
            } else {
                blocked = !test_line(context, host, flags, p0a, p1, ignore_object_index)
                    || !test_line(context, host, flags, p0b, p1, ignore_object_index);
            }
        } else {
            if !clear_line_of_sight {
                return classify_collision_distance(p0, p1, collision_t);
            }
            let p1a = point_from_line(p1, perpendicular, 0.1);
            let p1b = point_from_line(p1, perpendicular, -0.1);
            let p1c = point_from_line(p1, [0.0, 0.0, -1.0], 0.1);
            blocked = test_line(context, host, flags, p1a, p0, ignore_object_index)
                || test_line(context, host, flags, p1b, p0, ignore_object_index)
                || test_line(context, host, flags, p1c, p0, ignore_object_index);
        }
        if blocked {
            return OCCLUDED;
        }
        classify_distance = !clear_line_of_sight;
    }
    if !classify_distance {
        return CLEAR;
    }
    classify_collision_distance(p0, p1, collision_t)
}

/// `ai_test_line_of_sight`'s blocked line by how far along it the block is.
fn classify_collision_distance(p0: [f32; 3], p1: [f32; 3], collision_t: f32) -> i16 {
    use line_of_sight::*;
    let distance = magnitude_squared(sub(p1, p0)).sqrt();
    if distance < 1.0 {
        return OBSTRUCTED;
    }
    if distance * collision_t < 1.0 {
        return FROM_COVER;
    }
    if (1.0 - collision_t) * distance < 4.0 {
        return TO_COVER;
    }
    OBSTRUCTED
}

/// actor_perception.c's `struct actor_perception_definition_view`: the
/// actor definition's perception as the vision distances read it.
mod vision {
    pub const MAXIMUM_VISION_ANGLE: u32 = 0x1c;
    pub const CENTRAL_VISION_ANGLE: u32 = 0x20;
    pub const PERIPHERAL_DISTANCE: u32 = 0x28;
    pub const MAXIMUM_PERIPHERAL_DISTANCE: u32 = 0x2c;
}

/// `actor_get_vision_distances`: how far the actor sees fully and partly
/// at a horizontal angle from where it looks.
fn vision_distances(m: &dyn AiMemory, context: &AiStepContext, maximum_vision_distance: f32, perception_factor: f32, horizontal_angle: f32) -> (f32, f32) {
    let definition = context.definition;
    if horizontal_angle > m.f32(definition + vision::PERIPHERAL_DISTANCE) {
        return (0.0, 0.0);
    }
    let maximum_distance = maximum_vision_distance * perception_factor;
    let peripheral_distance = perception_factor * m.f32(definition + vision::MAXIMUM_PERIPHERAL_DISTANCE);
    let minimum_full_distance = 0.7 * maximum_distance;
    let mut minimum_partial_distance = 0.7 * peripheral_distance;
    if minimum_partial_distance > 3.5 {
        minimum_partial_distance = 3.5;
    }
    let central_vision_angle = m.f32(definition + vision::CENTRAL_VISION_ANGLE);
    if horizontal_angle > central_vision_angle {
        return (minimum_partial_distance, peripheral_distance);
    }
    let maximum_vision_angle = m.f32(definition + vision::MAXIMUM_VISION_ANGLE);
    let full_vision_angle = 0.8 * maximum_vision_angle;
    let partial_distance = if horizontal_angle < maximum_vision_angle {
        maximum_distance
    } else {
        let interpolation = (horizontal_angle - maximum_vision_angle) / (central_vision_angle - maximum_vision_angle);
        (1.0 - interpolation) * maximum_distance + peripheral_distance * interpolation
    };
    let full_distance = if horizontal_angle < full_vision_angle {
        minimum_full_distance
    } else {
        let interpolation = (horizontal_angle - full_vision_angle) / (central_vision_angle - full_vision_angle);
        (1.0 - interpolation) * minimum_full_distance + minimum_partial_distance * interpolation
    };
    (full_distance, partial_distance)
}

/// `actor_visibility_at_point`: how well (0 to 3) the actor sees the
/// target from its sense position, by its line of sight, its vision's
/// range (its firing variant's, scaled by its knowledge), the lighting,
/// and (looking, not a swarm) the angle it looks at. A player's sighting
/// is noted in its debug info. `firing_variant` gives the firing variant's
/// definition (asked only when the line of sight lets it see).
#[allow(clippy::too_many_arguments)]
pub fn visibility_at_point(
    m: &dyn AiMemory,
    context: &AiStepContext,
    position: &SensePosition,
    target_position: [f32; 3],
    lighting: i8,
    line_of_sight: i16,
    use_maximum_distance: bool,
    target_is_player: bool,
    perception_knowledge: i16,
    firing_variant: impl FnOnce() -> u32,
) -> i16 {
    let mut result = 0;
    if line_of_sight != 0 && line_of_sight != 1 {
        return result;
    }
    let at = |offset: usize| context.actor + offset as u32;
    let definition = context.definition;
    let firing_variant = firing_variant();
    let mut maximum_distance = m.f32(definition + d::PERCEPTION_MAXIMUM_VISION_DISTANCE as u32);
    let modified_vision_range =
        m.f32(firing_variant + crate::layout::actor_variant_definition::RANGED_COMBAT_MODIFIED_VISION_RANGE as u32);
    if modified_vision_range > 0.0 {
        maximum_distance = modified_vision_range;
    }
    let knowledge_factor = match perception_knowledge {
        0 => 0.4,
        1 => 0.6,
        2 => 0.8,
        _ => 1.0,
    };
    maximum_distance *= knowledge_factor;
    let direction = sub(target_position, position.head);
    let distance_squared = magnitude_squared(direction);
    if distance_squared < maximum_distance * maximum_distance {
        let mut perception_factor = 1.0f32;
        if m.r32(definition + d::FLAGS as u32) & 1 == 0 {
            match lighting {
                0 => perception_factor = 0.3,
                1 => perception_factor = 0.7,
                _ => {}
            }
        }
        // (the fog between them is none)
        if !(perception_factor > 0.15) {
            perception_factor = 0.15;
        }
        if target_is_player {
            let debug_at = |offset: usize| context.debug_info + offset as u32;
            m.w32(debug_at(debug::VISION_LAST_TIME), context.game_time as u32);
            m.set_f32(debug_at(debug::VISION_LAST_MAXIMUM_DISTANCE), maximum_distance);
            m.set_f32(debug_at(debug::VISION_LAST_PERCEPTION_FACTOR), perception_factor);
        }
        let mut visible_distance = perception_factor * maximum_distance;
        if distance_squared < visible_distance * visible_distance {
            let full_distance;
            if !m.bool(at(a::META_SWARM)) && use_maximum_distance {
                let facing = [
                    dot(direction, m.vector(at(a::INPUT_LOOKING_VECTOR))),
                    dot(direction, m.vector(at(a::INPUT_LOOKING_LEFT_VECTOR))),
                    dot(direction, m.vector(at(a::INPUT_LOOKING_UP_VECTOR))),
                ];
                let pitch = arctangent(facing[2], (facing[0] * facing[0] + facing[1] * facing[1]).sqrt());
                if pitch > 0.5235988 || pitch < -0.7853982 {
                    full_distance = 0.0;
                    visible_distance = 0.0;
                } else {
                    let (full, partial) = vision_distances(
                        m,
                        context,
                        maximum_distance,
                        perception_factor,
                        arctangent(facing[1], facing[0]).abs(),
                    );
                    full_distance = full;
                    visible_distance = partial;
                }
            } else {
                full_distance = 0.7 * visible_distance;
            }
            if line_of_sight == 0 && distance_squared < full_distance * full_distance {
                result = if distance_squared < 36.0 { 3 } else { 2 };
            } else if distance_squared < visible_distance * visible_distance {
                result = 1;
            }
        }
    }
    result
}

/// `actor_audibility_at_point`: how well (0, 2 or 3) the actor hears a
/// sound of a type (a unit's constant sound) from its sense position: by
/// its hearing, the way it faces, its mode, the sound's type, deafening
/// background sounds and the line of sight, against the structure's sound
/// distance between the clusters. Its debug info notes the hearing.
pub fn audibility_at_point(
    m: &dyn AiMemory,
    context: &AiStepContext,
    position: &SensePosition,
    source_position: [f32; 3],
    source_cluster_index: i16,
    source_type: i16,
    line_of_sight: i16,
) -> i16 {
    let mut result = 0i16;
    if source_type == 0 || position.cluster == NONE as i16 || source_cluster_index == NONE as i16 {
        return result;
    }
    let mut maximum_distance = m.f32(context.definition + d::PERCEPTION_HEARING_DISTANCE as u32);
    let mut encoded_distance = -1.0f32;
    let mut audible_distance = -1.0f32;
    let direction = sub(source_position, position.head);
    let distance_squared = magnitude_squared(direction);
    if dot(direction, position.body_facing) < 0.0 {
        maximum_distance *= 0.8;
    }
    let combat_status = m.i16(context.actor + a::STATE_MODE as u32);
    if combat_status == 2 {
        maximum_distance *= 0.7;
    } else if combat_status == 1 {
        maximum_distance *= 0.4;
    }
    if source_type == 4 {
        maximum_distance *= 0.2;
    } else if source_type == 1 {
        maximum_distance *= 0.45;
    } else if source_type == 3 {
        maximum_distance *= 0.7;
    }
    if location_deafening(m, context, position.cluster) || location_deafening(m, context, source_cluster_index) {
        maximum_distance *= 0.25;
    }
    if line_of_sight != 0 && line_of_sight != 1 {
        maximum_distance *= 0.7;
    }
    if distance_squared < maximum_distance * maximum_distance {
        let encoded_sound_distance = encoded_sound_distance(m, context.structure, source_cluster_index, position.cluster);
        if encoded_sound_distance & 0x80 == 0 {
            encoded_distance = (encoded_sound_distance & 0x7f) as f32 * 2.015748;
            audible_distance = encoded_distance * 2.0;
            let distance = distance_squared.sqrt();
            audible_distance = if audible_distance > distance { audible_distance } else { distance };
            if audible_distance < maximum_distance {
                result = (source_type >= 3) as i16 + 2;
            }
        }
    }
    let debug_at = |offset: usize| context.debug_info + offset as u32;
    m.set_f32(debug_at(debug::FIELD_A8), maximum_distance);
    m.set_bool(debug_at(debug::FIELD_A4), true);
    m.set_i16(debug_at(debug::FIELD_A6), result);
    m.set_f32(debug_at(debug::FIELD_B0), encoded_distance);
    m.set_f32(debug_at(debug::FIELD_AC), distance_squared.sqrt());
    m.set_f32(debug_at(debug::FIELD_B4), audible_distance);
    result
}

/// The constant sound of a unit's definition (`unit_definition_get`'s
/// `unit.constant_sound`; none of an object that is gone).
pub fn unit_constant_sound(m: &dyn AiMemory, context: &AiStepContext, unit_index: i32) -> i16 {
    use crate::layout::{_unit_definition, unit_datum, unit_definition};
    let Some(unit) = object_get(m, context, unit_index) else { return 0 };
    let definition_index = m.i32(unit + unit_datum::DEFINITION_INDEX as u32);
    tag_get(m, context, UNIT_TAG, definition_index)
        .map_or(0, |definition| m.i16(definition + (unit_definition::UNIT + _unit_definition::CONSTANT_SOUND) as u32))
}
