//! Transparent shaders as the Xbox's register combiner programs.
//!
//! `shader_transparent_chicago` and `shader_transparent_generic` are drawn
//! by NV2A pixel shaders that their preprocessors build from the tag
//! (`shader_transparent_chicago_preprocessor.c`,
//! `shader_transparent_generic_preprocessor.c`). This module builds the same
//! programs: general combiner stages (each `A*B`, `C*D`, their sum or mux, with
//! input and output mappings) over registers holding the textures (t0-t3),
//! constants (c0, c1), vertex colours (v0, v1) and scratch (r0, r1); then the
//! final combiner. A renderer evaluates them per pixel (the port's
//! `nv2a_psh.c` is the reference for that evaluation).

use crate::{layout, shader::ShaderType, CacheFile, Ptr, Result, TagRef};

pub const MAXIMUM_STAGES: usize = 8;

/// One NV2A pixel shader's combiner state.
#[derive(Debug, Clone, Default)]
pub struct CombinerProgram {
    /// General combiner stages used.
    pub stage_count: usize,
    pub rgb_inputs: [u32; MAXIMUM_STAGES],
    pub rgb_outputs: [u32; MAXIMUM_STAGES],
    pub alpha_inputs: [u32; MAXIMUM_STAGES],
    pub alpha_outputs: [u32; MAXIMUM_STAGES],
    /// Per-stage constants c0 and c1 (RGBA, 0..1).
    pub constant0: [[f32; 4]; MAXIMUM_STAGES],
    pub constant1: [[f32; 4]; MAXIMUM_STAGES],
    /// `PSCombinerCount` flags: 0x100 mux on r0.a's top bit, 0x1000 a c0 per
    /// stage, 0x10000 a c1 per stage.
    pub flags: u32,
    pub final_abcd: u32,
    pub final_efg: u32,
}

/// The chicago combiner table: per map function, the stage's RGB inputs
/// (without and with the alpha-replicate flag), alpha inputs, and how much
/// each input byte's register advances per map (the next map's texture).
const CHICAGO_RGB_INPUTS: [[u32; 13]; 2] = [
    [
        0x0c200000, 0x08200000, 0x080c0000, 0x080c080c, 0x08200c20, 0x08204c20, 0x0c204820, 0x08200c40, 0x0c200840,
        0x081c0c3c, 0x0c1c083c, 0x08180c38, 0x0c180838,
    ],
    [
        0x0c200000, 0x18200000, 0x180c0000, 0x180c180c, 0x18200c20, 0x18204c20, 0x0c205820, 0x18200c40, 0x0c201840,
        0x181c0c3c, 0x0c1c183c, 0x18180c38, 0x0c181838,
    ],
];
const CHICAGO_ALPHA_INPUTS: [u32; 13] = [
    0x1c200000, 0x18200000, 0x181c0000, 0x181c181c, 0x18201c20, 0x18205c20, 0x1c205820, 0x18201c40, 0x1c201840,
    0x181c1c3c, 0x1c1c183c, 0x18181c38, 0x1c181838,
];
const CHICAGO_STAGE_INCREMENTS: [u32; 13] = [
    0x00000000, 0x01000000, 0x01000000, 0x01000100, 0x01000000, 0x01000000, 0x00000100, 0x01000000, 0x00000100,
    0x01000000, 0x00000100, 0x01010001, 0x00010101,
];
const CHICAGO_MAXIMUM_MAPS: usize = 4;
const CHICAGO_MAP_ALPHA_REPLICATE: u16 = 1 << 1;

/// The generic preprocessor's register and mapping tables (indices are the
/// tag's enum values).
const GENERIC_COLOR_INPUT_REGISTERS: [i32; 25] =
    [-1, -1, -1, -1, -1, 8, 9, 10, 11, 4, 5, 12, 13, 1, 2, 24, 25, 26, 27, 20, 21, 28, 29, 17, 18];
const GENERIC_ALPHA_INPUT_REGISTERS: [i32; 25] =
    [-1, -1, -1, -1, -1, 24, 25, 26, 27, 20, 21, 28, 29, 17, 18, 8, 9, 10, 11, 4, 5, 12, 13, 1, 2];
