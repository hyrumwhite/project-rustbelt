//! Halo's unit update (ROADMAP phase 7, objects and physics): `unit_update`
//! in OpenCE's `source/units/units.c`, ported as it is, in the same
//! single-precision arithmetic.
//!
//! Each tick a unit takes its controls (its driver's and gunner's, if it
//! carries them; a blind run's), runs down its timers (persistent controls,
//! camouflage, stun, dropping its weapon, feigning death, delayed damage,
//! flaming death), readies the weapon and grenade it wants, zooms, turns
//! its aim and look toward where it wants them (within its animation's
//! euler screens, limited in speed and acceleration), throws grenades,
//! hands its weapon its controls, powers its seats, and works its
//! integrated light and night vision.
//!
//! What it asks of the rest of the game goes through a [`UnitUpdateHost`],
//! at the moments the C asks it ([`call`]'s kinds): weapons, grenades,
//! death, sounds and effects, the AI, dialogue, illumination. The game's
//! random seed is the update's own as a blind run draws from it.

use crate::biped_update::euler_aiming_update_oriented;
use crate::model::Matrix4x3;
use crate::{CacheFile, Result, layout};

/// What a unit's update reads and changes of it (the game's fields, laid
/// out for the game's side, `halo_sim.c`).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct UnitUpdateState {
    pub flags: u32,
    pub control_flags: u32,
    pub persistent_control_timer: i32,
    pub persistent_control_flags: u32,
    pub desired_facing_vector: [f32; 3],
    pub desired_aiming_vector: [f32; 3],
    pub aiming_vector: [f32; 3],
    pub aiming_velocity: [f32; 3],
    pub desired_looking_vector: [f32; 3],
    pub looking_vector: [f32; 3],
    pub looking_velocity: [f32; 3],
    pub throttle: [f32; 3],
    pub primary_trigger: f32,
    pub timer: i16,
    pub grenade_throw_ticks: i16,
    pub weapon_drop_delay_ticks: i8,
    pub grenade_throw_state: i8,
    pub current_grenade_index: i8,
    pub desired_grenade_index: i8,
    pub grenade_counts: [i8; 2],
    pub current_zoom_level: i8,
    pub desired_zoom_level: i8,
    pub gunner_inactive_ticks: i8,
    pub aiming_change: u8,
    pub flaming_death_delay: u8,
    pub aiming_speed: i8,
    pub cause_for_camo_regrowth: i16,
    pub body_stun_ticks: i16,
    pub feign_death_timer: i16,
    pub delayed_damage_timer: i16,
    pub last_damage_category: i16,
    pub owner_team_index: i16,
    pub delayed_damage_peak: f32,
    pub delayed_damage_attacker_object_index: i32,
    pub body_stun: f32,
    pub seat_power: [f32; 2],
    pub integrated_light_power: f32,
    pub integrated_light_battery: f32,
    pub integrated_night_vision_power: f32,
    pub seat_acceleration: [f32; 3],
    pub seat_desired_acceleration: [f32; 3],
    pub active_camouflage: f32,
    pub active_camouflage_super_amount: f32,
    pub mouth_aperture: f32,
    pub run_blindly_angle: f32,
    pub run_blindly_angle_delta: f32,
    // what it reads, which its calls may change
    pub current_weapon_index: i16,
    pub desired_weapon_index: i16,
    pub animation_flags: u16,
    pub animation_state: i8,
    pub aiming_with_euler_screen: u8,
    pub looking_with_euler_screen: u8,
    pub pad: [u8; 3],
    pub aiming_screen_bounds: [f32; 4],
    pub looking_screen_bounds: [f32; 4],
    pub player_index: i32,
    pub driver_object_index: i32,
    pub gunner_object_index: i32,
    pub parent_object_index: i32,
    pub object_flags: u32,
    pub damage_flags: u32,
    pub body_vitality: f32,
    /// Its animation's frame (a grenade's wind-up reads it).
    pub animation_frame_index: i16,
    /// A biped's melee counter (0 for another unit).
    pub player_melee_ticks: i8,
    pub pad2: i8,
    /// `object_get_orientation`, and its own forward (`object.forward`).
    pub forward: [f32; 3],
    pub up: [f32; 3],
    pub object_forward: [f32; 3],
}

const _: () = assert!(std::mem::size_of::<UnitUpdateState>() == 332);

/// The game's global unit timer (`unit_globals`): which unit's turn it is
/// to use the time-sliced work (illumination).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct UnitGlobals {
    pub next_timer: i16,
    pub highest_timer: i16,
    pub used_time: u8,
    pub pad: [u8; 3],
}

/// The unit carrying it: its driver's or gunner's controls, as they are.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Controller {
    pub owner_team_index: i16,
    /// A player's, or not entering or leaving its seat.
    pub controls: bool,
    pub control_flags: u32,
    pub desired_facing_vector: [f32; 3],
    pub desired_aiming_vector: [f32; 3],
    pub throttle: [f32; 3],
    pub primary_trigger: f32,
}

/// What a unit's update reads besides its state.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct UnitUpdateInputs {
    pub allow_integrated_lights: bool,
    pub game_engine_running: bool,
    pub cheat_jetpack: bool,
    pub cheat_infinite_ammo: bool,
    pub driver: Option<Controller>,
    pub gunner: Option<Controller>,
    /// Its current weapon's camouflage regrowth rate (0 for none).
    pub weapon_camo_regrowth_rate: f32,
}

