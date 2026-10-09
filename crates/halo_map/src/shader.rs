//! Shaders: what a surface looks like.
//!
//! Every shader group starts with `shader_base`: radiosity properties (the
//! light a surface emits when the lightmaps are baked) and `type`, which says
//! which structure follows (`shader_get_and_verify_type`).

use crate::{groups, layout, CacheFile, Result, TagRef};

/// `shader_base.type` values (`_shader_type_*`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShaderType {
    Environment,
    Model,
    TransparentGeneric,
    TransparentChicago,
    Water,
    Glass,
    Meter,
    Plasma,
    Other(i16),
}

/// `shader_base.type` as a [`ShaderType`].
pub fn shader_type(raw: i16) -> ShaderType {
    ShaderType::from_raw(raw)
}

impl ShaderType {
    fn from_raw(raw: i16) -> Self {
        // `_shader_type_*` (shaders.c): screen, effect and decal come first
        match raw {
            3 => Self::Environment,
            4 => Self::Model,
            5 => Self::TransparentGeneric,
            6 => Self::TransparentChicago,
            7 => Self::Water,
            8 => Self::Glass,
            9 => Self::Meter,
            10 => Self::Plasma,
            other => Self::Other(other),
        }
    }

    pub fn is_transparent(self) -> bool {
        !matches!(self, Self::Environment | Self::Model | Self::Other(_))
    }
}

/// `framebuffer_blend_function` of transparent shaders.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlendFunction {
    AlphaBlend,
    Multiply,
    DoubleMultiply,
    Add,
    Subtract,
    ComponentMin,
    ComponentMax,
    AlphaMultiplyAdd,
}

impl BlendFunction {
    fn from_raw(raw: i16) -> Self {
        match raw {
            1 => Self::Multiply,
            2 => Self::DoubleMultiply,
            3 => Self::Add,
            4 => Self::Subtract,
            5 => Self::ComponentMin,
            6 => Self::ComponentMax,
            7 => Self::AlphaMultiplyAdd,
            _ => Self::AlphaBlend,
        }
    }
}

/// A transparent shader's map (one per stage).
#[derive(Debug, Clone, Copy)]
pub struct TransparentMap {
    pub bitmap: TagRef,
    pub uv_scale: [f32; 2],
    pub uv_offset: [f32; 2],
}

/// The parts of a transparent generic or chicago shader the renderer uses.
#[derive(Debug, Clone)]
pub struct TransparentInfo {
    pub blend: BlendFunction,
    /// 0 for a 2D first map; 1..=3 for the cube map kinds.
    pub first_map_type: i16,
    /// One per stage, in stage order (the combiner names them t0..t3);
    /// `None` where a stage has no bitmap.
    pub maps: Vec<Option<TransparentMap>>,
}

/// `_detail_map_function_*` (rasterizer_xbox_environment.c).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DetailFunction {
    /// `2 * base * detail`: a 50% grey detail texel leaves the base alone.
    DoubleBiasedMultiply,
    /// `base * detail`.
    Multiply,
    /// `base + 2 * detail - 1`.
    DoubleBiasedAdd,
}

impl DetailFunction {
    fn from_raw(raw: i16) -> Self {
        match raw {
            1 => Self::Multiply,
            2 => Self::DoubleBiasedAdd,
            _ => Self::DoubleBiasedMultiply,
        }
    }
}

/// `_shader_environment_type_*`: which alpha mixes the two detail maps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnvironmentType {
    /// The secondary detail map's alpha mixes towards the primary.
    Normal,
    /// The base map's alpha mixes towards the primary.
    Blended,
    /// As blended; the base map's alpha is also the specular mask.
    BlendedBaseSpecular,
}

/// A detail map and how many times it repeats across the base map.
#[derive(Debug, Clone, Copy)]
pub struct DetailMap {
    pub bitmap: TagRef,
    pub scale: f32,
}