const GENERIC_INPUT_MAPPINGS: [u32; 8] = [0x00, 0x20, 0x40, 0x60, 0x80, 0xa0, 0xc0, 0xe0];
/// The five constant inputs (zero, one, one half, negative one, negative one
/// half), per mapping: a zero register with a mapping that makes the value.
const GENERIC_CONSTANT_INPUTS: [[u32; 8]; 5] = [
    [0x00, 0x20, 0x40, 0x20, 0x80, 0xa0, 0x00, 0x00],
    [0x20, 0x00, 0x20, 0x40, 0xa0, 0x80, 0x20, 0x40],
    [0xa0, 0xa0, 0x00, 0x00, 0x00, 0x00, 0xa0, 0x80],
    [0x00, 0x20, 0x40, 0x20, 0x80, 0xa0, 0x40, 0x20],
    [0x00, 0x20, 0x40, 0x20, 0x80, 0xa0, 0x80, 0xa0],
];
const GENERIC_COLOR_OUTPUT_REGISTERS: [u32; 9] = [0, 12, 13, 4, 5, 8, 9, 10, 11];
const GENERIC_ALPHA_OUTPUT_REGISTERS: [u32; 9] = [0, 28, 29, 20, 21, 24, 25, 26, 27];
const GENERIC_OUTPUT_MAPPINGS: [u32; 6] = [0x00, 0x30, 0x10, 0x20, 0x08, 0x18];
const GENERIC_STAGE_COLOR_MUX: u16 = 1 << 0;
const GENERIC_STAGE_ALPHA_MUX: u16 = 1 << 1;
/// The generic shader keeps one combiner for the draw's fog/fade stage.
const GENERIC_MAXIMUM_STAGES: usize = MAXIMUM_STAGES - 1;

const OUTPUT_FLAG_CD_DOT: u32 = 1 << 0;
const OUTPUT_FLAG_AB_DOT: u32 = 1 << 1;
const OUTPUT_FLAG_MUX: u32 = 1 << 2;

/// The combiner program of a transparent chicago or generic shader.
pub fn program(cache: &CacheFile, shader: TagRef) -> Result<Option<CombinerProgram>> {
    let Some(datum) = shader.datum else { return Ok(None) };
    let root = cache.tag_root(datum, crate::groups::SHADER)?;
    let shader_type = crate::shader::shader_type(root.i16(layout::shader_base::TYPE)?);
    Ok(match shader_type {
        ShaderType::TransparentChicago => Some(chicago(root)?),
        ShaderType::TransparentGeneric => Some(generic(root)?),
        _ => None,
    })
}

/// `_shader_type_transparent_meter`'s pixel shader (its constants are the
/// draw's: `MeterInfo::constants`). The final combiner takes r0 times the
/// map's alpha, and r0's alpha; the map's zero alpha is killed.
pub fn meter(flash_color_is_negative: bool) -> CombinerProgram {
    let mut program = CombinerProgram {
        stage_count: 4,
        flags: 0x11100,
        final_abcd: 0x0C180000,
        final_efg: 0x00001C00,
        ..Default::default()
    };
    program.alpha_inputs[..4].copy_from_slice(&[0x12081208, 0x6C200000, 0x0820B120, 0x12201120]);
    program.alpha_outputs[..4].copy_from_slice(&[0x20C00, 0xC0, 0xC00, 0x4C00]);
    program.rgb_inputs[..4].copy_from_slice(&[
        0x1120E820,
        0x3C011C02,
        ((if flash_color_is_negative { 0xE0 } else { 0 }) + 2) | 0x0C201C00,
        0x0C200120,
    ]);
    program.rgb_outputs[..4].copy_from_slice(&[0x20C00, 0xC00, 0xC00, 0x4C00]);
    program
}

/// rasterizer_xbox_plasma_energy.c's pixel shader: t0 and t1 the two noise
/// maps, v0 the colour (and alpha) by the view's angle.
pub fn plasma() -> CombinerProgram {
    let mut program = CombinerProgram {
        stage_count: 4,
        flags: 0x100,
        final_abcd: 0x0C0F0000,
        final_efg: 0x1C1C1400,
        ..Default::default()
    };
    program.alpha_inputs[..4].copy_from_slice(&[0x0820A920, 0x1C1C0C0C, 0x00005C5C, 0x14150000]);
    program.alpha_outputs[..4].copy_from_slice(&[0xC00, 0x24C00, 0x4D00, 0x40]);
    program.rgb_inputs[..4].copy_from_slice(&[0x1920B820, 0, 0, 0x1C051DA0]);
    program.rgb_outputs[..4].copy_from_slice(&[0xC00, 0, 0, 0xC00]);
    program
}

