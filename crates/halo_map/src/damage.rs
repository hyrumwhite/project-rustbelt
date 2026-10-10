//! Halo's damage (ROADMAP phase 7, objects and physics): `object_cause_damage`
//! with `object_damage_shield` and `object_damage_body`, from OpenCE's
//! `source/objects/damage.c`, ported as they are, in the same
//! single-precision arithmetic.
//!
//! A damage effect hits an object: its damage is drawn from the effect's
//! range (from the game's random numbers), scaled by the attacker's AI, the
//! gametype or the difficulty; it reaches the object, the objects it rides
//! in (up the parents) and its umbrella shield, and a vehicle passes it on
//! to its riders. Each damaged object's collision model resists it: the
//! material hit (or the indirect one), the shield (its leak, its failure
//! curve, over-charging, depletion and stun), the body (friendly
//! resistance, headshots, regions, death and destruction).
//!
//! What damage does to the rest of the game goes through a [`DamageHost`]
//! at the moments the C does it ([`call`]'s kinds): the network's
//! authority, the AI's adjustment, the gametype's multipliers and friendly
//! fire, teams and difficulty, players' screen effects, depleting and
//! destroying bodies, shields and regions, damage effects, and the
//! aftermath (acceleration, statistics, the unit's reaction).

use crate::{CacheFile, Result, layout};

/// `struct damage_data`, laid out as the game's (84 bytes on the 32-bit
/// game).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct DamageData {
    pub definition_index: u32,
    pub flags: u32,
    pub owner_player_index: i32,
    pub owner_object_index: i32,
    pub owner_team_index: i16,
    pub pad0: i16,
    pub location_leaf: i32,
    pub location_cluster: i16,
    pub location_bonus: u16,
    pub origin: [f32; 3],
    pub epicenter: [f32; 3],
    pub direction: [f32; 3],
    pub scale: f32,
    pub multiplier: f32,
    pub material_effect_scale: f32,
    pub material_type: i16,
    pub pad1: i16,
    /// `material_response` (a pointer in the 32-bit game).
    pub material_response: u32,
}

const _: () = assert!(std::mem::size_of::<DamageData>() == 84);

/// An object's damage state, what damage reads and changes of it (the
/// game's fields, laid out for the game's side, `halo_sim.c`).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ObjectDamageState {
    pub object_index: u32,
    pub definition_index: u32,
    pub object_type: i16,
    pub owner_team_index: i16,
    pub maximum_body_vitality: f32,
    pub maximum_shield_vitality: f32,
    pub body_vitality: f32,
    pub shield_vitality: f32,
    pub current_shield_damage: f32,
    pub current_body_damage: f32,
    pub recent_shield_damage: f32,
    pub recent_body_damage: f32,
    pub shield_damage_decay_timer: i32,
    pub body_damage_decay_timer: i32,
    pub umbrella_shield_object_index: i32,
    pub next_object_index: i32,
    pub first_child_object_index: i32,
    pub parent_object_index: i32,
    pub shield_stun_ticks: i16,
    pub damage_flags: u16,
    pub regions_destroyed_flags: u16,
    pub pad: u16,
    pub region_damage: [u8; 8],
    /// A unit's: 1 if a unit, its player (NONE for none) and driver.
    pub unit: u32,
    pub player_index: i32,
    pub driver_object_index: i32,
}

const _: () = assert!(std::mem::size_of::<ObjectDamageState>() == 96);

/// A damage effect's damage (`damage_definition`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DamageDefinition {
    pub side_effect: i16,
    pub category: i16,
    pub flags: u32,
    pub damage_minimum: f32,
    pub damage_lower_bound: f32,
    pub damage_upper_bound: f32,
    pub vehicle_passthrough_penalty: f32,
    pub material_modifiers: Vec<f32>,
}

impl DamageDefinition {
    pub fn load(cache: &CacheFile, datum: u32) -> Result<Self> {
        use layout::damage_definition as d;
        let root = cache.tag_root(datum, crate::groups::DAMAGE_EFFECT)?.add(layout::damage_effect_definition::DAMAGE);
        let mut material_modifiers = Vec::new();
        for index in 0..layout::material_types::COUNT {
            material_modifiers.push(root.f32(d::MATERIAL_MODIFIERS + index * 4)?);
        }
        Ok(DamageDefinition {
            side_effect: root.i16(d::SIDE_EFFECT)?,
            category: root.i16(d::CATEGORY)?,
            flags: root.u32(d::FLAGS)?,
            damage_minimum: root.f32(d::DAMAGE_MINIMUM)?,
            damage_lower_bound: root.f32(d::DAMAGE_LOWER_BOUND)?,
            damage_upper_bound: root.f32(d::DAMAGE_UPPER_BOUND)?,
            vehicle_passthrough_penalty: root.f32(d::VEHICLE_PASSTHROUGH_PENALTY)?,
            material_modifiers,
        })
    }
}

/// A damage resistance's material (`damage_resistance_material`).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ResistanceMaterial {
    pub flags: u32,
    pub material_type: i16,
    pub shield_leak_fraction: f32,
    pub shield_damage_multiplier: f32,
    pub body_damage_multiplier: f32,
}

/// A damage region: its flags and the damage that destroys it.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct DamageRegion {
    pub flags: u32,
    pub damage_threshold: f32,
}

