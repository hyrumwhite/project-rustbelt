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
    /// degrees
    pub rotation: f32,
    /// `_shader_transparent_map_flag_*` (chicago: bit 0 unfiltered, 1 alpha
    /// replicate, 2 u clamped, 3 v clamped; generic: 0 unfiltered, 1 u
    /// clamped, 2 v clamped)
    pub flags: u16,
    pub animation: MapAnimation,
}

/// One of a `shader_texture_animation`'s channels (u, v or rotation).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct AnimationChannel {
    /// 0 none (1), else an object function (A-D: 1-4)
    pub source: i16,
    /// a periodic function
    pub function: i16,
    pub period: f32,
    pub phase: f32,
    pub scale: f32,
}

/// `shader_texture_animation`: a map's offset (and rotation, about a
/// centre) moving with time and the object's functions.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct MapAnimation {
    pub u: AnimationChannel,
    pub v: AnimationChannel,
    pub r: AnimationChannel,
    pub r_center: [f32; 2],
}

impl MapAnimation {
    fn read(p: crate::Ptr<'_>, at: usize) -> Result<Self> {
        let channel = |o: usize| -> Result<AnimationChannel> {
            Ok(AnimationChannel {
                source: p.i16(at + o)?,
                function: p.i16(at + o + 2)?,
                period: p.f32(at + o + 4)?,
                phase: p.f32(at + o + 8)?,
                scale: p.f32(at + o + 0xc)?,
            })
        };
        Ok(Self { u: channel(0)?, v: channel(0x10)?, r: channel(0x20)?, r_center: [p.f32(at + 0x30)?, p.f32(at + 0x34)?] })
    }

    /// Whether the transform changes over time or with the object (the
    /// periodic function `one` with no source is a constant).
    pub fn animated(&self) -> bool {
        [self.u, self.v, self.r].iter().any(|c| c.source != 0 || c.function != PERIODIC_ONE)
    }

    /// Whether an object function drives it.
    pub fn uses_object(&self) -> bool {
        [self.u, self.v, self.r].iter().any(|c| c.source != 0)
    }

    /// `shader_texture_animation_evaluate`: the map's texture coordinate
    /// transform, u' = u_row . (u, v, 0, 1) and v' likewise, for a map of this
    /// scale, offset and rotation (degrees) at `time` (seconds), with the
    /// object's function values (`None`: each source 1, as for the level).
    #[allow(clippy::too_many_arguments)]
    pub fn evaluate(
        &self,
        values: Option<&[f32; 4]>,
        scale: [f32; 2],
        offset: [f32; 2],
        rotation: f32,
        time: f32,
        periodic: impl Fn(i16, f32) -> f32,
    ) -> ([f32; 4], [f32; 4]) {
        let input = |c: &AnimationChannel| {
            let source = match values {
                Some(values) if c.source > 0 => values.get(c.source as usize - 1).copied().unwrap_or(1.0),
                _ => 1.0,
            };
            let period = if c.period == 0.0 { 1.0 } else { c.period };
            periodic(c.function, (c.phase + time) / period) * c.scale * source
        };
        let u = offset[0] - self.r_center[0] + input(&self.u);
        let v = offset[1] - self.r_center[1] + input(&self.v);
        let angle = input(&self.r) + rotation;
        let (sine, cosine) = if angle != 0.0 { angle.to_radians().sin_cos() } else { (0.0, 1.0) };
        (
            [cosine * scale[0], -(sine * scale[1]), 0.0, cosine * u - sine * v + self.r_center[0]],
            [sine * scale[0], cosine * scale[1], 0.0, cosine * v + sine * u + self.r_center[1]],
        )
    }
}

/// `_periodic_function_one`.
pub const PERIODIC_ONE: i16 = 0;

/// A generic shader stage's constant colour 0, moving between its two
/// colours (`shader_transparent_generic_stage`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StageColorAnimation {
    /// bit 3: the object's function A drives it instead of time
    pub flags: u16,
    /// 0 none, else an object's change colour (A-D: 1-4) to multiply by
    pub source: i16,
    pub function: i16,
    pub period: f32,
    /// rgba
    pub color0: [f32; 4],
    pub color1: [f32; 4],
}

impl StageColorAnimation {
    /// `_shader_transparent_generic_stage_flag_a_out_controls_color0_animation_bit`
    pub const A_OUT_CONTROLS_COLOR0: u16 = 1 << 3;