/// `shader_transparent_chicago_create`.
fn chicago(root: Ptr<'_>) -> Result<CombinerProgram> {
    use layout::{shader_transparent as t, shader_transparent_chicago_map as m};
    let maps = root.block(layout::shader_transparent_chicago_definition::TRANSPARENT + t::MAPS, m::SIZEOF)?;
    let map_count = maps.count.min(CHICAGO_MAXIMUM_MAPS);
    let mut program = CombinerProgram {
        stage_count: map_count + 1,
        flags: 0x11000,
        final_abcd: 0x0000000C,
        final_efg: 0x00001C00,
        ..Default::default()
    };
    for map_index in 0..map_count {
        let map = maps.get(map_index).unwrap();
        if map_index + 1 < map_count {
            let color = map.i16(m::COLOR_FUNCTION)?;
            let alpha = map.i16(m::ALPHA_FUNCTION)?;
            if (0..13).contains(&color) && (0..13).contains(&alpha) {
                let replicate = usize::from(map.u16(m::FLAGS)? & CHICAGO_MAP_ALPHA_REPLICATE != 0);
                let next = map_index as u32 + 1;
                program.alpha_inputs[map_index + 1] =
                    CHICAGO_STAGE_INCREMENTS[alpha as usize] * next + CHICAGO_ALPHA_INPUTS[alpha as usize];
                program.rgb_inputs[map_index + 1] =
                    CHICAGO_STAGE_INCREMENTS[color as usize] * next + CHICAGO_RGB_INPUTS[replicate][color as usize];
            }
        } else {
            // the first stage loads map 0 into r0
            program.alpha_inputs[0] = 0x18200000;
            program.rgb_inputs[0] = 0x08200000;
        }
        program.alpha_outputs[map_index] = 0x00000c00;
        program.rgb_outputs[map_index] = 0x00000c00;
    }
    Ok(program)
}

fn generic_input(registers: &[i32; 25], register: i16, mapping: i16) -> u32 {
    let (Ok(register), Ok(mapping)) = (usize::try_from(register), usize::try_from(mapping)) else { return 0 };
    if register >= 25 || mapping >= 8 {
        return 0;
    }
    match registers[register] {
        -1 => GENERIC_CONSTANT_INPUTS[register][mapping],
        value => value as u32 | GENERIC_INPUT_MAPPINGS[mapping],
    }
}

fn generic_output(registers: &[u32; 9], register: i16) -> u32 {
    usize::try_from(register).ok().and_then(|r| registers.get(r)).copied().unwrap_or(0)
}

fn argb(p: Ptr<'_>, offset: usize) -> Result<[f32; 4]> {
    // real_argb_color: alpha, red, green, blue
    let (a, r, g, b) = (p.f32(offset)?, p.f32(offset + 4)?, p.f32(offset + 8)?, p.f32(offset + 12)?);
    Ok([r, g, b, a].map(|c| c.clamp(0.0, 1.0)))
}