/// A collision model's damage resistance, and its nodes' body parts.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DamageResistance {
    pub flags: u32,
    pub indirect_damage_material_index: i16,
    pub friendly_damage_resistance: f32,
    pub has_localized_damage_effect: bool,
    pub area_damage_effect_threshold: f32,
    pub has_area_damage_effect: bool,
    pub body_damaged_effect_threshold: f32,
    pub body_destroyed_threshold: f32,
    pub shield_material_type: i16,
    pub shield_failure_function: i16,
    pub shield_failure_threshold: f32,
    pub maximum_shield_failure: f32,
    pub minimum_shield_stun_damage: f32,
    pub shield_stun_time: f32,
    pub shield_damaged_effect_threshold: f32,
    pub materials: Vec<ResistanceMaterial>,
    pub regions: Vec<DamageRegion>,
    /// Each node's `runtime_damage_part`.
    pub node_damage_parts: Vec<i16>,
}

// `damage_region` (private to damage.c, its offsets asserted there)
const DAMAGE_REGION_SIZE: usize = 0x54;
const DAMAGE_REGION_FLAGS: usize = 0x20;
const DAMAGE_REGION_THRESHOLD: usize = 0x28;

impl DamageResistance {
    /// The resistance of a collision model (a `coll` tag).
    pub fn load(cache: &CacheFile, datum: u32) -> Result<Self> {
        use layout::{collision_node as n, damage_resistance as r, damage_resistance_material as m};
        let model = cache.tag_root(datum, crate::groups::COLLISION_MODEL)?;
        let root = model.add(layout::collision_model::RESISTANCE);
        let mut materials = Vec::new();
        for material in root.block(r::MATERIALS, m::SIZEOF)?.iter() {
            materials.push(ResistanceMaterial {
                flags: material.u32(m::FLAGS)?,
                material_type: material.i16(m::MATERIAL_TYPE)?,
                shield_leak_fraction: material.f32(m::SHIELD_LEAK_FRACTION)?,
                shield_damage_multiplier: material.f32(m::SHIELD_DAMAGE_MULTIPLIER)?,
                body_damage_multiplier: material.f32(m::BODY_DAMAGE_MULTIPLIER)?,
            });
        }
        let mut regions = Vec::new();
        for region in root.block(r::REGIONS, DAMAGE_REGION_SIZE)?.iter() {
            regions.push(DamageRegion { flags: region.u32(DAMAGE_REGION_FLAGS)?, damage_threshold: region.f32(DAMAGE_REGION_THRESHOLD)? });
        }
        let mut node_damage_parts = Vec::new();
        for node in model.block(layout::collision_model::NODES, n::SIZEOF)?.iter() {
            node_damage_parts.push(node.i16(n::RUNTIME_DAMAGE_PART)?);
        }
        Ok(DamageResistance {
            flags: root.u32(r::FLAGS)?,
            indirect_damage_material_index: root.i16(r::INDIRECT_DAMAGE_MATERIAL_INDEX)?,
            friendly_damage_resistance: root.f32(r::FRIENDLY_DAMAGE_RESISTANCE)?,
            has_localized_damage_effect: root.reference(r::LOCALIZED_DAMAGE_EFFECT)?.datum.is_some(),
            area_damage_effect_threshold: root.f32(r::AREA_DAMAGE_EFFECT_THRESHOLD)?,
            has_area_damage_effect: root.reference(r::AREA_DAMAGE_EFFECT)?.datum.is_some(),
            body_damaged_effect_threshold: root.f32(r::BODY_DAMAGED_EFFECT_THRESHOLD)?,
            body_destroyed_threshold: root.f32(r::BODY_DESTROYED_THRESHOLD)?,
            shield_material_type: root.i16(r::SHIELD_MATERIAL_TYPE)?,
            shield_failure_function: root.i16(r::SHIELD_FAILURE_FUNCTION)?,
            shield_failure_threshold: root.f32(r::SHIELD_FAILURE_THRESHOLD)?,
            maximum_shield_failure: root.f32(r::MAXIMUM_SHIELD_FAILURE)?,
            minimum_shield_stun_damage: root.f32(r::MINIMUM_SHIELD_STUN_DAMAGE)?,
            shield_stun_time: root.f32(r::SHIELD_STUN_TIME)?,
            shield_damaged_effect_threshold: root.f32(r::SHIELD_DAMAGED_EFFECT_THRESHOLD)?,
            materials,
            regions,
            node_damage_parts,
        })
    }
}

/// An object definition's collision model (its `coll` tag), if any.
pub fn object_collision_model(cache: &CacheFile, definition: u32) -> Result<Option<u32>> {
    let root = cache.tag_root(definition, crate::groups::OBJECT)?;
    Ok(root.reference(layout::_object_definition::COLLISION_MODEL)?.datum)
}

/// A unit definition's share of damage its riders take.
pub fn unit_child_damage_fraction(cache: &CacheFile, definition: u32) -> Result<f32> {
    let root = cache.tag_root(definition, crate::groups::OBJECT)?.add(layout::unit_definition::UNIT);
    root.f32(layout::_unit_definition::CHILD_DAMAGE_FRACTION)
}