    pub fn animated(&self) -> bool {
        self.color0 != self.color1 || self.source != 0
    }

    pub fn uses_object(&self) -> bool {
        self.source != 0 || (self.flags & Self::A_OUT_CONTROLS_COLOR0 != 0 && self.color0 != self.color1)
    }

    /// The stage's constant colour 0 (rgba) at `time`, with the object's
    /// function values and change colours (`None` for the level's).
    pub fn evaluate(&self, values: Option<&[f32; 4]>, colors: Option<&[[f32; 3]; 4]>, time: f32, periodic: impl Fn(i16, f32) -> f32) -> [f32; 4] {
        let fraction = match values {
            Some(values) if self.flags & Self::A_OUT_CONTROLS_COLOR0 != 0 => values[0],
            _ => periodic(self.function, time / if self.period == 0.0 { 1.0 } else { self.period }),
        };
        let mut color: [f32; 4] = std::array::from_fn(|c| self.color0[c] + (self.color1[c] - self.color0[c]) * fraction);
        if let (Some(colors), 1..=4) = (colors, self.source) {
            for c in 0..3 {
                color[c] *= colors[self.source as usize - 1][c].clamp(0.0, 1.0);
            }
        }
        color
    }
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
    /// `_shader_transparent_flag_*`: bit 0 alpha tested, 1 decal, 2 two
    /// sided, 3 first map in screen space, 4 draw before water, 5 ignore
    /// effect, 6 scale first map with distance, 7 numeric
    pub flags: u8,
    pub numeric_counter_limit: u8,
    /// 0 none, 1 fade when perpendicular, 2 fade when parallel
    pub fade_mode: i16,
    /// 0 none, else an object function (A-D: 1-4) scaling the fade
    pub fade_source: i16,
    /// Shaders drawn first, over the same surface.
    pub extra_layers: Vec<TagRef>,
    /// chicago's: bit 0 don't fade active camouflage, 1 numeric countdown
    /// timer
    pub extra_flags: u16,
    /// generic's stages' constant colour 0, in stage order
    pub stage_colors: Vec<StageColorAnimation>,
}

impl TransparentInfo {
    pub const ALPHA_TESTED: u8 = 1 << 0;
    pub const DECAL: u8 = 1 << 1;
    pub const TWO_SIDED: u8 = 1 << 2;
    pub const FIRST_MAP_IN_SCREEN_SPACE: u8 = 1 << 3;
    pub const SCALE_FIRST_MAP_WITH_DISTANCE: u8 = 1 << 6;
    pub const NUMERIC: u8 = 1 << 7;

    /// Whether anything changes over time (map transforms, colours).
    pub fn animated(&self) -> bool {
        self.maps.iter().flatten().any(|m| m.animation.animated()) || self.stage_colors.iter().any(|s| s.animated())
    }

