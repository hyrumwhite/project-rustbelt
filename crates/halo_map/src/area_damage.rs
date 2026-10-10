//! Halo's area damage (ROADMAP phase 7, objects and physics): a detonation's
//! `area_of_effect_cause_damage` with `area_of_effect_cause_damage_to_object`,
//! from OpenCE's `source/objects/damage.c`, ported as they are, in the same
//! single-precision arithmetic.
//!
//! The objects within the effect's cutoff radius (`objects_in_sphere`: the
//! clusters the sphere reaches, each one's collideable and then
//! noncollideable objects, each once) are each damaged if the detonation
//! reaches them: a unit, when the effect has a core radius, if any of four
//! rays spread about the line to it gets through, another object if the line
//! itself does. The owner and friends may be spared, and infection forms
//! popped. The damage's direction and scale (by the falloff) are the
//! object's; a collision model whose children take area damage passes it to
//! them, and through them to their siblings.
//!
//! What area damage asks of the game goes through an [`AreaDamageHost`]
//! ([`call`]'s kinds): ray tests, teams, the difficulty, the damage itself
//! (`object_cause_damage`, the game's or Rust's) and breakable surfaces.

use std::sync::Arc;

use crate::damage::{DamageData, DamageDefinition, DamageResistance};

const NONE: i32 = -1;
const EPSILON: f32 = 0.0001;

/// `_damage_area_of_effect_collision_flags`: front-facing surfaces, the
/// structure, and vehicles, scenery and machines (not the objects bit: the
/// game's rays test the structure alone).
pub const COLLISION_FLAGS: u32 = (1 << 0) | (1 << 5) | (1 << 9) | (1 << 14) | (1 << 15);

/// `HALO_PORT_MAXIMUM_AREA_OF_EFFECT_OBJECTS`, the native builds' (64 in the
/// retail game).
pub const MAXIMUM_AREA_OF_EFFECT_OBJECTS: usize = 256;
/// `MAXIMUM_OBJECTS_PER_MAP` (`HALO_PORT_MAXIMUM_OBJECTS_PER_MAP`), what
/// `objects_in_clusters_by_indices` gathers at most.
pub const MAXIMUM_OBJECTS_PER_MAP: usize = 8192;
/// `MAXIMUM_CLUSTERS_PER_STRUCTURE`.
pub const MAXIMUM_CLUSTERS_PER_STRUCTURE: usize = 512;

// damage data flags
const AREA_OF_EFFECT: u32 = 1 << 0;
const DAMAGED_ONE_OBJECT: u32 = 1 << 6;
// damage definition flags
const DOES_NOT_HURT_OWNER: u32 = 1 << 0;
const DOES_NOT_HURT_FRIENDS: u32 = 1 << 3;
const DOES_NOT_HURT_INFECTION_FORMS: u32 = 1 << 10;
const INFECTION_FORM_POP: u32 = 1 << 12;
// damage effect flags
const DONT_SCALE_DAMAGE_BY_DISTANCE: u32 = 1 << 0;
// damage resistance flags
const CHILDREN_TAKE_AREA_DAMAGE: u32 = 1 << 3;
// object flags
const OBJECT_INVISIBLE: u32 = 1 << 0;
// unit definition flags
const UNIT_IS_INCONSEQUENTIAL: u32 = 1 << 19;
// object types (bipeds and vehicles are units)
const OBJECT_MASK_UNIT: u32 = (1 << 0) | (1 << 1);

/// An object, as area damage reads it (the game's fields, laid out for the
/// game's side, `halo_sim.c`).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct AreaObject {
    pub object_index: u32,
    pub definition_index: u32,
    pub object_type: i16,
    pub owner_team_index: i16,
    pub flags: u32,
    pub bounding_center: [f32; 3],
    pub bounding_radius: f32,
    pub parent_object_index: i32,
    pub first_child_object_index: i32,
    pub next_object_index: i32,
}

const _: () = assert!(std::mem::size_of::<AreaObject>() == 44);

