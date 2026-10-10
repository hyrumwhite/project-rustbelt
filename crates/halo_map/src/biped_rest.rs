//! The rest of Halo's biped update (ROADMAP phase 7, objects and physics),
//! from OpenCE's `source/units/bipeds.c`, ported as it is: a biped riding a
//! vehicle or another biped (`biped_update`'s parented branch), and what
//! follows the unparented update (`biped_try_to_make_footsteps`,
//! `biped_falling_danger`, `biped_check_discard`).
//!
//! What it asks of the rest of the game goes through a [`RestHost`] at the
//! moments the C asks it ([`call`]'s kinds): footsteps, the ground below a
//! falling biped, its scream, discarding it, vehicle speech and leaving a
//! seat.

use crate::{CacheFile, Result, layout};

/// What these read and change of a biped (the game's fields, laid out for
/// the game's side, `halo_sim.c`).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct RestState {
    pub throttle: [f32; 3],
    pub translational_velocity: [f32; 3],
    pub position: [f32; 3],
    /// `object_get_origin`.
    pub origin: [f32; 3],
    pub control_flags: u32,
    pub unit_flags: u32,
    pub object_flags: u32,
    pub damage_flags: u32,
    pub actor_index: i32,
    pub parent_object_index: i32,
    pub last_falling_communication_time: i32,
    pub animation_graph_index: u32,
    pub animation_index: i16,
    pub animation_frame_index: i16,
    pub location_cluster: i16,
    pub stop_ticks: i8,
    pub biped_state: i8,
    pub animation_state: i8,
    pub base_seat_index: i8,
    pub airborne_ticks: i8,
    /// The update's animation data: the state it asks for.
    pub state_desired: i8,
    /// The ground below a falling biped (`biped_find_ground_surface`'s
    /// point), as its call left it.
    pub ground_z: f32,
}

const _: () = assert!(std::mem::size_of::<RestState>() == 96);

/// What they read besides the state.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct RestInputs {
    /// The biped definition's flags.
    pub definition_flags: u32,
    pub game_time: i32,
    pub game_engine_running: bool,
    /// The falling damage's speed past which a fall kills
    /// (`runtime_maximum_damage_velocity`).
    pub maximum_damage_velocity: f32,
    /// What it rides: a vehicle (its up's k, and whether on the ground), a
    /// biped (whether dead), or something else.
    pub parent: Option<Parent>,
    /// `rider_ejection` (a debug global, on).
    pub rider_ejection: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Parent {
    Vehicle { up_k: f32, on_ground: bool },
    Biped { dead: bool },
    Other,
}

/// An animation's foot frames (`private_left_foot_frame_index`, `_right_`).
pub fn foot_frames(cache: &CacheFile, graph: u32, animation_index: i16) -> Result<(u8, u8)> {
    use layout::animation as a;
    let root = cache.tag_root(graph, crate::groups::ANIMATION_GRAPH)?;
    let animations = root.block(layout::animation_graph::ANIMATIONS, a::SIZEOF)?;
    let Some(animation) = animations.get(animation_index.max(0) as usize).filter(|_| animation_index >= 0) else { return Ok((0, 0)) };
    Ok((animation.u8(a::PRIVATE_LEFT_FOOT_FRAME_INDEX)?, animation.u8(a::PRIVATE_RIGHT_FOOT_FRAME_INDEX)?))
}

/// What they ask of the game (`RestHost::call`'s kinds).
pub mod call {
    /// `biped_make_footstep` of a shuffle, `a` the foot.
    pub const FOOTSTEP_SHUFFLE: i32 = 1;
    /// `biped_make_footstep` of a step, `a` the effect type (standing or
    /// not), `b` the foot (0 or 1).
    pub const FOOTSTEP: i32 = 2;
    /// `biped_find_ground_surface` 6 below: 1 if found (its point's z in the
    /// state's `ground_z`).
    pub const FIND_GROUND: i32 = 3;
    /// `unit_scream` of a fall.
    pub const SCREAM: i32 = 4;
    /// It is erased (`object_delete`).
    pub const DISCARD: i32 = 5;
    /// `biped_vehicle_speech`.
    pub const VEHICLE_SPEECH: i32 = 6;
    /// `unit_try_and_exit_seat`.
    pub const TRY_TO_EXIT_SEAT: i32 = 7;
    /// `unit_exit_seat_end`: thrown from an upturned vehicle.
    pub const EXIT_SEAT_END: i32 = 8;
}

/// A running game, as these ask it. The state is theirs as it stands (the
/// host writes it to the biped first, and reads back what the call
/// changed).
pub trait RestHost {
    fn call(&self, state: &mut RestState, kind: i32, a: i32, b: i32) -> i32;
}

/// The foot frames of the animation playing, if any.
pub type FootFrames = Option<(u8, u8)>;