/// `shader_transparent_generic_create`, with each stage's c0 at the middle
/// of its animation (the draw animates it between the two bounds).
fn generic(root: Ptr<'_>) -> Result<CombinerProgram> {
    use layout::shader_transparent_generic_stage as s;
    let stages = root.block(layout::shader_transparent_generic_definition::STAGES, s::SIZEOF)?;
    let stage_count = stages.count.min(GENERIC_MAXIMUM_STAGES);
    let mut program = CombinerProgram {
        stage_count: stage_count.max(1),
        flags: 0x11100,
        final_abcd: 0x0000000C,
        final_efg: 0x00001C00,
        ..Default::default()
    };
    if stage_count == 0 {
        program.rgb_inputs[0] = 0x08200000;
        program.rgb_outputs[0] = 0x000000c0;
        program.alpha_inputs[0] = 0x18200000;
        program.alpha_outputs[0] = 0x000000c0;
        return Ok(program);
    }
    for index in 0..stage_count {
        let stage = stages.get(index).unwrap();
        let flags = stage.u16(s::FLAGS)?;
        let short = |offset: usize| stage.i16(offset);
        // color inputs A-D, then outputs AB, AB function, CD, CD function,
        // mux/sum, mapping; alpha inputs A-D, then outputs A, B, C, mapping
        let ci = s::COLOR_INPUT_A;
        let color_inputs = [
            generic_input(&GENERIC_COLOR_INPUT_REGISTERS, short(ci)?, short(ci + 2)?),
            generic_input(&GENERIC_COLOR_INPUT_REGISTERS, short(ci + 4)?, short(ci + 6)?),
            generic_input(&GENERIC_COLOR_INPUT_REGISTERS, short(ci + 8)?, short(ci + 10)?),
            generic_input(&GENERIC_COLOR_INPUT_REGISTERS, short(ci + 12)?, short(ci + 14)?),
        ];
        let co = s::COLOR_OUTPUT_AB;
        let color_ab = generic_output(&GENERIC_COLOR_OUTPUT_REGISTERS, short(co)?);
        let ab_dot = short(co + 2)? == 1;
        let color_cd = generic_output(&GENERIC_COLOR_OUTPUT_REGISTERS, short(co + 4)?);
        let cd_dot = short(co + 6)? == 1;
        let color_sum = generic_output(&GENERIC_COLOR_OUTPUT_REGISTERS, short(co + 8)?);
        let color_mapping = usize::try_from(short(s::COLOR_OUTPUT_MAPPING)?).ok();
        let mut color_flags = color_mapping.and_then(|m| GENERIC_OUTPUT_MAPPINGS.get(m)).copied().unwrap_or(0);
        if ab_dot {
            color_flags |= OUTPUT_FLAG_AB_DOT;
        }
        if cd_dot {
            color_flags |= OUTPUT_FLAG_CD_DOT;
        }
        if flags & GENERIC_STAGE_COLOR_MUX != 0 {
            color_flags |= OUTPUT_FLAG_MUX;
        }

        let ai = s::ALPHA_INPUT_A;
        let alpha_inputs = [
            generic_input(&GENERIC_ALPHA_INPUT_REGISTERS, short(ai)?, short(ai + 2)?),
            generic_input(&GENERIC_ALPHA_INPUT_REGISTERS, short(ai + 4)?, short(ai + 6)?),
            generic_input(&GENERIC_ALPHA_INPUT_REGISTERS, short(ai + 8)?, short(ai + 10)?),
            generic_input(&GENERIC_ALPHA_INPUT_REGISTERS, short(ai + 12)?, short(ai + 14)?),
        ];
        let ao = s::ALPHA_OUTPUT_A;
        let alpha_a = generic_output(&GENERIC_ALPHA_OUTPUT_REGISTERS, short(ao)?);
        let alpha_b = generic_output(&GENERIC_ALPHA_OUTPUT_REGISTERS, short(ao + 2)?);
        let alpha_c = generic_output(&GENERIC_ALPHA_OUTPUT_REGISTERS, short(ao + 4)?);
        let alpha_mapping = usize::try_from(short(s::ALPHA_OUTPUT_MAPPING)?).ok();
        let mut alpha_flags = alpha_mapping.and_then(|m| GENERIC_OUTPUT_MAPPINGS.get(m)).copied().unwrap_or(0);
        if flags & GENERIC_STAGE_ALPHA_MUX != 0 {
            alpha_flags |= OUTPUT_FLAG_MUX;
        }

        let pack = |i: [u32; 4]| ((i[0] << 8 | i[1]) << 8 | i[2]) << 8 | i[3];
        program.rgb_inputs[index] = pack(color_inputs);
        program.rgb_outputs[index] = ((color_flags << 4 | (color_sum & 0xf)) << 4 | (color_ab & 0xf)) << 4 | (color_cd & 0xf);
        program.alpha_inputs[index] = pack(alpha_inputs);
        program.alpha_outputs[index] =
            ((alpha_flags << 4 | (alpha_c & 0xf)) << 4 | (alpha_a & 0xf)) << 4 | (alpha_b & 0xf);

        let lower = argb(stage, s::CONSTANT_COLOR0_LOWER_BOUND)?;
        let upper = argb(stage, s::CONSTANT_COLOR0_UPPER_BOUND)?;
        program.constant0[index] = std::array::from_fn(|c| (lower[c] + upper[c]) * 0.5);
        program.constant1[index] = argb(stage, s::COLOR1)?;
    }
    Ok(program)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generic_constant_one_is_inverted_zero() {
        // input "one" (1) with the unsigned identity mapping reads 1 - zero
        assert_eq!(generic_input(&GENERIC_COLOR_INPUT_REGISTERS, 1, 0), 0x20);
        // map color 0 (5) with the unsigned invert mapping (1): t0, inverted
        assert_eq!(generic_input(&GENERIC_COLOR_INPUT_REGISTERS, 5, 1), 0x28);
        // its alpha form reads t0's alpha
        assert_eq!(generic_input(&GENERIC_ALPHA_INPUT_REGISTERS, 5, 0), 24);
    }
}