/// A unit definition, what its update reads.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct UnitUpdateDefinition {
    pub flags: u32,
    pub aiming_velocity_maximum: f32,
    pub aiming_acceleration_maximum: f32,
    pub casual_aiming_modifier: f32,
    pub looking_velocity_maximum: f32,
    pub looking_acceleration_maximum: f32,
    /// Each powered seat's power-up and power-down times.
    pub powered_seats: Vec<(f32, f32)>,
}

impl UnitUpdateDefinition {
    pub fn load(cache: &CacheFile, datum: u32) -> Result<Self> {
        use layout::{_unit_definition as u, powered_seat_definition as p};
        let root = cache.tag_root(datum, crate::groups::OBJECT)?.add(layout::unit_definition::UNIT);
        let mut powered_seats = Vec::new();
        for seat in root.block(u::POWERED_SEATS, p::SIZEOF)?.iter() {
            powered_seats.push((seat.f32(p::POWERUP_TIME)?, seat.f32(p::POWERDOWN_TIME)?));
        }
        Ok(UnitUpdateDefinition {
            flags: root.u32(u::FLAGS)?,
            aiming_velocity_maximum: root.f32(u::AIMING_VELOCITY_MAXIMUM)?,
            aiming_acceleration_maximum: root.f32(u::AIMING_ACCELERATION_MAXIMUM)?,
            casual_aiming_modifier: root.f32(u::CASUAL_AIMING_MODIFIER)?,
            looking_velocity_maximum: root.f32(u::LOOKING_VELOCITY_MAXIMUM)?,
            looking_acceleration_maximum: root.f32(u::LOOKING_ACCELERATION_MAXIMUM)?,
            powered_seats,
        })
    }
}

/// A weapon definition's camouflage regrowth rate.
pub fn weapon_camo_regrowth_rate(cache: &CacheFile, datum: u32) -> Result<f32> {
    let root = cache.tag_root(datum, crate::groups::OBJECT)?.add(layout::weapon_definition::WEAPON);
    root.f32(layout::_weapon_definition::ACTIVE_CAMO_REGROWTH_RATE)
}

/// What the update asks of the game (`UnitUpdateHost::call`'s kinds).
pub mod call {
    /// Its actor's blind run (the vector written to its desired facing):
    /// 1 if it has one.
    pub const RUNNING_BLIND_VECTOR: i32 = 1;
    pub const DROP_CURRENT_WEAPON: i32 = 2;
    pub const RESURRECT: i32 = 3;
    pub const DIED: i32 = 4;
    /// Its desired weapon readied, if it may use it.
    pub const READY_DESIRED_WEAPON: i32 = 5;
    /// `unit_inventory_next_grenade` from `a`: the grenade (NONE for none).
    pub const NEXT_GRENADE: i32 = 6;
    /// Its zoom changed (the weapon's zoom sound, for a local player).
    pub const ZOOM_CHANGED: i32 = 7;
    pub const GRENADE_THROW_BEGIN: i32 = 8;
    pub const GRENADE_MOVE_TO_HAND: i32 = 9;
    pub const GRENADE_RELEASE: i32 = 10;
    /// `weapon_set_integrated_light_power` of its current weapon.
    pub const WEAPON_LIGHT_POWER: i32 = 11;
    /// `weapon_owner_update` of its current weapon: the controls `a`, the
    /// trigger `b`.
    pub const WEAPON_OWNER_UPDATE: i32 = 12;
    pub const SEAT_UPDATE: i32 = 13;
    /// The AI's damage, delayed.
    pub const DELAYED_DAMAGE: i32 = 14;
    pub const CONTINUOUS_MELEE_DAMAGE: i32 = 15;
    pub const DIALOGUE_UPDATE: i32 = 16;
    pub const REFRESH_ILLUMINATION: i32 = 17;
    pub const FLAME_TO_DEATH: i32 = 18;
    /// `unit_integrated_night_vision_is_active`: 1 if so.
    pub const NIGHT_VISION_ACTIVE: i32 = 19;
    /// The night vision's on or off effect.
    pub const NIGHT_VISION_EFFECT: i32 = 20;
    /// The integrated light's toggle effect.
    pub const LIGHT_EFFECT: i32 = 21;
}

/// A running game, as a unit's update asks it. The state is the update's as
/// it stands (the host writes it to the unit first, and reads back what the
/// call changed).
pub trait UnitUpdateHost {
    fn random_seed(&self) -> u32;
    fn set_random_seed(&self, seed: u32);
    fn call(&self, state: &mut UnitUpdateState, kind: i32, a: i32, b: f32) -> i32;
}