/// What damage asks of the game (`DamageCall::kind`).
pub mod call {
    /// `network_damage_deals` for a rider's damage: 1 if it is dealt here.
    pub const NETWORK_DEALS: i32 = 1;
    /// `ai_adjust_damage` of the attacker's actor: `f[0]` the damage, the
    /// adjusted damage out.
    pub const AI_ADJUST: i32 = 2;
    /// `game_engine_get_damage_multiplier` between the attacker's and the
    /// victim's players: out.
    pub const GAMETYPE_MULTIPLIER: i32 = 3;
    /// `game_team_is_enemy(a, b)`: 1 if so.
    pub const TEAM_IS_ENEMY: i32 = 4;
    /// `game_difficulty_get_value` of the enemies' damage: out.
    pub const DIFFICULTY_ENEMY_DAMAGE: i32 = 5;
    /// `game_difficulty_get_team_value(a, b)`: out.
    pub const DIFFICULTY_TEAM_VALUE: i32 = 6;
    /// `unit_unsuspecting` of the object, from the epicenter: 1 if so.
    pub const UNSUSPECTING: i32 = 7;
    /// `game_engine_friendly_damage` to the object (`a` explosive): the
    /// friendly fire (all 0, none 1, shields 2).
    pub const FRIENDLY_DAMAGE: i32 = 8;
    /// `transition_function_evaluate(a, f[0])`: out.
    pub const TRANSITION: i32 = 9;
    /// A unit's player's screen effect of `f[0]` damage.
    pub const PLAYER_EFFECT: i32 = 10;
    pub const DEPLETE_BODY: i32 = 11;
    pub const DEPLETE_SHIELD: i32 = 12;
    /// `object_destroy` (it is deleted).
    pub const DESTROY: i32 = 13;
    /// `object_destroy_region(a)`.
    pub const DESTROY_REGION: i32 = 14;
    pub const SHIELD_DAMAGED_EFFECT: i32 = 15;
    pub const BODY_DAMAGED_EFFECT: i32 = 16;
    /// The localized damage effect at the node `a` (`b` 1 with the hit's
    /// normal).
    pub const LOCALIZED_EFFECT: i32 = 17;
    pub const AREA_EFFECT: i32 = 18;
    /// The aftermath: `a` the being-damaged flags, `b` the body part,
    /// `f` the shield damage, body damage and multiplier (and the network's,
    /// and deleting a destroyed body).
    pub const AFTERMATH: i32 = 19;
    /// The deathless cheat: 1 if a player controls the unit.
    pub const PLAYER_CONTROLLED: i32 = 20;
}

/// One call of the game.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct DamageCall {
    pub kind: i32,
    pub object_index: i32,
    pub a: i32,
    pub b: i32,
    pub f: [f32; 3],
    pub out: f32,
}

/// A running game, as damage asks it. The damage data and the objects'
/// states are damage's as they stand (the host writes them to the game
/// first, and reads back what the call changed).
pub trait DamageHost {
    fn random_seed(&self) -> u32;
    fn set_random_seed(&self, seed: u32);
    /// An object's state, as damage first reads it (`None` if it is gone).
    fn object(&self, index: u32) -> Option<ObjectDamageState>;
    /// The tags it reads.
    fn definition(&self, damage_definition: u32) -> Option<std::sync::Arc<DamageDefinition>>;
    fn resistance(&self, object_definition: u32) -> Option<std::sync::Arc<DamageResistance>>;
    fn child_damage_fraction(&self, object_definition: u32) -> f32;
    /// A call: the objects damage has read so far are written to the game
    /// first, and read back after.
    fn call(&self, damage: &mut DamageData, objects: &mut [ObjectDamageState], call: &mut DamageCall) -> i32;
}

/// What damage reads besides the objects.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct DamageInputs {
    pub game_engine_running: bool,
    pub distributed_damage_authorized: bool,
    pub cheat_omnipotent: bool,
    pub cheat_deathless_player: bool,
}

// damage data flags
const AREA_OF_EFFECT: u32 = 1 << 0;
const CREATE_LOCALIZED_EFFECT: u32 = 1 << 1;
const KILL_INSTANTLY: u32 = 1 << 2;
const BYPASSES_SHIELDS: u32 = 1 << 5;
// damage definition flags
const CAN_CAUSE_HEADSHOTS: u32 = 1 << 1;
const DETONATES_EXPLOSIVES: u32 = 1 << 5;
const ONLY_HURTS_SHIELDS: u32 = 1 << 6;
const SKIPS_SHIELDS: u32 = 1 << 9;
const CAN_CAUSE_MULTIPLAYER_HEADSHOTS: u32 = 1 << 11;
// damage resistance flags
const TAKES_SHIELD_DAMAGE_FOR_CHILDREN: u32 = 1 << 0;
const TAKES_BODY_DAMAGE_FOR_CHILDREN: u32 = 1 << 1;
const ALWAYS_SHIELDS_FRIENDLY_DAMAGE: u32 = 1 << 2;
const PARENT_NEVER_TAKES_BODY_DAMAGE_FOR_US: u32 = 1 << 4;
const ONLY_HURT_BY_EXPLOSIVES: u32 = 1 << 5;
const ONLY_HURT_WHILE_OCCUPIED: u32 = 1 << 6;
// object damage flags
const PASSED_BODY_DAMAGE_THRESHOLD: u16 = 1 << 0;
const PASSED_SHIELD_DAMAGE_THRESHOLD: u16 = 1 << 1;
const DEAD: u16 = 1 << 2;
const SHIELD_DEPLETED: u16 = 1 << 3;
const SHIELD_OVER_CHARGING: u16 = 1 << 4;
const CANNOT_TAKE_DAMAGE: u16 = 1 << 11;
// being damaged flags
const BEING_BODY_DEPLETED: u32 = 1 << 0;
const BEING_REGION_DESTROYED: u32 = 1 << 1;
const BEING_BODY_DESTROYED: u32 = 1 << 2;
const BEING_SHIELD_DEPLETED: u32 = 1 << 3;
const BEING_BY_FRIENDLY: u32 = 1 << 4;
const BEING_MULTIPLIED_BY_DIFFICULTY: u32 = 1 << 5;
const BEING_KILLED_INSTANTLY: u32 = 1 << 6;
const BEING_FORCE_HARD_PING: u32 = 1 << 7;
// side effects, categories
const SIDE_EFFECT_LETHAL_TO_THE_UNSUSPECTING: i16 = 2;
const SIDE_EFFECT_EMP: i16 = 3;
const CATEGORY_FALLING: i16 = 1;
const CATEGORY_GRENADE: i16 = 3;
const CATEGORY_HIGH_EXPLOSIVE: i16 = 4;
const CATEGORY_FLAME: i16 = 7;
const MATERIAL_HEAD: u32 = 1 << 0;
const REGION_DIES_WHEN_OBJECT_DIES: u32 = 1 << 2;
const FRIENDLY_DAMAGE_ALL: i32 = 0;
const FRIENDLY_DAMAGE_NONE: i32 = 1;
const TEAM_PLAYER: i32 = 1;
const OBJECT_TYPE_BIPED: i16 = 0;
const OBJECT_TYPE_VEHICLE: i16 = 1;
const MAXIMUM_REGIONS: usize = 8;
const MAXIMUM_DAMAGED_OBJECTS: usize = 16;
const NONE: i32 = -1;
const EPSILON: f32 = 0.0001;

