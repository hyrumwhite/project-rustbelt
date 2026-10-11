//! Contrail updates (`contrails.c`'s `contrails_update`, each contrail's:
//! its density and activity, new points at its attachment's markers, its
//! animation and texture scroll, and its points' states and physics, with
//! `contrail_add_points`, `contrail_compute_new_point_count`,
//! `contrail_update_points` and `contrail_next_frame`). They run once a frame.
//!
//! A contrail's points are datums of their own, linked from the contrail;
//! the update takes the contrail with all of them and leaves them, the game
//! writing them back. What reads the world or allocates goes through
//! [`ContrailHost::call`] (contrails.c's `contrail_call`, shared with the C):
//! the object's function value, markers and velocity, locations, a point's
//! physics, its datum, and deleting the contrail.

// (the C's shape: its operand order and its per-component loops)
#![allow(clippy::assign_op_pattern, clippy::needless_range_loop)]

use crate::cache::CacheFile;
use crate::projectile::Random;
use crate::{Result, layout};

const NONE: i32 = -1;

/// `MAXIMUM_CONTRAIL_INSTANCES`.
pub const MAXIMUM_CONTRAIL_INSTANCES: usize = 4;

/// `_contrail_active_bit`.
const CONTRAIL_ACTIVE: u16 = 1 << 0;

/// A point's flags.
const POINT_NEW: u8 = 1 << 0;
const POINT_TRANSITIONING: u8 = 1 << 1;
const POINT_EXPIRED: u8 = 1 << 2;

/// A point state's scale flags.
const STATE_DURATION_BIT: u32 = 0;
const STATE_TRANSITION_DURATION_BIT: u32 = 2;

/// The definition's scale flags.
const SCALES_POINT_GENERATION_RATE: u32 = 0;
const SCALES_POINT_VELOCITY: u32 = 1;
const SCALES_POINT_VELOCITY_CONE_ANGLE: u32 = 3;
const SCALES_INHERITED_VELOCITY_FRACTION: u32 = 4;
const SCALES_SEQUENCE_ANIMATION_RATE: u32 = 5;
const SCALES_TEXTURE_ANIMATION_U: u32 = 8;
const SCALES_TEXTURE_ANIMATION_V: u32 = 9;

/// A contrail's calls of the rest of the game (halo_sim.h's
/// `_contrail_call_*`).
pub mod call {
    /// `object_get_function_value(a, b)` from `f[0]`: the result whether it
    /// has one, `out[0]` the value.
    pub const FUNCTION_VALUE: i32 = 1;
    /// The markers of the object `a`'s attachment `b`: the result their
    /// count, marker `c`'s position and forward in `out`.
    pub const MARKERS: i32 = 2;
    /// `object_get_velocities(a)`: its translational velocity in `out`.
    pub const VELOCITY: i32 = 3;
    /// A point's datum: the result its index (NONE for none).
    pub const POINT_NEW: i32 = 4;
    /// Deleting the point `a`'s datum.
    pub const POINT_DELETE: i32 = 5;
    /// `scenario_location_from_point` at `f`, from the location `a` (leaf),
    /// `b` (cluster), `c` (bonus): in `out_location_*`.
    pub const LOCATION_FROM_POINT: i32 = 6;
    /// `scenario_location_from_line` from `f[0..3]` to `f[3..6]`, from the
    /// location in `a`, `b`, `c`: in `out_location_*`.
    pub const LOCATION_FROM_LINE: i32 = 7;
    /// `point_physics_update` with the point physics `d`, from the location
    /// in `a`, `b`, `c`, the position `f[0..3]` and velocity `f[3..6]`, the
    /// radius `f[6]` and time `f[7]`: the position and velocity in `out`, the
    /// location in `out_location_*`.
    pub const PHYSICS: i32 = 8;
    /// `contrail_delete` (its points are none by then).
    pub const DELETE: i32 = 9;
    /// Making a contrail on the object `a`'s attachment `b`: its datum, the
    /// result its index (NONE for none), `out_location_leaf` the
    /// attachment's density function (`primary_scale_function_reference`
    /// less one).
    pub const NEW: i32 = 10;
}

