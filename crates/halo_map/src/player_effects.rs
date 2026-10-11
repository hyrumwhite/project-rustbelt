//! Player effects (`player_effects.c`): a local player's screen flash,
//! camera impulse (and the permanent turn it gives the player's aim), camera
//! shake, continuous shake and damage indicators, started by damage
//! (`player_effect_start`), by the game (`player_effect_screen_flash`,
//! `player_telefrag_effect_start`) and by looping sounds
//! (`player_effect_add_continuous_effect`), and read each frame as the
//! camera's effect matrix and the screen flash.
//!
//! What they take of the rest of the game goes through
//! [`PlayerEffectHost::call`] (player_effects.c's `player_effect_call`,
//! shared with the C): the player's facing and its permanent impulse, the
//! controller's rumble, the damage effect's sound, and where the attacker is
//! from the player's camera.

// (the C's shape: its operand order, PIN's comparisons, per-component loops)
#![allow(clippy::assign_op_pattern, clippy::manual_clamp, clippy::needless_range_loop)]

use crate::cache::CacheFile;
use crate::model::Matrix4x3;
use crate::object_functions::FunctionTables;
use crate::{Result, layout};

const NONE: i32 = -1;

/// `TICKS_PER_SECOND`.
const TICKS_PER_SECOND: f32 = 30.0;
/// `_real_epsilon`.
const REAL_EPSILON: f32 = 0.0001;
/// `NUMBER_OF_SCREEN_FLASH_TYPES`.
const NUMBER_OF_SCREEN_FLASH_TYPES: i16 = 7;
/// `_screen_flash_type_lighten`, `_screen_flash_high_priority`.
const SCREEN_FLASH_TYPE_LIGHTEN: i16 = 1;
const SCREEN_FLASH_HIGH_PRIORITY: i16 = 2;
/// `_render_screen_flash_type_lighten`.
const RENDER_SCREEN_FLASH_TYPE_LIGHTEN: i16 = 1;
/// `_transition_function_cosine`.
const TRANSITION_FUNCTION_COSINE: i16 = 5;

/// A player effect's flags.
const SCREEN_FLASH_JUST_STARTED: u8 = 1 << 0;
const CAMERA_IMPULSE_JUST_STARTED: u8 = 1 << 1;
const CAMERA_SHAKE_JUST_STARTED: u8 = 1 << 2;

/// The scripted effect's flags.
const SCRIPTED_ACTIVE: u32 = 1 << 0;
const SCRIPTED_STOPPING: u32 = 1 << 1;

/// `_damage_draw_indicators_down_bit`.
const DAMAGE_DRAW_INDICATORS_DOWN: u32 = 1 << 8;

/// A player effect's calls of the rest of the game (halo_sim.h's
/// `_player_effect_call_*`).
pub mod call {
    /// `player_control_get_facing_angles(a)`: yaw and pitch in `out`.
    pub const FACING_ANGLES: i32 = 1;
    /// `player_control_get_facing_direction(a)`, in `out`.
    pub const FACING_DIRECTION: i32 = 2;
    /// `player_control_permanent_impulse(a)` by the yaw and pitch `f`.
    pub const PERMANENT_IMPULSE: i32 = 3;
    /// `rumble_player_impulse(a)` with the damage effect `b`'s vibration at
    /// the scale `f[0]`.
    pub const RUMBLE_IMPULSE: i32 = 4;
    /// `unspatialized_impulse_sound_new(a)`.
    pub const SOUND: i32 = 5;
    /// The damage indicators' geometry for the local player `a` and the
    /// attacker `b`: the result whether there are a unit, an attacker and a
    /// camera; `out` the unit's head, the attacker's origin, and the camera's
    /// forward and up.
    pub const INDICATOR: i32 = 6;
    /// `rumble_player_continuous(a)` with the motors `f[0]`, `f[1]`.
    pub const RUMBLE_CONTINUOUS: i32 = 7;
    /// `rumble_player_set_scale(f[0])`.
    pub const RUMBLE_SET_SCALE: i32 = 8;
}

/// halo_sim.h's `struct halo_sim_player_effect_call`.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlayerEffectCall {
    pub kind: i32,
    pub a: i32,
    pub b: i32,
    pub f: [f32; 2],
    pub result: i32,
    pub out: [f32; 12],
}

impl PlayerEffectCall {
    pub fn new(kind: i32) -> Self {
        PlayerEffectCall { kind, a: 0, b: 0, f: [0.0; 2], result: 0, out: [0.0; 12] }
    }
}

/// `struct screen_flash_definition`.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ScreenFlash {
    pub flash_type: i16,
    pub priority: i16,
    pub unused1: [u32; 3],
    pub duration: f32,
    pub fade_function: i16,
    pub pad: u16,
    pub unused2: [u32; 2],
    pub max_intensity: f32,
    pub zero_scale_factor: f32,
    /// alpha, red, green, blue.
    pub color: [f32; 4],
}