fn pin(n: f32, a: f32, b: f32) -> f32 {
    if n < a {
        a
    } else if n > b {
        b
    } else {
        n
    }
}

/// The objects damage has read, as it stands.
struct Objects<'h> {
    host: &'h dyn DamageHost,
    states: Vec<ObjectDamageState>,
}

impl Objects<'_> {
    /// An object's slot (read the first time).
    fn slot(&mut self, index: u32) -> Option<usize> {
        if let Some(slot) = self.states.iter().position(|s| s.object_index == index) {
            return Some(slot);
        }
        let state = self.host.object(index)?;
        self.states.push(state);
        Some(self.states.len() - 1)
    }

    fn get(&mut self, index: u32) -> Option<ObjectDamageState> {
        self.slot(index).map(|slot| self.states[slot])
    }

    fn call(&mut self, damage: &mut DamageData, kind: i32, object_index: i32, a: i32, b: i32, f: [f32; 3]) -> (i32, f32) {
        let mut call = DamageCall { kind, object_index, a, b, f, out: 0.0 };
        let result = self.host.call(damage, &mut self.states, &mut call);
        (result, call.out)
    }
}

/// `object_cause_damage`. The network's authority over the outermost call
/// is the caller's (`nested` false); a vehicle's riders' damage asks it.
#[allow(clippy::too_many_arguments)]
pub fn object_cause_damage(
    inputs: &DamageInputs,
    host: &dyn DamageHost,
    damage: &mut DamageData,
    object_index: u32,
    node_index: i16,
    region_index: i16,
    material_index: i16,
    has_normal: bool,
) {
    let mut objects = Objects { host, states: Vec::new() };
    cause_damage(inputs, &mut objects, damage, object_index, node_index, region_index, material_index, has_normal, false);
}

