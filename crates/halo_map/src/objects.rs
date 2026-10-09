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
    /// A light fixture's light: colour and intensity, as the lightmapper
    /// used them.
    pub light: Option<([f32; 3], f32)>,
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
                    Some((element.vec3(l::COLOR)?, element.f32(l::INTENSITY)?))
                } else {
                    None
                },
            });
        }
    }
    Ok(out)
}

/// The model of an object definition tag (any object group).
pub fn object_model(cache: &CacheFile, definition: TagRef) -> Result<Option<TagRef>> {
    let Some(datum) = definition.datum else { return Ok(None) };
    let root = cache.tag_root(datum, crate::groups::OBJECT)?;
    let model = root.reference(layout::_object_definition::MODEL)?;
    Ok(model.datum.map(|_| model))
}