/// `struct camera_impulse_definition`.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct CameraImpulse {
    pub temporary_duration: f32,
    pub temporary_transition: i16,
    pub pad: u16,
    pub temporary_rotation: f32,
    pub temporary_translation: f32,
    pub temporary_jitter_lower_bound: f32,
    pub temporary_jitter_upper_bound: f32,
    pub temporary_zero_scale_factor: f32,
    pub temporary_unused: u32,
    pub permanent_angle: f32,
    pub permanent_zero_scale_factor: f32,
    pub permanent_unused: [u32; 3],
}

/// `struct camera_shake_definition`.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct CameraShake {
    pub duration: f32,
    pub falloff_transition_function: i16,
    pub pad0: u16,
    pub random_translation_magnitude: f32,
    pub random_rotation_magnitude: f32,
    pub unused_motion_scales: [i32; 3],
    pub periodic_function: i16,
    pub pad1: u16,
    pub periodic_period: f32,
    pub periodic_weight: f32,
    pub zero_scale_factor: f32,
    pub unused: [i32; 7],
}

const _: () = assert!(std::mem::size_of::<ScreenFlash>() == 0x38);
const _: () = assert!(std::mem::size_of::<CameraImpulse>() == 0x34);
const _: () = assert!(std::mem::size_of::<CameraShake>() == 0x48);

/// `struct player_effect_datum`, as the game holds it.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct PlayerEffectDatum {
    pub direction: [f32; 3],
    pub jitter: [f32; 3],
    pub screen_flash: ScreenFlash,
    pub camera_impulse: CameraImpulse,
    pub camera_shake: CameraShake,
    /// vibrate frequencies (2), translational shake, rotational shake.
    pub continuous: [f32; 4],
    pub continuous_effect_timer: i16,
    pub screen_flash_time_left: i16,
    pub camera_impulse_time_left: i16,
    pub camera_shake_time_left: i16,
    pub damage_indicator_ticks: [u8; 4],
    pub flags: u8,
    pub pad: [u8; 3],
}

const _: () = assert!(std::mem::size_of::<PlayerEffectDatum>() == 0xEC);

/// What an operation reads and changes: a local player's effect, the shared
/// screen fade and scripted effect, and the local random seed.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct PlayerEffectState {
    pub local_player_index: i32,
    pub datum: PlayerEffectDatum,
    /// The screen fade: colour, start time, ticks, fading out.
    pub fade_color: [f32; 3],
    pub fade_start_time: i32,
    pub fade_ticks: i16,
    pub fade_fading_out: u8,
    pub fade_pad: u8,
    /// The scripted effect: translation, rotation (yaw, pitch, roll),
    /// intensity, timer, total time.
    pub scripted_max_translation: [f32; 3],
    pub scripted_max_rotation: [f32; 3],
    pub scripted_max_intensity: f32,
    pub scripted_timer: i16,
    pub scripted_total_time: i16,
    pub global_flags: u32,
    /// `get_global_local_random_seed_address`'s.
    pub local_random_seed: u32,
}

const _: () = assert!(std::mem::size_of::<PlayerEffectState>() == 0xEC + 4 + 20 + 32 + 8);

/// What an operation reads besides the state.
pub struct PlayerEffectInputs<'a> {
    /// `game_time_get()`, `game_time_get_elapsed()`.
    pub game_time: i32,
    pub elapsed: i16,
    /// `console_is_active()`.
    pub console_active: bool,
    pub functions: &'a FunctionTables<'a>,
    /// `random_direction_table_get`'s table.
    pub directions: &'a [[f32; 3]],
}

/// The game's side of a player effect.
pub trait PlayerEffectHost {
    fn call(&self, state: &mut PlayerEffectState, call: &mut PlayerEffectCall);
}

/// What player effects read of a damage effect.
#[derive(Debug, Clone, Copy, Default)]
pub struct DamageEffectView {
    pub screen_flash: ScreenFlash,
    pub camera_impulse: CameraImpulse,
    pub camera_shake: CameraShake,
    pub sound: i32,
    pub damage_flags: u32,
}

fn read_struct<T: Copy + Default>(p: crate::cache::Ptr, rel: usize) -> Result<T> {
    let bytes = p.bytes(rel, std::mem::size_of::<T>())?;
    let mut value = T::default();
    // SAFETY: T is plain repr(C) data of that size; any bytes are a value
    unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), (&mut value as *mut T).cast::<u8>(), bytes.len()) };
    Ok(value)
}

impl DamageEffectView {
    pub fn load(cache: &CacheFile, datum: u32) -> Result<Self> {
        use layout::{damage_definition as g, damage_effect_definition as d};
        let root = cache.tag_root(datum, crate::groups::DAMAGE_EFFECT)?;
        Ok(DamageEffectView {
            screen_flash: read_struct(root, d::SCREEN_FLASH)?,
            camera_impulse: read_struct(root, d::CAMERA_IMPULSE)?,
            camera_shake: read_struct(root, d::CAMERA_SHAKE)?,
            sound: root.reference(d::SOUND)?.datum.map_or(NONE, |d| d as i32),
            damage_flags: root.u32(d::DAMAGE + g::FLAGS)?,
        })
    }
}