    /// Whether the object's functions or colours change it (a copy of its
    /// material for each object).
    pub fn uses_object(&self) -> bool {
        self.maps.iter().flatten().any(|m| m.animation.uses_object())
            || self.stage_colors.iter().any(|s| s.uses_object())
            || self.fade_source != 0
            || self.flags & Self::NUMERIC != 0
    }
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

/// `shader_transparent_glass`: a tinted, reflective see-through surface.
#[derive(Debug, Clone, Copy)]
pub struct Glass {
    pub tint: [f32; 3],
    pub tint_map: Option<TagRef>,
    pub diffuse_map: Option<TagRef>,
    /// Reflection colour seen head-on and at grazing angles (RGBA).
    pub reflection_perpendicular: [f32; 4],
    pub reflection_parallel: [f32; 4],
}

/// `shader_transparent_water`.
#[derive(Debug, Clone, Copy)]
pub struct Water {
    pub base_map: Option<TagRef>,
    /// Tint seen head-on and at grazing angles (RGBA).
    pub perpendicular_tint: [f32; 4],
    pub parallel_tint: [f32; 4],
}

fn argb(p: crate::Ptr<'_>, offset: usize) -> Result<[f32; 4]> {
    // real_argb_color: alpha, red, green, blue
    Ok([p.f32(offset + 4)?, p.f32(offset + 8)?, p.f32(offset + 12)?, p.f32(offset)?])
}

/// Light a surface gives off by itself, beyond its radiosity emission.
#[derive(Debug, Clone, Copy)]
pub enum SelfIllumination {
    /// `shader_model`: the multipurpose map's green channel (Xbox order:
    /// specular, self-illumination, colour change, auxiliary) masks the base
    /// map, tinted by a colour animated between two bounds.
    Model { multipurpose_map: TagRef, color_lower: [f32; 3], color_upper: [f32; 3] },
    /// `shader_environment`: the self-illumination map's red, green and blue
    /// channels weight the primary, secondary and plasma "on" colours (see
    /// [`EnvironmentSelfIlluminationAnimation`] for how they animate).
    Environment { map: TagRef, primary: [f32; 3], secondary: [f32; 3], plasma: [f32; 3] },
}

/// How a `shader_environment`'s self-illumination animates (its lightmap
/// pass): the primary and secondary colours between their "off" and "on"
/// colours by their periodic functions; the plasma, its "off" colour with
/// its "on" colour where the map's alpha is near its function's value. Each
/// function of (time + phase) / period; in the order primary, secondary,
/// plasma.
#[derive(Debug, Clone, Copy, Default)]
pub struct EnvironmentSelfIlluminationAnimation {
    pub on_colors: [[f32; 3]; 3],
    pub off_colors: [[f32; 3]; 3],
    pub functions: [i16; 3],
    pub periods: [f32; 3],
    pub phases: [f32; 3],
}

/// How a `shader_model`'s self-illumination animates (its pass in
/// rasterizer_xbox_models.c): its colour between the lower and upper bounds
/// by its periodic function of time / period + phase, the phase each
/// object's own (random from its object index) unless flagged, then times
/// one of the object's change colours (`color_source`, 0-3 for A-D).
#[derive(Debug, Clone, Copy, Default)]
pub struct ModelSelfIlluminationAnimation {
    pub lower: [f32; 3],
    pub upper: [f32; 3],
    pub function: i16,
    pub period: f32,
    pub random_phase: bool,
    pub color_source: Option<usize>,
}

/// A `shader_model`'s detail map (rasterizer_xbox_models.c's
/// `set_environment_shader_pixel_shader`): pulled toward neutral (0.5 for the
/// biased functions, 1 for multiply) where its mask says, then combined with
/// the base map by its function (0 double biased multiply, 1 multiply, 2
/// double biased add). The mask (0 none) is the multipurpose map's
/// reflection (red), self-illumination (green) or colour change (blue)
/// channel, or its alpha, each inverted first (odd) or not (even): 1 and 2
/// red, 3 and 4 green, 5 and 6 blue, 7 and 8 alpha. Its scale (u, v) is
/// against the model's own texture coordinates.
#[derive(Debug, Clone, Copy)]
pub struct ModelDetail {
    pub map: TagRef,
    pub function: i16,
    pub mask: i16,
    pub scale: [f32; 2],
}

/// A `shader_environment`'s texture animation
/// (`shader_environment_texture_animation_evaluate`): each texture
/// coordinate offset by its periodic function of time / period, times its
/// scale.
#[derive(Debug, Clone, Copy, Default)]
pub struct TextureAnimation {
    pub functions: [i16; 2],
    pub periods: [f32; 2],
    pub scales: [f32; 2],
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
    /// Alpha-tested: an environment shader by its flag (the bump map's alpha
    /// cuts it out), a model shader unless flagged otherwise (the base map's).
    pub alpha_tested: bool,
    /// Drawn without back-face culling (a model shader's two-sided flags).
    pub two_sided: bool,
    pub uv_scale: [f32; 2],
    /// For transparent generic and chicago shaders.
    pub transparent: Option<TransparentInfo>,
    pub self_illumination: Option<SelfIllumination>,
    /// `shader_model`'s multipurpose map (see [`SelfIllumination::Model`]).
    pub multipurpose_map: Option<TagRef>,
    /// `shader_model`: which of the object's change colours (0-3 for A-D)
    /// tints the base map, where the multipurpose map's alpha says.
    pub change_color: Option<usize>,
    /// `shader_environment`'s diffuse layers.
    pub environment: Option<EnvironmentDiffuse>,
    pub reflection: Option<Reflection>,
    pub glass: Option<Glass>,
    pub water: Option<Water>,
    /// `shader_environment`'s self-illumination animation (with a map).
    pub self_illumination_animation: Option<EnvironmentSelfIlluminationAnimation>,
    /// `shader_model`'s detail map.
    pub model_detail: Option<ModelDetail>,
    /// `shader_model`'s self-illumination animation (with a multipurpose
    /// map).
    pub model_self_illumination_animation: Option<ModelSelfIlluminationAnimation>,
    /// `shader_environment`'s texture animation (when it moves).
    pub texture_animation: Option<TextureAnimation>,
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
        two_sided: false,
        uv_scale: [1.0, 1.0],
        transparent: None,
        self_illumination: None,
        multipurpose_map: None,
        change_color: None,
        environment: None,
        reflection: None,
        glass: None,
        water: None,
        self_illumination_animation: None,
        model_self_illumination_animation: None,
        model_detail: None,
        texture_animation: None,
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
            info.self_illumination_animation = Some(EnvironmentSelfIlluminationAnimation {
                on_colors: colors,
                off_colors: [root.vec3(e::PRIMARY_OFF_COLOR)?, root.vec3(e::SECONDARY_OFF_COLOR)?, root.vec3(e::PLASMA_OFF_COLOR)?],
                functions: [
                    root.i16(e::PRIMARY_ANIMATION_FUNCTION)?,
                    root.i16(e::SECONDARY_ANIMATION_FUNCTION)?,
                    root.i16(e::PLASMA_ANIMATION_FUNCTION)?,
                ],
                periods: [
                    nonzero(root.f32(e::PRIMARY_ANIMATION_PERIOD)?),
                    nonzero(root.f32(e::SECONDARY_ANIMATION_PERIOD)?),
                    nonzero(root.f32(e::PLASMA_ANIMATION_PERIOD)?),
                ],
                phases: [
                    root.f32(e::PRIMARY_ANIMATION_PHASE)?,
                    root.f32(e::SECONDARY_ANIMATION_PHASE)?,
                    root.f32(e::PLASMA_ANIMATION_PHASE)?,
                ],
            });
        }
        let texture_animation = TextureAnimation {
            functions: [root.i16(e::U_ANIMATION_FUNCTION)?, root.i16(e::V_ANIMATION_FUNCTION)?],
            periods: [nonzero(root.f32(e::U_ANIMATION_PERIOD)?), nonzero(root.f32(e::V_ANIMATION_PERIOD)?)],
            scales: [root.f32(e::U_ANIMATION_SCALE)?, root.f32(e::V_ANIMATION_SCALE)?],
        };
        if texture_animation.scales.iter().any(|&s| s != 0.0) {
            info.texture_animation = Some(texture_animation);
        }
    } else if tag.group == groups::SHADER_MODEL {
        use layout::shader_model_definition as m;
        info.base_map = present(root.reference(m::BASE_MAP)?);
        // shader_model flags (rasterizer_xbox_models.c): bit 1 two-sided, 2
        // not alpha-tested, 3 alpha-blended decal, 5 two-sided without
        // culling; the base map's alpha cuts the rest out
        let flags = root.u16(m::FLAGS)?;
        info.alpha_tested = flags & (1 << 2 | 1 << 3) == 0;
        info.two_sided = flags & (1 << 1 | 1 << 5) != 0;
        info.multipurpose_map = present(root.reference(m::MULTIPURPOSE_MAP)?);
        // (0 none, 1-4 A-D)
        let source = root.i16(m::CHANGE_COLOR_SOURCE)?;
        info.change_color = (1..=4).contains(&source).then(|| source as usize - 1);
        if let Some(map) = present(root.reference(m::DETAIL_MAP)?) {
            let scale = nonzero(root.f32(m::DETAIL_MAP_SCALE)?);
            info.model_detail = Some(ModelDetail {
                map,
                function: root.i16(m::DETAIL_FUNCTION)?.clamp(0, 2),
                mask: root.i16(m::DETAIL_MASK)?.clamp(0, 8),
                scale: [scale, scale * nonzero(root.f32(m::DETAIL_MAP_V_SCALE)?)],
            });
        }
        let (lower, upper) =
            (root.vec3(m::SELF_ILLUMINATION_COLOR_LOWER_BOUND)?, root.vec3(m::SELF_ILLUMINATION_COLOR_UPPER_BOUND)?);
        if let Some(multipurpose_map) = info.multipurpose_map
            && lower.iter().chain(&upper).any(|&c| c > 0.0)
        {
            info.self_illumination =
                Some(SelfIllumination::Model { multipurpose_map, color_lower: lower, color_upper: upper });
            let color_source = root.i16(m::SELF_ILLUMINATION_COLOR_SOURCE)?;
            info.model_self_illumination_animation = Some(ModelSelfIlluminationAnimation {
                lower,
                upper,
                function: root.i16(m::SELF_ILLUMINATION_ANIMATION_FUNCTION)?,
                period: nonzero(root.f32(m::SELF_ILLUMINATION_ANIMATION_PERIOD)?),
                // (flag 0: no random phase)
                random_phase: root.u16(m::SELF_ILLUMINATION_FLAGS)? & 1 == 0,
                color_source: (1..=4).contains(&color_source).then(|| color_source as usize - 1),
            });
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

    if shader_type == ShaderType::Glass {
        use layout::shader_transparent_glass_definition as g;
        info.glass = Some(Glass {
            tint: root.vec3(g::TINT_COLOR)?,
            tint_map: present(root.reference(g::TINT_MAP)?),
            diffuse_map: present(root.reference(g::DIFFUSE_MAP)?),
            reflection_perpendicular: argb(root, g::REFLECTION_VIEW_PERPENDICULAR_COLOR)?,
            reflection_parallel: argb(root, g::REFLECTION_VIEW_PARALLEL_COLOR)?,
        });
    }
    if shader_type == ShaderType::Water {
        use layout::shader_transparent_water_definition as w;
        info.water = Some(Water {
            base_map: present(root.reference(w::BASE_MAP)?),
            perpendicular_tint: argb(root, w::VIEW_PERPENDICULAR_TINT_COLOR)?,
            parallel_tint: argb(root, w::VIEW_PARALLEL_TINT_COLOR)?,
        });
    }
    if matches!(shader_type, ShaderType::TransparentGeneric | ShaderType::TransparentChicago) {
        use layout::shader_transparent as t;
        let generic = shader_type == ShaderType::TransparentGeneric;
        let (base, map_size, map, scale_u, scale_v, offset_u, offset_v, rotation, animation) = if generic {
            use layout::shader_transparent_generic_map as m;
            (
                layout::shader_transparent_generic_definition::TRANSPARENT,
                m::SIZEOF,
                m::MAP,
                m::MAP_U_SCALE,
                m::MAP_V_SCALE,
                m::MAP_U_OFFSET,
                m::MAP_V_OFFSET,
                m::MAP_ROTATION,
                m::MAP_ANIMATION,
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
                m::MAP_ROTATION,
                m::MAP_ANIMATION,
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
                        rotation: m.f32(rotation)?,
                        flags: m.u16(0)?,
                        animation: MapAnimation::read(m, animation)?,
                    }))
                };
                read()
            })
            .collect::<Result<Vec<_>>>()?;
        let extra_layers = root.block(base + t::EXTRA_LAYERS, 16)?.iter().filter_map(|l| l.reference(0).ok().and_then(present)).collect();
        let mut stage_colors = Vec::new();
        if generic {
            use layout::shader_transparent_generic_stage as s;
            for stage in root.block(layout::shader_transparent_generic_definition::STAGES, s::SIZEOF)?.iter() {
                stage_colors.push(StageColorAnimation {
                    flags: stage.u16(s::FLAGS)?,
                    source: stage.i16(s::CONSTANT_COLOR0_ANIMATION_SOURCE)?,
                    function: stage.i16(s::CONSTANT_COLOR0_ANIMATION_FUNCTION)?,
                    period: stage.f32(s::CONSTANT_COLOR0_ANIMATION_PERIOD)?,
                    color0: argb(stage, s::CONSTANT_COLOR0_LOWER_BOUND)?.map(|c| c.clamp(0.0, 1.0)),
                    color1: argb(stage, s::CONSTANT_COLOR0_UPPER_BOUND)?.map(|c| c.clamp(0.0, 1.0)),
                });
            }
        }
        info.transparent = Some(TransparentInfo {
            blend: BlendFunction::from_raw(root.i16(base + t::FRAMEBUFFER_BLEND_FUNCTION)?),
            first_map_type: root.i16(base + t::FIRST_MAP_TYPE)?,
            maps,
            flags: root.u8(base + t::FLAGS)?,
            numeric_counter_limit: root.u8(base + t::NUMERIC_COUNTER_LIMIT)?,
            fade_mode: root.i16(base + t::FRAMEBUFFER_FADE_MODE)?,
            fade_source: root.i16(base + t::FRAMEBUFFER_FADE_SOURCE)?,
            extra_layers,
            extra_flags: if generic { 0 } else { root.u16(base + t::CHICAGO_EXTRA_FLAGS)? },
            stage_colors,
        });
    }
    Ok(Some(info))
}