// `_unit` flags
const ACTIVELY_CONTROLLED: u32 = 1 << 0;
const ACTIVE_CAMOUFLAGED: u32 = 1 << 4;
const SUPER_CAMOUFLAGED: u32 = 1 << 5;
const INTEGRATED_LIGHT_ON: u32 = 1 << 19;
const RUNNING_BLINDLY: u32 = 1 << 25;
const INTEGRATED_NIGHT_VISION_ON: u32 = 1 << 26;
const POSSESSED_BY_RECORDING: u32 = 1 << 27;
const DESIRED_INTEGRATED_LIGHT_ON: u32 = 1 << 28;
const DESIRED_INTEGRATED_LIGHT_OFF: u32 = 1 << 29;
// `_unit_definition` flags
const HAS_NO_AIMING: u32 = 1 << 10;
const SIMPLE_CREATURE: u32 = 1 << 11;
const INTEGRATED_LIGHT_CONTROLS_WEAPON_DIRECTLY: u32 = 1 << 23;
const INTEGRATED_LIGHT_LASTS_FOREVER: u32 = 1 << 24;
// `_unit_control` bits
const CONTROL_INTEGRATED_LIGHT: u32 = 1 << 4;
const CONTROL_RELOAD: u32 = 1 << 10;
const CONTROL_PRIMARY_TRIGGER: u32 = 1 << 11;
const CONTROL_SECONDARY_TRIGGER: u32 = 1 << 12;
const CONTROL_THROW_GRENADE: u32 = 1 << 13;
const CONTROL_DRIVER_MASK: u32 = 0x3f;
const CONTROL_GUNNER_MASK: u32 = 0x7c00;
// `_weapon_control` bits
const WEAPON_INTEGRATED_LIGHT: i32 = 1 << 0;
const WEAPON_PRIMARY_TRIGGER: i32 = 1 << 1;
const WEAPON_SECONDARY_TRIGGER: i32 = 1 << 2;
const WEAPON_RELOAD: i32 = 1 << 3;
const WEAPON_USER_BUSY: i32 = 1 << 4;
const WEAPON_USER_SWITCHING: i32 = 1 << 5;
const WEAPON_ZOOMED: i32 = 1 << 6;
// object flags
const OBJECT_AT_REST: u32 = 1 << 5;
// object damage flags
const OBJECT_DEAD: u32 = 1 << 2;
const OBJECT_CANNOT_HOLD_WEAPON: u32 = 1 << 10;
const ANIMATION_SHOWING_ACCELERATION: u16 = 1 << 1;
// grenade throws
const THROW_IDLE: i8 = 0;
const THROW_WIND_UP: i8 = 1;
const THROW_IN_HAND: i8 = 2;
const THROW_ENDING: i8 = 3;
const STATE_THROW_GRENADE: i8 = 33;
const AIMING_SPEED_CASUAL: i8 = 1;
const CAMO_REGROWTH_DEFAULT: i16 = 0;
const CAMO_REGROWTH_SHOT_FIRED: i16 = 1;

const NONE: i32 = -1;
const TICKS_PER_SECOND: f32 = 30.0;
/// `_pi`, as a float.
const PI: f32 = std::f32::consts::PI;
const UP: [f32; 3] = [0.0, 0.0, 1.0];
const FORWARD: [f32; 3] = [1.0, 0.0, 0.0];

