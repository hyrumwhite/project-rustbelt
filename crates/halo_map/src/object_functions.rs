//! An object's function values and change colours (ROADMAP phase 7):
//! objects.c's `object_compute_function_values` and
//! `object_compute_change_colors`, with periodic_functions.c's
//! `periodic_function_evaluate` and `transition_function_evaluate` and
//! bitmap_utilities.c's `rgb_colors_interpolate` (and its HSV conversions),
//! ported as they are, in the same single-precision arithmetic.
//!
//! Each of an object's definition's functions (four at most): a periodic
//! function of the time (each object at its own phase), its period and
//! value scaled by other functions, inverted, wobbled, squared off, stepped,
//! sawtoothed, added to and scaled, then mapped through a transition
//! function and its bounds, and whether it is active. Each change colour
//! (four at most), when its definition scales them: between its bounds (in
//! RGB, or HSV either way round) by a function, darkened by another.
//!
//! The periodic and transition functions' tables are the game's, built when
//! it starts (some from its random numbers).

// (the C's clamps and its index test, as it writes them)
#![allow(clippy::manual_clamp, clippy::int_plus_one)]

use crate::{CacheFile, Result, layout};

const NONE: i32 = -1;
const EPSILON: f32 = 0.0001;
const NUMBER_OF_INCOMING_OBJECT_FUNCTIONS: usize = 4;
const NUMBER_OF_OUTGOING_OBJECT_FUNCTIONS: usize = 4;
const NUMBER_OF_OBJECT_CHANGE_COLORS: usize = 4;

// periodic functions
const PERIODIC_FUNCTION_ONE: i16 = 0;
const NUMBER_OF_PERIODIC_FUNCTIONS: usize = 12;
const SLIDE_PERIODIC_FUNCTION_FLAGS: u32 = (1 << 6) | (1 << 7);
// transition functions
const TRANSITION_FUNCTION_LINEAR: i16 = 0;
const NUMBER_OF_TRANSITION_FUNCTIONS: usize = 6;
// object function flags (bit 1 the additive flag, which the C tests as
// `_object_function_clip_to_bounds_and_normalize`, 1)
const FUNCTION_INVERT: u32 = 1 << 0;
const FUNCTION_ADDITIVE: u32 = 1 << 1;
const FUNCTION_DOES_NOT_DEACTIVATE_BELOW_LOWER_BOUND: u32 = 1 << 2;
// bounds modes
const BOUNDS_CLIP_AND_NORMALIZE: i16 = 1;
const BOUNDS_SCALE_TO_FIT: i16 = 2;
// object definition runtime flags
const SCALED_CHANGE_COLORS: u32 = 1 << 0;
// rgb colour interpolation flags
const INTERPOLATION_HSV: u32 = 1 << 0;
const INTERPOLATION_HSV_REVERSE: u32 = 1 << 1;

