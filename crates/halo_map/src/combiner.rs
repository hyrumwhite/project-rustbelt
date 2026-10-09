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