/// What continuous shake reads of a continuous damage effect.
#[derive(Debug, Clone, Copy, Default)]
pub struct ContinuousEffectView {
    pub falloff_radius: f32,
    pub cutoff_radius: f32,
    pub vibrate_frequencies: [f32; 2],
    pub random_translation_magnitude: f32,
    pub random_rotation_magnitude: f32,
    pub periodic_function: i16,
    pub periodic_period: f32,
    pub periodic_weight: f32,
}

impl ContinuousEffectView {
    pub fn load(cache: &CacheFile, datum: u32) -> Result<Self> {
        use layout::{continuous_camera_shake_definition as s, continuous_damage_effect_definition as c};
        let root = cache.tag_root(datum, crate::fourcc(b"cdmg"))?;
        let shake = root.add(c::CAMERA_SHAKE);
        Ok(ContinuousEffectView {
            falloff_radius: root.f32(c::FALLOFF_RADIUS)?,
            cutoff_radius: root.f32(c::CUTOFF_RADIUS)?,
            vibrate_frequencies: [root.f32(c::VIBRATE)?, root.f32(c::VIBRATE + 4)?],
            random_translation_magnitude: shake.f32(s::RANDOM_TRANSLATION_MAGNITUDE)?,
            random_rotation_magnitude: shake.f32(s::RANDOM_ROTATION_MAGNITUDE)?,
            periodic_function: shake.i16(s::PERIODIC_FUNCTION)?,
            periodic_period: shake.f32(s::PERIODIC_PERIOD)?,
            periodic_weight: shake.f32(s::PERIODIC_WEIGHT)?,
        })
    }
}

/// `struct render_screen_flash`.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct RenderScreenFlash {
    pub flash_type: i16,
    pub pad: i16,
    pub intensity: f32,
    /// alpha, red, green, blue.
    pub color: [f32; 4],
}

