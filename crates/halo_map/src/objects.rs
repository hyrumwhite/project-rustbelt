//! Objects the scenario places: scenery, units, items and devices.
//!
//! Each object block has a palette of object definition tags; a placement
//! names its palette entry, where it is, how it is turned, which BSPs it is
//! in (`on_bsp_flags`) and, for most kinds, which permutation of each model
//! region to show. The game creates a placement when its BSP loads unless it
//! is flagged not automatic (`objects_place`, object_types.c).

use crate::{layout, math, CacheFile, Result, TagRef};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ObjectKind {
    Scenery,
    Biped,
    Vehicle,
    Equipment,
    Weapon,
    Machine,
    Control,
    LightFixture,
}

impl ObjectKind {
    pub const ALL: [ObjectKind; 8] = [
        Self::Scenery,
        Self::Biped,
        Self::Vehicle,
        Self::Equipment,
        Self::Weapon,
        Self::Machine,
        Self::Control,
        Self::LightFixture,
    ];

    /// (placement block, palette block, element size, permutation offset)
    fn blocks(self) -> (usize, usize, usize, Option<usize>) {
        use layout::scenario as s;
        match self {
            Self::Scenery => (
                s::SCENERY,
                s::SCENERY_PALETTE,
                layout::scenery_placement::SIZEOF,
                Some(layout::scenery_placement::PERMUTATION),
            ),
            Self::Biped => {
                (s::BIPEDS, s::BIPED_PALETTE, layout::unit_placement::SIZEOF, Some(layout::unit_placement::PERMUTATION))
            }
            Self::Vehicle => (
                s::VEHICLES,
                s::VEHICLE_PALETTE,
                layout::unit_placement::SIZEOF,
                Some(layout::unit_placement::PERMUTATION),
            ),
            Self::Equipment => (s::EQUIPMENT, s::EQUIPMENT_PALETTE, layout::equipment_placement::SIZEOF, None),
            Self::Weapon => (
                s::WEAPONS,
                s::WEAPON_PALETTE,
                layout::scenario_weapon_datum::SIZEOF,
                Some(layout::scenario_weapon_datum::PERMUTATION),
            ),
            Self::Machine => (s::MACHINES, s::MACHINE_PALETTE, layout::machine_placement::SIZEOF, None),
            Self::Control => (s::CONTROLS, s::CONTROL_PALETTE, layout::control_placement::SIZEOF, None),
            Self::LightFixture => {
                (s::LIGHT_FIXTURES, s::LIGHT_FIXTURES_PALETTE, layout::light_fixture_placement::SIZEOF, None)
            }
        }
    }
}

const PLACEMENT_NOT_AUTOMATIC: u16 = 1 << 0;

#[derive(Debug, Clone)]
pub struct Placement {
    pub kind: ObjectKind,
    /// The object definition tag (`scen`, `vehi`, ...).
    pub definition: TagRef,
    pub position: [f32; 3],
    /// Yaw, pitch, roll in radians.
    pub rotation: [f32; 3],
    /// Bit n set: the object is in BSP n.
    pub on_bsp_flags: u16,
    /// Only placed by scripts.
    pub not_automatic: bool,
    /// Permutation index per model region.
    pub region_permutations: [u8; 8],
    /// A light fixture's light: colour, intensity and angles, as the lightmapper
    /// used them.
    pub light: Option<FixtureLight>,
}

impl Placement {
    pub fn in_bsp(&self, bsp_index: usize) -> bool {
        bsp_index < 16 && self.on_bsp_flags & (1 << bsp_index) != 0
    }

    /// The object's axes in Halo coordinates: forward, left, up
    /// (`matrix4x3_rotation_from_angles`, matrix_math.c).
    pub fn axes(&self) -> [[f32; 3]; 3] {
        let [yaw, pitch, roll] = self.rotation;
        let (sy, cy) = yaw.sin_cos();
        let (sp, cp) = pitch.sin_cos();
        let (sr, cr) = roll.sin_cos();
        [
            [cy * cp, sy * cr - cy * sp * sr, cy * sp * cr + sy * sr],
            [-(sy * cp), sy * sp * sr + cy * cr, cy * sr - sy * sp * cr],
            [-sp, -(cp * sr), cp * cr],
        ]
    }

    /// The object's position in Bevy's convention (see [`math::halo_to_bevy`]).
    pub fn bevy_position(&self) -> [f32; 3] {
        math::halo_to_bevy(self.position, math::METERS_PER_WORLD_UNIT)
    }
}