/// Evaluates a combiner program for one pixel, as `halo_transparent.wesl`
/// does on the GPU (and the port's `nv2a_psh.c`): `textures` are t0-t3 as
/// stored (8-bit gamma, 0..1); v0 and v1 are white. Returns RGBA, 0..1.
pub fn evaluate(program: &CombinerProgram, textures: [[f32; 4]; 4]) -> [f32; 4] {
    type Rgba = [f32; 4];
    let mut registers = [[0.0f32; 4]; 16];
    registers[8..12].copy_from_slice(&textures);
    registers[4] = [1.0; 4];
    registers[5] = [1.0; 4];
    registers[12] = [0.0, 0.0, 0.0, textures[0][3]];

    let unique_c0 = program.flags & 0x1000 != 0;
    let unique_c1 = program.flags & 0x10000 != 0;
    let mux_msb = program.flags & 0x100 != 0;
    fn map4(v: Rgba, f: impl Fn(f32) -> f32) -> Rgba {
        v.map(f)
    }
    fn mul(a: Rgba, b: Rgba) -> Rgba {
        std::array::from_fn(|i| a[i] * b[i])
    }
    fn dot3(a: Rgba, b: Rgba) -> f32 {
        a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
    }
    /// One input byte's register as RGB, or as a scalar for the alpha portion.
    fn channels(source: Rgba, byte: u32, alpha_portion: bool) -> Rgba {
        let alpha_channel = byte & 0x10 != 0;
        if alpha_portion {
            [if alpha_channel { source[3] } else { source[2] }; 4]
        } else if alpha_channel {
            [source[3]; 4]
        } else {
            [source[0], source[1], source[2], 0.0]
        }
    }

    let read = |registers: &[Rgba; 16], index: u32, stage: usize| -> Rgba {
        match index {
            1 => program.constant0[if unique_c0 { stage } else { 0 }],
            2 => program.constant1[if unique_c1 { stage } else { 0 }],
            i => registers[i as usize],
        }
    };
    let input = |registers: &[Rgba; 16], byte: u32, alpha_portion: bool, stage: usize| -> Rgba {
        let value = channels(read(registers, byte & 0x0f, stage), byte, alpha_portion);
        match byte & 0xe0 {
            0x00 => map4(value, |x| x.max(0.0)),
            0x20 => map4(value, |x| 1.0 - x.clamp(0.0, 1.0)),
            0x40 => map4(value, |x| 2.0 * x.max(0.0) - 1.0),
            0x60 => map4(value, |x| 1.0 - 2.0 * x.max(0.0)),
            0x80 => map4(value, |x| x.max(0.0) - 0.5),
            0xa0 => map4(value, |x| 0.5 - x.max(0.0)),
            0xc0 => value,
            _ => map4(value, |x| -x),
        }
    };
    let output_mapping = |v: Rgba, flags: u32| -> Rgba {
        let mapped = match flags & 0x38 {
            0x08 => map4(v, |x| x - 0.5),
            0x10 => map4(v, |x| x * 2.0),
            0x18 => map4(v, |x| (x - 0.5) * 2.0),
            0x20 => map4(v, |x| x * 4.0),
            0x30 => map4(v, |x| x * 0.5),
            _ => v,
        };
        map4(mapped, |x| x.clamp(-1.0, 1.0))
    };

    for stage in 0..program.stage_count.min(MAXIMUM_STAGES) {
        let mut results = [[0.0f32; 4]; 6];
        for portion in 0..2 {
            let alpha = portion == 1;
            let (inputs, outputs) = if alpha {
                (program.alpha_inputs[stage], program.alpha_outputs[stage])
            } else {
                (program.rgb_inputs[stage], program.rgb_outputs[stage])
            };
            let flags = outputs >> 12;
            let a = input(&registers, (inputs >> 24) & 0xff, alpha, stage);
            let b = input(&registers, (inputs >> 16) & 0xff, alpha, stage);
            let c = input(&registers, (inputs >> 8) & 0xff, alpha, stage);
            let d = input(&registers, inputs & 0xff, alpha, stage);
            let ab = if !alpha && flags & 0x02 != 0 { [dot3(a, b); 4] } else { mul(a, b) };
            let cd = if !alpha && flags & 0x01 != 0 { [dot3(c, d); 4] } else { mul(c, d) };
            let sum = if flags & 0x04 != 0 {
                let r0a = registers[12][3];
                let pick_cd = if mux_msb { r0a >= 0.5 } else { ((r0a * 255.0 + 0.5) as u32) & 1 != 0 };
                if pick_cd { cd } else { ab }
            } else {
                std::array::from_fn(|i| ab[i] + cd[i])
            };
            results[portion * 3] = output_mapping(ab, flags);
            results[portion * 3 + 1] = output_mapping(cd, flags);
            results[portion * 3 + 2] = output_mapping(sum, flags);
        }
        // write back once both portions have read their inputs
        for portion in 0..2 {
            let alpha = portion == 1;
            let outputs = if alpha { program.alpha_outputs[stage] } else { program.rgb_outputs[stage] };
            let destinations = [(outputs >> 4) & 0x0f, outputs & 0x0f, (outputs >> 8) & 0x0f];
            for (k, &destination) in destinations.iter().enumerate() {
                // only v0, v1, t0-t3, r0 and r1 are writable
                if !(destination == 4 || destination == 5 || (8..=13).contains(&destination)) {
                    continue;
                }
                let value = results[portion * 3 + k];
                let register = &mut registers[destination as usize];
                if alpha {
                    register[3] = value[3];
                } else {
                    register[..3].copy_from_slice(&value[..3]);
                }
            }
        }
    }

    // final combiner: rgb = A*B + (1-A)*C + D, alpha = G
    let final_input = |registers: &[Rgba; 16], byte: u32, alpha_portion: bool| -> Rgba {
        let source = match byte & 0x0f {
            1 => program.constant0[0],
            2 => program.constant1[0],
            i => registers[i as usize],
        };
        let value = map4(channels(source, byte, alpha_portion), |x| x.clamp(0.0, 1.0));
        if byte & 0x20 != 0 { map4(value, |x| 1.0 - x) } else { value }
    };
    let (abcd, efg) = (program.final_abcd, program.final_efg);
    let e = final_input(&registers, (efg >> 24) & 0xff, false);
    let f = final_input(&registers, (efg >> 16) & 0xff, false);
    registers[15] = mul(e, f);
    registers[14] = std::array::from_fn(|i| (registers[5][i] + registers[12][i]).clamp(0.0, 1.0));
    let fa = final_input(&registers, (abcd >> 24) & 0xff, false);
    let fb = final_input(&registers, (abcd >> 16) & 0xff, false);
    let fc = final_input(&registers, (abcd >> 8) & 0xff, false);
    let fd = final_input(&registers, abcd & 0xff, false);
    let rgb: [f32; 3] = std::array::from_fn(|i| (fa[i] * fb[i] + (1.0 - fa[i]) * fc[i] + fd[i]).clamp(0.0, 1.0));
    let alpha = final_input(&registers, (efg >> 8) & 0xff, true)[3];
    [rgb[0], rgb[1], rgb[2], alpha]
}