/// halo_sim.h's `struct halo_sim_contrail_call`.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ContrailCall {
    pub kind: i32,
    pub a: i32,
    pub b: i32,
    pub c: i32,
    pub d: i32,
    pub f: [f32; 8],
    pub result: i32,
    pub out: [f32; 6],
    pub out_location_leaf: i32,
    pub out_location_cluster: i16,
    pub out_location_bonus: u16,
}

impl ContrailCall {
    pub fn new(kind: i32) -> Self {
        ContrailCall {
            kind,
            a: 0,
            b: 0,
            c: 0,
            d: 0,
            f: [0.0; 8],
            result: 0,
            out: [0.0; 6],
            out_location_leaf: 0,
            out_location_cluster: 0,
            out_location_bonus: 0,
        }
    }
}

/// What a contrail's update changes of the contrail (`struct
/// contrail_datum`), and the game's local random seed.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ContrailState {
    pub contrail_index: u32,
    pub definition_index: u32,
    pub flags: u16,
    pub attachment_index: i16,
    pub object_index: i32,
    pub density_function_index: i16,
    pub sequence_index: i16,
    pub frame_index: i16,
    pub pad: i16,
    pub density: f32,
    pub texture_offset_u: f32,
    pub texture_offset_v: f32,
    pub time_until_point: f32,
    pub frame_time: f32,
    pub expired_dt: f32,
    pub point_counts: [i16; MAXIMUM_CONTRAIL_INSTANCES],
    pub first_points: [i32; MAXIMUM_CONTRAIL_INSTANCES],
    /// `get_global_local_random_seed_address`'s.
    pub local_random_seed: u32,
}

/// One of a contrail's points (`struct contrail_point_datum`), with its
/// datum index; `next` is a datum index too.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ContrailPoint {
    pub index: i32,
    pub next: i32,
    pub flags: u8,
    pub state_index: i8,
    pub pad: u16,
    pub time: f32,
    pub delta: f32,
    pub density: f32,
    pub width: f32,
    pub location_leaf: i32,
    pub location_cluster: i16,
    pub location_bonus: u16,
    pub position: [f32; 3],
    pub velocity: [f32; 3],
}

const _: () = assert!(std::mem::size_of::<ContrailState>() == 76);
const _: () = assert!(std::mem::size_of::<ContrailPoint>() == 60);
const _: () = assert!(std::mem::size_of::<ContrailCall>() == 88);

/// What a frame's contrail updates read of the game besides the contrail.
pub struct ContrailInputs<'a> {
    /// The frame's time.
    pub dt: f32,
    /// The time new points are due for (the time since the last tick's
    /// first frame, on that frame; 0 on the others).
    pub point_dt: f32,
    /// `random_direction_table_get`'s table.
    pub directions: &'a [[f32; 3]],
}

/// The game's side of a contrail's update.
pub trait ContrailHost {
    /// A call: the seed (and for `DELETE` the contrail) written to the game
    /// first and read back after.
    fn call(&self, state: &mut ContrailState, call: &mut ContrailCall);
}

/// `struct contrail_point_state`, as an update reads it.
#[derive(Debug, Clone, Default)]
pub struct PointStateDefinition {
    pub duration_lower_bound: f32,
    pub duration_upper_bound: f32,
    pub transition_duration_lower_bound: f32,
    pub transition_duration_upper_bound: f32,
    pub physics: i32,
    pub width: f32,
    pub scale_flags: u32,
}

/// `struct contrail_definition`, as an update reads it.
#[derive(Debug, Clone, Default)]
pub struct ContrailDefinition {
    pub scale_flags: u32,
    pub point_generation_rate: f32,
    pub point_velocity_lower_bound: f32,
    pub point_velocity_upper_bound: f32,
    pub point_velocity_cone_angle: f32,
    pub point_inherited_velocity_fraction: f32,
    pub texture_animation_u: f32,
    pub texture_animation_v: f32,
    pub frames_per_second: f32,
    pub first_sequence_index: i16,
    pub sequence_count: i16,
    /// Its bitmap's sequences' bitmap counts.
    pub sequence_bitmap_counts: Vec<i16>,
    pub states: Vec<PointStateDefinition>,
}