/// The game's function tables (1024 bytes each), `None` before it builds
/// them.
#[derive(Debug, Clone, Copy, Default)]
pub struct FunctionTables<'a> {
    pub periodic: [Option<&'a [u8; 1024]>; NUMBER_OF_PERIODIC_FUNCTIONS],
    pub transition: [Option<&'a [u8; 1024]>; NUMBER_OF_TRANSITION_FUNCTIONS],
}

impl FunctionTables<'_> {
    fn initialized(&self) -> bool {
        self.periodic.iter().chain(self.transition.iter()).all(Option::is_some)
    }

    /// `periodic_function_evaluate`.
    pub fn periodic(&self, function_type: i16, time: f32) -> f32 {
        if function_type == PERIODIC_FUNCTION_ONE {
            return 1.0;
        }
        if function_type < 0 || function_type as usize >= NUMBER_OF_PERIODIC_FUNCTIONS {
            return 1.0;
        }
        if !self.initialized() {
            return 0.0;
        }
        let table = self.periodic[function_type as usize].unwrap();
        let time = time * 25.6;
        let fraction = (time as f64 % 1.0) as f32;
        // (fast_ftol: rounded to nearest)
        let mut index = ((time - fraction) as f64).round_ties_even() as i32;
        index &= 1023;
        let first_value = table[index as usize] as f32 * (1.0 / 255.0);
        index = (index + 1) & 1023;
        let mut second_value = table[index as usize] as f32 * (1.0 / 255.0);
        if SLIDE_PERIODIC_FUNCTION_FLAGS & (1 << function_type) != 0 {
            if first_value > 0.75 && second_value < 0.25 {
                second_value += 1.0;
            }
            let result = (1.0 - fraction) * first_value + second_value * fraction;
            return if result > 1.0 { result - 1.0 } else { result };
        }
        (1.0 - fraction) * first_value + second_value * fraction
    }

    /// `transition_function_evaluate`.
    pub fn transition(&self, function_type: i16, value: f32) -> f32 {
        let value = if value < 0.0 {
            0.0
        } else if value > 1.0 {
            1.0
        } else {
            value
        };
        if function_type == TRANSITION_FUNCTION_LINEAR {
            return value;
        }
        if function_type < 0 || function_type as usize >= NUMBER_OF_TRANSITION_FUNCTIONS {
            return value;
        }
        if !self.initialized() {
            return 0.0;
        }
        let table = self.transition[function_type as usize].unwrap();
        let scaled = value * 1023.0;
        let fraction = (scaled as f64 % 1.0) as f32;
        let index = ((scaled - 0.5) as f64).round_ties_even() as i32;
        if index as i16 == 1023 {
            return table[1023] as f32 * (1.0 / 255.0);
        }
        let index = index as i16 as usize;
        let first_value = table[index] as f32 * (1.0 / 255.0);
        let second_value = table[index + 1] as f32 * (1.0 / 255.0);
        first_value * (1.0 - fraction) + second_value * fraction
    }
}

/// An object's functions and change colours, as their computation reads
/// and changes them (the game's fields, laid out for the game's side,
/// `halo_sim.c`).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ObjectFunctionsState {
    pub object_index: u32,
    pub definition_index: u32,
    pub functions_active_flags: u8,
    pub pad: [u8; 3],
    pub incoming_function_values: [f32; NUMBER_OF_INCOMING_OBJECT_FUNCTIONS],
    pub outgoing_function_values: [f32; NUMBER_OF_OUTGOING_OBJECT_FUNCTIONS],
    pub outgoing_change_colors: [[f32; 3]; NUMBER_OF_OBJECT_CHANGE_COLORS],
}

const _: () = assert!(std::mem::size_of::<ObjectFunctionsState>() == 92);

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct FunctionDefinition {
    pub flags: u32,
    pub scale_period_by_function_index: i16,
    pub function_type: i16,
    pub scale_function_by_function_index: i16,
    pub wobble_function_type: i16,
    pub wobble_period: f32,
    pub wobble_magnitude: f32,
    pub square_wave_threshold: f32,
    pub step_count: i16,
    pub map_result_to_transition_function: i16,
    pub add_function_index: i16,
    pub scale_result_by_function_index: i16,
    pub bounds_mode: i16,
    pub lower_bound: f32,
    pub upper_bound: f32,
    pub turn_off_with_function_index: i16,
    pub scale_by: f32,
    pub reciprocal_bounds_range: f32,
    pub reciprocal_sawtooth_count: f32,
    pub reciprocal_step_count: f32,
    pub one_over_period: f32,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ChangeColorDefinition {
    pub darken_by: i16,
    pub scaled_by: i16,
    pub scale_flags: u32,
    pub color_lower_bound: [f32; 3],
    pub color_upper_bound: [f32; 3],
}

/// An object definition's functions and change colours (the first four
/// of each), and its runtime flags.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ObjectFunctionsDefinition {
    pub runtime_flags: u32,
    pub functions: Vec<FunctionDefinition>,
    pub change_colors: Vec<ChangeColorDefinition>,
}