#[cfg(test)]
mod evaluate_tests {
    use super::*;

    /// The generic shader with no stages copies its first map.
    #[test]
    fn stageless_generic_passes_map_0() {
        let mut program =
            CombinerProgram { stage_count: 1, flags: 0x11100, final_abcd: 0x0C, final_efg: 0x1C00, ..Default::default() };
        program.rgb_inputs[0] = 0x08200000;
        program.rgb_outputs[0] = 0x000000c0;
        program.alpha_inputs[0] = 0x18200000;
        program.alpha_outputs[0] = 0x000000c0;
        let t0 = [0.25, 0.5, 0.75, 0.6];
        let out = evaluate(&program, [t0, [0.0; 4], [0.0; 4], [0.0; 4]]);
        for i in 0..4 {
            assert!((out[i] - t0[i]).abs() < 1e-6, "{out:?}");
        }
    }

    /// Chicago "multiply" (function 2): map 0 times map 1.
    #[test]
    fn chicago_multiply() {
        let mut program =
            CombinerProgram { stage_count: 3, flags: 0x11000, final_abcd: 0x0C, final_efg: 0x1C00, ..Default::default() };
        program.rgb_inputs[0] = 0x08200000;
        program.alpha_inputs[0] = 0x18200000;
        program.rgb_inputs[1] = CHICAGO_STAGE_INCREMENTS[2] + CHICAGO_RGB_INPUTS[0][2];
        program.alpha_inputs[1] = CHICAGO_STAGE_INCREMENTS[2] + CHICAGO_ALPHA_INPUTS[2];
        for s in 0..2 {
            program.rgb_outputs[s] = 0xc00;
            program.alpha_outputs[s] = 0xc00;
        }
        let out = evaluate(&program, [[0.5, 0.5, 0.5, 1.0], [0.5, 1.0, 0.0, 0.5], [0.0; 4], [0.0; 4]]);
        assert_eq!(out, [0.25, 0.5, 0.0, 0.5]);
    }
}