/// `DEGREES_TO_RADIANS`.
fn degrees(angle: f32) -> f32 {
    angle * PI / 180.0
}
/// `MIN` and `PIN`, as the macros compare.
fn min(a: f32, b: f32) -> f32 {
    if a > b { b } else { a }
}
fn pin(n: f32, a: f32, b: f32) -> f32 {
    if n < a {
        a
    } else if n > b {
        b
    } else {
        n
    }
}
fn dot3(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross3(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn magnitude_squared3(v: [f32; 3]) -> f32 {
    v[0] * v[0] + v[1] * v[1] + v[2] * v[2]
}
/// `normalize3d`: the magnitude, 0 (leaving the vector) below epsilon.
fn normalize3(v: &mut [f32; 3]) -> f32 {
    let magnitude = magnitude_squared3(*v).sqrt();
    if !(0.0001 > magnitude.abs()) {
        let inverse = 1.0 / magnitude;
        *v = [v[0] * inverse, v[1] * inverse, v[2] * inverse];
        magnitude
    } else {
        0.0
    }
}
fn sine(a: f32) -> f32 {
    (a as f64).sin() as f32
}
fn cosine(a: f32) -> f32 {
    (a as f64).cos() as f32
}
/// `rotate_vector_about_axis`.
fn rotate_vector_about_axis(v: [f32; 3], n: [f32; 3], sine: f32, cosine: f32) -> [f32; 3] {
    let one_minus_cosine_times_v_dot_n = dot3(n, v) * (1.0 - cosine);
    let v_cross_n = [n[2] * v[1] - v[2] * n[1], n[0] * v[2] - v[0] * n[2], n[1] * v[0] - v[1] * n[0]];
    [
        cosine * v[0] + one_minus_cosine_times_v_dot_n * n[0] - sine * v_cross_n[0],
        cosine * v[1] + one_minus_cosine_times_v_dot_n * n[1] - sine * v_cross_n[1],
        cosine * v[2] + one_minus_cosine_times_v_dot_n * n[2] - sine * v_cross_n[2],
    ]
}
/// `angle_between_vectors3d`.
fn angle_between_vectors3d(a: [f32; 3], b: [f32; 3]) -> f32 {
    let aa_bb = magnitude_squared3(a) * magnitude_squared3(b);
    if aa_bb == 0.0 {
        return 0.0;
    }
    let ab = dot3(a, b);
    let c = pin((ab / aa_bb * ab) + (ab / aa_bb * ab) - 1.0, -1.0, 1.0);
    let angle = ((c as f64).acos() as f32) * 0.5;
    if ab < 0.0 { PI - angle } else { angle }
}
/// `real_seed_random_range` of the game's seed.
fn random_range(seed: &mut u32, lower: f32, upper: f32) -> f32 {
    *seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
    let random = (*seed >> 16) as f32 / 65535.0;
    lower + (upper - lower) * random
}

/// `matrix4x3_transform_normal` and its inverse, for a rotation.
fn transform_normal(m: &Matrix4x3, n: [f32; 3]) -> [f32; 3] {
    let [i, j, k] = n;
    [
        i * m.forward[0] + j * m.left[0] + k * m.up[0],
        i * m.forward[1] + j * m.left[1] + k * m.up[1],
        i * m.forward[2] + j * m.left[2] + k * m.up[2],
    ]
}
fn inverse_transform_normal(m: &Matrix4x3, n: [f32; 3]) -> [f32; 3] {
    let [i, j, k] = n;
    [
        i * m.forward[0] + j * m.forward[1] + k * m.forward[2],
        i * m.left[0] + j * m.left[1] + k * m.left[2],
        i * m.up[0] + j * m.up[1] + k * m.up[2],
    ]
}

/// The unit's orientation, as its aiming updates take it.
fn orientation(state: &UnitUpdateState) -> Matrix4x3 {
    Matrix4x3 { scale: 1.0, forward: state.forward, left: cross3(state.up, state.forward), up: state.up, position: [0.0; 3] }
}

/// `euler_angles2d_from_vector3d` and back: (yaw, pitch).
fn euler_from_vector(v: [f32; 3]) -> [f32; 2] {
    let horizontal = (v[0] * v[0] + v[1] * v[1]).sqrt();
    [(v[1] as f64).atan2(v[0] as f64) as f32, (v[2] as f64).atan2(horizontal as f64) as f32]
}
fn vector_from_euler(angles: [f32; 2]) -> [f32; 3] {
    let cosine_pitch = cosine(angles[1]);
    [cosine(angles[0]) * cosine_pitch, sine(angles[0]) * cosine_pitch, sine(angles[1])]
}

/// `unit_clip_to_aiming_bounds`: a vector kept within the unit's aiming
/// (or looking) screen, if it aims with one.
fn clip_to_aiming_bounds(state: &UnitUpdateState, vector: &mut [f32; 3], aiming: bool) -> bool {
    let (euler, bounds) = if aiming {
        (state.aiming_with_euler_screen != 0, state.aiming_screen_bounds)
    } else {
        (state.looking_with_euler_screen != 0, state.looking_screen_bounds)
    };
    let mut result = false;
    if euler {
        let matrix = orientation(state);
        let relative = inverse_transform_normal(&matrix, *vector);
        let mut angles = euler_from_vector(relative);
        let [x0, x1, y0, y1] = bounds;
        if angles[0] < x0 {
            angles[0] = x0;
            result = true;
        } else if angles[0] > x1 {
            angles[0] = x1;
            result = true;
        }
        if angles[1] < y0 {
            result = true;
            angles[1] = y0;
        } else if angles[1] > y1 {
            result = true;
            angles[1] = y1;
        }
        if result {
            *vector = transform_normal(&matrix, vector_from_euler(angles));
        }
    }
    result
}

/// `angular_accelerate_to_position`.
fn angular_accelerate_to_position(
    position: &mut [f32; 3],
    position_desired: [f32; 3],
    angular_velocity: &mut [f32; 3],
    velocity_maximum: f32,
    acceleration_maximum: f32,
) {
    if acceleration_maximum <= 0.0 && velocity_maximum <= 0.0 {
        *angular_velocity = [0.0; 3];
        *position = position_desired;
        return;
    }
    let cosine_value = pin(dot3(*position, position_desired), -1.0, 1.0);
    let mut speed_squared = (cosine_value as f64).acos() as f32;
    speed_squared = speed_squared * acceleration_maximum + speed_squared * acceleration_maximum;
    let speed = if speed_squared >= velocity_maximum * velocity_maximum {
        velocity_maximum
    } else {
        (speed_squared as f64).sqrt() as f32
    };
    let mut axis = cross3(*position, position_desired);
    normalize3(&mut axis);
    axis = [axis[0] * speed, axis[1] * speed, axis[2] * speed];
    let delta = [axis[0] - angular_velocity[0], axis[1] - angular_velocity[1], axis[2] - angular_velocity[2]];
    let delta_magnitude_squared = magnitude_squared3(delta);
    if delta_magnitude_squared < acceleration_maximum * acceleration_maximum {
        if speed < 1.0000001e-6 {
            *angular_velocity = [0.0; 3];
            *position = position_desired;
            return;
        }
        *angular_velocity = axis;
    } else {
        let scale = acceleration_maximum / (delta_magnitude_squared as f64).sqrt() as f32;
        *angular_velocity = [
            delta[0] * scale + angular_velocity[0],
            delta[1] * scale + angular_velocity[1],
            delta[2] * scale + angular_velocity[2],
        ];
    }
    let mut axis = *angular_velocity;
    let angle = normalize3(&mut axis);
    if angle != 0.0 {
        *position = rotate_vector_about_axis(*position, axis, sine(angle), cosine(angle));
        normalize3(position);
    }
}

/// `unit_animation_busy`.
fn animation_busy(state: i8) -> bool {
    matches!(state, 23 | 24 | 25 | 26 | 27 | 29 | 30 | 31 | 32 | 33 | 34 | 35 | 39 | 41)
}

/// `unit_running_blind`: its run's facing (its actor's, or forward),
/// wandering left and right as it goes.
fn running_blind(host: Option<&dyn UnitUpdateHost>, state: &mut UnitUpdateState) {
    let actor_controlled = host.is_some_and(|h| h.call(state, call::RUNNING_BLIND_VECTOR, 0, 0.0) != 0);
    if !actor_controlled {
        state.desired_facing_vector = FORWARD;
    }
    let mut positive_angle_allowed = 1.0f32;
    let mut negative_angle_allowed = 1.0f32;
    if actor_controlled {
        let negative_angle_bounds_dist = degrees(45.0) - state.run_blindly_angle;
        let positive_angle_bounds_dist = degrees(45.0) + state.run_blindly_angle;
        negative_angle_allowed = min(negative_angle_allowed, negative_angle_bounds_dist / degrees(13.5));
        positive_angle_allowed = min(positive_angle_allowed, positive_angle_bounds_dist / degrees(13.5));
    }
    let negative_velocity_bounds_dist = degrees(12.0) - state.run_blindly_angle_delta;
    let positive_velocity_bounds_dist = degrees(12.0) + state.run_blindly_angle_delta;
    negative_angle_allowed = min(negative_angle_allowed, negative_velocity_bounds_dist * 15.915494);
    positive_angle_allowed = min(positive_angle_allowed, positive_velocity_bounds_dist * 15.915494);
    let mut seed = host.map_or(0, |h| h.random_seed());
    let acceleration = if negative_angle_allowed < positive_angle_allowed {
        if negative_angle_allowed < -1.0 {
            -0.020943951
        } else {
            let min_allowed = min(1.0, negative_angle_allowed);
            random_range(&mut seed, -0.020943951, 0.020943951 * min_allowed)
        }
    } else if positive_angle_allowed < -1.0 {
        0.020943951
    } else {
        let max_allowed = min(1.0, positive_angle_allowed);
        random_range(&mut seed, -0.020943951 * max_allowed, 0.020943951)
    };
    if let Some(host) = host {
        host.set_random_seed(seed);
    }
    state.run_blindly_angle_delta += acceleration;
    state.run_blindly_angle += state.run_blindly_angle_delta;
    if state.run_blindly_angle < -PI {
        state.run_blindly_angle += 2.0 * PI;
    } else if state.run_blindly_angle > PI {
        state.run_blindly_angle -= 2.0 * PI;
    }
    state.desired_facing_vector =
        rotate_vector_about_axis(state.desired_facing_vector, UP, sine(state.run_blindly_angle), cosine(state.run_blindly_angle));
}

/// `unit_update`. `host` is the running game's (none: its calls answer
/// nothing, its blind run's random numbers unknown).
pub fn unit_update(
    definition: &UnitUpdateDefinition,
    inputs: &UnitUpdateInputs,
    globals: &mut UnitGlobals,
    state: &mut UnitUpdateState,
    host: Option<&dyn UnitUpdateHost>,
) {
    let call = |state: &mut UnitUpdateState, kind: i32, a: i32, b: f32| host.map_or(0, |h| h.call(state, kind, a, b));
    let mut used_time = false;
    let mut jetpack_cheat_active = false;
    let mut light_change_state = false;
    let dead = |state: &UnitUpdateState| state.damage_flags & OBJECT_DEAD != 0;

    state.timer = state.timer.wrapping_add(1);
    if globals.used_time != 0 || state.timer <= globals.next_timer {
        globals.highest_timer = if globals.highest_timer > state.timer { globals.highest_timer } else { state.timer };
    } else {
        used_time = true;
        globals.used_time = 1;
        state.timer = 0;
    }

    if state.flags & RUNNING_BLINDLY != 0 {
        running_blind(host, state);
        state.desired_aiming_vector = state.desired_facing_vector;
        state.desired_looking_vector = state.desired_facing_vector;
        state.throttle = FORWARD;
        state.control_flags = 0;
    } else if state.flags & ACTIVELY_CONTROLLED == 0 {
        state.desired_looking_vector = state.object_forward;
        state.desired_aiming_vector = state.object_forward;
        state.desired_facing_vector = state.object_forward;
        state.throttle = [0.0; 3];
        state.control_flags = 0;
    }

    if definition.flags & SIMPLE_CREATURE == 0 {
        let mut driven = false;
        if state.persistent_control_timer > 0 {
            state.control_flags |= state.persistent_control_flags;
            if state.persistent_control_flags & CONTROL_PRIMARY_TRIGGER != 0 {
                if state.persistent_control_timer % 7 == 0 {
                    state.control_flags |= CONTROL_PRIMARY_TRIGGER;
                } else {
                    state.control_flags &= !CONTROL_PRIMARY_TRIGGER;
                }
                state.primary_trigger = 1.0;
            } else {
                state.primary_trigger = 0.0;
            }
            state.persistent_control_timer -= 1;
            if state.persistent_control_timer == 0 {
                state.persistent_control_flags = 0;
            }
        }

        if state.flags & POSSESSED_BY_RECORDING == 0 {
            if state.driver_object_index != NONE
                && !dead(state)
                && let Some(driver) = inputs.driver
            {
                state.owner_team_index = driver.owner_team_index;
                driven = true;
                if driver.controls {
                    state.control_flags |= driver.control_flags & CONTROL_DRIVER_MASK;
                    state.desired_facing_vector = driver.desired_facing_vector;
                    state.throttle = driver.throttle;
                }
            }
            if state.gunner_object_index != NONE
                && !dead(state)
                && let Some(gunner) = inputs.gunner
            {
                if !driven {
                    state.owner_team_index = gunner.owner_team_index;
                }
                if gunner.controls {
                    state.desired_aiming_vector = gunner.desired_aiming_vector;
                    state.desired_looking_vector = gunner.desired_aiming_vector;
                    state.control_flags |= gunner.control_flags & CONTROL_GUNNER_MASK;
                    state.primary_trigger = gunner.primary_trigger;
                }
            }
            if state.control_flags & CONTROL_GUNNER_MASK != 0 {
                state.gunner_inactive_ticks = 0;
            } else if state.gunner_inactive_ticks < i8::MAX {
                state.gunner_inactive_ticks += 1;
            }
        }

        if inputs.cheat_jetpack && state.control_flags & CONTROL_PRIMARY_TRIGGER != 0 && state.control_flags & CONTROL_THROW_GRENADE != 0 {
            jetpack_cheat_active = true;
        }

        if state.flags & ACTIVE_CAMOUFLAGED != 0 {
            let change = if inputs.game_engine_running && state.cause_for_camo_regrowth != 0 {
                if state.cause_for_camo_regrowth == CAMO_REGROWTH_SHOT_FIRED && inputs.weapon_camo_regrowth_rate != 0.0 {
                    inputs.weapon_camo_regrowth_rate
                } else {
                    1.0 / 120.0
                }
            } else {
                1.0 / 120.0
            };
            state.active_camouflage += change;
            if state.active_camouflage > 1.0 {
                state.active_camouflage = 1.0;
                state.cause_for_camo_regrowth = CAMO_REGROWTH_DEFAULT;
            }
        } else {
            state.active_camouflage -= 1.0 / 120.0;
            if state.active_camouflage < 0.0 {
                state.active_camouflage = 0.0;
            }
        }

        if state.flags & SUPER_CAMOUFLAGED != 0 {
            state.active_camouflage_super_amount += 1.0 / 90.0;
            if state.active_camouflage_super_amount > 1.0 {
                state.active_camouflage_super_amount = 1.0;
            }
        } else {
            state.active_camouflage_super_amount -= 1.0 / 90.0;
            if state.active_camouflage_super_amount < 0.0 {
                state.active_camouflage_super_amount = 0.0;
            }
        }

        if state.body_stun_ticks > 0 {
            state.body_stun_ticks -= 1;
            if state.body_stun_ticks == 0 {
                state.body_stun = 0.0;
            }
        }

        if state.weapon_drop_delay_ticks > 0 {
            state.weapon_drop_delay_ticks -= 1;
            if state.weapon_drop_delay_ticks == 0 {
                call(state, call::DROP_CURRENT_WEAPON, 0, 0.0);
            }
        }

        if state.feign_death_timer > 0 && state.object_flags & OBJECT_AT_REST != 0 {
            state.feign_death_timer -= 1;
            if state.feign_death_timer == 0 {
                if state.body_vitality > 0.0 {
                    call(state, call::RESURRECT, 0, 0.0);
                } else {
                    call(state, call::DIED, 0, 0.0);
                }
            }
        }
    }

    if definition.flags & HAS_NO_AIMING == 0 {
        if !dead(state) {
            if state.damage_flags & OBJECT_CANNOT_HOLD_WEAPON != 0 {
                call(state, call::DROP_CURRENT_WEAPON, 0, 0.0);
            } else if state.desired_weapon_index != state.current_weapon_index && !animation_busy(state.animation_state) {
                call(state, call::READY_DESIRED_WEAPON, 0, 0.0);
            }

            if state.desired_grenade_index != state.current_grenade_index && !animation_busy(state.animation_state) {
                let grenade_index = call(state, call::NEXT_GRENADE, state.desired_grenade_index as i32, 0.0);
                if grenade_index != NONE {
                    state.current_grenade_index = grenade_index as i8;
                }
            }

            if inputs.cheat_infinite_ammo && state.player_index != NONE {
                for count in &mut state.grenade_counts {
                    *count = if *count > 1 { *count } else { 1 };
                }
                if state.desired_grenade_index == NONE as i8 {
                    state.desired_grenade_index = 0;
                }
            }

            if state.desired_zoom_level != state.current_zoom_level {
                state.current_zoom_level = state.desired_zoom_level;
                if state.desired_zoom_level == NONE as i8 {
                    state.integrated_night_vision_power = 0.0;
                }
                call(state, call::ZOOM_CHANGED, 0, 0.0);
            }
        }

        let aim_scale = if state.aiming_speed == AIMING_SPEED_CASUAL { definition.casual_aiming_modifier } else { 1.0 };
        let aiming_velocity_limit = aim_scale * definition.aiming_velocity_maximum / TICKS_PER_SECOND;
        let aiming_acceleration_limit = aim_scale * definition.aiming_acceleration_maximum / (TICKS_PER_SECOND * TICKS_PER_SECOND);
        let last_aiming_vector = state.aiming_vector;

        if aiming_velocity_limit == 0.0 && aiming_acceleration_limit == 0.0 {
            state.aiming_vector = state.desired_aiming_vector;
            let mut v = state.aiming_vector;
            clip_to_aiming_bounds(state, &mut v, true);
            state.aiming_vector = v;
            state.aiming_velocity = [0.0; 3];
        } else if state.aiming_with_euler_screen != 0 {
            let matrix = orientation(state);
            euler_aiming_update_oriented(
                Some(&matrix),
                &mut state.aiming_vector,
                state.desired_aiming_vector,
                &mut state.aiming_velocity,
                state.aiming_screen_bounds,
                aiming_velocity_limit,
                aiming_acceleration_limit,
            );
        } else {
            let desired = state.desired_aiming_vector;
            angular_accelerate_to_position(
                &mut state.aiming_vector,
                desired,
                &mut state.aiming_velocity,
                aiming_velocity_limit,
                aiming_acceleration_limit,
            );
        }

        let angle = angle_between_vectors3d(state.aiming_vector, last_aiming_vector);
        let change = angle / (definition.aiming_velocity_maximum / TICKS_PER_SECOND);
        state.aiming_change = (pin(change, 0.0, 1.0) * 255.0) as u8;

        let looking_velocity_limit = (aim_scale * definition.looking_velocity_maximum) / TICKS_PER_SECOND;
        let looking_acceleration_limit = (aim_scale * definition.looking_acceleration_maximum) / 900.0;
        if looking_velocity_limit == 0.0 && looking_acceleration_limit == 0.0 {
            state.looking_vector = state.desired_looking_vector;
            let mut v = state.looking_vector;
            clip_to_aiming_bounds(state, &mut v, false);
            state.looking_vector = v;
            state.looking_velocity = [0.0; 3];
        } else if state.looking_with_euler_screen != 0 {
            let matrix = orientation(state);
            euler_aiming_update_oriented(
                Some(&matrix),
                &mut state.looking_vector,
                state.desired_looking_vector,
                &mut state.looking_velocity,
                state.looking_screen_bounds,
                looking_velocity_limit,
                looking_acceleration_limit,
            );
        } else {
            let desired = state.desired_looking_vector;
            angular_accelerate_to_position(
                &mut state.looking_vector,
                desired,
                &mut state.looking_velocity,
                looking_velocity_limit,
                looking_acceleration_limit,
            );
        }

        if !jetpack_cheat_active {
            let throw_grenade = state.control_flags & CONTROL_THROW_GRENADE != 0;
            match state.grenade_throw_state {
                THROW_IDLE => {
                    if throw_grenade {
                        call(state, call::GRENADE_THROW_BEGIN, 0, 0.0);
                    }
                }
                THROW_WIND_UP => {
                    if state.animation_frame_index >= 2 {
                        call(state, call::GRENADE_MOVE_TO_HAND, 0, 0.0);
                    }
                }
                THROW_ENDING => {
                    if state.animation_state != STATE_THROW_GRENADE && !throw_grenade {
                        state.grenade_throw_state = THROW_IDLE;
                    }
                }
                THROW_IN_HAND => {
                    state.grenade_throw_ticks = state.grenade_throw_ticks.wrapping_add(1);
                    if state.animation_state != STATE_THROW_GRENADE {
                        call(state, call::GRENADE_RELEASE, 0, 0.0);
                    }
                }
                _ => {}
            }
        }

        if state.current_weapon_index != NONE as i16 {
            let mut flags = 0i32;
            let primary_trigger = state.primary_trigger;
            if state.current_weapon_index == state.desired_weapon_index {
                let time_remaining = state.persistent_control_timer > 0 && state.persistent_control_flags & CONTROL_PRIMARY_TRIGGER != 0;
                if !jetpack_cheat_active {
                    if inputs.allow_integrated_lights && state.control_flags & CONTROL_INTEGRATED_LIGHT != 0 {
                        flags |= WEAPON_INTEGRATED_LIGHT;
                    }
                    if state.control_flags & CONTROL_PRIMARY_TRIGGER != 0 {
                        flags |= WEAPON_PRIMARY_TRIGGER;
                    }
                    if state.control_flags & CONTROL_SECONDARY_TRIGGER != 0 {
                        flags |= WEAPON_SECONDARY_TRIGGER;
                    }
                }
                if definition.flags & INTEGRATED_LIGHT_CONTROLS_WEAPON_DIRECTLY != 0 {
                    call(state, call::WEAPON_LIGHT_POWER, 0, state.integrated_light_power);
                }
                if state.control_flags & CONTROL_RELOAD != 0 {
                    flags |= WEAPON_RELOAD;
                }
                if animation_busy(state.animation_state) && !time_remaining {
                    flags |= WEAPON_USER_BUSY;
                }
                if state.player_melee_ticks > 0 {
                    flags |= WEAPON_USER_BUSY;
                }
                if state.current_zoom_level != NONE as i8 {
                    flags |= WEAPON_ZOOMED;
                }
            } else {
                flags |= WEAPON_USER_SWITCHING;
            }
            call(state, call::WEAPON_OWNER_UPDATE, flags, primary_trigger);
        }
    }

    if definition.flags & SIMPLE_CREATURE == 0 {
        if state.animation_flags & ANIMATION_SHOWING_ACCELERATION != 0 {
            call(state, call::SEAT_UPDATE, 0, 0.0);
            let (d, a) = (state.seat_desired_acceleration, state.seat_acceleration);
            state.seat_acceleration = [d[0] * 0.3 + a[0] * 0.7, d[1] * 0.3 + a[1] * 0.7, d[2] * 0.3 + a[2] * 0.7];
        }
        for (seat_index, &(powerup_time, powerdown_time)) in definition.powered_seats.iter().take(2).enumerate() {
            let powered = if seat_index == 0 {
                state.driver_object_index != NONE || state.flags & ACTIVELY_CONTROLLED != 0
            } else {
                state.gunner_object_index != NONE && state.gunner_object_index != state.driver_object_index
            };
            let power = &mut state.seat_power[seat_index];
            if state.damage_flags & OBJECT_DEAD != 0 || !powered {
                if *power != 0.0 {
                    *power -= 1.0 / (powerdown_time * TICKS_PER_SECOND);
                    if *power < 0.0 {
                        *power = 0.0;
                    }
                }
            } else if *power != 1.0 {
                *power += 1.0 / (powerup_time * TICKS_PER_SECOND);
                if *power > 1.0 {
                    *power = 1.0;
                }
            }
        }
    }

    if state.delayed_damage_timer > 0 {
        state.delayed_damage_timer -= 1;
        if state.delayed_damage_timer == 0 {
            call(state, call::DELAYED_DAMAGE, 0, 0.0);
            state.last_damage_category = 0;
            state.delayed_damage_attacker_object_index = NONE;
            state.delayed_damage_peak = 0.0;
        }
    }

    call(state, call::CONTINUOUS_MELEE_DAMAGE, 0, 0.0);
    call(state, call::DIALOGUE_UPDATE, 0, 0.0);

    if used_time || state.player_index != NONE {
        call(state, call::REFRESH_ILLUMINATION, 0, 0.0);
    }

    if state.flaming_death_delay > 0 {
        state.flaming_death_delay -= 1;
        if state.flaming_death_delay == 0 {
            call(state, call::FLAME_TO_DEATH, 0, 0.0);
        }
    }

    // (interpolate_scalar toward 0 by 0.1)
    let difference = 0.0 - state.mouth_aperture;
    state.mouth_aperture += pin(difference, -0.1, 0.1);

    if state.flags & DESIRED_INTEGRATED_LIGHT_ON != 0 {
        if state.flags & INTEGRATED_LIGHT_ON == 0 {
            light_change_state = true;
        }
        state.flags &= !DESIRED_INTEGRATED_LIGHT_ON;
    }
    if state.flags & DESIRED_INTEGRATED_LIGHT_OFF != 0 {
        if state.flags & INTEGRATED_LIGHT_ON != 0 {
            light_change_state = true;
        }
        state.flags &= !DESIRED_INTEGRATED_LIGHT_OFF;
    }

    if (inputs.allow_integrated_lights && state.control_flags & CONTROL_INTEGRATED_LIGHT != 0)
        || state.integrated_light_battery <= 0.0
        || light_change_state
    {
        let active = call(state, call::NIGHT_VISION_ACTIVE, 0, 0.0) != 0;
        let toggling_night_vision = active && state.control_flags & CONTROL_INTEGRATED_LIGHT != 0;
        if toggling_night_vision {
            call(state, call::NIGHT_VISION_EFFECT, 0, 0.0);
            state.flags ^= INTEGRATED_NIGHT_VISION_ON;
        }
        if !toggling_night_vision
            && (state.flags & INTEGRATED_LIGHT_ON != 0 || state.integrated_light_battery > 0.2)
            && state.parent_object_index == NONE
        {
            call(state, call::LIGHT_EFFECT, 0, 0.0);
            state.flags ^= INTEGRATED_LIGHT_ON;
        }
    }

    if state.flags & INTEGRATED_LIGHT_ON != 0 {
        if definition.flags & INTEGRATED_LIGHT_LASTS_FOREVER == 0 {
            state.integrated_light_battery -= 1.0 / (2.0 * 1800.0);
        }
        if state.parent_object_index != NONE || state.damage_flags & OBJECT_DEAD != 0 {
            state.flags &= !INTEGRATED_LIGHT_ON;
        }
        if state.integrated_light_power != 1.0 {
            state.integrated_light_power += 1.0 / 6.0;
            if state.integrated_light_power > 1.0 {
                state.integrated_light_power = 1.0;
            }
        }
    } else {
        if state.integrated_light_battery < 1.0 {
            state.integrated_light_battery += 1.0 / 900.0;
        }
        if state.integrated_light_power != 0.0 {
            state.integrated_light_power -= 1.0 / 24.0;
            if state.integrated_light_power < 0.0 {
                state.integrated_light_power = 0.0;
            }
        }
    }

    if call(state, call::NIGHT_VISION_ACTIVE, 0, 0.0) != 0 {
        if state.flags & INTEGRATED_NIGHT_VISION_ON != 0 {
            if state.integrated_night_vision_power != 1.0 {
                state.integrated_night_vision_power += 1.0 / 12.0;
                if state.integrated_night_vision_power > 1.0 {
                    state.integrated_night_vision_power = 1.0;
                }
            }
        } else if state.integrated_night_vision_power != 0.0 {
            state.integrated_night_vision_power -= 1.0 / 24.0;
            if state.integrated_night_vision_power < 0.0 {
                state.integrated_night_vision_power = 0.0;
            }
        }
    }
}