impl ObjectFunctionsDefinition {
    pub fn load(cache: &CacheFile, datum: u32) -> Result<Self> {
        use layout::{_object_definition as o, object_change_color_definition as c, object_function_definition as f};
        let root = cache.tag_root(datum, crate::groups::OBJECT)?;
        let mut functions = Vec::new();
        for function in root.block(o::FUNCTIONS, f::SIZEOF)?.iter().take(NUMBER_OF_OUTGOING_OBJECT_FUNCTIONS) {
            functions.push(FunctionDefinition {
                flags: function.u32(f::FLAGS)?,
                scale_period_by_function_index: function.i16(f::SCALE_PERIOD_BY_FUNCTION_INDEX)?,
                function_type: function.i16(f::FUNCTION_TYPE)?,
                scale_function_by_function_index: function.i16(f::SCALE_FUNCTION_BY_FUNCTION_INDEX)?,
                wobble_function_type: function.i16(f::WOBBLE_FUNCTION_TYPE)?,
                wobble_period: function.f32(f::WOBBLE_PERIOD)?,
                wobble_magnitude: function.f32(f::WOBBLE_MAGNITUDE)?,
                square_wave_threshold: function.f32(f::SQUARE_WAVE_THRESHOLD)?,
                step_count: function.i16(f::STEP_COUNT)?,
                map_result_to_transition_function: function.i16(f::MAP_RESULT_TO_TRANSITION_FUNCTION)?,
                add_function_index: function.i16(f::ADD_FUNCTION_INDEX)?,
                scale_result_by_function_index: function.i16(f::SCALE_RESULT_BY_FUNCTION_INDEX)?,
                bounds_mode: function.i16(f::BOUNDS_MODE)?,
                lower_bound: function.f32(f::LOWER_BOUND)?,
                upper_bound: function.f32(f::UPPER_BOUND)?,
                turn_off_with_function_index: function.i16(f::TURN_OFF_WITH_FUNCTION_INDEX)?,
                scale_by: function.f32(f::SCALE_BY)?,
                reciprocal_bounds_range: function.f32(f::RUNTIME_RECIPROCAL_BOUNDS_RANGE)?,
                reciprocal_sawtooth_count: function.f32(f::RUNTIME_RECIPROCAL_SAWTOOTH_COUNT)?,
                reciprocal_step_count: function.f32(f::RUNTIME_RECIPROCAL_STEP_COUNT)?,
                one_over_period: function.f32(f::RUNTIME_ONE_OVER_PERIOD)?,
            });
        }
        let mut change_colors = Vec::new();
        for color in root.block(o::CHANGE_COLORS, c::SIZEOF)?.iter().take(NUMBER_OF_OBJECT_CHANGE_COLORS) {
            change_colors.push(ChangeColorDefinition {
                darken_by: color.i16(c::DARKEN_BY)?,
                scaled_by: color.i16(c::SCALED_BY)?,
                scale_flags: color.u32(c::SCALE_FLAGS)?,
                color_lower_bound: color.vec3(c::COLOR_LOWER_BOUND)?,
                color_upper_bound: color.vec3(c::COLOR_UPPER_BOUND)?,
            });
        }
        Ok(ObjectFunctionsDefinition { runtime_flags: root.u32(o::RUNTIME_FLAGS)?, functions, change_colors })
    }
}

/// `OBJECT_INCOMING_FUNCTION_GET_VALUE`: 1-4 the incoming functions, 5-8 the
/// outgoing ones, anything else none (0).
fn function_value(state: &ObjectFunctionsState, index: i16) -> f32 {
    let i = (index as i32 - 1) as u32;
    if i >= (NUMBER_OF_INCOMING_OBJECT_FUNCTIONS + NUMBER_OF_OUTGOING_OBJECT_FUNCTIONS) as u32 {
        0.0
    } else if index as usize >= NUMBER_OF_INCOMING_OBJECT_FUNCTIONS + 1 {
        state.outgoing_function_values[index as usize - NUMBER_OF_INCOMING_OBJECT_FUNCTIONS - 1]
    } else {
        state.incoming_function_values[index as usize - 1]
    }
}