/// What area damage asks of the game (`AreaCall::kind`).
pub mod call {
    /// `collision_test_vector` with the area flags: `a` the flags, `f` the
    /// point and vector, the object the ignored one; `result` 1 on a hit,
    /// `out` the point it reached.
    pub const RAY_TEST: i32 = 1;
    /// `game_team_is_enemy(a, b)`: 1 if so.
    pub const TEAM_IS_ENEMY: i32 = 2;
    /// `game_difficulty_get_value` of the infection forms' toughness: `out[0]`.
    pub const DIFFICULTY_INFECTION_FORMS: i32 = 3;
    /// `object_cause_damage(damage, object, NONE, NONE, NONE, NULL)`.
    pub const CAUSE_DAMAGE: i32 = 4;
    /// `breakable_surface_damage_area_of_effect(damage)`.
    pub const BREAKABLE_SURFACES: i32 = 5;
}

/// One call of the game.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct AreaCall {
    pub kind: i32,
    pub object_index: i32,
    pub a: i32,
    pub b: i32,
    pub f: [f32; 6],
    pub result: i32,
    pub out: [f32; 3],
}

/// A running game, as area damage asks it.
pub trait AreaDamageHost {
    fn random_seed(&self) -> u32;
    fn set_random_seed(&self, seed: u32);
    fn definition(&self, damage_definition: u32) -> Option<Arc<DamageDefinition>>;
    fn resistance(&self, object_definition: u32) -> Option<Arc<DamageResistance>>;
    fn unit_definition_flags(&self, object_definition: u32) -> u32;
    /// `structure_clusters_in_sphere` (at most `maximum`).
    fn clusters_in_sphere(&self, cluster: i16, center: [f32; 3], radius: f32, maximum: usize) -> Vec<i16>;
    /// A cluster's objects: its collideable partition's (`partition` 0) or
    /// its noncollideable one's (1), in the partition's order.
    fn cluster_objects(&self, cluster: i16, partition: u32, out: &mut Vec<u32>);
    /// An object as it stands (`None` if it is gone).
    fn object(&self, index: u32) -> Option<AreaObject>;
    /// A call: the damage data as it stands written to the game first, and
    /// read back after.
    fn call(&self, damage: &mut DamageData, call: &mut AreaCall) -> i32;
}