impl ContrailDefinition {
    pub fn load(cache: &CacheFile, datum: u32) -> Result<Self> {
        use layout::{bitmap_group as b, bitmap_group_sequence as s, contrail_definition as c, contrail_point_state as p};
        let root = cache.tag_root(datum, crate::fourcc(b"cont"))?;
        let mut sequence_bitmap_counts = Vec::new();
        if let Some(bitmap) = root.reference(c::BITMAP)?.datum {
            let group = cache.tag_root(bitmap, crate::groups::BITMAP)?;
            for sequence in group.block(b::SEQUENCES, s::SIZEOF)?.iter() {
                sequence_bitmap_counts.push(sequence.i16(s::BITMAP_COUNT)?);
            }
        }
        let mut states = Vec::new();
        for state in root.block(c::STATES, p::SIZEOF)?.iter() {
            states.push(PointStateDefinition {
                duration_lower_bound: state.f32(p::DURATION_LOWER_BOUND)?,
                duration_upper_bound: state.f32(p::DURATION_UPPER_BOUND)?,
                transition_duration_lower_bound: state.f32(p::TRANSITION_DURATION_LOWER_BOUND)?,
                transition_duration_upper_bound: state.f32(p::TRANSITION_DURATION_UPPER_BOUND)?,
                physics: state.reference(p::PHYSICS)?.datum.map_or(NONE, |d| d as i32),
                width: state.f32(p::WIDTH)?,
                scale_flags: state.u32(p::SCALE_FLAGS)?,
            });
        }
        Ok(ContrailDefinition {
            scale_flags: root.u16(c::SCALE_FLAGS)? as u32,
            point_generation_rate: root.f32(c::POINT_GENERATION_RATE)?,
            point_velocity_lower_bound: root.f32(c::POINT_VELOCITY_LOWER_BOUND)?,
            point_velocity_upper_bound: root.f32(c::POINT_VELOCITY_UPPER_BOUND)?,
            point_velocity_cone_angle: root.f32(c::POINT_VELOCITY_CONE_ANGLE)?,
            point_inherited_velocity_fraction: root.f32(c::POINT_INHERITED_VELOCITY_FRACTION)?,
            texture_animation_u: root.f32(c::TEXTURE_ANIMATION_U)?,
            texture_animation_v: root.f32(c::TEXTURE_ANIMATION_V)?,
            frames_per_second: root.f32(c::FRAMES_PER_SECOND)?,
            first_sequence_index: root.i16(c::FIRST_SEQUENCE_INDEX)?,
            sequence_count: root.i16(c::SEQUENCE_COUNT)?,
            sequence_bitmap_counts,
            states,
        })
    }
}

/// `contrail_scale_value`.
fn scale_value(value: f32, scale: f32, flags: u32, bit: u32) -> f32 {
    let mut result = scale;
    if flags & (1 << bit) != 0 {
        result *= value;
    }
    result
}

/// `real_local_random_range`.
fn real_local_random_range(seed: &mut u32, lower_bound: f32, upper_bound: f32) -> f32 {
    *seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
    let random = (*seed >> 16) as f32 / 65535.0;
    lower_bound + (upper_bound - lower_bound) * random
}

/// `contrail_scale_random_value`.
fn scale_random_value(seed: &mut u32, value: f32, lower_bound: f32, upper_bound: f32, flags: u32, bit: u32) -> f32 {
    let minimum = scale_value(value, lower_bound, flags, bit);
    let range = scale_value(value, upper_bound - lower_bound, flags, bit + 1);
    real_local_random_range(seed, 0.0, range) + minimum
}

/// A contrail's update.
struct Update<'a, H: ContrailHost> {
    definition: &'a ContrailDefinition,
    inputs: &'a ContrailInputs<'a>,
    host: &'a H,
}

fn location_args(call: &mut ContrailCall, leaf: i32, cluster: i16, bonus: u16) {
    call.a = leaf;
    call.b = cluster as i32;
    call.c = bonus as i32;
}

fn point_mut(points: &mut [ContrailPoint], index: i32) -> Option<&mut ContrailPoint> {
    points.iter_mut().find(|p| p.index == index)
}