#[allow(clippy::too_many_arguments)]
fn cause_damage(
    inputs: &DamageInputs,
    objects: &mut Objects<'_>,
    damage: &mut DamageData,
    object_index: u32,
    node_index: i16,
    region_index: i16,
    material_index: i16,
    has_normal: bool,
    nested: bool,
) {
    let host = objects.host;
    if nested && objects.call(damage, call::NETWORK_DEALS, object_index as i32, 0, 0, [0.0; 3]).0 == 0 {
        return;
    }
    let Some(definition) = host.definition(damage.definition_index) else { return };
    let mut damage_was_modified = false;
    let mut parent_takes_body_damage = true;

    // (real_random_range of the effect's damage)
    let mut seed = host.random_seed();
    seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
    let random = (seed >> 16) as f32 / 65535.0;
    host.set_random_seed(seed);
    let random_damage = definition.damage_lower_bound + (definition.damage_upper_bound - definition.damage_lower_bound) * random;
    let mut total_damage =
        ((1.0 - damage.scale) * definition.damage_minimum + random_damage * damage.scale) * damage.multiplier;

    if damage.owner_object_index != NONE {
        total_damage = objects.call(damage, call::AI_ADJUST, object_index as i32, 0, 0, [total_damage, 0.0, 0.0]).1;
    }

    if inputs.game_engine_running {
        total_damage *= objects.call(damage, call::GAMETYPE_MULTIPLIER, object_index as i32, 0, 0, [0.0; 3]).1;
    } else if damage.owner_team_index != NONE as i16
        && objects.call(damage, call::TEAM_IS_ENEMY, NONE, damage.owner_team_index as i32, TEAM_PLAYER, [0.0; 3]).0 != 0
    {
        total_damage *= objects.call(damage, call::DIFFICULTY_ENEMY_DAMAGE, NONE, 0, 0, [0.0; 3]).1;
        damage_was_modified = true;
    }

    let mut damaged: Vec<u32> = Vec::new();
    let mut material_effect_recorded = false;
    if damage.flags & (AREA_OF_EFFECT | KILL_INSTANTLY) != 0 {
        damaged.push(object_index);
    } else {
        let mut index = object_index as i32;
        while index != NONE && damaged.len() < MAXIMUM_DAMAGED_OBJECTS {
            damaged.push(index as u32);
            index = objects.get(index as u32).map_or(NONE, |o| o.parent_object_index);
        }
    }

    let Some(object) = objects.get(object_index) else { return };
    if let Some(resistance) = host.resistance(object.definition_index) {
        parent_takes_body_damage = resistance.flags & PARENT_NEVER_TAKES_BODY_DAMAGE_FOR_US == 0;
    }
    if object.umbrella_shield_object_index != NONE && damaged.len() < MAXIMUM_DAMAGED_OBJECTS {
        damaged.push(object.umbrella_shield_object_index as u32);
    }
    if damage.flags & AREA_OF_EFFECT == 0 && object.object_type == OBJECT_TYPE_VEHICLE {
        damage.multiplier = (1.0 - definition.vehicle_passthrough_penalty) * host.child_damage_fraction(object.definition_index);
        let mut child_index = objects.get(object_index).map_or(NONE, |o| o.first_child_object_index);
        while child_index != NONE {
            let Some(child) = objects.get(child_index as u32) else { break };
            if child.object_type == OBJECT_TYPE_BIPED {
                let vehicle = objects.get(object_index).unwrap_or(object);
                if child.player_index != NONE || child_index == vehicle.driver_object_index {
                    if child.player_index == NONE {
                        damage.flags |= BYPASSES_SHIELDS;
                    } else {
                        damage.flags &= !BYPASSES_SHIELDS;
                    }
                    cause_damage(inputs, objects, damage, child_index as u32, NONE as i16, NONE as i16, NONE as i16, false, true);
                    damage.flags &= !BYPASSES_SHIELDS;
                }
            }
            child_index = objects.get(child_index as u32).map_or(NONE, |c| c.next_object_index);
        }
        damage.multiplier = 1.0;
    }

    for &index in &damaged {
        if objects.get(index).is_some_and(|o| o.unit != 0) {
            objects.call(damage, call::PLAYER_EFFECT, index as i32, 0, 0, [total_damage, 0.0, 0.0]);
        }
    }

    if total_damage > 0.0 {
        let mut damaged_object_count = damaged.len() as i32;
        loop {
            damaged_object_count -= 1;
            if damaged_object_count < 0 {
                return;
            }
            let current_index = damaged[damaged_object_count as usize];
            let Some(current) = objects.get(current_index) else { return };
            let mut shield_damage = 0.0f32;
            let mut body_damage = 0.0f32;
            let mut body_damage_multiplier = 0.0f32;
            let mut being_damaged_flags = 0u32;
            let mut body_part: i16 = NONE as i16;
            let mut friendly_damage = FRIENDLY_DAMAGE_ALL;

            if let Some(resistance) = host.resistance(current.definition_index) {
                let mut force_kill = damage.flags & KILL_INSTANTLY != 0;
                if node_index >= 0 && (node_index as usize) < resistance.node_damage_parts.len() {
                    body_part = resistance.node_damage_parts[node_index as usize];
                }
                if damage_was_modified {
                    being_damaged_flags |= BEING_MULTIPLIED_BY_DIFFICULTY;
                }
                if damage.owner_team_index != NONE as i16
                    && objects
                        .call(damage, call::TEAM_IS_ENEMY, NONE, current.owner_team_index as i32, damage.owner_team_index as i32, [0.0; 3])
                        .0
                        == 0
                {
                    being_damaged_flags |= BEING_BY_FRIENDLY;
                    let explosive = damage.flags & AREA_OF_EFFECT != 0
                        || definition.category == CATEGORY_GRENADE
                        || definition.category == CATEGORY_HIGH_EXPLOSIVE;
                    friendly_damage = objects.call(damage, call::FRIENDLY_DAMAGE, current_index as i32, explosive as i32, 0, [0.0; 3]).0;
                }

                let default_material = ResistanceMaterial::default();
                let material = if damaged_object_count == 0 && material_index >= 0 && (material_index as usize) < resistance.materials.len() {
                    resistance.materials[material_index as usize]
                } else if resistance.indirect_damage_material_index >= 0
                    && (resistance.indirect_damage_material_index as usize) < resistance.materials.len()
                {
                    resistance.materials[resistance.indirect_damage_material_index as usize]
                } else {
                    default_material
                };
                damage.material_type = material.material_type;

                if inputs.cheat_omnipotent && damage.owner_player_index != NONE {
                    force_kill = true;
                }
                if definition.side_effect == SIDE_EFFECT_LETHAL_TO_THE_UNSUSPECTING
                    && objects.call(damage, call::UNSUSPECTING, current_index as i32, 0, 0, [0.0; 3]).0 != 0
                    && objects.get(current_index).is_some_and(|o| o.damage_flags & CANNOT_TAKE_DAMAGE == 0)
                {
                    force_kill = true;
                }
                if force_kill
                    && (friendly_damage == FRIENDLY_DAMAGE_ALL || inputs.distributed_damage_authorized)
                    && objects.get(current_index).is_some_and(|o| o.damage_flags & DEAD == 0)
                {
                    if let Some(slot) = objects.slot(current_index) {
                        objects.states[slot].body_vitality = 0.0;
                    }
                    objects.call(damage, call::DEPLETE_BODY, current_index as i32, 0, 0, [0.0; 3]);
                    being_damaged_flags |= BEING_BODY_DEPLETED | BEING_KILLED_INSTANTLY;
                }

                let current = objects.get(current_index).unwrap_or(current);
                if friendly_damage != FRIENDLY_DAMAGE_NONE
                    && damage.flags & BYPASSES_SHIELDS == 0
                    && definition.flags & SKIPS_SHIELDS == 0
                    && current.maximum_shield_vitality > 0.0
                    && (damaged_object_count == 0 || resistance.flags & TAKES_SHIELD_DAMAGE_FOR_CHILDREN != 0)
                {
                    damage_shield(
                        inputs,
                        objects,
                        current_index,
                        &resistance,
                        &material,
                        &definition,
                        damage,
                        &mut being_damaged_flags,
                        &mut shield_damage,
                        &mut total_damage,
                    );
                }

                if friendly_damage == FRIENDLY_DAMAGE_ALL
                    && (damaged_object_count == 0 || (parent_takes_body_damage && resistance.flags & TAKES_BODY_DAMAGE_FOR_CHILDREN != 0))
                    && definition.flags & ONLY_HURTS_SHIELDS == 0
                {
                    if resistance.flags & ONLY_HURT_BY_EXPLOSIVES != 0 && definition.flags & DETONATES_EXPLOSIVES == 0 {
                        total_damage = 0.0;
                    }
                    let first = damaged_object_count == 0;
                    damage_body(
                        inputs,
                        objects,
                        current_index,
                        if first { region_index } else { NONE as i16 },
                        if first { node_index } else { NONE as i16 },
                        first && has_normal,
                        &resistance,
                        &material,
                        &definition,
                        damage,
                        &mut being_damaged_flags,
                        &mut body_damage,
                        &mut body_damage_multiplier,
                        total_damage,
                    );
                    damaged_object_count = 0;
                }

                if !material_effect_recorded && (shield_damage > EPSILON || body_damage > EPSILON) {
                    let current = objects.get(current_index).unwrap_or(current);
                    if shield_damage > body_damage {
                        damage.material_type = resistance.shield_material_type;
                        damage.material_effect_scale = current.shield_vitality;
                    } else {
                        damage.material_effect_scale = pin(current.body_vitality, 0.0, 1.0);
                    }
                    material_effect_recorded = true;
                }
            }

            let network = if current_index == object_index { 1 } else { 0 };
            objects.call(
                damage,
                call::AFTERMATH,
                current_index as i32,
                being_damaged_flags as i32,
                ((body_part as u16 as i32) << 1) | network,
                [shield_damage, body_damage, body_damage_multiplier],
            );
            if total_damage <= 0.0 {
                return;
            }
        }
    }
}