/// How a `shader_environment` builds its diffuse colour, as the Xbox's
/// pixel shader combines it (`rasterizer_xbox_environment.c`): base map
/// (t0), primary and secondary detail maps (t1, t2) mixed by an alpha, then
/// the micro detail map (t3).
#[derive(Debug, Clone, Copy)]
pub struct EnvironmentDiffuse {
    pub kind: EnvironmentType,
    pub detail_function: DetailFunction,
    pub primary_detail: Option<DetailMap>,
    pub secondary_detail: Option<DetailMap>,
    pub micro_detail_function: DetailFunction,
    pub micro_detail: Option<DetailMap>,
    /// Detail scales are relative to the base map's size over the detail
    /// map's (`_shader_environment_diffuse_rescale_detail_maps_bit`).
    pub rescale_detail_maps: bool,
    /// A palettized normal map whose alpha is also the cut-out of
    /// alpha-tested shaders (the renderer alpha-tests the bump map's pass).
    pub bump_map: Option<DetailMap>,
    /// The bump scale is relative to the base map's size over the bump
    /// map's (`_shader_environment_diffuse_rescale_bump_map_bit`).
    pub rescale_bump_map: bool,
}

/// Where a shader keeps its specular (shininess) mask.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpecularMask {
    /// Uniform shininess.
    None,
    /// The base map's alpha (`shader_environment` normal and blended base
    /// specular types; blended types use it to mix detail maps instead).
    BaseAlpha,
    /// The multipurpose map's red channel (`shader_model`, Xbox order).
    MultipurposeRed,
}

/// How shiny a surface is: Halo's specular and cube-map reflection settings.
#[derive(Debug, Clone, Copy)]
pub struct Reflection {
    /// Reflection brightness seen head-on and at grazing angles.
    pub perpendicular_brightness: f32,
    pub parallel_brightness: f32,
    /// The reflection's tint (model shaders) or specular colour
    /// (environment shaders) seen head-on.
    pub tint: [f32; 3],
    pub cube_map: Option<TagRef>,
    pub mask: SpecularMask,
}

/// Light a surface gives off by itself, beyond its radiosity emission.
#[derive(Debug, Clone, Copy)]
pub enum SelfIllumination {
    /// `shader_model`: the multipurpose map's green channel (Xbox order:
    /// specular, self-illumination, colour change, auxiliary) masks the base
    /// map, tinted by a colour animated between two bounds.
    Model { multipurpose_map: TagRef, color_lower: [f32; 3], color_upper: [f32; 3] },
    /// `shader_environment`: the self-illumination map's red, green and blue
    /// channels weight the primary, secondary and plasma "on" colours.
    Environment { map: TagRef, primary: [f32; 3], secondary: [f32; 3], plasma: [f32; 3] },
}

/// What the renderer needs from a shader so far.
#[derive(Debug, Clone)]
pub struct ShaderInfo {
    pub name: String,
    pub group: u32,
    pub datum: u32,
    pub shader_type: ShaderType,
    /// Light the surface emits (radiosity `color_of_emitted_light` × `power`,
    /// as the lightmapper used it); zero for most shaders.
    pub emitted_light: [f32; 3],
    pub emitted_power: f32,
    pub base_map: Option<TagRef>,
    pub detail_maps: Vec<(TagRef, f32)>,
    pub bump_map: Option<TagRef>,
    pub self_illumination_map: Option<TagRef>,
    /// `shader_environment` type (normal, blended, specular-mask-in-alpha).
    pub environment_type: Option<i16>,
    /// `shader_environment` alpha-tested flag.
    pub alpha_tested: bool,
    pub uv_scale: [f32; 2],
    /// For transparent generic and chicago shaders.
    pub transparent: Option<TransparentInfo>,
    pub self_illumination: Option<SelfIllumination>,
    /// `shader_model`'s multipurpose map (see [`SelfIllumination::Model`]).
    pub multipurpose_map: Option<TagRef>,
    /// `shader_environment`'s diffuse layers.
    pub environment: Option<EnvironmentDiffuse>,
    pub reflection: Option<Reflection>,
}

fn nonzero(v: f32) -> f32 {
    if v == 0.0 { 1.0 } else { v }
}

fn present(reference: TagRef) -> Option<TagRef> {
    reference.datum.map(|_| reference)
}