fn sub3(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn scale3(v: [f32; 3], c: f32) -> [f32; 3] {
    [c * v[0], c * v[1], c * v[2]]
}

fn cross3(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

/// `normalize3d`: the magnitude, 0 (leaving the vector) below epsilon.
fn normalize3(v: &mut [f32; 3]) -> f32 {
    let magnitude = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    if !(EPSILON > (magnitude - 0.0).abs()) {
        *v = scale3(*v, 1.0 / magnitude);
        magnitude
    } else {
        0.0
    }
}

/// `perpendicular3d`.
fn perpendicular3(a: [f32; 3]) -> [f32; 3] {
    let (i, j, k) = (a[0].abs(), a[1].abs(), a[2].abs());
    if i <= j && i <= k {
        [0.0, a[2], -a[1]]
    } else if j <= k {
        [-a[2], 0.0, a[0]]
    } else {
        [a[1], -a[0], 0.0]
    }
}

/// `distance_squared3d`.
fn distance_squared3(a: [f32; 3], b: [f32; 3]) -> f32 {
    let d = sub3(a, b);
    d[0] * d[0] + d[1] * d[1] + d[2] * d[2]
}

/// `objects_in_sphere` with no class or type: the objects whose bounding
/// spheres the sphere touches.
pub fn objects_in_sphere(
    host: &dyn AreaDamageHost,
    cluster: i16,
    center: [f32; 3],
    radius: f32,
    maximum: usize,
) -> Vec<u32> {
    let clusters = host.clusters_in_sphere(cluster, center, radius, MAXIMUM_CLUSTERS_PER_STRUCTURE);
    objects_in_clusters_in_sphere(
        0,
        0,
        &clusters,
        &mut |cluster, partition, out| host.cluster_objects(cluster, partition, out),
        &mut |index| host.object(index).map(|o| (o.object_type, o.bounding_center, o.bounding_radius)),
        center,
        radius,
        maximum,
    )
}

/// `objects_in_sphere` past its clusters: `class_flags` the partitions
/// (bit 0 collideable, 1 noncollideable; 0 for both) and `type_flags` the
/// object types (0 for all), each object once in the clusters' order (the
/// marker's, `objects_in_clusters_by_indices`), then those whose bounding
/// spheres the sphere touches. `object` gives an object's type and bounding
/// sphere (`None` if it is gone).
#[allow(clippy::too_many_arguments)]
pub fn objects_in_clusters_in_sphere(
    class_flags: u32,
    type_flags: u32,
    clusters: &[i16],
    cluster_objects: &mut dyn FnMut(i16, u32, &mut Vec<u32>),
    object: &mut dyn FnMut(u32) -> Option<(i16, [f32; 3], f32)>,
    center: [f32; 3],
    radius: f32,
    maximum: usize,
) -> Vec<u32> {
    let class_flags = if class_flags == 0 { u32::MAX } else { class_flags };
    let type_flags = if type_flags == 0 { u32::MAX } else { type_flags };
    let mut in_clusters: Vec<u32> = Vec::new();
    let mut marked = std::collections::HashSet::new();
    let mut partition = Vec::new();
    'clusters: for &cluster in clusters {
        for kind in 0..2 {
            if class_flags & (1 << kind) == 0 {
                continue;
            }
            partition.clear();
            cluster_objects(cluster, kind, &mut partition);
            for &index in &partition {
                if marked.insert(index) {
                    if in_clusters.len() >= MAXIMUM_OBJECTS_PER_MAP {
                        break 'clusters;
                    }
                    in_clusters.push(index);
                }
            }
        }
    }
    let mut result = Vec::new();
    for &index in &in_clusters {
        if result.len() >= maximum {
            break;
        }
        let Some((object_type, bounding_center, bounding_radius)) = object(index) else { continue };
        let reach = bounding_radius + radius;
        if type_flags & (1u32 << (object_type as u32 & 31)) != 0 && distance_squared3(center, bounding_center) <= reach * reach {
            result.push(index);
        }
    }
    result
}

/// `area_of_effect_cause_damage`.
pub fn area_of_effect_cause_damage(host: &dyn AreaDamageHost, damage: &mut DamageData) {
    let Some(definition) = host.definition(damage.definition_index) else { return };
    let objects =
        objects_in_sphere(host, damage.location_cluster, damage.origin, definition.cutoff_radius, MAXIMUM_AREA_OF_EFFECT_OBJECTS);
    for &object_index in &objects {
        cause_damage_to_object(host, &definition, damage, object_index, false);
    }
    host.call(damage, &mut AreaCall { kind: call::BREAKABLE_SURFACES, object_index: NONE, ..Default::default() });
}

fn ultimate_parent(host: &dyn AreaDamageHost, mut index: u32) -> u32 {
    while let Some(object) = host.object(index) {
        if object.parent_object_index == NONE {
            break;
        }
        index = object.parent_object_index as u32;
    }
    index
}

fn ray_test(host: &dyn AreaDamageHost, damage: &mut DamageData, point: [f32; 3], vector: [f32; 3], ignore: u32) -> (bool, [f32; 3]) {
    let mut call = AreaCall {
        kind: call::RAY_TEST,
        object_index: ignore as i32,
        a: COLLISION_FLAGS as i32,
        f: [point[0], point[1], point[2], vector[0], vector[1], vector[2]],
        ..Default::default()
    };
    let result = host.call(damage, &mut call);
    (result != 0, call.out)
}

/// `area_of_effect_cause_damage_to_object`.
fn cause_damage_to_object(
    host: &dyn AreaDamageHost,
    definition: &DamageDefinition,
    damage: &mut DamageData,
    object_index: u32,
    damage_next_object: bool,
) {
    let Some(object) = host.object(object_index) else { return };
    let mut can_damage = object.flags & OBJECT_INVISIBLE == 0;
    let mut did_damage = false;
    let mut infection_form = false;
    let epicenter = damage.epicenter;
    let is_unit = OBJECT_MASK_UNIT & (1 << object.object_type as u32) != 0;

    let collision_blocked = if can_damage && is_unit && definition.area_of_effect_core_radius > EPSILON {
        let mut blocked = true;
        let to_object = sub3(object.bounding_center, epicenter);
        let mut perpendicular = perpendicular3(to_object);
        normalize3(&mut perpendicular);
        let mut cross = cross3(to_object, perpendicular);
        normalize3(&mut cross);
        let core = definition.area_of_effect_core_radius;
        for ray_index in 0..4 {
            let collision_vector = match ray_index {
                0 => scale3(perpendicular, core),
                1 => scale3(perpendicular, -core),
                2 => scale3(cross, core),
                _ => scale3(cross, -core),
            };
            let (_, offset_point) = ray_test(host, damage, epicenter, collision_vector, ultimate_parent(host, object_index));
            let ultimate_parent_index = ultimate_parent(host, object_index);
            let offset_vector = sub3(object.bounding_center, offset_point);
            if !ray_test(host, damage, offset_point, offset_vector, ultimate_parent_index).0 {
                blocked = false;
            }
        }
        blocked
    } else {
        let ultimate_parent_index = ultimate_parent(host, object_index);
        let direct_vector = sub3(object.bounding_center, epicenter);
        ray_test(host, damage, epicenter, direct_vector, ultimate_parent_index).0
    };
    if collision_blocked {
        can_damage = false;
    }

    if definition.flags & DOES_NOT_HURT_OWNER != 0 && object_index as i32 == damage.owner_object_index {
        can_damage = false;
    }

    if definition.flags & DOES_NOT_HURT_FRIENDS != 0 {
        let mut call = AreaCall {
            kind: call::TEAM_IS_ENEMY,
            object_index: NONE,
            a: object.owner_team_index as i32,
            b: damage.owner_team_index as i32,
            ..Default::default()
        };
        if host.call(damage, &mut call) == 0 {
            can_damage = false;
        }
    }

    if can_damage && definition.flags & INFECTION_FORM_POP != 0 {
        can_damage = false;
        if is_unit
            && host.unit_definition_flags(object.definition_index) & UNIT_IS_INCONSEQUENTIAL != 0
            && object_index as i32 != damage.owner_object_index
        {
            let mut call = AreaCall { kind: call::DIFFICULTY_INFECTION_FORMS, object_index: NONE, ..Default::default() };
            host.call(damage, &mut call);
            let infection_form_toughness = call.out[0];

            can_damage = true;
            if (infection_form_toughness > 0.0 || definition.flags & DOES_NOT_HURT_INFECTION_FORMS != 0)
                && damage.flags & DAMAGED_ONE_OBJECT != 0
            {
                can_damage = false;
            }

            if infection_form_toughness > 0.0 {
                // (real_seed_random of the game's seed)
                let mut seed = host.random_seed();
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                host.set_random_seed(seed);
                if ((seed >> 16) as f32 / 65535.0) < infection_form_toughness * 0.25 {
                    can_damage = false;
                }
            }

            infection_form = true;
        }
    }

    damage.flags |= AREA_OF_EFFECT;
    // (what follows reads the object as the damage left it: a unit killed
    // drops its weapon; if it is gone, as it was)
    let mut after = object;
    if can_damage {
        let mut direction = sub3(object.bounding_center, epicenter);
        let distance = normalize3(&mut direction);
        damage.direction = direction;
        let radius_delta = definition.cutoff_radius - definition.falloff_radius;
        let scale = if radius_delta > 0.0 {
            let n = 1.0 - (distance - definition.falloff_radius) / radius_delta;
            // (PIN, as the C has it)
            #[allow(clippy::manual_clamp)]
            if n < 0.0 {
                0.0
            } else if n > 1.0 {
                1.0
            } else {
                n
            }
        } else {
            1.0
        };

        if definition.effect_flags & DONT_SCALE_DAMAGE_BY_DISTANCE == 0 {
            damage.scale = scale;
        }

        if scale > 0.0 {
            host.call(damage, &mut AreaCall { kind: call::CAUSE_DAMAGE, object_index: object_index as i32, ..Default::default() });
            did_damage = true;
            after = host.object(object_index).unwrap_or(object);
        }

        if host.resistance(object.definition_index).is_some_and(|r| r.flags & CHILDREN_TAKE_AREA_DAMAGE != 0)
            && after.first_child_object_index != NONE
        {
            cause_damage_to_object(host, definition, damage, after.first_child_object_index as u32, true);
            after = host.object(object_index).unwrap_or(after);
        }
    }

    if infection_form && (!can_damage || did_damage) {
        damage.flags |= DAMAGED_ONE_OBJECT;
    }

    if damage_next_object {
        let next = host.object(object_index).unwrap_or(after).next_object_index;
        if next != NONE {
            cause_damage_to_object(host, definition, damage, next as u32, true);
        }
    }
}