/// `object_get_maximum_shield_vitality` (`which` 2) or body (1).
fn maximum_vitality(objects: &mut Objects<'_>, damage: &mut DamageData, object: &ObjectDamageState, which: i32, ignore_difficulty: bool) -> f32 {
    let mut result = if which == 2 { object.maximum_shield_vitality } else { object.maximum_body_vitality };
    if !ignore_difficulty {
        result *= objects.call(damage, call::DIFFICULTY_TEAM_VALUE, NONE, which, object.owner_team_index as i32, [0.0; 3]).1;
    }
    result
}

/// `object_damage_shield`.
#[allow(clippy::too_many_arguments)]
fn damage_shield(
    inputs: &DamageInputs,
    objects: &mut Objects<'_>,
    object_index: u32,
    resistance: &DamageResistance,
    material: &ResistanceMaterial,
    definition: &DamageDefinition,
    damage: &mut DamageData,
    being_damaged_flags: &mut u32,
    shield_damage_reference: &mut f32,
    total_damage_reference: &mut f32,
) {
    let Some(slot) = objects.slot(object_index) else { return };
    let mut total_damage = *total_damage_reference;
    let mut negligible_damage = false;
    let mut shield_damage = total_damage;
    let object = objects.states[slot];
    let ignore_difficulty =
        !inputs.game_engine_running && definition.category == CATEGORY_FALLING && object.owner_team_index as i32 == TEAM_PLAYER;

    if object.shield_vitality > 0.0 {
        let maximum_shield_vitality = maximum_vitality(objects, damage, &object, 2, ignore_difficulty);
        let inverse_maximum_shield_vitality = if maximum_shield_vitality > 0.0 { 1.0 / maximum_shield_vitality } else { 0.0 };

        if *being_damaged_flags & BEING_BY_FRIENDLY == 0 || resistance.flags & ALWAYS_SHIELDS_FRIENDLY_DAMAGE == 0 {
            shield_damage = (1.0 - material.shield_leak_fraction) * total_damage;
            let shield_vitality = objects.states[slot].shield_vitality;
            if shield_vitality <= resistance.shield_failure_threshold && resistance.shield_failure_threshold > 0.0 {
                let shield_failure = objects
                    .call(
                        damage,
                        call::TRANSITION,
                        NONE,
                        resistance.shield_failure_function as i32,
                        0,
                        [shield_vitality / resistance.shield_failure_threshold, 0.0, 0.0],
                    )
                    .1;
                shield_damage *= (1.0 - resistance.maximum_shield_failure) * shield_failure + resistance.maximum_shield_failure;
            }
        }

        let slot = objects.slot(object_index).unwrap_or(slot);
        if objects.states[slot].damage_flags & SHIELD_OVER_CHARGING != 0 {
            shield_damage = total_damage;
            total_damage = 0.0;
        } else {
            if shield_damage < 0.0 {
                shield_damage = 0.0;
            }
            total_damage -= shield_damage;
            if *being_damaged_flags & BEING_BY_FRIENDLY != 0 && *being_damaged_flags & BEING_MULTIPLIED_BY_DIFFICULTY != 0 {
                let difficulty = objects.call(damage, call::DIFFICULTY_ENEMY_DAMAGE, NONE, 0, 0, [0.0; 3]).1;
                if difficulty > 0.0 {
                    shield_damage /= difficulty;
                }
            }
            let mut actual_shield_damage = material.shield_damage_multiplier * shield_damage;
            if resistance.shield_material_type >= 0 && (resistance.shield_material_type as usize) < definition.material_modifiers.len() {
                actual_shield_damage *= definition.material_modifiers[resistance.shield_material_type as usize];
            }
            if actual_shield_damage < EPSILON {
                negligible_damage = true;
            }
            let normalized_shield_damage = actual_shield_damage * inverse_maximum_shield_vitality;
            let slot = objects.slot(object_index).unwrap_or(slot);
            let shield_vitality = objects.states[slot].shield_vitality;
            if normalized_shield_damage > shield_vitality || definition.side_effect == SIDE_EFFECT_EMP {
                let excess_damage = actual_shield_damage - maximum_shield_vitality * shield_vitality;
                if excess_damage > 0.0 {
                    total_damage += excess_damage;
                }
                objects.states[slot].shield_vitality = 0.0;
                if objects.states[slot].damage_flags & SHIELD_DEPLETED == 0 {
                    objects.call(damage, call::DEPLETE_SHIELD, object_index as i32, 0, 0, [0.0; 3]);
                    *being_damaged_flags |= BEING_SHIELD_DEPLETED;
                }
            } else {
                if objects.states[slot].damage_flags & CANNOT_TAKE_DAMAGE == 0 {
                    objects.states[slot].shield_vitality -= normalized_shield_damage;
                }
                if objects.states[slot].damage_flags & PASSED_SHIELD_DAMAGE_THRESHOLD == 0
                    && objects.states[slot].shield_vitality < resistance.shield_damaged_effect_threshold
                {
                    objects.call(damage, call::SHIELD_DAMAGED_EFFECT, object_index as i32, 0, 0, [0.0; 3]);
                    let slot = objects.slot(object_index).unwrap_or(slot);
                    objects.states[slot].damage_flags |= PASSED_SHIELD_DAMAGE_THRESHOLD;
                }
            }
        }

        if !negligible_damage {
            let normalized_damage = (*total_damage_reference - total_damage) * inverse_maximum_shield_vitality;
            let slot = objects.slot(object_index).unwrap_or(slot);
            let o = &mut objects.states[slot];
            o.shield_damage_decay_timer = 0;
            if o.damage_flags & SHIELD_DEPLETED == 0 {
                o.current_shield_damage = 1.0;
            }
            o.recent_shield_damage += normalized_damage;
            if o.current_shield_damage > 1.0 {
                o.current_shield_damage = 1.0;
            }
            if o.recent_shield_damage > 1.0 {
                o.recent_shield_damage = 1.0;
            }
        }
    } else {
        shield_damage = 0.0;
        objects.states[slot].shield_vitality = 0.0;
    }

    let slot = objects.slot(object_index).unwrap_or(slot);
    if shield_damage >= resistance.minimum_shield_stun_damage || objects.states[slot].shield_vitality == 0.0 {
        objects.states[slot].shield_stun_ticks = (resistance.shield_stun_time * 30.0) as i32 as i16;
    }
    *shield_damage_reference = shield_damage;
    *total_damage_reference = total_damage;
}