pub fn shader_info(cache: &CacheFile, reference: TagRef) -> Result<Option<ShaderInfo>> {
    let Some(datum) = reference.datum else { return Ok(None) };
    let tag = cache.tag(datum)?;
    let root = cache.tag_root(datum, groups::SHADER)?;
    let radiosity = layout::shader_base::RADIOSITY;
    let shader_type = ShaderType::from_raw(root.i16(layout::shader_base::TYPE)?);

    let mut info = ShaderInfo {
        name: tag.name.clone(),
        group: tag.group,
        datum,
        shader_type,
        emitted_light: root.vec3(radiosity + layout::shader_radiosity_properties::COLOR_OF_EMITTED_LIGHT)?,
        emitted_power: root.f32(radiosity + layout::shader_radiosity_properties::POWER)?,
        base_map: None,
        detail_maps: Vec::new(),
        bump_map: None,
        self_illumination_map: None,
        environment_type: None,
        alpha_tested: false,
        uv_scale: [1.0, 1.0],
        transparent: None,
        self_illumination: None,
        multipurpose_map: None,
        environment: None,
        reflection: None,
    };

    if tag.group == groups::SHADER_ENVIRONMENT {
        use layout::shader_environment_definition as e;
        info.environment_type = Some(root.i16(e::TYPE)?);
        // shader_environment flags: bit 0 alpha-tested
        info.alpha_tested = root.u16(e::FLAGS)? & 1 != 0;
        info.base_map = present(root.reference(e::BASE_MAP)?);
        for (map, scale) in [
            (e::PRIMARY_DETAIL_MAP, e::PRIMARY_DETAIL_MAP_SCALE),
            (e::SECONDARY_DETAIL_MAP, e::SECONDARY_DETAIL_MAP_SCALE),
        ] {
            if let Some(r) = present(root.reference(map)?) {
                info.detail_maps.push((r, root.f32(scale)?));
            }
        }
        info.bump_map = present(root.reference(e::BUMP_MAP)?);
        let detail = |map: usize, scale: usize| -> Result<Option<DetailMap>> {
            Ok(present(root.reference(map)?).map(|bitmap| DetailMap { bitmap, scale: nonzero(root.f32(scale).unwrap_or(1.0)) }))
        };
        info.environment = Some(EnvironmentDiffuse {
            kind: match root.i16(e::TYPE)? {
                1 => EnvironmentType::Blended,
                2 => EnvironmentType::BlendedBaseSpecular,
                _ => EnvironmentType::Normal,
            },
            detail_function: DetailFunction::from_raw(root.i16(e::DETAIL_MAP_FUNCTION)?),
            primary_detail: detail(e::PRIMARY_DETAIL_MAP, e::PRIMARY_DETAIL_MAP_SCALE)?,
            secondary_detail: detail(e::SECONDARY_DETAIL_MAP, e::SECONDARY_DETAIL_MAP_SCALE)?,
            micro_detail_function: DetailFunction::from_raw(root.i16(e::MICRO_DETAIL_MAP_FUNCTION)?),
            micro_detail: detail(e::MICRO_DETAIL_MAP, e::MICRO_DETAIL_MAP_SCALE)?,
            rescale_detail_maps: root.u16(e::DIFFUSE_FLAGS)? & 1 != 0,
            rescale_bump_map: root.u16(e::DIFFUSE_FLAGS)? & 2 != 0,
            bump_map: detail(e::BUMP_MAP, e::BUMP_MAP_SCALE)?,
        });
        {
            use layout::{
                shader_environment_properties as ep, shader_environment_reflection_properties as r,
                shader_environment_specular_properties as sp,
            };
            // shader_environment_definition is `struct shader` then the
            // environment properties (rasterizer_xbox_environment.c)
            let props = layout::shader_base::TYPE + 4;
            let reflection = props + ep::REFLECTION;
            let specular = props + ep::SPECULAR;
            let kind = info.environment.as_ref().map(|e| e.kind);
            info.reflection = Some(Reflection {
                perpendicular_brightness: root.f32(reflection + r::VIEW_PERPENDICULAR_BRIGHTNESS)?,
                parallel_brightness: root.f32(reflection + r::VIEW_PARALLEL_BRIGHTNESS)?,
                tint: root.vec3(specular + sp::VIEW_PERPENDICULAR_COLOR)?,
                cube_map: present(root.reference(reflection + r::CUBE_MAP)?),
                mask: match kind {
                    Some(EnvironmentType::Blended) => SpecularMask::None,
                    _ => SpecularMask::BaseAlpha,
                },
            });
        }
        info.self_illumination_map = present(root.reference(e::SELF_ILLUMINATION_MAP)?);
        let colors = [root.vec3(e::PRIMARY_ON_COLOR)?, root.vec3(e::SECONDARY_ON_COLOR)?, root.vec3(e::PLASMA_ON_COLOR)?];
        if let Some(map) = info.self_illumination_map
            && colors.iter().flatten().any(|&c| c > 0.0)
        {
            info.self_illumination = Some(SelfIllumination::Environment {
                map,
                primary: colors[0],
                secondary: colors[1],
                plasma: colors[2],
            });
        }
    } else if tag.group == groups::SHADER_MODEL {
        use layout::shader_model_definition as m;
        info.base_map = present(root.reference(m::BASE_MAP)?);
        info.multipurpose_map = present(root.reference(m::MULTIPURPOSE_MAP)?);
        let (lower, upper) =
            (root.vec3(m::SELF_ILLUMINATION_COLOR_LOWER_BOUND)?, root.vec3(m::SELF_ILLUMINATION_COLOR_UPPER_BOUND)?);
        if let Some(multipurpose_map) = info.multipurpose_map
            && lower.iter().chain(&upper).any(|&c| c > 0.0)
        {
            info.self_illumination =
                Some(SelfIllumination::Model { multipurpose_map, color_lower: lower, color_upper: upper });
        }
        info.reflection = Some(Reflection {
            perpendicular_brightness: root.f32(m::PERPENDICULAR_BRIGHTNESS)?,
            parallel_brightness: root.f32(m::PARALLEL_BRIGHTNESS)?,
            tint: root.vec3(m::PERPENDICULAR_TINT_COLOR)?,
            cube_map: present(root.reference(m::REFLECTION_CUBE_MAP)?),
            mask: if info.multipurpose_map.is_some() { SpecularMask::MultipurposeRed } else { SpecularMask::None },
        });
        let (u, v) = (root.f32(m::MAP_U_SCALE)?, root.f32(m::MAP_V_SCALE)?);
        info.uv_scale = [if u == 0.0 { 1.0 } else { u }, if v == 0.0 { 1.0 } else { v }];
    }

    if matches!(shader_type, ShaderType::TransparentGeneric | ShaderType::TransparentChicago) {
        use layout::shader_transparent as t;
        let (base, map_size, map, scale_u, scale_v, offset_u, offset_v) = if shader_type == ShaderType::TransparentGeneric {
            use layout::shader_transparent_generic_map as m;
            (
                layout::shader_transparent_generic_definition::TRANSPARENT,
                m::SIZEOF,
                m::MAP,
                m::MAP_U_SCALE,
                m::MAP_V_SCALE,
                m::MAP_U_OFFSET,
                m::MAP_V_OFFSET,
            )
        } else {
            use layout::shader_transparent_chicago_map as m;
            (
                layout::shader_transparent_chicago_definition::TRANSPARENT,
                m::SIZEOF,
                m::MAP,
                m::MAP_U_SCALE,
                m::MAP_V_SCALE,
                m::MAP_U_OFFSET,
                m::MAP_V_OFFSET,
            )
        };
        let maps = root
            .block(base + t::MAPS, map_size)?
            .iter()
            .map(|m| {
                let read = || -> Result<Option<TransparentMap>> {
                    let Some(bitmap) = present(m.reference(map)?) else { return Ok(None) };
                    let (u, v) = (m.f32(scale_u)?, m.f32(scale_v)?);
                    Ok(Some(TransparentMap {
                        bitmap,
                        uv_scale: [if u == 0.0 { 1.0 } else { u }, if v == 0.0 { 1.0 } else { v }],
                        uv_offset: [m.f32(offset_u)?, m.f32(offset_v)?],
                    }))
                };
                read()
            })
            .collect::<Result<Vec<_>>>()?;
        info.transparent = Some(TransparentInfo {
            blend: BlendFunction::from_raw(root.i16(base + t::FRAMEBUFFER_BLEND_FUNCTION)?),
            first_map_type: root.i16(base + t::FIRST_MAP_TYPE)?,
            maps,
        });
    }
    Ok(Some(info))
}