/// Every object placement in the scenario.
pub fn placements(cache: &CacheFile) -> Result<Vec<Placement>> {
    use layout::{scenario_object_datum as d, scenario_object_palette_entry as pe};
    let scenario = cache.scenario()?;
    let mut out = Vec::new();
    for kind in ObjectKind::ALL {
        let (block, palette, size, permutation) = kind.blocks();
        let palette = scenario.block(palette, pe::SIZEOF)?;
        for element in scenario.block(block, size)?.iter() {
            let palette_index = element.i16(d::PALETTE_ENTRY_INDEX)?;
            let Some(entry) = (palette_index >= 0).then(|| palette.get(palette_index as usize)).flatten() else {
                continue;
            };
            let definition = entry.reference(pe::REFERENCE)?;
            if definition.datum.is_none() {
                continue;
            }
            let mut region_permutations = [0u8; 8];
            if let Some(offset) = permutation {
                let at = offset + layout::scenario_object_permutation::REGION_PERMUTATIONS;
                region_permutations.copy_from_slice(element.bytes(at, 8)?);
            }
            out.push(Placement {
                kind,
                definition,
                position: element.vec3(d::POSITION)?,
                rotation: element.vec3(d::ROTATION)?,
                on_bsp_flags: element.u16(d::ON_BSP_FLAGS)?,
                not_automatic: element.u16(d::PLACEMENT_FLAGS)? & PLACEMENT_NOT_AUTOMATIC != 0,
                region_permutations,
                light: if kind == ObjectKind::LightFixture {
                    use layout::scenario_light_fixture_datum as l;
                    Some(FixtureLight {
                        color: element.vec3(l::COLOR)?,
                        intensity: element.f32(l::INTENSITY)?,
                        falloff_angle: element.f32(l::FALLOFF_ANGLE)?,
                        cutoff_angle: element.f32(l::CUTOFF_ANGLE)?,
                    })
                } else {
                    None
                },
            });
        }
    }
    Ok(out)
}

/// A light fixture's light (device_light_fixtures.h's
/// `scenario_light_fixture_datum`): it shines along the fixture's forward
/// axis, fully within `falloff_angle` of it and fading out to
/// `cutoff_angle` (radians; pi or more for every way).
#[derive(Debug, Clone, Copy)]
pub struct FixtureLight {
    pub color: [f32; 3],
    pub intensity: f32,
    pub falloff_angle: f32,
    pub cutoff_angle: f32,
}

/// The model of an object definition tag (any object group).
/// An object definition's modifier shader (a shield's plasma), drawn over its
/// model with its functions and colours.
pub fn modifier_shader(cache: &CacheFile, definition: TagRef) -> Result<Option<TagRef>> {
    let Some(datum) = definition.datum else { return Ok(None) };
    let root = cache.tag_root(datum, crate::groups::OBJECT)?;
    let shader = root.reference(layout::object_definition::OBJECT_MODIFIER_SHADER)?;
    Ok(shader.datum.map(|_| shader))
}

/// Whether an object definition's transparent parts occlude one another
/// ("transparency self occludes", Cortana's): the game draws them all into
/// the depth buffer first, then each where it is nearest (render_objects.c's
/// modifier effect, rasterizer_xbox_transparent_geometry.c).
pub fn transparency_self_occludes(cache: &CacheFile, definition: TagRef) -> Result<bool> {
    let Some(datum) = definition.datum else { return Ok(false) };
    let root = cache.tag_root(datum, crate::groups::OBJECT)?;
    Ok(root.u16(layout::_object_definition::FLAGS)? & (1 << 1) != 0)
}

pub fn object_model(cache: &CacheFile, definition: TagRef) -> Result<Option<TagRef>> {
    let Some(datum) = definition.datum else { return Ok(None) };
    let root = cache.tag_root(datum, crate::groups::OBJECT)?;
    let model = root.reference(layout::_object_definition::MODEL)?;
    Ok(model.datum.map(|_| model))
}

/// The collision model of an object definition tag (any object group).
pub fn object_collision_model(cache: &CacheFile, definition: TagRef) -> Result<Option<TagRef>> {
    let Some(datum) = definition.datum else { return Ok(None) };
    let root = cache.tag_root(datum, crate::groups::OBJECT)?;
    let model = root.reference(layout::_object_definition::COLLISION_MODEL)?;
    Ok(model.datum.map(|_| model))
}