// biped definition flags
const FLYING: u32 = 1 << 2;
const IMMUNE_TO_FALLING_DAMAGE: u32 = 1 << 7;
// unit flags
const NO_FALLING_DAMAGE: u32 = 1 << 12;
const CONTROL_ACTION: u32 = 1 << 6;
// object flags
const OBJECT_DEAD: u32 = 1 << 2;
const OBJECT_OUTSIDE_OF_MAP: u32 = 1 << 21;
const STATE_AI_IMPULSE: i8 = 29;
const BASE_SEAT_STAND: i8 = 2;
const BIPED_STATE_IDLE: i8 = 0;
const BIPED_STATE_MOVING: i8 = 1;
const NONE: i32 = -1;
const GLOBAL_GRAVITY: f32 = crate::physics::GLOBAL_GRAVITY;

/// `biped_update`'s parented branch: riding a vehicle (its speech, leaving
/// the seat when asked, thrown out when it lies upturned on the ground), or
/// a biped (the state it asks for: riding, or its dead mount's).
pub fn biped_riding(inputs: &RestInputs, state: &mut RestState, host: Option<&dyn RestHost>) {
    let call = |state: &mut RestState, kind: i32| host.map_or(0, |h| h.call(state, kind, 0, 0));
    match inputs.parent {
        Some(Parent::Vehicle { up_k, on_ground }) => {
            call(state, call::VEHICLE_SPEECH);
            if state.control_flags & CONTROL_ACTION != 0 {
                call(state, call::TRY_TO_EXIT_SEAT);
            }
            if inputs.rider_ejection && up_k < 0.0 && on_ground {
                call(state, call::EXIT_SEAT_END);
            }
        }
        Some(Parent::Biped { dead }) => {
            state.state_desired = ((if dead { OBJECT_DEAD } else { 0 }) | 0x20) as i8;
        }
        _ => {}
    }
}

/// What follows an unparented biped's update: its footsteps, the AI's
/// notice of a dangerous fall, and erasing it if it fell out of the world.
/// Returns whether it is still there.
pub fn biped_after_update(inputs: &RestInputs, feet: FootFrames, state: &mut RestState, host: Option<&dyn RestHost>) -> bool {
    let call = |state: &mut RestState, kind: i32, a: i32, b: i32| host.map_or(0, |h| h.call(state, kind, a, b));

    // (biped_try_to_make_footsteps)
    let mut moving = false;
    let mut air_contact = false;
    match state.animation_state {
        2 | 3 => air_contact = true,
        4..=7 => {
            let t = state.throttle;
            if t[0] * t[0] + t[1] * t[1] + t[2] * t[2] > 0.25 {
                moving = true;
            }
        }
        _ => {}
    }
    if state.animation_index != NONE as i16 {
        let (left, right) = feet.unwrap_or((0, 0));
        if air_contact {
            if state.animation_frame_index == 0 {
                call(state, call::FOOTSTEP_SHUFFLE, 0, 0);
                call(state, call::FOOTSTEP_SHUFFLE, 1, 0);
            }
        } else if moving && (left != 0 || right != 0) {
            let frame_index = state.animation_frame_index;
            let right_foot = if frame_index == left as i16 {
                Some(false)
            } else if frame_index == right as i16 {
                Some(true)
            } else {
                None
            };
            if let Some(right_foot) = right_foot {
                call(state, call::FOOTSTEP, (state.base_seat_index == BASE_SEAT_STAND) as i32, right_foot as i32);
            }
        }
    }
    match state.biped_state {
        BIPED_STATE_IDLE => {
            if state.stop_ticks > 0 {
                state.stop_ticks += 1;
                if state.stop_ticks > 3 {
                    call(state, call::FOOTSTEP_SHUFFLE, 0, 0);
                    call(state, call::FOOTSTEP_SHUFFLE, 1, 0);
                    state.stop_ticks = 0;
                }
            }
        }
        BIPED_STATE_MOVING => state.stop_ticks = 1,
        _ => state.stop_ticks = 0,
    }

    // (biped_falling_danger)
    if state.damage_flags & OBJECT_DEAD == 0
        && inputs.definition_flags & (FLYING | IMMUNE_TO_FALLING_DAMAGE) == 0
        && state.unit_flags & NO_FALLING_DAMAGE == 0
        && state.actor_index != NONE
        && state.animation_state != STATE_AI_IMPULSE
    {
        let game_time = inputs.game_time;
        if state.airborne_ticks > 30
            && (state.last_falling_communication_time == NONE || state.last_falling_communication_time.wrapping_add(15) < game_time)
        {
            let mut dangerous = true;
            state.last_falling_communication_time = game_time;
            if call(state, call::FIND_GROUND, 0, 0) != 0 {
                let v = state.translational_velocity[2];
                let maximum = inputs.maximum_damage_velocity;
                if v > 0.0 || (state.origin[2] - state.ground_z) * GLOBAL_GRAVITY * 2.0 + v * v < maximum * maximum {
                    dangerous = false;
                }
            }
            if dangerous {
                call(state, call::SCREAM, 0, 0);
            }
        }
    }

    // (biped_check_discard)
    if !inputs.game_engine_running
        && (state.object_flags & OBJECT_OUTSIDE_OF_MAP != 0 || state.location_cluster == NONE as i16)
        && state.position[2] < -2000.0
    {
        call(state, call::DISCARD, 0, 0);
        return false;
    }
    true
}