impl<H: ContrailHost> Update<'_, H> {
    fn call(&self, state: &mut ContrailState, call: &mut ContrailCall) {
        self.host.call(state, call);
    }

    /// `contrail_next_frame`.
    fn next_frame(&self, state: &mut ContrailState) {
        let definition = self.definition;
        let sequence_index = state.sequence_index;
        state.frame_time = 0.0;
        state.frame_index = state.frame_index.wrapping_add(1);
        let counts = &definition.sequence_bitmap_counts;
        if sequence_index < 0
            || sequence_index as usize >= counts.len()
            || state.frame_index < 0
            || state.frame_index >= counts[sequence_index as usize]
        {
            // local_random_range
            let seed = &mut state.local_random_seed;
            *seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let random = (*seed >> 16) as u16 as u32;
            let lower = definition.first_sequence_index;
            let upper = definition.sequence_count.wrapping_add(definition.first_sequence_index);
            let span = (upper as i32 - lower as i32) as u32;
            state.sequence_index = lower.wrapping_add((span.wrapping_mul(random) >> 16) as i16);
            state.frame_index = 0;
        }
    }

    /// `contrail_compute_new_point_count`.
    fn compute_new_point_count(&self, state: &mut ContrailState, mut dt: f32) -> i16 {
        let mut point_generation_rate = self.definition.point_generation_rate;
        if self.definition.scale_flags & (1 << SCALES_POINT_GENERATION_RATE) != 0 {
            point_generation_rate = point_generation_rate * state.density;
        }
        let time_between_points = 1.0 / point_generation_rate;
        let mut count: i16 = 0;
        while dt != 0.0 {
            if dt >= state.time_until_point {
                dt = dt - state.time_until_point;
                state.time_until_point = time_between_points;
                count = count.wrapping_add(1);
            } else {
                state.time_until_point = state.time_until_point - dt;
                break;
            }
        }
        count
    }

    /// `contrail_add_points`.
    fn add_points(&self, state: &mut ContrailState, points: &mut Vec<ContrailPoint>, count: i16, force: bool) {
        if count == 0 {
            return;
        }
        let definition = self.definition;
        let mut markers: Vec<([f32; 3], [f32; 3])> = Vec::new();
        let mut marker_count = 1;
        let mut k = 0;
        while k < marker_count && k < MAXIMUM_CONTRAIL_INSTANCES as i32 {
            let mut call = ContrailCall::new(call::MARKERS);
            call.a = state.object_index;
            call.b = state.attachment_index as i32;
            call.c = k;
            self.call(state, &mut call);
            marker_count = call.result;
            if k < marker_count {
                markers.push(([call.out[0], call.out[1], call.out[2]], [call.out[3], call.out[4], call.out[5]]));
            }
            k += 1;
        }
        if markers.is_empty() {
            return;
        }

        let point_velocity = scale_random_value(
            &mut state.local_random_seed,
            state.density,
            definition.point_velocity_lower_bound,
            definition.point_velocity_upper_bound,
            definition.scale_flags,
            SCALES_POINT_VELOCITY,
        );
        let point_velocity_cone_angle = scale_value(
            state.density,
            definition.point_velocity_cone_angle,
            definition.scale_flags,
            SCALES_POINT_VELOCITY_CONE_ANGLE,
        );
        let point_inherited_velocity_fraction = scale_value(
            state.density,
            definition.point_inherited_velocity_fraction,
            definition.scale_flags,
            SCALES_INHERITED_VELOCITY_FRACTION,
        );

        for (marker_index, (marker_position, marker_forward)) in markers.into_iter().enumerate() {
            let first = state.first_points[marker_index];
            let (previous, new_count) = if first != NONE {
                (points.iter().find(|p| p.index == first).copied(), count)
            } else {
                (None, 1)
            };
            if let Some(previous) = &previous
                && marker_position.map(f32::to_bits) == previous.position.map(f32::to_bits)
                && !force
            {
                continue;
            }

            for point_number in 1..=new_count {
                let mut call = ContrailCall::new(call::POINT_NEW);
                self.call(state, &mut call);
                let index = call.result;
                if index == NONE {
                    continue;
                }
                let mut point = ContrailPoint {
                    index,
                    flags: POINT_NEW | POINT_TRANSITIONING,
                    state_index: NONE as i8,
                    density: state.density,
                    ..Default::default()
                };
                let direction = {
                    let mut random = Random { seed: state.local_random_seed, directions: self.inputs.directions };
                    let direction = random.vector_in_cone(marker_forward, 0.0, point_velocity_cone_angle);
                    state.local_random_seed = random.seed;
                    direction
                };
                point.position = marker_position;
                let mut call = ContrailCall::new(call::LOCATION_FROM_POINT);
                location_args(&mut call, point.location_leaf, point.location_cluster, point.location_bonus);
                call.f[..3].copy_from_slice(&point.position);
                self.call(state, &mut call);
                (point.location_leaf, point.location_cluster, point.location_bonus) =
                    (call.out_location_leaf, call.out_location_cluster, call.out_location_bonus);
                let mut call = ContrailCall::new(call::VELOCITY);
                call.a = state.object_index;
                self.call(state, &mut call);
                let object_velocity = [call.out[0], call.out[1], call.out[2]];
                for c in 0..3 {
                    point.velocity[c] =
                        point_inherited_velocity_fraction * object_velocity[c] + direction[c] * point_velocity;
                }

                if point_number < new_count
                    && let Some(previous) = &previous
                {
                    let t = point_number as f32 / new_count as f32;
                    let one_minus_t = 1.0 - t;
                    point.density = one_minus_t * previous.density + t * point.density;
                    let mut interpolated = [0.0; 3];
                    for c in 0..3 {
                        interpolated[c] = one_minus_t * previous.position[c] + t * point.position[c];
                    }
                    let mut call = ContrailCall::new(call::LOCATION_FROM_LINE);
                    location_args(&mut call, point.location_leaf, point.location_cluster, point.location_bonus);
                    call.f[..3].copy_from_slice(&point.position);
                    call.f[3..6].copy_from_slice(&interpolated);
                    self.call(state, &mut call);
                    (point.location_leaf, point.location_cluster, point.location_bonus) =
                        (call.out_location_leaf, call.out_location_cluster, call.out_location_bonus);
                    point.position = interpolated;
                    for c in 0..3 {
                        point.velocity[c] = one_minus_t * previous.velocity[c] + t * point.velocity[c];
                    }
                }

                point.next = state.first_points[marker_index];
                state.point_counts[marker_index] = state.point_counts[marker_index].wrapping_add(1);
                state.first_points[marker_index] = index;
                points.push(point);
            }
        }
    }

    /// `contrail_update_points`.
    fn update_points(&self, state: &mut ContrailState, points: &mut Vec<ContrailPoint>, dt: f32) {
        let definition = self.definition;
        let states = &definition.states;
        for instance_index in 0..MAXIMUM_CONTRAIL_INSTANCES {
            let mut order: Vec<i32> = Vec::new();
            let mut index = state.first_points[instance_index];
            while index != NONE {
                let Some(mut point) = points.iter().find(|p| p.index == index).copied() else { break };
                if point.flags & POINT_EXPIRED == 0 {
                    point.time = dt * point.delta + point.time;
                    while point.delta == 0.0 || point.time > 1.0 {
                        if point.flags & POINT_TRANSITIONING != 0 {
                            let next = states.get((point.state_index as i32 + 1) as usize).cloned().unwrap_or_default();
                            point.state_index = point.state_index.wrapping_add(1);
                            point.time = 0.0;
                            point.delta = scale_random_value(
                                &mut state.local_random_seed,
                                point.density,
                                next.duration_lower_bound,
                                next.duration_upper_bound,
                                next.scale_flags,
                                STATE_DURATION_BIT,
                            );
                            if point.delta != 0.0 {
                                point.delta = 1.0 / point.delta;
                            }
                            point.flags &= !POINT_TRANSITIONING;
                        } else if (point.state_index as i32 + 1) < states.len() as i32 {
                            let current = states.get(point.state_index as usize).cloned().unwrap_or_default();
                            point.time = 0.0;
                            point.delta = scale_random_value(
                                &mut state.local_random_seed,
                                point.density,
                                current.transition_duration_lower_bound,
                                current.transition_duration_upper_bound,
                                current.scale_flags,
                                STATE_TRANSITION_DURATION_BIT,
                            );
                            if point.delta != 0.0 {
                                point.delta = 1.0 / point.delta;
                            }
                            point.flags |= POINT_TRANSITIONING;
                        } else {
                            point.flags |= POINT_EXPIRED;
                            break;
                        }
                    }
                }

                if point.flags & POINT_NEW != 0 {
                    point.flags &= !POINT_NEW;
                } else if point.flags & POINT_EXPIRED == 0 {
                    let current = states.get(point.state_index as usize).cloned().unwrap_or_default();
                    if current.physics != NONE {
                        let mut call = ContrailCall::new(call::PHYSICS);
                        location_args(&mut call, point.location_leaf, point.location_cluster, point.location_bonus);
                        call.d = current.physics;
                        call.f[..3].copy_from_slice(&point.position);
                        call.f[3..6].copy_from_slice(&point.velocity);
                        call.f[6] = current.width * 0.5;
                        call.f[7] = dt;
                        self.call(state, &mut call);
                        point.position = [call.out[0], call.out[1], call.out[2]];
                        point.velocity = [call.out[3], call.out[4], call.out[5]];
                        (point.location_leaf, point.location_cluster, point.location_bonus) =
                            (call.out_location_leaf, call.out_location_cluster, call.out_location_bonus);
                    }
                }

                if let Some(stored) = point_mut(points, index) {
                    *stored = point;
                }
                order.push(index);
                index = point.next;
            }

            let mut count = order.len();
            while count > 1 {
                count -= 1;
                let current = points.iter().find(|p| p.index == order[count]).copied().unwrap_or_default();
                let previous = points.iter().find(|p| p.index == order[count - 1]).copied().unwrap_or_default();
                if current.flags & POINT_EXPIRED == 0 || previous.flags & POINT_EXPIRED == 0 || current.next != NONE {
                    break;
                }
                if let Some(previous) = point_mut(points, order[count - 1]) {
                    previous.next = NONE;
                }
                state.point_counts[instance_index] = state.point_counts[instance_index].wrapping_sub(1);
                self.delete_point(state, points, order[count]);
            }

            if state.point_counts[instance_index] == 1 {
                let first = state.first_points[instance_index];
                let expired = points.iter().find(|p| p.index == first).is_some_and(|p| p.flags & POINT_EXPIRED != 0);
                if expired {
                    self.delete_point(state, points, first);
                    state.first_points[instance_index] = NONE;
                    state.point_counts[instance_index] = 0;
                }
            }
        }
    }

    fn delete_point(&self, state: &mut ContrailState, points: &mut Vec<ContrailPoint>, index: i32) {
        let mut call = ContrailCall::new(call::POINT_DELETE);
        call.a = index;
        self.call(state, &mut call);
        points.retain(|p| p.index != index);
    }

    /// The loop body of `contrails_update`: false if it deleted the contrail.
    fn update(&self, state: &mut ContrailState, points: &mut Vec<ContrailPoint>) -> bool {
        let definition = self.definition;
        let dt = self.inputs.dt;
        let frame_dt = dt - state.expired_dt;
        state.expired_dt = 0.0;

        if state.object_index != NONE {
            let mut call = ContrailCall::new(call::FUNCTION_VALUE);
            call.a = state.object_index;
            call.b = state.density_function_index as i32;
            call.f[0] = state.density;
            self.call(state, &mut call);
            state.density = call.out[0];
            let active = call.result != 0;
            if active != (state.flags & CONTRAIL_ACTIVE != 0) {
                let density = state.density;
                state.density = 0.0;
                self.add_points(state, points, 1, true);
                state.density = density;
            }
            if active {
                state.flags |= CONTRAIL_ACTIVE;
            } else {
                state.flags &= !CONTRAIL_ACTIVE;
            }
            if active {
                let count = self.compute_new_point_count(state, self.inputs.point_dt);
                self.add_points(state, points, count, true);
            }
        }

        let mut remaining_dt = frame_dt;
        let mut frames_per_second = definition.frames_per_second;
        if definition.scale_flags & (1 << SCALES_SEQUENCE_ANIMATION_RATE) != 0 {
            frames_per_second = frames_per_second * state.density;
        }
        let frame_period = 1.0 / frames_per_second;
        while remaining_dt > 0.0 {
            let time_until_next_frame = frame_period - state.frame_time;
            if time_until_next_frame <= remaining_dt {
                self.next_frame(state);
                remaining_dt = remaining_dt - time_until_next_frame;
            } else {
                state.frame_time = remaining_dt + state.frame_time;
                break;
            }
        }

        let mut texture_animation_u = definition.texture_animation_u;
        if definition.scale_flags & (1 << SCALES_TEXTURE_ANIMATION_U) != 0 {
            texture_animation_u = texture_animation_u * state.density;
        }
        state.texture_offset_u = state.texture_offset_u - texture_animation_u * frame_dt;
        let mut texture_animation_v = definition.texture_animation_v;
        if definition.scale_flags & (1 << SCALES_TEXTURE_ANIMATION_V) != 0 {
            texture_animation_v = texture_animation_v * state.density;
        }
        state.texture_offset_v = texture_animation_v * frame_dt + state.texture_offset_v;

        self.update_points(state, points, dt);

        if state.first_points.iter().all(|&first| first == NONE) && state.object_index == NONE {
            self.call(state, &mut ContrailCall::new(call::DELETE));
            return false;
        }
        true
    }
}