/// How far either side of the plasma's animated value
/// [`environment_plasma_band`] reaches.
pub const PLASMA_BAND_REACH: f32 = 0.1;

/// The environment shader's plasma self-illumination (its lightmap pass,
/// rasterizer_xbox_environment.c): where the self-illumination map's alpha
/// is near the plasma function's value, the blue channel lights with the
/// plasma's "on" colour over its "off" one. This is that band's strength,
/// from the game's own combiner program, at `samples` distances from the
/// value (0 to `PLASMA_BAND_REACH`; the distance wraps round 0 and 1): 1 at
/// the value, 0 beyond about 0.08.
pub fn environment_plasma_band(samples: usize) -> Vec<f32> {
    let mut program = CombinerProgram { stage_count: 6, flags: 0x11100, final_abcd: 0x0A0F000C, final_efg: 0x1C011800, ..Default::default() };
    let words = [
        (0x1120B920, 0x00000C00, 0x1920B120, 0x00000C00),
        (0xDCDCCCCC, 0x00024C00, 0x20200000, 0x00002080),
        (0x1C1C0920, 0x000000C9, 0x09010902, 0x000030CD),
        (0x00005C5C, 0x00004C00, 0x0C010D02, 0x00000D00),
        (0x34201408, 0x00000C00, 0x011C0220, 0x00000C00),
        (0x00000000, 0x00000000, 0x0C190D20, 0x00000C00),
    ];
    for (stage, (alpha_inputs, alpha_outputs, rgb_inputs, rgb_outputs)) in words.into_iter().enumerate() {
        program.alpha_inputs[stage] = alpha_inputs;
        program.alpha_outputs[stage] = alpha_outputs;
        program.rgb_inputs[stage] = rgb_inputs;
        program.rgb_outputs[stage] = rgb_outputs;
    }
    // (the value at the middle; the primary and secondary colours black, the
    // plasma's on white and off black; no lightmap, so the result is the
    // self-illumination alone)
    program.constant0[0] = [0.0, 0.0, 0.0, 0.5];
    program.constant0[2] = [1.0, 0.0, 0.0, 0.0];
    program.constant1[2] = [0.0, 1.0, 0.0, 0.0];
    program.constant0[4] = [1.0, 1.0, 1.0, 0.0];
    (0..samples)
        .map(|index| {
            let distance = PLASMA_BAND_REACH * index as f32 / (samples - 1).max(1) as f32;
            evaluate(&program, [[0.5, 0.5, 1.0, 1.0], [0.0, 0.0, 1.0, 0.5 + distance], [0.0; 4], [0.0; 4]])[2]
        })
        .collect()
}

#[cfg(test)]
mod plasma_tests {
    use super::*;

    #[test]
    fn band_peaks_at_the_value() {
        let band = environment_plasma_band(41);
        assert_eq!(band[0], 1.0);
        // (0.02 from it, about half; 0.1, none)
        assert!(band[40] == 0.0 && band[8] > 0.4 && band[8] < 0.6);
    }
}