/// `object_compute_function_values`. `game_time` the game's tick.
pub fn object_compute_function_values(
    definition: &ObjectFunctionsDefinition,
    tables: &FunctionTables,
    game_time: i32,
    state: &mut ObjectFunctionsState,
) {
    let absolute_index = (state.object_index & 0xffff) as i32;
    let huh = (57i32.wrapping_mul(absolute_index).wrapping_add(game_time)) as f32 * 0.033333335;
    for (function_index, function) in definition.functions.iter().enumerate() {
        let mut function_is_active = true;
        let mut period = function.one_over_period;
        if function.scale_period_by_function_index != 0 {
            let function_value = function_value(state, function.scale_period_by_function_index);
            if function_value > 0.0 {
                period /= function_value;
            }
        }

        let mut value = tables.periodic(function.function_type, huh * period);
        if function.scale_function_by_function_index != 0 {
            value *= function_value(state, function.scale_function_by_function_index);
        }
        if function.flags & FUNCTION_INVERT != 0 {
            value = 1.0 - value;
        }
        if function.wobble_magnitude != 0.0 {
            let val = tables.periodic(function.wobble_function_type, huh * function.wobble_period);
            value += 2.0 * function.wobble_magnitude * (val - 0.5);
        }
        if function.square_wave_threshold != 0.0 {
            value = if value > function.square_wave_threshold { 1.0 } else { 0.0 };
        }
        if function.step_count > 1 {
            let floor_value = ((function.step_count as f32 * value) as f64).floor() as f32;
            value = floor_value * function.reciprocal_step_count;
        }
        if function.reciprocal_sawtooth_count > 0.0 {
            value = (value as f64 % function.reciprocal_sawtooth_count as f64) as f32;
        }
        if function.add_function_index != 0 {
            value += function_value(state, function.add_function_index);
            value = if value < 1.0 { value } else { 1.0 };
        }
        if function.scale_result_by_function_index != 0 {
            value *= function_value(state, function.scale_result_by_function_index);
        }

        let mut output = tables.transition(function.map_result_to_transition_function, value);
        if function.scale_by > 0.0 {
            output *= function.scale_by;
        }
        if function.bounds_mode == BOUNDS_SCALE_TO_FIT {
            output *= function.upper_bound - function.lower_bound;
            output += function.lower_bound;
            if function.lower_bound + EPSILON >= output {
                function_is_active = function.flags & FUNCTION_DOES_NOT_DEACTIVATE_BELOW_LOWER_BOUND != 0;
            }
        } else {
            if function.lower_bound + EPSILON >= output {
                output = function.lower_bound;
                function_is_active = function.flags & FUNCTION_DOES_NOT_DEACTIVATE_BELOW_LOWER_BOUND != 0;
            }
            if output > function.upper_bound {
                output = function.upper_bound;
            }
            if function.bounds_mode == BOUNDS_CLIP_AND_NORMALIZE {
                output = (output - function.lower_bound) * function.reciprocal_bounds_range;
            }
        }

        let turn_off = function.turn_off_with_function_index as i32;
        if turn_off != NONE && !((0..32).contains(&turn_off) && (state.functions_active_flags as u32 >> turn_off) & 1 != 0) {
            function_is_active = false;
        }
        if function.flags & FUNCTION_ADDITIVE != 0 {
            output = ((output + state.outgoing_function_values[function_index]) as f64 % 1.0) as f32;
        }

        state.outgoing_function_values[function_index] = output;
        if function_is_active {
            state.functions_active_flags |= 1 << function_index;
        } else {
            state.functions_active_flags &= !(1 << function_index);
        }
    }
}