/// A contrail's update over the frame (the loop body of `contrails_update`),
/// from `state` and its `points`, where they are left (the points in no
/// order). False if it deleted the contrail.
pub fn contrail_update(
    definition: &ContrailDefinition,
    inputs: &ContrailInputs,
    host: &impl ContrailHost,
    state: &mut ContrailState,
    points: &mut Vec<ContrailPoint>,
) -> bool {
    Update { definition, inputs, host }.update(state, points)
}

/// A contrail's points in the order the game walks them: each instance's
/// list from its first point.
pub fn points_in_order(state: &ContrailState, points: &[ContrailPoint]) -> Vec<ContrailPoint> {
    let mut ordered = Vec::with_capacity(points.len());
    for &first in &state.first_points {
        let mut index = first;
        while index != NONE {
            let Some(point) = points.iter().find(|p| p.index == index) else { break };
            ordered.push(*point);
            index = point.next;
            if ordered.len() > points.len() {
                break;
            }
        }
    }
    ordered
}

/// `contrail_new`: the new contrail on the object's attachment (NONE for
/// none), from `state` (the seed; its index NONE), where it and its points
/// are left.
#[allow(clippy::too_many_arguments)]
pub fn contrail_new(
    definition: &ContrailDefinition,
    definition_index: i32,
    object_index: i32,
    attachment_index: i16,
    inputs: &ContrailInputs,
    host: &impl ContrailHost,
    state: &mut ContrailState,
    points: &mut Vec<ContrailPoint>,
) -> i32 {
    if definition_index == NONE {
        return NONE;
    }
    let update = Update { definition, inputs, host };
    let mut call = ContrailCall::new(call::NEW);
    call.a = object_index;
    call.b = attachment_index as i32;
    update.call(state, &mut call);
    let contrail_index = call.result;
    if contrail_index == NONE {
        return NONE;
    }
    // (the datum is new: zeros)
    let seed = state.local_random_seed;
    *state = ContrailState { contrail_index: contrail_index as u32, local_random_seed: seed, ..Default::default() };
    state.flags = 0;
    state.definition_index = definition_index as u32;
    state.object_index = object_index;
    state.attachment_index = attachment_index;
    state.density_function_index = call.out_location_leaf as i16;
    state.sequence_index = NONE as i16;
    update.next_frame(state);
    state.texture_offset_u = 0.0;
    state.texture_offset_v = 0.0;
    state.point_counts = [0; MAXIMUM_CONTRAIL_INSTANCES];
    state.first_points = [NONE; MAXIMUM_CONTRAIL_INSTANCES];

    let mut call = ContrailCall::new(call::FUNCTION_VALUE);
    call.a = state.object_index;
    call.b = state.density_function_index as i32;
    call.f[0] = state.density;
    update.call(state, &mut call);
    state.density = call.out[0];
    if call.result != 0 {
        state.flags |= CONTRAIL_ACTIVE;
        update.add_points(state, points, 1, true);
    }
    contrail_index
}