/// `object_damage_body`.
#[allow(clippy::too_many_arguments)]
fn damage_body(
    inputs: &DamageInputs,
    objects: &mut Objects<'_>,
    object_index: u32,
    region_index: i16,
    node_index: i16,
    has_normal: bool,
    resistance: &DamageResistance,
    material: &ResistanceMaterial,
    definition: &DamageDefinition,
    damage: &mut DamageData,
    being_damaged_flags: &mut u32,
    body_damage: &mut f32,
    body_damage_multiplier: &mut f32,
    total_damage: f32,
) {
    let Some(slot) = objects.slot(object_index) else { return };
    let object = objects.states[slot];
    let mut damage_amount = material.body_damage_multiplier * total_damage;
    if resistance.flags & ONLY_HURT_WHILE_OCCUPIED != 0 && object.object_type == OBJECT_TYPE_VEHICLE && object.driver_object_index == NONE {
        damage_amount = 0.0;
    }
    let ignore_difficulty =
        !inputs.game_engine_running && definition.category == CATEGORY_FALLING && object.owner_team_index as i32 == TEAM_PLAYER;
    let maximum_body_vitality = maximum_vitality(objects, damage, &object, 1, ignore_difficulty);
    let inverse_maximum_body_vitality = if maximum_body_vitality > 0.0 { 1.0 / maximum_body_vitality } else { 0.0 };

    let mut actual_damage = damage_amount;
    if *being_damaged_flags & BEING_BY_FRIENDLY != 0 {
        actual_damage = (1.0 - resistance.friendly_damage_resistance) * damage_amount;
        if *being_damaged_flags & BEING_MULTIPLIED_BY_DIFFICULTY != 0 {
            let difficulty = objects.call(damage, call::DIFFICULTY_ENEMY_DAMAGE, NONE, 0, 0, [0.0; 3]).1;
            if difficulty > 0.0 {
                actual_damage /= difficulty;
            }
        }
    }
    actual_damage *= inverse_maximum_body_vitality;
    if material.material_type >= 0 && (material.material_type as usize) < definition.material_modifiers.len() {
        actual_damage *= definition.material_modifiers[material.material_type as usize];
    }

    let slot = objects.slot(object_index).unwrap_or(slot);
    if objects.states[slot].damage_flags & CANNOT_TAKE_DAMAGE == 0 {
        if damage_amount > 0.0 && material.flags & MATERIAL_HEAD != 0 {
            if definition.flags & CAN_CAUSE_HEADSHOTS != 0 {
                let o = objects.states[slot];
                if inputs.game_engine_running || o.object_type != OBJECT_TYPE_BIPED || o.player_index == NONE {
                    objects.states[slot].body_vitality = 0.0;
                    *being_damaged_flags |= BEING_KILLED_INSTANTLY;
                    if inputs.game_engine_running {
                        *being_damaged_flags |= BEING_FORCE_HARD_PING;
                    }
                }
            } else if definition.flags & CAN_CAUSE_MULTIPLAYER_HEADSHOTS != 0 && inputs.game_engine_running {
                actual_damage *= 2.0;
                if actual_damage > objects.states[slot].body_vitality {
                    *being_damaged_flags |= BEING_FORCE_HARD_PING;
                }
            }
        }
        objects.states[slot].body_vitality -= actual_damage;
    }

    let region_count = resistance.regions.len().min(MAXIMUM_REGIONS);
    let region_index = if region_index != NONE as i16 && !(region_index >= 0 && (region_index as usize) < region_count) {
        NONE as i16
    } else {
        region_index
    };
    if region_index != NONE as i16 && objects.states[slot].regions_destroyed_flags & (1 << region_index) == 0 {
        let region = resistance.regions[region_index as usize];
        let r = region_index as usize;
        objects.states[slot].region_damage[r] = (actual_damage * 255.0 + objects.states[slot].region_damage[r] as f32) as i32 as u8;
        if region.damage_threshold > 0.0 && objects.states[slot].region_damage[r] as f32 * (1.0 / 255.0) > region.damage_threshold {
            objects.call(damage, call::DESTROY_REGION, object_index as i32, region_index as i32, 0, [0.0; 3]);
            *being_damaged_flags |= BEING_REGION_DESTROYED;
        }
    }

    let slot = objects.slot(object_index).unwrap_or(slot);
    {
        let o = &mut objects.states[slot];
        o.body_damage_decay_timer = 0;
        o.current_body_damage += actual_damage;
        o.recent_body_damage += actual_damage;
        if o.current_body_damage > 1.0 {
            o.current_body_damage = 1.0;
        }
        if o.recent_body_damage > 1.0 {
            o.recent_body_damage = 1.0;
        }
    }

    if inputs.cheat_deathless_player
        && objects.states[slot].body_vitality < 0.0
        && objects.states[slot].unit != 0
        && objects.call(damage, call::PLAYER_CONTROLLED, object_index as i32, 0, 0, [0.0; 3]).0 != 0
    {
        let slot = objects.slot(object_index).unwrap_or(slot);
        objects.states[slot].body_vitality = 0.0;
    }

    // (object_get_actual_body_vitality, not ignoring the difficulty)
    let slot = objects.slot(object_index).unwrap_or(slot);
    let o = objects.states[slot];
    let body_vitality = maximum_vitality(objects, damage, &o, 1, false) * o.body_vitality;
    if resistance.body_destroyed_threshold < 0.0 && body_vitality < resistance.body_destroyed_threshold {
        objects.call(damage, call::DESTROY, object_index as i32, 0, 0, [0.0; 3]);
        *being_damaged_flags |= BEING_BODY_DEPLETED | BEING_BODY_DESTROYED;
    } else if body_vitality < 0.0 {
        if objects.get(object_index).is_some_and(|o| o.damage_flags & DEAD == 0) {
            for (dying_region_index, region) in resistance.regions.iter().take(MAXIMUM_REGIONS).enumerate() {
                if region.flags & REGION_DIES_WHEN_OBJECT_DIES != 0 {
                    objects.call(damage, call::DESTROY_REGION, object_index as i32, dying_region_index as i32, 0, [0.0; 3]);
                }
            }
            objects.call(damage, call::DEPLETE_BODY, object_index as i32, 0, 0, [0.0; 3]);
            *being_damaged_flags |= BEING_BODY_DEPLETED;
        }
    } else if body_vitality < resistance.body_damaged_effect_threshold
        && objects.get(object_index).is_some_and(|o| o.damage_flags & PASSED_BODY_DAMAGE_THRESHOLD == 0)
    {
        objects.call(damage, call::BODY_DAMAGED_EFFECT, object_index as i32, 0, 0, [0.0; 3]);
        if let Some(slot) = objects.slot(object_index) {
            objects.states[slot].damage_flags |= PASSED_BODY_DAMAGE_THRESHOLD;
        }
    }

    if damage.flags & CREATE_LOCALIZED_EFFECT != 0 && resistance.has_localized_damage_effect {
        objects.call(damage, call::LOCALIZED_EFFECT, object_index as i32, node_index as i32, has_normal as i32, [0.0; 3]);
    }
    if damage.flags & AREA_OF_EFFECT != 0
        && damage_amount > resistance.area_damage_effect_threshold
        && resistance.has_area_damage_effect
        && definition.category != CATEGORY_FLAME
    {
        objects.call(damage, call::AREA_EFFECT, object_index as i32, 0, 0, [0.0; 3]);
    }

    *body_damage = damage_amount;
    *body_damage_multiplier = material.body_damage_multiplier;
}