/// `real_rgb_color_to_real_hsv_color`: hue, saturation, value.
fn rgb_to_hsv(rgb: [f32; 3]) -> [f32; 3] {
    let [red, green, blue] = rgb;
    let value = if red > if green > blue { green } else { blue } {
        red
    } else if green > blue {
        green
    } else {
        blue
    };
    let minimum = if red > if green > blue { blue } else { green } {
        if green > blue { blue } else { green }
    } else {
        red
    };
    let delta = value - minimum;
    let saturation = if value == 0.0 { 0.0 } else { delta / value };
    if saturation == 0.0 {
        return [0.0, saturation, value];
    }
    let mut hue = if red == value {
        (green - blue) / delta
    } else if green == value {
        (blue - red) / delta + 2.0
    } else {
        (red - green) / delta + 4.0
    };
    hue *= 1.0 / 6.0;
    if hue < 0.0 {
        hue += 1.0;
    }
    [hue, saturation, value]
}

/// `real_hsv_color_to_real_rgb_color` (`rgb` left as it was for a sector
/// past five).
fn hsv_to_rgb(hsv: [f32; 3], rgb: &mut [f32; 3]) {
    let [hue, saturation, value] = hsv;
    let mut scaled_hue = hue * 6.0;
    if saturation == 0.0 {
        *rgb = [value; 3];
        return;
    }
    let truncated_sector = scaled_hue as i32;
    let sector = if truncated_sector as f32 > scaled_hue { truncated_sector - 1 } else { truncated_sector };
    scaled_hue -= sector as f32;
    let p = (1.0 - saturation) * value;
    let q = (1.0 - scaled_hue * saturation) * value;
    let t = (1.0 - (1.0 - scaled_hue) * saturation) * value;
    match sector {
        0 => *rgb = [value, t, p],
        1 => *rgb = [q, value, p],
        2 => *rgb = [p, value, t],
        3 => *rgb = [p, q, value],
        4 => *rgb = [t, p, value],
        5 => *rgb = [value, p, q],
        _ => {}
    }
}

/// `rgb_colors_interpolate`.
fn rgb_colors_interpolate(result: &mut [f32; 3], flags: u32, lower: [f32; 3], upper: [f32; 3], factor: f32) {
    let inverse = 1.0 - factor;
    if flags & INTERPOLATION_HSV != 0 {
        let mut hsv_lower = rgb_to_hsv(lower);
        let mut hsv_upper = rgb_to_hsv(upper);
        if (((hsv_lower[0] - hsv_upper[0]) as f64).abs() > 0.5) != (flags & INTERPOLATION_HSV_REVERSE != 0) {
            if hsv_lower[0] < hsv_upper[0] {
                hsv_lower[0] += 1.0;
            } else {
                hsv_upper[0] += 1.0;
            }
        }
        let mut hue = inverse * hsv_lower[0] + factor * hsv_upper[0];
        if hue > 1.0 {
            hue -= 1.0;
        }
        let saturation = inverse * hsv_lower[1] + factor * hsv_upper[1];
        let value = inverse * hsv_lower[2] + factor * hsv_upper[2];
        hsv_to_rgb([hue, saturation, value], result);
    } else {
        for c in 0..3 {
            result[c] = inverse * lower[c] + factor * upper[c];
        }
    }
}

/// `PIN(value, 0, 1)`.
fn pin01(value: f32) -> f32 {
    if value < 0.0 {
        0.0
    } else if value > 1.0 {
        1.0
    } else {
        value
    }
}

/// `object_compute_change_colors`.
pub fn object_compute_change_colors(definition: &ObjectFunctionsDefinition, state: &mut ObjectFunctionsState) {
    if definition.runtime_flags & SCALED_CHANGE_COLORS == 0 {
        return;
    }
    for (index, change_color) in definition.change_colors.iter().enumerate() {
        if change_color.scaled_by != 0 {
            let factor = function_value(state, change_color.scaled_by);
            let mut color = state.outgoing_change_colors[index];
            rgb_colors_interpolate(&mut color, change_color.scale_flags, change_color.color_lower_bound, change_color.color_upper_bound, factor);
            state.outgoing_change_colors[index] = color;
        }
        if change_color.darken_by != 0 {
            let scale = function_value(state, change_color.darken_by);
            for c in &mut state.outgoing_change_colors[index] {
                *c *= scale;
            }
        }
        for c in &mut state.outgoing_change_colors[index] {
            *c = pin01(*c);
        }
    }
}