fn sine(a: f32) -> f32 {
    (a as f64).sin() as f32
}
fn cosine(a: f32) -> f32 {
    (a as f64).cos() as f32
}
fn dot3(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross3(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
/// `normalize3d`.
fn normalize3(v: &mut [f32; 3]) -> f32 {
    crate::markers::normalize3(v)
}
/// `realcmp`.
fn realcmp(a: f32, b: f32) -> bool {
    ((a - b) as f64).abs() < REAL_EPSILON as f64
}
/// `PIN`.
fn pin(n: f32, a: f32, b: f32) -> f32 {
    if n < a {
        a
    } else if n > b {
        b
    } else {
        n
    }
}
/// `MAX`.
fn max(a: f32, b: f32) -> f32 {
    if a > b { a } else { b }
}
/// `(short)value` as x86 truncates it.
fn c_short(value: f32) -> i16 {
    (if (-2_147_483_648.0..2_147_483_648.0).contains(&value) { value as i32 } else { i32::MIN }) as i16
}
/// `real_seed_random_range` on the local seed.
fn real_local_random_range(seed: &mut u32, lower_bound: f32, upper_bound: f32) -> f32 {
    *seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
    let random = (*seed >> 16) as f32 / 65535.0;
    lower_bound + (upper_bound - lower_bound) * random
}
/// `seed_random_direction3d` on the local seed.
fn local_random_direction3d(seed: &mut u32, directions: &[[f32; 3]]) -> [f32; 3] {
    *seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
    let random = (*seed >> 16) as u16 as u32;
    let span = directions.len() as i16 as i32 as u32;
    let index = ((span.wrapping_mul(random)) >> 16) as i16;
    directions.get(index as usize).copied().unwrap_or([0.0; 3])
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

fn identity() -> Matrix4x3 {
    Matrix4x3 { scale: 1.0, forward: [1.0, 0.0, 0.0], left: [0.0, 1.0, 0.0], up: [0.0, 0.0, 1.0], position: [0.0; 3] }
}

/// `matrix4x3_rotation_from_angles`.
fn rotation_from_angles(yaw: f32, pitch: f32, roll: f32) -> Matrix4x3 {
    let cosine_roll = cosine(roll);
    let sine_roll = sine(roll);
    let cosine_pitch = cosine(pitch);
    let sine_pitch = sine(pitch);
    let cosine_yaw = cosine(yaw);
    let sine_yaw = sine(yaw);
    let sine_pitch_cosine_roll = sine_pitch * cosine_roll;
    let sine_pitch_sine_roll = sine_pitch * sine_roll;
    Matrix4x3 {
        scale: 1.0,
        forward: [
            cosine_yaw * cosine_pitch,
            sine_yaw * cosine_roll - cosine_yaw * sine_pitch_sine_roll,
            cosine_yaw * sine_pitch_cosine_roll + sine_yaw * sine_roll,
        ],
        left: [
            -(sine_yaw * cosine_pitch),
            sine_yaw * sine_pitch_sine_roll + cosine_yaw * cosine_roll,
            cosine_yaw * sine_roll - sine_yaw * sine_pitch_cosine_roll,
        ],
        up: [-sine_pitch, -(cosine_pitch * sine_roll), cosine_pitch * cosine_roll],
        position: [0.0; 3],
    }
}

fn rows(m: &Matrix4x3) -> [f32; 12] {
    [
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

/// `matrix4x3_multiply`, as the C restates its SSE block.
fn multiply(a: &Matrix4x3, b: &Matrix4x3) -> Matrix4x3 {
    let (ae, be) = (rows(a), rows(b));
    let mut product = [0.0f32; 12];
    for row in 0..4 {
        for column in 0..3 {
            let mut value =
                be[row * 3] * ae[column] + be[row * 3 + 1] * ae[3 + column] + be[row * 3 + 2] * ae[6 + column];
            if row == 3 {
                value = value * a.scale + ae[9 + column];
            }
            product[row * 3 + column] = value;
        }
    }
    Matrix4x3 {
        scale: a.scale * b.scale,
        forward: [product[0], product[1], product[2]],
        left: [product[3], product[4], product[5]],
        up: [product[6], product[7], product[8]],
        position: [product[9], product[10], product[11]],
    }
}

/// `effect_scale_factor`.
fn effect_scale_factor(zero_scale_factor: f32, scale: f32) -> f32 {
    zero_scale_factor + (1.0 - zero_scale_factor) * scale
}

struct Effects<'a, H: PlayerEffectHost> {
    inputs: &'a PlayerEffectInputs<'a>,
    host: &'a H,
}

impl<H: PlayerEffectHost> Effects<'_, H> {
    fn call(&self, state: &mut PlayerEffectState, call: &mut PlayerEffectCall) {
        self.host.call(state, call);
    }

    /// `player_effect_update_screen_flash`.
    fn update_screen_flash(&self, state: &mut PlayerEffectState, screen_flash: &ScreenFlash, scale: f32, time_scale: f32) {
        let time_factor = time_scale * TICKS_PER_SECOND;
        // (a type that is no type is no flash: the game says so once)
        if !(0..NUMBER_OF_SCREEN_FLASH_TYPES).contains(&screen_flash.flash_type) {
            return;
        }
        let effect = &mut state.datum;
        // (the render type map is the identity: only `none` is none)
        if !(effect.screen_flash.priority > screen_flash.priority
            && effect.screen_flash_time_left as f32 > screen_flash.duration * time_factor)
            && screen_flash.flash_type != 0
        {
            effect.screen_flash = *screen_flash;
            effect.screen_flash.duration = time_factor * effect.screen_flash.duration;
            effect.screen_flash_time_left = c_short(effect.screen_flash.duration);
            effect.screen_flash.zero_scale_factor =
                pin(effect_scale_factor(screen_flash.zero_scale_factor, scale), 0.0, screen_flash.max_intensity);
            effect.flags |= SCREEN_FLASH_JUST_STARTED;
        }
    }

    /// `player_effect_update_camera_shake`.
    fn update_camera_shake(&self, state: &mut PlayerEffectState, camera_shake: &CameraShake, scale: f32, time_scale: f32) {
        let time_factor = time_scale * TICKS_PER_SECOND;
        let intensity = effect_scale_factor(camera_shake.zero_scale_factor, scale);
        let effect = &mut state.datum;
        let time_left = effect.camera_shake_time_left as f32;
        if time_left < time_factor * camera_shake.duration
            || intensity > effect.camera_shake.zero_scale_factor
            || (intensity >= effect.camera_shake.zero_scale_factor && time_left < time_factor * camera_shake.duration)
        {
            effect.camera_shake = *camera_shake;
            effect.camera_shake.zero_scale_factor = intensity;
            effect.camera_shake.duration = time_factor * effect.camera_shake.duration;
            effect.camera_shake_time_left = c_short(effect.camera_shake.duration);
            effect.camera_shake.periodic_period = time_factor * effect.camera_shake.periodic_period;
            effect.flags |= CAMERA_SHAKE_JUST_STARTED;
        }
    }

    /// `player_effect_update_camera_impulse`.
    fn update_camera_impulse(
        &self,
        state: &mut PlayerEffectState,
        camera_impulse: &CameraImpulse,
        direction: [f32; 3],
        scale: f32,
        time_scale: f32,
    ) {
        let local_player_index = state.local_player_index;
        let time_factor = time_scale * TICKS_PER_SECOND;
        let intensity = effect_scale_factor(camera_impulse.temporary_zero_scale_factor, scale);
        let effect = state.datum;
        let time_left = effect.camera_impulse_time_left as f32;
        if effect.camera_impulse.temporary_duration > time_left
            || intensity > effect.camera_impulse.temporary_zero_scale_factor
            || (intensity >= effect.camera_impulse.temporary_zero_scale_factor
                && time_left < time_factor * camera_impulse.temporary_duration)
        {
            let mut flattened = direction;
            flattened[2] = 0.0;
            normalize3(&mut flattened);
            let mut call = PlayerEffectCall::new(call::FACING_ANGLES);
            call.a = local_player_index;
            self.call(state, &mut call);
            let (yaw, pitch) = (call.out[0], call.out[1]);
            let cosine_pitch = cosine(pitch);
            let mut facing = [cosine(yaw) * cosine_pitch, sine(yaw) * cosine_pitch, sine(pitch)];
            facing[2] = 0.0;
            normalize3(&mut facing);

            if realcmp(dot3(flattened, flattened), 1.0) && realcmp(dot3(facing, facing), 1.0) {
                // signed_angle_between_vectors2d
                let dot = facing[0] * flattened[0] + facing[1] * flattened[1];
                let mut angle = (pin(dot, -1.0, 1.0) as f64).acos() as f32;
                if facing[0] * flattened[1] - facing[1] * flattened[0] < 0.0 {
                    angle = -angle;
                }
                let effect = &mut state.datum;
                effect.camera_impulse = *camera_impulse;
                effect.camera_impulse.temporary_zero_scale_factor = intensity;
                effect.camera_impulse.temporary_duration = time_factor * effect.camera_impulse.temporary_duration;
                effect.camera_impulse_time_left = c_short(effect.camera_impulse.temporary_duration);
                effect.direction = [cosine(angle), sine(angle), 0.0];
                let jitter_upper_bound = effect.camera_impulse.temporary_jitter_upper_bound;
                let jitter_lower_bound = effect.camera_impulse.temporary_jitter_lower_bound;
                let jitter_magnitude =
                    real_local_random_range(&mut state.local_random_seed, jitter_lower_bound, jitter_upper_bound);
                let jitter_angle =
                    real_local_random_range(&mut state.local_random_seed, 0.0, 2.0 * std::f32::consts::PI);
                let effect = &mut state.datum;
                let mut jitter = cross3(effect.direction, [0.0, 0.0, 1.0]);
                normalize3(&mut jitter);
                jitter = rotate_vector_about_axis(jitter, effect.direction, sine(jitter_angle), cosine(jitter_angle));
                effect.jitter = [jitter[0] * jitter_magnitude, jitter[1] * jitter_magnitude, jitter[2] * jitter_magnitude];
                effect.flags |= CAMERA_IMPULSE_JUST_STARTED;
            }
        }

        let permanent_intensity = effect_scale_factor(camera_impulse.permanent_zero_scale_factor, scale);
        let mut call = PlayerEffectCall::new(call::FACING_DIRECTION);
        call.a = local_player_index;
        self.call(state, &mut call);
        let facing = [call.out[0], call.out[1], call.out[2]];
        let left = cross3([0.0, 0.0, 1.0], facing);
        let mut call = PlayerEffectCall::new(call::PERMANENT_IMPULSE);
        call.a = local_player_index;
        call.f[0] = dot3(left, direction) * camera_impulse.permanent_angle * permanent_intensity;
        call.f[1] = dot3(facing, direction) * camera_impulse.permanent_angle * permanent_intensity;
        self.call(state, &mut call);
    }

    /// `player_effect_start`'s, for a local player.
    #[allow(clippy::too_many_arguments)]
    fn start(
        &self,
        state: &mut PlayerEffectState,
        definition_index: i32,
        definition: &DamageEffectView,
        owner_object_index: i32,
        direction: [f32; 3],
        scale: f32,
        total_damage: f32,
    ) {
        let local_player_index = state.local_player_index;
        self.update_screen_flash(state, &definition.screen_flash, scale, 1.0);
        self.update_camera_impulse(state, &definition.camera_impulse, direction, scale, 1.0);
        self.update_camera_shake(state, &definition.camera_shake, scale, 1.0);
        let mut call = PlayerEffectCall::new(call::RUMBLE_IMPULSE);
        call.a = local_player_index;
        call.b = definition_index;
        call.f[0] = scale;
        self.call(state, &mut call);
        if definition.sound != NONE {
            let mut call = PlayerEffectCall::new(call::SOUND);
            call.a = definition.sound;
            self.call(state, &mut call);
        }
        if total_damage > 0.0 && owner_object_index != NONE {
            if definition.damage_flags & DAMAGE_DRAW_INDICATORS_DOWN != 0 {
                state.datum.damage_indicator_ticks[2] = 1;
            } else {
                let mut call = PlayerEffectCall::new(call::INDICATOR);
                call.a = local_player_index;
                call.b = owner_object_index;
                self.call(state, &mut call);
                if call.result != 0 {
                    let o = call.out;
                    let head = [o[0], o[1], o[2]];
                    let attacker = [o[3], o[4], o[5]];
                    let forward = [o[6], o[7], o[8]];
                    let up = [o[9], o[10], o[11]];
                    let delta = [attacker[0] - head[0], attacker[1] - head[1], attacker[2] - head[2]];
                    let left = cross3(forward, up);
                    let mut relative = [dot3(left, delta), dot3(delta, forward), dot3(delta, up)];
                    if normalize3(&mut relative) != 0.0 {
                        let ticks = &mut state.datum.damage_indicator_ticks;
                        if (relative[2] as f64).abs() > 0.5 {
                            if relative[2] > 0.0 {
                                ticks[0] = 1;
                            } else {
                                ticks[2] = 1;
                            }
                        }
                        let angle = (relative[1] as f64).atan2(relative[0] as f64) as f32;
                        let absolute_angle = (angle as f64).abs() as f32;
                        let pi = std::f32::consts::PI;
                        if angle < pi / 4.0 || angle > 3.0 * pi / 4.0 {
                            if absolute_angle > pi / 2.0 {
                                ticks[1] = 1;
                            } else {
                                ticks[3] = 1;
                            }
                        }
                    }
                }
            }
        }
    }

    /// `player_telefrag_effect_start`'s, for a local player.
    fn telefrag(&self, state: &mut PlayerEffectState, intensity: f32) {
        let camera_shake = CameraShake {
            // (intensity * 0.01, a double)
            random_translation_magnitude: (intensity as f64 * 0.01) as f32,
            duration: 1.0,
            ..Default::default()
        };
        let screen_flash = ScreenFlash {
            max_intensity: intensity,
            flash_type: SCREEN_FLASH_TYPE_LIGHTEN,
            priority: SCREEN_FLASH_HIGH_PRIORITY,
            duration: 1.0,
            zero_scale_factor: 0.0,
            color: [1.0, 1.0, 1.0, 1.0],
            ..Default::default()
        };
        let mut call = PlayerEffectCall::new(call::RUMBLE_CONTINUOUS);
        call.a = state.local_player_index;
        call.f = [intensity, intensity];
        self.call(state, &mut call);
        self.update_screen_flash(state, &screen_flash, intensity, 1.0);
        self.update_camera_shake(state, &camera_shake, intensity, 1.0);
    }

    /// `player_effect_add_continuous_effect`.
    fn add_continuous(&self, state: &mut PlayerEffectState, definition: &ContinuousEffectView, distance: f32) {
        if distance < definition.cutoff_radius {
            let scale = pin(
                1.0 - (distance - definition.falloff_radius) / (definition.falloff_radius - definition.cutoff_radius),
                0.0,
                1.0,
            );
            let periodic = self
                .inputs
                .functions
                .periodic(definition.periodic_function, self.inputs.game_time as f32 / definition.periodic_period);
            let magnitude =
                ((1.0 - definition.periodic_weight) + periodic * definition.periodic_weight) * scale;
            let effect = &mut state.datum;
            if effect.continuous_effect_timer > 0 {
                effect.continuous_effect_timer = 0;
                effect.continuous = [0.0; 4];
            }
            effect.continuous[2] += max(magnitude * definition.random_translation_magnitude, 0.0);
            effect.continuous[3] += max(magnitude * definition.random_rotation_magnitude, 0.0);
            effect.continuous[0] += scale * definition.vibrate_frequencies[0];
            effect.continuous[1] += scale * definition.vibrate_frequencies[1];
        }
    }

    /// `get_shake_matrix`.
    fn shake_matrix(&self, state: &mut PlayerEffectState, translation: f32, rotation: f32, matrix: &mut Matrix4x3) {
        if rotation != 0.0 {
            let direction = local_random_direction3d(&mut state.local_random_seed, self.inputs.directions);
            let rotated = crate::vehicle_physics::rotation_from_axis_and_angle(direction, sine(rotation), cosine(rotation));
            matrix.forward = rotated.forward;
            matrix.left = rotated.left;
            matrix.up = rotated.up;
            matrix.scale = rotated.scale;
            matrix.position = rotated.position;
        }
        if translation != 0.0 {
            let direction = local_random_direction3d(&mut state.local_random_seed, self.inputs.directions);
            matrix.position = [direction[0] * translation, direction[1] * translation, direction[2] * translation];
        }
    }

    /// `effect_scale_value`.
    fn scale_value(&self, transition_function: i16, zero_scale_factor: f32, elapsed: f32, duration: f32) -> f32 {
        self.inputs.functions.transition(transition_function, 1.0 - elapsed / duration) * zero_scale_factor
    }

    /// `player_effect_get_camera_effect_matrix`'s, for a local player.
    fn camera_matrix(&self, state: &mut PlayerEffectState) -> Matrix4x3 {
        let local_player_index = state.local_player_index;
        let elapsed = self.inputs.elapsed;
        // (shaken once a tick: from a seed made of the tick, the local seed
        // kept)
        let saved_local_seed = state.local_random_seed;
        let mut tick_seed = (self.inputs.game_time as u32).wrapping_mul(0x9E37_79B1)
            ^ (local_player_index as u32).wrapping_mul(0x85EB_CA6B);
        tick_seed ^= tick_seed >> 16;
        tick_seed = tick_seed.wrapping_mul(0x7FEB_352D);
        tick_seed ^= tick_seed >> 15;
        state.local_random_seed = tick_seed;

        let mut matrix;
        if state.global_flags & SCRIPTED_ACTIVE != 0 {
            let mut intensity = state.scripted_max_intensity;
            matrix = identity();
            if state.scripted_timer > 0 {
                let fraction = state.scripted_timer as f32 / state.scripted_total_time as f32;
                if state.global_flags & SCRIPTED_STOPPING != 0 {
                    intensity = fraction * intensity;
                } else {
                    intensity = (1.0 - fraction) * intensity;
                }
                state.scripted_timer = state.scripted_timer.wrapping_sub(elapsed);
            } else if state.global_flags & SCRIPTED_STOPPING != 0 {
                state.global_flags &= !SCRIPTED_ACTIVE;
                let mut call = PlayerEffectCall::new(call::RUMBLE_SET_SCALE);
                call.f[0] = 0.0;
                self.call(state, &mut call);
            }
            if state.global_flags & SCRIPTED_ACTIVE != 0 {
                intensity = pin(intensity, 0.0, 1.0);
                let mut call = PlayerEffectCall::new(call::RUMBLE_SET_SCALE);
                call.f[0] = intensity;
                self.call(state, &mut call);
                let seed = &mut state.local_random_seed;
                let random_roll = real_local_random_range(seed, -1.0, 1.0);
                let random_pitch = real_local_random_range(seed, -1.0, 1.0);
                let random_yaw = real_local_random_range(seed, -1.0, 1.0);
                matrix = rotation_from_angles(
                    state.scripted_max_rotation[0] * random_yaw * intensity,
                    state.scripted_max_rotation[1] * random_pitch * intensity,
                    state.scripted_max_rotation[2] * random_roll * intensity,
                );
                let seed = &mut state.local_random_seed;
                let random_depth = real_local_random_range(seed, -1.0, 1.0);
                let random_horizontal = real_local_random_range(seed, -1.0, 1.0);
                let random_vertical = real_local_random_range(seed, -1.0, 1.0);
                matrix.position[2] = state.scripted_max_translation[2] * random_depth * intensity;
                matrix.position[1] = state.scripted_max_translation[0] * random_horizontal * intensity;
                matrix.position[0] = state.scripted_max_translation[1] * random_vertical * intensity;
            }
        } else {
            let mut effect_matrix;
            let effect = state.datum;
            if effect.camera_impulse_time_left > 0 || effect.flags & CAMERA_IMPULSE_JUST_STARTED != 0 {
                let scale = if effect.flags & CAMERA_IMPULSE_JUST_STARTED != 0 {
                    1.0
                } else {
                    let duration = effect.camera_impulse.temporary_duration;
                    self.scale_value(
                        effect.camera_impulse.temporary_transition,
                        effect.camera_impulse.temporary_zero_scale_factor,
                        duration - effect.camera_impulse_time_left as f32,
                        duration,
                    )
                };
                state.datum.flags &= !CAMERA_IMPULSE_JUST_STARTED;
                let axis = cross3([0.0, 0.0, 1.0], effect.direction);
                let rotation = scale * effect.camera_impulse.temporary_rotation;
                effect_matrix = crate::vehicle_physics::rotation_from_axis_and_angle(axis, sine(rotation), cosine(rotation));
                let translation = scale * effect.camera_impulse.temporary_translation;
                for c in 0..3 {
                    effect_matrix.position[c] = effect.direction[c] * translation + effect.jitter[c] * scale;
                }
                state.datum.camera_impulse_time_left = state.datum.camera_impulse_time_left.wrapping_sub(elapsed);
                matrix = effect_matrix;
            } else {
                matrix = identity();
            }

            let camera_shake_ticks = state.datum.camera_shake_time_left;
            if camera_shake_ticks > 0 || state.datum.flags & CAMERA_SHAKE_JUST_STARTED != 0 {
                let effect = state.datum;
                effect_matrix = identity();
                let scale = if effect.flags & CAMERA_SHAKE_JUST_STARTED != 0 {
                    1.0
                } else {
                    let duration = effect.camera_shake.duration;
                    self.scale_value(
                        effect.camera_shake.falloff_transition_function,
                        effect.camera_shake.zero_scale_factor,
                        duration - camera_shake_ticks as f32,
                        duration,
                    )
                };
                let periodic = self.inputs.functions.periodic(
                    effect.camera_shake.periodic_function,
                    (effect.camera_shake.duration - effect.camera_shake_time_left as f32) / effect.camera_shake.periodic_period,
                );
                let magnitude =
                    (periodic * effect.camera_shake.periodic_weight + (1.0 - effect.camera_shake.periodic_weight)) * scale;
                let random_translation_magnitude = max(magnitude * effect.camera_shake.random_translation_magnitude, 0.0);
                let random_rotation_magnitude = max(magnitude * effect.camera_shake.random_rotation_magnitude, 0.0);
                state.datum.flags &= !CAMERA_SHAKE_JUST_STARTED;
                let continuous = state.datum.continuous;
                self.shake_matrix(
                    state,
                    random_translation_magnitude + continuous[2],
                    random_rotation_magnitude + continuous[3],
                    &mut effect_matrix,
                );
                let mut call = PlayerEffectCall::new(call::RUMBLE_CONTINUOUS);
                call.a = local_player_index;
                call.f = [continuous[0], continuous[1]];
                self.call(state, &mut call);
                // (the continuous shake is used up once a frame)
                state.datum.continuous_effect_timer = 0;
                state.datum.continuous = [0.0; 4];
                self.shake_matrix(state, random_translation_magnitude, random_rotation_magnitude, &mut effect_matrix);
                state.datum.camera_shake_time_left = state.datum.camera_shake_time_left.wrapping_sub(elapsed);
                matrix = multiply(&matrix, &effect_matrix);
            }
        }
        state.local_random_seed = saved_local_seed;
        matrix
    }

    /// `player_effect_get_screen_flash`'s.
    fn screen_flash(&self, state: &mut PlayerEffectState, screen_flash: &mut RenderScreenFlash) {
        if self.inputs.console_active {
            return;
        }
        let game_time = self.inputs.game_time;
        if state.fade_ticks != NONE as i16
            && (state.fade_fading_out != 0 || game_time.wrapping_sub(state.fade_start_time) <= state.fade_ticks as i32)
        {
            screen_flash.flash_type = RENDER_SCREEN_FLASH_TYPE_LIGHTEN;
            screen_flash.color = [1.0, state.fade_color[0], state.fade_color[1], state.fade_color[2]];
            screen_flash.intensity = if state.fade_ticks > 0 {
                self.inputs.functions.transition(
                    TRANSITION_FUNCTION_COSINE,
                    pin(game_time.wrapping_sub(state.fade_start_time) as f32 / state.fade_ticks as f32, 0.0, 1.0),
                )
            } else {
                1.0
            };
            if state.fade_fading_out == 0 {
                screen_flash.intensity = 1.0 - screen_flash.intensity;
            }
            screen_flash.intensity = pin(screen_flash.intensity, 0.0, 1.0);
        } else if state.local_player_index != NONE {
            state.fade_ticks = NONE as i16;
            let effect = &mut state.datum;
            if effect.screen_flash_time_left > 0 || effect.flags & SCREEN_FLASH_JUST_STARTED != 0 {
                effect.flags &= !SCREEN_FLASH_JUST_STARTED;
                let flash = effect.screen_flash;
                // (the render type map is the identity)
                screen_flash.flash_type = flash.flash_type;
                screen_flash.color = flash.color;
                screen_flash.intensity = if flash.duration > 0.0 {
                    self.inputs.functions.transition(
                        flash.fade_function,
                        flash.zero_scale_factor * (effect.screen_flash_time_left as f32 / flash.duration),
                    )
                } else {
                    flash.zero_scale_factor
                };
                effect.screen_flash_time_left = effect.screen_flash_time_left.wrapping_sub(self.inputs.elapsed);
            }
        }
    }
}

/// A player effect operation (halo_sim.h's `_player_effect_op_*`).
pub enum PlayerEffectOp<'a> {
    Start {
        definition_index: i32,
        definition: &'a DamageEffectView,
        owner_object_index: i32,
        direction: [f32; 3],
        scale: f32,
        total_damage: f32,
    },
    ScreenFlash { screen_flash: ScreenFlash, scale: f32 },
    Telefrag { intensity: f32 },
    Continuous { definition: &'a ContinuousEffectView, distance: f32 },
    CameraMatrix,
    GetScreenFlash,
}

/// What an operation gives back.
#[derive(Debug, Clone, Copy, Default)]
pub struct PlayerEffectOutput {
    pub matrix: Option<Matrix4x3>,
    pub screen_flash: RenderScreenFlash,
}

/// A player effect operation on `state`, where it is left. `screen_flash`
/// is what `GetScreenFlash` starts from (the caller's).
pub fn player_effect(
    op: &PlayerEffectOp,
    inputs: &PlayerEffectInputs,
    host: &impl PlayerEffectHost,
    state: &mut PlayerEffectState,
    screen_flash: RenderScreenFlash,
) -> PlayerEffectOutput {
    let effects = Effects { inputs, host };
    let mut output = PlayerEffectOutput { matrix: None, screen_flash };
    match *op {
        PlayerEffectOp::Start { definition_index, definition, owner_object_index, direction, scale, total_damage } => {
            if state.local_player_index != NONE {
                effects.start(state, definition_index, definition, owner_object_index, direction, scale, total_damage);
            }
        }
        PlayerEffectOp::ScreenFlash { screen_flash, scale } => effects.update_screen_flash(state, &screen_flash, scale, 1.0),
        PlayerEffectOp::Telefrag { intensity } => effects.telefrag(state, intensity),
        PlayerEffectOp::Continuous { definition, distance } => effects.add_continuous(state, definition, distance),
        PlayerEffectOp::CameraMatrix => {
            if state.local_player_index != NONE {
                output.matrix = Some(effects.camera_matrix(state));
            }
        }
        PlayerEffectOp::GetScreenFlash => effects.screen_flash(state, &mut output.screen_flash),
    }
    output
}
