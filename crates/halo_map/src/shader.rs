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
    pub maps: Vec<TransparentMap>,
}

/// What the renderer needs from a shader so far.
#[derive(Debug, Clone)]
pub struct ShaderInfo {
    pub name: String,
    pub group: u32,
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
        info.self_illumination_map = present(root.reference(e::SELF_ILLUMINATION_MAP)?);
    } else if tag.group == groups::SHADER_MODEL {
        use layout::shader_model_definition as m;
        info.base_map = present(root.reference(m::BASE_MAP)?);
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
            .filter_map(|m| {
                let read = || -> Result<Option<TransparentMap>> {
                    let Some(bitmap) = present(m.reference(map)?) else { return Ok(None) };
                    let (u, v) = (m.f32(scale_u)?, m.f32(scale_v)?);
                    Ok(Some(TransparentMap {
                        bitmap,
                        uv_scale: [if u == 0.0 { 1.0 } else { u }, if v == 0.0 { 1.0 } else { v }],
                        uv_offset: [m.f32(offset_u)?, m.f32(offset_v)?],
                    }))
                };
                read().transpose()
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
